# app/ledger/state.py

from __future__ import annotations

from dataclasses import dataclass, field

from app.ledger.transaction import TransactionVertex


@dataclass(slots=True)
class LedgerState:
    balances: dict[str, int] = field(default_factory=dict)
    nonces: dict[str, int] = field(default_factory=dict)
    applied_txs: set[str] = field(default_factory=set)

    def ensure_account(self, address: str) -> None:
        if address not in self.balances:
            self.balances[address] = 0
        if address not in self.nonces:
            self.nonces[address] = 0

    def get_balance(self, address: str) -> int:
        self.ensure_account(address)
        return self.balances[address]

    def get_nonce(self, address: str) -> int:
        self.ensure_account(address)
        return self.nonces[address]

    def credit(self, address: str, amount: int) -> None:
        self.ensure_account(address)
        self.balances[address] += amount

    def can_apply(self, tx: TransactionVertex) -> tuple[bool, str]:
        self.ensure_account(tx.sender)
        self.ensure_account(tx.receiver)

        if tx.tx_id in self.applied_txs:
            return False, "transaction already applied"

        if tx.amount <= 0:
            return False, "amount must be positive"

        if self.balances[tx.sender] < tx.amount:
            return False, "insufficient balance"

        expected_nonce = self.nonces[tx.sender] + 1
        if tx.nonce != expected_nonce:
            return False, f"invalid nonce: expected {expected_nonce}, got {tx.nonce}"

        return True, "ok"

    def apply_transaction(self, tx: TransactionVertex) -> None:
        ok, reason = self.can_apply(tx)
        if not ok:
            raise ValueError(reason)

        self.balances[tx.sender] -= tx.amount
        self.balances[tx.receiver] += tx.amount
        self.nonces[tx.sender] = tx.nonce
        self.applied_txs.add(tx.tx_id)

    def snapshot(self) -> dict[str, object]:
        return {
            "balances": dict(self.balances),
            "nonces": dict(self.nonces),
            "applied_txs": list(self.applied_txs),
        }