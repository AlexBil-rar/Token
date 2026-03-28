// ghost-params/src/lib.rs

pub const BETA: f64 = 0.7;
pub const EPSILON: f64 = 0.10;
pub const EPSILON_PRIVACY: f64 = 0.20;
pub const EPSILON_CONSENSUS: f64 = 0.00;
pub const MAX_PARENTS: usize = 2;

pub const SIGMA: f64 = 2.0;
pub const THETA: u64 = 6;
pub const RESOLVE_MIN_WEIGHT: u64 = 3;

pub const MIN_STAKE: u64 = 1_000;
pub const SLASH_PERCENT: f64 = 0.10;
pub const MAX_VIOLATIONS: usize = 3;

pub const MIN_DIFFICULTY: usize = 2;
pub const MAX_DIFFICULTY: usize = 6;

pub const CHECKPOINT_INTERVAL: u64 = 500;
pub const CHECKPOINT_MIN_WEIGHT: u64 = THETA;

pub const MAX_PEERS: usize = 128;
pub const STEM_MAX_TTL: u8 = 10;
pub const MAX_MSG_BYTES: usize = 1 * 1024 * 1024;

pub const TOTAL_SUPPLY: u64 = 21_000_000;
pub const GENESIS_SHARE: f64 = 0.10;

pub mod wire {
    pub const WIRE_VERSION: u8 = 1;
    pub const WIRE_MAGIC: [u8; 4] = [0x47, 0x48, 0x53, 0x54];
    pub const MAX_WIRE_PAYLOAD: usize = 1 * 1024 * 1024;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_beta_in_range() {
        assert!(BETA > 0.0 && BETA <= 1.0);
    }

    #[test]
    fn test_epsilon_presets_ordered() {
        assert!(EPSILON_CONSENSUS < EPSILON);
        assert!(EPSILON < EPSILON_PRIVACY);
    }

    #[test]
    fn test_sigma_above_one() {
        assert!(SIGMA > 1.0);
    }

    #[test]
    fn test_checkpoint_weight_matches_theta() {
        assert_eq!(CHECKPOINT_MIN_WEIGHT, THETA);
    }

    #[test]
    fn test_min_stake_positive() {
        assert!(MIN_STAKE > 0);
    }

    #[test]
    fn test_difficulty_range_valid() {
        assert!(MIN_DIFFICULTY < MAX_DIFFICULTY);
    }
}