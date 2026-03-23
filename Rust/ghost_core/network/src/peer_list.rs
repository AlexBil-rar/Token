// network/src/peer_list.rs

use std::collections::{HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_PEERS: usize = 128;

const GOSSIP_SAMPLE_SIZE: usize = 8;

const ECLIPSE_SUBNET_MAX_RATIO: f64 = 0.80;

const ECLIPSE_MIN_PEERS: usize = 10;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone)]
pub struct PeerEntry {
    pub address: String,
    pub added_at: u64,
    pub last_seen: u64,
    pub failures: u32,
}

impl PeerEntry {
    fn new(address: String) -> Self {
        let now = now_secs();
        PeerEntry { address, added_at: now, last_seen: now, failures: 0 }
    }
}

#[derive(Debug, PartialEq)]
pub enum EclipseCheck {
    Clean,
    Suspected {
        subnet: String,
        count: usize,
        total: usize,
    },
}

#[derive(Debug, Default, Clone)]
pub struct PeerList {
    peers: HashMap<String, PeerEntry>,
    sample_seed: u64,
}

impl PeerList {
    pub fn new() -> Self {
        PeerList {
            peers: HashMap::new(),
            sample_seed: now_secs(),
        }
    }

    pub fn add(&mut self, address: &str) -> bool {
        let address = address.trim_end_matches('/').to_string();

        if self.peers.contains_key(&address) {
            return true; 
        }

        if self.peers.len() >= MAX_PEERS {
            return false;
        }

        self.peers.insert(address.clone(), PeerEntry::new(address));
        true
    }

    pub fn remove(&mut self, address: &str) {
        self.peers.remove(address.trim_end_matches('/'));
    }

    pub fn has(&self, address: &str) -> bool {
        self.peers.contains_key(address.trim_end_matches('/'))
    }

    pub fn get_all(&self) -> Vec<String> {
        self.peers.keys().cloned().collect()
    }

    pub fn size(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    pub fn mark_seen(&mut self, address: &str) {
        if let Some(entry) = self.peers.get_mut(address.trim_end_matches('/')) {
            entry.last_seen = now_secs();
            entry.failures = 0;
        }
    }

    pub fn mark_failure(&mut self, address: &str) {
        if let Some(entry) = self.peers.get_mut(address.trim_end_matches('/')) {
            entry.failures += 1;
        }
    }

    pub fn evict_failed(&mut self, threshold: u32) {
        self.peers.retain(|_, e| e.failures < threshold);
    }

    pub fn random_sample(&mut self, n: usize) -> Vec<String> {
        let mut all: Vec<String> = self.peers.keys().cloned().collect();

        self.sample_seed ^= self.sample_seed << 13;
        self.sample_seed ^= self.sample_seed >> 7;
        self.sample_seed ^= self.sample_seed << 17;
        let mut seed = self.sample_seed;

        for i in (1..all.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let j = (seed as usize) % (i + 1);
            all.swap(i, j);
        }

        all.truncate(n);
        all
    }

    pub fn gossip_sample(&mut self) -> Vec<String> {
        self.random_sample(GOSSIP_SAMPLE_SIZE)
    }

    pub fn check_eclipse(&self) -> EclipseCheck {
        if self.peers.len() < ECLIPSE_MIN_PEERS {
            return EclipseCheck::Clean;
        }

        let mut subnet_counts: HashMap<String, usize> = HashMap::new();
        let mut countable = 0usize;

        for address in self.peers.keys() {
            if let Some(subnet) = extract_subnet(address) {
                *subnet_counts.entry(subnet).or_insert(0) += 1;
                countable += 1;
            }
        }

        if countable == 0 {
            return EclipseCheck::Clean;
        }

        for (subnet, count) in &subnet_counts {
            let ratio = *count as f64 / countable as f64;
            if ratio >= ECLIPSE_SUBNET_MAX_RATIO {
                return EclipseCheck::Suspected {
                    subnet: subnet.clone(),
                    count: *count,
                    total: countable,
                };
            }
        }

        EclipseCheck::Clean
    }

    pub fn freshest_peers(&self, n: usize) -> Vec<String> {
        let mut entries: Vec<&PeerEntry> = self.peers.values().collect();
        entries.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        entries.iter().take(n).map(|e| e.address.clone()).collect()
    }
}

fn extract_subnet(address: &str) -> Option<String> {
    let host = address
        .trim_start_matches("wss://")
        .trim_start_matches("ws://");

    let ip = if let Some(pos) = host.rfind(':') {
        &host[..pos]
    } else {
        host
    };

    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        Some(format!("{}.{}", parts[0], parts[1]))
    } else {
        None 
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashSet};


    #[test]
    fn test_add_and_has() {
        let mut peers = PeerList::new();
        peers.add("ws://1.2.3.4:9000");
        assert!(peers.has("ws://1.2.3.4:9000"));
    }

    #[test]
    fn test_remove() {
        let mut peers = PeerList::new();
        peers.add("ws://1.2.3.4:9000");
        peers.remove("ws://1.2.3.4:9000");
        assert!(!peers.has("ws://1.2.3.4:9000"));
    }

    #[test]
    fn test_no_duplicates() {
        let mut peers = PeerList::new();
        peers.add("ws://1.2.3.4:9000");
        peers.add("ws://1.2.3.4:9000");
        assert_eq!(peers.size(), 1);
    }

