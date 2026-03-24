// ledger/src/parent_selection.rs

use std::collections::HashMap;
use crate::dag::DAG;
use crate::privacy::DecoyPool;


#[derive(Debug, Clone)]
pub struct ParentSelectionPolicy {
    pub beta: f64,
    pub epsilon: f64,
    pub max_parents: usize,
}

impl Default for ParentSelectionPolicy {
    fn default() -> Self {
        ParentSelectionPolicy { beta: 0.7, epsilon: 0.10, max_parents: 2 }
    }
}

impl ParentSelectionPolicy {
    pub fn consensus_mode() -> Self {
        ParentSelectionPolicy { beta: 1.0, epsilon: 0.0, max_parents: 2 }
    }

    pub fn privacy_mode() -> Self {
        ParentSelectionPolicy { beta: 0.3, epsilon: 0.25, max_parents: 2 }
    }

    pub fn random_baseline() -> Self {
        ParentSelectionPolicy { beta: 0.0, epsilon: 0.0, max_parents: 2 }
    }
}


#[derive(Debug, Clone)]
pub struct SelectionResult {
    pub parents: Vec<String>,
    pub consensus_parents: usize,
    pub decoy_parents: usize,
}

pub fn select_parents(
    dag: &DAG,
    conflict_sets: &HashMap<(String, u64), Vec<String>>,
    stake_weights: &HashMap<String, f64>,
    total_stake: f64,
    decoy_pool: &mut DecoyPool,
    policy: &ParentSelectionPolicy,
    rng_seed: u64,
) -> SelectionResult {
    let all_tips = dag.get_tips();

    if all_tips.is_empty() {
        return SelectionResult { parents: vec![], consensus_parents: 0, decoy_parents: 0 };
    }

    let losers = compute_losers(&all_tips, dag, conflict_sets, stake_weights, total_stake);

    let candidates: Vec<String> = all_tips.iter()
        .filter(|id| !losers.contains(*id))
        .cloned()
        .collect();

    let candidates = if candidates.is_empty() { all_tips.clone() } else { candidates };

    let selected = weighted_select_with_bias(
        dag, &candidates, stake_weights, total_stake,
        policy.beta, policy.max_parents, rng_seed,
    );

    let consensus_count = selected.len();

    let (final_parents, decoy_count) = apply_privacy_noise(
        selected, decoy_pool, policy.epsilon, policy.max_parents, rng_seed,
    );

    SelectionResult {
        parents: final_parents,
        consensus_parents: consensus_count.saturating_sub(decoy_count),
        decoy_parents: decoy_count,
    }
}


fn compute_losers(
    tips: &[String],
    dag: &DAG,
    conflict_sets: &HashMap<(String, u64), Vec<String>>,
    stake_weights: &HashMap<String, f64>,
    total_stake: f64,
) -> std::collections::HashSet<String> {
    let mut losers = std::collections::HashSet::new();

    for ids in conflict_sets.values() {
        if ids.len() < 2 { continue; }

        let conflict_tips: Vec<&String> = ids.iter()
            .filter(|id| tips.contains(id))
            .collect();

        if conflict_tips.len() < 2 { continue; }

        let scores: Vec<(String, f64)> = conflict_tips.iter()
            .filter_map(|id| {
                let tx = dag.get_transaction(id)?;
                let stake = stake_weights.get(&tx.sender).copied().unwrap_or(0.0);
                let ratio = if total_stake > 0.0 {
                    (stake / total_stake).clamp(0.0, 1.0)
                } else { 0.0 };
                let multiplier = 1.0 + ratio * 2.0;
                Some(((*id).clone(), tx.weight as f64 * multiplier))
            })
            .collect();

        if scores.is_empty() { continue; }

        let winner = scores.iter()
            .max_by(|(id_a, sa), (id_b, sb)| {
                sa.partial_cmp(sb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| id_b.cmp(id_a))
            })
            .map(|(id, _)| id.clone());

        if let Some(winner_id) = winner {
            for (id, _) in &scores {
                if id != &winner_id {
                    losers.insert(id.clone());
                }
            }
        }
    }

    losers
}

