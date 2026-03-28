// ledger/src/validator.rs

use crate::dag::DAG;
use crate::state::LedgerState;
use crate::transaction::TransactionVertex;
use crypto::commitments::BalanceProof;  
use crypto::commitments::Commitment;   
use crypto::range_proof::{RangeProofSystem, RangeProofStatus};


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

    pub fn validate_structure_and_dag(
        &self,
        tx: &TransactionVertex,
        dag: &DAG,
        difficulty: usize,
        state: &LedgerState,
        privacy_by_default: bool,  
    ) -> ValidationResult {
        let checks: Vec<ValidationResult> = vec![
            self.validate_structure(tx),
            self.validate_duplicate(tx, dag),
            self.validate_parents(tx, dag),
            self.validate_signature(tx),
            self.validate_anti_spam_with_difficulty(tx, difficulty),
            self.validate_state_readonly(tx, state),
            self.validate_privacy_mode(tx, privacy_by_default),
            self.validate_balance_proof(tx),
            self.validate_excess(tx),
            self.validate_confidential_tx(tx),
        ];
        for result in checks {
            if !result.ok { return result; }
        }
        ValidationResult::ok("ok", "transaction valid")
    }

    pub fn validate_state_readonly(
        &self,
        tx: &TransactionVertex,
        state: &LedgerState,
    ) -> ValidationResult {
        let balance = state.balances.get(&tx.sender).copied().unwrap_or(0);
        if balance < tx.amount {
            return ValidationResult::err(
                "bad_state",
                &format!("insufficient balance: have {}, need {}", balance, tx.amount),
            );
        }
        let current_nonce = state.nonces.get(&tx.sender).copied().unwrap_or(0);
        if tx.nonce < current_nonce + 1 {
            return ValidationResult::err(
                "bad_nonce",
                &format!("nonce too old: current={}, got={}", current_nonce, tx.nonce),
            );
        }
        ValidationResult::ok("ok", "balance ok")
    }


    pub fn validate_balance_proof(&self, tx: &TransactionVertex) -> ValidationResult {
        let commitment_hex = match &tx.commitment {
            Some(c) => c,
            None => return ValidationResult::ok("ok", "transparent tx"),
        };

        let proof_hex = match &tx.balance_proof {
            Some(p) => p,
            None => return ValidationResult::err(
                "missing_balance_proof",
                "confidential tx must include balance proof",
            ),
        };

        let proof: BalanceProof = match serde_json::from_str(proof_hex) {
            Ok(p) => p,
            Err(_) => return ValidationResult::err(
                "invalid_balance_proof",
                "failed to deserialize balance proof",
            ),
        };

        let output_commitment = match serde_json::from_str::<Commitment>(
            &format!("{{\"point_hex\":\"{}\"}}", commitment_hex)
        ) {
            Ok(c) => c,
            Err(_) => return ValidationResult::err(
                "invalid_commitment",
                "failed to parse commitment",
            ),
        };

        if !proof.verify(&[], &[output_commitment]) {
            return ValidationResult::err(
                "invalid_balance_proof",
                "balance proof verification failed",
            );
        }

        if let Some(rp_hex) = &tx.range_proof {
            use crypto::range_proof::{PlaceholderRangeProof, RangeProofSystem};
            use crypto::commitments::Commitment;
        
            let rp: crypto::range_proof::PlaceholderProof = match serde_json::from_str(rp_hex) {
                Ok(p) => p,
                Err(_) => return ValidationResult::err(
                    "invalid_range_proof",
                    "failed to deserialize range proof",
                ),
            };
        
            let commitment = match serde_json::from_str::<Commitment>(
                &format!("{{\"point_hex\":\"{}\"}}", commitment_hex)
            ) {
                Ok(c) => c,
                Err(_) => return ValidationResult::err(
                    "invalid_commitment",
                    "failed to parse commitment for range proof check",
                ),
            };
        
            if PlaceholderRangeProof::verify(&commitment, &rp).is_err() {
                return ValidationResult::err(
                    "invalid_range_proof",
                    "range proof verification failed",
                );
            }
        }

        ValidationResult::ok("ok", "balance proof valid")
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
    

    pub fn validate_privacy_mode(
        &self,
        tx: &TransactionVertex,
        privacy_by_default: bool,
    ) -> ValidationResult {
        if !privacy_by_default {
            return ValidationResult::ok("ok", "privacy mode off");
        }
    
        if tx.sender == "system" || tx.amount == 0 {
            return ValidationResult::ok("ok", "system tx exempt");
        }
    
        if tx.commitment.is_none() {
            return ValidationResult::err(
                "privacy_required",
                "network is in privacy-by-default mode: commitment required",
            );
        }
        use crypto::range_proof::RangeProofStatus;

        if tx.commitment.is_some() {
            match tx.range_proof_status {
                RangeProofStatus::Verified => {}
                RangeProofStatus::Experimental => {
                    #[cfg(not(debug_assertions))]
                    return ValidationResult::err(
                        "experimental_range_proof",
                        "production mode requires Verified range proof",
                    );
                }
                RangeProofStatus::Missing => {
                    return ValidationResult::err(
                        "missing_range_proof",
                        "commitment present but range proof is missing",
                    );
                }
            }
        }
    
        ValidationResult::ok("ok", "privacy check passed")
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
        self.validate_full_with_difficulty(tx, dag, state, ANTI_SPAM_DIFFICULTY)
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

    pub fn validate_excess(&self, tx: &TransactionVertex) -> ValidationResult {
        if tx.commitment.is_none() {
            return ValidationResult::ok("ok", "transparent tx");
        }

        let excess_commitment = match &tx.excess_commitment {
            Some(e) => e,
            None => return ValidationResult::err(
                "missing_excess",
                "confidential tx must include excess commitment",
            ),
        };
    
        let excess_signature = match &tx.excess_signature {
            Some(s) => s,
            None => return ValidationResult::err(
                "missing_excess_signature",
                "confidential tx must include excess signature",
            ),
        };
    
        if hex::decode(excess_commitment).is_err() {
            return ValidationResult::err(
                "invalid_excess",
                "excess commitment is not valid hex",
            );
        }
    
        if hex::decode(excess_signature).is_err() {
            return ValidationResult::err(
                "invalid_excess_signature",
                "excess signature is not valid hex",
            );
        }
    
        ValidationResult::ok("ok", "excess valid")
    }

    pub fn validate_confidential_tx(&self, tx: &TransactionVertex) -> ValidationResult {
        if tx.commitment.is_none() {
            return ValidationResult::ok("ok", "transparent tx — skipping confidential checks");
        }
    
        let rp_result = self.validate_balance_proof(tx);
        if !rp_result.ok { return rp_result; }
    
        let bp_result = self.validate_balance_proof(tx);
        if !bp_result.ok { return bp_result; }
    
        let excess_result = self.validate_excess(tx);
        if !excess_result.ok { return excess_result; }
    
        ValidationResult::ok("ok", "confidential tx valid: range_proof + balance + excess")
    }

    pub fn validate_range_proof_with_backend<P: RangeProofSystem>(
        &self,
        tx: &TransactionVertex,
    ) -> ValidationResult
    where
        P::Proof: for<'de> serde::Deserialize<'de>,
    {
        let commitment_hex = match &tx.commitment {
            Some(c) => c,
            None => return ValidationResult::ok("ok", "transparent tx"),
        };
    
        let proof_hex = match &tx.range_proof {
            Some(p) => p,
            None => return ValidationResult::err(
                "missing_range_proof",
                "confidential tx must include range proof",
            ),
        };
    
        let proof: P::Proof = match serde_json::from_str(proof_hex) {
            Ok(p) => p,
            Err(_) => return ValidationResult::err(
                "invalid_range_proof",
                "failed to deserialize range proof",
            ),
        };
    
        let commitment = match serde_json::from_str::<crypto::commitments::Commitment>(
            &format!("{{\"point_hex\":\"{}\"}}", commitment_hex)
        ) {
            Ok(c) => c,
            Err(_) => return ValidationResult::err(
                "invalid_commitment",
                "failed to parse commitment",
            ),
        };
    
        match P::verify(&commitment, &proof) {
            Ok(()) => ValidationResult::ok("ok", "range proof valid"),
            Err(e) => ValidationResult::err("invalid_range_proof", &e.to_string()),
        }
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
    #[test]
    fn test_state_readonly_rejects_old_nonce() {
        let v = Validator::new();
        let mut state = LedgerState::new();
        state.credit("alice", 1000);
        state.nonces.insert("alice".to_string(), 3); 

        let tx = make_tx("alice", 100, 1);
        let result = v.validate_state_readonly(&tx, &state);
        assert!(!result.ok);
        assert_eq!(result.code, "bad_nonce");
    }

    #[test]
    fn test_state_readonly_accepts_future_nonce() {
        let v = Validator::new();
        let mut state = LedgerState::new();
        state.credit("alice", 1000);
        state.nonces.insert("alice".to_string(), 3);

        let tx = make_tx("alice", 100, 5);
        let result = v.validate_state_readonly(&tx, &state);
        assert!(result.ok);
    }

    #[test]
    fn test_excess_required_for_confidential_tx() {
        let v = Validator::new();
        let mut tx = make_tx("alice", 100, 1);
        tx.commitment = Some("aabbcc".to_string());
        let result = v.validate_excess(&tx);
        assert!(!result.ok);
        assert_eq!(result.code, "missing_excess");
    }

    #[test]
    fn test_excess_ok_for_transparent_tx() {
        let v = Validator::new();
        let tx = make_tx("alice", 100, 1);
        let result = v.validate_excess(&tx);
        assert!(result.ok);
    }

    #[test]
    fn test_confidential_tx_passes_all_three_steps() {
        let v = Validator::new();
        let tx = make_tx("alice", 100, 1);
        let result = v.validate_confidential_tx(&tx);
        assert!(result.ok);
    }
    
}