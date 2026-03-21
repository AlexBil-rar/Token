// storage/src/snapshot.rs

use std::path::{Path, PathBuf};
use std::fs;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use ledger::dag::DAG;
use ledger::state::LedgerState;
use ledger::transaction::TransactionVertex;

#[derive(Serialize, Deserialize)]
struct SnapshotData {
    vertices: HashMap<String, TransactionVertex>,
    state: StateSnapshot,
    wallet: HashMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct StateSnapshot {
    balances: HashMap<String, u64>,
    nonces: HashMap<String, u64>,
    applied_txs: Vec<String>,
}

pub struct SnapshotStorage {
    path: PathBuf,
}

impl SnapshotStorage {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        SnapshotStorage { path }
    }

    pub fn save(
        &self,
        dag: &DAG,
        state: &LedgerState,
        wallet_data: Option<HashMap<String, String>>,
    ) -> Result<(), String> {
        let data = SnapshotData {
            vertices: dag.vertices.clone(),
            state: StateSnapshot {
                balances: state.balances.clone(),
                nonces: state.nonces.clone(),
                applied_txs: state.applied_txs.iter().cloned().collect(),
            },
            wallet: wallet_data.unwrap_or_default(),
        };

        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("serialize error: {}", e))?;

        fs::write(&self.path, json)
            .map_err(|e| format!("write error: {}", e))?;

        Ok(())
    }

    pub fn load(
        &self,
        dag: &mut DAG,
        state: &mut LedgerState,
    ) -> Result<Option<HashMap<String, String>>, String> {
        if !self.path.exists() {
            return Ok(None);
        }

        let json = fs::read_to_string(&self.path)
            .map_err(|e| format!("read error: {}", e))?;

        let data: SnapshotData = serde_json::from_str(&json)
            .map_err(|e| format!("deserialize error: {}", e))?;

        for (_, tx) in data.vertices {
            if !dag.has_transaction(&tx.tx_id) {
                dag.add_transaction(tx).ok();
            }
        }

        state.balances.extend(data.state.balances);
        state.nonces.extend(data.state.nonces);
        state.applied_txs.extend(data.state.applied_txs);

        Ok(Some(data.wallet))
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn delete(&self) {
        fs::remove_file(&self.path).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::dag::DAG;
    use ledger::state::LedgerState;
    use ledger::transaction::TransactionVertex;

    fn tmp_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "ghost_test_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ))
    }

    fn make_tx(tx_id: &str) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            "alice".to_string(), "bob".to_string(),
            100, 1, 1000, "pk".to_string(), vec![],
        );
        tx.tx_id = tx_id.to_string();
        tx
    }

    #[test]
    fn test_save_and_load_empty() {
        let path = tmp_path();
        let storage = SnapshotStorage::new(&path);
        let dag = DAG::new();
        let state = LedgerState::new();

        storage.save(&dag, &state, None).unwrap();
        assert!(storage.exists());

        let mut dag2 = DAG::new();
        let mut state2 = LedgerState::new();
        let result = storage.load(&mut dag2, &mut state2).unwrap();
        assert!(result.is_some());

        storage.delete();
    }

    #[test]
    fn test_save_and_load_balance() {
        let path = tmp_path();
        let storage = SnapshotStorage::new(&path);
        let dag = DAG::new();
        let mut state = LedgerState::new();
        state.credit("alice", 1000);

        storage.save(&dag, &state, None).unwrap();

        let mut dag2 = DAG::new();
        let mut state2 = LedgerState::new();
        storage.load(&mut dag2, &mut state2).unwrap();

        assert_eq!(state2.balances["alice"], 1000);
        storage.delete();
    }

    #[test]
    fn test_save_and_load_transactions() {
        let path = tmp_path();
        let storage = SnapshotStorage::new(&path);
        let mut dag = DAG::new();
        let state = LedgerState::new();

        dag.add_transaction(make_tx("tx1")).unwrap();
        storage.save(&dag, &state, None).unwrap();

        let mut dag2 = DAG::new();
        let mut state2 = LedgerState::new();
        storage.load(&mut dag2, &mut state2).unwrap();

        assert!(dag2.has_transaction("tx1"));
        storage.delete();
    }

    #[test]
    fn test_no_snapshot_returns_none() {
        let path = tmp_path();
        let storage = SnapshotStorage::new(&path);
        let mut dag = DAG::new();
        let mut state = LedgerState::new();

        let result = storage.load(&mut dag, &mut state).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_wallet_data_roundtrip() {
        let path = tmp_path();
        let storage = SnapshotStorage::new(&path);
        let dag = DAG::new();
        let state = LedgerState::new();

        let mut wallet = HashMap::new();
        wallet.insert("address".to_string(), "abc123".to_string());
        wallet.insert("private_key".to_string(), "secret".to_string());

        storage.save(&dag, &state, Some(wallet.clone())).unwrap();

        let mut dag2 = DAG::new();
        let mut state2 = LedgerState::new();
        let loaded = storage.load(&mut dag2, &mut state2).unwrap().unwrap();

        assert_eq!(loaded["address"], "abc123");
        storage.delete();
    }
}