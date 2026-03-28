// ghost-wire/src/lib.rs

use serde::{Deserialize, Serialize};
use ledger::transaction::TransactionVertex;
use ghost_params::wire::{WIRE_MAGIC, WIRE_VERSION, MAX_WIRE_PAYLOAD};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireTransaction {
    pub version: u8,
    pub tx_id: String,
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
    pub commitment: Option<String>,
    pub balance_proof: Option<String>,
    pub range_proof: Option<String>,
    pub excess_commitment: Option<String>,
    pub excess_signature: Option<String>,
    pub stem_ttl: u8,
}

impl From<&TransactionVertex> for WireTransaction {
    fn from(tx: &TransactionVertex) -> Self {
        WireTransaction {
            version: WIRE_VERSION,
            tx_id: tx.tx_id.clone(),
            sender: tx.sender.clone(),
            receiver: tx.receiver.clone(),
            amount: tx.amount,
            nonce: tx.nonce,
            timestamp: tx.timestamp,
            public_key: tx.public_key.clone(),
            parents: tx.parents.clone(),
            signature: tx.signature.clone(),
            anti_spam_nonce: tx.anti_spam_nonce,
            anti_spam_hash: tx.anti_spam_hash.clone(),
            commitment: tx.commitment.clone(),
            balance_proof: tx.balance_proof.clone(),
            range_proof: tx.range_proof.clone(),
            excess_commitment: tx.excess_commitment.clone(),
            excess_signature: tx.excess_signature.clone(),
            stem_ttl: tx.stem_ttl,
        }
    }
}

impl From<WireTransaction> for TransactionVertex {
    fn from(w: WireTransaction) -> Self {
        let mut tx = TransactionVertex::new(
            w.sender,
            w.receiver,
            w.amount,
            w.nonce,
            w.timestamp,
            w.public_key,
            w.parents,
        );
        tx.tx_id = w.tx_id;
        tx.signature = w.signature;
        tx.anti_spam_nonce = w.anti_spam_nonce;
        tx.anti_spam_hash = w.anti_spam_hash;
        tx.commitment = w.commitment;
        tx.balance_proof = w.balance_proof;
        tx.range_proof = w.range_proof;
        tx.excess_commitment = w.excess_commitment;
        tx.excess_signature = w.excess_signature;
        tx.stem_ttl = w.stem_ttl;
        tx
    }
}

#[derive(Debug)]
pub enum WireError {
    TooLarge(usize),
    EncodeFailed(String),
    DecodeFailed(String),
    BadMagic,
    BadVersion(u8),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::TooLarge(n) => write!(f, "payload too large: {} bytes", n),
            WireError::EncodeFailed(s) => write!(f, "encode failed: {}", s),
            WireError::DecodeFailed(s) => write!(f, "decode failed: {}", s),
            WireError::BadMagic => write!(f, "bad magic bytes"),
            WireError::BadVersion(v) => write!(f, "unsupported version: {}", v),
        }
    }
}

pub fn encode(tx: &TransactionVertex) -> Result<Vec<u8>, WireError> {
    let wire = WireTransaction::from(tx);
    let payload = bincode::serialize(&wire)
        .map_err(|e| WireError::EncodeFailed(e.to_string()))?;

    if payload.len() > MAX_WIRE_PAYLOAD {
        return Err(WireError::TooLarge(payload.len()));
    }

    let mut buf = Vec::with_capacity(4 + 1 + payload.len());
    buf.extend_from_slice(&WIRE_MAGIC);
    buf.push(WIRE_VERSION);
    buf.extend_from_slice(&payload);

    Ok(buf)
}

pub fn decode(buf: &[u8]) -> Result<TransactionVertex, WireError> {
    if buf.len() < 5 {
        return Err(WireError::DecodeFailed("too short".into()));
    }

    if &buf[..4] != &WIRE_MAGIC {
        return Err(WireError::BadMagic);
    }

    let version = buf[4];
    if version != WIRE_VERSION {
        return Err(WireError::BadVersion(version));
    }

    let payload = &buf[5..];

    if payload.len() > MAX_WIRE_PAYLOAD {
        return Err(WireError::TooLarge(payload.len()));
    }

    let wire: WireTransaction = bincode::deserialize(payload)
        .map_err(|e| WireError::DecodeFailed(e.to_string()))?;

    Ok(TransactionVertex::from(wire))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx() -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            "alice".to_string(),
            "bob".to_string(),
            100, 1, 1000,
            "pk".to_string(),
            vec![],
        );
        tx.mine_anti_spam(2);
        tx.finalize();
        tx
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let tx = make_tx();
        let encoded = encode(&tx).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.tx_id, tx.tx_id);
        assert_eq!(decoded.sender, tx.sender);
        assert_eq!(decoded.amount, tx.amount);
    }

    #[test]
    fn test_magic_bytes_present() {
        let tx = make_tx();
        let encoded = encode(&tx).unwrap();
        assert_eq!(&encoded[..4], b"GHST");
    }

    #[test]
    fn test_version_byte_present() {
        let tx = make_tx();
        let encoded = encode(&tx).unwrap();
        assert_eq!(encoded[4], WIRE_VERSION);
    }

    #[test]
    fn test_bad_magic_rejected() {
        let tx = make_tx();
        let mut encoded = encode(&tx).unwrap();
        encoded[0] = 0x00;
        assert!(matches!(decode(&encoded), Err(WireError::BadMagic)));
    }

    #[test]
    fn test_bad_version_rejected() {
        let tx = make_tx();
        let mut encoded = encode(&tx).unwrap();
        encoded[4] = 99;
        assert!(matches!(decode(&encoded), Err(WireError::BadVersion(99))));
    }

    #[test]
    fn test_encode_with_commitment() {
        let mut tx = make_tx();
        tx.commitment = Some("aabbcc".to_string());
        tx.excess_commitment = Some("ddeeff".to_string());
        let encoded = encode(&tx).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.commitment, tx.commitment);
        assert_eq!(decoded.excess_commitment, tx.excess_commitment);
    }

    #[test]
    fn test_bincode_smaller_than_json() {
        let tx = make_tx();
        let bincode_size = encode(&tx).unwrap().len();
        let json_size = serde_json::to_string(&tx).unwrap().len();
        assert!(bincode_size < json_size,
            "bincode {} should be smaller than json {}", bincode_size, json_size);
    }

    #[test]
    fn test_parents_preserved() {
        let mut tx = make_tx();
        tx.parents = vec!["parent1".to_string(), "parent2".to_string()];
        let encoded = encode(&tx).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.parents, tx.parents);
    }
}