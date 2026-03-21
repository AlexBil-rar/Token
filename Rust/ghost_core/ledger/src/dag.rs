// ledger/src/dag.rs

use std::collections::{HashMap, HashSet};
use crate::transaction::{TransactionVertex, TxStatus};

const CONFIRMATION_THRESHOLD: u64 = 6;

#[derive(Debug, Default)]
pub struct DAG {
    pub vertices: HashMap<String, TransactionVertex>,
    pub children_map: HashMap<String, HashSet<String>>,
    pub tips: HashSet<String>,
}

impl DAG {
    pub fn new() -> Self {
        DAG::default()
    }

    pub fn add_transaction(&mut self, tx: TransactionVertex) -> Result<(), String> {
        if self.vertices.contains_key(&tx.tx_id) {
            return Err(format!("transaction already exists: {}", tx.tx_id));
        }

        for parent_id in &tx.parents {
            self.children_map
                .entry(parent_id.clone())
                .or_default()
                .insert(tx.tx_id.clone());
            self.tips.remove(parent_id);
        }

        self.tips.insert(tx.tx_id.clone());
        self.vertices.insert(tx.tx_id.clone(), tx);
        Ok(())
    }

    pub fn has_transaction(&self, tx_id: &str) -> bool {
        self.vertices.contains_key(tx_id)
    }

    pub fn get_transaction(&self, tx_id: &str) -> Option<&TransactionVertex> {
        self.vertices.get(tx_id)
    }

    pub fn get_transaction_mut(&mut self, tx_id: &str) -> Option<&mut TransactionVertex> {
        self.vertices.get_mut(tx_id)
    }

    pub fn get_tips(&self) -> Vec<String> {
        self.tips
            .iter()
            .filter(|tx_id| {
                if let Some(tx) = self.vertices.get(*tx_id) {
                    !matches!(tx.status, TxStatus::Rejected | TxStatus::Conflict)
                } else {
                    false
                }
            })
            .cloned()
            .collect()
    }

    pub fn propagate_weight(&mut self, tx_id: &str) {
        let parents = match self.vertices.get(tx_id) {
            Some(tx) => tx.parents.clone(),
            None => return,
        };

        let mut stack: Vec<String> = parents;
        let mut visited: HashSet<String> = HashSet::new();

        while let Some(parent_id) = stack.pop() {
            if visited.contains(&parent_id) {
                continue;
            }
            visited.insert(parent_id.clone());

            if let Some(parent) = self.vertices.get_mut(&parent_id) {
                parent.weight += 1;

                if parent.weight >= CONFIRMATION_THRESHOLD
                    && !matches!(parent.status, TxStatus::Rejected)
                {
                    parent.status = TxStatus::Confirmed;
                }

                let grandparents = parent.parents.clone();
                stack.extend(grandparents);
            }
        }
    }

    pub fn stats(&self) -> DagStats {
        let mut confirmed = 0u64;
        let mut rejected = 0u64;
        let mut pending = 0u64;
        let mut conflict = 0u64;

        for tx in self.vertices.values() {
            match tx.status {
                TxStatus::Confirmed => confirmed += 1,
                TxStatus::Rejected => rejected += 1,
                TxStatus::Conflict => conflict += 1,
                TxStatus::Pending => pending += 1,
            }
        }

        DagStats {
            total_vertices: self.vertices.len() as u64,
            tips: self.get_tips().len() as u64,
            confirmed,
            pending,
            rejected,
            conflict,
        }
    }
}

#[derive(Debug)]
pub struct DagStats {
    pub total_vertices: u64,
    pub tips: u64,
    pub confirmed: u64,
    pub pending: u64,
    pub rejected: u64,
    pub conflict: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::TransactionVertex;

    fn make_tx(tx_id: &str, parents: Vec<String>) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            "alice".to_string(),
            "bob".to_string(),
            10,
            1,
            1000,
            "pk".to_string(),
            parents,
        );
        tx.tx_id = tx_id.to_string();
        tx
    }

    #[test]
    fn test_add_transaction_stores_vertex() {
        let mut dag = DAG::new();
        let tx = make_tx("tx1", vec![]);
        dag.add_transaction(tx).unwrap();
        assert!(dag.has_transaction("tx1"));
    }

    #[test]
    fn test_add_transaction_adds_to_tips() {
        let mut dag = DAG::new();
        let tx = make_tx("tx1", vec![]);
        dag.add_transaction(tx).unwrap();
        assert!(dag.get_tips().contains(&"tx1".to_string()));
    }

    #[test]
    fn test_add_transaction_removes_parent_from_tips() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx1", vec![])).unwrap();
        dag.add_transaction(make_tx("tx2", vec!["tx1".to_string()])).unwrap();

        let tips = dag.get_tips();
        assert!(!tips.contains(&"tx1".to_string()));
        assert!(tips.contains(&"tx2".to_string()));
    }

    #[test]
    fn test_add_duplicate_fails() {
        let mut dag = DAG::new();
        let tx = make_tx("tx1", vec![]);
        dag.add_transaction(tx.clone()).unwrap();
        assert!(dag.add_transaction(tx).is_err());
    }

    #[test]
    fn test_propagate_weight_increments_parent() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx1", vec![])).unwrap();
        dag.add_transaction(make_tx("tx2", vec!["tx1".to_string()])).unwrap();
        dag.propagate_weight("tx2");

        assert_eq!(dag.vertices["tx1"].weight, 2); 
    }

    #[test]
    fn test_propagate_weight_confirms_after_threshold() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx0", vec![])).unwrap();

        for i in 1..6 {
            let tx = make_tx(&format!("tx{}", i), vec!["tx0".to_string()]);
            dag.add_transaction(tx).unwrap();
            dag.propagate_weight(&format!("tx{}", i));
        }

        assert!(matches!(dag.vertices["tx0"].status, TxStatus::Confirmed));
    }

    #[test]
    fn test_propagate_weight_does_not_confirm_below_threshold() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx0", vec![])).unwrap();

        for i in 1..5 {
            let tx = make_tx(&format!("tx{}", i), vec!["tx0".to_string()]);
            dag.add_transaction(tx).unwrap();
            dag.propagate_weight(&format!("tx{}", i));
        }

        assert!(matches!(dag.vertices["tx0"].status, TxStatus::Pending));
    }

    #[test]
    fn test_stats_counts_correctly() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx1", vec![])).unwrap();

        let mut tx2 = make_tx("tx2", vec![]);
        tx2.status = TxStatus::Conflict;
        dag.add_transaction(tx2).unwrap();

        let mut tx3 = make_tx("tx3", vec![]);
        tx3.status = TxStatus::Rejected;
        dag.add_transaction(tx3).unwrap();

        let stats = dag.stats();
        assert_eq!(stats.total_vertices, 3);
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.conflict, 1);
        assert_eq!(stats.rejected, 1);
    }

    #[test]
    fn test_get_tips_excludes_conflict() {
        let mut dag = DAG::new();
        let mut tx = make_tx("tx1", vec![]);
        tx.status = TxStatus::Conflict;
        dag.add_transaction(tx).unwrap();
        assert!(!dag.get_tips().contains(&"tx1".to_string()));
    }

    #[test]
    fn test_get_tips_excludes_rejected() {
        let mut dag = DAG::new();
        let mut tx = make_tx("tx1", vec![]);
        tx.status = TxStatus::Rejected;
        dag.add_transaction(tx).unwrap();
        assert!(!dag.get_tips().contains(&"tx1".to_string()));
    }
}