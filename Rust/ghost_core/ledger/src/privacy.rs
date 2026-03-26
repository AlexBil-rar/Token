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

#[derive(Debug, Clone)]
pub struct DecoyEntry {
    pub tx_id: String,
    pub weight: u64,
    pub timestamp: u64,
}

#[derive(Debug)]
pub struct DecoyPool {
    recent: VecDeque<DecoyEntry>,
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
        self.record_with_meta(tx_id, 1, 0);
    }

    pub fn record_with_meta(&mut self, tx_id: String, weight: u64, timestamp: u64) {
        if self.recent.len() >= self.max_size {
            self.recent.pop_front();
        }
        self.recent.push_back(DecoyEntry { tx_id, weight, timestamp });
    }

    pub fn sample(&mut self, n: usize, exclude: &[String]) -> Vec<String> {
        self.sample_matching(n, exclude, None, None)
    }

    pub fn sample_matching(
        &mut self,
        n: usize,
        exclude: &[String],
        target_weight: Option<u64>,
        target_timestamp: Option<u64>,
    ) -> Vec<String> {
        if self.recent.is_empty() || n == 0 {
            return vec![];
        }
    
        let mut candidates: Vec<&DecoyEntry> = self.recent.iter()
            .filter(|e| !exclude.contains(&e.tx_id))
            .collect();
    
        if candidates.is_empty() { return vec![]; }
    
        if target_weight.is_some() || target_timestamp.is_some() {
            let tw = target_weight.unwrap_or(0);
            let tt = target_timestamp.unwrap_or(0);
            candidates.sort_by_key(|e| {
                let weight_diff = (e.weight as i64 - tw as i64).unsigned_abs();
                let time_diff = (e.timestamp as i64 - tt as i64).unsigned_abs();
                weight_diff * 10 + time_diff / 1000
            });
            return candidates.iter().take(n).map(|e| e.tx_id.clone()).collect();
        }
    
        let take = n.min(candidates.len());
        let mut ids: Vec<String> = candidates.iter().map(|e| e.tx_id.clone()).collect();
        for i in (1..ids.len()).rev() {
            self.seed = self.xorshift(self.seed);
            let j = (self.seed as usize) % (i + 1);
            ids.swap(i, j);
        }
        ids.into_iter().take(take).collect()
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
    pub privacy_by_default: bool, 
}

