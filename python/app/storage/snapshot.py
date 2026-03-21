# app/storage/snapshot.py

from __future__ import annotations

import json
import os
from pathlib import Path

from app.ledger.dag import DAG
from app.ledger.state import LedgerState
from app.ledger.transaction import TransactionVertex


DATA_DIR = Path("data")
SNAPSHOT_FILE = DATA_DIR / "dag_snapshot.json"


class SnapshotStorage:
    def __init__(self, path: Path = SNAPSHOT_FILE) -> None:
        self.path = path
        self.path.parent.mkdir(parents=True, exist_ok=True)

    def save(self, dag: DAG, state: LedgerState, wallet_data: dict | None = None) -> None:
        data = {
            "vertices": {
                tx_id: tx.to_dict()
                for tx_id, tx in dag.vertices.items()
            },
            "state": state.snapshot(),
            "wallet": wallet_data or {},
        }
        with open(self.path, "w") as f:
            json.dump(data, f, indent=2)

    def load(self, dag: DAG, state: LedgerState) -> dict | None:
        if not self.path.exists():
            return None

        with open(self.path) as f:
            data = json.load(f)

        for tx_dict in data.get("vertices", {}).values():
            tx = TransactionVertex.from_dict(tx_dict)
            if not dag.has_transaction(tx.tx_id):
                dag.vertices[tx.tx_id] = tx
                for parent_id in tx.parents:
                    dag.children_map[parent_id].add(tx.tx_id)
                    dag.tips.discard(parent_id)
                dag.tips.add(tx.tx_id)

        saved_state = data.get("state", {})
        state.balances.update(saved_state.get("balances", {}))
        state.nonces.update(saved_state.get("nonces", {}))
        state.applied_txs.update(saved_state.get("applied_txs", []))

        return data 

    def exists(self) -> bool:
        return self.path.exists()

    def delete(self) -> None:
        if self.path.exists():
            self.path.unlink()