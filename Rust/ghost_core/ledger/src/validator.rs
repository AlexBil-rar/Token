// ledger/src/validator.rs

use crate::dag::DAG;
use crate::state::LedgerState;
use crate::transaction::TransactionVertex;

const ANTI_SPAM_DIFFICULTY: usize = 3;
const MAX_PARENTS: usize = 2;

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub ok: bool,
    pub code: String,
    pub reason: String,
}

impl ValidationResult {
    pub fn ok(code: &str, reason: &str) -> Self {
        ValidationResult { ok: true, code: code.to_string(), reason: reason.to_string() }
    }

    pub fn err(code: &str, reason: &str) -> Self {
        ValidationResult { ok: false, code: code.to_string(), reason: reason.to_string() }
    }
}

pub struct Validator;

impl Validator {
    pub fn new() -> Self { Validator }

    pub fn validate_structure(&self, tx: &TransactionVertex) -> ValidationResult {
        if tx.sender.is_empty() {
            return ValidationResult::err("bad_sender", "sender is empty");
        }
        if tx.receiver.is_empty() {
            return ValidationResult::err("bad_receiver", "receiver is empty");
        }
        if tx.amount == 0 {
            return ValidationResult::err("bad_amount", "amount must be positive");
        }
        if tx.nonce == 0 {
            return ValidationResult::err("bad_nonce", "nonce must be positive");
        }
        if tx.parents.len() > MAX_PARENTS {
            return ValidationResult::err("bad_parents", "too many parents");
        }
        ValidationResult::ok("ok", "structure valid")
    }

    pub fn validate_parents(&self, tx: &TransactionVertex, dag: &DAG) -> ValidationResult {
        if dag.vertices.is_empty() && !tx.parents.is_empty() {
            return ValidationResult::err("bad_parents", "genesis tx must not have parents");
        }

        if !dag.vertices.is_empty() && !(1..=2).contains(&tx.parents.len()) {
            return ValidationResult::err("bad_parents", "tx must reference 1 or 2 parents");
        }

        for parent_id in &tx.parents {
            match dag.get_transaction(parent_id) {
                None => return ValidationResult::err("missing_parent", &format!("parent not found: {}", parent_id)),
                Some(p) if p.status == crate::transaction::TxStatus::Rejected => {
                    return ValidationResult::err("bad_parent", &format!("parent rejected: {}", parent_id));
                }
                _ => {}
            }
        }

        ValidationResult::ok("ok", "parents valid")
    }

    pub fn validate_signature(&self, tx: &TransactionVertex) -> ValidationResult {
        use sha2::{Sha256, Digest};

        let mut hasher = Sha256::new();
        hasher.update(tx.public_key.as_bytes());
        let hash = hex::encode(hasher.finalize());
        let derived_address = &hash[..40];

        if derived_address != tx.sender {
            return ValidationResult::err("bad_signature", "sender does not match public key");
        }

        if tx.signature.is_empty() {
            return ValidationResult::err("bad_signature", "missing signature");
        }

        let pub_bytes = match hex::decode(&tx.public_key) {
            Ok(b) => b,
            Err(_) => return ValidationResult::err("bad_signature", "invalid public key hex"),
        };
        let sig_bytes = match hex::decode(&tx.signature) {
            Ok(b) => b,
            Err(_) => return ValidationResult::err("bad_signature", "invalid signature hex"),
        };

        if pub_bytes.len() != 32 || sig_bytes.len() != 64 {
            return ValidationResult::err("bad_signature", "invalid key or signature length");
        }

        let mut pub_arr = [0u8; 32];
        pub_arr.copy_from_slice(&pub_bytes);
        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);

        use ed25519_dalek::{VerifyingKey, Signature, Verifier};
        let verifying_key = match VerifyingKey::from_bytes(&pub_arr) {
            Ok(k) => k,
            Err(_) => return ValidationResult::err("bad_signature", "invalid public key"),
        };
        let signature = Signature::from_bytes(&sig_arr);
        let payload = tx.signing_payload();