    #[test]
    fn test_trailing_slash_normalized() {
        let mut peers = PeerList::new();
        peers.add("ws://1.2.3.4:9000/");
        assert!(peers.has("ws://1.2.3.4:9000"));
    }

    #[test]
    fn test_get_all() {
        let mut peers = PeerList::new();
        peers.add("ws://1.2.3.4:9000");
        peers.add("ws://5.6.7.8:9000");
        assert_eq!(peers.size(), 2);
        let all = peers.get_all();
        assert!(all.contains(&"ws://1.2.3.4:9000".to_string()));
        assert!(all.contains(&"ws://5.6.7.8:9000".to_string()));
    }

    #[test]
    fn test_empty() {
        let peers = PeerList::new();
        assert!(peers.is_empty());
        assert_eq!(peers.size(), 0);
    }


    #[test]
    fn test_max_peers_limit() {
        let mut peers = PeerList::new();
        for i in 0..MAX_PEERS {
            let addr = format!("ws://10.0.{}.{}:9000", i / 256, i % 256);
            assert!(peers.add(&addr));
        }
        let result = peers.add("ws://99.99.99.99:9000");
        assert!(!result);
        assert_eq!(peers.size(), MAX_PEERS);
    }

    #[test]
    fn test_mark_seen_resets_failures() {
        let mut peers = PeerList::new();
        peers.add("ws://1.2.3.4:9000");
        peers.mark_failure("ws://1.2.3.4:9000");
        peers.mark_failure("ws://1.2.3.4:9000");
        peers.mark_seen("ws://1.2.3.4:9000");
        assert_eq!(peers.peers["ws://1.2.3.4:9000"].failures, 0);
    }

    #[test]
    fn test_evict_failed_removes_bad_peers() {
        let mut peers = PeerList::new();
        peers.add("ws://1.2.3.4:9000");
        peers.add("ws://5.6.7.8:9000");
        for _ in 0..5 {
            peers.mark_failure("ws://1.2.3.4:9000");
        }
        peers.evict_failed(5);
        assert!(!peers.has("ws://1.2.3.4:9000"));
        assert!(peers.has("ws://5.6.7.8:9000"));
    }

    #[test]
    fn test_random_sample_size() {
        let mut peers = PeerList::new();
        for i in 0..20 {
            peers.add(&format!("ws://10.0.0.{}:9000", i));
        }
        let sample = peers.random_sample(5);
        assert_eq!(sample.len(), 5);
    }

    #[test]
    fn test_random_sample_no_duplicates() {
        let mut peers = PeerList::new();
        for i in 0..20 {
            peers.add(&format!("ws://10.0.0.{}:9000", i));
        }
        let sample = peers.random_sample(10);
        let unique: HashSet<_> = sample.iter().collect();
        assert_eq!(unique.len(), sample.len());
    }

    #[test]
    fn test_random_sample_less_than_available() {
        let mut peers = PeerList::new();
        peers.add("ws://1.2.3.4:9000");
        peers.add("ws://5.6.7.8:9000");
        let sample = peers.random_sample(100);
        assert_eq!(sample.len(), 2);
    }

    #[test]
    fn test_eclipse_clean_diverse_peers() {
        let mut peers = PeerList::new();
        for i in 0..15 {
            peers.add(&format!("ws://{}.0.0.1:9000", i + 1));
        }
        assert_eq!(peers.check_eclipse(), EclipseCheck::Clean);
    }

    #[test]
    fn test_eclipse_detected_same_subnet() {
        let mut peers = PeerList::new();
        for i in 0..10 {
            peers.add(&format!("ws://10.0.0.{}:9000", i + 1));
        }
        let result = peers.check_eclipse();
        assert!(matches!(result, EclipseCheck::Suspected { .. }));
        if let EclipseCheck::Suspected { subnet, count, total } = result {
            assert_eq!(subnet, "10.0");
            assert_eq!(count, 10);
            assert_eq!(total, 10);
        }
    }

    #[test]
    fn test_eclipse_below_min_peers_is_clean() {
        let mut peers = PeerList::new();
        for i in 0..5 {
            peers.add(&format!("ws://10.0.0.{}:9000", i));
        }
        assert_eq!(peers.check_eclipse(), EclipseCheck::Clean);
    }

    #[test]
    fn test_extract_subnet_valid_ipv4() {
        assert_eq!(extract_subnet("ws://192.168.1.100:9000"), Some("192.168".to_string()));
        assert_eq!(extract_subnet("ws://10.0.0.1:9000"), Some("10.0".to_string()));
    }

    #[test]
    fn test_extract_subnet_hostname_returns_none() {
        assert_eq!(extract_subnet("ws://example.com:9000"), None);
    }

    #[test]
    fn test_freshest_peers_ordering() {
        let mut peers = PeerList::new();
        peers.add("ws://1.0.0.1:9000");
        peers.add("ws://1.0.0.2:9000");
        // Напрямую выставляем last_seen чтобы не зависеть от sleep
        peers.peers.get_mut("ws://1.0.0.1:9000").unwrap().last_seen = 1000;
        peers.peers.get_mut("ws://1.0.0.2:9000").unwrap().last_seen = 2000;
        let fresh = peers.freshest_peers(1);
        assert_eq!(fresh[0], "ws://1.0.0.2:9000");
    }
}