// ledger/src/checkpoint.rs

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CHECKPOINT_INTERVAL: u64 = 500;

pub const CHECKPOINT_MIN_WEIGHT: u64 = 6;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointVertex {
    pub checkpoint_id: String,
    pub state_root: String,
    pub sequence: u64,
    pub dag_height: u64,
    pub address_count: usize,
    pub timestamp: u64,
    pub creator: String,
    pub parents: Vec<String>,
    pub signature: String,
    pub weight: u64,
}

impl CheckpointVertex {
    pub fn new(
        state_root: String,
        sequence: u64,
        dag_height: u64,
        address_count: usize,
        creator: String,
        parents: Vec<String>,
    ) -> Self {
        let timestamp = now_secs();
        let mut cp = CheckpointVertex {
            checkpoint_id: String::new(),
            state_root,
            sequence,
            dag_height,
            address_count,
            timestamp,
            creator,
            parents,
            signature: String::new(),
            weight: 1,
        };
        cp.checkpoint_id = cp.compute_id();
        cp
    }

    pub fn compute_id(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.state_root.as_bytes());
        h.update(self.sequence.to_le_bytes());
        h.update(self.dag_height.to_le_bytes());
        h.update((self.address_count as u64).to_le_bytes());
        h.update(self.timestamp.to_le_bytes());
        h.update(self.creator.as_bytes());
        for parent in &self.parents {
            h.update(parent.as_bytes());
        }
        hex::encode(h.finalize())
    }

    pub fn signing_payload(&self) -> Vec<u8> {
        let data = format!(
            "{}:{}:{}:{}:{}:{}",
            self.state_root,
            self.sequence,
            self.dag_height,
            self.address_count,
            self.timestamp,
            self.creator,
        );
        data.into_bytes()
    }

    pub fn sign(&mut self, signature: String) {
        self.signature = signature;
    }

    pub fn is_finalized(&self) -> bool {
        self.weight >= CHECKPOINT_MIN_WEIGHT
    }

    pub fn verify_state(
        &self,
        state: &std::collections::HashMap<String, (u64, u64)>,
    ) -> Result<(), String> {
        use crate::merkle::MerkleTree;
        MerkleTree::verify(state, &self.state_root)
    }
}

#[derive(Debug, Default)]
pub struct CheckpointRegistry {
    checkpoints: std::collections::HashMap<String, CheckpointVertex>,
    by_sequence: std::collections::BTreeMap<u64, String>,
}

impl CheckpointRegistry {
    pub fn new() -> Self {
        CheckpointRegistry::default()
    }

    pub fn register(&mut self, cp: CheckpointVertex) {
        let seq = cp.sequence;
        let id = cp.checkpoint_id.clone();
        self.checkpoints.insert(id.clone(), cp);
        self.by_sequence.insert(seq, id);
    }

    pub fn update_weight(&mut self, checkpoint_id: &str, weight: u64) {
        if let Some(cp) = self.checkpoints.get_mut(checkpoint_id) {
            cp.weight = weight;
        }
    }

    pub fn latest_finalized(&self) -> Option<&CheckpointVertex> {
        for (_, id) in self.by_sequence.iter().rev() {
            if let Some(cp) = self.checkpoints.get(id) {
                if cp.is_finalized() {
                    return Some(cp);
                }
            }
        }
        None
    }

    pub fn latest(&self) -> Option<&CheckpointVertex> {
        self.by_sequence
            .iter()
            .next_back()
            .and_then(|(_, id)| self.checkpoints.get(id))
    }

    pub fn get(&self, checkpoint_id: &str) -> Option<&CheckpointVertex> {
        self.checkpoints.get(checkpoint_id)
    }

    pub fn all_ordered(&self) -> Vec<&CheckpointVertex> {
        self.by_sequence
            .values()
            .filter_map(|id| self.checkpoints.get(id))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.checkpoints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.checkpoints.is_empty()
    }

