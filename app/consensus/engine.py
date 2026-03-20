# app/consensus/engine.py

from __future__ import annotations

from dataclasses import dataclass

from app.config import CONFIRMATION_THRESHOLD
from app.consensus.conflict_resolver import ConflictResolver
from app.ledger.dag import DAG
from app.ledger.mempool import Mempool
from app.ledger.state import LedgerState
from app.ledger.transaction import (
    TX_STATUS_CONFLICT,
    TX_STATUS_CONFIRMED,
    TX_STATUS_REJECTED,
    TransactionVertex,
)


@dataclass
class ConsensusDecision:
    accepted: bool
    code: str
    reason: str


class ConsensusEngine:
    def __init__(self, confirmation_threshold: int = CONFIRMATION_THRESHOLD) -> None:
        self.confirmation_threshold = confirmation_threshold

    def resolve_conflict(
        self,
        tx: TransactionVertex,
        conflicts: list[TransactionVertex],
    ) -> ConsensusDecision:
        if not conflicts:
            return ConsensusDecision(True, "accepted", "no conflict")

        all_txs = [tx, *conflicts]
        winner = min(all_txs, key=lambda t: (t.timestamp, t.tx_id))

        if winner.tx_id == tx.tx_id:
            for loser in conflicts:
                loser.status = TX_STATUS_CONFLICT
            return ConsensusDecision(True, "accepted_conflict_winner", "tx won conflict resolution")

        tx.status = TX_STATUS_CONFLICT
        return ConsensusDecision(False, "conflict_loser", "tx lost conflict resolution")

    def confirm_transactions(self, dag: DAG) -> None:
        for tx in dag.vertices.values():
            if tx.status in (TX_STATUS_REJECTED, TX_STATUS_CONFLICT):
                continue
            if tx.weight >= self.confirmation_threshold:
                tx.status = TX_STATUS_CONFIRMED

    def process_mempool(
        self,
        mempool: Mempool,
        dag: DAG,
        state: LedgerState,
        conflicts: ConflictResolver,
    ) -> list[TransactionVertex]:
        accepted: list[TransactionVertex] = []

        for tx in mempool.get_all():
            conflict_ids = conflicts.get_conflicts(tx) - {tx.tx_id}
            existing_conflicts = [
                dag.vertices[cid] for cid in conflict_ids if cid in dag.vertices
            ]

            decision = self.resolve_conflict(tx, existing_conflicts)
            if not decision.accepted:
                continue

            ok, _ = state.can_apply(tx)
            if not ok:
                tx.status = TX_STATUS_REJECTED
                continue

            state.apply_transaction(tx)
            dag.add_transaction(tx)
            dag.propagate_weight(tx.tx_id)
            conflicts.resolve(dag, tx)
            accepted.append(tx)

        for tx in accepted:
            mempool.remove(tx.tx_id)

        self.confirm_transactions(dag)
        return accepted