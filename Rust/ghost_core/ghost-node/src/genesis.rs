// ghost-node/src/genesis.rs

use ledger::state::LedgerState;
use tracing::info;

const GENESIS_BALANCE: u64 = 10_000_000;

pub fn bootstrap(state: &mut LedgerState, address: &str) {
    info!("Bootstrapping genesis state");
    info!("Genesis address: {}", address);
    info!("Genesis balance: {} GHOST", GENESIS_BALANCE);

    state.credit(address, GENESIS_BALANCE);

    info!("Genesis state created successfully");
}

pub fn validate_address(address: &str) -> bool {
    address.len() == 40 && address.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_address_valid() {
        assert!(validate_address("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"));
    }

    #[test]
    fn test_validate_address_too_short() {
        assert!(!validate_address("a1b2c3"));
    }

    #[test]
    fn test_validate_address_invalid_chars() {
        assert!(!validate_address("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"));
    }

    #[test]
    fn test_bootstrap_credits_balance() {
        let mut state = ledger::state::LedgerState::new();
        bootstrap(&mut state, "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2");
        assert_eq!(state.get_balance("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"), GENESIS_BALANCE);
    }
}