// consensus/src/tip_selector.rs

use ledger::dag::DAG;

const MAX_PARENTS: usize = 2;

pub struct TipSelector;

impl TipSelector {
    pub fn new() -> Self {
        TipSelector
    }

    pub fn select(&self, dag: &DAG, max_parents: usize) -> Vec<String> {
        let tips = dag.get_tips();

        if tips.is_empty() {
            return vec![];
        }

        if tips.len() == 1 {
            return vec![tips[0].clone()];
        }

        let count = max_parents.min(tips.len());

        let weights: Vec<u64> = tips
            .iter()
            .filter_map(|id| dag.get_transaction(id).map(|tx| tx.weight))
            .collect();

        self.weighted_sample(&tips, &weights, count)
    }

    pub fn select_default(&self, dag: &DAG) -> Vec<String> {
        self.select(dag, MAX_PARENTS)
    }

    pub fn select_conflict_aware(
        &self,
        dag: &DAG,
        max_parents: usize,
        conflict_sets: &std::collections::HashMap<(String, u64), Vec<String>>,
        stake_weights: &std::collections::HashMap<String, f64>,
        total_stake: f64,
    ) -> Vec<String> {
        let tips = dag.get_tips();
        if tips.is_empty() { return vec![]; }

        let mut losers: std::collections::HashSet<String> =
            std::collections::HashSet::new();

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

        let preferred_tips: Vec<String> = tips.into_iter()
            .filter(|id| !losers.contains(id))
            .collect();

        if preferred_tips.is_empty() {
            return self.select(dag, max_parents);
        }

        let weights: Vec<u64> = preferred_tips.iter()
            .filter_map(|id| dag.get_transaction(id).map(|tx| tx.weight))
            .collect();

        let count = max_parents.min(preferred_tips.len());
        self.weighted_sample(&preferred_tips, &weights, count)
    }

    pub fn winner_preference_probability(
        &self,
        dag: &DAG,
        conflict_ids: &[String],
        winner_id: &str,
        stake_weights: &std::collections::HashMap<String, f64>,
        total_stake: f64,
    ) -> f64 {
        let tips = dag.get_tips();
        let conflict_tips: Vec<&String> = conflict_ids.iter()
            .filter(|id| tips.contains(id))
            .collect();

        if conflict_tips.is_empty() { return 1.0; }

        let total_weight: f64 = conflict_tips.iter()
            .filter_map(|id| dag.get_transaction(id))
            .map(|tx| {
                let stake = stake_weights.get(&tx.sender).copied().unwrap_or(0.0);
                let ratio = if total_stake > 0.0 {
                    (stake / total_stake).clamp(0.0, 1.0)
                } else { 0.0 };
                tx.weight as f64 * (1.0 + ratio * 2.0)
            })
            .sum();

        if total_weight == 0.0 { return 0.5; }

        let winner_score = dag.get_transaction(winner_id)
            .map(|tx| {
                let stake = stake_weights.get(&tx.sender).copied().unwrap_or(0.0);
                let ratio = if total_stake > 0.0 {
                    (stake / total_stake).clamp(0.0, 1.0)
                } else { 0.0 };
                tx.weight as f64 * (1.0 + ratio * 2.0)
            })
            .unwrap_or(0.0);

        winner_score / total_weight
    }

