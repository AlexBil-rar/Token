// ghost-node/src/bootstrap.rs

use tracing::{info, warn};
use network::ws_client::WsClient;
use network::ws_message::{WsMessage, MessageType};


const BOOTSTRAP_NODES: &[&str] = &[
];

pub async fn discover_peers(
    known_peers: &[String],
    self_port: u16,
) -> Vec<String> {
    let client = WsClient::with_timeout(5);
    let self_addr = format!("ws://127.0.0.1:{}", self_port);

    let mut all_candidates: Vec<String> = BOOTSTRAP_NODES
        .iter()
        .map(|s| s.to_string())
        .collect();
    all_candidates.extend_from_slice(known_peers);

    let mut discovered: Vec<String> = Vec::new();

    for peer_url in &all_candidates {
        if *peer_url == self_addr {
            continue; 
        }

        info!("Querying peer list from {}...", peer_url);

        match fetch_peer_list(&client, peer_url).await {
            Some(peers) => {
                info!("Got {} peers from {}", peers.len(), peer_url);
                for p in peers {
                    if p != self_addr && !discovered.contains(&p) {
                        discovered.push(p);
                    }
                }
                if !discovered.contains(peer_url) {
                    discovered.push(peer_url.clone());
                }
            }
            None => {
                warn!("Could not get peer list from {}", peer_url);
                if client.ping(peer_url).await {
                    if !discovered.contains(peer_url) {
                        discovered.push(peer_url.clone());
                    }
                }
            }
        }
    }

    info!("Discovered {} peers total", discovered.len());
    discovered
}

async fn fetch_peer_list(
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
        std::time::Duration::from_secs(5),
        async move {
            let (mut ws, _): (WsStream, _) = connect_async(&url_str).await.ok()?;
            ws.send(Message::Text(msg.to_json())).await.ok()?;
            let raw = ws.next().await?.ok()?;
            let _ = ws.close(None).await;
            if let Message::Text(text) = raw {
                let response = WsMessage::from_json(&text).ok()?;
                if let Some(peers) = response.payload.get("peers") {
                    let list: Vec<String> = peers
                        .as_array()?
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    return Some(list);
                }
            }
            None
        }
    ).await {
        Ok(Some(peers)) => Some(peers),
        _ => None,
    }
}

pub async fn announce_self(
    self_addr: &str,
    peers: &[String],
) {
    let _client = WsClient::with_timeout(3);
    let msg = WsMessage::new(
        MessageType::PeerList,
        serde_json::json!({"peers": [self_addr]}),
    );

    for peer_url in peers {
        let url = peer_url.clone();
        let json = msg.to_json();
        tokio::spawn(async move {
            use futures_util::SinkExt;
            use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream, MaybeTlsStream};
            use tokio::net::TcpStream;
            type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

            if let Ok(result) = connect_async(&url).await {
                let (mut ws, _) = result;
                ws.send(Message::Text(json)).await.ok();
                let _ = ws.close(None).await;
            }
        });
    }
}