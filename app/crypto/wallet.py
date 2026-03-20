# app/crypto/wallet.py

from __future__ import annotations

import secrets
from dataclasses import dataclass

from app.crypto.hashing import sha256_hex


@dataclass(slots=True)
class Wallet:
    private_key: str
    public_key: str
    address: str

    @classmethod
    def generate(cls) -> "Wallet":
        private_key = secrets.token_hex(32)
        public_key = sha256_hex(private_key.encode())
        address = sha256_hex(public_key.encode())[:40]
        return cls(
            private_key=private_key,
            public_key=public_key,
            address=address,
        )

    def sign(self, payload: bytes) -> str:
        return sha256_hex(payload + self.private_key.encode())