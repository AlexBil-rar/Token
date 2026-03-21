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
use network::ws_client::WsClient;
use network::ws_message::{WsMessage, MessageType};

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

        MessageType::StateRequest => {
            handle_state_request(node).await
        }

        MessageType::PeerList => {
            handle_peer_list(msg.payload, peers).await;
            None
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
    let result = {
        let mut n = node.lock().await;
        n.submit_transaction(tx)
    };

    if result.ok {
        info!("Transaction accepted: {}...", tx_id_short);

        let peer_list = peers.lock().await.get_all();
        if !peer_list.is_empty() {
            let broadcast_msg = WsMessage::new(MessageType::Transaction, payload);
            let json = broadcast_msg.to_json();
            for peer_url in peer_list {
                let json = json.clone();
                tokio::spawn(async move {
                    debug!("Gossiping tx to {}", peer_url);
                    let client = WsClient::with_timeout(3);
                    let msg = WsMessage::from_json(&json).unwrap_or_else(|_| WsMessage::ping());
                    let _ = client.ping(&peer_url).await;
                    drop(msg);
                });
            }
        }
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
) {
    if let Some(peer_arr) = payload.get("peers").and_then(|p| p.as_array()) {
        let mut peer_list = peers.lock().await;
        for p in peer_arr {
            if let Some(addr) = p.as_str() {
                peer_list.add(addr);
                debug!("Added peer from peer list: {}", addr);
            }
        }
    }
}