# app/ledger/node.py

from __future__ import annotations

import time
from dataclasses import dataclass, field
from pathlib import Path

from app.ledger.pruner import Pruner


from app.consensus.conflict_resolver import ConflictResolver
from app.config import ANTI_SPAM_DIFFICULTY
from app.consensus.engine import ConsensusEngine
from app.crypto.wallet import Wallet
from app.ledger.dag import DAG
from app.ledger.mempool import Mempool
from app.ledger.state import LedgerState
from app.ledger.transaction import TransactionVertex
from app.ledger.validator import ValidationResult, Validator
from app.consensus.tip_selector import TipSelector
from app.storage.snapshot import SnapshotStorage


@dataclass
class Node:
    conflicts: ConflictResolver = field(default_factory=ConflictResolver)
    dag: DAG = field(default_factory=DAG)
    state: LedgerState = field(default_factory=LedgerState)
    validator: Validator = field(default_factory=Validator)
    mempool: Mempool = field(default_factory=Mempool)
    consensus: ConsensusEngine = field(default_factory=ConsensusEngine)
    tip_selector: TipSelector = field(default_factory=TipSelector)
    storage: SnapshotStorage = field(default_factory=SnapshotStorage)
    pruner: Pruner = field(default_factory=Pruner)


    def select_parents(self) -> list[str]:
        return self.tip_selector.select(self.dag)

    def bootstrap_genesis(self, address: str, balance: int) -> None:
        self.state.credit(address, balance)

    def load_snapshot(self) -> bool:
        loaded = self.storage.load(self.dag, self.state)
        if loaded is not None:
            print(f"Snapshot loaded: {len(self.dag.vertices)} transactions")
        return loaded is not None

    def save_snapshot(self) -> None:
        self.storage.save(self.dag, self.state)

    def mine_anti_spam(self, tx: TransactionVertex) -> None:
        nonce = 0
        while True:
            tx.anti_spam_nonce = nonce
            tx.anti_spam_hash = tx.compute_anti_spam_hash()
            if tx.anti_spam_hash.startswith("0" * ANTI_SPAM_DIFFICULTY):
                return
            nonce += 1

    def create_transaction(self, wallet: Wallet, receiver: str, amount: int) -> TransactionVertex:
        nonce = self.state.get_nonce(wallet.address) + 1
        parents = self.select_parents()

        tx = TransactionVertex(
            sender=wallet.address,
            receiver=receiver,
            amount=amount,
            nonce=nonce,
            timestamp=int(time.time()),
            public_key=wallet.public_key,
            parents=parents,
        )

        self.mine_anti_spam(tx)
        tx.signature = wallet.sign(tx.signing_payload())
        tx.finalize()
        return tx

    def submit_transaction(self, tx: TransactionVertex) -> ValidationResult:
        result = self.validator.validate_full(tx, self.dag, self.state)
        if not result.ok:
            return result

        if self.mempool.has(tx.tx_id):
            return ValidationResult(False, "duplicate_mempool", "transaction already in mempool")

        self.mempool.add(tx)
        self.conflicts.register_transaction(tx)

        accepted = self.consensus.process_mempool(
            self.mempool,
            self.dag,
            self.state,
            self.conflicts,
        )

        if any(item.tx_id == tx.tx_id for item in accepted):
            self.save_snapshot()                  
            
            if self.pruner.should_prune(self.dag):
                result = self.pruner.prune(self.dag, self.state)
                print(f"Pruned {result.pruned_count} old transactions")

            return ValidationResult(True, "accepted", "transaction accepted")

        return ValidationResult(False, "not_committed", "transaction did not pass consensus")

    def faucet(self, address: str, amount: int) -> None:
        self.state.credit(address, amount)

    def get_state_view(self) -> dict[str, object]:
        return {
            "balances": dict(self.state.balances),
            "nonces": dict(self.state.nonces),
            "applied_txs": list(self.state.applied_txs),
        }

    def get_dag_view(self) -> dict[str, object]:
        return {
            "stats": self.dag.stats(),
            "tips": self.dag.get_tips(),
            "transactions": {
                tx_id: tx.to_dict()
                for tx_id, tx in self.dag.vertices.items()
            },
        }

    def get_mempool_view(self) -> dict[str, object]:
        return {
            "size": self.mempool.size(),
            "transactions": [tx.to_dict() for tx in self.mempool.get_all()],
        }