// ledger/src/merkle.rs

use sha2::{Sha256, Digest};

#[derive(Debug, Clone)]
pub struct StateLeaf {
    pub address: String,
    pub balance: u64,
    pub nonce: u64,
}

impl StateLeaf {
    pub fn new(address: String, balance: u64, nonce: u64) -> Self {
        StateLeaf { address, balance, nonce }
    }

    pub fn hash(&self) -> Vec<u8> {
        let data = format!("{}:{}:{}", self.address, self.balance, self.nonce);
        let mut h = Sha256::new();
        h.update(data.as_bytes());
        h.finalize().to_vec()
    }
}

#[derive(Debug, Clone)]
pub struct MerkleTree {
    pub root: String,
    pub leaf_count: usize,
}

impl MerkleTree {
    pub fn build(leaves: &mut Vec<StateLeaf>) -> Self {
        if leaves.is_empty() {
            return MerkleTree {
                root: Self::empty_root(),
                leaf_count: 0,
            };
        }

        leaves.sort_by(|a, b| a.address.cmp(&b.address));

        let mut hashes: Vec<Vec<u8>> = leaves.iter().map(|l| l.hash()).collect();
        let leaf_count = hashes.len();

        while hashes.len() > 1 {
            hashes = Self::build_level(hashes);
        }

        MerkleTree {
            root: hex::encode(&hashes[0]),
            leaf_count,
        }
    }

    pub fn from_state(state: &std::collections::HashMap<String, (u64, u64)>) -> Self {
        let mut leaves: Vec<StateLeaf> = state
            .iter()
            .map(|(addr, (balance, nonce))| StateLeaf::new(addr.clone(), *balance, *nonce))
            .collect();

        Self::build(&mut leaves)
    }

    pub fn verify(
        state: &std::collections::HashMap<String, (u64, u64)>,
        expected_root: &str,
    ) -> Result<(), String> {
        let tree = Self::from_state(state);

        if tree.root == expected_root {
            Ok(())
        } else {
            Err(format!(
                "state root mismatch: computed={}, expected={}",
                tree.root, expected_root
            ))
        }
    }

    fn build_level(hashes: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
        let mut next_level = Vec::new();

        let mut i = 0;
        while i < hashes.len() {
            let left = &hashes[i];

            let right = if i + 1 < hashes.len() {
                &hashes[i + 1]
            } else {
                &hashes[i]
            };

            next_level.push(Self::hash_pair(left, right));
            i += 2;
        }

        next_level
    }

    fn hash_pair(left: &[u8], right: &[u8]) -> Vec<u8> {
        let mut h = Sha256::new();
        h.update(left);
        h.update(right);
        h.finalize().to_vec()
    }

