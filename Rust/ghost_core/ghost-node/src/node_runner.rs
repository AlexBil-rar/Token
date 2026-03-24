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

use crate::cli::Cli;
use crate::genesis;
use crate::ws_server::{self, SharedResolver};

pub async fn run(cli: Cli) -> Result<(), String> {
    std::fs::create_dir_all(&cli.data_dir)
        .map_err(|e| format!("failed to create data dir: {}", e))?;

    let snapshot_path = PathBuf::from(cli.snapshot_path());
    let storage = SnapshotStorage::new(&snapshot_path);
    let mut node = Node::new();

    let loaded = storage.load(&mut node.dag, &mut node.state)
        .map_err(|e| format!("snapshot load error: {}", e))?;

    if let Some(wallet_data) = loaded {
        info!("Resumed from snapshot — {} transactions in DAG", node.dag.vertices.len());
        if let Some(addr) = wallet_data.get("address") {
            info!("Genesis address: {}", addr);
        }
    } else {
        info!("No snapshot found — starting fresh");
        if cli.genesis {
            match &cli.genesis_address {
                Some(addr) => {
                    if !genesis::validate_address(addr) {
                        return Err(format!("invalid genesis address: {}", addr));
                    }
                    genesis::bootstrap(&mut node.state, addr);
                    let mut wallet_data = std::collections::HashMap::new();
                    wallet_data.insert("address".to_string(), addr.clone());
                    storage.save(&node.dag, &node.state, Some(wallet_data))
                        .map_err(|e| format!("failed to save genesis: {}", e))?;
                    info!("Genesis snapshot saved to {}", cli.snapshot_path());
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
        conflict_resolver.resolve_all_with_stake(&mut node.dag, &stake_weights, total_stake);
    }

    let mut peers = PeerList::new();
    for peer in &cli.peers {
        peers.add(peer);
        info!("Added peer: {}", peer);
    }

    let resolver: SharedResolver = Arc::new(Mutex::new(conflict_resolver));

    let node = Arc::new(Mutex::new(node));
    let peers = Arc::new(Mutex::new(peers));
    let storage = Arc::new(storage);

    if !cli.peers.is_empty() {
        sync_from_peers(Arc::clone(&node), Arc::clone(&peers)).await;
    }

    {
        let n = node.lock().await;
        let stats = n.dag_stats();
        info!("Node ready — DAG: {} tx, {} tips, {} confirmed",
            stats.total_vertices, stats.tips, stats.confirmed);
    }

    let port = cli.port;

    let server_node = Arc::clone(&node);
    let server_peers = Arc::clone(&peers);
    let server_resolver = Arc::clone(&resolver);
    let server_task = tokio::spawn(async move {
        if let Err(e) = ws_server::start(port, server_node, server_peers, server_resolver).await {
            error!("WebSocket server error: {}", e);
        }
    });

    let snapshot_node = Arc::clone(&node);
    let snapshot_storage = Arc::clone(&storage);
    let snapshot_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let n = snapshot_node.lock().await;
            match snapshot_storage.save(&n.dag, &n.state, None) {
                Ok(_) => info!("Auto-snapshot saved"),
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
    snapshot_task.abort();

    {
        let n = node.lock().await;
        match storage.save(&n.dag, &n.state, None) {
            Ok(_) => info!("Final snapshot saved"),
            Err(e) => warn!("Failed to save final snapshot: {}", e),
        }
        let stats = n.dag_stats();
        info!("Shutdown — DAG: {} tx, {} confirmed", stats.total_vertices, stats.confirmed);
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
                    info!("State synced from {} — {} accounts", peer_url, map.len());
                    return;
                }
            }
            None => warn!("Could not sync from {}", peer_url),
        }
    }
    warn!("Could not sync from any peer — starting with local state");
}