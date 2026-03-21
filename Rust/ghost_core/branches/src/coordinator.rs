// branches/src/coordinator.rs

use std::collections::HashMap;
use ledger::state::LedgerState;
use crate::branch::Branch;

pub struct Coordinator {
    pub root_state: LedgerState,
    pub merge_count: u64,
}

impl Coordinator {
    pub fn new() -> Self {
        Coordinator {
            root_state: LedgerState::new(),
            merge_count: 0,
        }
    }

    pub fn quorum_size(total: usize) -> usize {
        total / 2 + 1
    }

    pub fn merge(&mut self, branches: &[&Branch]) -> &LedgerState {
        if branches.is_empty() {
            return &self.root_state;
        }

        let total = branches.len();
        let quorum = Self::quorum_size(total);

        let mut all_addresses: std::collections::HashSet<String> = std::collections::HashSet::new();
        for branch in branches {
            all_addresses.extend(branch.state.balances.keys().cloned());
        }

        let mut new_state = LedgerState::new();

        for address in &all_addresses {
            let balance_votes: Vec<u64> = branches.iter()
                .filter_map(|b| b.state.balances.get(address))
                .cloned()
                .collect();

            let nonce_votes: Vec<u64> = branches.iter()
                .filter_map(|b| b.state.nonces.get(address))
                .cloned()
                .collect();

            if !balance_votes.is_empty() {
                new_state.balances.insert(
                    address.clone(),
                    Self::quorum_value(&balance_votes, quorum),
                );
            }

            if !nonce_votes.is_empty() {
                new_state.nonces.insert(
                    address.clone(),
                    Self::quorum_value(&nonce_votes, quorum),
                );
            }
        }

        for branch in branches {
            new_state.applied_txs.extend(branch.state.applied_txs.iter().cloned());
        }

        self.root_state = new_state;
        self.merge_count += 1;
        &self.root_state
    }

    pub fn quorum_value(votes: &[u64], quorum: usize) -> u64 {
        let mut counts: HashMap<u64, usize> = HashMap::new();
        for &v in votes {
            *counts.entry(v).or_insert(0) += 1;
        }

        let mut sorted: Vec<(u64, usize)> = counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));

        for (value, count) in sorted {
            if count >= quorum {
                return value;
            }
        }

        *votes.iter().min().unwrap()
    }

    pub fn get_balance(&self, address: &str) -> u64 {
        *self.root_state.balances.get(address).unwrap_or(&0)
    }

    pub fn has_quorum(&self, branches: &[&Branch], address: &str) -> bool {
        let total = branches.len();
        let quorum = Self::quorum_size(total);

        let votes: Vec<u64> = branches.iter()
            .filter_map(|b| b.state.balances.get(address))
            .cloned()
            .collect();

        if votes.is_empty() {
            return false;
        }

        let mut counts: HashMap<u64, usize> = HashMap::new();
        for v in &votes {
            *counts.entry(*v).or_insert(0) += 1;
        }

        counts.values().any(|&count| count >= quorum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch::Branch;

    fn make_branch(id: &str, balances: Vec<(&str, u64)>, nonces: Vec<(&str, u64)>) -> Branch {
        let mut branch = Branch::new(id);
        for (addr, bal) in balances {
            branch.state.credit(addr, bal);
        }
        for (addr, nonce) in nonces {
            branch.state.nonces.insert(addr.to_string(), nonce);
        }
        branch
    }

    #[test]
    fn test_quorum_size_odd() {
        assert_eq!(Coordinator::quorum_size(5), 3);
        assert_eq!(Coordinator::quorum_size(3), 2);
    }

    #[test]
    fn test_quorum_size_even() {
        assert_eq!(Coordinator::quorum_size(4), 3);
        assert_eq!(Coordinator::quorum_size(2), 2);
    }

    #[test]
    fn test_quorum_value_clear_winner() {
        let votes = vec![900, 900, 900, 850, 800];
        assert_eq!(Coordinator::quorum_value(&votes, 3), 900);
    }

    #[test]
    fn test_quorum_value_fallback_to_min() {
        let votes = vec![900, 850, 800];
        assert_eq!(Coordinator::quorum_value(&votes, 2), 800);
    }

    #[test]
    fn test_quorum_value_two_branches_agree() {
        let votes = vec![500, 500];
        assert_eq!(Coordinator::quorum_value(&votes, 2), 500);
    }

    #[test]
    fn test_merge_balances() {
        let branch_a = make_branch("A", vec![("alice", 500)], vec![]);
        let branch_b = make_branch("B", vec![("bob", 300)], vec![]);

        let mut coord = Coordinator::new();
        coord.merge(&[&branch_a, &branch_b]);

        assert_eq!(coord.get_balance("alice"), 500);
        assert_eq!(coord.get_balance("bob"), 300);
    }

    #[test]
    fn test_merge_quorum_majority_wins() {
        let b1 = make_branch("A", vec![("alice", 900)], vec![]);
        let b2 = make_branch("B", vec![("alice", 900)], vec![]);
        let b3 = make_branch("C", vec![("alice", 900)], vec![]);
        let b4 = make_branch("D", vec![("alice", 900)], vec![]);
        let b5 = make_branch("E", vec![("alice", 500)], vec![]);

        let mut coord = Coordinator::new();
        coord.merge(&[&b1, &b2, &b3, &b4, &b5]);

        assert_eq!(coord.get_balance("alice"), 900);
    }

    #[test]
    fn test_merge_no_quorum_uses_min() {
        let b1 = make_branch("A", vec![("alice", 1000)], vec![]);
        let b2 = make_branch("B", vec![("alice", 800)], vec![]);
        let b3 = make_branch("C", vec![("alice", 600)], vec![]);

        let mut coord = Coordinator::new();
        coord.merge(&[&b1, &b2, &b3]);

        assert_eq!(coord.get_balance("alice"), 600);
    }

    #[test]
    fn test_merge_count_increments() {
        let branch = make_branch("A", vec![("alice", 100)], vec![]);
        let mut coord = Coordinator::new();
        coord.merge(&[&branch]);
        coord.merge(&[&branch]);
        assert_eq!(coord.merge_count, 2);
    }

    #[test]
    fn test_has_quorum_true() {
        let b1 = make_branch("A", vec![("alice", 900)], vec![]);
        let b2 = make_branch("B", vec![("alice", 900)], vec![]);
        let b3 = make_branch("C", vec![("alice", 850)], vec![]);

        let coord = Coordinator::new();
        assert!(coord.has_quorum(&[&b1, &b2, &b3], "alice"));
    }

    #[test]
    fn test_has_quorum_false() {
        let b1 = make_branch("A", vec![("alice", 900)], vec![]);
        let b2 = make_branch("B", vec![("alice", 800)], vec![]);
        let b3 = make_branch("C", vec![("alice", 700)], vec![]);

        let coord = Coordinator::new();
        assert!(!coord.has_quorum(&[&b1, &b2, &b3], "alice"));
    }
}