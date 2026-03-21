// network/src/peer_list.rs

use std::collections::HashSet;

#[derive(Debug, Default, Clone)]
pub struct PeerList {
    peers: HashSet<String>,
}

impl PeerList {
    pub fn new() -> Self {
        PeerList::default()
    }

    pub fn add(&mut self, address: &str) {
        self.peers.insert(address.trim_end_matches('/').to_string());
    }

    pub fn remove(&mut self, address: &str) {
        self.peers.remove(address.trim_end_matches('/'));
    }

    pub fn has(&self, address: &str) -> bool {
        self.peers.contains(address.trim_end_matches('/'))
    }

    pub fn get_all(&self) -> Vec<String> {
        self.peers.iter().cloned().collect()
    }

    pub fn size(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}