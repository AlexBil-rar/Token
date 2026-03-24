// network/src/ws_message.rs

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Transaction,
    Ping,
    Pong,
    StateRequest,
    StateResponse,
    PeerList,
    DifficultyRequest,
    DifficultyResponse,
    ExplorerRequest,
    ExplorerResponse,
    CheckpointRequest,
    CheckpointResponse,
    PartitionHandshake,
    PartitionHandshakeAck,
    PartitionSyncRequest,
    PartitionSyncResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage {
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    pub payload: serde_json::Value,
    pub timestamp: f64,
    pub sender: String,
}

impl WsMessage {
    pub fn new(msg_type: MessageType, payload: serde_json::Value) -> Self {
        WsMessage {
            msg_type,
            payload,
            timestamp: now_secs(),
            sender: String::new(),
        }
    }

    pub fn with_sender(mut self, sender: &str) -> Self {
        self.sender = sender.to_string();
        self
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    pub fn from_json(data: &str) -> Result<Self, String> {
        serde_json::from_str(data).map_err(|e| format!("parse error: {}", e))
    }

    pub fn ping() -> Self {
        WsMessage::new(MessageType::Ping, serde_json::json!({}))
    }

    pub fn pong(sender: &str) -> Self {
        WsMessage::new(MessageType::Pong, serde_json::json!({"status": "ok"}))
            .with_sender(sender)
    }

    pub fn state_request() -> Self {
        WsMessage::new(MessageType::StateRequest, serde_json::json!({}))
    }

    pub fn peer_list(peers: &[String]) -> Self {
        WsMessage::new(MessageType::PeerList, serde_json::json!({ "peers": peers }))
    }

    pub fn checkpoint_request() -> Self {
        WsMessage::new(MessageType::CheckpointRequest, serde_json::json!({}))
    }

    pub fn checkpoint_response(
        checkpoint_id: &str,
        state_root: &str,
        sequence: u64,
        dag_height: u64,
        address_count: usize,
        timestamp: u64,
        is_finalized: bool,
    ) -> Self {
        WsMessage::new(MessageType::CheckpointResponse, serde_json::json!({
            "checkpoint_id": checkpoint_id,
            "state_root": state_root,
            "sequence": sequence,
            "dag_height": dag_height,
            "address_count": address_count,
            "timestamp": timestamp,
            "is_finalized": is_finalized,
        }))
    }

    // ── PHA messages ─────────────────────────────────────────────────────────

    pub fn partition_handshake(
        my_checkpoint_id: &str,
        my_dag_height: u64,
        my_sequence: u64,
    ) -> Self {
        WsMessage::new(MessageType::PartitionHandshake, serde_json::json!({
            "checkpoint_id": my_checkpoint_id,
            "dag_height":    my_dag_height,
            "sequence":      my_sequence,
        }))
    }

    pub fn partition_handshake_ack(
        common_checkpoint_id: &str,
        common_sequence: u64,
        ready_to_sync: bool,
    ) -> Self {
        WsMessage::new(MessageType::PartitionHandshakeAck, serde_json::json!({
            "common_checkpoint_id": common_checkpoint_id,
            "common_sequence":      common_sequence,
            "ready_to_sync":        ready_to_sync,
        }))
    }

    pub fn partition_sync_request(above_checkpoint_id: &str) -> Self {
        WsMessage::new(MessageType::PartitionSyncRequest, serde_json::json!({
            "above_checkpoint_id": above_checkpoint_id,
        }))
    }

    pub fn partition_sync_response(
        checkpoint_id: &str,
        transactions: serde_json::Value,
        tx_count: usize,
    ) -> Self {
        WsMessage::new(MessageType::PartitionSyncResponse, serde_json::json!({
            "checkpoint_id": checkpoint_id,
            "transactions":  transactions,
            "tx_count":      tx_count,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_to_json() {
        let msg = WsMessage::ping();
        let json = msg.to_json();
        assert!(json.contains("ping"));
        assert!(json.contains("timestamp"));
    }

    #[test]
    fn test_message_from_json() {
        let raw = r#"{"type":"transaction","payload":{"tx_id":"abc123"},"timestamp":1000.0,"sender":"node1"}"#;
        let msg = WsMessage::from_json(raw).unwrap();
        assert_eq!(msg.msg_type, MessageType::Transaction);
        assert_eq!(msg.sender, "node1");
        assert_eq!(msg.payload["tx_id"], "abc123");
    }

    #[test]
    fn test_message_roundtrip() {
        let msg = WsMessage::new(
            MessageType::PeerList,
            serde_json::json!({"peers": ["ws://1.2.3.4:9000"]}),
        ).with_sender("node_a");

        let restored = WsMessage::from_json(&msg.to_json()).unwrap();
        assert_eq!(restored.msg_type, msg.msg_type);
        assert_eq!(restored.sender, msg.sender);
        assert_eq!(restored.payload, msg.payload);
    }

    #[test]
    fn test_all_message_types_serialize() {
        let types: Vec<MessageType> = vec![
            MessageType::Transaction,
            MessageType::Ping,
            MessageType::Pong,
            MessageType::StateRequest,
            MessageType::StateResponse,
            MessageType::PeerList,
            MessageType::CheckpointRequest,
            MessageType::CheckpointResponse,
            MessageType::PartitionHandshake,
            MessageType::PartitionHandshakeAck,
            MessageType::PartitionSyncRequest,
            MessageType::PartitionSyncResponse,
        ];

        for mt in types {
            let msg = WsMessage::new(mt.clone(), serde_json::json!({}));
            let restored = WsMessage::from_json(&msg.to_json()).unwrap();
            assert_eq!(restored.msg_type, mt);
        }
    }

    #[test]
    fn test_ping_pong() {
        let ping = WsMessage::ping();
        assert_eq!(ping.msg_type, MessageType::Ping);

        let pong = WsMessage::pong("node1");
        assert_eq!(pong.msg_type, MessageType::Pong);
        assert_eq!(pong.sender, "node1");
    }

    #[test]
    fn test_peer_list_message() {
        let peers = vec!["ws://1.2.3.4:9000".to_string(), "ws://5.6.7.8:9000".to_string()];
        let msg = WsMessage::peer_list(&peers);
        assert_eq!(msg.msg_type, MessageType::PeerList);
        assert_eq!(msg.payload["peers"][0], "ws://1.2.3.4:9000");
    }

    #[test]
    fn test_partition_handshake_roundtrip() {
        let msg = WsMessage::partition_handshake("cp_abc", 1500, 3);
        assert_eq!(msg.msg_type, MessageType::PartitionHandshake);
        let r = WsMessage::from_json(&msg.to_json()).unwrap();
        assert_eq!(r.payload["checkpoint_id"], "cp_abc");
        assert_eq!(r.payload["dag_height"],    1500);
        assert_eq!(r.payload["sequence"],      3);
    }

    #[test]
    fn test_partition_handshake_ack_roundtrip() {
        let msg = WsMessage::partition_handshake_ack("cp_common", 2, true);
        assert_eq!(msg.msg_type, MessageType::PartitionHandshakeAck);
        let r = WsMessage::from_json(&msg.to_json()).unwrap();
        assert_eq!(r.payload["common_checkpoint_id"], "cp_common");
        assert_eq!(r.payload["ready_to_sync"],        true);
    }

    #[test]
    fn test_partition_sync_request_roundtrip() {
        let msg = WsMessage::partition_sync_request("cp_star_123");
        assert_eq!(msg.msg_type, MessageType::PartitionSyncRequest);
        let r = WsMessage::from_json(&msg.to_json()).unwrap();
        assert_eq!(r.payload["above_checkpoint_id"], "cp_star_123");
    }

    #[test]
    fn test_partition_sync_response_roundtrip() {
        let txs = serde_json::json!([{"tx_id": "abc"}, {"tx_id": "def"}]);
        let msg = WsMessage::partition_sync_response("cp_star_123", txs, 2);
        assert_eq!(msg.msg_type, MessageType::PartitionSyncResponse);
        let r = WsMessage::from_json(&msg.to_json()).unwrap();
        assert_eq!(r.payload["tx_count"],    2);
        assert_eq!(r.payload["checkpoint_id"], "cp_star_123");
        assert_eq!(r.payload["transactions"][0]["tx_id"], "abc");
    }

    #[test]
    fn test_invalid_json_returns_error() {
        let result = WsMessage::from_json("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_checkpoint_response_roundtrip() {
        let msg = WsMessage::checkpoint_response(
            "cp_id", "merkle_root", 3, 1500, 10, 1_700_000_000, false
        );
        let restored = WsMessage::from_json(&msg.to_json()).unwrap();
        assert_eq!(restored.msg_type, MessageType::CheckpointResponse);
        assert_eq!(restored.payload["dag_height"], 1500);
    }
}