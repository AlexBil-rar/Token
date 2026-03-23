// ledger/src/node.rs

use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use crate::dag::DAG;
use crate::mempool::Mempool;
use crate::state::LedgerState;
use crate::transaction::TransactionVertex;
use crate::validator::{ValidationResult, Validator};
use crate::pruner::Pruner;
use crate::anti_spam::{AntiSpamController, RateLimitResult};
use crate::merkle::{MerkleTree, StateCheckpoint};
use crate::checkpoint::{CheckpointVertex, CheckpointRegistry};

pub struct WalletInfo {
    pub address: String,
    pub public_key: String,
    pub private_key_hex: String,
}

#[derive(Debug, PartialEq)]
pub enum StakeResult {
    Registered { amount: u64 },
    InsufficientBalance,
    BelowMinimum { min: u64 },
    AlreadyStaking,
}

#[derive(Debug, Clone)]
pub struct NodeStake {
    pub address: String,
    pub amount: u64,
    pub active: bool,
    pub violations: u32,
}

impl NodeStake {
    pub fn new(address: String, amount: u64) -> Self {
        NodeStake { address, amount, active: true, violations: 0 }
    }

    pub const MIN_STAKE: u64 = 1_000;

    pub fn is_validator(&self) -> bool {
        self.active && self.amount >= Self::MIN_STAKE
    }
}

pub struct Node {
    pub dag: DAG,
    pub state: LedgerState,
    pub mempool: Mempool,
    pub validator: Validator,
    pub pruner: Pruner,
    pub anti_spam: AntiSpamController,
    pub stakes: HashMap<String, NodeStake>,
    pub last_state_root: Option<String>,
    pub checkpoint_height: u64,
    pub checkpoint_registry: CheckpointRegistry,
}

impl Node {
    pub fn new() -> Self {
        Node {
            dag: DAG::new(),
            state: LedgerState::new(),
            mempool: Mempool::new(),
            validator: Validator::new(),
            pruner: Pruner::default(),
            anti_spam: AntiSpamController::new(),
            stakes: HashMap::new(),
            last_state_root: None,
            checkpoint_height: 0,
            checkpoint_registry: CheckpointRegistry::new(),
        }
    }

    pub fn bootstrap_genesis(&mut self, address: &str, balance: u64) {
        self.state.credit(address, balance);
        self.update_state_root();
    }

    pub fn faucet(&mut self, address: &str, amount: u64) {
        self.state.credit(address, amount);
    }

    pub fn select_parents(&self) -> Vec<String> {
        let tips = self.dag.get_tips();
        if tips.is_empty() {
            return vec![];
        }
        tips.into_iter().take(2).collect()
    }

    pub fn current_difficulty(&self) -> usize {
        self.anti_spam.current_difficulty()
    }

    pub fn mine_anti_spam(&self, tx: &mut TransactionVertex) {
        let difficulty = self.anti_spam.current_difficulty();
        tx.mine_anti_spam(difficulty);
    }

    pub fn register_stake(&mut self, address: &str, amount: u64) -> StakeResult {
        if amount < NodeStake::MIN_STAKE {
            return StakeResult::BelowMinimum { min: NodeStake::MIN_STAKE };
        }

        if let Some(existing) = self.stakes.get(address) {
            if existing.active {
                return StakeResult::AlreadyStaking;
            }
        }

        let balance = self.state.get_balance(address);
        if balance < amount {
            return StakeResult::InsufficientBalance;
        }

        if let Some(b) = self.state.balances.get_mut(address) {
            *b = b.saturating_sub(amount);
        }

        self.stakes.insert(address.to_string(), NodeStake::new(address.to_string(), amount));
        StakeResult::Registered { amount }
    }

    pub fn total_stake(&self) -> f64 {
        self.stakes.values()
            .filter(|s| s.is_validator())
            .map(|s| s.amount as f64)
            .sum()
    }

    pub fn stake_of(&self, address: &str) -> f64 {
        self.stakes.get(address)
            .filter(|s| s.is_validator())
            .map(|s| s.amount as f64)
            .unwrap_or(0.0)
    }

    pub fn stake_weights(&self) -> HashMap<String, f64> {
        self.stakes.values()
            .filter(|s| s.is_validator())
            .map(|s| (s.address.clone(), s.amount as f64))
            .collect()
    }

    pub fn is_validator(&self, address: &str) -> bool {
        self.stakes.get(address)
            .map(|s| s.is_validator())
            .unwrap_or(false)
    }

