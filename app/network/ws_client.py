# app/network/ws_client.py

from __future__ import annotations

import asyncio
import json

import websockets

from app.ledger.transaction import TransactionVertex
from app.network.ws_manager import WSMessage, MessageType


class WSClient:
    def __init__(self, timeout: float = 5.0) -> None:
        self.timeout = timeout

    async def send_transaction(
        self, peer_url: str, tx: TransactionVertex
    ) -> bool:
        msg = WSMessage(
            type=MessageType.TRANSACTION,
            payload=tx.to_dict(),
        )
        return await self._send(peer_url, msg)

    async def ping(self, peer_url: str) -> bool:
        msg = WSMessage(type=MessageType.PING, payload={})
        try:
            async with websockets.connect(peer_url, open_timeout=self.timeout) as ws:
                await ws.send(msg.to_json())
                raw = await asyncio.wait_for(ws.recv(), timeout=self.timeout)
                response = WSMessage.from_json(raw)
                return response.type == MessageType.PONG
        except Exception:
            return False

    async def fetch_state(self, peer_url: str) -> dict | None:
        msg = WSMessage(type=MessageType.STATE_REQUEST, payload={})
        try:
            async with websockets.connect(peer_url, open_timeout=self.timeout) as ws:
                await ws.send(msg.to_json())
                raw = await asyncio.wait_for(ws.recv(), timeout=self.timeout)
                response = WSMessage.from_json(raw)
                if response.type == MessageType.STATE_RESPONSE:
                    return response.payload
        except Exception:
            pass
        return None

    async def broadcast(
        self, peers: list[str], tx: TransactionVertex
    ) -> dict[str, bool]:
        tasks = {peer: self.send_transaction(peer, tx) for peer in peers}
        results = {}
        for peer, coro in tasks.items():
            try:
                results[peer] = await coro
            except Exception:
                results[peer] = False
        return results

    async def _send(self, peer_url: str, msg: WSMessage) -> bool:
        try:
            async with websockets.connect(peer_url, open_timeout=self.timeout) as ws:
                await ws.send(msg.to_json())
                return True
        except Exception:
            return False