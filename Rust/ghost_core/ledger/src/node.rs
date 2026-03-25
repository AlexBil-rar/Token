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
use crate::privacy::{DecoyPool, DiffusionConfig, GraphPrivacyAnalyzer, IntersectionAttackDetector, DandelionPhase};
use crate::parent_selection::{ParentSelectionPolicy, select_parents as policy_select};
use token::staking::StakingManager;


const RESOLVE_MIN_WEIGHT: u64 = 3;

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

#[derive(Debug, Default)]
pub struct ConflictResolverState {
    pub conflict_sets: std::collections::HashMap<(String, u64), Vec<String>>,
    pub resolved: std::collections::HashMap<(String, u64), String>,
}

impl ConflictResolverState {
    pub fn new() -> Self { Self::default() }

    pub fn register(&mut self, sender: &str, nonce: u64, tx_id: &str) {
        self.conflict_sets
            .entry((sender.to_string(), nonce))
            .or_default()
            .push(tx_id.to_string());
    }

    pub fn resolve_ready(
        &mut self,
        dag: &mut crate::dag::DAG,
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
    ) -> Vec<(String, String)> {
        let ready: Vec<(String, u64)> = self.conflict_sets.iter()
            .filter(|(key, ids)| {
                !self.resolved.contains_key(*key) && ids.len() > 1 &&
                ids.iter().all(|id| {
                    dag.get_transaction(id)
                        .map(|t| t.weight >= RESOLVE_MIN_WEIGHT)
                        .unwrap_or(false)
                })
            })
            .map(|(k, _)| k.clone())
            .collect();

        let mut losers = Vec::new();

        for key in ready {
            let ids = match self.conflict_sets.get(&key) { Some(v) => v.clone(), None => continue };

            let scores: Vec<(String, f64)> = ids.iter().filter_map(|id| {
                let tx = dag.get_transaction(id)?;
                let stake = stake_weights.get(&tx.sender).copied().unwrap_or(0.0);
                let ratio = if total_stake > 0.0 { (stake / total_stake).clamp(0.0, 1.0) } else { 0.0 };
                let multiplier = 1.0 + ratio * 2.0;
                Some((id.clone(), tx.weight as f64 * multiplier))
            }).collect();

            if scores.is_empty() { continue; }

            let winner = scores.iter()
                .max_by(|(id_a, sa), (id_b, sb)| {
                    sa.partial_cmp(sb).unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| id_b.cmp(id_a))
                })
                .map(|(id, _)| id.clone());

            if let Some(winner_id) = winner {
                for id in &ids {
                    if id != &winner_id {
                        if let Some(t) = dag.get_transaction_mut(id) {
                            t.status = crate::transaction::TxStatus::Conflict;
                            losers.push((id.clone(), t.sender.clone()));
                        }
                    }
                }
                self.resolved.insert(key, winner_id);
            }
        }

        losers
    }
}

pub struct WalletInfo {
    pub address: String,
    pub public_key: String,
    pub private_key_hex: String,
}

