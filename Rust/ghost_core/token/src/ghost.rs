// token/src/ghost.rs

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const TOTAL_SUPPLY: u64 = 21_000_000;
const GENESIS_SHARE: f64 = 0.10;
const ADDRESS_CAP: f64 = 0.001;
const BASE_REWARD_PER_HOUR: u64 = 10;
const HALVENING_INTERVAL: f64 = 4.0 * 365.0 * 24.0 * 3600.0;

const UPTIME_TIERS: &[(f64, f64)] = &[
    (24.0 * 3600.0,   1.00),
    (72.0 * 3600.0,   0.50),
    (168.0 * 3600.0,  0.25),
    (f64::INFINITY,   0.10),
];

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[derive(Debug, Clone)]
pub struct NodeUptime {
    pub address: String,
    pub first_seen: f64,
    pub last_seen: f64,
    pub continuous_since: f64,
    pub total_earned: u64,
}

impl NodeUptime {
    pub fn new(address: String, now: f64) -> Self {
        NodeUptime {
            address,
            first_seen: now,
            last_seen: now,
            continuous_since: now,
            total_earned: 0,
        }
    }

    pub fn ping(&mut self, now: f64) {
        if now - self.last_seen > 2.0 * 3600.0 {
            self.continuous_since = now;
        }
        self.last_seen = now;
    }

    pub fn continuous_uptime(&self, now: f64) -> f64 {
        now - self.continuous_since
    }
}

#[derive(Debug)]
pub struct GhostToken {
    pub network_start: f64,
    pub balances: HashMap<String, u64>,
    pub total_minted: u64,
    pub nodes: HashMap<String, NodeUptime>,
    pub address_cap: u64,
    pub genesis_supply: u64,
}

impl GhostToken {
    pub fn new() -> Self {
        let now = now_secs();
        GhostToken {
            network_start: now,
            balances: HashMap::new(),
            total_minted: 0,
            nodes: HashMap::new(),
            address_cap: (TOTAL_SUPPLY as f64 * ADDRESS_CAP) as u64,
            genesis_supply: (TOTAL_SUPPLY as f64 * GENESIS_SHARE) as u64,
        }
    }

    pub fn with_start(network_start: f64) -> Self {
        let mut g = GhostToken::new();
        g.network_start = network_start;
        g
    }

    pub fn genesis(&mut self, founder_address: &str) -> u64 {
        if self.total_minted > 0 { return 0; }
        let amount = self.genesis_supply.min(self.address_cap);
        *self.balances.entry(founder_address.to_string()).or_insert(0) += amount;
        self.total_minted += amount;
        amount
    }

    pub fn register_node(&mut self, address: &str, now: f64) {
        self.nodes.entry(address.to_string())
            .or_insert_with(|| NodeUptime::new(address.to_string(), now));
    }

    pub fn ping_node(&mut self, address: &str, now: f64) {
        if !self.nodes.contains_key(address) {
            self.register_node(address, now);
        }
        self.nodes.get_mut(address).unwrap().ping(now);
    }

    pub fn claim_reward(&mut self, address: &str, now: f64) -> u64 {
        if self.total_minted >= TOTAL_SUPPLY { return 0; }

        let node = match self.nodes.get_mut(address) {
            Some(n) => n,
            None => return 0,
        };

        let continuous = node.continuous_uptime(now);
        let multiplier = uptime_multiplier(continuous);
        let halvening = halvening_multiplier(self.network_start, now);
        let reward = (BASE_REWARD_PER_HOUR as f64 * multiplier * halvening) as u64;

        if reward == 0 { return 0; }

        let current = *self.balances.get(address).unwrap_or(&0);
        let available_cap = self.address_cap.saturating_sub(current);
        if available_cap == 0 { return 0; }

        let remaining = TOTAL_SUPPLY - self.total_minted;
        let actual = reward.min(available_cap).min(remaining);
        if actual == 0 { return 0; }

        *self.balances.entry(address.to_string()).or_insert(0) += actual;
        self.total_minted += actual;
        self.nodes.get_mut(address).unwrap().total_earned += actual;

        actual
    }

    pub fn get_balance(&self, address: &str) -> u64 {
        *self.balances.get(address).unwrap_or(&0)
    }
}

fn uptime_multiplier(continuous_seconds: f64) -> f64 {
    for &(threshold, multiplier) in UPTIME_TIERS {
        if continuous_seconds <= threshold {
            return multiplier;
        }
    }
    UPTIME_TIERS.last().unwrap().1
}

fn halvening_multiplier(network_start: f64, now: f64) -> f64 {
    let elapsed = now - network_start;
    let halvings = (elapsed / HALVENING_INTERVAL) as u32;
    1.0 / (2u32.pow(halvings) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_distributes() {
        let mut token = GhostToken::new();
        let amount = token.genesis("founder");
        assert!(amount > 0);
        assert_eq!(token.get_balance("founder"), amount);
    }

    #[test]
    fn test_genesis_only_once() {
        let mut token = GhostToken::new();
        token.genesis("founder");
        let second = token.genesis("founder2");
        assert_eq!(second, 0);
    }

    #[test]
    fn test_uptime_multiplier_first_day() {
        assert_eq!(uptime_multiplier(12.0 * 3600.0), 1.0);
    }

    #[test]
    fn test_uptime_multiplier_second_day() {
        assert_eq!(uptime_multiplier(48.0 * 3600.0), 0.5);
    }

    #[test]
    fn test_uptime_multiplier_week() {
        assert_eq!(uptime_multiplier(100.0 * 3600.0), 0.25);
    }

    #[test]
    fn test_claim_reward_fresh_node() {
        let now = now_secs();
        let mut token = GhostToken::with_start(now);
        token.register_node("node1", now);
        let reward = token.claim_reward("node1", now + 3600.0);
        assert!(reward > 0);
    }

    #[test]
    fn test_claim_reward_unknown_node_zero() {
        let mut token = GhostToken::new();
        assert_eq!(token.claim_reward("unknown", now_secs()), 0);
    }

    #[test]
    fn test_halvening_reduces_reward() {
        let now = now_secs();
        let four_years = 4.0 * 365.0 * 24.0 * 3600.0;

        let mut early = GhostToken::with_start(now);
        let mut late = GhostToken::with_start(now - four_years);

        early.register_node("node", now);
        late.register_node("node", now);

        let r_early = early.claim_reward("node", now + 3600.0);
        let r_late = late.claim_reward("node", now + 3600.0);

        assert!(r_early > r_late);
    }

    #[test]
    fn test_ping_resets_streak_after_gap() {
        let now = now_secs();
        let mut token = GhostToken::new();
        token.register_node("node1", now);
        token.ping_node("node1", now + 3.0 * 3600.0);
        assert_eq!(token.nodes["node1"].continuous_since, now + 3.0 * 3600.0);
    }
}