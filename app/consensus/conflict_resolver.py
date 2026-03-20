# app/consensus/conflict_resolver.py

# TODO: унифицировать с ConflictResolver на следующем этапе

from __future__ import annotations

from dataclasses import dataclass, field

from app.ledger.dag import DAG
from app.ledger.transaction import (
    TX_STATUS_CONFLICT,
    TransactionVertex,
)


@dataclass
class ConflictResolver:
    conflict_sets: dict[tuple[str, int], set[str]] = field(default_factory=dict)

    def register_transaction(self, tx: TransactionVertex) -> None:
        key = (tx.sender, tx.nonce)

        if key not in self.conflict_sets:
            self.conflict_sets[key] = set()

        self.conflict_sets[key].add(tx.tx_id)

    def get_conflicts(self, tx: TransactionVertex) -> set[str]:
        key = (tx.sender, tx.nonce)
        return self.conflict_sets.get(key, set())

    def resolve(self, dag: DAG, tx: TransactionVertex) -> None:
        conflicts = self.get_conflicts(tx)

        if len(conflicts) <= 1:
            return

        vertices = [dag.vertices[c] for c in conflicts if c in dag.vertices]

        winner = max(vertices, key=lambda v: v.weight)

        for v in vertices:
            if v.tx_id != winner.tx_id:
                v.status = TX_STATUS_CONFLICT