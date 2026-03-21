# app/crypto/stealth.py

from __future__ import annotations

import hashlib
import os
from dataclasses import dataclass

from cryptography.hazmat.primitives.asymmetric.x25519 import (
    X25519PrivateKey,
    X25519PublicKey,
)
from cryptography.hazmat.primitives.serialization import (
    Encoding,
    PublicFormat,
    PrivateFormat,
    NoEncryption,
)

from app.crypto.hashing import sha256_hex


@dataclass
class StealthKeys:
    spend_private: bytes  # 32 байта
    spend_public: bytes   # 32 байта

    @classmethod
    def generate(cls) -> "StealthKeys":
        private = X25519PrivateKey.generate()
        public = private.public_key()
        return cls(
            spend_private=private.private_bytes(
                Encoding.Raw, PrivateFormat.Raw, NoEncryption()
            ),
            spend_public=public.public_bytes(Encoding.Raw, PublicFormat.Raw),
        )

    def spend_public_hex(self) -> str:
        return self.spend_public.hex()

    def spend_private_hex(self) -> str:
        return self.spend_private.hex()


@dataclass
class StealthPayment:
    stealth_address: str    
    ephemeral_pubkey: str


def generate_stealth_payment(recipient_spend_pubkey_hex: str) -> StealthPayment:
    ephemeral_private = X25519PrivateKey.generate()
    ephemeral_public = ephemeral_private.public_key()

    spend_pubkey_bytes = bytes.fromhex(recipient_spend_pubkey_hex)
    recipient_pubkey = X25519PublicKey.from_public_bytes(spend_pubkey_bytes)

    shared_secret = ephemeral_private.exchange(recipient_pubkey)

    stealth_address = _derive_stealth_address(shared_secret, spend_pubkey_bytes)

    ephemeral_pubkey_hex = ephemeral_public.public_bytes(
        Encoding.Raw, PublicFormat.Raw
    ).hex()

    return StealthPayment(
        stealth_address=stealth_address,
        ephemeral_pubkey=ephemeral_pubkey_hex,
    )


def scan_for_payment(
    spend_private_hex: str,
    spend_public_hex: str,
    ephemeral_pubkey_hex: str,
) -> str | None:
    try:
        spend_private_bytes = bytes.fromhex(spend_private_hex)
        spend_public_bytes = bytes.fromhex(spend_public_hex)
        ephemeral_pubkey_bytes = bytes.fromhex(ephemeral_pubkey_hex)

        spend_private = X25519PrivateKey.from_private_bytes(spend_private_bytes)
        ephemeral_pubkey = X25519PublicKey.from_public_bytes(ephemeral_pubkey_bytes)

        shared_secret = spend_private.exchange(ephemeral_pubkey)

        stealth_address = _derive_stealth_address(shared_secret, spend_public_bytes)

        return stealth_address

    except Exception:
        return None


def _derive_stealth_address(shared_secret: bytes, spend_pubkey: bytes) -> str:
    h = hashlib.sha256(shared_secret + spend_pubkey).digest()
    return h.hex()[:40]