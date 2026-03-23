// storage/src/snapshot.rs

use std::path::{Path, PathBuf};
use std::fs;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use ledger::dag::DAG;
use ledger::state::LedgerState;
use ledger::transaction::TransactionVertex;
use ledger::node::NodeStake;

#[derive(Serialize, Deserialize)]
struct SnapshotData {
    vertices: HashMap<String, TransactionVertex>,
    state: StateSnapshot,
    wallet: HashMap<String, String>,
    #[serde(default)]
    stakes: HashMap<String, StakeSnapshot>,
    #[serde(default)]
    network_start: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone)]
struct StakeSnapshot {
    pub amount: u64,
    pub original_amount: u64,
    pub active: bool,
    pub violations: u32,
}

impl From<&NodeStake> for StakeSnapshot {
    fn from(s: &NodeStake) -> Self {
        StakeSnapshot {
            amount: s.amount,
            original_amount: s.amount, 
            active: s.active,
            violations: s.violations,
        }
    }
}

impl From<StakeSnapshot> for NodeStake {
    fn from(s: StakeSnapshot) -> Self {
        NodeStake {
            address: String::new(), 
            amount: s.amount,
            active: s.active,
            violations: s.violations,
        }
    }
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
        self.save_with_stakes(dag, state, wallet_data, &HashMap::new())
    }

    pub fn save_with_stakes(
        &self,
        dag: &DAG,
        state: &LedgerState,
        wallet_data: Option<HashMap<String, String>>,
        stakes: &HashMap<String, NodeStake>,
    ) -> Result<(), String> {
        self.save_full(dag, state, wallet_data, stakes, None)
    }

    pub fn save_full(
        &self,
        dag: &DAG,
        state: &LedgerState,
        wallet_data: Option<HashMap<String, String>>,
        stakes: &HashMap<String, NodeStake>,
        network_start: Option<f64>,
    ) -> Result<(), String> {
        let stakes_snap: HashMap<String, StakeSnapshot> = stakes.iter()
            .map(|(addr, s)| (addr.clone(), StakeSnapshot::from(s)))
            .collect();

        let data = SnapshotData {
            vertices: dag.vertices.clone(),
            state: StateSnapshot {
                balances: state.balances.clone(),
                nonces: state.nonces.clone(),
                applied_txs: state.applied_txs.iter().cloned().collect(),
            },
            wallet: wallet_data.unwrap_or_default(),
            stakes: stakes_snap,
            network_start,
        };

        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("serialize error: {}", e))?;

        let tmp_path = self.path.with_extension("tmp");
        fs::write(&tmp_path, &json)
            .map_err(|e| format!("write error: {}", e))?;
        fs::rename(&tmp_path, &self.path)
            .map_err(|e| format!("rename error: {}", e))?;

        Ok(())
    }

    pub fn load(
        &self,
        dag: &mut DAG,
        state: &mut LedgerState,
    ) -> Result<Option<HashMap<String, String>>, String> {
        self.load_with_stakes(dag, state).map(|opt| opt.map(|(w, _, _)| w))
    }

    pub fn load_with_stakes(
        &self,
        dag: &mut DAG,
        state: &mut LedgerState,
    ) -> Result<Option<(HashMap<String, String>, HashMap<String, NodeStake>, Option<f64>)>, String> {
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

        let stakes: HashMap<String, NodeStake> = data.stakes.into_iter()
            .map(|(addr, snap)| {
                let mut stake = NodeStake::from(snap);
                stake.address = addr.clone();
                (addr, stake)
            })
            .collect();

        Ok(Some((data.wallet, stakes, data.network_start)))
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
use ledger::node::NodeStake;

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

    #[test]
    fn test_stakes_roundtrip() {
        let path = tmp_path();
        let storage = SnapshotStorage::new(&path);
        let dag = DAG::new();
        let state = LedgerState::new();

        let mut stakes = HashMap::new();
        stakes.insert("alice".to_string(), NodeStake {
            address: "alice".to_string(),
            amount: 5000,
            active: true,
            violations: 1,
        });
        stakes.insert("bob".to_string(), NodeStake {
            address: "bob".to_string(),
            amount: 1000,
            active: false,
            violations: 0,
        });

        storage.save_with_stakes(&dag, &state, None, &stakes).unwrap();

        let mut dag2 = DAG::new();
        let mut state2 = LedgerState::new();
        let (_, loaded_stakes, _) = storage.load_with_stakes(&mut dag2, &mut state2)
            .unwrap().unwrap();

        assert_eq!(loaded_stakes.len(), 2);
        assert_eq!(loaded_stakes["alice"].amount, 5000);
        assert_eq!(loaded_stakes["alice"].active, true);
        assert_eq!(loaded_stakes["alice"].violations, 1);
        assert_eq!(loaded_stakes["bob"].amount, 1000);
        assert_eq!(loaded_stakes["bob"].active, false);

        storage.delete();
    }

    #[test]
    fn test_empty_stakes_roundtrip() {
        let path = tmp_path();
        let storage = SnapshotStorage::new(&path);
        let dag = DAG::new();
        let state = LedgerState::new();

        storage.save_with_stakes(&dag, &state, None, &HashMap::new()).unwrap();

        let mut dag2 = DAG::new();
        let mut state2 = LedgerState::new();
        let (_, loaded_stakes, _) = storage.load_with_stakes(&mut dag2, &mut state2)
            .unwrap().unwrap();

        assert!(loaded_stakes.is_empty());
        storage.delete();
    }

    #[test]
    fn test_atomic_write_creates_no_tmp_on_success() {
        let path = tmp_path();
        let storage = SnapshotStorage::new(&path);
        let dag = DAG::new();
        let state = LedgerState::new();

        storage.save(&dag, &state, None).unwrap();

        let tmp = path.with_extension("tmp");
        assert!(!tmp.exists(), ".tmp file should not exist after successful save");
        assert!(path.exists(), "snapshot file should exist");

        storage.delete();
    }

    #[test]
    fn test_network_start_roundtrip() {
        let path = tmp_path();
        let storage = SnapshotStorage::new(&path);
        let dag = DAG::new();
        let state = LedgerState::new();
        let network_start = 1_700_000_000.0f64;

        storage.save_full(&dag, &state, None, &HashMap::new(), Some(network_start)).unwrap();

        let mut dag2 = DAG::new();
        let mut state2 = LedgerState::new();
        let (_, _, loaded_ns) = storage.load_with_stakes(&mut dag2, &mut state2)
            .unwrap().unwrap();

        assert_eq!(loaded_ns, Some(network_start));
        storage.delete();
    }

    #[test]
    fn test_network_start_none_when_not_set() {
        let path = tmp_path();
        let storage = SnapshotStorage::new(&path);
        let dag = DAG::new();
        let state = LedgerState::new();

        storage.save(&dag, &state, None).unwrap();

        let mut dag2 = DAG::new();
        let mut state2 = LedgerState::new();
        let (_, _, loaded_ns) = storage.load_with_stakes(&mut dag2, &mut state2)
            .unwrap().unwrap();

        assert_eq!(loaded_ns, None);
        storage.delete();
    }

    #[test]
    fn test_network_start_preserved_after_stakes_update() {
        let path = tmp_path();
        let storage = SnapshotStorage::new(&path);
        let dag = DAG::new();
        let state = LedgerState::new();
        let network_start = 1_700_000_000.0f64;

        storage.save_full(&dag, &state, None, &HashMap::new(), Some(network_start)).unwrap();

        let stakes = HashMap::new();
        storage.save_with_stakes(&dag, &state, None, &stakes).unwrap();

        let mut dag2 = DAG::new();
        let mut state2 = LedgerState::new();
        let (_, _, loaded_ns) = storage.load_with_stakes(&mut dag2, &mut state2)
            .unwrap().unwrap();
        assert_eq!(loaded_ns, None);

        storage.delete();
    }
}