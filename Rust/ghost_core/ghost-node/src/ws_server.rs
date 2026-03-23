// ghost-node/src/ws_server.rs

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

use crate::gossip;

pub type SharedNode = Arc<Mutex<Node>>;
pub type SharedPeers = Arc<Mutex<PeerList>>;

pub async fn start(
    port: u16,
    node: SharedNode,
    peers: SharedPeers,
) -> Result<(), String> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await
        .map_err(|e| format!("failed to bind {}: {}", addr, e))?;

    info!("WebSocket server listening on ws://{}", addr);

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                debug!("Incoming connection from {}", peer_addr);
                let node = Arc::clone(&node);
                let peers = Arc::clone(&peers);
                tokio::spawn(handle_connection(stream, node, peers));
            }
            Err(e) => {
                warn!("Accept error: {}", e);
            }
        }
    }
}

async fn handle_connection(
    stream: TcpStream,
    node: SharedNode,
    peers: SharedPeers,
) {
    let peer_addr = stream.peer_addr()
        .map(|a| a.to_string())
        .unwrap_or("unknown".to_string());

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
                if text.len() > 1_048_576 {
                    warn!("Oversized message from {}: {} bytes", peer_addr, text.len());
                    break;
                }
                let response = handle_message(&text, &node, &peers).await;
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
            Ok(Message::Ping(data)) => {
                let _ = sender.send(Message::Pong(data)).await;
            }
            Err(e) => {
                warn!("WebSocket error from {}: {}", peer_addr, e);
                break;
            }
            _ => {}
        }
    }
}

async fn handle_message(
    raw: &str,
    node: &SharedNode,
    peers: &SharedPeers,
) -> Option<String> {
    let msg = match WsMessage::from_json(raw) {
        Ok(m) => m,
        Err(e) => {
            warn!("Invalid message: {}", e);
            return None;
        }
    };

    match msg.msg_type {
        MessageType::Ping => {
            Some(WsMessage::pong("ghostledger").to_json())
        }

        MessageType::Transaction => {
            handle_transaction(msg.payload, node, peers).await
        }

        MessageType::DifficultyRequest => {
            let difficulty = node.lock().await.current_difficulty();
            let resp = WsMessage::new(
                MessageType::DifficultyResponse,
                serde_json::json!({"difficulty": difficulty}),
            );
            Some(resp.to_json())
        }

        MessageType::StateRequest => {
            handle_state_request(node).await
        }

        MessageType::PeerList => {
            handle_peer_list(msg.payload, peers).await
        }

        MessageType::ExplorerRequest => {
            handle_explorer_request(node, peers).await
        }

        _ => None,
    }
}

async fn handle_transaction(
    payload: serde_json::Value,
    node: &SharedNode,
    peers: &SharedPeers,
) -> Option<String> {
    let tx: TransactionVertex = match serde_json::from_value(payload.clone()) {
        Ok(t) => t,
        Err(e) => {
            warn!("Invalid transaction payload: {}", e);
            let resp = serde_json::json!({
                "ok": false,
                "code": "invalid_payload",
                "reason": e.to_string()
            });
            return Some(resp.to_string());
        }
    };

    let tx_id_short = tx.tx_id[..16.min(tx.tx_id.len())].to_string();
    let tx_clone = tx.clone();

    let result = {
        let mut n = node.lock().await;
        n.submit_transaction(tx)
    };

    if result.ok {
        info!("Transaction accepted: {}...", tx_id_short);
        gossip::broadcast_transaction(&tx_clone, Arc::clone(peers), None).await;
    } else {
        debug!("Transaction rejected: {} — {}", tx_id_short, result.reason);
    }

    let resp = serde_json::json!({
        "ok": result.ok,
        "code": result.code,
        "reason": result.reason,
    });
    Some(resp.to_string())
}

async fn handle_state_request(node: &SharedNode) -> Option<String> {
    let n = node.lock().await;
    let state_view = serde_json::json!({
        "balances": n.state.balances,
        "nonces": n.state.nonces,
    });
    let resp = WsMessage::new(MessageType::StateResponse, state_view);
    Some(resp.to_json())
}

async fn handle_peer_list(
    payload: serde_json::Value,
    peers: &SharedPeers,
) -> Option<String> {
    if let Some(peer_arr) = payload.get("peers").and_then(|p| p.as_array()) {
        let mut peer_list = peers.lock().await;
        for p in peer_arr {
            if let Some(addr) = p.as_str() {
                peer_list.add(addr);
                debug!("Added peer from peer list: {}", addr);
            }
        }
        let my_peers = peer_list.get_all();
        let resp_payload = serde_json::json!({ "peers": my_peers });
        let resp = WsMessage::new(MessageType::PeerList, resp_payload);
        Some(resp.to_json())
    } else {
        None
    }
}

async fn handle_explorer_request(
    node: &SharedNode,
    peers: &SharedPeers,
) -> Option<String> {
    let n = node.lock().await;
    let stats = n.dag_stats();
    let difficulty = n.current_difficulty();
    let tps = n.anti_spam.current_tps();
    let peer_count = peers.lock().await.size();

    let mut txs: Vec<serde_json::Value> = n.dag.vertices.values()
        .map(|tx| serde_json::json!({
            "tx_id": &tx.tx_id[..16.min(tx.tx_id.len())],
            "sender": &tx.sender[..8.min(tx.sender.len())],
            "receiver": &tx.receiver[..8.min(tx.receiver.len())],
            "amount": if tx.commitment.is_some() {
                serde_json::Value::String("private".to_string())
            } else {
                serde_json::Value::Number(tx.amount.into())
            },
            "private": tx.commitment.is_some(),
            "status": tx.status.as_str(),
            "timestamp": tx.timestamp,
            "parents": tx.parents.len(),
            "weight": tx.weight,
        }))
        .collect();

    txs.sort_by(|a, b| {
        let ta = a["timestamp"].as_u64().unwrap_or(0);
        let tb = b["timestamp"].as_u64().unwrap_or(0);
        tb.cmp(&ta)
    });
    txs.truncate(50);

    let payload = serde_json::json!({
        "stats": {
            "total_tx": stats.total_vertices,
            "tips": stats.tips,
            "confirmed": stats.confirmed,
            "pending": stats.pending,
            "difficulty": difficulty,
            "tps": tps,
            "peers": peer_count,
        },
        "transactions": txs,
    });

    let resp = WsMessage::new(MessageType::ExplorerResponse, payload);
    Some(resp.to_json())
}