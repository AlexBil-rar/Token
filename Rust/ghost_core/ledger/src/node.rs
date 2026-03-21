// ledger/src/node.rs

use std::time::{SystemTime, UNIX_EPOCH};
use crate::dag::DAG;
use crate::mempool::Mempool;
use crate::state::LedgerState;
use crate::transaction::TransactionVertex;
use crate::validator::{ValidationResult, Validator};
use crate::pruner::Pruner;

const ANTI_SPAM_DIFFICULTY: usize = 3;

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
}

impl Node {
    pub fn new() -> Self {
        Node {
            dag: DAG::new(),
            state: LedgerState::new(),
            mempool: Mempool::new(),
            validator: Validator::new(),
            pruner: Pruner::default(),
        }
    }

    pub fn bootstrap_genesis(&mut self, address: &str, balance: u64) {
        self.state.credit(address, balance);
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

    pub fn mine_anti_spam(&self, tx: &mut TransactionVertex) {
        let prefix = "0".repeat(ANTI_SPAM_DIFFICULTY);
        let mut nonce = 0u64;
        loop {
            tx.anti_spam_nonce = nonce;
            let hash = tx.compute_anti_spam_hash();
            if hash.starts_with(&prefix) {
                tx.anti_spam_hash = hash;
                return;
            }
            nonce += 1;
        }
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
        let result = self.validator.validate_full(&tx, &self.dag, &mut self.state);
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

        if self.pruner.should_prune_default(&self.dag) {
            let result = self.pruner.prune(&mut self.dag, &self.state);
            println!("Pruned {} old transactions", result.pruned_count);
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

    fn fake_sign(_payload: &[u8]) -> String {
        "0".repeat(128)
    }

    #[test]
    fn test_bootstrap_genesis() {
        let mut node = Node::new();
        node.bootstrap_genesis("genesis_addr", 10_000_000);
        assert_eq!(node.get_balance("genesis_addr"), 10_000_000);
    }

    #[test]
    fn test_select_parents_empty_dag() {
        let node = Node::new();
        assert!(node.select_parents().is_empty());
    }

    #[test]
    fn test_mine_anti_spam() {
        let mut node = Node::new();
        let mut tx = TransactionVertex::new(
            "alice".to_string(), "bob".to_string(),
            100, 1, 1000, "pk".to_string(), vec![],
        );
        node.mine_anti_spam(&mut tx);
        assert!(tx.anti_spam_hash.starts_with("000"));
    }

    #[test]
    fn test_dag_stats_empty() {
        let node = Node::new();
        let stats = node.dag_stats();
        assert_eq!(stats.total_vertices, 0);
    }
}