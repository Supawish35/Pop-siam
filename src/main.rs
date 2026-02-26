use std::{
    collections::HashMap,
    fs,
    io::Error as IoError,
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex},
};

use chrono::Utc;
use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, stream::TryStreamExt, StreamExt};

use dotenv::dotenv;
use std::env;

use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite;

type Tx = UnboundedSender<tungstenite::protocol::Message>;
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Tx>>>;
const TOTAL_FILE_PATH: &str = "total.txt";
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8765";

#[derive(Serialize, Deserialize, Debug)]
struct AppState {
    total_clicks: u64,
    client_clicks: HashMap<SocketAddr, u64>,
}

impl AppState {
    fn new() -> Self {
        AppState {
            total_clicks: 0,
            client_clicks: HashMap::new(),
        }
    }
}

fn read_total_from_file() -> u64 {
    match fs::read_to_string(TOTAL_FILE_PATH) {
        Ok(content) => content.trim().parse::<u64>().unwrap_or(0),
        Err(_) => 0,
    }
}

fn save_total_to_file(total: u64) {
    if let Some(parent) = Path::new(TOTAL_FILE_PATH).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(err) = fs::create_dir_all(parent) {
                eprintln!("Failed to create directory for {}: {}", TOTAL_FILE_PATH, err);
                return;
            }
        }
    }

    if let Err(err) = fs::write(TOTAL_FILE_PATH, total.to_string()) {
        eprintln!("Failed to write total to {}: {}", TOTAL_FILE_PATH, err);
    }
}

