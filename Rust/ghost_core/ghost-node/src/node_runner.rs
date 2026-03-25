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
use crate::ws_server::{self, SharedResolver};

pub async fn run(cli: Cli) -> Result<(), String> {
    std::fs::create_dir_all(&cli.data_dir)
        .map_err(|e| format!("failed to create data dir: {}", e))?;

    let snapshot_path = PathBuf::from(cli.snapshot_path());
    let storage = SnapshotStorage::new(&snapshot_path);
    let mut node = Node::new();

    let loaded = storage.load(&mut node.dag, &mut node.state)
        .map_err(|e| format!("snapshot load error: {}", e))?;

    match node.checkpoint_registry.verify_chain() {
        Ok(n) if n > 0 => info!("Checkpoint chain verified: {} finalized checkpoints", n),
        Ok(_) => info!("No finalized checkpoints yet — fresh start"),
        Err(e) => {
            return Err(format!("Checkpoint chain invalid on load: {}", e));
        }
    }
        
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
                    match node.register_stake(addr, token::staking::MIN_STAKE) {
                        Ok(()) => info!("Genesis stake registered for {}", addr),
                        Err(e) => warn!("Genesis stake failed (non-fatal): {}", e),
                    }
                
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
    let total_stake   = node.total_stake();
    if !stake_weights.is_empty() {
        conflict_resolver.resolve_all_with_stake(&mut node.dag, &stake_weights, total_stake);

        use token::staking::ViolationType;
        for tx in node.dag.vertices.values() {
            if matches!(tx.status, ledger::transaction::TxStatus::Conflict) {
                if node.staking.is_eligible(&tx.sender) {
                    if let Some(result) = node.staking.slash(
                        &tx.sender,
                        ViolationType::ConflictingTx,
                        &tx.tx_id,
                    ) {
                        warn!(
                            "Slashed {} for conflicting tx {}: -{} GHOST",
                            tx.sender, &tx.tx_id[..8.min(tx.tx_id.len())], result.slashed_amount
                        );
                    }
                }
            }
        }
    }

    let mut peers = PeerList::new();
    for peer in &cli.peers {
        peers.add(peer);
        info!("Added peer: {}", peer);
    }

    let resolver: SharedResolver = Arc::new(Mutex::new(conflict_resolver));

    let node    = Arc::new(Mutex::new(node));
    let peers   = Arc::new(Mutex::new(peers));
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

    let server_node     = Arc::clone(&node);
    let server_peers    = Arc::clone(&peers);
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
        info!("Syncing state from {}...", peer_url);

        if let Some(cp_json) = client.fetch_checkpoint(peer_url).await {
            let n = node.lock().await;
            let peer_root = cp_json["state_root"].as_str().unwrap_or("").to_string();
            let peer_seq  = cp_json["sequence"].as_u64().unwrap_or(0);
            let is_finalized = cp_json["is_finalized"].as_bool().unwrap_or(false);

            if !is_finalized {
                info!("Peer {} has no finalized checkpoint — skipping checkpoint verification", peer_url);
            } else {
                info!("Peer checkpoint seq={} root={}...", peer_seq, &peer_root[..8.min(peer_root.len())]);
            }
            drop(n);
        }

        match client.fetch_state(peer_url).await {
            Some(state_json) => {
                let mut candidate = ledger::state::LedgerState::new();
                if let Some(balances) = state_json.get("balances").and_then(|b| b.as_object()) {
                    for (addr, val) in balances {
                        if let Some(balance) = val.as_u64() {
                            candidate.ensure_account(addr);
                            candidate.balances.insert(addr.clone(), balance);
                        }
                    }
                }
                if let Some(nonces) = state_json.get("nonces").and_then(|n| n.as_object()) {
                    for (addr, val) in nonces {
                        if let Some(nonce) = val.as_u64() {
                            candidate.nonces.insert(addr.clone(), nonce);
                        }
                    }
                }

                let n = node.lock().await;
                match n.verify_synced_state(&candidate) {
                    Ok(()) => {
                        drop(n);
                        let mut n = node.lock().await;
                        let account_count = candidate.balances.len();
                        n.state.balances.extend(candidate.balances);
                        n.state.nonces.extend(candidate.nonces);
                        info!("State synced from {} — {} accounts (root verified)", peer_url, account_count);
                        return;
                    }
                    Err(e) => {
                        warn!("State from {} failed root verification: {}", peer_url, e);
                    }
                }
            }
            None => warn!("Could not fetch state from {}", peer_url),
        }
    }
    warn!("Could not sync from any peer — starting with local state");
}
