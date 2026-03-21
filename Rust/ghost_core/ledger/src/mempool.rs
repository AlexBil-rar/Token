// ledger/src/mempool.rs

use std::collections::HashMap;
use crate::transaction::TransactionVertex;

#[derive(Debug, Default)]
pub struct Mempool {
    transactions: HashMap<String, TransactionVertex>,
}

impl Mempool {
    pub fn new() -> Self {
        Mempool::default()
    }

    pub fn add(&mut self, tx: TransactionVertex) {
        self.transactions.insert(tx.tx_id.clone(), tx);
    }

    pub fn remove(&mut self, tx_id: &str) {
        self.transactions.remove(tx_id);
    }

    pub fn has(&self, tx_id: &str) -> bool {
        self.transactions.contains_key(tx_id)
    }

    pub fn get(&self, tx_id: &str) -> Option<&TransactionVertex> {
        self.transactions.get(tx_id)
    }

    pub fn get_all(&self) -> Vec<&TransactionVertex> {
        self.transactions.values().collect()
    }

    pub fn get_all_ids(&self) -> Vec<String> {
        self.transactions.keys().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.transactions.clear();
    }

    pub fn size(&self) -> usize {
        self.transactions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::TransactionVertex;

    fn make_tx(tx_id: &str) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            "alice".to_string(), "bob".to_string(),
            100, 1, 1000, "pk".to_string(), vec![],
        );
        tx.tx_id = tx_id.to_string();
        tx
    }

    #[test]
    fn test_add_and_has() {
        let mut mempool = Mempool::new();
        mempool.add(make_tx("tx1"));
        assert!(mempool.has("tx1"));
    }

    #[test]
    fn test_remove() {
        let mut mempool = Mempool::new();
        mempool.add(make_tx("tx1"));
        mempool.remove("tx1");
        assert!(!mempool.has("tx1"));
    }

    #[test]
    fn test_size() {
        let mut mempool = Mempool::new();
        assert_eq!(mempool.size(), 0);
        mempool.add(make_tx("tx1"));
        mempool.add(make_tx("tx2"));
        assert_eq!(mempool.size(), 2);
    }

    #[test]
    fn test_clear() {
        let mut mempool = Mempool::new();
        mempool.add(make_tx("tx1"));
        mempool.add(make_tx("tx2"));
        mempool.clear();
        assert!(mempool.is_empty());
    }

    #[test]
    fn test_get_all() {
        let mut mempool = Mempool::new();
        mempool.add(make_tx("tx1"));
        mempool.add(make_tx("tx2"));
        assert_eq!(mempool.get_all().len(), 2);
    }

    #[test]
    fn test_remove_nonexistent_is_ok() {
        let mut mempool = Mempool::new();
        mempool.remove("nonexistent");
    }
}