    pub fn update_state_root(&mut self) {
        let state_map: HashMap<String, (u64, u64)> = self.state.balances
            .iter()
            .map(|(addr, bal)| {
                let nonce = self.state.nonces.get(addr).copied().unwrap_or(0);
                (addr.clone(), (*bal, nonce))
            })
            .collect();

        let tree = MerkleTree::from_state(&state_map);
        self.last_state_root = Some(tree.root);
    }

    pub fn create_checkpoint(&mut self) -> StateCheckpoint {
        self.update_state_root();
        self.checkpoint_height += 1;

        StateCheckpoint::new(
            self.last_state_root.clone().unwrap_or_default(),
            self.checkpoint_height,
            self.state.balances.len(),
        )
    }

    pub fn maybe_create_dag_checkpoint(&mut self) -> Option<String> {
        let dag_height = self.dag.vertices.len() as u64;
        if !self.checkpoint_registry.should_checkpoint(dag_height) {
            return None;
        }

        self.update_state_root();
        let state_root = self.last_state_root.clone()?;
        let sequence = self.checkpoint_registry.len() as u64 + 1;
        let parents = self.select_parents();

        let cp = CheckpointVertex::new(
            state_root.clone(),
            sequence,
            dag_height,
            self.state.balances.len(),
            "node".to_string(),
            parents,
        );

        let cp_id = cp.checkpoint_id.clone();
        self.checkpoint_registry.register(cp);
        Some(cp_id)
    }

    pub fn latest_trusted_checkpoint(&self) -> Option<&CheckpointVertex> {
        self.checkpoint_registry.latest_finalized()
    }

    pub fn verify_state_root(&self, expected_root: &str) -> Result<(), String> {
        let state_map: HashMap<String, (u64, u64)> = self.state.balances
            .iter()
            .map(|(addr, bal)| {
                let nonce = self.state.nonces.get(addr).copied().unwrap_or(0);
                (addr.clone(), (*bal, nonce))
            })
            .collect();

        MerkleTree::verify(&state_map, expected_root)
    }

    pub fn create_transaction(
        &mut self,
        wallet: &WalletInfo,
        receiver: &str,
        amount: u64,
        sign_fn: impl Fn(&[u8]) -> String,
    ) -> TransactionVertex {
        let nonce = self.state.get_nonce(&wallet.address) + 1;
        let parents = self.select_parents();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut tx = TransactionVertex::new(
            wallet.address.clone(),
            receiver.to_string(),
            amount,
            nonce,
            timestamp,
            wallet.public_key.clone(),
            parents,
        );

        self.mine_anti_spam(&mut tx);
        tx.signature = sign_fn(&tx.signing_payload());
        tx.finalize();
        tx
    }

    pub fn submit_transaction(&mut self, tx: TransactionVertex) -> ValidationResult {
        match self.anti_spam.check_and_record_address(&tx.sender) {
            RateLimitResult::Rejected { reason } => {
                return ValidationResult::err("rate_limited", &reason);
            }
            RateLimitResult::Allowed(_priority) => {
            }
        }

        let difficulty = self.anti_spam.current_difficulty();

        let result = self.validator.validate_full_with_difficulty(
            &tx, &self.dag, &mut self.state, difficulty,
        );
        if !result.ok {
            return result;
        }

        if self.mempool.has(&tx.tx_id) {
            return ValidationResult::err("duplicate_mempool", "transaction already in mempool");
        }

        let tx_id = tx.tx_id.clone();
        self.mempool.add(tx.clone());

        if self.state.apply_transaction(&tx).is_err() {
            self.mempool.remove(&tx_id);
            return ValidationResult::err("state_error", "failed to apply transaction");
        }

        if self.dag.add_transaction(tx).is_err() {
            self.mempool.remove(&tx_id);
            return ValidationResult::err("dag_error", "failed to add to DAG");
        }

        self.dag.propagate_weight(&tx_id);
        self.mempool.remove(&tx_id);
        self.anti_spam.record_transaction();

        let total = self.dag.vertices.len() as u64;
        if total % 100 == 0 {
            self.update_state_root();
        }

        if let Some(cp_id) = self.maybe_create_dag_checkpoint() {
            let _ = cp_id; 
        }

        if self.pruner.should_prune_default(&self.dag) {
            let result = self.pruner.prune(&mut self.dag, &self.state);
            if result.pruned_count > 0 {
                self.update_state_root();
            }
        }

        ValidationResult::ok("accepted", "transaction accepted")
    }

    pub fn get_balance(&mut self, address: &str) -> u64 {
        self.state.get_balance(address)
    }

    pub fn get_nonce(&mut self, address: &str) -> u64 {
        self.state.get_nonce(address)
    }

