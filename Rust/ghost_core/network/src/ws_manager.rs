// network/src/ws_manager.rs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message;
use crate::ws_message::WsMessage;

pub type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

pub struct WsConnectionManager {
    connections: HashMap<String, Arc<Mutex<WsSink>>>,
}

impl WsConnectionManager {
    pub fn new() -> Self {
        WsConnectionManager {
            connections: HashMap::new(),
        }
    }

    pub fn register(&mut self, address: String, sink: Arc<Mutex<WsSink>>) {
        self.connections.insert(address, sink);
    }

    pub fn unregister(&mut self, address: &str) {
        self.connections.remove(address);
    }

    pub fn is_connected(&self, address: &str) -> bool {
        self.connections.contains_key(address)
    }

    pub fn get_active_peers(&self) -> Vec<String> {
        self.connections.keys().cloned().collect()
    }

    pub async fn send(&mut self, address: &str, msg: &WsMessage) -> bool {
        let sink = match self.connections.get(address) {
            Some(s) => s.clone(),
            None => return false,
        };

        let json = msg.to_json();
        let mut sink_guard = sink.lock().await;
        match sink_guard.send(Message::Text(json)).await {
            Ok(_) => true,
            Err(_) => {
                drop(sink_guard);
                self.connections.remove(address);
                false
            }
        }
    }

    pub async fn broadcast(&mut self, msg: &WsMessage, exclude: &str) -> HashMap<String, bool> {
        let peers: Vec<String> = self.connections.keys().cloned().collect();
        let mut results = HashMap::new();

        for peer in peers {
            if peer == exclude {
                continue;
            }
            let ok = self.send(&peer, msg).await;
            results.insert(peer, ok);
        }

        results
    }

    pub fn stats(&self) -> ConnectionStats {
        ConnectionStats {
            active_connections: self.connections.len(),
            peers: self.get_active_peers(),
        }
    }
}

pub struct ConnectionStats {
    pub active_connections: usize,
    pub peers: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_new_is_empty() {
        let manager = WsConnectionManager::new();
        assert_eq!(manager.get_active_peers().len(), 0);
    }

    #[test]
    fn test_is_connected_false_for_unknown() {
        let manager = WsConnectionManager::new();
        assert!(!manager.is_connected("peer1"));
    }

    #[test]
    fn test_stats_empty() {
        let manager = WsConnectionManager::new();
        let stats = manager.stats();
        assert_eq!(stats.active_connections, 0);
        assert!(stats.peers.is_empty());
    }

    #[test]
    fn test_unregister_nonexistent_is_ok() {
        let mut manager = WsConnectionManager::new();
        manager.unregister("nonexistent"); 
    }
}