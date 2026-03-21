// ghost-node/src/gossip.rs

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use ledger::transaction::TransactionVertex;
use network::peer_list::PeerList;
use network::ws_message::{WsMessage, MessageType};

pub async fn broadcast_transaction(
    tx: &TransactionVertex,
    peers: Arc<Mutex<PeerList>>,
    exclude: Option<&str>,
) {
    let peer_list = peers.lock().await.get_all();

    if peer_list.is_empty() {
        return;
    }

    let payload = match serde_json::to_value(tx) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to serialize transaction: {}", e);
            return;
        }
    };

    let msg = WsMessage::new(MessageType::Transaction, payload);
    let json = msg.to_json();
    let exclude_addr = exclude.map(|s| s.to_string());

    for peer_url in peer_list {
        if let Some(ref excl) = exclude_addr {
            if &peer_url == excl {
                continue;
            }
        }

        let json = json.clone();
        let peer = peer_url.clone();

        tokio::spawn(async move {
            send_to_peer(&peer, &json).await;
        });
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
        Ok(Some(_)) => {
            debug!("Gossip sent to {}", peer_url);
        }
        _ => {
            debug!("Gossip failed to {}", peer_url);
        }
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
        if peer_url == new_peer {
            continue;
        }
        let json = json.clone();
        let peer = peer_url.clone();
        tokio::spawn(async move {
            send_to_peer(&peer, &json).await;
        });
    }
}