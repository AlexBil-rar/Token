// ledger/src/genesis.rs

use crate::transaction::TransactionVertex;
use crate::cut_through::TxKernel;

pub const GENESIS_AMOUNT: u64 = 21_000_000;

pub struct GenesisResult {
    pub tx: TransactionVertex,
    pub kernel: Option<TxKernel>,
}

pub fn create_genesis_tx(
    genesis_address: &str,
    public_key: &str,
    sign_fn: impl Fn(&[u8]) -> String,
) -> GenesisResult {
    let mut tx = TransactionVertex::new(
        "system".to_string(),
        genesis_address.to_string(),
        GENESIS_AMOUNT,
        1,
        0,
        public_key.to_string(),
        vec![],
    );

    tx.anti_spam_nonce = 0;
    tx.anti_spam_hash = tx.compute_anti_spam_hash();
    tx.signature = sign_fn(&tx.signing_payload());
    tx.finalize();

    GenesisResult { tx, kernel: None }
}

pub fn verify_genesis_kernel_sum(_kernel: &TxKernel) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_sign(_payload: &[u8]) -> String {
        "a".repeat(128)
    }

    #[test]
    fn test_genesis_tx_is_transparent() {
        let result = create_genesis_tx("genesis_addr", &"b".repeat(64), dummy_sign);
        assert!(result.tx.commitment.is_none(), "transparent genesis has no commitment");
        assert!(result.kernel.is_none());
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
    fn test_genesis_tx_sender_is_system() {
        let result = create_genesis_tx("genesis_addr", &"b".repeat(64), dummy_sign);
        assert_eq!(result.tx.sender, "system");
    }

    #[test]
    fn test_genesis_tx_id_not_empty() {
        let result = create_genesis_tx("genesis_addr", &"b".repeat(64), dummy_sign);
        assert!(!result.tx.tx_id.is_empty());
    }

    #[test]
    fn test_genesis_no_blinding_secret() {
        // Transparent genesis requires no secret to be preserved by operator
        let r1 = create_genesis_tx("addr", &"b".repeat(64), dummy_sign);
        let r2 = create_genesis_tx("addr", &"b".repeat(64), dummy_sign);
        assert_eq!(r1.tx.tx_id, r2.tx.tx_id, "genesis is deterministic");
    }
}