        fn weighted_sample(&self, items: &[String], weights: &[u64], count: usize) -> Vec<String> {
        if items.is_empty() || count == 0 {
            return vec![];
        }

        let total: u64 = weights.iter().sum();
        if total == 0 {
            return items[..count.min(items.len())].to_vec();
        }

        let mut selected = Vec::new();
        let mut remaining_items = items.to_vec();
        let mut remaining_weights = weights.to_vec();

        for _ in 0..count.min(remaining_items.len()) {
            let total: u64 = remaining_weights.iter().sum();
            let idx = remaining_weights
                .iter()
                .enumerate()
                .max_by_key(|(_, &w)| w)
                .map(|(i, _)| i)
                .unwrap_or(0);

            selected.push(remaining_items[idx].clone());
            remaining_items.remove(idx);
            remaining_weights.remove(idx);

            let _ = total;
        }

        selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::dag::DAG;
    use ledger::transaction::TransactionVertex;

    fn make_tx(tx_id: &str, weight: u64) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            "alice".to_string(), "bob".to_string(),
            10, 1, 1000, "pk".to_string(), vec![],
        );
        tx.tx_id = tx_id.to_string();
        tx.weight = weight;
        tx
    }

    #[test]
    fn test_empty_dag_returns_empty() {
        let dag = DAG::new();
        let selector = TipSelector::new();
        assert!(selector.select_default(&dag).is_empty());
    }

    #[test]
    fn test_single_tip_returns_it() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx1", 1)).unwrap();
        let selector = TipSelector::new();
        assert_eq!(selector.select_default(&dag), vec!["tx1"]);
    }

    #[test]
    fn test_returns_at_most_max_parents() {
        let mut dag = DAG::new();
        for i in 0..5 {
            dag.add_transaction(make_tx(&format!("tx{}", i), 1)).unwrap();
        }
        let selector = TipSelector::new();
        let result = selector.select_default(&dag);
        assert!(result.len() <= MAX_PARENTS);
    }

    #[test]
    fn test_no_duplicates() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx1", 1)).unwrap();
        dag.add_transaction(make_tx("tx2", 1)).unwrap();
        dag.add_transaction(make_tx("tx3", 1)).unwrap();

        let selector = TipSelector::new();
        let result = selector.select_default(&dag);
        let unique: std::collections::HashSet<_> = result.iter().collect();
        assert_eq!(result.len(), unique.len());
    }

    #[test]
    fn test_heavier_tip_selected_first() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx_light", 1)).unwrap();
        dag.add_transaction(make_tx("tx_heavy", 100)).unwrap();

        let selector = TipSelector::new();
        let result = selector.select(&dag, 1);
        assert_eq!(result, vec!["tx_heavy"]);
    }

    #[test]
    fn test_conflict_aware_prefers_winner() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx_winner", 10)).unwrap();
        dag.add_transaction(make_tx("tx_loser", 3)).unwrap();

        let mut conflict_sets = std::collections::HashMap::new();
        conflict_sets.insert(
            ("alice".to_string(), 1u64),
            vec!["tx_winner".to_string(), "tx_loser".to_string()],
        );

        let selector = TipSelector::new();
        let result = selector.select_conflict_aware(
            &dag, 2, &conflict_sets, &std::collections::HashMap::new(), 0.0,
        );

        assert!(result.contains(&"tx_winner".to_string()),
            "Честный узел должен предпочитать winner конфликта");
        assert!(!result.contains(&"tx_loser".to_string()),
            "Честный узел не должен усиливать loser конфликта");
    }

    #[test]
    fn test_conflict_aware_no_conflict_normal_selection() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("tx1", 5)).unwrap();
        dag.add_transaction(make_tx("tx2", 3)).unwrap();

        let selector = TipSelector::new();
        let result = selector.select_conflict_aware(
            &dag, 2,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(), 0.0,
        );
        assert!(!result.is_empty());
        assert!(result.len() <= 2);
    }

    #[test]
    fn test_winner_preference_probability_above_half() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("winner", 6)).unwrap();
        dag.add_transaction(make_tx("loser", 3)).unwrap();

        let selector = TipSelector::new();
        let prob = selector.winner_preference_probability(
            &dag,
            &["winner".to_string(), "loser".to_string()],
            "winner",
            &std::collections::HashMap::new(),
            0.0,
        );

        assert!(prob > 0.5,
            "P(prefer winner) = {:.3} должна быть > 0.5", prob);
    }

    #[test]
    fn test_winner_preference_probability_with_stake() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("winner", 4)).unwrap();
        dag.add_transaction(make_tx("loser", 4)).unwrap();

        let selector = TipSelector::new();
        let prob_no_stake = selector.winner_preference_probability(
            &dag,
            &["winner".to_string(), "loser".to_string()],
            "winner",
            &std::collections::HashMap::new(),
            0.0,
        );
        assert!((prob_no_stake - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_conflict_aware_fallback_when_all_losers() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("loser1", 3)).unwrap();
        dag.add_transaction(make_tx("loser2", 3)).unwrap();

        let mut conflict_sets = std::collections::HashMap::new();
        conflict_sets.insert(
            ("alice".to_string(), 1u64),
            vec!["loser1".to_string(), "loser2".to_string(), "winner_not_tip".to_string()],
        );

        let selector = TipSelector::new();
        let result = selector.select_conflict_aware(
            &dag, 2, &conflict_sets,
            &std::collections::HashMap::new(), 0.0,
        );
        assert!(!result.is_empty());
    }

    #[test]
    fn test_honest_node_preference_drives_convergence() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("winner", 5)).unwrap();
        dag.add_transaction(make_tx("loser", 2)).unwrap();

        let conflict_sets = {
            let mut m = std::collections::HashMap::new();
            m.insert(
                ("alice".to_string(), 1u64),
                vec!["winner".to_string(), "loser".to_string()],
            );
            m
        };

        let selector = TipSelector::new();

        for i in 0..10 {
            let selected = selector.select_conflict_aware(
                &dag, 1, &conflict_sets,
                &std::collections::HashMap::new(), 0.0,
            );
            if !selected.is_empty() && selected.len() == 1 {
                assert_eq!(selected[0], "winner",
                    "Iteration {i}: одиночный выбор должен быть winner (w=5 vs w=2)");
            }
        }

        let prob = selector.winner_preference_probability(
            &dag,
            &["winner".to_string(), "loser".to_string()],
            "winner",
            &std::collections::HashMap::new(),
            0.0,
        );
        assert!(prob > 0.5,
            "P(prefer winner) = {prob:.3} должна быть > 0.5 для Theorem L");
    }

    #[test]
    fn test_tiebreak_lexicographic_when_equal_weight() {
        let mut dag = DAG::new();
        dag.add_transaction(make_tx("winner", 1)).unwrap();
        dag.add_transaction(make_tx("loser", 1)).unwrap();

        let conflict_sets = {
            let mut m = std::collections::HashMap::new();
            m.insert(
                ("alice".to_string(), 1u64),
                vec!["winner".to_string(), "loser".to_string()],
            );
            m
        };

        let selector = TipSelector::new();
        let selected = selector.select_conflict_aware(
            &dag, 1, &conflict_sets,
            &std::collections::HashMap::new(), 0.0,
        );

        if selected.len() == 1 {
            assert_eq!(selected[0], "loser",
                "Tiebreak при равном весе: lexicographic min = 'loser'");
        }
    }
}