    fn empty_root() -> String {
        let mut h = Sha256::new();
        h.update(b"ghostledger:empty_state");
        hex::encode(h.finalize())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StateCheckpoint {
    pub state_root: String,
    pub dag_height: u64,
    pub timestamp: u64,
    pub address_count: usize,
}

impl StateCheckpoint {
    pub fn new(state_root: String, dag_height: u64, address_count: usize) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        StateCheckpoint { state_root, dag_height, timestamp, address_count }
    }

    pub fn hash(&self) -> String {
        let data = format!(
            "{}:{}:{}:{}",
            self.state_root, self.dag_height, self.timestamp, self.address_count
        );
        let mut h = Sha256::new();
        h.update(data.as_bytes());
        hex::encode(h.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_state(entries: &[(&str, u64, u64)]) -> HashMap<String, (u64, u64)> {
        entries.iter()
            .map(|(addr, bal, nonce)| (addr.to_string(), (*bal, *nonce)))
            .collect()
    }

    #[test]
    fn test_empty_state_has_deterministic_root() {
        let state = HashMap::new();
        let t1 = MerkleTree::from_state(&state);
        let t2 = MerkleTree::from_state(&state);
        assert_eq!(t1.root, t2.root);
    }

    #[test]
    fn test_same_state_same_root() {
        let state = make_state(&[("alice", 100, 1), ("bob", 50, 2)]);
        let t1 = MerkleTree::from_state(&state);
        let t2 = MerkleTree::from_state(&state);
        assert_eq!(t1.root, t2.root);
    }

    #[test]
    fn test_different_state_different_root() {
        let state1 = make_state(&[("alice", 100, 1)]);
        let state2 = make_state(&[("alice", 101, 1)]); 
        let t1 = MerkleTree::from_state(&state1);
        let t2 = MerkleTree::from_state(&state2);
        assert_ne!(t1.root, t2.root);
    }

    #[test]
    fn test_order_independent_root() {
        let mut state1 = HashMap::new();
        state1.insert("alice".to_string(), (100u64, 1u64));
        state1.insert("bob".to_string(), (50u64, 2u64));
        state1.insert("carol".to_string(), (200u64, 3u64));

        let mut state2 = HashMap::new();
        state2.insert("carol".to_string(), (200u64, 3u64));
        state2.insert("alice".to_string(), (100u64, 1u64));
        state2.insert("bob".to_string(), (50u64, 2u64));

        let t1 = MerkleTree::from_state(&state1);
        let t2 = MerkleTree::from_state(&state2);
        assert_eq!(t1.root, t2.root);
    }

    #[test]
    fn test_single_entry_tree() {
        let state = make_state(&[("alice", 100, 1)]);
        let tree = MerkleTree::from_state(&state);
        assert_eq!(tree.leaf_count, 1);
        assert!(!tree.root.is_empty());
    }

    #[test]
    fn test_odd_number_of_leaves() {
        let state = make_state(&[("alice", 100, 1), ("bob", 50, 2), ("carol", 200, 3)]);
        let tree = MerkleTree::from_state(&state);
        assert_eq!(tree.leaf_count, 3);
        assert!(!tree.root.is_empty());
    }

    #[test]
    fn test_verify_correct_state() {
        let state = make_state(&[("alice", 100, 1), ("bob", 50, 2)]);
        let tree = MerkleTree::from_state(&state);
        assert!(MerkleTree::verify(&state, &tree.root).is_ok());
    }

    #[test]
    fn test_verify_tampered_state_fails() {
        let state = make_state(&[("alice", 100, 1), ("bob", 50, 2)]);
        let tree = MerkleTree::from_state(&state);

        let mut tampered = state.clone();
        tampered.insert("alice".to_string(), (9999, 1));

        let result = MerkleTree::verify(&tampered, &tree.root);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("mismatch"));
    }

    #[test]
    fn test_verify_added_address_fails() {
        let state = make_state(&[("alice", 100, 1)]);
        let tree = MerkleTree::from_state(&state);

        let mut extended = state.clone();
        extended.insert("attacker".to_string(), (1_000_000, 0));

        let result = MerkleTree::verify(&extended, &tree.root);
        assert!(result.is_err());
    }

    #[test]
    fn test_state_checkpoint_hash_deterministic() {
        let cp = StateCheckpoint {
            state_root: "abc123".to_string(),
            dag_height: 100,
            timestamp: 1_700_000_000,
            address_count: 42,
        };
        let h1 = cp.hash();
        let h2 = cp.hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_leaf_hash_includes_all_fields() {
        let l1 = StateLeaf::new("alice".to_string(), 100, 1);
        let l2 = StateLeaf::new("alice".to_string(), 101, 1); 
        let l3 = StateLeaf::new("alice".to_string(), 100, 2); 
        let l4 = StateLeaf::new("bob".to_string(), 100, 1);  

        assert_ne!(l1.hash(), l2.hash());
        assert_ne!(l1.hash(), l3.hash());
        assert_ne!(l1.hash(), l4.hash());
    }

    #[test]
    fn test_large_state_performance() {
        let state: HashMap<String, (u64, u64)> = (0..1000)
            .map(|i| (format!("addr_{:04}", i), (i as u64 * 100, i as u64)))
            .collect();

        let tree = MerkleTree::from_state(&state);
        assert_eq!(tree.leaf_count, 1000);
        assert!(MerkleTree::verify(&state, &tree.root).is_ok());
    }
}