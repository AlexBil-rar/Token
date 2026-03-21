// ghost-node/src/peer_discovery.rs

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, debug, warn};

use network::peer_list::PeerList;
use network::ws_client::WsClient;
use network::ws_message::{WsMessage, MessageType};

pub async fn run_discovery_loop(
    peers: Arc<Mutex<PeerList>>,
    self_port: u16,
    interval_secs: u64,
) {
    let mut interval = tokio::time::interval(
        std::time::Duration::from_secs(interval_secs)
    );

    loop {
        interval.tick().await;

        let peer_list = peers.lock().await.get_all();
        if peer_list.is_empty() {
            continue;
        }

        let client = WsClient::with_timeout(3);
        let self_addr = format!("ws://127.0.0.1:{}", self_port);

        let mut new_peers: Vec<String> = Vec::new();

        for peer_url in &peer_list {
            match fetch_peers_from(&client, peer_url).await {
                Some(remote_peers) => {
                    for p in remote_peers {
                        if p != self_addr {
                            new_peers.push(p);
                        }
                    }
                }
                None => {
                    debug!("Peer {} did not respond to discovery", peer_url);
                }
            }
        }

        if !new_peers.is_empty() {
            let mut peer_list = peers.lock().await;
            let before = peer_list.size();
            for p in &new_peers {
                peer_list.add(p);
            }
            let after = peer_list.size();
            if after > before {
                info!("Discovered {} new peers (total: {})", after - before, after);
            }
        }
    }
}

async fn fetch_peers_from(
    _client: &WsClient,
    peer_url: &str,
) -> Option<Vec<String>> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream, MaybeTlsStream};
    use tokio::net::TcpStream;
    type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

    let url_str = peer_url.to_string();
    let msg = WsMessage::new(
        MessageType::PeerList,
        serde_json::json!({"request": true}),
    );

    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        async move {
            let (mut ws, _): (WsStream, _) = connect_async(&url_str).await.ok()?;
            ws.send(Message::Text(msg.to_json())).await.ok()?;
            let raw = ws.next().await?.ok()?;
            let _ = ws.close(None).await;
            if let Message::Text(text) = raw {
                let response = WsMessage::from_json(&text).ok()?;
                if let Some(arr) = response.payload.get("peers").and_then(|p| p.as_array()) {
                    return Some(
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    );
                }
            }
            None
        }
    ).await {
        Ok(Some(peers)) => Some(peers),
        _ => None,
    }
}

pub async fn health_check(
    peers: Arc<Mutex<PeerList>>,
) {
    let peer_list = peers.lock().await.get_all();
    let client = WsClient::with_timeout(3);

    let mut dead: Vec<String> = Vec::new();

    for peer_url in &peer_list {
        if !client.ping(peer_url).await {
            dead.push(peer_url.clone());
        }
    }

    if !dead.is_empty() {
        let mut peer_list = peers.lock().await;
        for p in &dead {
            peer_list.remove(p);
            warn!("Removed dead peer: {}", p);
        }
    }
}