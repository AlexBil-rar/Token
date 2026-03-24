// consensus/src/byzantine_sim.rs

use std::collections::HashMap;

pub const SIGMA: f64 = 2.0;
pub const ALPHA: f64 = 3.0;
pub const BYZANTINE_BOUND: f64 = 1.0 / (SIGMA * ALPHA);

#[derive(Debug, Clone)]
pub struct SimulationParams {
    pub adversary_stake_fraction: f64,
    pub initial_winner_score: f64,
    pub initial_loser_score: f64,
    pub max_steps: usize,
    pub trials: usize,
}

impl SimulationParams {
    pub fn new(f: f64) -> Self {
        SimulationParams {
            adversary_stake_fraction: f,
            initial_winner_score: SIGMA * 3.0,
            initial_loser_score: 3.0,
            max_steps: 10_000,
            trials: 1_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub adversary_stake_fraction: f64,
    pub revert_probability: f64,
    pub mean_steps_to_revert: Option<f64>,
    pub trials: usize,
    pub reverts: usize,
    pub theoretical_bound: f64,
}

impl SimulationResult {
    pub fn bound_holds(&self) -> bool {
        self.revert_probability <= self.theoretical_bound + 1e-6
    }

    pub fn is_safe(&self) -> bool {
        self.adversary_stake_fraction < BYZANTINE_BOUND &&
        self.revert_probability < 0.01
    }
}

pub fn simulate_adversary(params: &SimulationParams) -> SimulationResult {
    let f = params.adversary_stake_fraction;

    let p_honest = 1.0 - f;
    let p_adversary = f;

    let mut reverts = 0usize;
    let mut steps_to_revert_sum = 0.0;

    let seed = 12345u64;
    let mut rng_state = seed;

    for _ in 0..params.trials {
        let mut winner_score = params.initial_winner_score;
        let mut loser_score = params.initial_loser_score;
        let mut reverted = false;
        let mut steps = 0;

        for _ in 0..params.max_steps {
            rng_state = xorshift64(rng_state);
            let roll = (rng_state as f64) / (u64::MAX as f64);

            if roll < p_honest {
                winner_score += 1.0;
            } else {
                loser_score += ALPHA;
            }

            steps += 1;

            if loser_score >= winner_score {
                reverted = true;
                break;
            }
        }

        if reverted {
            reverts += 1;
            steps_to_revert_sum += steps as f64;
        }
    }

    let revert_prob = reverts as f64 / params.trials as f64;

    let mean_steps = if reverts > 0 {
        Some(steps_to_revert_sum / reverts as f64)
    } else {
        None
    };

    let drift = p_honest - p_adversary * ALPHA;
    let theoretical = if drift > 0.0 {
        (params.initial_loser_score / params.initial_winner_score).min(1.0)
    } else {
        1.0
    };

    SimulationResult {
        adversary_stake_fraction: f,
        revert_probability: revert_prob,
        mean_steps_to_revert: mean_steps,
        trials: params.trials,
        reverts,
        theoretical_bound: theoretical,
    }
}

fn xorshift64(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRIFT_BOUNDARY: f64 = 1.0 / (1.0 + ALPHA);

    #[test]
    fn test_byzantine_bound_constant() {
        assert!((BYZANTINE_BOUND - 1.0/6.0).abs() < 1e-9);
        assert!(BYZANTINE_BOUND < DRIFT_BOUNDARY,
            "Conservative bound должен быть меньше drift boundary");
    }

    #[test]
    fn test_drift_positive_below_boundary() {
        let f = 0.10;
        let drift = (1.0 - f) - f * ALPHA;
        assert!(drift > 0.0,
            "f={f}: drift={drift:.4} должен быть положительным");
    }

    #[test]
    fn test_drift_negative_above_boundary() {
        let f = 0.30;
        assert!(f > DRIFT_BOUNDARY);
        let drift = (1.0 - f) - f * ALPHA;
        assert!(drift < 0.0,
            "f={f}: drift={drift:.4} должен быть отрицательным");
    }

    #[test]
    fn test_safe_adversary_rarely_reverts() {
        let f = 0.10;
        let mut params = SimulationParams::new(f);
        params.initial_winner_score = SIGMA * 20.0;
        params.initial_loser_score = 20.0;
        let result = simulate_adversary(&params);
        assert!(result.revert_probability < 0.20,
            "f={f:.2}: P(revert)={:.4} должна быть < 20% при большом gap",
            result.revert_probability);
    }

    #[test]
    fn test_dangerous_adversary_reverts_more() {
        let f_safe = 0.05;
        let f_dangerous = 0.30;
        assert!(f_dangerous > DRIFT_BOUNDARY);
        let result_safe = simulate_adversary(&SimulationParams::new(f_safe));
        let result_dangerous = simulate_adversary(&SimulationParams::new(f_dangerous));
        assert!(result_dangerous.revert_probability > result_safe.revert_probability,
            "Более сильный adversary должен иметь выше P(revert)");
    }

    #[test]
    fn test_revert_probability_decreases_with_gap() {
        let f = 0.10;
        let mut params_small = SimulationParams::new(f);
        let mut params_large = SimulationParams::new(f);
        params_small.initial_winner_score = SIGMA * 3.0;
        params_small.initial_loser_score = 3.0;
        params_large.initial_winner_score = SIGMA * 30.0;
        params_large.initial_loser_score = 30.0;
        let result_small = simulate_adversary(&params_small);
        let result_large = simulate_adversary(&params_large);
        assert!(result_large.revert_probability <= result_small.revert_probability,
            "Больший gap: {:.4} <= {:.4}",
            result_large.revert_probability,
            result_small.revert_probability);
    }

    #[test]
    fn test_conjecture_f_zero_adversary_never_reverts() {
        let result = simulate_adversary(&SimulationParams::new(0.0));
        assert_eq!(result.reverts, 0);
    }

    #[test]
    fn test_conjecture_f_high_adversary_often_reverts() {
        let mut params = SimulationParams::new(0.35);
        params.trials = 500;
        let result = simulate_adversary(&params);
        assert!(result.revert_probability > 0.1,
            "35% adversary должен часто reverting: P={:.4}",
            result.revert_probability);
    }

    #[test]
    fn test_conjecture_f_boundary_drift() {
        let f_below = DRIFT_BOUNDARY * 0.5;
        let f_above = DRIFT_BOUNDARY * 1.5;
        let drift_below = (1.0 - f_below) - f_below * ALPHA;
        let drift_above = (1.0 - f_above) - f_above * ALPHA;
        assert!(drift_below > 0.0, "Ниже boundary: drift={drift_below:.4} > 0");
        assert!(drift_above < 0.0, "Выше boundary: drift={drift_above:.4} < 0");
        let result_below = simulate_adversary(&SimulationParams::new(f_below));
        let result_above = simulate_adversary(&SimulationParams::new(f_above));
        assert!(result_below.revert_probability < result_above.revert_probability,
            "below={:.4} < above={:.4}",
            result_below.revert_probability,
            result_above.revert_probability);
    }

    #[test]
    fn test_sigma_conservative_bound() {
        for f in [0.05f64, 0.10, 0.15] {
            let drift = (1.0 - f) - f * ALPHA;
            assert!(drift > 0.0,
                "f={f}: drift={drift:.4} должен быть > 0 при f < {BYZANTINE_BOUND:.4}");
        }
    }
}