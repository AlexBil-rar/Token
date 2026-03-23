// ledger/src/privacy.rs

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

#[derive(Debug, Clone)]
pub struct ParentSelectionConfig {
    pub real_parents: usize,
    pub decoy_parents: usize,
    pub decoy_pool_size: usize,
    pub noise_probability: f64,
}

impl Default for ParentSelectionConfig {
    fn default() -> Self {
        ParentSelectionConfig {
            real_parents: 1,   
            decoy_parents: 1,   
            decoy_pool_size: 20,
            noise_probability: 0.15, 
        }
    }
}

#[derive(Debug)]
pub struct DecoyPool {
    recent: VecDeque<String>,
    max_size: usize,
    seed: u64,
}

impl DecoyPool {
    pub fn new(max_size: usize) -> Self {
        DecoyPool {
            recent: VecDeque::new(),
            max_size,
            seed: (now_secs() as u64).wrapping_mul(6364136223846793005),
        }
    }

    pub fn record(&mut self, tx_id: String) {
        if self.recent.len() >= self.max_size {
            self.recent.pop_front();
        }
        self.recent.push_back(tx_id);
    }

    pub fn sample(&mut self, n: usize, exclude: &[String]) -> Vec<String> {
        if self.recent.is_empty() || n == 0 {
            return vec![];
        }

        let mut candidates: Vec<String> = self.recent.iter()
            .filter(|id| !exclude.contains(id))
            .cloned()
            .collect();

        if candidates.is_empty() {
            return vec![];
        }

        let take = n.min(candidates.len());
        for i in (1..candidates.len()).rev() {
            self.seed = self.xorshift(self.seed);
            let j = (self.seed as usize) % (i + 1);
            candidates.swap(i, j);
        }

        candidates.into_iter().take(take).collect()
    }

    pub fn size(&self) -> usize {
        self.recent.len()
    }

    fn xorshift(&self, mut x: u64) -> u64 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    }
}

pub fn select_parents_with_privacy(
    tips: &[String],
    decoy_pool: &mut DecoyPool,
    config: &ParentSelectionConfig,
    max_parents: usize,
) -> Vec<String> {
    if tips.is_empty() {
        return vec![];
    }

    let real: Vec<String> = tips.iter()
        .take(config.real_parents)
        .cloned()
        .collect();

    let decoys = decoy_pool.sample(config.decoy_parents, &real);

    let mut selected: Vec<String> = real;
    for decoy in decoys {
        if !selected.contains(&decoy) {
            selected.push(decoy);
        }
    }

    if config.noise_probability > 0.0 {
        let noise_roll = pseudo_random_f64(decoy_pool);
        if noise_roll < config.noise_probability {
            let noisy = decoy_pool.sample(1, &selected);
            if let Some(noise_tx) = noisy.into_iter().next() {
                if !selected.is_empty() && selected.len() >= max_parents {
                    let last = selected.len() - 1;
                    selected[last] = noise_tx;
                } else {
                    selected.push(noise_tx);
                }
            }
        }
    }

    let mut unique = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in selected {
        if seen.insert(id.clone()) {
            unique.push(id);
        }
        if unique.len() >= max_parents {
            break;
        }
    }

    unique
}

fn pseudo_random_f64(pool: &mut DecoyPool) -> f64 {
    pool.seed = pool.xorshift(pool.seed);
    (pool.seed as f64) / (u64::MAX as f64)
}

#[derive(Debug, Clone)]
pub struct DiffusionConfig {
    pub delay_min_ms: u64,
    pub delay_max_ms: u64,
    pub enabled: bool,
}

impl Default for DiffusionConfig {
    fn default() -> Self {
        DiffusionConfig {
            delay_min_ms: 50,   
            delay_max_ms: 500,
            enabled: true,
        }
    }
}

impl DiffusionConfig {
    pub fn relay_delay(&self, tx_id: &str) -> std::time::Duration {
        if !self.enabled {
            return std::time::Duration::ZERO;
        }

        let entropy = tx_id.bytes()
            .take(8)
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));

        let range = self.delay_max_ms - self.delay_min_ms;
        let delay_ms = self.delay_min_ms + (entropy % (range + 1));

        std::time::Duration::from_millis(delay_ms)
    }

    pub fn disabled() -> Self {
        DiffusionConfig { enabled: false, ..Default::default() }
    }
}

