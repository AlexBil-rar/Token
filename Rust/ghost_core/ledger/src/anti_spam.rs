// ledger/src/anti_spam.rs

use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MIN_DIFFICULTY: usize = 2;
pub const MAX_DIFFICULTY: usize = 6;

const TPS_SCALE_UP: f64 = 10.0;
const TPS_SCALE_DOWN: f64 = 2.0;

const WINDOW_SECS: f64 = 60.0;

const RATE_LIMIT_WINDOW_SECS: f64 = 10.0; 
const RATE_LIMIT_MAX_TX: usize = 5;        
const RATE_LIMIT_BURST_MAX_TX: usize = 10; 

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TxPriority {
    Low = 0,
    Normal = 1,
    High = 2,
}

#[derive(Debug, PartialEq)]
pub enum RateLimitResult {
    Allowed(TxPriority),
    Rejected { reason: String },
}

#[derive(Debug)]
struct AddressWindow {
    timestamps: VecDeque<f64>,
}

impl AddressWindow {
    fn new() -> Self {
        AddressWindow { timestamps: VecDeque::new() }
    }

    fn record(&mut self, now: f64) {
        self.timestamps.push_back(now);
        self.evict(now);
    }

    fn evict(&mut self, now: f64) {
        let cutoff = now - RATE_LIMIT_WINDOW_SECS;
        while let Some(&front) = self.timestamps.front() {
            if front < cutoff {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
    }

    fn count(&self) -> usize {
        self.timestamps.len()
    }

    fn is_empty_or_old(&self, now: f64) -> bool {
        match self.timestamps.front() {
            None => true,
            Some(&t) => now - t > RATE_LIMIT_WINDOW_SECS * 2.0,
        }
    }
}

#[derive(Debug)]
pub struct AntiSpamController {
    pub difficulty: usize,
    tx_timestamps: VecDeque<f64>,
    last_adjusted: f64,
    address_windows: HashMap<String, AddressWindow>,
    gc_counter: u64,
}

impl AntiSpamController {
    pub fn new() -> Self {
        AntiSpamController {
            difficulty: MIN_DIFFICULTY,
            tx_timestamps: VecDeque::new(),
            last_adjusted: now_secs(),
            address_windows: HashMap::new(),
            gc_counter: 0,
        }
    }

    pub fn record_transaction(&mut self) {
        let now = now_secs();
        self.tx_timestamps.push_back(now);
        self.evict_old(now);
        self.maybe_adjust(now);
    }

    pub fn check_and_record_address(&mut self, sender: &str) -> RateLimitResult {
        let now = now_secs();

        let window = self.address_windows
            .entry(sender.to_string())
            .or_insert_with(AddressWindow::new);

        window.evict(now);
        let count = window.count();

        if count >= RATE_LIMIT_BURST_MAX_TX {
            return RateLimitResult::Rejected {
                reason: format!(
                    "burst limit exceeded: {} tx in last {}s (max {})",
                    count, RATE_LIMIT_WINDOW_SECS, RATE_LIMIT_BURST_MAX_TX
                ),
            };
        }

        if count >= RATE_LIMIT_MAX_TX {
            return RateLimitResult::Rejected {
                reason: format!(
                    "rate limit exceeded: {} tx in last {}s (max {})",
                    count, RATE_LIMIT_WINDOW_SECS, RATE_LIMIT_MAX_TX
                ),
            };
        }

        window.record(now);
        let new_count = window.count();

        self.gc_counter += 1;
        if self.gc_counter % 500 == 0 {
            self.gc_inactive(now);
        }

        let priority = if new_count <= 1 {
            TxPriority::High
        } else {
            TxPriority::Normal
        };

        RateLimitResult::Allowed(priority)
    }

    pub fn current_difficulty(&self) -> usize {
        self.difficulty
    }

    pub fn current_tps(&self) -> f64 {
        let count = self.tx_timestamps.len() as f64;
        if count == 0.0 { return 0.0; }
        count / WINDOW_SECS
    }

    pub fn address_tx_count(&self, sender: &str) -> usize {
        self.address_windows
            .get(sender)
            .map(|w| w.count())
            .unwrap_or(0)
    }

    pub fn set_difficulty(&mut self, d: usize) {
        self.difficulty = d.clamp(MIN_DIFFICULTY, MAX_DIFFICULTY);
    }

    fn evict_old(&mut self, now: f64) {
        let cutoff = now - WINDOW_SECS;
        while let Some(&front) = self.tx_timestamps.front() {
            if front < cutoff {
                self.tx_timestamps.pop_front();
            } else {
                break;
            }
        }
    }

    fn maybe_adjust(&mut self, now: f64) {
        if now - self.last_adjusted < 10.0 {
            return;
        }

        let tps = self.current_tps();

        if tps > TPS_SCALE_UP && self.difficulty < MAX_DIFFICULTY {
            self.difficulty += 1;
            self.last_adjusted = now;
        } else if tps < TPS_SCALE_DOWN && self.difficulty > MIN_DIFFICULTY {
            self.difficulty -= 1;
            self.last_adjusted = now;
        }
    }

    fn gc_inactive(&mut self, now: f64) {
        self.address_windows.retain(|_, w| !w.is_empty_or_old(now));
    }
}

impl Default for AntiSpamController {
    fn default() -> Self {
        AntiSpamController::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_difficulty_is_min() {
        let ctrl = AntiSpamController::new();
        assert_eq!(ctrl.difficulty, MIN_DIFFICULTY);
    }

    #[test]
    fn test_record_increases_tps() {
        let mut ctrl = AntiSpamController::new();
        for _ in 0..10 {
            ctrl.tx_timestamps.push_back(now_secs());
        }
        ctrl.evict_old(now_secs());
        assert!(ctrl.current_tps() > 0.0);
    }

    #[test]
    fn test_set_difficulty_clamps_to_min() {
        let mut ctrl = AntiSpamController::new();
        ctrl.set_difficulty(0);
        assert_eq!(ctrl.difficulty, MIN_DIFFICULTY);
    }

    #[test]
    fn test_set_difficulty_clamps_to_max() {
        let mut ctrl = AntiSpamController::new();
        ctrl.set_difficulty(100);
        assert_eq!(ctrl.difficulty, MAX_DIFFICULTY);
    }

    #[test]
    fn test_set_difficulty_valid() {
        let mut ctrl = AntiSpamController::new();
        ctrl.set_difficulty(4);
        assert_eq!(ctrl.difficulty, 4);
    }

    #[test]
    fn test_high_tps_raises_difficulty() {
        let mut ctrl = AntiSpamController::new();
        let now = now_secs();
        for i in 0..900 {
            ctrl.tx_timestamps.push_back(now - 59.0 + (i as f64 / 15.0));
        }
        ctrl.last_adjusted = now - 20.0;
        ctrl.maybe_adjust(now);
        assert!(ctrl.difficulty > MIN_DIFFICULTY);
    }

    #[test]
    fn test_low_tps_lowers_difficulty() {
        let mut ctrl = AntiSpamController::new();
        ctrl.difficulty = MAX_DIFFICULTY;
        ctrl.last_adjusted = now_secs() - 20.0;
        ctrl.maybe_adjust(now_secs());
        assert!(ctrl.difficulty < MAX_DIFFICULTY);
    }

    #[test]
    fn test_tps_zero_when_no_txs() {
        let ctrl = AntiSpamController::new();
        assert_eq!(ctrl.current_tps(), 0.0);
    }

    #[test]
    fn test_old_txs_evicted() {
        let mut ctrl = AntiSpamController::new();
        let old_time = now_secs() - WINDOW_SECS - 10.0;
        ctrl.tx_timestamps.push_back(old_time);
        ctrl.evict_old(now_secs());
        assert!(ctrl.tx_timestamps.is_empty());
    }

    #[test]
    fn test_recent_txs_not_evicted() {
        let mut ctrl = AntiSpamController::new();
        let recent = now_secs() - 5.0;
        ctrl.tx_timestamps.push_back(recent);
        ctrl.evict_old(now_secs());
        assert_eq!(ctrl.tx_timestamps.len(), 1);
    }

    #[test]
    fn test_difficulty_stays_in_bounds() {
        let mut ctrl = AntiSpamController::new();
        ctrl.difficulty = MIN_DIFFICULTY;
        ctrl.last_adjusted = now_secs() - 20.0;
        ctrl.maybe_adjust(now_secs());
        assert_eq!(ctrl.difficulty, MIN_DIFFICULTY);
    }

    #[test]
    fn test_first_tx_from_address_is_high_priority() {
        let mut ctrl = AntiSpamController::new();
        let result = ctrl.check_and_record_address("alice");
        assert_eq!(result, RateLimitResult::Allowed(TxPriority::High));
    }

    #[test]
    fn test_second_tx_from_address_is_normal_priority() {
        let mut ctrl = AntiSpamController::new();
        ctrl.check_and_record_address("alice");
        let result = ctrl.check_and_record_address("alice");
        assert_eq!(result, RateLimitResult::Allowed(TxPriority::Normal));
    }

    #[test]
    fn test_exceeding_soft_limit_is_rejected() {
        let mut ctrl = AntiSpamController::new();
        for _ in 0..RATE_LIMIT_MAX_TX {
            ctrl.check_and_record_address("spammer");
        }
        let result = ctrl.check_and_record_address("spammer");
        assert!(matches!(result, RateLimitResult::Rejected { .. }));
    }

    #[test]
    fn test_burst_limit_is_rejected() {
        let mut ctrl = AntiSpamController::new();
        let window = ctrl.address_windows
            .entry("attacker".to_string())
            .or_insert_with(AddressWindow::new);
        for _ in 0..RATE_LIMIT_BURST_MAX_TX {
            window.timestamps.push_back(now_secs());
        }
        let result = ctrl.check_and_record_address("attacker");
        assert!(matches!(result, RateLimitResult::Rejected { .. }));
    }

    #[test]
    fn test_different_addresses_are_independent() {
        let mut ctrl = AntiSpamController::new();
        for _ in 0..RATE_LIMIT_MAX_TX {
            ctrl.check_and_record_address("alice");
        }
        let result = ctrl.check_and_record_address("bob");
        assert_eq!(result, RateLimitResult::Allowed(TxPriority::High));
    }

    #[test]
    fn test_address_tx_count_accurate() {
        let mut ctrl = AntiSpamController::new();
        ctrl.check_and_record_address("carol");
        ctrl.check_and_record_address("carol");
        assert_eq!(ctrl.address_tx_count("carol"), 2);
    }

    #[test]
    fn test_unknown_address_count_is_zero() {
        let ctrl = AntiSpamController::new();
        assert_eq!(ctrl.address_tx_count("nobody"), 0);
    }

    #[test]
    fn test_priority_ordering() {
        assert!(TxPriority::High > TxPriority::Normal);
        assert!(TxPriority::Normal > TxPriority::Low);
    }
}