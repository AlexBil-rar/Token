from __future__ import annotations

from dataclasses import dataclass, field

from app.ledger.transaction import TransactionVertex


@dataclass
class Mempool:
    transactions: dict[str, TransactionVertex] = field(default_factory=dict)

    def add(self, tx: TransactionVertex) -> None:
        self.transactions[tx.tx_id] = tx

    def remove(self, tx_id: str) -> None:
        self.transactions.pop(tx_id, None)

    def has(self, tx_id: str) -> bool:
        return tx_id in self.transactions

    def get(self, tx_id: str) -> TransactionVertex | None:
        return self.transactions.get(tx_id)

    def get_all(self) -> list[TransactionVertex]:
        return list(self.transactions.values())

    def clear(self) -> None:
        self.transactions.clear()

    def size(self) -> int:
        return len(self.transactions)