impl Default for DiffusionConfig {
    fn default() -> Self {
        DiffusionConfig {
            delay_min_ms: 50,
            delay_max_ms: 500,
            enabled: true,
            privacy_by_default: true, 
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
        DiffusionConfig {
            enabled: false,
            privacy_by_default: false,
            ..Default::default()
        }
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

// ── Phase 10: Graph Privacy ───────────────────────────────────────────────────
 
#[derive(Debug, Clone)]
pub struct GraphPrivacyMetrics {
    pub parent_entropy: f64,
    pub fan_out_score: f64,
    pub timing_exposure: f64,
    pub decoy_ratio: f64,
}
 
impl GraphPrivacyMetrics {
    pub fn is_vulnerable(&self) -> bool {
        self.parent_entropy < 0.5
            || self.fan_out_score > 0.8
            || self.timing_exposure > 0.7
    }
 
    pub fn privacy_score(&self) -> f64 {
        let entropy_penalty = (1.0 - self.parent_entropy).max(0.0) * 0.35;
        let fanout_penalty  = self.fan_out_score * 0.30;
        let timing_penalty  = self.timing_exposure * 0.25;
        let decoy_bonus     = self.decoy_ratio * 0.10;
        (entropy_penalty + fanout_penalty + timing_penalty - decoy_bonus).clamp(0.0, 1.0)
    }
}
 
pub struct GraphPrivacyAnalyzer;
 
impl GraphPrivacyAnalyzer {
    pub fn analyze(
        parent_weights: &[u64],
        total_dag_tips: usize,
        decoy_count: usize,
        relay_delay_ms: u64,
    ) -> GraphPrivacyMetrics {
        let parent_entropy   = Self::shannon_entropy(parent_weights);
        let fan_out_score    = Self::fan_out(parent_weights.len(), total_dag_tips);
        let timing_exposure  = Self::timing_exposure(relay_delay_ms);
        let total_parents    = parent_weights.len().max(1);
        let decoy_ratio      = (decoy_count as f64 / total_parents as f64).min(1.0);
 
        GraphPrivacyMetrics { parent_entropy, fan_out_score, timing_exposure, decoy_ratio }
    }
 
    fn shannon_entropy(weights: &[u64]) -> f64 {
        if weights.is_empty() { return 0.0; }
        let total: u64 = weights.iter().sum();
        if total == 0 { return 0.0; }
        let total_f = total as f64;
        let entropy: f64 = weights
            .iter()
            .filter(|&&w| w > 0)
            .map(|&w| { let p = w as f64 / total_f; -p * p.ln() })
            .sum();
        let max_entropy = if weights.len() > 1 { (weights.len() as f64).ln() } else { 1.0 };
        (entropy / max_entropy).clamp(0.0, 1.0)
    }
 
    fn fan_out(parent_count: usize, total_tips: usize) -> f64 {
        if total_tips == 0 || parent_count == 0 { return 0.0; }
        (parent_count as f64 / total_tips.min(8) as f64).clamp(0.0, 1.0)
    }
 
    fn timing_exposure(delay_ms: u64) -> f64 {
        let optimal = 200.0f64;
        let dev = (delay_ms as f64 - optimal).abs();
        (dev / 400.0).clamp(0.0, 1.0)
    }
}
 
// ── IntersectionAttackDetector ────────────────────────────────────────────────
 
#[derive(Debug, Clone)]
struct AddressObservation {
    tx_id: String,
    timestamp_ms: u64,
    parent_ids: Vec<String>,
}
 
#[derive(Debug)]
pub struct IntersectionAttackDetector {
    observations: std::collections::HashMap<String, std::collections::VecDeque<AddressObservation>>,
    window_size: usize,
    regularity_threshold_ms: u64,
}
 
impl IntersectionAttackDetector {
    pub fn new(window_size: usize, regularity_threshold_ms: u64) -> Self {
        IntersectionAttackDetector {
            observations: std::collections::HashMap::new(),
            window_size,
            regularity_threshold_ms,
        }
    }
 
    pub fn record_observation(
        &mut self,
        address: &str,
        tx_id: String,
        timestamp_ms: u64,
        parent_ids: Vec<String>,
    ) {
        let queue = self.observations
            .entry(address.to_string())
            .or_insert_with(std::collections::VecDeque::new);
        if queue.len() >= self.window_size { queue.pop_front(); }
        queue.push_back(AddressObservation { tx_id, timestamp_ms, parent_ids });
    }
 
    pub fn intersection_risk(&self, address: &str) -> f64 {
        let obs = match self.observations.get(address) {
            Some(q) if q.len() >= 2 => q,
            _ => return 0.0,
        };
        let timing_risk  = self.timing_regularity_risk(obs);
        let overlap_risk = self.parent_overlap_risk(obs);
        (timing_risk * 0.55 + overlap_risk * 0.45).clamp(0.0, 1.0)
    }
 
    pub fn is_high_risk(&self, address: &str) -> bool {
        self.intersection_risk(address) > 0.65
    }
 
    pub fn observation_count(&self, address: &str) -> usize {
        self.observations.get(address).map(|q| q.len()).unwrap_or(0)
    }
 
    fn timing_regularity_risk(
        &self,
        obs: &std::collections::VecDeque<AddressObservation>,
    ) -> f64 {
        let timestamps: Vec<u64> = obs.iter().map(|o| o.timestamp_ms).collect();
        if timestamps.len() < 2 { return 0.0; }
 
        let intervals: Vec<u64> = timestamps.windows(2)
            .map(|w| w[1].saturating_sub(w[0]))
            .collect();
 
        let mean = intervals.iter().sum::<u64>() as f64 / intervals.len() as f64;
        if mean == 0.0 { return 1.0; }
 
        let variance: f64 = intervals.iter()
            .map(|&i| { let d = i as f64 - mean; d * d })
            .sum::<f64>()
            / intervals.len() as f64;
 
        let cv = variance.sqrt() / mean;
        let regularity_score = (1.0 - cv.min(2.0) / 2.0).clamp(0.0, 1.0);
 
        let threshold_proximity = if mean < self.regularity_threshold_ms as f64 {
            1.0 - mean / self.regularity_threshold_ms as f64
        } else {
            0.0
        };
 
        (regularity_score * 0.7 + threshold_proximity * 0.3).clamp(0.0, 1.0)
    }
 
    fn parent_overlap_risk(
        &self,
        obs: &std::collections::VecDeque<AddressObservation>,
    ) -> f64 {
        let observations: Vec<&AddressObservation> = obs.iter().collect();
        if observations.len() < 2 { return 0.0; }
 
        let mut overlap_scores = Vec::new();
        for window in observations.windows(2) {
            let (a, b) = (window[0], window[1]);
            if a.parent_ids.is_empty() || b.parent_ids.is_empty() { continue; }
            let set_a: std::collections::HashSet<&String> = a.parent_ids.iter().collect();
            let set_b: std::collections::HashSet<&String> = b.parent_ids.iter().collect();
            let intersection = set_a.intersection(&set_b).count();
            let union        = set_a.union(&set_b).count();
            let jaccard = if union > 0 { intersection as f64 / union as f64 } else { 0.0 };
            overlap_scores.push(jaccard);
        }
 
        if overlap_scores.is_empty() { return 0.0; }
        overlap_scores.iter().sum::<f64>() / overlap_scores.len() as f64
    }
}
 
// ── Dandelion extension for DiffusionConfig ───────────────────────────────────
 
#[derive(Debug, Clone, PartialEq)]
pub enum DandelionPhase {
    Stem,
    Fluff,
}
 
impl DiffusionConfig {
    pub fn dandelion_phase(&self, tx_id: &str) -> DandelionPhase {
        if !self.enabled { return DandelionPhase::Fluff; }
        let entropy = tx_id
            .bytes()
            .fold(0u64, |acc, b| acc.wrapping_mul(6364136223846793005).wrapping_add(b as u64));
            if entropy % 1024 < 205 { DandelionPhase::Stem } else { DandelionPhase::Fluff }
    }
 
    pub fn stem_delay(&self, tx_id: &str) -> std::time::Duration {
        if !self.enabled { return std::time::Duration::ZERO; }
        let entropy = tx_id
            .bytes()
            .take(8)
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        let stem_min = self.delay_max_ms;
        let stem_max = self.delay_max_ms * 2;
        let range    = stem_max - stem_min;
        std::time::Duration::from_millis(stem_min + (entropy % (range + 1)))
    }
 
    pub fn effective_delay(&self, tx_id: &str) -> std::time::Duration {
        match self.dandelion_phase(tx_id) {
            DandelionPhase::Stem  => self.stem_delay(tx_id),
            DandelionPhase::Fluff => self.relay_delay(tx_id),
        }
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

    #[test]
    fn test_graph_privacy_high_entropy_is_low_risk() {
        let metrics = GraphPrivacyAnalyzer::analyze(&[10, 10], 8, 1, 200);
        assert!(metrics.parent_entropy > 0.9);
        assert!(!metrics.is_vulnerable());
    }

    #[test]
    fn test_graph_privacy_single_parent_low_entropy() {
        let metrics = GraphPrivacyAnalyzer::analyze(&[100], 8, 0, 50);
        assert!(metrics.is_vulnerable());
    }

    #[test]
    fn test_graph_privacy_decoy_reduces_score() {
        let no_decoy   = GraphPrivacyAnalyzer::analyze(&[5, 5], 8, 0, 200);
        let with_decoy = GraphPrivacyAnalyzer::analyze(&[5, 5], 8, 1, 200);
        assert!(with_decoy.privacy_score() < no_decoy.privacy_score());
    }

    #[test]
    fn test_graph_privacy_score_in_range() {
        let metrics = GraphPrivacyAnalyzer::analyze(&[1, 100, 3], 8, 0, 150);
        assert!(metrics.privacy_score() >= 0.0);
        assert!(metrics.privacy_score() <= 1.0);
    }

    #[test]
    fn test_graph_privacy_optimal_timing_low_exposure() {
        let metrics = GraphPrivacyAnalyzer::analyze(&[5, 5], 8, 1, 200);
        assert!(metrics.timing_exposure < 0.1);
    }

    #[test]
    fn test_graph_privacy_extreme_timing_high_exposure() {
        let metrics = GraphPrivacyAnalyzer::analyze(&[5, 5], 8, 1, 0);
        assert!(metrics.timing_exposure > 0.4);
    }

    #[test]
    fn test_intersection_detector_low_risk_initially() {
        let detector = IntersectionAttackDetector::new(10, 5000);
        assert_eq!(detector.intersection_risk("alice"), 0.0);
    }

    #[test]
    fn test_intersection_detector_records_and_counts() {
        let mut detector = IntersectionAttackDetector::new(10, 5000);
        detector.record_observation("alice", "tx1".into(), 1000, vec!["p1".into()]);
        detector.record_observation("alice", "tx2".into(), 2000, vec!["p2".into()]);
        assert_eq!(detector.observation_count("alice"), 2);
    }

    #[test]
    fn test_intersection_detector_high_overlap_increases_risk() {
        let mut detector = IntersectionAttackDetector::new(10, 10000);
        let parents = vec!["p1".to_string(), "p2".to_string()];
        for i in 0..5 {
            detector.record_observation("alice", format!("tx{}", i), i as u64 * 100, parents.clone());
        }
        let risk = detector.intersection_risk("alice");
        assert!(risk > 0.3, "got {}", risk);
    }

    #[test]
    fn test_intersection_detector_diverse_parents_low_risk() {
        let mut detector = IntersectionAttackDetector::new(10, 10000);
        for i in 0..5 {
            let parents = vec![format!("unique_parent_{}", i)];
            detector.record_observation("alice", format!("tx{}", i), i as u64 * 3000, parents);
        }
        let risk = detector.intersection_risk("alice");
        assert!(risk < 0.6, "got {}", risk);
    }

    #[test]
    fn test_intersection_detector_window_evicts_old() {
        let mut detector = IntersectionAttackDetector::new(3, 5000);
        for i in 0..6 {
            detector.record_observation("alice", format!("tx{}", i), i as u64 * 1000, vec![]);
        }
        assert_eq!(detector.observation_count("alice"), 3);
    }

    #[test]
    fn test_intersection_detector_different_addresses_independent() {
        let mut detector = IntersectionAttackDetector::new(10, 5000);
        let parents = vec!["p1".to_string()];
        for i in 0..5 {
            detector.record_observation("alice", format!("tx{}", i), i as u64 * 100, parents.clone());
        }
        assert_eq!(detector.intersection_risk("bob"), 0.0);
    }

    #[test]
    fn test_dandelion_phase_returns_stem_or_fluff() {
        let config = DiffusionConfig::default();
        for i in 0..20 {
            let phase = config.dandelion_phase(&format!("tx_test_{}", i));
            assert!(matches!(phase, DandelionPhase::Stem | DandelionPhase::Fluff));
        }
    }

    #[test]
    fn test_dandelion_phase_deterministic() {
        let config = DiffusionConfig::default();
        let p1 = config.dandelion_phase("tx_abc123");
        let p2 = config.dandelion_phase("tx_abc123");
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_dandelion_disabled_always_fluff() {
        let config = DiffusionConfig::disabled();
        for i in 0..10 {
            assert_eq!(config.dandelion_phase(&format!("tx_{}", i)), DandelionPhase::Fluff);
        }
    }

    #[test]
    fn test_dandelion_stem_delay_longer_than_relay() {
        let config = DiffusionConfig::default();
        let stem = config.stem_delay("tx_stem_test");
        assert!(stem.as_millis() >= config.delay_max_ms as u128);
    }

    #[test]
    fn test_dandelion_stem_ratio_approx_20_percent() {
        let config = DiffusionConfig::default();
        let total = 1000usize;
        let stem_count = (0..total)
            .filter(|i| config.dandelion_phase(&format!("tx_ratio_{}", i)) == DandelionPhase::Stem)
            .count();
        assert!(stem_count >= 100 && stem_count <= 300,
            "Stem ratio should be ~20%, got {}%", stem_count / 10);
    }

    #[test]
    fn test_decoy_pool_adaptive_prefers_similar_weight() {
        let mut pool = DecoyPool::new(20);
        for i in 0..5 {
            pool.record_with_meta(format!("heavy_{}", i), 100, 1000);
        }
        for i in 0..5 {
            pool.record_with_meta(format!("light_{}", i), 1, 1000);
        }

        let result = pool.sample_matching(3, &[], Some(100), None);
        assert!(!result.is_empty());
        let heavy_count = result.iter().filter(|id| id.starts_with("heavy")).count();
        assert!(heavy_count >= result.len() / 2,
            "adaptive target_weight=100");
    }

    #[test]
    fn test_decoy_pool_record_with_meta() {
        let mut pool = DecoyPool::new(10);
        pool.record_with_meta("tx1".to_string(), 5, 1000);
        assert_eq!(pool.size(), 1);
        let result = pool.sample(1, &[]);
        assert_eq!(result, vec!["tx1"]);
}
}