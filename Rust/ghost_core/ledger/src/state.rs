// ledger/src/state.rs

use std::collections::{HashMap, HashSet};
use crate::transaction::TransactionVertex;

#[derive(Debug, Clone, Default)]
pub struct LedgerState {
    pub balances: HashMap<String, u64>,
    pub nonces: HashMap<String, u64>,
    pub applied_txs: HashSet<String>,
}

impl LedgerState {
    pub fn new() -> Self {
        LedgerState::default()
    }

    pub fn ensure_account(&mut self, address: &str) {
        self.balances.entry(address.to_string()).or_insert(0);
        self.nonces.entry(address.to_string()).or_insert(0);
    }

    pub fn get_balance(&mut self, address: &str) -> u64 {
        self.ensure_account(address);
        *self.balances.get(address).unwrap()
    }

    pub fn get_nonce(&mut self, address: &str) -> u64 {
        self.ensure_account(address);
        *self.nonces.get(address).unwrap_or(&0)
    }

    pub fn credit(&mut self, address: &str, amount: u64) {
        self.ensure_account(address);
        *self.balances.get_mut(address).unwrap() += amount;
    }

    pub fn can_apply(&mut self, tx: &TransactionVertex) -> Result<(), String> {
        self.ensure_account(&tx.sender);
        self.ensure_account(&tx.receiver);

        if self.applied_txs.contains(&tx.tx_id) {
            return Err("transaction already applied".to_string());
        }

        if tx.amount == 0 {
            return Err("amount must be positive".to_string());
        }

        let balance = *self.balances.get(&tx.sender).unwrap();
        if balance < tx.amount {
            return Err(format!("insufficient balance: have {}, need {}", balance, tx.amount));
        }

        let expected_nonce = self.nonces.get(&tx.sender).copied().unwrap_or(0) + 1;
        if tx.nonce != expected_nonce {
            return Err(format!("invalid nonce: expected {}, got {}", expected_nonce, tx.nonce));
        }

        Ok(())
    }

    pub fn apply_transaction(&mut self, tx: &TransactionVertex) -> Result<(), String> {
        self.can_apply(tx)?;
    
        if let Some(b) = self.balances.get_mut(&tx.sender) {
            *b = b.saturating_sub(tx.amount);
        }
        if let Some(b) = self.balances.get_mut(&tx.receiver) {
            *b = b.saturating_add(tx.amount);
        }
        if let Some(n) = self.nonces.get_mut(&tx.sender) {
            *n = tx.nonce;
        }
        self.applied_txs.insert(tx.tx_id.clone());
    
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::TransactionVertex;

    fn make_tx(sender: &str, receiver: &str, amount: u64, nonce: u64) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            sender.to_string(),
            receiver.to_string(),
            amount,
            nonce,
            1000,
            "pk".to_string(),
            vec![],
        );
        tx.finalize();
        tx
    }

    #[test]
    fn test_credit_and_balance() {
        let mut state = LedgerState::new();
        state.credit("alice", 1000);
        assert_eq!(state.get_balance("alice"), 1000);
    }

    #[test]
    fn test_new_account_has_zero_balance() {
        let mut state = LedgerState::new();
        assert_eq!(state.get_balance("alice"), 0);
    }

    #[test]
    fn test_apply_transaction_updates_balances() {
        let mut state = LedgerState::new();
        state.credit("alice", 1000);

        let tx = make_tx("alice", "bob", 100, 1);
        state.apply_transaction(&tx).unwrap();

        assert_eq!(state.get_balance("alice"), 900);
        assert_eq!(state.get_balance("bob"), 100);
    }

    #[test]
    fn test_insufficient_balance_rejected() {
        let mut state = LedgerState::new();
        state.credit("alice", 50);

        let tx = make_tx("alice", "bob", 100, 1);
        assert!(state.can_apply(&tx).is_err());
    }

    #[test]
    fn test_duplicate_transaction_rejected() {
        let mut state = LedgerState::new();
        state.credit("alice", 1000);

        let tx = make_tx("alice", "bob", 100, 1);
        state.apply_transaction(&tx).unwrap();

        assert!(state.can_apply(&tx).is_err());
    }

    #[test]
    fn test_nonce_increments() {
        let mut state = LedgerState::new();
        state.credit("alice", 1000);

        let tx1 = make_tx("alice", "bob", 100, 1);
        state.apply_transaction(&tx1).unwrap();
        assert_eq!(state.get_nonce("alice"), 1);

        let tx2 = make_tx("alice", "bob", 100, 2);
        state.apply_transaction(&tx2).unwrap();
        assert_eq!(state.get_nonce("alice"), 2);
    }

    #[test]
    fn test_wrong_nonce_rejected() {
        let mut state = LedgerState::new();
        state.credit("alice", 1000);

        let tx = make_tx("alice", "bob", 100, 5); 
        assert!(state.can_apply(&tx).is_err());
    }

    #[test]
    fn test_multiple_credits() {
        let mut state = LedgerState::new();
        state.credit("alice", 500);
        state.credit("alice", 500);
        assert_eq!(state.get_balance("alice"), 1000);
    }
}