// crypto/src/wallet.rs

use ed25519_dalek::{SigningKey, VerifyingKey, Signer, Verifier, Signature};
use rand::rngs::OsRng;
use crate::hashing::sha256_hex;

pub struct Wallet {
    pub private_key: [u8; 32],   
    pub public_key: [u8; 32],    
    pub address: String,          
}

impl Wallet {
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let private_key = signing_key.to_bytes();
        let public_key = verifying_key.to_bytes();
        let address = derive_address(&public_key);

        Wallet { private_key, public_key, address }
    }

    pub fn from_private_key(private_key_hex: &str) -> Result<Self, String> {
        let bytes = hex::decode(private_key_hex)
            .map_err(|e| format!("invalid hex: {}", e))?;

        if bytes.len() != 32 {
            return Err("private key must be 32 bytes".to_string());
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&bytes);

        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();

        let public_key = verifying_key.to_bytes();
        let address = derive_address(&public_key);

        Ok(Wallet { private_key: key_bytes, public_key, address })
    }

    pub fn sign(&self, payload: &[u8]) -> String {
        let signing_key = SigningKey::from_bytes(&self.private_key);
        let signature: Signature = signing_key.sign(payload);
        hex::encode(signature.to_bytes())
    }

    pub fn private_key_hex(&self) -> String {
        hex::encode(self.private_key)
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key)
    }
}

fn derive_address(public_key: &[u8; 32]) -> String {
    let pub_hex = hex::encode(public_key);
    let hash = sha256_hex(pub_hex.as_bytes());
    hash[..40].to_string()
}

pub fn verify_signature(
    public_key_hex: &str,
    payload: &[u8],
    signature_hex: &str,
) -> bool {
    let pub_bytes = match hex::decode(public_key_hex) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let sig_bytes = match hex::decode(signature_hex) {
        Ok(b) => b,
        Err(_) => return false,
    };

    if pub_bytes.len() != 32 || sig_bytes.len() != 64 {
        return false;
    }

    let mut pub_arr = [0u8; 32];
    pub_arr.copy_from_slice(&pub_bytes);

    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);

    let verifying_key = match VerifyingKey::from_bytes(&pub_arr) {
        Ok(k) => k,
        Err(_) => return false,
    };

    let signature = Signature::from_bytes(&sig_arr);
    verifying_key.verify(payload, &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_wallet() {
        let wallet = Wallet::generate();
        assert_eq!(wallet.address.len(), 40);
        assert_eq!(wallet.public_key.len(), 32);
        assert_eq!(wallet.private_key.len(), 32);
    }

    #[test]
    fn test_two_wallets_are_different() {
        let w1 = Wallet::generate();
        let w2 = Wallet::generate();
        assert_ne!(w1.address, w2.address);
        assert_ne!(w1.public_key, w2.public_key);
    }

    #[test]
    fn test_sign_and_verify() {
        let wallet = Wallet::generate();
        let payload = b"ghostledger transaction payload";

        let signature = wallet.sign(payload);

        assert!(verify_signature(
            &wallet.public_key_hex(),
            payload,
            &signature,
        ));
    }

    #[test]
    fn test_wrong_key_fails_verification() {
        let wallet_a = Wallet::generate();
        let wallet_b = Wallet::generate();
        let payload = b"test payload";

        let signature = wallet_a.sign(payload);

        assert!(!verify_signature(
            &wallet_b.public_key_hex(),
            payload,
            &signature,
        ));
    }

    #[test]
    fn test_tampered_payload_fails() {
        let wallet = Wallet::generate();
        let payload = b"original payload";
        let signature = wallet.sign(payload);

        assert!(!verify_signature(
            &wallet.public_key_hex(),
            b"tampered payload",
            &signature,
        ));
    }

    #[test]
    fn test_from_private_key_roundtrip() {
        let wallet = Wallet::generate();
        let priv_hex = wallet.private_key_hex();

        let restored = Wallet::from_private_key(&priv_hex).unwrap();
        assert_eq!(wallet.address, restored.address);
        assert_eq!(wallet.public_key_hex(), restored.public_key_hex());
    }

    #[test]
    fn test_address_is_40_chars() {
        for _ in 0..10 {
            let wallet = Wallet::generate();
            assert_eq!(wallet.address.len(), 40);
        }
    }
}