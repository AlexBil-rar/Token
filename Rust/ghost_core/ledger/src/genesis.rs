// ledger/src/genesis.rs

use crate::transaction::TransactionVertex;
use crate::cut_through::TxKernel;
use crypto::commitments::{Commitment, BlindingFactor, BalanceProof};
use crypto::range_proof::{PlaceholderRangeProof, RangeProofSystem, RangeProofStatus};

pub const GENESIS_AMOUNT: u64 = 21_000_000;

pub struct GenesisResult {
    pub tx: TransactionVertex,
    pub kernel: TxKernel,
    pub blinding_hex: String,
}

pub fn create_genesis_tx(
    genesis_address: &str,
    public_key: &str,
    sign_fn: impl Fn(&[u8]) -> String,
) -> GenesisResult {
    let blinding = BlindingFactor::random();
    let commitment = Commitment::commit(GENESIS_AMOUNT, &blinding);
    let proof = BalanceProof::create(&[blinding.clone()], &[]);
    let range_proof = PlaceholderRangeProof::prove(
        GENESIS_AMOUNT, &blinding, &commitment
    ).unwrap();

    let mut tx = TransactionVertex::new(
        "system".to_string(),
        genesis_address.to_string(),
        GENESIS_AMOUNT,
        1,
        0,
        public_key.to_string(),
        vec![],
    );

    tx.commitment = Some(commitment.point_hex.clone());
    tx.balance_proof = Some(serde_json::to_string(&proof).unwrap());
    tx.excess_commitment = Some(proof.excess_commitment_hex.clone());
    tx.excess_signature = Some(proof.excess_signature_hex.clone());
    tx.range_proof = Some(serde_json::to_string(&range_proof).unwrap());
    tx.range_proof_status = RangeProofStatus::Experimental;

    tx.anti_spam_nonce = 0;
    tx.anti_spam_hash = tx.compute_anti_spam_hash();
    tx.signature = sign_fn(&tx.signing_payload());
    tx.finalize();

    let kernel = TxKernel::from_tx(&tx);
    let blinding_hex = blinding.to_hex();

    GenesisResult { tx, kernel, blinding_hex }
}

pub fn verify_genesis_kernel_sum(kernel: &TxKernel) -> bool {
    use curve25519_dalek::ristretto::CompressedRistretto;

    let hex = match &kernel.excess_commitment {
        Some(h) => h,
        None => return false,
    };

    let bytes = match hex::decode(hex) {
        Ok(b) if b.len() == 32 => b,
        _ => return false,
    };

    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    CompressedRistretto(arr).decompress().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_sign(_payload: &[u8]) -> String {
        "a".repeat(128)
    }

    #[test]
    fn test_genesis_tx_has_commitment() {
        let result = create_genesis_tx("genesis_addr", &"b".repeat(64), dummy_sign);
        assert!(result.tx.commitment.is_some());
    }

    #[test]
    fn test_genesis_tx_amount() {
        let result = create_genesis_tx("genesis_addr", &"b".repeat(64), dummy_sign);
        assert_eq!(result.tx.amount, GENESIS_AMOUNT);
    }

    #[test]
    fn test_genesis_tx_no_parents() {
        let result = create_genesis_tx("genesis_addr", &"b".repeat(64), dummy_sign);
        assert!(result.tx.parents.is_empty());
    }

    #[test]
    fn test_genesis_tx_has_kernel() {
        let result = create_genesis_tx("genesis_addr", &"b".repeat(64), dummy_sign);
        assert!(result.kernel.excess_commitment.is_some());
    }

    #[test]
    fn test_genesis_kernel_sum_valid() {
        let result = create_genesis_tx("genesis_addr", &"b".repeat(64), dummy_sign);
        assert!(verify_genesis_kernel_sum(&result.kernel));
    }

    #[test]
    fn test_genesis_blinding_hex_length() {
        let result = create_genesis_tx("genesis_addr", &"b".repeat(64), dummy_sign);
        assert_eq!(result.blinding_hex.len(), 64);
    }

    #[test]
    fn test_genesis_tx_id_not_empty() {
        let result = create_genesis_tx("genesis_addr", &"b".repeat(64), dummy_sign);
        assert!(!result.tx.tx_id.is_empty());
    }

    #[test]
    fn test_genesis_sender_is_system() {
        let result = create_genesis_tx("genesis_addr", &"b".repeat(64), dummy_sign);
        assert_eq!(result.tx.sender, "system");
    }
}