// ghost-node/src/node_runner.rs

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::signal;
use tracing::{info, warn, error};

use ledger::node::Node;
use network::peer_list::PeerList;
use network::ws_client::WsClient;
use storage::snapshot::SnapshotStorage;
use consensus::conflict_resolver::ConflictResolver;

use crate::cli::Cli;
use crate::genesis;
use crate::ws_server;
use crate::peer_discovery;

pub async fn run(cli: Cli) -> Result<(), String> {
    std::fs::create_dir_all(&cli.data_dir)
        .map_err(|e| format!("failed to create data dir: {}", e))?;

    let snapshot_path = PathBuf::from(cli.snapshot_path());
    let storage = SnapshotStorage::new(&snapshot_path);
    let mut node = Node::new();

    let loaded = storage.load_with_stakes(&mut node.dag, &mut node.state)
        .map_err(|e| format!("snapshot load error: {}", e))?;

    if let Some((wallet_data, saved_stakes)) = loaded {
        node.stakes = saved_stakes;
        if !node.stakes.is_empty() {
            info!("Restored {} validator stakes from snapshot", node.stakes.len());
        }
        info!("Resumed from snapshot — {} transactions in DAG", node.dag.vertices.len());
        if let Some(addr) = wallet_data.get("address") {
            info!("Genesis address: {}", addr);
        }
        node.update_state_root();
        if let Some(root) = &node.last_state_root {
            info!("State root: {}...", &root[..16]);
        }
    } else {
        info!("No snapshot found — starting fresh");
        if cli.genesis {
            match &cli.genesis_address {
                Some(addr) => {
                    if !genesis::validate_address(addr) {
                        return Err(format!("invalid genesis address: {}", addr));
                    }
                    node.bootstrap_genesis(addr, 10_000_000);
                    let mut wallet_data = std::collections::HashMap::new();
                    wallet_data.insert("address".to_string(), addr.clone());
                    storage.save(&node.dag, &node.state, Some(wallet_data))
                        .map_err(|e| format!("failed to save genesis: {}", e))?;
                    info!("Genesis snapshot saved to {}", cli.snapshot_path());
                    if let Some(root) = &node.last_state_root {
                        info!("Genesis state root: {}...", &root[..16]);
                    }
                }
                None => return Err("--genesis requires --genesis-address".to_string()),
            }
        }
    }

    let mut conflict_resolver = ConflictResolver::new();
    for tx in node.dag.vertices.values() {
        conflict_resolver.register_transaction(tx);
    }

    let stake_weights = node.stake_weights();
    let total_stake = node.total_stake();
    if !stake_weights.is_empty() {
        info!("Resolving conflicts with stake weights ({} validators, total stake: {})",
            stake_weights.len(), total_stake);
        conflict_resolver.resolve_all_with_stake(&mut node.dag, &stake_weights, total_stake);
    }

    let mut peers = PeerList::new();
    for peer in &cli.peers {
        peers.add(peer);
        info!("Added peer: {}", peer);
    }

    let node = Arc::new(Mutex::new(node));
    let peers = Arc::new(Mutex::new(peers));
    let storage = Arc::new(storage);

    if !cli.peers.is_empty() {
        sync_from_peers(Arc::clone(&node), Arc::clone(&peers)).await;
    }

    {
        let n = node.lock().await;
        let stats = n.dag_stats();
        let validator_count = n.stakes.values().filter(|s| s.is_validator()).count();
        info!("Node ready — DAG: {} tx, {} tips, {} confirmed, {} validators",
            stats.total_vertices, stats.tips, stats.confirmed, validator_count);
    }

    let port = cli.port;

    let discovery_peers = Arc::clone(&peers);
    let discovery_task = tokio::spawn(async move {
        peer_discovery::run_discovery_loop(discovery_peers, port, 30).await;
    });

    let health_peers = Arc::clone(&peers);
    let health_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            std::time::Duration::from_secs(60)
        );
        loop {
            interval.tick().await;
            peer_discovery::health_check(Arc::clone(&health_peers)).await;
        }
    });

    let server_node = Arc::clone(&node);
    let server_peers = Arc::clone(&peers);
    let server_task = tokio::spawn(async move {
        if let Err(e) = ws_server::start(port, server_node, server_peers).await {
            error!("WebSocket server error: {}", e);
        }
    });

    let snapshot_node = Arc::clone(&node);
    let snapshot_storage = Arc::clone(&storage);
    let snapshot_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let mut n = snapshot_node.lock().await;
            n.update_state_root();
            let root_short = n.last_state_root.as_ref()
                .map(|r| &r[..16.min(r.len())])
                .unwrap_or("none")
                .to_string();
            match snapshot_storage.save_with_stakes(&n.dag, &n.state, None, &n.stakes) {
                Ok(_) => info!("Auto-snapshot saved (root: {}...)", root_short),
                Err(e) => warn!("Auto-snapshot failed: {}", e),
            }
        }
    });

    info!("Node running on ws://0.0.0.0:{}", port);
    info!("Peers: {}", cli.peers.len());
    info!("Press Ctrl+C to stop");

    match signal::ctrl_c().await {
        Ok(()) => info!("Shutting down..."),
        Err(e) => error!("Ctrl+C error: {}", e),
    }

    server_task.abort();
    discovery_task.abort();
    health_task.abort();
    snapshot_task.abort();

    {
        let mut n = node.lock().await;
        n.update_state_root();
        match storage.save_with_stakes(&n.dag, &n.state, None, &n.stakes) {
            Ok(_) => info!("Final snapshot saved"),
            Err(e) => warn!("Failed to save final snapshot: {}", e),
        }
        let stats = n.dag_stats();
        info!("Shutdown — DAG: {} tx, {} confirmed", stats.total_vertices, stats.confirmed);
        if let Some(root) = &n.last_state_root {
            info!("Final state root: {}...", &root[..16]);
        }
    }

    info!("Goodbye.");
    Ok(())
}

async fn sync_from_peers(node: Arc<Mutex<Node>>, peers: Arc<Mutex<PeerList>>) {
    let peer_list = peers.lock().await.get_all();
    let client = WsClient::with_timeout(5);

    for peer_url in &peer_list {
        info!("Trying to sync from {}...", peer_url);
        match client.fetch_state(peer_url).await {
            Some(state_json) => {
                let mut n = node.lock().await;
                if let Some(map) = state_json.get("balances").and_then(|b| b.as_object()) {
                    for (addr, val) in map {
                        if let Some(balance) = val.as_u64() {
                            n.state.ensure_account(addr);
                            n.state.balances.insert(addr.clone(), balance);
                        }
                    }
                    n.update_state_root();
                    info!("State synced from {} — {} accounts", peer_url, map.len());
                    return;
                }
            }
            None => warn!("Could not sync from {}", peer_url),
        }
    }
    warn!("Could not sync from any peer — starting with local state");
}