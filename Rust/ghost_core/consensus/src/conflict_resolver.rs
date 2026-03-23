// consensus/src/conflict_resolver.rs

use std::collections::HashMap;
use ledger::transaction::{TransactionVertex, TxStatus};
use ledger::dag::DAG;

pub const RESOLUTION_MIN_WEIGHT: u64 = 3;

const MAX_STAKE_INFLUENCE: f64 = 3.0;

#[derive(Debug, Clone, PartialEq)]
pub enum ConflictStatus {
    Pending,
    Ready,
    Resolved { winner: String },
}

#[derive(Debug, Default)]
pub struct ConflictResolver {
    conflict_sets: HashMap<(String, u64), Vec<String>>,
    resolved: HashMap<(String, u64), String>,
}

impl ConflictResolver {
    pub fn new() -> Self { ConflictResolver::default() }

    pub fn register_transaction(&mut self, tx: &TransactionVertex) {
        let key = (tx.sender.clone(), tx.nonce);
        self.conflict_sets.entry(key).or_default().push(tx.tx_id.clone());
    }

    pub fn get_conflicts(&self, tx: &TransactionVertex) -> Vec<String> {
        let key = (tx.sender.clone(), tx.nonce);
        self.conflict_sets.get(&key).cloned().unwrap_or_default()
            .into_iter().filter(|id| id != &tx.tx_id).collect()
    }

    pub fn conflict_status(&self, tx: &TransactionVertex, dag: &DAG) -> ConflictStatus {
        let key = (tx.sender.clone(), tx.nonce);
        if let Some(winner) = self.resolved.get(&key) {
            return ConflictStatus::Resolved { winner: winner.clone() };
        }
        let ids = match self.conflict_sets.get(&key) {
            Some(v) if v.len() > 1 => v,
            _ => return ConflictStatus::Pending,
        };
        let all_ready = ids.iter().all(|id| {
            dag.get_transaction(id).map(|t| t.weight >= RESOLUTION_MIN_WEIGHT).unwrap_or(false)
        });
        if all_ready { ConflictStatus::Ready } else { ConflictStatus::Pending }
    }

    pub fn resolve(&self, dag: &mut DAG, tx: &TransactionVertex) {
        self.resolve_with_stake(dag, tx, &HashMap::new(), 0.0);
    }

    pub fn resolve_with_stake(
        &self, dag: &mut DAG, tx: &TransactionVertex,
        stake_weights: &HashMap<String, f64>, total_stake: f64,
    ) {
        let conflicts = self.get_conflicts(tx);
        if conflicts.is_empty() { return; }
        let all_ids: Vec<String> = conflicts.iter().chain(std::iter::once(&tx.tx_id)).cloned().collect();
        let canonical: Vec<String> = all_ids.iter()
            .filter(|id| dag.get_transaction(id).map(|t| t.weight >= RESOLUTION_MIN_WEIGHT).unwrap_or(true))
            .cloned().collect();
        let scores = Self::compute_scores(
            dag, if canonical.is_empty() { &all_ids } else { &canonical },
            stake_weights, total_stake,
        );
        if scores.is_empty() { return; }
        if let Some(winner) = Self::pick_winner(&scores) {
            for id in &all_ids {
                if id != &winner {
                    if let Some(t) = dag.get_transaction_mut(id) { t.status = TxStatus::Conflict; }
                }
            }
        }
    }

    pub fn resolve_ready(
        &mut self, dag: &mut DAG,
        stake_weights: &HashMap<String, f64>, total_stake: f64,
    ) -> Vec<String> {
        let ready_keys: Vec<(String, u64)> = self.conflict_sets.iter()
            .filter(|(key, ids)| {
                !self.resolved.contains_key(*key) && ids.len() > 1 &&
                ids.iter().all(|id| dag.get_transaction(id).map(|t| t.weight >= RESOLUTION_MIN_WEIGHT).unwrap_or(false))
            })
            .map(|(k, _)| k.clone()).collect();

        let mut resolved_winners = Vec::new();
        for key in ready_keys {
            let ids = match self.conflict_sets.get(&key) { Some(v) => v.clone(), None => continue };
            let scores = Self::compute_scores(dag, &ids, stake_weights, total_stake);
            if scores.is_empty() { continue; }
            if let Some(winner_id) = Self::pick_winner(&scores) {
                for id in &ids {
                    if id != &winner_id {
                        if let Some(t) = dag.get_transaction_mut(id) { t.status = TxStatus::Conflict; }
                    }
                }
                self.resolved.insert(key, winner_id.clone());
                resolved_winners.push(winner_id);
            }
        }
        resolved_winners
    }

