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
                if is_subnet_allowed(&peer_list, p) {
                    peer_list.add(p);
                } else {
                    debug!("Peer {} rejected: subnet diversity limit", p);
                }
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

    pub async fn check_and_respond_eclipse(peers: Arc<Mutex<PeerList>>) {
        use network::peer_list::EclipseCheck;
    
        let check = peers.lock().await.check_eclipse();
        if let EclipseCheck::Suspected { subnet, count, total } = check {
            warn!(
                "Eclipse suspected: subnet {} has {}/{} peers — rotating",
                subnet, count, total
            );
    
            let to_drop: Vec<String> = {
                let pl = peers.lock().await;
                pl.get_all().into_iter()
                    .filter(|addr| {
                        addr.trim_start_matches("ws://")
                            .trim_start_matches("wss://")
                            .starts_with(&subnet)
                    })
                    .take(count / 2)
                    .collect()
            };
    
            {
                let mut pl = peers.lock().await;
                for addr in &to_drop {
                    pl.remove(addr);
                    warn!("Eclipse mitigation: dropped peer {}", addr);
                }
            }
        }
    }
    
}

pub fn is_subnet_allowed(peers: &PeerList, new_addr: &str) -> bool {
    let total = peers.size();
    if total < 5 {
        return true;
    }

    let new_subnet = extract_subnet(new_addr);
    if new_subnet.is_none() {
        return true;
    }
    let new_subnet = new_subnet.unwrap();

    let same_subnet = peers.get_all().iter()
        .filter(|addr| extract_subnet(addr).as_deref() == Some(&new_subnet))
        .count();

    let ratio_after = (same_subnet + 1) as f64 / (total + 1) as f64;
    ratio_after <= 0.60
}

fn extract_subnet(address: &str) -> Option<String> {
    let host = address
        .trim_start_matches("wss://")
        .trim_start_matches("ws://");
    let ip = if let Some(pos) = host.rfind(':') { &host[..pos] } else { host };
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        Some(format!("{}.{}", parts[0], parts[1]))
    } else {
        None
    }
}