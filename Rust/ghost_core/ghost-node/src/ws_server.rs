// ghost-node/src/ws_server.rs

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};
use tracing::{info, warn, debug};

use ledger::node::Node;
use ledger::transaction::TransactionVertex;
use network::peer_list::PeerList;
use network::ws_message::{WsMessage, MessageType};
use consensus::conflict_resolver::{ConflictResolver, CheckpointAnchor};

use crate::gossip;

pub type SharedNode         = Arc<Mutex<Node>>;
pub type SharedPeers        = Arc<Mutex<PeerList>>;
pub type SharedSeen         = Arc<Mutex<SeenSet>>;
pub type SharedResolver     = Arc<Mutex<ConflictResolver>>;


pub struct SeenSet {
    seen:     HashSet<String>,
    max_size: usize,
}

impl SeenSet {
    pub fn new(max_size: usize) -> Self {
        SeenSet { seen: HashSet::new(), max_size }
    }

    pub fn check_and_insert(&mut self, tx_id: &str) -> bool {
        if self.seen.contains(tx_id) { return true; }
        if self.seen.len() >= self.max_size {
            let half: Vec<String> = self.seen.iter().take(self.max_size / 2).cloned().collect();
            for id in half { self.seen.remove(&id); }
        }
        self.seen.insert(tx_id.to_string());
        false
    }

    pub fn size(&self) -> usize { self.seen.len() }
}

pub async fn start(
    port: u16,
    node: SharedNode,
    peers: SharedPeers,
    resolver: SharedResolver,
) -> Result<(), String> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await
        .map_err(|e| format!("failed to bind {}: {}", addr, e))?;

    info!("WebSocket server listening on ws://{}", addr);

    let seen: SharedSeen = Arc::new(Mutex::new(SeenSet::new(10_000)));

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                debug!("Incoming connection from {}", peer_addr);
                let node     = Arc::clone(&node);
                let peers    = Arc::clone(&peers);
                let seen     = Arc::clone(&seen);
                let resolver = Arc::clone(&resolver);
                tokio::spawn(handle_connection(stream, node, peers, seen, resolver));
            }
            Err(e) => { warn!("Accept error: {}", e); }
        }
    }
}

async fn handle_connection(
    stream:   TcpStream,
    node:     SharedNode,
    peers:    SharedPeers,
    seen:     SharedSeen,
    resolver: SharedResolver,
) {
    let peer_addr = stream.peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            warn!("WebSocket handshake failed from {}: {}", peer_addr, e);
            return;
        }
    };

    info!("Peer connected: {}", peer_addr);
    let (mut sender, mut receiver) = ws_stream.split();

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                const MAX_MSG_BYTES: usize = 1 * 1024 * 1024; // 1 MB
                if text.len() > MAX_MSG_BYTES {
                    warn!(
                        "Oversized message from {}: {} bytes — dropped",
                        peer_addr, text.len()
                    );
                    continue;
                }
                let response = handle_message(
                    &text, &node, &peers, &seen, &resolver,
                ).await;
                if let Some(resp) = response {
                    if let Err(e) = sender.send(Message::Text(resp)).await {
                        warn!("Failed to send response to {}: {}", peer_addr, e);
                        break;
                    }
                }
            }
            Ok(Message::Close(_)) => {
                info!("Peer disconnected: {}", peer_addr);
                break;
            }
            Ok(Message::Ping(data)) => { let _ = sender.send(Message::Pong(data)).await; }
            Err(e) => { warn!("WebSocket error from {}: {}", peer_addr, e); break; }
            _ => {}
        }
    }
}


