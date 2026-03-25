// ledger/src/transaction.rs

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TxStatus {
    Pending,
    Confirmed,
    Rejected,
    Conflict,
}

impl TxStatus {
    pub fn as_str(&self) -> &str {
        match self {
            TxStatus::Pending => "pending",
            TxStatus::Confirmed => "confirmed",
            TxStatus::Rejected => "rejected",
            TxStatus::Conflict => "conflict",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionVertex {
    pub sender: String,
    pub receiver: String,
    pub amount: u64,
    pub nonce: u64,
    pub timestamp: u64,
    pub public_key: String,
    pub parents: Vec<String>,
    pub signature: String,
    pub anti_spam_nonce: u64,
    pub anti_spam_hash: String,
    pub ephemeral_pubkey: String,
    pub status: TxStatus,
    pub weight: u64,
    pub tx_id: String,
    pub commitment: Option<String>,    
    pub balance_proof: Option<String>,
    pub stem_ttl: u8,
}

impl TransactionVertex {
    pub fn new(
        sender: String,
        receiver: String,
        amount: u64,
        nonce: u64,
        timestamp: u64,
        public_key: String,
        parents: Vec<String>,
    ) -> Self {
        TransactionVertex {
            sender,
            receiver,
            amount,
            nonce,
            timestamp,
            public_key,
            parents,
            signature: String::new(),
            anti_spam_nonce: 0,
            anti_spam_hash: String::new(),
            ephemeral_pubkey: String::new(),
            status: TxStatus::Pending,
            weight: 1,
            tx_id: String::new(),
            commitment: None,
            balance_proof: None,
            stem_ttl: 0,
        }
    }

    pub fn signing_payload(&self) -> Vec<u8> {
        let mut map = BTreeMap::new();
        map.insert("sender", serde_json::Value::String(self.sender.clone()));
        map.insert("receiver", serde_json::Value::String(self.receiver.clone()));
        map.insert("amount", serde_json::Value::Number(self.amount.into()));
        map.insert("nonce", serde_json::Value::Number(self.nonce.into()));
        map.insert("timestamp", serde_json::Value::Number(self.timestamp.into()));
        map.insert("public_key", serde_json::Value::String(self.public_key.clone()));
        map.insert("parents", serde_json::Value::Array(
            self.parents.iter().map(|p| serde_json::Value::String(p.clone())).collect()
        ));
        map.insert("anti_spam_nonce", serde_json::Value::Number(self.anti_spam_nonce.into()));
        map.insert("ephemeral_pubkey", serde_json::Value::String(self.ephemeral_pubkey.clone()));

        serde_json::to_string(&map).unwrap().into_bytes()
    }

    pub fn compute_anti_spam_hash(&self) -> String {
        let payload = self.signing_payload();
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        hex::encode(hasher.finalize())
    }

    pub fn compute_tx_id(&self) -> String {
        let mut map = BTreeMap::new();
        map.insert("sender", serde_json::Value::String(self.sender.clone()));
        map.insert("receiver", serde_json::Value::String(self.receiver.clone()));
        map.insert("amount", serde_json::Value::Number(self.amount.into()));
        map.insert("nonce", serde_json::Value::Number(self.nonce.into()));
        map.insert("timestamp", serde_json::Value::Number(self.timestamp.into()));
        map.insert("public_key", serde_json::Value::String(self.public_key.clone()));
        map.insert("parents", serde_json::Value::Array(
            self.parents.iter().map(|p| serde_json::Value::String(p.clone())).collect()
        ));
        map.insert("anti_spam_nonce", serde_json::Value::Number(self.anti_spam_nonce.into()));
        map.insert("anti_spam_hash", serde_json::Value::String(self.anti_spam_hash.clone()));
        map.insert("signature", serde_json::Value::String(self.signature.clone()));
        map.insert("ephemeral_pubkey", serde_json::Value::String(self.ephemeral_pubkey.clone()));

        let json = serde_json::to_string(&map).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        hex::encode(hasher.finalize())
    }

    pub fn finalize(&mut self) {
        self.anti_spam_hash = self.compute_anti_spam_hash();
        self.tx_id = self.compute_tx_id();
    }

    pub fn mine_anti_spam(&mut self, difficulty: usize) {
        let prefix = "0".repeat(difficulty);
        let mut nonce = 0u64;
        loop {
            self.anti_spam_nonce = nonce;
            let hash = self.compute_anti_spam_hash();
            if hash.starts_with(&prefix) {
                self.anti_spam_hash = hash;
                return;
            }
            nonce += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx() -> TransactionVertex {
        TransactionVertex::new(
            "sender_address".to_string(),
            "receiver_address".to_string(),
            100,
            1,
            1000,
            "public_key_hex".to_string(),
            vec![],
        )
    }

    #[test]
    fn test_new_transaction_has_pending_status() {
        let tx = make_tx();
        assert_eq!(tx.status, TxStatus::Pending);
        assert_eq!(tx.weight, 1);
        assert!(tx.tx_id.is_empty());
    }

    #[test]
    fn test_signing_payload_is_deterministic() {
        let tx = make_tx();
        let p1 = tx.signing_payload();
        let p2 = tx.signing_payload();
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_anti_spam_hash_is_deterministic() {
        let tx = make_tx();
        let h1 = tx.compute_anti_spam_hash();
        let h2 = tx.compute_anti_spam_hash();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_finalize_sets_tx_id() {
        let mut tx = make_tx();
        assert!(tx.tx_id.is_empty());
        tx.anti_spam_hash = tx.compute_anti_spam_hash();
        tx.finalize();
        assert!(!tx.tx_id.is_empty());
        assert_eq!(tx.tx_id.len(), 64);
    }

    #[test]
    fn test_mine_anti_spam() {
        let mut tx = make_tx();
        tx.mine_anti_spam(2);
        assert!(tx.anti_spam_hash.starts_with("00"));
    }

    #[test]
    fn test_different_amounts_give_different_tx_ids() {
        let mut tx1 = make_tx();
        let mut tx2 = TransactionVertex::new(
            "sender_address".to_string(),
            "receiver_address".to_string(),
            200, 
            1,
            1000,
            "public_key_hex".to_string(),
            vec![],
        );
        tx1.finalize();
        tx2.finalize();
        assert_ne!(tx1.tx_id, tx2.tx_id);
    }

    #[test]
    fn test_status_strings() {
        assert_eq!(TxStatus::Pending.as_str(), "pending");
        assert_eq!(TxStatus::Confirmed.as_str(), "confirmed");
        assert_eq!(TxStatus::Rejected.as_str(), "rejected");
        assert_eq!(TxStatus::Conflict.as_str(), "conflict");
    }
}