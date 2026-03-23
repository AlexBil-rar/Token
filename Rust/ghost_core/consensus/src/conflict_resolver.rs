// consensus/src/conflict_resolver.rs

use std::collections::HashMap;
use ledger::transaction::{TransactionVertex, TxStatus};
use ledger::dag::DAG;

const MAX_STAKE_INFLUENCE: f64 = 3.0;

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
        self.resolve_with_stake(dag, tx, &HashMap::new(), 0.0);
    }

    pub fn resolve_with_stake(
        &self,
        dag: &mut DAG,
        tx: &TransactionVertex,
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
    ) {
        let conflicts = self.get_conflicts(tx);
        if conflicts.is_empty() {
            return;
        }

        let all_ids: Vec<String> = conflicts.iter()
            .chain(std::iter::once(&tx.tx_id))
            .cloned()
            .collect();

        let scores = Self::compute_scores(dag, &all_ids, stake_weights, total_stake);

        if scores.is_empty() {
            return;
        }

        let winner_id = Self::pick_winner(&scores);

        if let Some(winner) = winner_id {
            for id in &all_ids {
                if id != &winner {
                    if let Some(t) = dag.get_transaction_mut(id) {
                        t.status = TxStatus::Conflict;
                    }
                }
            }
        }
    }

    pub fn resolve_all_with_stake(
        &self,
        dag: &mut DAG,
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
    ) {
        let keys: Vec<(String, u64)> = self.conflict_sets
            .iter()
            .filter(|(_, ids)| ids.len() > 1)
            .map(|(k, _)| k.clone())
            .collect();

        for key in keys {
            let ids = match self.conflict_sets.get(&key) {
                Some(v) => v.clone(),
                None => continue,
            };

            let scores = Self::compute_scores(dag, &ids, stake_weights, total_stake);

            if scores.is_empty() {
                continue;
            }

            if let Some(winner_id) = Self::pick_winner(&scores) {
                for id in &ids {
                    if id != &winner_id {
                        if let Some(t) = dag.get_transaction_mut(id) {
                            t.status = TxStatus::Conflict;
                        }
                    }
                }
            }
        }
    }

    fn compute_scores(
        dag: &DAG,
        ids: &[String],
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
    ) -> Vec<(String, f64)> {
        ids.iter()
            .filter_map(|id| {
                let dag_tx = dag.get_transaction(id)?;
                let dag_weight = dag_tx.weight as f64;

                let stake_amount = stake_weights
                    .get(&dag_tx.sender)
                    .copied()
                    .unwrap_or(0.0);

                let stake_ratio = if total_stake > 0.0 {
                    (stake_amount / total_stake).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                let stake_multiplier = 1.0 + stake_ratio * (MAX_STAKE_INFLUENCE - 1.0);

                let score = dag_weight * stake_multiplier;
                Some((id.clone(), score))
            })
            .collect()
    }

    fn pick_winner(scores: &[(String, f64)]) -> Option<String> {
        scores.iter()
            .max_by(|(id_a, score_a), (id_b, score_b)| {
                score_a.partial_cmp(score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| id_b.cmp(id_a)) 
            })
            .map(|(id, _)| id.clone())
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

    fn make_tx_with_weight(tx_id: &str, sender: &str, nonce: u64, weight: u64) -> TransactionVertex {
        let mut tx = make_tx(tx_id, sender, nonce);
        tx.weight = weight;
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
        assert!(resolver.get_conflicts(&tx).is_empty());
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

    #[test]
    fn test_stake_cap_prevents_dominance() {
        // tx1 и tx2 от одного sender + одинаковый nonce = double-spend конфликт.
        // Оба имеют одинаковый weight и одинаковый stake → тай-брейкер по tx_id.
        // "tx1" < "tx2" → tx1 побеждает.
        let mut resolver = ConflictResolver::new();
        let tx1 = make_tx_with_weight("tx1", "alice", 1, 1);
        let tx2 = make_tx_with_weight("tx2", "alice", 1, 1);

        resolver.register_transaction(&tx1);
        resolver.register_transaction(&tx2);

        let mut dag = DAG::new();
        dag.add_transaction(tx1.clone()).unwrap();
        dag.add_transaction(tx2.clone()).unwrap();

        let mut stake_weights = HashMap::new();
        stake_weights.insert("alice".to_string(), 999999.0);
        let total_stake = 1_000_000.0;

        resolver.resolve_with_stake(&mut dag, &tx1, &stake_weights, total_stake);

        assert!(matches!(
            dag.get_transaction("tx2").unwrap().status,
            TxStatus::Conflict
        ));
        assert!(!matches!(
            dag.get_transaction("tx1").unwrap().status,
            TxStatus::Conflict
        ));
    }

    #[test]
    fn test_stake_cap_max_influence_is_bounded() {
        let stake_ratio = 1.0f64; 
        let multiplier = 1.0 + stake_ratio * (MAX_STAKE_INFLUENCE - 1.0);
        assert!((multiplier - MAX_STAKE_INFLUENCE).abs() < 1e-9);
        assert!(multiplier <= MAX_STAKE_INFLUENCE);
    }

    #[test]
    fn test_zero_total_stake_no_panic() {
        let mut resolver = ConflictResolver::new();
        let tx1 = make_tx_with_weight("tx1", "alice", 1, 5);
        let tx2 = make_tx_with_weight("tx2", "alice", 1, 1);
        resolver.register_transaction(&tx1);
        resolver.register_transaction(&tx2);

        let mut dag = DAG::new();
        dag.add_transaction(tx1.clone()).unwrap();
        dag.add_transaction(tx2.clone()).unwrap();

        resolver.resolve_with_stake(&mut dag, &tx2, &HashMap::new(), 0.0);

        assert!(matches!(dag.get_transaction("tx2").unwrap().status, TxStatus::Conflict));
        assert!(!matches!(dag.get_transaction("tx1").unwrap().status, TxStatus::Conflict));
    }

    #[test]
    fn test_dag_weight_wins_without_stake() {
        let mut resolver = ConflictResolver::new();
        let tx1 = make_tx_with_weight("tx1", "alice", 1, 10);
        let tx2 = make_tx_with_weight("tx2", "alice", 1, 1);
        resolver.register_transaction(&tx1);
        resolver.register_transaction(&tx2);

        let mut dag = DAG::new();
        dag.add_transaction(tx1.clone()).unwrap();
        dag.add_transaction(tx2.clone()).unwrap();

        resolver.resolve_with_stake(&mut dag, &tx2, &HashMap::new(), 0.0);

        assert!(matches!(dag.get_transaction("tx2").unwrap().status, TxStatus::Conflict));
        assert!(!matches!(dag.get_transaction("tx1").unwrap().status, TxStatus::Conflict));
    }

    #[test]
    fn test_tiebreaker_min_tx_id() {
        let mut resolver = ConflictResolver::new();
        let tx_a = make_tx_with_weight("aaa", "alice", 1, 1);
        let tx_b = make_tx_with_weight("bbb", "alice", 1, 1);
        resolver.register_transaction(&tx_a);
        resolver.register_transaction(&tx_b);

        let mut dag = DAG::new();
        dag.add_transaction(tx_a.clone()).unwrap();
        dag.add_transaction(tx_b.clone()).unwrap();

        resolver.resolve_with_stake(&mut dag, &tx_b, &HashMap::new(), 0.0);

        assert!(!matches!(dag.get_transaction("aaa").unwrap().status, TxStatus::Conflict));
        assert!(matches!(dag.get_transaction("bbb").unwrap().status, TxStatus::Conflict));
    }

    #[test]
    fn test_resolve_all_with_stake_and_cap() {
        let mut resolver = ConflictResolver::new();
        let tx1 = make_tx_with_weight("tx1", "alice", 1, 5);
        let tx2 = make_tx_with_weight("tx2", "alice", 1, 1);
        let tx3 = make_tx_with_weight("tx3", "bob", 2, 1);
        let tx4 = make_tx_with_weight("tx4", "bob", 2, 3);
        resolver.register_transaction(&tx1);
        resolver.register_transaction(&tx2);
        resolver.register_transaction(&tx3);
        resolver.register_transaction(&tx4);

        let mut dag = DAG::new();
        dag.add_transaction(tx1).unwrap();
        dag.add_transaction(tx2).unwrap();
        dag.add_transaction(tx3).unwrap();
        dag.add_transaction(tx4).unwrap();

        resolver.resolve_all_with_stake(&mut dag, &HashMap::new(), 0.0);

        assert!(!matches!(dag.get_transaction("tx1").unwrap().status, TxStatus::Conflict));
        assert!(matches!(dag.get_transaction("tx2").unwrap().status, TxStatus::Conflict));
        assert!(!matches!(dag.get_transaction("tx4").unwrap().status, TxStatus::Conflict));
        assert!(matches!(dag.get_transaction("tx3").unwrap().status, TxStatus::Conflict));
    }

    #[test]
    fn test_no_conflict_resolve_does_nothing() {
        let resolver = ConflictResolver::new();
        let tx = make_tx("tx1", "alice", 1);
        let mut dag = DAG::new();
        dag.add_transaction(tx.clone()).unwrap();
        resolver.resolve_with_stake(&mut dag, &tx, &HashMap::new(), 0.0);
        assert!(!matches!(
            dag.get_transaction("tx1").unwrap().status,
            TxStatus::Conflict
        ));
    }
}