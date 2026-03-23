// branches/src/branch.rs

use ledger::dag::DAG;
use ledger::mempool::Mempool;
use ledger::state::LedgerState;
use ledger::transaction::TransactionVertex;
use ledger::validator::{ValidationResult, Validator};
use std::collections::HashMap;

pub struct Branch {
    pub branch_id: String,
    pub dag: DAG,
    pub state: LedgerState,
    pub mempool: Mempool,
    validator: Validator,
}

impl Branch {
    pub fn new(branch_id: &str) -> Self {
        Branch {
            branch_id: branch_id.to_string(),
            dag: DAG::new(),
            state: LedgerState::new(),
            mempool: Mempool::new(),
            validator: Validator::new(),
        }
    }

    pub fn submit_transaction(&mut self, tx: TransactionVertex) -> ValidationResult {
        let result = self.validator.validate_full(&tx, &self.dag, &mut self.state);
        if !result.ok {
            return result;
        }

        if self.mempool.has(&tx.tx_id) {
            return ValidationResult::err("duplicate_mempool", "already in mempool");
        }

        let tx_id = tx.tx_id.clone();
        self.mempool.add(tx.clone());

        if self.state.apply_transaction(&tx).is_err() {
            self.mempool.remove(&tx_id);
            return ValidationResult::err("state_error", "failed to apply");
        }

        if self.dag.add_transaction(tx).is_err() {
            self.mempool.remove(&tx_id);
            return ValidationResult::err("dag_error", "failed to add to DAG");
        }

        self.dag.propagate_weight(&tx_id);
        self.mempool.remove(&tx_id);

        ValidationResult::ok("accepted", &format!("accepted in branch {}", self.branch_id))
    }

    pub fn get_stats(&self) -> BranchStats {
        BranchStats {
            branch_id: self.branch_id.clone(),
            dag_stats: self.dag.stats(),
            balances: self.state.balances.clone(),
        }
    }

    pub fn snapshot(&self) -> BranchSnapshot {
        BranchSnapshot {
            branch_id: self.branch_id.clone(),
            balances: self.state.balances.clone(),
            nonces: self.state.nonces.clone(),
            applied_txs: self.state.applied_txs.iter().cloned().collect(),
            vertices: self.dag.vertices.iter()
                .map(|(id, tx)| (id.clone(), tx.clone()))
                .collect(),
        }
    }

    pub fn mempool_size(&self) -> usize {
        self.mempool.size()
    }
}

pub struct BranchStats {
    pub branch_id: String,
    pub dag_stats: ledger::dag::DagStats,
    pub balances: HashMap<String, u64>,
}

pub struct BranchSnapshot {
    pub branch_id: String,
    pub balances: HashMap<String, u64>,
    pub nonces: HashMap<String, u64>,
    pub applied_txs: Vec<String>,
    pub vertices: HashMap<String, TransactionVertex>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_branch_creation() {
        let branch = Branch::new("A");
        assert_eq!(branch.branch_id, "A");
        assert_eq!(branch.mempool_size(), 0);
    }

    #[test]
    fn test_branch_stats() {
        let branch = Branch::new("A");
        let stats = branch.get_stats();
        assert_eq!(stats.branch_id, "A");
        assert_eq!(stats.dag_stats.total_vertices, 0);
    }

    #[test]
    fn test_branch_snapshot_empty() {
        let branch = Branch::new("A");
        let snap = branch.snapshot();
        assert_eq!(snap.branch_id, "A");
        assert!(snap.vertices.is_empty());
    }

    #[test]
    fn test_two_branches_independent() {
        let mut branch_a = Branch::new("A");
        let mut branch_b = Branch::new("B");

        branch_a.state.credit("alice", 1000);
        branch_b.state.credit("bob", 1000);

        assert_eq!(branch_a.state.balances.get("alice"), Some(&1000));
        assert_eq!(branch_b.state.balances.get("bob"), Some(&1000));
        assert_eq!(branch_a.state.balances.get("bob"), None);
        assert_eq!(branch_b.state.balances.get("alice"), None);
    }
}