pub struct Node {
    pub dag: DAG,
    pub state: LedgerState,
    pub mempool: Mempool,
    pub validator: Validator,
    pub pruner: Pruner,
    pub anti_spam: AntiSpamController,
    pub staking: StakingManager,
    pub network_start: f64,
    pub last_state_root: Option<String>,
    pub checkpoint_height: u64,
    pub checkpoint_registry: CheckpointRegistry,
    pub conflict_resolver: ConflictResolverState,
    pub decoy_pool: DecoyPool,
    pub diffusion: DiffusionConfig,
    pub parent_policy: ParentSelectionPolicy,
    pub intersection_detector: IntersectionAttackDetector,
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
            staking: StakingManager::new(),
            network_start: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
            last_state_root: None,
            checkpoint_height: 0,
            checkpoint_registry: CheckpointRegistry::new(),
            conflict_resolver: ConflictResolverState::new(),
            decoy_pool: DecoyPool::new(50),
            diffusion: DiffusionConfig::default(),
            parent_policy: ParentSelectionPolicy::default(),
            intersection_detector: IntersectionAttackDetector::new(20, 3000),
        }
    }

    pub fn try_apply_deferred(&mut self) -> usize {
        let mut applied = 0;

        let candidates: Vec<String> = self.dag.vertices
            .iter()
            .filter(|(_, tx)| {
                matches!(tx.status, crate::transaction::TxStatus::Confirmed)
                && !self.state.applied_txs.contains(&tx.tx_id)
            })
            .map(|(id, _)| id.clone())
            .collect();

        let mut ordered: Vec<(String, u64, u64)> = candidates
            .iter()
            .filter_map(|id| {
                self.dag.get_transaction(id)
                    .map(|tx| (id.clone(), tx.nonce, tx.timestamp))
            })
            .collect();
        ordered.sort_by_key(|(_, nonce, ts)| (*nonce, *ts));

        for (tx_id, _, _) in ordered {
            if let Some(tx) = self.dag.get_transaction(&tx_id).cloned() {
                if matches!(tx.status, crate::transaction::TxStatus::Conflict) {
                    continue;
                }
                match self.state.apply_transaction(&tx) {
                    Ok(_) => { applied += 1; }
                    Err(_) => {
                    }
                }
            }
        }

        applied
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
        if tips.is_empty() { return vec![]; }
        tips.into_iter().take(2).collect()
    }

    pub fn select_parents_private(&mut self) -> Vec<String> {
        let conflict_sets = self.conflict_resolver.conflict_sets.clone();
        let stake_weights = self.stake_weights();
        let total_stake   = self.total_stake();

        let seed = (self.dag.vertices.len() as u64)
        .wrapping_mul(6364136223846793005)
        .wrapping_add(self.anti_spam.current_difficulty() as u64);

        let result = policy_select(
            &self.dag,
            &conflict_sets,
            &stake_weights,
            total_stake,
            &mut self.decoy_pool,
            &self.parent_policy,
            seed,
        );

        result.parents
    }

    pub fn current_difficulty(&self) -> usize {
        self.anti_spam.current_difficulty()
    }

    pub fn mine_anti_spam(&self, tx: &mut TransactionVertex) {
        let difficulty = self.anti_spam.current_difficulty();
        tx.mine_anti_spam(difficulty);
    }

    pub fn register_stake(&mut self, address: &str, amount: u64) -> Result<(), String> {
        self.staking.stake(address, amount, &mut self.state.balances)
    }

    pub fn stake_weights(&self) -> HashMap<String, f64> {
        self.staking.active_validators()
    }

    pub fn total_stake(&self) -> f64 {
        self.staking.total_stake()
    }

    pub fn is_validator(&self, address: &str) -> bool {
        self.staking.is_eligible(address)
    }

    pub fn stake_multiplier(&self, address: &str, total_stake: f64) -> f64 {
        let stake = self.staking.get_stake_amount(address);
        if total_stake <= 0.0 || stake <= 0.0 {
            return 1.0;
        }
        let ratio = (stake / total_stake).clamp(0.0, 1.0);
        1.0 + ratio * 2.0
    }

    pub fn stake_of(&self, address: &str) -> f64 {
        self.staking.get_stake_amount(address)
    }

    pub fn conflict_sets(&self) -> &std::collections::HashMap<(String, u64), Vec<String>> {
        &self.conflict_resolver.conflict_sets
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

        let prev_hash = self.checkpoint_registry
            .latest_finalized()
            .map(|cp| cp.checkpoint_id.clone())
            .unwrap_or_default();

        let cp = CheckpointVertex::new(
            state_root.clone(),
            prev_hash,
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

    pub fn relay_delay(&self, tx_id: &str) -> std::time::Duration {
        self.diffusion.relay_delay(tx_id)
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
        let parents = self.select_parents_private();
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

    pub fn verify_synced_state(
        &self,
        state: &LedgerState,
    ) -> Result<(), String> {
        let trusted_root = match self.checkpoint_registry.latest_trusted_root() {
            Some(r) => r,
            None => return Ok(()), 
        };
    
        let state_map: HashMap<String, (u64, u64)> = state.balances
            .iter()
            .map(|(addr, bal)| {
                let nonce = state.nonces.get(addr).copied().unwrap_or(0);
                (addr.clone(), (*bal, nonce))
            })
            .collect();
    
        MerkleTree::verify(&state_map, trusted_root)
            .map_err(|e| format!("sync state root mismatch: {}", e))
    }

    pub fn submit_transaction(&mut self, tx: TransactionVertex) -> ValidationResult {
        match self.anti_spam.check_and_record_address(&tx.sender) {
            RateLimitResult::Rejected { reason } => {
                return ValidationResult::err("rate_limited", &reason);
            }
            RateLimitResult::Allowed(_priority) => {}
        }

        let difficulty = self.anti_spam.current_difficulty();

        let result = self.validator.validate_structure_and_dag(
            &tx, &self.dag, difficulty, &self.state,
        );
        if !result.ok {
            return result;
        }

        if self.mempool.has(&tx.tx_id) {
            return ValidationResult::err("duplicate_mempool", "transaction already in mempool");
        }

        let tx_id = tx.tx_id.clone();
        self.mempool.add(tx.clone());

        if self.dag.add_transaction(tx).is_err() {
            self.mempool.remove(&tx_id);
            return ValidationResult::err("dag_error", "failed to add to DAG");
        }

        self.dag.propagate_weight(&tx_id);
        self.mempool.remove(&tx_id);
        self.anti_spam.record_transaction();
        self.try_apply_deferred();
        self.decoy_pool.record(tx_id.clone());

        {
            use std::time::{SystemTime, UNIX_EPOCH};
            let ts_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let sender_addr = self.dag.get_transaction(&tx_id)
                .map(|t| t.sender.clone())
                .unwrap_or_default();
            let parent_ids = self.dag.get_transaction(&tx_id)
                .map(|t| t.parents.clone())
                .unwrap_or_default();
            if !sender_addr.is_empty() {
                self.intersection_detector.record_observation(
                    &sender_addr, tx_id.clone(), ts_ms, parent_ids,
                );
            }
        }

        {
            let sender = self.dag.get_transaction(&tx_id)
                .map(|t| (t.sender.clone(), t.nonce))
                .unwrap_or_default();
            if !sender.0.is_empty() {
                self.conflict_resolver.register(&sender.0, sender.1, &tx_id);
            }
            let stake_weights = self.stake_weights();
            let total_stake = self.total_stake();
            let _losers = self.conflict_resolver.resolve_ready(
                &mut self.dag, &stake_weights, total_stake,
            );

            use token::staking::ViolationType;
            for (loser_id, loser_sender) in &_losers {
                if self.staking.is_eligible(&loser_sender) {
                    if let Some(result) = self.staking.slash(
                        &loser_sender,
                        ViolationType::ConflictingTx,
                        &loser_id,
                    ) {
                        let _ = result;
                    }
                }
            }
        }

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

    fn make_wallet(address: &str, public_key: &str) -> WalletInfo {
        WalletInfo {
            address: address.to_string(),
            public_key: public_key.to_string(),
            private_key_hex: String::new(),
        }
    }

    #[test]
    fn test_bootstrap_genesis() {
        let mut node = Node::new();
        node.try_apply_deferred();
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
        assert!(result.is_ok(), "stake should succeed: {:?}", result);
        assert!(node.is_validator("alice"));
    }

    #[test]
    fn test_register_stake_below_minimum() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        let result = node.register_stake("alice", 100);
        assert!(result.is_err(), "stake below minimum should fail");
        assert!(!node.is_validator("alice"));
    }

    #[test]
    fn test_register_stake_insufficient_balance() {
        let mut node = Node::new();
        node.state.credit("alice", 500);
        let result = node.register_stake("alice", 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_register_stake_deducts_balance() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        node.register_stake("alice", 1000).ok();
        assert_eq!(node.get_balance("alice"), 4000);
    }

    #[test]
    fn test_register_stake_already_staking() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        node.register_stake("alice", 1000).unwrap();
        let result = node.register_stake("alice", 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_stake_multiplier_no_stake_is_one() {
        let node = Node::new();
        assert_eq!(node.stake_multiplier("nobody", 1000.0), 1.0);
    }

    #[test]
    fn test_stake_multiplier_with_stake() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        node.register_stake("alice", 1000).unwrap();
        let total = node.total_stake();
        let m = node.stake_multiplier("alice", total);
        assert!(m > 1.0, "multiplier should be > 1.0 for staked node");
        assert!(m <= 3.0, "multiplier capped at 3.0");
    }

    #[test]
    fn test_total_stake_sums_validators() {
        let mut node = Node::new();
        node.state.credit("alice", 5000);
        node.state.credit("bob", 5000);
        node.register_stake("alice", 2000).ok();
        node.register_stake("bob", 3000).ok();
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
        node.register_stake("alice", 2000).ok();
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

    #[test]
    fn test_select_parents_private_empty_dag() {
        let mut node = Node::new();
        assert!(node.select_parents_private().is_empty());
    }

    #[test]
    fn test_select_parents_private_with_tips() {
        let mut node = Node::new();
        node.bootstrap_genesis("alice", 10_000);
        for i in 1..=5u64 {
            let mut tx = TransactionVertex::new(
                "alice".to_string(), "bob".to_string(),
                10, i, 1000, "pk".to_string(), vec![],
            );
            tx.tx_id = format!("tx_{}", i);
            node.dag.add_transaction(tx).unwrap();
        }
        let parents = node.select_parents_private();
        assert!(!parents.is_empty());
        assert!(parents.len() <= 2);
    }

    #[test]
    fn test_decoy_pool_grows_with_transactions() {
        let node = Node::new();
        assert_eq!(node.decoy_pool.size(), 0);
    }

    #[test]
    fn test_relay_delay_in_range() {
        let node = Node::new();
        let delay = node.relay_delay("tx_test_abc");
        assert!(delay.as_millis() >= 50);
        assert!(delay.as_millis() <= 500);
    }

    #[test]
    fn test_relay_delay_deterministic() {
        let node = Node::new();
        let d1 = node.relay_delay("tx_same_id");
        let d2 = node.relay_delay("tx_same_id");
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_relay_delay_different_for_different_txs() {
        let node = Node::new();
        let d1 = node.relay_delay("tx_aaaaaa");
        let d2 = node.relay_delay("tx_zzzzzz");
        assert!(d1.as_millis() >= 50 && d1.as_millis() <= 500);
        assert!(d2.as_millis() >= 50 && d2.as_millis() <= 500);
    }

    #[test]
    fn test_no_relay_delay_when_disabled() {
        let mut node = Node::new();
        node.diffusion = crate::privacy::DiffusionConfig::disabled();
        let delay = node.relay_delay("any_tx_id");
        assert_eq!(delay, std::time::Duration::ZERO);
    }
}