    pub fn resolve_all_with_stake(
        &mut self, dag: &mut DAG,
        stake_weights: &HashMap<String, f64>, total_stake: f64,
    ) {
        let keys: Vec<(String, u64)> = self.conflict_sets.iter()
            .filter(|(_, ids)| ids.len() > 1)
            .map(|(k, _)| k.clone()).collect();
        for key in keys {
            if self.resolved.contains_key(&key) { continue; }
            let ids = match self.conflict_sets.get(&key) { Some(v) => v.clone(), None => continue };
            let scores = Self::compute_scores(dag, &ids, stake_weights, total_stake);
            if scores.is_empty() { continue; }
            if let Some(winner_id) = Self::pick_winner(&scores) {
                for id in &ids {
                    if id != &winner_id {
                        if let Some(t) = dag.get_transaction_mut(id) { t.status = TxStatus::Conflict; }
                    }
                }
                self.resolved.insert(key, winner_id);
            }
        }
    }

    pub fn resolved_count(&self) -> usize { self.resolved.len() }

    pub fn winner_of(&self, sender: &str, nonce: u64) -> Option<&String> {
        self.resolved.get(&(sender.to_string(), nonce))
    }

    fn compute_scores(dag: &DAG, ids: &[String], stake_weights: &HashMap<String, f64>, total_stake: f64) -> Vec<(String, f64)> {
        ids.iter().filter_map(|id| {
            let dag_tx = dag.get_transaction(id)?;
            let stake_amount = stake_weights.get(&dag_tx.sender).copied().unwrap_or(0.0);
            let stake_ratio = if total_stake > 0.0 { (stake_amount / total_stake).clamp(0.0, 1.0) } else { 0.0 };
            let stake_multiplier = 1.0 + stake_ratio * (MAX_STAKE_INFLUENCE - 1.0);
            Some((id.clone(), dag_tx.weight as f64 * stake_multiplier))
        }).collect()
    }

    fn pick_winner(scores: &[(String, f64)]) -> Option<String> {
        scores.iter().max_by(|(id_a, score_a), (id_b, score_b)| {
            score_a.partial_cmp(score_b).unwrap_or(std::cmp::Ordering::Equal).then_with(|| id_b.cmp(id_a))
        }).map(|(id, _)| id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::transaction::TransactionVertex;

    fn make_tx(tx_id: &str, sender: &str, nonce: u64) -> TransactionVertex {
        let mut tx = TransactionVertex::new(sender.to_string(), "bob".to_string(), 100, nonce, 1000, "pk".to_string(), vec![]);
        tx.tx_id = tx_id.to_string();
        tx
    }

    fn make_tx_w(tx_id: &str, sender: &str, nonce: u64, weight: u64) -> TransactionVertex {
        let mut tx = make_tx(tx_id, sender, nonce);
        tx.weight = weight;
        tx
    }

    #[test]
    fn test_register_and_get_conflicts() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx("tx1", "alice", 1);
        let tx2 = make_tx("tx2", "alice", 1);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        assert!(r.get_conflicts(&tx1).contains(&"tx2".to_string()));
    }

    #[test]
    fn test_no_conflict_single_tx() {
        let mut r = ConflictResolver::new();
        let tx = make_tx("tx1", "alice", 1);
        r.register_transaction(&tx);
        assert!(r.get_conflicts(&tx).is_empty());
    }

    #[test]
    fn test_different_senders_no_conflict() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx("tx1", "alice", 1);
        let tx2 = make_tx("tx2", "bob", 1);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        assert!(r.get_conflicts(&tx1).is_empty());
        assert!(r.get_conflicts(&tx2).is_empty());
    }

