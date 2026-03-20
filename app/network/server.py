# app/network/server.py

from __future__ import annotations

from contextlib import asynccontextmanager
from typing import Callable
from fastapi import FastAPI, HTTPException
from app.ledger.node import Node
from app.ledger.transaction import TransactionVertex
from app.network.client import NetworkClient
from app.network.peer_list import PeerList


class NodeServer:
    def __init__(
        self,
        node: Node,
        peers: PeerList | None = None,
        lifespan: Callable | None = None,
    ) -> None:
        self.node = node
        self.peers = peers or PeerList()
        self.client = NetworkClient()
        self.app = FastAPI(lifespan=lifespan) if lifespan else FastAPI()
        self._register_routes()


    def _register_routes(self) -> None:
        app = self.app

        @app.get("/ping")
        def ping():
            return {"status": "ok"}

        @app.get("/peers")
        def get_peers():
            return {"peers": self.peers.get_all()}

        @app.post("/peers/add")
        def add_peer(data: dict):
            address = data.get("address", "")
            if address:
                self.peers.add(address)
            return {"peers": self.peers.get_all()}

        @app.get("/state")
        def get_state():
            return self.node.get_state_view()

        @app.get("/dag")
        def get_dag():
            return self.node.get_dag_view()

        @app.get("/mempool")
        def get_mempool():
            return self.node.get_mempool_view()

        @app.post("/receive_transaction")
        def receive_transaction(data: dict):
            try:
                tx = TransactionVertex.from_dict(data)
            except Exception as e:
                raise HTTPException(status_code=400, detail=f"invalid transaction: {e}")

            result = self.node.submit_transaction(tx)

            if result.ok:
                self.client.broadcast_transaction(tx, self.peers)

            return {
                "ok": result.ok,
                "code": result.code,
                "reason": result.reason,
            }
        
        @app.post("/sync_state")
        def sync_state(data: dict):
            """Синхронизирует balances от другого узла."""
            balances = data.get("balances", {})
            for address, balance in balances.items():
                self.node.state.balances[address] = balance
                self.node.state.ensure_account(address)
            return {"ok": True}
