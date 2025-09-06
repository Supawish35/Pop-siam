use std::{
    collections::HashMap,
    
    io::Error as IoError,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use chrono::Utc;
use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, stream::TryStreamExt, StreamExt};

use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};


use tokio_tungstenite::tungstenite;

type Tx = UnboundedSender<tungstenite::protocol::Message>;
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Tx>>>;

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

    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");
    println!("WebSocket connection established: {}", addr);

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
            sender.unbounded_send(tungstenite::protocol::Message::Text(json_msg.into())).unwrap();
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
                                sender.unbounded_send(tungstenite::protocol::Message::Text(json_response.into())).unwrap();
                            }

                            let broadcast_message = WsMessage::GlobalUpdate {
                                total_clicks: state.total_clicks,
                            };
                            let json_broadcast = serde_json::to_string(&broadcast_message).unwrap();

                            let peers = peer_map.lock().unwrap();
                            for (peer_addr, sender) in peers.iter() {
                                if *peer_addr != addr {
                                     sender.unbounded_send(tungstenite::protocol::Message::Text(json_broadcast.clone().into())).unwrap();
                                }
                            }
                        }
                        WsMessage::Ping => {
                             if let Some(sender) = peer_map.lock().unwrap().get(&addr) {
                                let pong = WsMessage::Pong;
                                let json_pong = serde_json::to_string(&pong).unwrap();
                                sender.unbounded_send(tungstenite::protocol::Message::Text(json_pong.into())).unwrap();
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
}

#[tokio::main]
async fn main() -> Result<(), IoError> {
    let addr = "0.0.0.0:8765";
    let listener = TcpListener::bind(&addr).await.expect("Can's listen");
    println!("Click Counter Server running at ws://{}", addr);

    let state = Arc::new(Mutex::new(AppState::new()));
    let peer_map = PeerMap::new(Mutex::new(HashMap::new()));

    println!("Press Ctrl+C to shutdown the programs.");
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
                break;
            }
        }
    }

    Ok(())
}