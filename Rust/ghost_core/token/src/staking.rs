// token/src/staking.rs

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MIN_STAKE: u64 = 1_000;
const SLASH_PERCENT: f64 = 0.10;
const SLASH_BURN_RATIO: f64 = 0.50;
const MAX_VIOLATIONS: usize = 3;

pub const MIN_VALIDATOR_STAKE: u64 = MIN_STAKE;

pub const MIN_REWARD_STAKE: u64 = MIN_STAKE / 2;

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[derive(Debug, Clone, PartialEq)]
pub enum StakeStatus {
    Active,
    Slashed,
    Ejected,
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ViolationType {
    DoubleVote,
    ConflictingTx,
    ReputationPenalty,
    InvalidState,
}

impl ViolationType {
    pub fn as_str(&self) -> &str {
        match self {
            ViolationType::DoubleVote       => "double_vote",
            ViolationType::ConflictingTx    => "conflicting_tx",
            ViolationType::ReputationPenalty => "reputation_penalty",
            ViolationType::InvalidState     => "invalid_state",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StakeRecord {
    pub address: String,
    pub amount: u64,
    pub original_amount: u64,
    pub staked_at: f64,
    pub status: StakeStatus,
    pub violations: Vec<String>,
    pub total_slashed: u64,
}

impl StakeRecord {
    pub fn is_active(&self) -> bool {
        self.status == StakeStatus::Active
    }

    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }

    pub fn stake_ratio(&self) -> f64 {
        if self.original_amount == 0 { return 0.0; }
        self.amount as f64 / self.original_amount as f64
    }

    pub fn is_validator(&self) -> bool {
        self.is_active() && self.amount >= MIN_VALIDATOR_STAKE
    }

    pub fn is_reward_eligible(&self) -> bool {
        self.is_active() && self.amount >= MIN_REWARD_STAKE
    }
}

#[derive(Debug)]
pub struct SlashResult {
    pub slashed_amount: u64,
    pub burned: u64,
    pub to_pool: u64,
    pub ejected: bool,
    pub reason: String,
}

#[derive(Debug, PartialEq)]
pub enum EligibilityStatus {
    Validator,
    RewardOnly,
    RelayOnly,
    Ejected,
}

#[derive(Debug, Default)]
pub struct StakingManager {
    pub stakes: HashMap<String, StakeRecord>,
    pub slash_pool: u64,
    pub total_burned: u64,
}

impl StakingManager {
    pub fn new() -> Self {
        StakingManager::default()
    }

    pub fn stake(
        &mut self,
        address: &str,
        amount: u64,
        balances: &mut HashMap<String, u64>,
    ) -> Result<(), String> {
        if amount < MIN_STAKE {
            return Err(format!("minimum stake is {} GHOST", MIN_STAKE));
        }

        if let Some(r) = self.stakes.get(address) {
            if r.is_active() {
                return Err("already staking".to_string());
            }
        }

        let balance = *balances.get(address).unwrap_or(&0);
        if balance < amount {
            return Err("insufficient balance".to_string());
        }

        *balances.entry(address.to_string()).or_insert(0) -= amount;

        self.stakes.insert(address.to_string(), StakeRecord {
            address: address.to_string(),
            amount,
            original_amount: amount,
            staked_at: now_secs(),
            status: StakeStatus::Active,
            violations: vec![],
            total_slashed: 0,
        });

        Ok(())
    }

    pub fn slash(
        &mut self,
        address: &str,
        violation: ViolationType,
        evidence: &str,
    ) -> Option<SlashResult> {
        let record = self.stakes.get_mut(address)?;

        if matches!(record.status, StakeStatus::Ejected | StakeStatus::Withdrawn) {
            return None;
        }

        record.violations.push(format!("{}:{}", violation.as_str(), evidence));
        record.status = StakeStatus::Slashed;

        let slash_amount = ((record.amount as f64 * SLASH_PERCENT) as u64).min(record.amount);
        let burned = (slash_amount as f64 * SLASH_BURN_RATIO) as u64;
        let to_pool = slash_amount - burned;

        record.amount -= slash_amount;
        record.total_slashed += slash_amount;
        self.total_burned += burned;
        self.slash_pool += to_pool;

        let mut ejected = false;

        if record.violation_count() >= MAX_VIOLATIONS {
            let remaining = record.amount;
            let burned_remaining = (remaining as f64 * SLASH_BURN_RATIO) as u64;
            let pool_remaining = remaining - burned_remaining;
            self.total_burned += burned_remaining;
            self.slash_pool += pool_remaining;
            record.amount = 0;
            record.status = StakeStatus::Ejected;
            ejected = true;
        } else if record.amount >= MIN_STAKE {
            record.status = StakeStatus::Active;
        }

        Some(SlashResult { slashed_amount: slash_amount, burned, to_pool, ejected, reason: violation.as_str().to_string() })
    }

    pub fn withdraw(
        &mut self,
        address: &str,
        balances: &mut HashMap<String, u64>,
    ) -> Result<u64, String> {
        let record = self.stakes.get_mut(address)
            .ok_or("not staking")?;

        if record.status == StakeStatus::Ejected {
            return Err("ejected nodes cannot withdraw".to_string());
        }
        if record.status == StakeStatus::Withdrawn {
            return Err("already withdrawn".to_string());
        }

        let amount = record.amount;
        *balances.entry(address.to_string()).or_insert(0) += amount;
        record.amount = 0;
        record.status = StakeStatus::Withdrawn;
        Ok(amount)
    }

    pub fn eligibility(&self, address: &str) -> EligibilityStatus {
        match self.stakes.get(address) {
            None => EligibilityStatus::RelayOnly,
            Some(r) => match r.status {
                StakeStatus::Ejected => EligibilityStatus::Ejected,
                StakeStatus::Withdrawn => EligibilityStatus::RelayOnly,
                _ if r.amount >= MIN_VALIDATOR_STAKE && r.is_active() => {
                    EligibilityStatus::Validator
                }
                _ if r.amount >= MIN_REWARD_STAKE && r.is_active() => {
                    EligibilityStatus::RewardOnly
                }
                _ => EligibilityStatus::RelayOnly,
            }
        }
    }

    pub fn is_eligible(&self, address: &str) -> bool {
        self.eligibility(address) == EligibilityStatus::Validator
    }

    pub fn is_reward_eligible(&self, address: &str) -> bool {
        matches!(
            self.eligibility(address),
            EligibilityStatus::Validator | EligibilityStatus::RewardOnly
        )
    }

    pub fn get_stake_amount(&self, address: &str) -> f64 {
        self.stakes.get(address)
            .filter(|r| r.is_active())
            .map(|r| r.amount as f64)
            .unwrap_or(0.0)
    }

    pub fn total_stake(&self) -> f64 {
        self.stakes.values()
            .filter(|r| r.is_active())
            .map(|r| r.amount as f64)
            .sum()
    }

    pub fn active_validators(&self) -> HashMap<String, f64> {
        self.stakes.values()
            .filter(|r| r.is_validator())
            .map(|r| (r.address.clone(), r.amount as f64))
            .collect()
    }

    pub fn distribute_slash_pool(
        &mut self,
        balances: &mut HashMap<String, u64>,
    ) -> u64 {
        if self.slash_pool == 0 { return 0; }

        let clean_nodes: Vec<String> = self.stakes.values()
            .filter(|r| r.is_active() && r.violation_count() == 0)
            .map(|r| r.address.clone())
            .collect();

        if clean_nodes.is_empty() { return 0; }

        let per_node = self.slash_pool / clean_nodes.len() as u64;
        if per_node == 0 { return 0; }

        let distributed = per_node * clean_nodes.len() as u64;
        for addr in &clean_nodes {
            *balances.entry(addr.clone()).or_insert(0) += per_node;
        }
        self.slash_pool -= distributed;
        distributed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_balances(address: &str, amount: u64) -> HashMap<String, u64> {
        let mut m = HashMap::new();
        m.insert(address.to_string(), amount);
        m
    }

    #[test]
    fn test_stake_success() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 5000);
        manager.stake("node1", MIN_STAKE, &mut balances).unwrap();
        assert_eq!(balances["node1"], 5000 - MIN_STAKE);
        assert_eq!(manager.stakes["node1"].amount, MIN_STAKE);
    }

    #[test]
    fn test_stake_below_minimum() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 5000);
        assert!(manager.stake("node1", MIN_STAKE - 1, &mut balances).is_err());
    }