fn sanitize_env_value(value: String) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_start_matches("ws://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

fn resolve_bind_addr() -> String {
    // Render sets PORT environment variable
    if let Ok(port) = env::var("PORT") {
        let port = sanitize_env_value(port);
        if !port.is_empty() {
            return format!("0.0.0.0:{}", port);
        }
    }

    if let Ok(addr) = env::var("ADDR") {
        let addr = sanitize_env_value(addr);
        if !addr.is_empty() {
            return addr;
        }
    }

    let host = env::var("HOST")
        .map(sanitize_env_value)
        .unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = env::var("PORT")
        .map(sanitize_env_value)
        .unwrap_or_else(|_| "8765".to_string());

    if host.is_empty() || port.is_empty() {
        DEFAULT_BIND_ADDR.to_string()
    } else {
        format!("{}:{}", host, port)
    }
}

async fn bind_listener_with_fallback(addr: String) -> Result<(TcpListener, String), IoError> {
    let mut bind_attempts = vec![addr];
    if !bind_attempts.iter().any(|a| a == DEFAULT_BIND_ADDR) {
        bind_attempts.push(DEFAULT_BIND_ADDR.to_string());
    }

    let mut last_err: Option<IoError> = None;

    for attempt in bind_attempts {
        match TcpListener::bind(&attempt).await {
            Ok(listener) => return Ok((listener, attempt)),
            Err(err) => {
                eprintln!("Failed to bind to '{}': {}", attempt, err);
                last_err = Some(err);
            }
        }
    }

    // Last resort: ask OS for any available port.
    match TcpListener::bind("0.0.0.0:0").await {
        Ok(listener) => {
            let actual = listener.local_addr()?.to_string();
            eprintln!("Using fallback bind address '{}'.", actual);
            Ok((listener, actual))
        }
        Err(err) => Err(last_err.unwrap_or(err)),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum WsMessage {
    #[serde(rename = "init")]
    Init { total_clicks: u64 },
    #[serde(rename = "click_response")]
    ClickResponse {
        client_clicks: u64,
        total_clicks: u64,
        timestamp: String,
    },
    #[serde(rename = "global_update")]
    GlobalUpdate { total_clicks: u64 },
    #[serde(rename = "click")]
    Click,
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "pong")]
    Pong,
}

async fn handle_connection(
    peer_map: PeerMap,
    stream: TcpStream,
    addr: SocketAddr,
    app_state: Arc<Mutex<AppState>>,
) {
    println!("Incoming TCP connection from: {}", addr);

    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(stream) => stream,
        Err(err) => {
            eprintln!("WebSocket handshake failed for {}: {}", addr, err);
            return;
        }
    };
    println!("WebSocket connection established: {}", addr);

    // Reload total from file when the first websocket client connects.
    let is_first_client = peer_map.lock().unwrap().is_empty();
    if is_first_client {
        let total = read_total_from_file();
        app_state.lock().unwrap().total_clicks = total;
    }

    // Insert the write part of this peer to the peer map.
    let (tx, rx) = unbounded();
    peer_map.lock().unwrap().insert(addr, tx);

    // Send initial state
    {
        let state = app_state.lock().unwrap();
        let init_msg = WsMessage::Init {
            total_clicks: state.total_clicks,
        };
        let json_msg = serde_json::to_string(&init_msg).unwrap();
        if let Some(sender) = peer_map.lock().unwrap().get(&addr) {
            let _ = sender.unbounded_send(tungstenite::protocol::Message::Text(json_msg.into()));
        }
    }

    let (outgoing, incoming) = ws_stream.split();

    let broadcast_incoming = incoming.try_for_each(|msg| {
        let peer_map = peer_map.clone();
        let app_state = app_state.clone();

        async move {
            if let Ok(text) = msg.to_text() {
                if let Ok(parsed_msg) = serde_json::from_str::<WsMessage>(text) {
                    match parsed_msg {
                        WsMessage::Click => {
                            let mut state = app_state.lock().unwrap();
                            state.total_clicks += 1;
                            let client_clicks =
                                state.client_clicks.entry(addr).or_insert(0);
                            *client_clicks += 1;

                            let response = WsMessage::ClickResponse {
                                client_clicks: *client_clicks,
                                total_clicks: state.total_clicks,
                                timestamp: Utc::now().to_rfc3339(),
                            };
                            let json_response = serde_json::to_string(&response).unwrap();
                            if let Some(sender) = peer_map.lock().unwrap().get(&addr) {
                                let _ = sender.unbounded_send(tungstenite::protocol::Message::Text(json_response.into()));
                            }

                            let broadcast_message = WsMessage::GlobalUpdate {
                                total_clicks: state.total_clicks,
                            };
                            let json_broadcast = serde_json::to_string(&broadcast_message).unwrap();

                            let peers = peer_map.lock().unwrap();
                            for (peer_addr, sender) in peers.iter() {
                                if *peer_addr != addr {
                                    let _ = sender.unbounded_send(tungstenite::protocol::Message::Text(json_broadcast.clone().into()));
                                }
                            }
                        }
                        WsMessage::Ping => {
                             if let Some(sender) = peer_map.lock().unwrap().get(&addr) {
                                let pong = WsMessage::Pong;
                                let json_pong = serde_json::to_string(&pong).unwrap();
                                let _ = sender.unbounded_send(tungstenite::protocol::Message::Text(json_pong.into()));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(())
        }
    });

    let receive_from_others = rx.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    println!("{} disconnected", &addr);
    peer_map.lock().unwrap().remove(&addr);
    app_state.lock().unwrap().client_clicks.remove(&addr);

    // Persist total when no websocket clients remain connected.
    let no_clients_connected = peer_map.lock().unwrap().is_empty();
    if no_clients_connected {
        let total = app_state.lock().unwrap().total_clicks;
        save_total_to_file(total);
    }
}

#[tokio::main]
async fn main() -> Result<(), IoError> {
    dotenv().ok();

    let addr = resolve_bind_addr();
    let (listener, bind_addr) = bind_listener_with_fallback(addr).await?;
    println!("Click Counter Server running at ws://{}", bind_addr);

    let state = Arc::new(Mutex::new(AppState::new()));
    let peer_map = PeerMap::new(Mutex::new(HashMap::new()));

    println!("Press Ctrl+C to shutdown the server.");
    loop {
        tokio::select! {
            Ok((stream, addr)) = listener.accept() => {
                tokio::spawn(handle_connection(
                    peer_map.clone(),
                    stream,
                    addr,
                    state.clone(),
                ));
            }
            _ = tokio::signal::ctrl_c() => {
                println!("Ctrl+C received, shutting down.");
                let total = state.lock().unwrap().total_clicks;
                save_total_to_file(total);
                break;
            }
        }
    }

    Ok(())
}
