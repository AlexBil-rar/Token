// network/src/ws_client.rs

use std::time::Duration;
use std::collections::HashMap;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
    WebSocketStream,
    MaybeTlsStream,
};
use tokio::net::TcpStream;
use ledger::transaction::TransactionVertex;
use crate::ws_message::{WsMessage, MessageType};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct WsClient {
    timeout: Duration,
}

impl WsClient {
    pub fn new() -> Self {
        WsClient { timeout: Duration::from_secs(5) }
    }

    pub fn with_timeout(secs: u64) -> Self {
        WsClient { timeout: Duration::from_secs(secs) }
    }

    pub async fn send_transaction(&self, peer_url: &str, tx: &TransactionVertex) -> bool {
        let payload = serde_json::to_value(tx).unwrap_or_default();
        let msg = WsMessage::new(MessageType::Transaction, payload);
        self.send_msg(peer_url, &msg).await
    }

    pub async fn ping(&self, peer_url: &str) -> bool {
        let msg = WsMessage::ping();
        let url_str = peer_url.to_string();
        let timeout = self.timeout;
        match tokio::time::timeout(timeout, async move {
            let (mut ws, _): (WsStream, _) = connect_async(&url_str).await.ok()?;
            ws.send(Message::Text(msg.to_json())).await.ok()?;
            let raw = ws.next().await?.ok()?;
            if let Message::Text(text) = raw {
                let response = WsMessage::from_json(&text).ok()?;
                if response.msg_type == MessageType::Pong {
                    return Some(true);
                }
            }
            None
        }).await {
            Ok(Some(true)) => true,
            _ => false,
        }
    }

    pub async fn fetch_state(&self, peer_url: &str) -> Option<serde_json::Value> {
        let msg = WsMessage::state_request();
        let url_str = peer_url.to_string();
        let timeout = self.timeout;
        match tokio::time::timeout(timeout, async move {
            let (mut ws, _): (WsStream, _) = connect_async(&url_str).await.ok()?;
            ws.send(Message::Text(msg.to_json())).await.ok()?;
            let raw = ws.next().await?.ok()?;
            if let Message::Text(text) = raw {
                let response = WsMessage::from_json(&text).ok()?;
                if response.msg_type == MessageType::StateResponse {
                    return Some(response.payload);
                }
            }
            None
        }).await {
            Ok(Some(state)) => Some(state),
            _ => None,
        }
    }

    pub async fn broadcast(&self, peers: &[String], tx: &TransactionVertex) -> HashMap<String, bool> {
        let mut results = HashMap::new();
        for peer in peers {
            let ok = self.send_transaction(peer, tx).await;
            results.insert(peer.clone(), ok);
        }
        results
    }

    async fn send_msg(&self, peer_url: &str, msg: &WsMessage) -> bool {
        let url_str = peer_url.to_string();
        let json = msg.to_json();
        let timeout = self.timeout;
        match tokio::time::timeout(timeout, async move {
            let (mut ws, _): (WsStream, _) = connect_async(&url_str).await.ok()?;
            ws.send(Message::Text(json)).await.ok()?;
            Some(true)
        }).await {
            Ok(Some(true)) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = WsClient::new();
        assert_eq!(client.timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_client_custom_timeout() {
        let client = WsClient::with_timeout(10);
        assert_eq!(client.timeout, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn test_ping_unreachable_returns_false() {
        let client = WsClient::with_timeout(1);
        let result = client.ping("ws://127.0.0.1:19999").await;
        assert!(!result);
    }

    #[tokio::test]
    async fn test_fetch_state_unreachable_returns_none() {
        let client = WsClient::with_timeout(1);
        let result = client.fetch_state("ws://127.0.0.1:19999").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_broadcast_empty_peers() {
        let client = WsClient::new();
        let tx = TransactionVertex::new(
            "alice".to_string(), "bob".to_string(),
            100, 1, 1000, "pk".to_string(), vec![],
        );
        let results = client.broadcast(&[], &tx).await;
        assert!(results.is_empty());
    }
}