    #[test]
    fn test_stake_insufficient_balance() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 500);
        assert!(manager.stake("node1", MIN_STAKE, &mut balances).is_err());
    }

    #[test]
    fn test_slash_reduces_stake() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 5000);
        manager.stake("node1", MIN_STAKE, &mut balances).unwrap();
        let result = manager.slash("node1", ViolationType::DoubleVote, "").unwrap();
        let expected = (MIN_STAKE as f64 * SLASH_PERCENT) as u64;
        assert_eq!(result.slashed_amount, expected);
    }

    #[test]
    fn test_slash_ejection_after_max_violations() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 10000);
        manager.stake("node1", MIN_STAKE, &mut balances).unwrap();
        let mut result = None;
        for _ in 0..MAX_VIOLATIONS {
            result = manager.slash("node1", ViolationType::DoubleVote, "");
        }
        assert!(result.unwrap().ejected);
        assert_eq!(manager.stakes["node1"].amount, 0);
    }

    #[test]
    fn test_withdraw_returns_stake() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 5000);
        manager.stake("node1", MIN_STAKE, &mut balances).unwrap();
        let amount = manager.withdraw("node1", &mut balances).unwrap();
        assert_eq!(amount, MIN_STAKE);
        assert_eq!(balances["node1"], 5000);
    }

    #[test]
    fn test_withdraw_ejected_fails() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 10000);
        manager.stake("node1", MIN_STAKE, &mut balances).unwrap();
        for _ in 0..MAX_VIOLATIONS {
            manager.slash("node1", ViolationType::DoubleVote, "");
        }
        assert!(manager.withdraw("node1", &mut balances).is_err());
    }

    #[test]
    fn test_distribute_slash_pool() {
        let mut manager = StakingManager::new();
        let mut balances = HashMap::new();
        balances.insert("node1".to_string(), 5000u64);
        balances.insert("node2".to_string(), 5000u64);
        balances.insert("bad".to_string(), 5000u64);
        manager.stake("node1", MIN_STAKE, &mut balances).unwrap();
        manager.stake("node2", MIN_STAKE, &mut balances).unwrap();
        manager.stake("bad", MIN_STAKE, &mut balances).unwrap();
        manager.slash("bad", ViolationType::DoubleVote, "");
        let b1_before = balances["node1"];
        let distributed = manager.distribute_slash_pool(&mut balances);
        assert!(distributed > 0);
        assert!(balances["node1"] > b1_before);
    }

    #[test]
    fn test_is_eligible_requires_min_stake() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 5000);
        assert!(!manager.is_eligible("node1"));
        manager.stake("node1", MIN_STAKE, &mut balances).unwrap();
        assert!(manager.is_eligible("node1"));
    }

    #[test]
    fn test_no_stake_is_relay_only() {
        let manager = StakingManager::new();
        assert_eq!(manager.eligibility("unknown"), EligibilityStatus::RelayOnly);
    }

    #[test]
    fn test_full_stake_is_validator() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 5000);
        manager.stake("node1", MIN_VALIDATOR_STAKE, &mut balances).unwrap();
        assert_eq!(manager.eligibility("node1"), EligibilityStatus::Validator);
    }

    #[test]
    fn test_ejected_node_is_ejected() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 10000);
        manager.stake("node1", MIN_STAKE, &mut balances).unwrap();
        for _ in 0..MAX_VIOLATIONS {
            manager.slash("node1", ViolationType::DoubleVote, "");
        }
        assert_eq!(manager.eligibility("node1"), EligibilityStatus::Ejected);
    }

    #[test]
    fn test_is_reward_eligible_with_validator_stake() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 5000);
        manager.stake("node1", MIN_STAKE, &mut balances).unwrap();
        assert!(manager.is_reward_eligible("node1"));
    }

    #[test]
    fn test_ejected_not_reward_eligible() {
        let mut manager = StakingManager::new();
        let mut balances = make_balances("node1", 10000);
        manager.stake("node1", MIN_STAKE, &mut balances).unwrap();
        for _ in 0..MAX_VIOLATIONS {
            manager.slash("node1", ViolationType::DoubleVote, "");
        }
        assert!(!manager.is_reward_eligible("node1"));
    }

    #[test]
    fn test_total_stake_sums_active() {
        let mut manager = StakingManager::new();
        let mut balances = HashMap::new();
        balances.insert("a".to_string(), 5000u64);
        balances.insert("b".to_string(), 5000u64);
        manager.stake("a", MIN_STAKE, &mut balances).unwrap();
        manager.stake("b", MIN_STAKE, &mut balances).unwrap();
        assert_eq!(manager.total_stake(), (MIN_STAKE * 2) as f64);
    }

    #[test]
    fn test_get_stake_amount_inactive_is_zero() {
        let manager = StakingManager::new();
        assert_eq!(manager.get_stake_amount("nobody"), 0.0);
    }

    #[test]
    fn test_active_validators_excludes_slashed() {
        let mut manager = StakingManager::new();
        let mut balances = HashMap::new();
        balances.insert("good".to_string(), 5000u64);
        balances.insert("bad".to_string(), 10000u64);
        manager.stake("good", MIN_STAKE, &mut balances).unwrap();
        manager.stake("bad", MIN_STAKE, &mut balances).unwrap();
        for _ in 0..MAX_VIOLATIONS {
            manager.slash("bad", ViolationType::DoubleVote, "");
        }
        let validators = manager.active_validators();
        assert!(validators.contains_key("good"));
        assert!(!validators.contains_key("bad"));
    }
}