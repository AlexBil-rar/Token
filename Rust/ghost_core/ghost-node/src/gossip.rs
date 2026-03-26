// ghost-node/src/gossip.rs

#![allow(dead_code)]

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use ledger::transaction::TransactionVertex;
use ledger::privacy::DandelionPhase;
use network::peer_list::PeerList;
use network::ws_message::{WsMessage, MessageType};

pub async fn broadcast_transaction(
    tx: &TransactionVertex,
    peers: Arc<Mutex<PeerList>>,
    exclude: Option<&str>,
) {
    let peer_list = peers.lock().await.get_all();
    if peer_list.is_empty() { return; }

    let payload = match serde_json::to_value(tx) {
        Ok(v) => v,
        Err(e) => { warn!("Failed to serialize transaction: {}", e); return; }
    };

    let msg = WsMessage::new(MessageType::Transaction, payload);
    let json = msg.to_json();
    let exclude_addr = exclude.map(|s| s.to_string());

    for peer_url in peer_list {
        if let Some(ref excl) = exclude_addr {
            if &peer_url == excl { continue; }
        }
        let json = json.clone();
        let peer = peer_url.clone();
        tokio::spawn(async move {
            send_to_peer(&peer, &json).await;
        });
    }
}

pub async fn stem_transaction(
    tx: &TransactionVertex,
    peers: Arc<Mutex<PeerList>>,
    exclude: Option<&str>,
) -> bool {
    let peer_list = peers.lock().await.get_all();
    if peer_list.is_empty() { return false; }

    let candidates: Vec<String> = peer_list.into_iter()
        .filter(|p| exclude.map(|e| p.as_str() != e).unwrap_or(true))
        .collect();

    if candidates.is_empty() { return false; }

    let entropy = tx.tx_id
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    let idx = (entropy as usize) % candidates.len();
    let stem_peer = &candidates[idx];

    const STEM_MAX_TTL: u8 = 10;

    let mut tx_with_ttl = tx.clone();
    if tx_with_ttl.stem_ttl == 0 {
        tx_with_ttl.stem_ttl = STEM_MAX_TTL;
    }
    if tx_with_ttl.stem_ttl <= 1 {
        debug!("Stem TTL exhausted for tx {}... — falling back to fluff",
            &tx.tx_id[..8.min(tx.tx_id.len())]);
        broadcast_transaction(tx, peers, exclude).await;
        return true;
    }
    tx_with_ttl.stem_ttl -= 1;

    let payload = match serde_json::to_value(&tx_with_ttl) {
        Ok(v) => v,
        Err(e) => { warn!("Failed to serialize tx for stem: {}", e); return false; }
    };

    let msg = WsMessage::new(MessageType::Transaction, payload);
    let json = msg.to_json();
    let peer = stem_peer.clone();

    debug!("Dandelion stem: tx {}... → {}", &tx.tx_id[..8.min(tx.tx_id.len())], peer);

    tokio::spawn(async move {
        send_to_peer(&peer, &json).await;
    });

    true
}

pub async fn dandelion_broadcast(
    tx: &TransactionVertex,
    peers: Arc<Mutex<PeerList>>,
    exclude: Option<&str>,
    phase: DandelionPhase,
) {
    match phase {
        DandelionPhase::Stem => {
            let sent = stem_transaction(tx, Arc::clone(&peers), exclude).await;
            if !sent {
                debug!("Stem fallback to fluff for tx {}...", &tx.tx_id[..8.min(tx.tx_id.len())]);
                broadcast_transaction(tx, peers, exclude).await;
            }
        }
        DandelionPhase::Fluff => {
            broadcast_transaction(tx, peers, exclude).await;
        }
    }
}

async fn send_to_peer(peer_url: &str, json: &str) {
    use futures_util::SinkExt;
    use tokio_tungstenite::{
        connect_async, tungstenite::Message,
        WebSocketStream, MaybeTlsStream,
    };
    use tokio::net::TcpStream;
    type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

    let url = peer_url.to_string();
    let json = json.to_string();

    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        async move {
            let (mut ws, _): (WsStream, _) = connect_async(&url).await.ok()?;
            ws.send(Message::Text(json)).await.ok()?;
            let _ = ws.close(None).await;
            Some(())
        }
    ).await {
        Ok(Some(_)) => debug!("Gossip sent to {}", peer_url),
        _ => debug!("Gossip failed to {}", peer_url),
    }
}

pub async fn announce_peer(
    new_peer: &str,
    peers: Arc<Mutex<PeerList>>,
) {
    let peer_list = peers.lock().await.get_all();
    let msg = WsMessage::new(
        MessageType::PeerList,
        serde_json::json!({"peers": [new_peer]}),
    );
    let json = msg.to_json();
    for peer_url in peer_list {
        if peer_url == new_peer { continue; }
        let json = json.clone();
        let peer = peer_url.clone();
        tokio::spawn(async move {
            send_to_peer(&peer, &json).await;
        });
    }
}