    pub fn dag_stats(&self) -> crate::dag::DagStats {
        self.dag.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_genesis() {
        let mut node = Node::new();
        node.bootstrap_genesis("genesis_addr", 10_000_000);
        assert_eq!(node.get_balance("genesis_addr"), 10_000_000);
    }

    #[test]
    fn test_bootstrap_genesis_sets_state_root() {
        let mut node = Node::new();
        node.bootstrap_genesis("genesis_addr", 10_000_000);
        assert!(node.last_state_root.is_some());
    }

    #[test]
    fn test_select_parents_empty_dag() {
        let node = Node::new();
        assert!(node.select_parents().is_empty());
    }

    #[test]
    fn test_mine_anti_spam() {
        let node = Node::new();
        let mut tx = TransactionVertex::new(
            "alice".to_string(), "bob".to_string(),
            100, 1, 1000, "pk".to_string(), vec![],
        );
        node.mine_anti_spam(&mut tx);
        let difficulty = node.current_difficulty();
        let prefix = "0".repeat(difficulty);
        assert!(tx.anti_spam_hash.starts_with(&prefix));
    }

    #[test]
    fn test_dag_stats_empty() {
        let node = Node::new();
        let stats = node.dag_stats();
        assert_eq!(stats.total_vertices, 0);
    }

    #[test]
    fn test_initial_difficulty() {
        let node = Node::new();
        assert!(node.current_difficulty() >= 2);
    }

    #[test]
    fn test_register_stake_success() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        let result = node.register_stake("alice", 1000);
        assert_eq!(result, StakeResult::Registered { amount: 1000 });
        assert!(node.is_validator("alice"));
    }

    #[test]
    fn test_register_stake_below_minimum() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        let result = node.register_stake("alice", 100);
        assert_eq!(result, StakeResult::BelowMinimum { min: NodeStake::MIN_STAKE });
        assert!(!node.is_validator("alice"));
    }

    #[test]
    fn test_register_stake_insufficient_balance() {
        let mut node = Node::new();
        node.state.credit("alice", 500);
        let result = node.register_stake("alice", 1000);
        assert_eq!(result, StakeResult::InsufficientBalance);
    }

    #[test]
    fn test_register_stake_deducts_balance() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        node.register_stake("alice", 1000);
        assert_eq!(node.get_balance("alice"), 4000);
    }

    #[test]
    fn test_register_stake_already_staking() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        node.register_stake("alice", 1000);
        let result = node.register_stake("alice", 1000);
        assert_eq!(result, StakeResult::AlreadyStaking);
    }

    #[test]
    fn test_total_stake_sums_validators() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        node.state.credit("bob", 5000);
        node.register_stake("alice", 2000);
        node.register_stake("bob", 3000);
        assert_eq!(node.total_stake(), 5000.0);
    }

    #[test]
    fn test_stake_of_non_validator_is_zero() {
        let node = Node::new();
        assert_eq!(node.stake_of("nobody"), 0.0);
    }

    #[test]
    fn test_stake_weights_includes_validators() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        node.register_stake("alice", 2000);
        let weights = node.stake_weights();
        assert!(weights.contains_key("alice"));
        assert_eq!(weights["alice"], 2000.0);
    }

    #[test]
    fn test_no_stake_not_in_weights() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        let weights = node.stake_weights();
        assert!(!weights.contains_key("alice"));
    }

    #[test]
    fn test_update_state_root_after_genesis() {
        let mut node = Node::new();
        node.bootstrap_genesis("alice", 1000);
        let root1 = node.last_state_root.clone().unwrap();

        node.state.credit("bob", 500);
        node.update_state_root();
        let root2 = node.last_state_root.clone().unwrap();

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_verify_state_root_correct() {
        let mut node = Node::new();
        node.bootstrap_genesis("alice", 1000);
        let root = node.last_state_root.clone().unwrap();
        assert!(node.verify_state_root(&root).is_ok());
    }

    #[test]
    fn test_verify_state_root_wrong_fails() {
        let mut node = Node::new();
        node.bootstrap_genesis("alice", 1000);
        assert!(node.verify_state_root("wrong_root").is_err());
    }

    #[test]
    fn test_create_checkpoint() {
        let mut node = Node::new();
        node.bootstrap_genesis("alice", 1000);
        let cp = node.create_checkpoint();
        assert!(!cp.state_root.is_empty());
        assert_eq!(cp.dag_height, 1);
        assert_eq!(cp.address_count, 1);
    }

    #[test]
    fn test_checkpoint_height_increments() {
        let mut node = Node::new();
        node.bootstrap_genesis("alice", 1000);
        let cp1 = node.create_checkpoint();
        let cp2 = node.create_checkpoint();
        assert_eq!(cp1.dag_height, 1);
        assert_eq!(cp2.dag_height, 2);
    }
}