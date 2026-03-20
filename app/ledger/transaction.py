# app/ledger/transaction.py

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from app.crypto.hashing import sha256_hex, stable_json_dumps


TX_STATUS_PENDING = "pending"
TX_STATUS_CONFIRMED = "confirmed"
TX_STATUS_REJECTED = "rejected"
TX_STATUS_CONFLICT = "conflict"


@dataclass(slots=True)
class TransactionVertex:
    sender: str
    receiver: str
    amount: int
    nonce: int
    timestamp: int
    public_key: str
    parents: list[str] = field(default_factory=list)
    signature: str = ""
    anti_spam_nonce: int = 0
    anti_spam_hash: str = ""
    status: str = TX_STATUS_PENDING
    weight: int = 1
    tx_id: str = ""

    def signing_payload(self) -> bytes:
        payload = {
            "sender": self.sender,
            "receiver": self.receiver,
            "amount": self.amount,
            "nonce": self.nonce,
            "timestamp": self.timestamp,
            "public_key": self.public_key,
            "parents": self.parents,
            "anti_spam_nonce": self.anti_spam_nonce,
        }
        return stable_json_dumps(payload)

    def compute_anti_spam_hash(self) -> str:
        return sha256_hex(self.signing_payload())

    def compute_tx_id(self) -> str:
        payload = {
            "sender": self.sender,
            "receiver": self.receiver,
            "amount": self.amount,
            "nonce": self.nonce,
            "timestamp": self.timestamp,
            "public_key": self.public_key,
            "parents": self.parents,
            "anti_spam_nonce": self.anti_spam_nonce,
            "anti_spam_hash": self.anti_spam_hash,
            "signature": self.signature,
        }
        return sha256_hex(stable_json_dumps(payload))

    def finalize(self) -> None:
        self.anti_spam_hash = self.compute_anti_spam_hash()
        self.tx_id = self.compute_tx_id()

    def to_dict(self) -> dict[str, Any]:
        return {
            "tx_id": self.tx_id,
            "sender": self.sender,
            "receiver": self.receiver,
            "amount": self.amount,
            "nonce": self.nonce,
            "timestamp": self.timestamp,
            "public_key": self.public_key,
            "parents": self.parents,
            "signature": self.signature,
            "anti_spam_nonce": self.anti_spam_nonce,
            "anti_spam_hash": self.anti_spam_hash,
            "status": self.status,
            "weight": self.weight,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "TransactionVertex":
        return cls(
            tx_id=data.get("tx_id", ""),
            sender=data["sender"],
            receiver=data["receiver"],
            amount=data["amount"],
            nonce=data["nonce"],
            timestamp=data["timestamp"],
            public_key=data["public_key"],
            parents=list(data.get("parents", [])),
            signature=data.get("signature", ""),
            anti_spam_nonce=data.get("anti_spam_nonce", 0),
            anti_spam_hash=data.get("anti_spam_hash", ""),
            status=data.get("status", TX_STATUS_PENDING),
            weight=data.get("weight", 1),
        )