async fn handle_message(
    raw:      &str,
    node:     &SharedNode,
    peers:    &SharedPeers,
    seen:     &SharedSeen,
    resolver: &SharedResolver,
) -> Option<String> {
    let msg = match WsMessage::from_json(raw) {
        Ok(m)  => m,
        Err(e) => { warn!("Invalid message: {}", e); return None; }
    };

    match msg.msg_type {
        MessageType::Ping => {
            Some(WsMessage::pong("ghostledger").to_json())
        }

        MessageType::Transaction => {
            handle_transaction(msg.payload, node, peers, seen).await
        }

        MessageType::StateRequest => {
            handle_state_request(node).await
        }

        MessageType::PeerList => {
            handle_peer_list(msg.payload, peers).await
        }

        MessageType::CheckpointRequest => {
            handle_checkpoint_request(node).await
        }

        MessageType::ExplorerRequest => {
            handle_explorer_request(node, peers).await
        }

        MessageType::DifficultyRequest => {
            let difficulty = node.lock().await.current_difficulty();
            let resp = WsMessage::new(
                MessageType::DifficultyResponse,
                serde_json::json!({"difficulty": difficulty}),
            );
            Some(resp.to_json())
        }

        MessageType::PartitionHandshake => {
            handle_partition_handshake(msg.payload, node).await
        }

        MessageType::PartitionSyncRequest => {
            handle_partition_sync_request(msg.payload, node).await
        }

        MessageType::PartitionSyncResponse => {
            handle_partition_sync_response(msg.payload, node, resolver).await
        }

        _ => None,
    }
}

async fn handle_transaction(
    payload:  serde_json::Value,
    node:     &SharedNode,
    peers:    &SharedPeers,
    seen:     &SharedSeen,
) -> Option<String> {
    let tx: TransactionVertex = match serde_json::from_value(payload) {
        Ok(t)  => t,
        Err(e) => {
            warn!("Invalid transaction payload: {}", e);
            return Some(serde_json::json!({
                "ok": false, "code": "invalid_payload", "reason": e.to_string()
            }).to_string());
        }
    };

    let tx_id_short = tx.tx_id[..16.min(tx.tx_id.len())].to_string();

    {
        let mut seen_guard = seen.lock().await;
        if seen_guard.check_and_insert(&tx.tx_id) {
            debug!("Duplicate tx ignored: {}...", tx_id_short);
            return None;
        }
    }

    let tx_clone = tx.clone();
    let result = {
        let mut n = node.lock().await;
        n.submit_transaction(tx)
    };

    if result.ok {
        info!("Transaction accepted: {}...", tx_id_short);
    
        let (phase, relay_delay) = {
            let n = node.lock().await;
            let phase = if tx_clone.stem_ttl > 0 {
                ledger::privacy::DandelionPhase::Stem
            } else {
                n.diffusion.dandelion_phase(&tx_clone.tx_id)
            };
            let delay = n.diffusion.effective_delay(&tx_clone.tx_id);
            (phase, delay)
        };
    
        if !relay_delay.is_zero() {
            tokio::time::sleep(relay_delay).await;
        }
    
        gossip::dandelion_broadcast(&tx_clone, Arc::clone(peers), None, phase).await;
    } else {
        debug!("Transaction rejected: {} — {}", tx_id_short, result.reason);
    }

    Some(serde_json::json!({
        "ok": result.ok, "code": result.code, "reason": result.reason,
    }).to_string())
}

async fn handle_state_request(node: &SharedNode) -> Option<String> {
    let n = node.lock().await;
    let state_root = n.last_state_root.clone().unwrap_or_default();
    let cp_seq = n.checkpoint_registry.latest_finalized()
        .map(|cp| cp.sequence)
        .unwrap_or(0);
    let resp = WsMessage::new(MessageType::StateResponse, serde_json::json!({
        "balances":   n.state.balances,
        "nonces":     n.state.nonces,
        "state_root": state_root,
        "cp_seq":     cp_seq,
    }));
    Some(resp.to_json())
}

async fn handle_peer_list(
    payload: serde_json::Value,
    peers:   &SharedPeers,
) -> Option<String> {
    if let Some(arr) = payload.get("peers").and_then(|p| p.as_array()) {
        let mut pl = peers.lock().await;
        for p in arr {
            if let Some(addr) = p.as_str() {
                pl.add(addr);
                debug!("Added peer: {}", addr);
            }
        }
        let my_peers = pl.get_all();
        let resp = WsMessage::new(
            MessageType::PeerList,
            serde_json::json!({ "peers": my_peers }),
        );
        return Some(resp.to_json());
    }
    None
}

