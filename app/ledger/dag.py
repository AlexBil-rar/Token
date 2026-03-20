# app/ledger/dag.py

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field

from app.config import CONFIRMATION_THRESHOLD
from app.ledger.transaction import (
    TX_STATUS_CONFIRMED,
    TX_STATUS_REJECTED,
    TX_STATUS_CONFLICT,
    TransactionVertex,
)

@dataclass(slots=True)
class DAG:
    vertices: dict[str, TransactionVertex] = field(default_factory=dict)
    children_map: dict[str, set[str]] = field(default_factory=lambda: defaultdict(set))
    tips: set[str] = field(default_factory=set)
    
    def propagate_weight(self, tx_id: str) -> None:
        visited: set[str] = set()
        stack: list[str] = list(self.vertices[tx_id].parents)

        while stack:
            parent_id = stack.pop()

            if parent_id in visited:
                continue

            visited.add(parent_id)

            parent = self.vertices.get(parent_id)
            if parent is None:
                continue

            parent.weight += 1

            if parent.weight >= CONFIRMATION_THRESHOLD and parent.status != TX_STATUS_REJECTED:
                parent.status = TX_STATUS_CONFIRMED

            stack.extend(parent.parents)
            
    def has_transaction(self, tx_id: str) -> bool:
        return tx_id in self.vertices

    def get_transaction(self, tx_id: str) -> TransactionVertex | None:
        return self.vertices.get(tx_id)

    def get_tips(self) -> list[str]:
        valid_tips: list[str] = []
        for tx_id in self.tips:
            tx = self.vertices.get(tx_id)
            if tx and tx.status not in (TX_STATUS_REJECTED, TX_STATUS_CONFLICT):
                valid_tips.append(tx_id)
        return valid_tips

    def add_transaction(self, tx: TransactionVertex) -> None:
        if tx.tx_id in self.vertices:
            raise ValueError(f"Transaction already exists: {tx.tx_id}")

        self.vertices[tx.tx_id] = tx

        for parent_id in tx.parents:
            self.children_map[parent_id].add(tx.tx_id)
            self.tips.discard(parent_id)

        self.tips.add(tx.tx_id)

    def get_children(self, tx_id: str) -> list[str]:
        return list(self.children_map.get(tx_id, set()))

    def stats(self) -> dict[str, int]:
        confirmed = 0
        rejected = 0
        pending = 0
        conflict = 0

        for tx in self.vertices.values():
            if tx.status == TX_STATUS_CONFIRMED:
                confirmed += 1
            elif tx.status == TX_STATUS_REJECTED:
                rejected += 1
            elif tx.status == TX_STATUS_CONFLICT:
                conflict += 1
            else:
                pending += 1

        return {
            "total_vertices": len(self.vertices),
            "tips": len(self.get_tips()),
            "confirmed": confirmed,
            "pending": pending,
            "rejected": rejected,
            "conflict": conflict,
        }