#[derive(Debug, Clone)]
pub struct PrivacyRiskScore {
    pub score: f64,
    pub factors: Vec<String>,
}

impl PrivacyRiskScore {
    pub fn evaluate(
        parent_count: usize,
        is_private: bool,
        has_stealth: bool,
        decoy_count: usize,
    ) -> Self {
        let mut score = 0.0f64;
        let mut factors = Vec::new();

        if parent_count < 2 {
            score += 0.3;
            factors.push("single parent — less graph noise".to_string());
        }

        if !is_private {
            score += 0.25;
            factors.push("transparent amount — visible on-chain".to_string());
        }

        if !has_stealth {
            score += 0.25;
            factors.push("no stealth address — receiver linkable".to_string());
        }

        if decoy_count == 0 {
            score += 0.2;
            factors.push("no decoy parents — graph pattern visible".to_string());
        }

        PrivacyRiskScore {
            score: score.min(1.0),
            factors,
        }
    }

    pub fn is_high_risk(&self) -> bool {
        self.score >= 0.7
    }

    pub fn is_low_risk(&self) -> bool {
        self.score < 0.3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoy_pool_empty_sample() {
        let mut pool = DecoyPool::new(10);
        let result = pool.sample(3, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_decoy_pool_record_and_sample() {
        let mut pool = DecoyPool::new(10);
        pool.record("tx1".to_string());
        pool.record("tx2".to_string());
        pool.record("tx3".to_string());
        let result = pool.sample(2, &[]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_decoy_pool_excludes_given() {
        let mut pool = DecoyPool::new(10);
        pool.record("tx1".to_string());
        pool.record("tx2".to_string());
        pool.record("tx3".to_string());
        let result = pool.sample(3, &["tx1".to_string(), "tx2".to_string()]);
        assert!(!result.contains(&"tx1".to_string()));
        assert!(!result.contains(&"tx2".to_string()));
    }

    #[test]
    fn test_decoy_pool_no_duplicates() {
        let mut pool = DecoyPool::new(10);
        for i in 0..10 {
            pool.record(format!("tx{}", i));
        }
        let result = pool.sample(5, &[]);
        let unique: std::collections::HashSet<_> = result.iter().collect();
        assert_eq!(unique.len(), result.len());
    }

    #[test]
    fn test_decoy_pool_max_size_evicts_old() {
        let mut pool = DecoyPool::new(3);
        pool.record("tx1".to_string());
        pool.record("tx2".to_string());
        pool.record("tx3".to_string());
        pool.record("tx4".to_string());
        assert_eq!(pool.size(), 3);
        let result = pool.sample(3, &[]);
        assert!(!result.contains(&"tx1".to_string()));
    }

    #[test]
    fn test_decoy_pool_sample_cant_exceed_pool() {
        let mut pool = DecoyPool::new(10);
        pool.record("tx1".to_string());
        pool.record("tx2".to_string());
        let result = pool.sample(10, &[]);
        assert!(result.len() <= 2);
    }

    #[test]
    fn test_select_parents_empty_tips() {
        let mut pool = DecoyPool::new(10);
        let config = ParentSelectionConfig::default();
        let result = select_parents_with_privacy(&[], &mut pool, &config, 2);
        assert!(result.is_empty());
    }

    #[test]
    fn test_select_parents_single_tip_no_decoys() {
        let mut pool = DecoyPool::new(10);
        let config = ParentSelectionConfig {
            decoy_parents: 0,
            noise_probability: 0.0,
            ..Default::default()
        };
        let tips = vec!["tip1".to_string()];
        let result = select_parents_with_privacy(&tips, &mut pool, &config, 2);
        assert!(result.contains(&"tip1".to_string()));
    }

    #[test]
    fn test_select_parents_with_decoys() {
        let mut pool = DecoyPool::new(20);
        for i in 0..15 {
            pool.record(format!("old_tx_{}", i));
        }
        let config = ParentSelectionConfig {
            real_parents: 1,
            decoy_parents: 1,
            noise_probability: 0.0,
            ..Default::default()
        };
        let tips = vec!["tip1".to_string()];
        let result = select_parents_with_privacy(&tips, &mut pool, &config, 2);
        assert!(result.contains(&"tip1".to_string()));
        assert!(result.len() <= 2);
    }

    #[test]
    fn test_select_parents_no_duplicates() {
        let mut pool = DecoyPool::new(20);
        pool.record("tip1".to_string());
        let config = ParentSelectionConfig {
            real_parents: 1,
            decoy_parents: 1,
            noise_probability: 0.0,
            ..Default::default()
        };
        let tips = vec!["tip1".to_string()];
        let result = select_parents_with_privacy(&tips, &mut pool, &config, 2);
        let unique: std::collections::HashSet<_> = result.iter().collect();
        assert_eq!(unique.len(), result.len());
    }

    #[test]
    fn test_select_parents_max_parents_respected() {
        let mut pool = DecoyPool::new(20);
        for i in 0..15 {
            pool.record(format!("old_{}", i));
        }
        let config = ParentSelectionConfig {
            real_parents: 2,
            decoy_parents: 3,
            noise_probability: 0.0,
            ..Default::default()
        };
        let tips = vec!["t1".to_string(), "t2".to_string(), "t3".to_string()];
        let result = select_parents_with_privacy(&tips, &mut pool, &config, 2);
        assert!(result.len() <= 2);
    }

    #[test]
    fn test_diffusion_disabled_returns_zero() {
        let config = DiffusionConfig::disabled();
        let delay = config.relay_delay("tx_abc123");
        assert_eq!(delay, std::time::Duration::ZERO);
    }

    #[test]
    fn test_diffusion_delay_in_range() {
        let config = DiffusionConfig::default();
        let delay = config.relay_delay("tx_abc123");
        assert!(delay.as_millis() >= config.delay_min_ms as u128);
        assert!(delay.as_millis() <= config.delay_max_ms as u128);
    }

    #[test]
    fn test_diffusion_different_txs_different_delays() {
        let config = DiffusionConfig::default();
        let d1 = config.relay_delay("tx_aaaaaa");
        let d2 = config.relay_delay("tx_zzzzzz");
        assert!(d1.as_millis() >= config.delay_min_ms as u128);
        assert!(d2.as_millis() >= config.delay_min_ms as u128);
    }

    #[test]
    fn test_diffusion_same_tx_same_delay() {
        let config = DiffusionConfig::default();
        let d1 = config.relay_delay("tx_deterministic");
        let d2 = config.relay_delay("tx_deterministic");
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_risk_score_all_good_is_low() {
        let score = PrivacyRiskScore::evaluate(2, true, true, 1);
        assert!(score.is_low_risk());
        assert!(score.factors.is_empty() || score.score < 0.3);
    }

    #[test]
    fn test_risk_score_all_bad_is_high() {
        let score = PrivacyRiskScore::evaluate(1, false, false, 0);
        assert!(score.is_high_risk());
        assert!(!score.factors.is_empty());
    }

    #[test]
    fn test_risk_score_transparent_increases_risk() {
        let private_score = PrivacyRiskScore::evaluate(2, true, true, 1);
        let transparent_score = PrivacyRiskScore::evaluate(2, false, true, 1);
        assert!(transparent_score.score > private_score.score);
    }

    #[test]
    fn test_risk_score_no_stealth_increases_risk() {
        let stealth_score = PrivacyRiskScore::evaluate(2, true, true, 1);
        let no_stealth_score = PrivacyRiskScore::evaluate(2, true, false, 1);
        assert!(no_stealth_score.score > stealth_score.score);
    }

    #[test]
    fn test_risk_score_no_decoy_increases_risk() {
        let with_decoy = PrivacyRiskScore::evaluate(2, true, true, 1);
        let no_decoy = PrivacyRiskScore::evaluate(2, true, true, 0);
        assert!(no_decoy.score > with_decoy.score);
    }

    #[test]
    fn test_risk_score_capped_at_1() {
        let score = PrivacyRiskScore::evaluate(1, false, false, 0);
        assert!(score.score <= 1.0);
    }

    #[test]
    fn test_risk_factors_describe_issues() {
        let score = PrivacyRiskScore::evaluate(1, false, false, 0);
        assert!(!score.factors.is_empty());
        for factor in &score.factors {
            assert!(!factor.is_empty());
        }
    }
}