async fn handle_checkpoint_request(node: &SharedNode) -> Option<String> {
    let n = node.lock().await;
    let cp = n.checkpoint_registry.latest_finalized()
        .or_else(|| n.checkpoint_registry.latest())?;

    let resp = WsMessage::checkpoint_response(
        &cp.checkpoint_id,
        &cp.state_root,
        cp.sequence,
        cp.dag_height,
        cp.address_count,
        cp.timestamp,
        cp.is_finalized(),
    );
    Some(resp.to_json())
}

async fn handle_explorer_request(
    node:  &SharedNode,
    peers: &SharedPeers,
) -> Option<String> {
    let n = node.lock().await;
    let stats       = n.dag_stats();
    let difficulty  = n.current_difficulty();
    let tps         = n.anti_spam.current_tps();
    let peer_count  = peers.lock().await.size();

    let mut txs: Vec<serde_json::Value> = n.dag.vertices.values()
        .map(|tx| serde_json::json!({
            "tx_id":    &tx.tx_id[..16.min(tx.tx_id.len())],
            "sender":   &tx.sender[..8.min(tx.sender.len())],
            "receiver": &tx.receiver[..8.min(tx.receiver.len())],
            "amount":   if tx.commitment.is_some() {
                            serde_json::Value::String("private".into())
                        } else {
                            serde_json::Value::Number(tx.amount.into())
                        },
            "private":  tx.commitment.is_some(),
            "status":   tx.status.as_str(),
            "timestamp":tx.timestamp,
            "parents":  tx.parents.len(),
            "weight":   tx.weight,
        }))
        .collect();

    txs.sort_by(|a, b| {
        let ta = a["timestamp"].as_u64().unwrap_or(0);
        let tb = b["timestamp"].as_u64().unwrap_or(0);
        tb.cmp(&ta)
    });
    txs.truncate(50);

    let resp = WsMessage::new(MessageType::ExplorerResponse, serde_json::json!({
        "stats": {
            "total_tx":  stats.total_vertices,
            "tips":      stats.tips,
            "confirmed": stats.confirmed,
            "pending":   stats.pending,
            "difficulty":difficulty,
            "tps":       tps,
            "peers":     peer_count,
            "privacy_by_default": n.diffusion.privacy_by_default,
        },
        "transactions": txs,
    }));
    Some(resp.to_json())
}

async fn handle_partition_handshake(
    payload: serde_json::Value,
    node:    &SharedNode,
) -> Option<String> {
    let peer_cp_id  = payload["checkpoint_id"].as_str()?;
    let peer_seq    = payload["sequence"].as_u64().unwrap_or(0);

    let n = node.lock().await;

    let our_latest_finalized = n.checkpoint_registry.latest_finalized();

    let (common_cp_id, common_seq, ready) = match our_latest_finalized {
        None => {
            warn!("PHA handshake: no finalized checkpoint on our side");
            ("", 0u64, false)
        }
        Some(our_cp) => {
            if peer_seq <= our_cp.sequence {
                match n.checkpoint_registry.get(peer_cp_id) {
                    Some(cp) if cp.is_finalized() => {
                        info!(
                            "PHA Step 1: cp* = {} (seq {}) — common finalized checkpoint found",
                            peer_cp_id, peer_seq
                        );
                        (peer_cp_id, peer_seq, true)
                    }
                    _ => {
                        info!(
                            "PHA Step 1: peer cp unknown locally, falling back to our \
                             latest finalized cp={} seq={}",
                            our_cp.checkpoint_id, our_cp.sequence
                        );
                        (our_cp.checkpoint_id.as_str(), our_cp.sequence, true)
                    }
                }
            } else {
                info!(
                    "PHA Step 1: peer ahead (seq {}), we use our latest finalized cp={} seq={}",
                    peer_seq, our_cp.checkpoint_id, our_cp.sequence
                );
                (our_cp.checkpoint_id.as_str(), our_cp.sequence, true)
            }
        }
    };

    let ack = WsMessage::partition_handshake_ack(common_cp_id, common_seq, ready);
    Some(ack.to_json())
}