fn weighted_select_with_bias(
    dag: &DAG,
    candidates: &[String],
    stake_weights: &HashMap<String, f64>,
    total_stake: f64,
    beta: f64,
    max_count: usize,
    seed: u64,
) -> Vec<String> {
    if candidates.is_empty() { return vec![]; }
    if candidates.len() == 1 { return vec![candidates[0].clone()]; }

    let mut scored: Vec<(String, f64)> = candidates.iter()
        .filter_map(|id| {
            let tx = dag.get_transaction(id)?;
            let stake = stake_weights.get(&tx.sender).copied().unwrap_or(0.0);
            let ratio = if total_stake > 0.0 {
                (stake / total_stake).clamp(0.0, 1.0)
            } else { 0.0 };
            let multiplier = 1.0 + ratio * 2.0;
            let score = tx.weight as f64 * multiplier;
            Some((id.clone(), score.max(1e-9)))
        })
        .collect();

    if scored.is_empty() {
        return candidates[..max_count.min(candidates.len())].to_vec();
    }

    if beta >= 1.0 {
        scored.sort_by(|(id_a, sa), (id_b, sb)| {
            sb.partial_cmp(sa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| id_b.cmp(id_a))
        });
        return scored.into_iter().take(max_count).map(|(id, _)| id).collect();
    }

    if beta <= 0.0 {
        let mut items: Vec<String> = scored.into_iter().map(|(id, _)| id).collect();
        let mut rng = if seed == 0 { 12345 } else { seed };
        for i in (1..items.len()).rev() {
            rng = xorshift64(rng);
            let j = (rng as usize) % (i + 1);
            items.swap(i, j);
        }
        return items.into_iter().take(max_count).collect();
    }

    let mut rng = if seed == 0 { 12345 } else { seed };
    let mut keyed: Vec<(String, f64)> = scored.iter()
        .map(|(id, score)| {
            rng = xorshift64(rng);
            let u = ((rng >> 11) as f64) / (((1u64 << 53) - 1) as f64);
            let u = u.max(1e-15);
            let w = score.powf(beta);
            let key = -u.ln() / w;
            (id.clone(), key)
        })
        .collect();

    keyed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    keyed.into_iter().take(max_count).map(|(id, _)| id).collect()
}

fn apply_privacy_noise(
    mut selected: Vec<String>,
    decoy_pool: &mut DecoyPool,
    epsilon: f64,
    max_parents: usize,
    seed: u64,
) -> (Vec<String>, usize) {
    if epsilon <= 0.0 || selected.is_empty() || decoy_pool.size() == 0 {
        return (selected, 0);
    }

    let roll = (xorshift64(seed.wrapping_add(42)) as f64) / (u64::MAX as f64);
    if roll >= epsilon {
        return (selected, 0);
    }

    let decoys = decoy_pool.sample(1, &selected);
    if decoys.is_empty() {
        return (selected, 0);
    }

    let decoy = decoys.into_iter().next().unwrap();

    if selected.len() < max_parents {
        selected.push(decoy);
    } else {
        let last = selected.len() - 1;
        selected[last] = decoy;
    }

    (selected, 1)
}

