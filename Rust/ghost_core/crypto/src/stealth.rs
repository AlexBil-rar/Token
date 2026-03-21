// crypto/src/stealth.rs

use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};
use sha2::{Sha256, Digest};
use rand::rngs::OsRng;

pub struct StealthKeys {
    pub spend_private: [u8; 32],
    pub spend_public: [u8; 32],
}

impl StealthKeys {
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        StealthKeys {
            spend_private: secret.to_bytes(),
            spend_public: public.to_bytes(),
        }
    }

    pub fn spend_public_hex(&self) -> String {
        hex::encode(self.spend_public)
    }

    pub fn spend_private_hex(&self) -> String {
        hex::encode(self.spend_private)
    }
}
pub struct StealthPayment {
    pub stealth_address: String,  
    pub ephemeral_pubkey: String, 
}

pub fn generate_stealth_payment(recipient_spend_pubkey_hex: &str) -> Result<StealthPayment, String> {
    let pub_bytes = hex::decode(recipient_spend_pubkey_hex)
        .map_err(|e| format!("invalid hex: {}", e))?;

    if pub_bytes.len() != 32 {
        return Err("public key must be 32 bytes".to_string());
    }

    let mut pub_arr = [0u8; 32];
    pub_arr.copy_from_slice(&pub_bytes);
    let recipient_pubkey = PublicKey::from(pub_arr);

    let ephemeral_secret = EphemeralSecret::random_from_rng(OsRng);
    let ephemeral_public = PublicKey::from(&ephemeral_secret);

    let shared_secret = ephemeral_secret.diffie_hellman(&recipient_pubkey);

    let stealth_address = derive_stealth_address(shared_secret.as_bytes(), &pub_arr);
    let ephemeral_pubkey = hex::encode(ephemeral_public.as_bytes());

    Ok(StealthPayment { stealth_address, ephemeral_pubkey })
}

pub fn scan_for_payment(
    spend_private_hex: &str,
    spend_public_hex: &str,
    ephemeral_pubkey_hex: &str,
) -> Option<String> {
    let priv_bytes = hex::decode(spend_private_hex).ok()?;
    let pub_bytes = hex::decode(spend_public_hex).ok()?;
    let eph_bytes = hex::decode(ephemeral_pubkey_hex).ok()?;

    if priv_bytes.len() != 32 || pub_bytes.len() != 32 || eph_bytes.len() != 32 {
        return None;
    }

    let mut priv_arr = [0u8; 32];
    priv_arr.copy_from_slice(&priv_bytes);

    let mut pub_arr = [0u8; 32];
    pub_arr.copy_from_slice(&pub_bytes);

    let mut eph_arr = [0u8; 32];
    eph_arr.copy_from_slice(&eph_bytes);

    let spend_private = StaticSecret::from(priv_arr);
    let ephemeral_pubkey = PublicKey::from(eph_arr);

    let shared_secret = spend_private.diffie_hellman(&ephemeral_pubkey);

    Some(derive_stealth_address(shared_secret.as_bytes(), &pub_arr))
}

fn derive_stealth_address(shared_secret: &[u8], spend_pubkey: &[u8; 32]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(shared_secret);
    hasher.update(spend_pubkey);
    let hash = hasher.finalize();
    hex::encode(hash)[..40].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_stealth_keys() {
        let keys = StealthKeys::generate();
        assert_eq!(keys.spend_public.len(), 32);
        assert_eq!(keys.spend_private.len(), 32);
        assert_ne!(keys.spend_public_hex(), keys.spend_private_hex());
    }

    #[test]
    fn test_generate_stealth_payment_produces_address() {
        let bob = StealthKeys::generate();
        let payment = generate_stealth_payment(&bob.spend_public_hex()).unwrap();
        assert_eq!(payment.stealth_address.len(), 40);
        assert_eq!(payment.ephemeral_pubkey.len(), 64);
    }

    #[test]
    fn test_two_payments_different_addresses() {
        let bob = StealthKeys::generate();
        let p1 = generate_stealth_payment(&bob.spend_public_hex()).unwrap();
        let p2 = generate_stealth_payment(&bob.spend_public_hex()).unwrap();
        assert_ne!(p1.stealth_address, p2.stealth_address);
        assert_ne!(p1.ephemeral_pubkey, p2.ephemeral_pubkey);
    }

    #[test]
    fn test_recipient_finds_own_payment() {
        let bob = StealthKeys::generate();
        let payment = generate_stealth_payment(&bob.spend_public_hex()).unwrap();

        let found = scan_for_payment(
            &bob.spend_private_hex(),
            &bob.spend_public_hex(),
            &payment.ephemeral_pubkey,
        );

        assert_eq!(found, Some(payment.stealth_address));
    }

    #[test]
    fn test_wrong_recipient_cannot_find_payment() {
        let bob = StealthKeys::generate();
        let alice = StealthKeys::generate();

        let payment = generate_stealth_payment(&bob.spend_public_hex()).unwrap();

        let found = scan_for_payment(
            &alice.spend_private_hex(),
            &alice.spend_public_hex(),
            &payment.ephemeral_pubkey,
        );

        assert_ne!(found, Some(payment.stealth_address));
    }

    #[test]
    fn test_stealth_payment_is_deterministic() {
        let bob = StealthKeys::generate();
        let payment = generate_stealth_payment(&bob.spend_public_hex()).unwrap();

        let found1 = scan_for_payment(
            &bob.spend_private_hex(),
            &bob.spend_public_hex(),
            &payment.ephemeral_pubkey,
        );
        let found2 = scan_for_payment(
            &bob.spend_private_hex(),
            &bob.spend_public_hex(),
            &payment.ephemeral_pubkey,
        );

        assert_eq!(found1, found2);
        assert_eq!(found1, Some(payment.stealth_address));
    }
}