        match verifying_key.verify(&payload, &signature) {
            Ok(_) => ValidationResult::ok("ok", "signature valid"),
            Err(_) => ValidationResult::err("bad_signature", "signature verification failed"),
        }
    }

    pub fn validate_anti_spam(&self, tx: &TransactionVertex) -> ValidationResult {
        let expected = tx.compute_anti_spam_hash();

        if tx.anti_spam_hash != expected {
            return ValidationResult::err("bad_pow", "anti spam hash mismatch");
        }

        let prefix = "0".repeat(ANTI_SPAM_DIFFICULTY);
        if !tx.anti_spam_hash.starts_with(&prefix) {
            return ValidationResult::err("bad_pow", "anti spam difficulty not satisfied");
        }

        ValidationResult::ok("ok", "anti spam valid")
    }

    pub fn validate_anti_spam_with_difficulty(
        &self,
        tx: &TransactionVertex,
        difficulty: usize,
    ) -> ValidationResult {
        let expected = tx.compute_anti_spam_hash();
        if tx.anti_spam_hash != expected {
            return ValidationResult::err("bad_pow", "anti spam hash mismatch");
        }
        let prefix = "0".repeat(difficulty);
        if !tx.anti_spam_hash.starts_with(&prefix) {
            return ValidationResult::err("bad_pow", &format!(
                "difficulty {} not satisfied", difficulty
            ));
        }
        ValidationResult::ok("ok", "anti spam valid")
    }

    pub fn validate_duplicate(&self, tx: &TransactionVertex, dag: &DAG) -> ValidationResult {
        if dag.has_transaction(&tx.tx_id) {
            return ValidationResult::err("duplicate", "transaction already exists");
        }
        ValidationResult::ok("ok", "not duplicate")
    }

    pub fn validate_state(&self, tx: &TransactionVertex, state: &mut LedgerState) -> ValidationResult {
        match state.can_apply(tx) {
            Ok(_) => ValidationResult::ok("ok", "state valid"),
            Err(reason) => ValidationResult::err("bad_state", &reason),
        }
    }

    pub fn validate_full(
        &self,
        tx: &TransactionVertex,
        dag: &DAG,
        state: &mut LedgerState,
    ) -> ValidationResult {
        let checks: Vec<ValidationResult> = vec![
            self.validate_structure(tx),
            self.validate_duplicate(tx, dag),
            self.validate_parents(tx, dag),
            self.validate_signature(tx),
            self.validate_anti_spam(tx),
            self.validate_state(tx, state),
        ];

        for result in checks {
            if !result.ok {
                return result;
            }
        }

        ValidationResult::ok("ok", "transaction valid")
    }

    pub fn validate_full_with_difficulty(
        &self,
        tx: &TransactionVertex,
        dag: &DAG,
        state: &mut LedgerState,
        difficulty: usize,
    ) -> ValidationResult {
        let checks: Vec<ValidationResult> = vec![
            self.validate_structure(tx),
            self.validate_duplicate(tx, dag),
            self.validate_parents(tx, dag),
            self.validate_signature(tx),
            self.validate_anti_spam_with_difficulty(tx, difficulty),
            self.validate_state(tx, state),
        ];
        for result in checks {
            if !result.ok { return result; }
        }
        ValidationResult::ok("ok", "transaction valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::TransactionVertex;
    use crate::dag::DAG;
    use crate::state::LedgerState;

    fn make_tx(sender: &str, amount: u64, nonce: u64) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            sender.to_string(), "receiver".to_string(),
            amount, nonce, 1000, "pk".to_string(), vec![],
        );
        tx.tx_id = format!("tx_{}_{}", sender, nonce);
        tx
    }

    #[test]
    fn test_structure_valid() {
        let v = Validator::new();
        let tx = make_tx("alice", 100, 1);
        assert!(v.validate_structure(&tx).ok);
    }

    #[test]
    fn test_structure_empty_sender() {
        let v = Validator::new();
        let mut tx = make_tx("alice", 100, 1);
        tx.sender = String::new();
        let result = v.validate_structure(&tx);
        assert!(!result.ok);
        assert_eq!(result.code, "bad_sender");
    }

    #[test]
    fn test_structure_zero_amount() {
        let v = Validator::new();
        let mut tx = make_tx("alice", 100, 1);
        tx.amount = 0;
        let result = v.validate_structure(&tx);
        assert!(!result.ok);
        assert_eq!(result.code, "bad_amount");
    }

    #[test]
    fn test_structure_too_many_parents() {
        let v = Validator::new();
        let mut tx = make_tx("alice", 100, 1);
        tx.parents = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let result = v.validate_structure(&tx);
        assert!(!result.ok);
        assert_eq!(result.code, "bad_parents");
    }

    #[test]
    fn test_duplicate_detected() {
        let v = Validator::new();
        let mut dag = DAG::new();
        let tx = make_tx("alice", 100, 1);
        dag.add_transaction(tx.clone()).unwrap();
        let result = v.validate_duplicate(&tx, &dag);
        assert!(!result.ok);
        assert_eq!(result.code, "duplicate");
    }

    #[test]
    fn test_not_duplicate() {
        let v = Validator::new();
        let dag = DAG::new();
        let tx = make_tx("alice", 100, 1);
        assert!(v.validate_duplicate(&tx, &dag).ok);
    }

    #[test]
    fn test_state_insufficient_balance() {
        let v = Validator::new();
        let mut state = LedgerState::new();
        state.credit("alice", 5);
        let tx = make_tx("alice", 100, 1);
        assert!(!v.validate_state(&tx, &mut state).ok);
    }

    #[test]
    fn test_state_sufficient_balance() {
        let v = Validator::new();
        let mut state = LedgerState::new();
        state.credit("alice", 1000);
        let tx = make_tx("alice", 100, 1);
        assert!(v.validate_state(&tx, &mut state).ok);
    }

    #[test]
    fn test_anti_spam_wrong_hash() {
        let v = Validator::new();
        let mut tx = make_tx("alice", 100, 1);
        tx.anti_spam_hash = "badhash".to_string();
        let result = v.validate_anti_spam(&tx);
        assert!(!result.ok);
        assert_eq!(result.code, "bad_pow");
    }
}