// ledger/src/pruner.rs

use crate::dag::DAG;
use crate::state::LedgerState;
use crate::transaction::TxStatus;

const DEFAULT_WINDOW: usize = 10_000;
const DEFAULT_INTERVAL: usize = 1_000;

#[derive(Debug)]
pub struct PruneResult {
    pub pruned_count: usize,
    pub remaining_count: usize,
    pub state_preserved: bool,
}

pub struct Pruner {
    pub window: usize,
}

impl Pruner {
    pub fn new(window: usize) -> Self {
        Pruner { window }
    }

    pub fn default() -> Self {
        Pruner { window: DEFAULT_WINDOW }
    }

    pub fn should_prune(&self, dag: &DAG, interval: usize) -> bool {
        let total = dag.vertices.len();
        total >= interval && total % interval == 0
    }

    pub fn should_prune_default(&self, dag: &DAG) -> bool {
        self.should_prune(dag, DEFAULT_INTERVAL)
    }

    pub fn prune(&self, dag: &mut DAG, _state: &LedgerState) -> PruneResult {
        let total = dag.vertices.len();

        if total <= self.window {
            return PruneResult {
                pruned_count: 0,
                remaining_count: total,
                state_preserved: true,
            };
        }

        let mut sorted_ids: Vec<(String, u64)> = dag.vertices
            .iter()
            .map(|(id, tx)| (id.clone(), tx.timestamp))
            .collect();
        sorted_ids.sort_by_key(|(_, ts)| *ts);

        let to_delete_count = total - self.window;
        let candidates: Vec<String> = sorted_ids
            .into_iter()
            .take(to_delete_count)
            .map(|(id, _)| id)
            .collect();

        let current_tips: std::collections::HashSet<String> = dag.get_tips()
            .into_iter()
            .collect();

        let mut pruned = 0;

        for tx_id in &candidates {
            let should_remove = dag.vertices.get(tx_id)
                .map(|tx| {
                    !current_tips.contains(tx_id)
                        && matches!(tx.status, TxStatus::Confirmed)
                })
                .unwrap_or(false);

            if should_remove {
                if let Some(tx) = dag.vertices.remove(tx_id) {
                    for parent_id in &tx.parents {
                        if let Some(children) = dag.children_map.get_mut(parent_id) {
                            children.remove(tx_id);
                            if children.is_empty() {
                                dag.children_map.remove(parent_id);
                            }
                        }
                    }
                    dag.tips.remove(tx_id);
                    pruned += 1;
                }
            }
        }

        PruneResult {
            pruned_count: pruned,
            remaining_count: dag.vertices.len(),
            state_preserved: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::DAG;
    use crate::state::LedgerState;
    use crate::transaction::{TransactionVertex, TxStatus};

    fn make_confirmed_tx(tx_id: &str, timestamp: u64) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            "alice".to_string(), "bob".to_string(),
            10, 1, timestamp, "pk".to_string(), vec![],
        );
        tx.tx_id = tx_id.to_string();
        tx.status = TxStatus::Confirmed;
        tx
    }

    fn make_pending_tx(tx_id: &str, timestamp: u64) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            "alice".to_string(), "bob".to_string(),
            10, 1, timestamp, "pk".to_string(), vec![],
        );
        tx.tx_id = tx_id.to_string();
        tx.status = TxStatus::Pending;
        tx
    }

    #[test]
    fn test_should_prune_at_interval() {
        let pruner = Pruner::new(100);
        let mut dag = DAG::new();
        for i in 0..1000 {
            let tx = make_confirmed_tx(&format!("tx{}", i), i as u64);
            dag.vertices.insert(tx.tx_id.clone(), tx);
        }
        assert!(pruner.should_prune(&dag, 1000));
    }

    #[test]
    fn test_should_not_prune_below_interval() {
        let pruner = Pruner::new(100);
        let mut dag = DAG::new();
        for i in 0..500 {
            let tx = make_confirmed_tx(&format!("tx{}", i), i as u64);
            dag.vertices.insert(tx.tx_id.clone(), tx);
        }
        assert!(!pruner.should_prune(&dag, 1000));
    }

    #[test]
    fn test_prune_removes_old_confirmed() {
        let pruner = Pruner::new(5);
        let mut dag = DAG::new();
        let state = LedgerState::new();

        for i in 0..10 {
            let tx = make_confirmed_tx(&format!("tx{}", i), i as u64);
            dag.vertices.insert(tx.tx_id.clone(), tx);
            dag.tips.insert(format!("tx{}", i));
        }
        dag.tips = std::collections::HashSet::from(["tx9".to_string()]);

        let result = pruner.prune(&mut dag, &state);
        assert!(result.pruned_count > 0);
        assert!(result.state_preserved);
    }

    #[test]
    fn test_prune_never_removes_tips() {
        let pruner = Pruner::new(2);
        let mut dag = DAG::new();
        let state = LedgerState::new();

        for i in 0..10 {
            let tx = make_confirmed_tx(&format!("tx{}", i), i as u64);
            dag.vertices.insert(tx.tx_id.clone(), tx);
            dag.tips.insert(format!("tx{}", i));
        }

        let result = pruner.prune(&mut dag, &state);
        assert_eq!(result.pruned_count, 0);
    }

    #[test]
    fn test_prune_never_removes_pending() {
        let pruner = Pruner::new(2);
        let mut dag = DAG::new();
        let state = LedgerState::new();

        for i in 0..5 {
            let tx = make_pending_tx(&format!("tx{}", i), i as u64);
            dag.vertices.insert(tx.tx_id.clone(), tx);
        }

        let result = pruner.prune(&mut dag, &state);
        assert_eq!(result.pruned_count, 0);
    }

    #[test]
    fn test_prune_within_window_does_nothing() {
        let pruner = Pruner::new(1000);
        let mut dag = DAG::new();
        let state = LedgerState::new();

        for i in 0..10 {
            let tx = make_confirmed_tx(&format!("tx{}", i), i as u64);
            dag.vertices.insert(tx.tx_id.clone(), tx);
        }

        let result = pruner.prune(&mut dag, &state);
        assert_eq!(result.pruned_count, 0);
        assert_eq!(result.remaining_count, 10);
    }

    #[test]
    fn test_prune_removes_oldest_first() {
        let pruner = Pruner::new(3);
        let mut dag = DAG::new();
        let state = LedgerState::new();

        for i in 0..6 {
            let tx = make_confirmed_tx(&format!("tx{}", i), i as u64);
            dag.vertices.insert(tx.tx_id.clone(), tx);
        }
        dag.tips = std::collections::HashSet::from(["tx5".to_string()]);

        pruner.prune(&mut dag, &state);

        assert!(!dag.vertices.contains_key("tx0"));
        assert!(!dag.vertices.contains_key("tx1"));
        assert!(!dag.vertices.contains_key("tx2"));
        assert!(dag.vertices.contains_key("tx5"));
    }
}