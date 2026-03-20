# app/network/peer_list.py

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class PeerList:
    peers: set[str] = field(default_factory=set)

    def add(self, address: str) -> None:
        self.peers.add(address.rstrip("/"))

    def remove(self, address: str) -> None:
        self.peers.discard(address.rstrip("/"))

    def get_all(self) -> list[str]:
        return list(self.peers)

    def has(self, address: str) -> bool:
        return address.rstrip("/") in self.peers

    def size(self) -> int:
        return len(self.peers)