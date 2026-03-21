# app/network/ws_server.py

from __future__ import annotations

import asyncio
import json

import websockets

from app.ledger.node import Node
from app.network.ws_manager import WSConnectionManager, WSMessage, MessageType


class WSServer:
    def __init__(self, node: Node, host: str = "0.0.0.0", port: int = 9000) -> None:
        self.node = node
        self.host = host
        self.port = port
        self.manager = WSConnectionManager()
        self._server = None

    async def start(self) -> None:
        self._server = await websockets.serve(
            self._handle_connection,
            self.host,
            self.port,
        )
        print(f"WebSocket server started on ws://{self.host}:{self.port}")

    async def stop(self) -> None:
        if self._server:
            self._server.close()
            await self._server.wait_closed()

    async def _handle_connection(self, websocket, path: str = "") -> None:
        peer_address = f"{websocket.remote_address[0]}:{websocket.remote_address[1]}"
        self.manager.register(peer_address, websocket)
        print(f"Peer connected: {peer_address}")

        try:
            async for raw_message in websocket:
                await self._handle_message(raw_message, peer_address, websocket)
        except websockets.exceptions.ConnectionClosed:
            pass
        finally:
            self.manager.unregister(peer_address)
            print(f"Peer disconnected: {peer_address}")

    async def _handle_message(
        self, raw: str, sender: str, websocket
    ) -> None:
        try:
            msg = WSMessage.from_json(raw)
        except Exception:
            return

        if msg.type == MessageType.PING:
            await self._handle_ping(websocket, sender)

        elif msg.type == MessageType.TRANSACTION:
            await self._handle_transaction(msg, sender)

        elif msg.type == MessageType.STATE_REQUEST:
            await self._handle_state_request(websocket)

        elif msg.type == MessageType.PEER_LIST:
            await self._handle_peer_list(msg)

    async def _handle_ping(self, websocket, sender: str) -> None:
        pong = WSMessage(
            type=MessageType.PONG,
            payload={"status": "ok"},
            sender=f"{self.host}:{self.port}",
        )
        try:
            await websocket.send(pong.to_json())
        except Exception:
            pass

    async def _handle_transaction(self, msg: WSMessage, sender: str) -> None:
        from app.ledger.transaction import TransactionVertex
        try:
            tx = TransactionVertex.from_dict(msg.payload)
        except Exception:
            return

        result = self.node.submit_transaction(tx)

        if result.ok:
            broadcast_msg = WSMessage(
                type=MessageType.TRANSACTION,
                payload=msg.payload,
                sender=f"{self.host}:{self.port}",
            )
            await self.manager.broadcast(broadcast_msg, exclude=sender)

    async def _handle_state_request(self, websocket) -> None:
        state = self.node.get_state_view()
        response = WSMessage(
            type=MessageType.STATE_RESPONSE,
            payload=state,
            sender=f"{self.host}:{self.port}",
        )
        try:
            await websocket.send(response.to_json())
        except Exception:
            pass

    async def _handle_peer_list(self, msg: WSMessage) -> None:
        peers = msg.payload.get("peers", [])