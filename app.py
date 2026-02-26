import asyncio
import json
import os
from aiohttp import web, WSMsgType
from pathlib import Path

TOTAL_FILE = Path("total.txt")

class AppState:
    def __init__(self):
        self.total_clicks = 0
        self.peers = set()  # set of WebSocketResponse
        self.lock = asyncio.Lock()


def read_total_from_file() -> int:
    try:
        if TOTAL_FILE.exists():
            s = TOTAL_FILE.read_text().strip()
            return int(s) if s else 0
    except Exception as e:
        print(f"Failed to read total from file: {e}")
    return 0


def save_total_to_file(total: int):
    try:
        # Ensure parent exists
        if TOTAL_FILE.parent and not TOTAL_FILE.parent.exists():
            TOTAL_FILE.parent.mkdir(parents=True, exist_ok=True)
        TOTAL_FILE.write_text(str(total))
    except Exception as e:
        print(f"Failed to write total to {TOTAL_FILE}: {e}")


async def websocket_handler(request: web.Request):
    state: AppState = request.app["state"]

    ws = web.WebSocketResponse()
    await ws.prepare(request)

    peer = request.remote or str(request.transport.get_extra_info("peername"))
    addr = str(peer)
    print(f"Incoming WS connection from: {addr}")

    async with state.lock:
        first_client = len(state.peers) == 0
        state.peers.add(ws)
        if first_client:
            # reload total
            state.total_clicks = read_total_from_file()

    # send init message
    await ws.send_json({"type": "init", "total_clicks": state.total_clicks})

    try:
        async for msg in ws:
            if msg.type == WSMsgType.TEXT:
                try:
                    data = json.loads(msg.data)
                except Exception:
                    continue

                mtype = data.get("type")
                if mtype == "click":
                    async with state.lock:
                        state.total_clicks += 1
                        total = state.total_clicks

                    # reply to sender
                    await ws.send_json({
                        "total_clicks": total,
                    })

                    # broadcast to others
                    bmsg = {"total_clicks": total}
                    async with state.lock:
                        peers = list(state.peers)
                    for p in peers:
                        if p is not ws:
                            try:
                                await p.send_json(bmsg)
                            except Exception:
                                pass

                elif mtype == "ping":
                    await ws.send_json({"type": "pong"})

            elif msg.type == WSMsgType.ERROR:
                print(f"WebSocket connection closed with exception: {ws.exception()}")

    finally:
        async with state.lock:
            if ws in state.peers:
                state.peers.remove(ws)
            no_clients = len(state.peers) == 0
            if no_clients:
                save_total_to_file(state.total_clicks)
        await ws.close()
        print(f"{addr} disconnected")

    return ws


async def index(request: web.Request):
    # serve index.html
    p = Path("index.html")
    if p.exists():
        return web.FileResponse(path=p)
    return web.Response(status=404, text="index.html not found")


async def on_shutdown(app: web.Application):
    state: AppState = app["state"]
    async with state.lock:
        save_total_to_file(state.total_clicks)
    # close websockets
    peers = list(state.peers)
    for ws in peers:
        try:
            await ws.close()
        except Exception:
            pass


def create_app():
    app = web.Application()
    app["state"] = AppState()

    app.router.add_get("/ws", websocket_handler)
    # static files (js, css, photos)
    app.router.add_static("/photos", Path("photos"), show_index=False)
    app.router.add_get("/", index)
    app.router.add_static("/", Path("."), show_index=False)

    app.on_shutdown.append(on_shutdown)
    return app


if __name__ == "__main__":
    dotenv_path = Path('.env')
    # load env if any
    try:
        from dotenv import load_dotenv
        if dotenv_path.exists():
            load_dotenv(dotenv_path)
    except Exception:
        pass

    port = int(os.environ.get("PORT", "8765"))
    host = os.environ.get("HOST", "0.0.0.0")

    app = create_app()
    web.run_app(app, host=host, port=port)