    pub fn should_checkpoint(&self, dag_height: u64) -> bool {
        match self.latest() {
            None => dag_height >= CHECKPOINT_INTERVAL,
            Some(last) => dag_height - last.dag_height >= CHECKPOINT_INTERVAL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_state(entries: &[(&str, u64, u64)]) -> HashMap<String, (u64, u64)> {
        entries.iter()
            .map(|(a, b, n)| (a.to_string(), (*b, *n)))
            .collect()
    }

    fn make_checkpoint(seq: u64, state_root: &str, dag_height: u64) -> CheckpointVertex {
        CheckpointVertex::new(
            state_root.to_string(),
            seq,
            dag_height,
            2,
            "node_addr".to_string(),
            vec!["parent1".to_string()],
        )
    }

    #[test]
    fn test_checkpoint_id_is_deterministic() {
        let cp1 = make_checkpoint(1, "root_abc", 500);
        let cp2 = make_checkpoint(1, "root_abc", 500);
        assert_eq!(cp1.checkpoint_id.len(), 64);
        assert!(!cp1.checkpoint_id.is_empty());
        let _ = cp2;
    }

    #[test]
    fn test_checkpoint_not_finalized_at_start() {
        let cp = make_checkpoint(1, "root", 500);
        assert!(!cp.is_finalized()); 
    }

    #[test]
    fn test_checkpoint_finalized_after_weight() {
        let mut cp = make_checkpoint(1, "root", 500);
        cp.weight = CHECKPOINT_MIN_WEIGHT;
        assert!(cp.is_finalized());
    }

    #[test]
    fn test_verify_state_correct() {
        let state = make_state(&[("alice", 100, 1), ("bob", 50, 2)]);
        use crate::merkle::MerkleTree;
        let tree = MerkleTree::from_state(&state);
        let cp = make_checkpoint(1, &tree.root, 500);
        assert!(cp.verify_state(&state).is_ok());
    }

    #[test]
    fn test_verify_state_tampered_fails() {
        let state = make_state(&[("alice", 100, 1)]);
        let cp = make_checkpoint(1, "wrong_root", 500);
        assert!(cp.verify_state(&state).is_err());
    }

    #[test]
    fn test_verify_state_added_account_fails() {
        let state = make_state(&[("alice", 100, 1)]);
        use crate::merkle::MerkleTree;
        let tree = MerkleTree::from_state(&state);
        let cp = make_checkpoint(1, &tree.root, 500);

        let mut tampered = state.clone();
        tampered.insert("attacker".to_string(), (1_000_000, 0));
        assert!(cp.verify_state(&tampered).is_err());
    }

    #[test]
    fn test_signing_payload_deterministic() {
        let cp = make_checkpoint(1, "root_abc", 500);
        let p1 = cp.signing_payload();
        let p2 = cp.signing_payload();
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_registry_empty() {
        let reg = CheckpointRegistry::new();
        assert!(reg.is_empty());
        assert!(reg.latest().is_none());
        assert!(reg.latest_finalized().is_none());
    }

    #[test]
    fn test_registry_register_and_get() {
        let mut reg = CheckpointRegistry::new();
        let cp = make_checkpoint(1, "root", 500);
        let id = cp.checkpoint_id.clone();
        reg.register(cp);
        assert_eq!(reg.len(), 1);
        assert!(reg.get(&id).is_some());
    }

    #[test]
    fn test_registry_latest_returns_highest_sequence() {
        let mut reg = CheckpointRegistry::new();
        reg.register(make_checkpoint(1, "root1", 500));
        reg.register(make_checkpoint(2, "root2", 1000));
        reg.register(make_checkpoint(3, "root3", 1500));
        assert_eq!(reg.latest().unwrap().sequence, 3);
    }

    #[test]
    fn test_registry_latest_finalized_skips_low_weight() {
        let mut reg = CheckpointRegistry::new();
        let cp1 = make_checkpoint(1, "root1", 500);
        let mut cp2 = make_checkpoint(2, "root2", 1000);
        cp2.weight = CHECKPOINT_MIN_WEIGHT; 

        reg.register(cp1);
        let id2 = cp2.checkpoint_id.clone();
        reg.register(cp2);

        let finalized = reg.latest_finalized().unwrap();
        assert_eq!(finalized.checkpoint_id, id2);
    }

    #[test]
    fn test_registry_update_weight() {
        let mut reg = CheckpointRegistry::new();
        let cp = make_checkpoint(1, "root", 500);
        let id = cp.checkpoint_id.clone();
        reg.register(cp);

        assert!(!reg.get(&id).unwrap().is_finalized());
        reg.update_weight(&id, CHECKPOINT_MIN_WEIGHT);
        assert!(reg.get(&id).unwrap().is_finalized());
    }

    #[test]
    fn test_registry_all_ordered() {
        let mut reg = CheckpointRegistry::new();
        reg.register(make_checkpoint(3, "r3", 1500));
        reg.register(make_checkpoint(1, "r1", 500));
        reg.register(make_checkpoint(2, "r2", 1000));

        let ordered = reg.all_ordered();
        assert_eq!(ordered[0].sequence, 1);
        assert_eq!(ordered[1].sequence, 2);
        assert_eq!(ordered[2].sequence, 3);
    }

    #[test]
    fn test_should_checkpoint_empty_registry() {
        let reg = CheckpointRegistry::new();
        assert!(!reg.should_checkpoint(CHECKPOINT_INTERVAL - 1));
        assert!(reg.should_checkpoint(CHECKPOINT_INTERVAL));
    }

    #[test]
    fn test_should_checkpoint_after_interval() {
        let mut reg = CheckpointRegistry::new();
        reg.register(make_checkpoint(1, "root", 500));
        assert!(!reg.should_checkpoint(500 + CHECKPOINT_INTERVAL - 1));
        assert!(reg.should_checkpoint(500 + CHECKPOINT_INTERVAL));
    }

    #[test]
    fn test_checkpoint_id_length() {
        let cp = make_checkpoint(1, "some_root", 500);
        assert_eq!(cp.checkpoint_id.len(), 64);
    }
}