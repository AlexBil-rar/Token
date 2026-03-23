// ledger/src/batch.rs

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_BATCH_SIZE: usize = 16;

pub const MIN_BATCH_SIZE: usize = 2;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchOutput {
    pub receiver: String,
    pub amount: u64,
    pub commitment: Option<String>,
    pub ephemeral_pubkey: Option<String>,
}

impl BatchOutput {
    pub fn transparent(receiver: String, amount: u64) -> Self {
        BatchOutput { receiver, amount, commitment: None, ephemeral_pubkey: None }
    }

    pub fn private(receiver: String, commitment: String, ephemeral_pubkey: String) -> Self {
        BatchOutput {
            receiver,
            amount: 0,
            commitment: Some(commitment),
            ephemeral_pubkey: Some(ephemeral_pubkey),
        }
    }

    pub fn is_private(&self) -> bool {
        self.commitment.is_some()
    }

    pub fn value(&self) -> u64 {
        self.amount
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchTransaction {
    pub batch_id: String,
    pub sender: String,
    pub outputs: Vec<BatchOutput>,
    pub total_amount: u64,
    pub nonce: u64,
    pub timestamp: u64,
    pub public_key: String,
    pub parents: Vec<String>,
    pub signature: String,
    pub anti_spam_nonce: u64,
    pub anti_spam_hash: String,
}

impl BatchTransaction {
    pub fn new(
        sender: String,
        outputs: Vec<BatchOutput>,
        nonce: u64,
        public_key: String,
        parents: Vec<String>,
    ) -> Result<Self, String> {
        if outputs.len() < MIN_BATCH_SIZE {
            return Err(format!(
                "batch requires at least {} outputs, got {}",
                MIN_BATCH_SIZE,
                outputs.len()
            ));
        }

        if outputs.len() > MAX_BATCH_SIZE {
            return Err(format!(
                "batch exceeds max size: {} > {}",
                outputs.len(),
                MAX_BATCH_SIZE
            ));
        }

        let has_private = outputs.iter().any(|o| o.is_private());
        let has_transparent = outputs.iter().any(|o| !o.is_private());
        if has_private && has_transparent {
            return Err("cannot mix private and transparent outputs in one batch".to_string());
        }

        let total_amount: u64 = outputs.iter().map(|o| o.amount).sum();
        let timestamp = now_secs();

        let mut tx = BatchTransaction {
            batch_id: String::new(),
            sender,
            outputs,
            total_amount,
            nonce,
            timestamp,
            public_key,
            parents,
            signature: String::new(),
            anti_spam_nonce: 0,
            anti_spam_hash: String::new(),
        };

        tx.batch_id = tx.compute_id();
        Ok(tx)
    }

    pub fn output_count(&self) -> usize {
        self.outputs.len()
    }

    pub fn receivers(&self) -> Vec<&str> {
        self.outputs.iter().map(|o| o.receiver.as_str()).collect()
    }

    pub fn is_private(&self) -> bool {
        self.outputs.iter().all(|o| o.is_private())
    }

    pub fn compute_id(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.sender.as_bytes());
        h.update(self.nonce.to_le_bytes());
        h.update(self.timestamp.to_le_bytes());
        for output in &self.outputs {
            h.update(output.receiver.as_bytes());
            h.update(output.amount.to_le_bytes());
            if let Some(c) = &output.commitment {
                h.update(c.as_bytes());
            }
        }
        hex::encode(h.finalize())
    }

    pub fn validate_structure(&self) -> Result<(), String> {
        if self.sender.is_empty() {
            return Err("sender is empty".to_string());
        }
        if self.outputs.is_empty() {
            return Err("outputs is empty".to_string());
        }
        if self.outputs.len() > MAX_BATCH_SIZE {
            return Err(format!("too many outputs: {}", self.outputs.len()));
        }
        if self.nonce == 0 {
            return Err("nonce must be positive".to_string());
        }
        if self.public_key.is_empty() {
            return Err("public_key is empty".to_string());
        }
        for (i, output) in self.outputs.iter().enumerate() {
            if output.receiver.is_empty() {
                return Err(format!("output[{}]: receiver is empty", i));
            }
            if !output.is_private() && output.amount == 0 {
                return Err(format!("output[{}]: amount must be positive for transparent output", i));
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct BatchAccumulator {
    pending: std::collections::HashMap<String, Vec<BatchOutput>>,
    first_seen: std::collections::HashMap<String, u64>,
    pub flush_timeout_secs: u64,
}

impl BatchAccumulator {
    pub fn new(flush_timeout_secs: u64) -> Self {
        BatchAccumulator {
            pending: std::collections::HashMap::new(),
            first_seen: std::collections::HashMap::new(),
            flush_timeout_secs,
        }
    }

    pub fn push(&mut self, sender: &str, output: BatchOutput) -> Option<Vec<BatchOutput>> {
        let now = now_secs();
        let queue = self.pending.entry(sender.to_string()).or_default();
        self.first_seen.entry(sender.to_string()).or_insert(now);

        queue.push(output);

        if queue.len() >= MAX_BATCH_SIZE {
            return Some(self.flush(sender));
        }

        None
    }

    pub fn flush(&mut self, sender: &str) -> Vec<BatchOutput> {
        self.first_seen.remove(sender);
        self.pending.remove(sender).unwrap_or_default()
    }

    pub fn flush_timed_out(&mut self) -> std::collections::HashMap<String, Vec<BatchOutput>> {
        let now = now_secs();
        let timeout = self.flush_timeout_secs;

        let timed_out: Vec<String> = self.first_seen
            .iter()
            .filter(|(_, &t)| now.saturating_sub(t) >= timeout)
            .map(|(s, _)| s.clone())
            .collect();

        let mut result = std::collections::HashMap::new();
        for sender in timed_out {
            let outputs = self.flush(&sender);
            if !outputs.is_empty() {
                result.insert(sender, outputs);
            }
        }
        result
    }

    pub fn pending_count(&self, sender: &str) -> usize {
        self.pending.get(sender).map(|v| v.len()).unwrap_or(0)
    }

    pub fn total_pending(&self) -> usize {
        self.pending.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output(receiver: &str, amount: u64) -> BatchOutput {
        BatchOutput::transparent(receiver.to_string(), amount)
    }

    fn make_outputs(n: usize) -> Vec<BatchOutput> {
        (0..n).map(|i| make_output(&format!("addr_{}", i), 100)).collect()
    }

    #[test]
    fn test_transparent_output() {
        let o = BatchOutput::transparent("alice".to_string(), 100);
        assert!(!o.is_private());
        assert_eq!(o.value(), 100);
    }

    #[test]
    fn test_private_output() {
        let o = BatchOutput::private(
            "stealth_addr".to_string(),
            "commitment_hex".to_string(),
            "ephem_pk".to_string(),
        );
        assert!(o.is_private());
        assert_eq!(o.value(), 0);
    }

    #[test]
    fn test_create_batch_success() {
        let outputs = make_outputs(3);
        let batch = BatchTransaction::new(
            "alice".to_string(), outputs, 1, "pk".to_string(), vec![],
        );
        assert!(batch.is_ok());
        let b = batch.unwrap();
        assert_eq!(b.output_count(), 3);
        assert_eq!(b.total_amount, 300);
    }

    #[test]
    fn test_batch_too_few_outputs() {
        let outputs = make_outputs(1);
        let result = BatchTransaction::new(
            "alice".to_string(), outputs, 1, "pk".to_string(), vec![],
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("at least"));
    }

    #[test]
    fn test_batch_too_many_outputs() {
        let outputs = make_outputs(MAX_BATCH_SIZE + 1);
        let result = BatchTransaction::new(
            "alice".to_string(), outputs, 1, "pk".to_string(), vec![],
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds max"));
    }

    #[test]
    fn test_batch_mixed_private_transparent_rejected() {
        let mut outputs = make_outputs(2);
        outputs.push(BatchOutput::private(
            "stealth".to_string(),
            "commit".to_string(),
            "ephem".to_string(),
        ));
        let result = BatchTransaction::new(
            "alice".to_string(), outputs, 1, "pk".to_string(), vec![],
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("mix"));
    }

    #[test]
    fn test_batch_id_deterministic() {
        let outputs = make_outputs(3);
        let b1 = BatchTransaction::new(
            "alice".to_string(), outputs.clone(), 1, "pk".to_string(), vec![],
        ).unwrap();
        let b2 = BatchTransaction::new(
            "alice".to_string(), outputs, 1, "pk".to_string(), vec![],
        ).unwrap();
        assert_eq!(b1.batch_id.len(), 64); 
        assert!(!b1.batch_id.is_empty());
    }

    #[test]
    fn test_batch_receivers() {
        let outputs = make_outputs(3);
        let batch = BatchTransaction::new(
            "alice".to_string(), outputs, 1, "pk".to_string(), vec![],
        ).unwrap();
        let receivers = batch.receivers();
        assert_eq!(receivers.len(), 3);
        assert!(receivers.contains(&"addr_0"));
        assert!(receivers.contains(&"addr_2"));
    }

    #[test]
    fn test_validate_structure_ok() {
        let outputs = make_outputs(2);
        let batch = BatchTransaction::new(
            "alice".to_string(), outputs, 1, "pk".to_string(), vec![],
        ).unwrap();
        assert!(batch.validate_structure().is_ok());
    }

    #[test]
    fn test_validate_structure_empty_receiver() {
        let outputs = vec![
            make_output("", 100),
            make_output("bob", 100),
        ];
        let batch = BatchTransaction::new(
            "alice".to_string(), outputs, 1, "pk".to_string(), vec![],
        ).unwrap();
        assert!(batch.validate_structure().is_err());
    }

    #[test]
    fn test_validate_structure_zero_amount_transparent() {
        let outputs = vec![
            make_output("bob", 0),
            make_output("carol", 100),
        ];
        let batch = BatchTransaction::new(
            "alice".to_string(), outputs, 1, "pk".to_string(), vec![],
        ).unwrap();
        assert!(batch.validate_structure().is_err());
    }

    #[test]
    fn test_all_private_outputs() {
        let outputs = vec![
            BatchOutput::private("s1".to_string(), "c1".to_string(), "e1".to_string()),
            BatchOutput::private("s2".to_string(), "c2".to_string(), "e2".to_string()),
        ];
        let batch = BatchTransaction::new(
            "alice".to_string(), outputs, 1, "pk".to_string(), vec![],
        ).unwrap();
        assert!(batch.is_private());
        assert_eq!(batch.total_amount, 0);
    }

    #[test]
    fn test_accumulator_push_no_flush() {
        let mut acc = BatchAccumulator::new(60);
        let result = acc.push("alice", make_output("bob", 100));
        assert!(result.is_none());
        assert_eq!(acc.pending_count("alice"), 1);
    }

    #[test]
    fn test_accumulator_flush_at_max() {
        let mut acc = BatchAccumulator::new(60);
        let mut result = None;
        for i in 0..MAX_BATCH_SIZE {
            let output = make_output(&format!("addr_{}", i), 100);
            result = acc.push("alice", output);
        }
        assert!(result.is_some());
        let flushed = result.unwrap();
        assert_eq!(flushed.len(), MAX_BATCH_SIZE);
        assert_eq!(acc.pending_count("alice"), 0);
    }

    #[test]
    fn test_accumulator_manual_flush() {
        let mut acc = BatchAccumulator::new(60);
        acc.push("alice", make_output("bob", 100));
        acc.push("alice", make_output("carol", 200));
        let flushed = acc.flush("alice");
        assert_eq!(flushed.len(), 2);
        assert_eq!(acc.pending_count("alice"), 0);
    }

    #[test]
    fn test_accumulator_different_senders_independent() {
        let mut acc = BatchAccumulator::new(60);
        acc.push("alice", make_output("bob", 100));
        acc.push("carol", make_output("dave", 200));
        assert_eq!(acc.pending_count("alice"), 1);
        assert_eq!(acc.pending_count("carol"), 1);
        assert_eq!(acc.total_pending(), 2);
    }

    #[test]
    fn test_accumulator_flush_timed_out() {
        let mut acc = BatchAccumulator::new(0); 
        acc.push("alice", make_output("bob", 100));
        acc.push("alice", make_output("carol", 200));
        std::thread::sleep(std::time::Duration::from_millis(10));
        let timed_out = acc.flush_timed_out();
        assert!(timed_out.contains_key("alice"));
        assert_eq!(timed_out["alice"].len(), 2);
        assert_eq!(acc.total_pending(), 0);
    }

    #[test]
    fn test_accumulator_flush_empty_sender() {
        let mut acc = BatchAccumulator::new(60);
        let flushed = acc.flush("nobody");
        assert!(flushed.is_empty());
    }
}