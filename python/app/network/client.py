# app/network/client.py

from __future__ import annotations

import httpx

from app.ledger.transaction import TransactionVertex
from app.network.peer_list import PeerList


class NetworkClient:
    def __init__(self, timeout: float = 3.0) -> None:
        self.timeout = timeout

    def broadcast_transaction(self, tx: TransactionVertex, peers: PeerList) -> dict[str, bool]:
        results: dict[str, bool] = {}

        for peer in peers.get_all():
            try:
                response = httpx.post(
                    f"{peer}/receive_transaction",
                    json=tx.to_dict(),
                    timeout=self.timeout,
                )
                results[peer] = response.status_code == 200
            except Exception:
                results[peer] = False

        return results

    def ping(self, peer_address: str) -> bool:
        try:
            response = httpx.get(f"{peer_address}/ping", timeout=self.timeout)
            return response.status_code == 200
        except Exception:
            return False

    def fetch_peers(self, peer_address: str) -> list[str]:
        try:
            response = httpx.get(f"{peer_address}/peers", timeout=self.timeout)
            if response.status_code == 200:
                return response.json().get("peers", [])
        except Exception:
            pass
        return []