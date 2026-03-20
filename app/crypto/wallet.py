# app/crypto/wallet.py

from __future__ import annotations

from dataclasses import dataclass

import nacl.signing
import nacl.encoding

from app.crypto.hashing import sha256_hex


@dataclass(slots=True)
class Wallet:
    private_key: str
    public_key: str
    address: str       

    @classmethod
    def generate(cls) -> "Wallet":
        signing_key = nacl.signing.SigningKey.generate()
        verify_key = signing_key.verify_key

        private_key = signing_key.encode(nacl.encoding.HexEncoder).decode()
        public_key = verify_key.encode(nacl.encoding.HexEncoder).decode()
        address = sha256_hex(public_key.encode())[:40]

        return cls(
            private_key=private_key,
            public_key=public_key,
            address=address,
        )

    def sign(self, payload: bytes) -> str:
        signing_key = nacl.signing.SigningKey(
            self.private_key.encode(),
            encoder=nacl.encoding.HexEncoder,
        )
        signed = signing_key.sign(payload)
        return signed.signature.hex()


def verify_signature(public_key_hex: str, payload: bytes, signature_hex: str) -> bool:
    try:
        verify_key = nacl.signing.VerifyKey(
            public_key_hex.encode(),
            encoder=nacl.encoding.HexEncoder,
        )
        signature = bytes.fromhex(signature_hex)
        verify_key.verify(payload, signature)
        return True
    except Exception:
        return False