    #[test]
    fn test_conflict_status_pending_low_weight() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, 1);
        let tx2 = make_tx_w("tx2", "alice", 1, 1);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        let mut dag = DAG::new();
        dag.add_transaction(tx1.clone()).unwrap();
        dag.add_transaction(tx2.clone()).unwrap();
        assert_eq!(r.conflict_status(&tx1, &dag), ConflictStatus::Pending);
    }

    #[test]
    fn test_conflict_status_ready_when_both_above_threshold() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, RESOLUTION_MIN_WEIGHT);
        let tx2 = make_tx_w("tx2", "alice", 1, RESOLUTION_MIN_WEIGHT);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        let mut dag = DAG::new();
        dag.add_transaction(tx1.clone()).unwrap();
        dag.add_transaction(tx2.clone()).unwrap();
        assert_eq!(r.conflict_status(&tx1, &dag), ConflictStatus::Ready);
    }

    #[test]
    fn test_conflict_status_pending_when_only_one_above_threshold() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, RESOLUTION_MIN_WEIGHT);
        let tx2 = make_tx_w("tx2", "alice", 1, 1);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        let mut dag = DAG::new();
        dag.add_transaction(tx1.clone()).unwrap();
        dag.add_transaction(tx2.clone()).unwrap();
        assert_eq!(r.conflict_status(&tx1, &dag), ConflictStatus::Pending);
    }

    #[test]
    fn test_conflict_status_resolved_after_resolution() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, RESOLUTION_MIN_WEIGHT);
        let tx2 = make_tx_w("tx2", "alice", 1, RESOLUTION_MIN_WEIGHT);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        let mut dag = DAG::new();
        dag.add_transaction(tx1.clone()).unwrap();
        dag.add_transaction(tx2.clone()).unwrap();
        r.resolve_all_with_stake(&mut dag, &HashMap::new(), 0.0);
        assert!(matches!(r.conflict_status(&tx1, &dag), ConflictStatus::Resolved { .. }));
    }

    #[test]
    fn test_resolve_ready_only_fires_when_threshold_met() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, RESOLUTION_MIN_WEIGHT);
        let tx2 = make_tx_w("tx2", "alice", 1, 1);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        let mut dag = DAG::new();
        dag.add_transaction(tx1).unwrap();
        dag.add_transaction(tx2).unwrap();
        let resolved = r.resolve_ready(&mut dag, &HashMap::new(), 0.0);
        assert!(resolved.is_empty());
        assert_eq!(r.resolved_count(), 0);
    }

    #[test]
    fn test_resolve_ready_fires_when_both_meet_threshold() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, RESOLUTION_MIN_WEIGHT);
        let tx2 = make_tx_w("tx2", "alice", 1, RESOLUTION_MIN_WEIGHT);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        let mut dag = DAG::new();
        dag.add_transaction(tx1).unwrap();
        dag.add_transaction(tx2).unwrap();
        let resolved = r.resolve_ready(&mut dag, &HashMap::new(), 0.0);
        assert_eq!(resolved.len(), 1);
        assert_eq!(r.resolved_count(), 1);
    }

    #[test]
    fn test_resolve_ready_not_called_twice() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, RESOLUTION_MIN_WEIGHT);
        let tx2 = make_tx_w("tx2", "alice", 1, RESOLUTION_MIN_WEIGHT);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        let mut dag = DAG::new();
        dag.add_transaction(tx1).unwrap();
        dag.add_transaction(tx2).unwrap();
        r.resolve_ready(&mut dag, &HashMap::new(), 0.0);
        let second = r.resolve_ready(&mut dag, &HashMap::new(), 0.0);
        assert!(second.is_empty());
        assert_eq!(r.resolved_count(), 1);
    }

    #[test]
    fn test_winner_of_returns_correct() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, RESOLUTION_MIN_WEIGHT + 5);
        let tx2 = make_tx_w("tx2", "alice", 1, RESOLUTION_MIN_WEIGHT);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        let mut dag = DAG::new();
        dag.add_transaction(tx1).unwrap();
        dag.add_transaction(tx2).unwrap();
        r.resolve_ready(&mut dag, &HashMap::new(), 0.0);
        assert_eq!(r.winner_of("alice", 1), Some(&"tx1".to_string()));
    }

    #[test]
    fn test_cross_node_consistency_same_result_different_order() {
        // Node A
        let mut r_a = ConflictResolver::new();
        let tx1_a = make_tx_w("tx1", "alice", 1, RESOLUTION_MIN_WEIGHT + 2);
        let tx2_a = make_tx_w("tx2", "alice", 1, RESOLUTION_MIN_WEIGHT);
        r_a.register_transaction(&tx1_a); r_a.register_transaction(&tx2_a);
        let mut dag_a = DAG::new();
        dag_a.add_transaction(tx1_a).unwrap();
        dag_a.add_transaction(tx2_a).unwrap();

        // Node B — другой порядок регистрации
        let mut r_b = ConflictResolver::new();
        let tx2_b = make_tx_w("tx2", "alice", 1, RESOLUTION_MIN_WEIGHT);
        let tx1_b = make_tx_w("tx1", "alice", 1, RESOLUTION_MIN_WEIGHT + 2);
        r_b.register_transaction(&tx2_b); r_b.register_transaction(&tx1_b);
        let mut dag_b = DAG::new();
        dag_b.add_transaction(tx2_b).unwrap();
        dag_b.add_transaction(tx1_b).unwrap();

        r_a.resolve_ready(&mut dag_a, &HashMap::new(), 0.0);
        r_b.resolve_ready(&mut dag_b, &HashMap::new(), 0.0);

        assert_eq!(r_a.winner_of("alice", 1), r_b.winner_of("alice", 1));
        assert_eq!(r_a.winner_of("alice", 1), Some(&"tx1".to_string()));
    }

    #[test]
    fn test_cross_node_no_resolve_before_threshold() {
        let mut r_early = ConflictResolver::new();
        let tx1_e = make_tx_w("tx1", "alice", 1, 1);
        let tx2_e = make_tx_w("tx2", "alice", 1, 5);
        r_early.register_transaction(&tx1_e); r_early.register_transaction(&tx2_e);
        let mut dag_early = DAG::new();
        dag_early.add_transaction(tx1_e).unwrap();
        dag_early.add_transaction(tx2_e).unwrap();
        r_early.resolve_all_with_stake(&mut dag_early, &HashMap::new(), 0.0);

        let mut r_late = ConflictResolver::new();
        let tx1_l = make_tx_w("tx1", "alice", 1, 10);
        let tx2_l = make_tx_w("tx2", "alice", 1, 5);
        r_late.register_transaction(&tx1_l); r_late.register_transaction(&tx2_l);
        let mut dag_late = DAG::new();
        dag_late.add_transaction(tx1_l).unwrap();
        dag_late.add_transaction(tx2_l).unwrap();
        r_late.resolve_all_with_stake(&mut dag_late, &HashMap::new(), 0.0);

        assert_ne!(r_early.winner_of("alice", 1), r_late.winner_of("alice", 1),
            "Documents divergence when resolving before threshold");
    }

    #[test]
    fn test_stake_cap_prevents_dominance() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, 1);
        let tx2 = make_tx_w("tx2", "alice", 1, 1);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        let mut dag = DAG::new();
        dag.add_transaction(tx1.clone()).unwrap();
        dag.add_transaction(tx2.clone()).unwrap();
        let mut sw = HashMap::new();
        sw.insert("alice".to_string(), 999999.0);
        r.resolve_with_stake(&mut dag, &tx1, &sw, 1_000_000.0);
        assert!(matches!(dag.get_transaction("tx2").unwrap().status, TxStatus::Conflict));
        assert!(!matches!(dag.get_transaction("tx1").unwrap().status, TxStatus::Conflict));
    }

    #[test]
    fn test_stake_cap_max_influence_is_bounded() {
        let multiplier = 1.0 + 1.0f64 * (MAX_STAKE_INFLUENCE - 1.0);
        assert!((multiplier - MAX_STAKE_INFLUENCE).abs() < 1e-9);
        assert!(multiplier <= MAX_STAKE_INFLUENCE);
    }

    #[test]
    fn test_zero_total_stake_no_panic() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, 5);
        let tx2 = make_tx_w("tx2", "alice", 1, 1);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        let mut dag = DAG::new();
        dag.add_transaction(tx1.clone()).unwrap();
        dag.add_transaction(tx2.clone()).unwrap();
        r.resolve_with_stake(&mut dag, &tx2, &HashMap::new(), 0.0);
        assert!(matches!(dag.get_transaction("tx2").unwrap().status, TxStatus::Conflict));
    }

    #[test]
    fn test_dag_weight_wins_without_stake() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, 10);
        let tx2 = make_tx_w("tx2", "alice", 1, 1);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        let mut dag = DAG::new();
        dag.add_transaction(tx1.clone()).unwrap();
        dag.add_transaction(tx2.clone()).unwrap();
        r.resolve_with_stake(&mut dag, &tx2, &HashMap::new(), 0.0);
        assert!(matches!(dag.get_transaction("tx2").unwrap().status, TxStatus::Conflict));
        assert!(!matches!(dag.get_transaction("tx1").unwrap().status, TxStatus::Conflict));
    }

    #[test]
    fn test_tiebreaker_min_tx_id() {
        let mut r = ConflictResolver::new();
        let tx_a = make_tx_w("aaa", "alice", 1, 1);
        let tx_b = make_tx_w("bbb", "alice", 1, 1);
        r.register_transaction(&tx_a); r.register_transaction(&tx_b);
        let mut dag = DAG::new();
        dag.add_transaction(tx_a.clone()).unwrap();
        dag.add_transaction(tx_b.clone()).unwrap();
        r.resolve_with_stake(&mut dag, &tx_b, &HashMap::new(), 0.0);
        assert!(!matches!(dag.get_transaction("aaa").unwrap().status, TxStatus::Conflict));
        assert!(matches!(dag.get_transaction("bbb").unwrap().status, TxStatus::Conflict));
    }

    #[test]
    fn test_resolve_all_with_stake_and_cap() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx_w("tx1", "alice", 1, 5);
        let tx2 = make_tx_w("tx2", "alice", 1, 1);
        let tx3 = make_tx_w("tx3", "bob", 2, 1);
        let tx4 = make_tx_w("tx4", "bob", 2, 3);
        r.register_transaction(&tx1); r.register_transaction(&tx2);
        r.register_transaction(&tx3); r.register_transaction(&tx4);
        let mut dag = DAG::new();
        dag.add_transaction(tx1).unwrap(); dag.add_transaction(tx2).unwrap();
        dag.add_transaction(tx3).unwrap(); dag.add_transaction(tx4).unwrap();
        r.resolve_all_with_stake(&mut dag, &HashMap::new(), 0.0);
        assert!(!matches!(dag.get_transaction("tx1").unwrap().status, TxStatus::Conflict));
        assert!(matches!(dag.get_transaction("tx2").unwrap().status, TxStatus::Conflict));
        assert!(!matches!(dag.get_transaction("tx4").unwrap().status, TxStatus::Conflict));
        assert!(matches!(dag.get_transaction("tx3").unwrap().status, TxStatus::Conflict));
    }

    #[test]
    fn test_no_conflict_resolve_does_nothing() {
        let r = ConflictResolver::new();
        let tx = make_tx("tx1", "alice", 1);
        let mut dag = DAG::new();
        dag.add_transaction(tx.clone()).unwrap();
        r.resolve_with_stake(&mut dag, &tx, &HashMap::new(), 0.0);
        assert!(!matches!(dag.get_transaction("tx1").unwrap().status, TxStatus::Conflict));
    }
}