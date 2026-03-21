// consensus/src/conflict_resolver.rs

use std::collections::HashMap;
use ledger::transaction::{TransactionVertex, TxStatus};
use ledger::dag::DAG;

#[derive(Debug, Default)]
pub struct ConflictResolver {
    conflict_sets: HashMap<(String, u64), Vec<String>>,
}

impl ConflictResolver {
    pub fn new() -> Self {
        ConflictResolver::default()
    }

    pub fn register_transaction(&mut self, tx: &TransactionVertex) {
        let key = (tx.sender.clone(), tx.nonce);
        self.conflict_sets
            .entry(key)
            .or_default()
            .push(tx.tx_id.clone());
    }

    pub fn get_conflicts(&self, tx: &TransactionVertex) -> Vec<String> {
        let key = (tx.sender.clone(), tx.nonce);
        self.conflict_sets
            .get(&key)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|id| id != &tx.tx_id)
            .collect()
    }

    pub fn resolve(&self, dag: &mut DAG, tx: &TransactionVertex) {
        let conflicts = self.get_conflicts(tx);
        if conflicts.len() <= 1 {
            return;
        }

        let winner_id = conflicts
            .iter()
            .chain(std::iter::once(&tx.tx_id))
            .filter_map(|id| dag.get_transaction(id).map(|t| (id.clone(), t.weight)))
            .max_by_key(|(_, w)| *w)
            .map(|(id, _)| id);

        if let Some(winner) = winner_id {
            for id in &conflicts {
                if id != &winner {
                    if let Some(t) = dag.get_transaction_mut(id) {
                        t.status = TxStatus::Conflict;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::transaction::TransactionVertex;

    fn make_tx(tx_id: &str, sender: &str, nonce: u64) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            sender.to_string(), "bob".to_string(),
            100, nonce, 1000, "pk".to_string(), vec![],
        );
        tx.tx_id = tx_id.to_string();
        tx
    }

    #[test]
    fn test_register_and_get_conflicts() {
        let mut resolver = ConflictResolver::new();
        let tx1 = make_tx("tx1", "alice", 1);
        let tx2 = make_tx("tx2", "alice", 1);

        resolver.register_transaction(&tx1);
        resolver.register_transaction(&tx2);

        let conflicts = resolver.get_conflicts(&tx1);
        assert!(conflicts.contains(&"tx2".to_string()));
        assert!(!conflicts.contains(&"tx1".to_string()));
    }

    #[test]
    fn test_no_conflict_single_tx() {
        let mut resolver = ConflictResolver::new();
        let tx = make_tx("tx1", "alice", 1);
        resolver.register_transaction(&tx);

        let conflicts = resolver.get_conflicts(&tx);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_different_senders_no_conflict() {
        let mut resolver = ConflictResolver::new();
        let tx1 = make_tx("tx1", "alice", 1);
        let tx2 = make_tx("tx2", "bob", 1);

        resolver.register_transaction(&tx1);
        resolver.register_transaction(&tx2);

        assert!(resolver.get_conflicts(&tx1).is_empty());
        assert!(resolver.get_conflicts(&tx2).is_empty());
    }
}