async fn handle_partition_sync_request(
    payload: serde_json::Value,
    node:    &SharedNode,
) -> Option<String> {
    let above_cp_id = payload["above_checkpoint_id"].as_str()?;

    let n = node.lock().await;

    let cp_finalized = n.checkpoint_registry.get(above_cp_id)
        .map(|cp| cp.is_finalized())
        .unwrap_or(false);

    if !cp_finalized {
        warn!(
            "PHA sync request for unfinalized checkpoint {} — rejected (Invariant G)",
            above_cp_id
        );
        return Some(WsMessage::partition_sync_response(above_cp_id, serde_json::json!([]), 0).to_json());
    }

    let descendant_ids = n.dag.descendants_of(above_cp_id);

    let txs: Vec<serde_json::Value> = descendant_ids.iter()
        .filter_map(|id| n.dag.get_transaction(id))
        .map(|tx| serde_json::to_value(tx).unwrap_or(serde_json::Value::Null))
        .filter(|v| !v.is_null())
        .collect();

    let count = txs.len();
    info!(
        "PHA Step 4: serving {} transactions above checkpoint {}",
        count, above_cp_id
    );

    let resp = WsMessage::partition_sync_response(
        above_cp_id,
        serde_json::Value::Array(txs),
        count,
    );
    Some(resp.to_json())
}

async fn handle_partition_sync_response(
    payload:  serde_json::Value,
    node:     &SharedNode,
    resolver: &SharedResolver,
) -> Option<String> {
    let cp_id = payload["checkpoint_id"].as_str()?;
    let txs   = payload["transactions"].as_array()?;
    let count = payload["tx_count"].as_u64().unwrap_or(0);

    info!(
        "PHA Step 4: received {} transactions above checkpoint {}",
        count, cp_id
    );

    {
        let n = node.lock().await;
        match n.checkpoint_registry.get(cp_id) {
            Some(cp) if cp.is_finalized() => {},
            _ => {
                warn!("PHA sync response: cp {} not finalized locally — aborting", cp_id);
                return None;
            }
        };
    }
    let mut added = 0usize;
    let mut skipped = 0usize;

    for tx_val in txs {
        let tx: TransactionVertex = match serde_json::from_value(tx_val.clone()) {
            Ok(t)  => t,
            Err(e) => { warn!("PHA: invalid tx in sync response: {}", e); skipped += 1; continue; }
        };

        let tx_id = tx.tx_id.clone();
        let mut n = node.lock().await;

        if n.dag.has_transaction(&tx_id) {
            skipped += 1;
            continue;
        }

        {
            let mut r = resolver.lock().await;
            r.register_transaction(&tx);
        }

        match n.dag.add_transaction(tx) {
            Ok(_) => {
                n.dag.propagate_weight(&tx_id);
                added += 1;
            }
            Err(e) => {
                debug!("PHA: could not add tx {}: {}", &tx_id[..8], e);
                skipped += 1;
            }
        }
    }

    info!(
        "PHA Step 4 complete: added {} txs, skipped {} (already present or invalid)",
        added, skipped
    );

    let cp_anchor = {
        let n = node.lock().await;
        let cp = n.checkpoint_registry.get(cp_id).unwrap().clone();
        CheckpointAnchor::from_dag(
            cp.checkpoint_id.clone(),
            cp.dag_height,
            cp.weight,
            &n.dag,
        )
    };

    let downgraded = {
        let mut r = resolver.lock().await;
        r.pha_downgrade_above(&cp_anchor)
    };

    if !downgraded.is_empty() {
        info!(
            "PHA Step 3: downgraded {} conflict(s) to Reconciling",
            downgraded.len()
        );
        for (sender, nonce) in &downgraded {
            debug!("  Reconciling: sender={} nonce={}", sender, nonce);
        }
    }

    let dag_snapshot = {
        node.lock().await
    };

    let (globally_closed, still_pending) = {
        let mut r = resolver.lock().await;
        r.pha_re_evaluate(&dag_snapshot.dag, &cp_anchor)
    };
    drop(dag_snapshot);

    info!(
        "PHA Steps 5+6 complete: {} conflict(s) → ClosedGlobal, {} still pending (Theorem L)",
        globally_closed, still_pending
    );

    let summary = serde_json::json!({
        "ok": true,
        "cp_star": cp_id,
        "txs_added": added,
        "txs_skipped": skipped,
        "conflicts_globally_closed": globally_closed,
        "conflicts_still_pending": still_pending,
    });
    Some(summary.to_string())
}