fn xorshift64(mut x: u64) -> u64 {
    if x == 0 { x = 12345; }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::DAG;
    use crate::transaction::TransactionVertex;
    use crate::privacy::DecoyPool;

    fn make_tx(tx_id: &str, sender: &str, weight: u64) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            sender.to_string(), "bob".to_string(),
            10, 1, 1000, "pk".to_string(), vec![],
        );
        tx.tx_id = tx_id.to_string();
        tx.weight = weight;
        tx
    }

    fn empty_pool() -> DecoyPool { DecoyPool::new(50) }
    fn empty_conflicts() -> HashMap<(String, u64), Vec<String>> { HashMap::new() }

    #[test]
    fn test_empty_dag_returns_empty() {
        let dag = DAG::new();
        let policy = ParentSelectionPolicy::default();
        let result = select_parents(
            &dag, &empty_conflicts(), &HashMap::new(), 0.0,
            &mut empty_pool(), &policy, 42,
        );
        assert!(result.parents.is_empty());
    }

    #[test]
    fn test_single_tip_returns_it() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx1", "alice", 5)).unwrap();
        let policy = ParentSelectionPolicy { epsilon: 0.0, ..Default::default() };
        let result = select_parents(
            &dag, &empty_conflicts(), &HashMap::new(), 0.0,
            &mut empty_pool(), &policy, 42,
        );
        assert_eq!(result.parents, vec!["tx1"]);
    }

    #[test]
    fn test_result_at_most_max_parents() {
        let mut dag = DAG::new();
        for i in 0..10 {
            dag.add_transaction(make_tx(&format!("tx{}", i), "alice", i as u64 + 1)).unwrap();
        }
        let policy = ParentSelectionPolicy::default();
        let result = select_parents(
            &dag, &empty_conflicts(), &HashMap::new(), 0.0,
            &mut empty_pool(), &policy, 42,
        );
        assert!(result.parents.len() <= policy.max_parents);
    }

    #[test]
    fn test_no_duplicates() {
        let mut dag = DAG::new();
        for i in 0..5 {
            dag.add_transaction(make_tx(&format!("tx{}", i), "alice", i as u64 + 1)).unwrap();
        }
        let policy = ParentSelectionPolicy::default();
        let result = select_parents(
            &dag, &empty_conflicts(), &HashMap::new(), 0.0,
            &mut empty_pool(), &policy, 42,
        );
        let unique: std::collections::HashSet<_> = result.parents.iter().collect();
        assert_eq!(unique.len(), result.parents.len());
    }

    #[test]
    fn test_does_not_select_loser() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("winner", "alice", 10)).unwrap();
        dag.add_transaction(make_tx("loser",  "alice",  2)).unwrap();

        let mut conflicts = HashMap::new();
        conflicts.insert(
            ("alice".to_string(), 1u64),
            vec!["winner".to_string(), "loser".to_string()],
        );

        let policy = ParentSelectionPolicy { beta: 1.0, epsilon: 0.0, max_parents: 1 };
        let result = select_parents(
            &dag, &conflicts, &HashMap::new(), 0.0,
            &mut empty_pool(), &policy, 42,
        );

        assert!(result.parents.contains(&"winner".to_string()));
        assert!(!result.parents.contains(&"loser".to_string()));
    }

    #[test]
    fn test_fallback_when_all_losers() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("loser1", "alice", 3)).unwrap();
        dag.add_transaction(make_tx("loser2", "alice", 3)).unwrap();

        let mut conflicts = HashMap::new();
        conflicts.insert(
            ("alice".to_string(), 1u64),
            vec!["loser1".to_string(), "loser2".to_string(), "winner_not_tip".to_string()],
        );

        let policy = ParentSelectionPolicy { beta: 1.0, epsilon: 0.0, max_parents: 1 };
        let result = select_parents(
            &dag, &conflicts, &HashMap::new(), 0.0,
            &mut empty_pool(), &policy, 42,
        );
        assert!(!result.parents.is_empty());
    }

    #[test]
    fn test_greedy_prefers_heaviest() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("heavy", "alice", 100)).unwrap();
        dag.add_transaction(make_tx("light", "bob",     1)).unwrap();

        let policy = ParentSelectionPolicy { beta: 1.0, epsilon: 0.0, max_parents: 1 };
        let result = select_parents(
            &dag, &empty_conflicts(), &HashMap::new(), 0.0,
            &mut empty_pool(), &policy, 42,
        );
        assert_eq!(result.parents, vec!["heavy"]);
    }

    #[test]
    fn test_no_noise_when_epsilon_zero() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tip1", "alice", 5)).unwrap();

        let mut pool = DecoyPool::new(50);
        for i in 0..20 { pool.record(format!("old_tx_{}", i)); }

        let policy = ParentSelectionPolicy { beta: 1.0, epsilon: 0.0, max_parents: 2 };
        let result = select_parents(
            &dag, &empty_conflicts(), &HashMap::new(), 0.0,
            &mut pool, &policy, 42,
        );
        assert_eq!(result.decoy_parents, 0);
    }

    #[test]
    fn test_consensus_mode_no_noise_picks_heaviest() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx1", "alice", 10)).unwrap();
        dag.add_transaction(make_tx("tx2", "bob",    5)).unwrap();

        let mut pool = DecoyPool::new(50);
        for i in 0..10 { pool.record(format!("old_{}", i)); }

        let policy = ParentSelectionPolicy { max_parents: 1, ..ParentSelectionPolicy::consensus_mode() };
        let result = select_parents(
            &dag, &empty_conflicts(), &HashMap::new(), 0.0,
            &mut pool, &policy, 42,
        );
        assert_eq!(result.decoy_parents, 0);
        assert_eq!(result.parents, vec!["tx1"]);
    }

    #[test]
    fn test_result_counts_consistent() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx1", "alice", 5)).unwrap();
        dag.add_transaction(make_tx("tx2", "bob",   3)).unwrap();

        let policy = ParentSelectionPolicy::default();
        let result = select_parents(
            &dag, &empty_conflicts(), &HashMap::new(), 0.0,
            &mut empty_pool(), &policy, 42,
        );
        assert_eq!(
            result.consensus_parents + result.decoy_parents,
            result.parents.len(),
        );
    }

    #[test]
    fn test_same_seed_same_result() {
        let mut dag = DAG::new();
        for i in 0..5 {
            dag.add_transaction(make_tx(&format!("tx{}", i), "alice", i as u64 + 1)).unwrap();
        }
        let policy = ParentSelectionPolicy { epsilon: 0.0, ..Default::default() };

        let r1 = select_parents(
            &dag, &empty_conflicts(), &HashMap::new(), 0.0,
            &mut empty_pool(), &policy, 999,
        );
        let r2 = select_parents(
            &dag, &empty_conflicts(), &HashMap::new(), 0.0,
            &mut empty_pool(), &policy, 999,
        );
        assert_eq!(r1.parents, r2.parents);
    }

    #[test]
    fn test_policy_presets_valid() {
        for p in [
            ParentSelectionPolicy::default(),
            ParentSelectionPolicy::consensus_mode(),
            ParentSelectionPolicy::privacy_mode(),
            ParentSelectionPolicy::random_baseline(),
        ] {
            assert!(p.beta >= 0.0 && p.beta <= 1.0);
            assert!(p.epsilon >= 0.0 && p.epsilon <= 1.0);
            assert!(p.max_parents > 0);
        }
    }
}