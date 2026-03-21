// branches/src/branch_manager.rs

use std::collections::HashMap;
use ledger::transaction::TransactionVertex;
use ledger::validator::ValidationResult;
use crate::branch::Branch;
use crate::coordinator::Coordinator;

pub struct BranchManager {
    pub branches: HashMap<String, Branch>,
    pub coordinator: Coordinator,
}

impl BranchManager {
    pub fn new() -> Self {
        BranchManager {
            branches: HashMap::new(),
            coordinator: Coordinator::new(),
        }
    }

    pub fn create_branch(&mut self, branch_id: &str) -> &Branch {
        self.branches.insert(branch_id.to_string(), Branch::new(branch_id));
        self.branches.get(branch_id).unwrap()
    }

    pub fn get_least_loaded_id(&self) -> Option<String> {
        self.branches
            .iter()
            .min_by_key(|(_, b)| b.mempool_size())
            .map(|(id, _)| id.clone())
    }

    pub fn submit_transaction(&mut self, tx: TransactionVertex) -> ValidationResult {
        let branch_id = match self.get_least_loaded_id() {
            Some(id) => id,
            None => return ValidationResult::err("no_branches", "no branches available"),
        };

        let result = self.branches
            .get_mut(&branch_id)
            .unwrap()
            .submit_transaction(tx);

        if result.ok {
            let branch_refs: Vec<&Branch> = self.branches.values().collect();
            self.coordinator.merge(&branch_refs);
        }

        result
    }

    pub fn credit(&mut self, address: &str, amount: u64) {
        for branch in self.branches.values_mut() {
            branch.state.credit(address, amount);
        }
    }

    pub fn get_stats(&self) -> ManagerStats {
        ManagerStats {
            branch_count: self.branches.len(),
            merge_count: self.coordinator.merge_count,
            branch_ids: self.branches.keys().cloned().collect(),
        }
    }
}

pub struct ManagerStats {
    pub branch_count: usize,
    pub merge_count: u64,
    pub branch_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_branch() {
        let mut manager = BranchManager::new();
        manager.create_branch("A");
        assert!(manager.branches.contains_key("A"));
    }

    #[test]
    fn test_get_least_loaded_empty() {
        let manager = BranchManager::new();
        assert!(manager.get_least_loaded_id().is_none());
    }

    #[test]
    fn test_get_least_loaded_single() {
        let mut manager = BranchManager::new();
        manager.create_branch("A");
        assert_eq!(manager.get_least_loaded_id(), Some("A".to_string()));
    }

    #[test]
    fn test_two_branches_independent() {
        let mut manager = BranchManager::new();
        manager.create_branch("A");
        manager.create_branch("B");

        manager.credit("alice", 1000);
        manager.credit("bob", 500);

        let stats = manager.get_stats();
        assert_eq!(stats.branch_count, 2);

        for branch in manager.branches.values() {
            assert_eq!(branch.state.balances.get("alice"), Some(&1000));
            assert_eq!(branch.state.balances.get("bob"), Some(&500));
        }
    }

    #[test]
    fn test_submit_no_branches_fails() {
        let mut manager = BranchManager::new();
        let mut tx = TransactionVertex::new(
            "alice".to_string(), "bob".to_string(),
            100, 1, 1000, "pk".to_string(), vec![],
        );
        tx.finalize();
        let result = manager.submit_transaction(tx);
        assert!(!result.ok);
        assert_eq!(result.code, "no_branches");
    }

    #[test]
    fn test_stats() {
        let mut manager = BranchManager::new();
        manager.create_branch("A");
        manager.create_branch("B");
        let stats = manager.get_stats();
        assert_eq!(stats.branch_count, 2);
        assert_eq!(stats.merge_count, 0);
    }
}