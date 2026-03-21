# app/ledger/validator.py

from __future__ import annotations

from dataclasses import dataclass

from app.config import ANTI_SPAM_DIFFICULTY, MAX_PARENTS
from app.crypto.hashing import sha256_hex
from app.ledger.dag import DAG
from app.ledger.state import LedgerState
from app.ledger.transaction import TransactionVertex


@dataclass(slots=True)
class ValidationResult:
    ok: bool
    code: str
    reason: str


class Validator:
    def validate_structure(self, tx: TransactionVertex) -> ValidationResult:
        if not tx.sender:
            return ValidationResult(False, "bad_sender", "sender is empty")
        if not tx.receiver:
            return ValidationResult(False, "bad_receiver", "receiver is empty")
        if tx.amount <= 0:
            return ValidationResult(False, "bad_amount", "amount must be positive")
        if tx.nonce <= 0:
            return ValidationResult(False, "bad_nonce", "nonce must be positive")
        if len(tx.parents) > MAX_PARENTS:
            return ValidationResult(False, "bad_parents", "too many parents")
        return ValidationResult(True, "ok", "structure valid")

    def validate_parents(self, tx: TransactionVertex, dag: DAG) -> ValidationResult:
        if not dag.vertices and tx.parents:
            return ValidationResult(False, "bad_parents", "genesis-like tx must not have parents")

        if dag.vertices and len(tx.parents) not in (1, 2):
            return ValidationResult(
                False,
                "bad_parents",
                "transaction must reference 1 or 2 parents",
            )

        for parent_id in tx.parents:
            parent = dag.get_transaction(parent_id)
            if parent is None:
                return ValidationResult(False, "missing_parent", f"parent not found: {parent_id}")
            if parent.status == "rejected":
                return ValidationResult(False, "bad_parent", f"parent rejected: {parent_id}")

        return ValidationResult(True, "ok", "parents valid")

    def validate_signature(self, tx: TransactionVertex) -> ValidationResult:
        from app.crypto.wallet import verify_signature
 
        derived_address = sha256_hex(tx.public_key.encode())[:40]
        if derived_address != tx.sender:
            return ValidationResult(False, "bad_signature", "sender does not match public key")
 
        if not tx.signature:
            return ValidationResult(False, "bad_signature", "missing signature")
 
        valid = verify_signature(tx.public_key, tx.signing_payload(), tx.signature)
        if not valid:
            return ValidationResult(False, "bad_signature", "signature verification failed")
 
        return ValidationResult(True, "ok", "signature valid")

    def validate_anti_spam(self, tx: TransactionVertex) -> ValidationResult:
        expected_hash = tx.compute_anti_spam_hash()

        if tx.anti_spam_hash != expected_hash:
            return ValidationResult(False, "bad_pow", "anti spam hash mismatch")

        if not tx.anti_spam_hash.startswith("0" * ANTI_SPAM_DIFFICULTY):
            return ValidationResult(False, "bad_pow", "anti spam difficulty not satisfied")

        return ValidationResult(True, "ok", "anti spam valid")

    def validate_state(self, tx: TransactionVertex, state: LedgerState) -> ValidationResult:
        ok, reason = state.can_apply(tx)
        if not ok:
            return ValidationResult(False, "bad_state", reason)
        return ValidationResult(True, "ok", "state valid")

    def validate_duplicate(self, tx: TransactionVertex, dag: DAG) -> ValidationResult:
        if dag.has_transaction(tx.tx_id):
            return ValidationResult(False, "duplicate", "transaction already exists")
        return ValidationResult(True, "ok", "not duplicate")

    def validate_full(self, tx: TransactionVertex, dag: DAG, state: LedgerState) -> ValidationResult:
        checks = [
            self.validate_structure(tx),
            self.validate_duplicate(tx, dag),
            self.validate_parents(tx, dag),
            self.validate_signature(tx),
            self.validate_anti_spam(tx),
            self.validate_state(tx, state),
        ]

        for result in checks:
            if not result.ok:
                return result

        return ValidationResult(True, "ok", "transaction valid")