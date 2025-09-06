import asyncio
import websockets
import json
from datetime import datetime

PORT = 8765
IP = "0.0.0.0"

connected_users = set()
total_clicks = 0
current_image_id = 1
client_clicks = {}

async def broadcast_all(message):
    if connected_users:  # เช็คก่อนว่ามี client เชื่อมต่อไหม
        await asyncio.gather(
            *[send_safe(user, message) for user in connected_users]
        )

async def send_safe(websocket, message):
    try:
        await websocket.send(message)
    except websockets.exceptions.ConnectionClosed:
        # Client ตัดการเชื่อมต่อระหว่าง broadcast
        connected_users.discard(websocket)
        client_clicks.pop(id(websocket), None)

async def send_init(websocket):
    message = json.dumps({
        "type": "init",
        "total_clicks": total_clicks,
    })
    await websocket.send(message)

async def handle_click(websocket):
    global total_clicks, current_image_id

    client_id = id(websocket)
    client_clicks[client_id] = client_clicks.get(client_id, 0) + 1
    total_clicks += 1

    response = json.dumps({
        "type": "click_response",
        "client_clicks": client_clicks[client_id],
        "total_clicks": total_clicks,
        "timestamp": datetime.now().isoformat(),
    })
    await websocket.send(response)

    broadcast_message = json.dumps({
        "type": "global_update",
        "total_clicks": total_clicks,
    })
    await broadcast_all(broadcast_message)

async def handler(websocket):
    connected_users.add(websocket)
    print(f"Client connected: {id(websocket)} | Total clients: {len(connected_users)}")
    await send_init(websocket)

    try:
        async for message in websocket:
            data = json.loads(message)
            if data.get("type") == "click":
                await handle_click(websocket)
            elif data.get("type") == "ping": 
                await websocket.send(json.dumps({"type": "pong"}))
    except websockets.exceptions.ConnectionClosed:
        print(f"Client disconnected: {id(websocket)}")
    finally:
        connected_users.discard(websocket)
        client_clicks.pop(id(websocket), None)
        print(f"Client removed: {id(websocket)} | Total clients: {len(connected_users)}")

async def main():
    print(f"Click Counter Server running at ws://{IP}:{PORT}")
    async with websockets.serve(handler, IP, PORT):
        await asyncio.Future()  # run forever

if __name__ == "__main__":
    asyncio.run(main())
