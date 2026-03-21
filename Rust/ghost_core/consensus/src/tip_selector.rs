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
}