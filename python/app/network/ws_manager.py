# app/network/ws_manager.py

from __future__ import annotations

import asyncio
import json
import time
from dataclasses import dataclass, field
from enum import Enum


class MessageType(Enum):
    TRANSACTION = "transaction"
    PING = "ping"
    PONG = "pong"
    STATE_REQUEST = "state_request"
    STATE_RESPONSE = "state_response"
    PEER_LIST = "peer_list"


@dataclass
class WSMessage:
    type: MessageType
    payload: dict
    timestamp: float = field(default_factory=time.time)
    sender: str = ""

    def to_json(self) -> str:
        return json.dumps({
            "type": self.type.value,
            "payload": self.payload,
            "timestamp": self.timestamp,
            "sender": self.sender,
        })

    @classmethod
    def from_json(cls, data: str) -> "WSMessage":
        d = json.loads(data)
        return cls(
            type=MessageType(d["type"]),
            payload=d.get("payload", {}),
            timestamp=d.get("timestamp", time.time()),
            sender=d.get("sender", ""),
        )


class WSConnectionManager:
    def __init__(self) -> None:
        self.connections: dict[str, object] = {}
        self.last_seen: dict[str, float] = {}

    def register(self, address: str, websocket: object) -> None:
        self.connections[address] = websocket
        self.last_seen[address] = time.time()

    def unregister(self, address: str) -> None:
        self.connections.pop(address, None)
        self.last_seen.pop(address, None)

    def is_connected(self, address: str) -> bool:
        return address in self.connections

    def get_active_peers(self) -> list[str]:
        return list(self.connections.keys())

    async def send(self, address: str, message: WSMessage) -> bool:
        ws = self.connections.get(address)
        if ws is None:
            return False
        try:
            await ws.send(message.to_json())
            return True
        except Exception:
            self.unregister(address)
            return False

    async def broadcast(self, message: WSMessage, exclude: str = "") -> dict[str, bool]:
        results = {}
        for address in list(self.connections.keys()):
            if address == exclude:
                continue
            results[address] = await self.send(address, message)
        return results

    def stats(self) -> dict:
        return {
            "active_connections": len(self.connections),
            "peers": self.get_active_peers(),
        }