# app/branches/branch.py

from __future__ import annotations

from dataclasses import dataclass, field

from app.consensus.conflict_resolver import ConflictResolver
from app.consensus.engine import ConsensusEngine
from app.consensus.tip_selector import TipSelector
from app.ledger.dag import DAG
from app.ledger.mempool import Mempool
from app.ledger.state import LedgerState
from app.ledger.transaction import TransactionVertex
from app.ledger.validator import ValidationResult, Validator


@dataclass
class Branch:
    branch_id: str
    dag: DAG = field(default_factory=DAG)
    state: LedgerState = field(default_factory=LedgerState)
    mempool: Mempool = field(default_factory=Mempool)
    conflicts: ConflictResolver = field(default_factory=ConflictResolver)
    consensus: ConsensusEngine = field(default_factory=ConsensusEngine)
    tip_selector: TipSelector = field(default_factory=TipSelector)
    validator: Validator = field(default_factory=Validator)

    def submit_transaction(self, tx: TransactionVertex) -> ValidationResult:
        result = self.validator.validate_full(tx, self.dag, self.state)
        if not result.ok:
            return result

        if self.mempool.has(tx.tx_id):
            return ValidationResult(False, "duplicate_mempool", "already in mempool")

        self.mempool.add(tx)
        self.conflicts.register_transaction(tx)

        accepted = self.consensus.process_mempool(
            self.mempool,
            self.dag,
            self.state,
            self.conflicts,
        )

        if any(item.tx_id == tx.tx_id for item in accepted):
            return ValidationResult(True, "accepted", f"accepted in branch {self.branch_id}")

        return ValidationResult(False, "not_committed", "did not pass consensus")

    def get_stats(self) -> dict:
        return {
            "branch_id": self.branch_id,
            "dag": self.dag.stats(),
            "balances": dict(self.state.balances),
        }

    def snapshot(self) -> dict:
        return {
            "branch_id": self.branch_id,
            "balances": dict(self.state.balances),
            "nonces": dict(self.state.nonces),
            "applied_txs": list(self.state.applied_txs),
            "vertices": {
                tx_id: tx.to_dict()
                for tx_id, tx in self.dag.vertices.items()
            },
        }