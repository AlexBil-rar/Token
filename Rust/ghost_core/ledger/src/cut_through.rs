// ledger/src/cut_through.rs

use std::collections::HashSet;
use crate::dag::DAG;
use crate::transaction::TransactionVertex;
use crypto::commitments::{Commitment, BlindingFactor};


#[derive(Debug, Default)]
pub struct CutThroughResult {
    pub removed_count: usize,
    pub retained_kernels: usize,
}

#[derive(Debug, Clone)]
pub struct TxKernel {
    pub tx_id: String,
    pub excess_commitment: Option<String>,
    pub excess_signature: Option<String>,
}

impl TxKernel {
    pub fn from_tx(tx: &TransactionVertex) -> Self {
        TxKernel {
            tx_id: tx.tx_id.clone(),
            excess_commitment: tx.excess_commitment.clone(),
            excess_signature: tx.excess_signature.clone(),
        }
    }
}

pub struct CutThroughPruner {
    pub kernels: Vec<TxKernel>,
}

impl CutThroughPruner {
    pub fn new() -> Self {
        CutThroughPruner { kernels: vec![] }
    }

    pub fn find_cut_through_candidates(dag: &DAG) -> HashSet<String> {
        let tips: HashSet<String> = dag.get_tips().into_iter().collect();
        let mut candidates = HashSet::new();

        for (tx_id, tx) in &dag.vertices {
            if tips.contains(tx_id) { continue; }

            if !matches!(tx.status, crate::transaction::TxStatus::Confirmed) { continue; }

            let children = dag.children_map.get(tx_id);
            let has_children = children.map(|c| !c.is_empty()).unwrap_or(false);
            if !has_children { continue; }

            candidates.insert(tx_id.clone());
        }

        candidates
    }

    pub fn apply(&mut self, dag: &mut DAG) -> CutThroughResult {
        let candidates = Self::find_cut_through_candidates(dag);
        let mut removed = 0;

        for tx_id in &candidates {
            if let Some(tx) = dag.vertices.get(tx_id) {
                self.kernels.push(TxKernel::from_tx(tx));
            }

            if dag.vertices.remove(tx_id).is_some() {
                dag.tips.remove(tx_id);
                removed += 1;
            }
        }

        CutThroughResult {
            removed_count: removed,
            retained_kernels: self.kernels.len(),
        }
    }

    pub fn validate_kernel_sum(&self) -> bool {
        use curve25519_dalek::ristretto::CompressedRistretto;
        use curve25519_dalek::ristretto::RistrettoPoint;
    
        if self.kernels.is_empty() {
            return true;
        }
    
        let mut sum = RistrettoPoint::default();
    
        for kernel in &self.kernels {
            let hex = match &kernel.excess_commitment {
                Some(h) => h,
                None => return false,
            };
    
            let bytes = match hex::decode(hex) {
                Ok(b) if b.len() == 32 => b,
                _ => return false,
            };
    
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
    
            let point = match CompressedRistretto(arr).decompress() {
                Some(p) => p,
                None => return false,
            };
    
            sum += point;
        }
    
        sum != RistrettoPoint::default()
    }

    pub fn kernel_count(&self) -> usize {
        self.kernels.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::DAG;
    use crate::transaction::{TransactionVertex, TxStatus};

    fn make_tx(tx_id: &str, parents: Vec<String>, status: TxStatus) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            "alice".to_string(), "bob".to_string(),
            10, 1, 1000, "pk".to_string(), parents,
        );
        tx.tx_id = tx_id.to_string();
        tx.status = status;
        tx
    }

    fn make_confirmed(tx_id: &str, parents: Vec<String>) -> TransactionVertex {
        make_tx(tx_id, parents, TxStatus::Confirmed)
    }

    fn make_pending(tx_id: &str, parents: Vec<String>) -> TransactionVertex {
        make_tx(tx_id, parents, TxStatus::Pending)
    }

    #[test]
    fn test_cut_through_removes_intermediate() {
        let mut dag = DAG::new();
        dag.add_transaction(make_confirmed("tx_a", vec![])).unwrap();
        dag.add_transaction(make_confirmed("tx_b", vec!["tx_a".to_string()])).unwrap();
        dag.children_map.entry("tx_a".to_string()).or_default()
            .insert("tx_b".to_string());

        let candidates = CutThroughPruner::find_cut_through_candidates(&dag);
        assert!(candidates.contains("tx_a"));
        assert!(!candidates.contains("tx_b"));
    }

    #[test]
    fn test_cut_through_preserves_tips() {
        let mut dag = DAG::new();
        dag.add_transaction(make_confirmed("tx_a", vec![])).unwrap();
        dag.add_transaction(make_confirmed("tx_b", vec!["tx_a".to_string()])).unwrap();
        dag.children_map.entry("tx_a".to_string()).or_default()
            .insert("tx_b".to_string());

        let mut pruner = CutThroughPruner::new();
        let result = pruner.apply(&mut dag);

        assert!(dag.vertices.contains_key("tx_b"));
        assert!(!dag.vertices.contains_key("tx_a"));
        assert_eq!(result.removed_count, 1);
    }

    #[test]
    fn test_cut_through_saves_kernel() {
        let mut dag = DAG::new();
        let mut tx_a = make_confirmed("tx_a", vec![]);
        tx_a.excess_commitment = Some("aabbcc".to_string());
        tx_a.excess_signature = Some("ddeeff".to_string());
        dag.add_transaction(tx_a).unwrap();
        dag.add_transaction(make_confirmed("tx_b", vec!["tx_a".to_string()])).unwrap();
        dag.children_map.entry("tx_a".to_string()).or_default()
            .insert("tx_b".to_string());

        let mut pruner = CutThroughPruner::new();
        pruner.apply(&mut dag);

        assert_eq!(pruner.kernel_count(), 1);
        assert_eq!(pruner.kernels[0].tx_id, "tx_a");
        assert!(pruner.kernels[0].excess_commitment.is_some());
    }

    #[test]
    fn test_cut_through_skips_pending() {
        let mut dag = DAG::new();
        dag.add_transaction(make_pending("tx_a", vec![])).unwrap();
        dag.add_transaction(make_confirmed("tx_b", vec!["tx_a".to_string()])).unwrap();
        dag.children_map.entry("tx_a".to_string()).or_default()
            .insert("tx_b".to_string());

        let candidates = CutThroughPruner::find_cut_through_candidates(&dag);
        assert!(!candidates.contains("tx_a"));
    }

    #[test]
    fn test_empty_dag_no_candidates() {
        let dag = DAG::new();
        let candidates = CutThroughPruner::find_cut_through_candidates(&dag);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_kernel_sum_valid_when_all_have_excess() {
        use crypto::commitments::{Commitment, BlindingFactor};
    
        let blinding = BlindingFactor::random();
        let commitment = Commitment::commit(100, &blinding);
    
        let mut pruner = CutThroughPruner::new();
        pruner.kernels.push(TxKernel {
            tx_id: "tx1".to_string(),
            excess_commitment: Some(commitment.point_hex.clone()),
            excess_signature: Some("ccdd".to_string()),
        });
        assert!(pruner.validate_kernel_sum());
    }

    #[test]
    fn test_kernel_sum_invalid_when_missing_excess() {
        let mut pruner = CutThroughPruner::new();
        pruner.kernels.push(TxKernel {
            tx_id: "tx1".to_string(),
            excess_commitment: None,
            excess_signature: None,
        });
        assert!(!pruner.validate_kernel_sum());
    }
}