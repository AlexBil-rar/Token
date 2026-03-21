# app/ledger/pruner.py

from __future__ import annotations

from dataclasses import dataclass

from app.ledger.dag import DAG
from app.ledger.state import LedgerState
from app.ledger.transaction import TX_STATUS_CONFIRMED


@dataclass
class PruneResult:
    pruned_count: int
    remaining_count: int
    state_preserved: bool


class Pruner:
    def __init__(self, window: int = 10_000) -> None:
        self.window = window

    def should_prune(self, dag: DAG, interval: int = 1_000) -> bool:
        return len(dag.vertices) >= interval and len(dag.vertices) % interval == 0

    def prune(self, dag: DAG, state: LedgerState) -> PruneResult:
        total = len(dag.vertices)

        if total <= self.window:
            return PruneResult(
                pruned_count=0,
                remaining_count=total,
                state_preserved=True,
            )

        sorted_txs = sorted(
            dag.vertices.values(),
            key=lambda tx: tx.timestamp,
        )

        to_delete_count = total - self.window
        candidates = sorted_txs[:to_delete_count]

        current_tips = set(dag.get_tips())

        pruned = 0
        for tx in candidates:
            if tx.tx_id in current_tips:
                continue
            if tx.status != TX_STATUS_CONFIRMED:
                continue

            dag.vertices.pop(tx.tx_id, None)

            for parent_id in tx.parents:
                if parent_id in dag.children_map:
                    dag.children_map[parent_id].discard(tx.tx_id)
                    if not dag.children_map[parent_id]:
                        del dag.children_map[parent_id]

            dag.tips.discard(tx.tx_id)

            pruned += 1

        return PruneResult(
            pruned_count=pruned,
            remaining_count=len(dag.vertices),
            state_preserved=True,
        )

    def stats(self, dag: DAG) -> dict:
        confirmed = sum(
            1 for tx in dag.vertices.values()
            if tx.status == TX_STATUS_CONFIRMED
        )
        return {
            "total_vertices": len(dag.vertices),
            "confirmed": confirmed,
            "window": self.window,
            "prunable": max(0, confirmed - self.window),
        }