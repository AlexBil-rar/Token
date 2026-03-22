// crypto/src/commitments.rs

use curve25519_dalek::{
    ristretto::{RistrettoPoint, CompressedRistretto},
    scalar::Scalar,
    constants::RISTRETTO_BASEPOINT_POINT,
};
use sha2::{Sha256, Sha512, Digest};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

fn h_point() -> RistrettoPoint {
    RistrettoPoint::hash_from_bytes::<Sha512>(b"GhostLedger_H_v1")
}

fn g_point() -> RistrettoPoint {
    RISTRETTO_BASEPOINT_POINT
}

#[derive(Clone, Zeroize)]
pub struct BlindingFactor(pub(crate) Scalar);

impl BlindingFactor {
    pub fn random() -> Self {
        let mut bytes = [0u8; 64];
        OsRng.fill_bytes(&mut bytes);
        BlindingFactor(Scalar::from_bytes_mod_order_wide(&bytes))
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: &[u8; 32]) -> Option<Self> {
        Scalar::from_canonical_bytes(*bytes)
            .map(BlindingFactor)
            .into()
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.to_bytes())
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let bytes = hex::decode(s).ok()?;
        if bytes.len() != 32 { return None; }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Self::from_bytes(&arr)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Commitment {
    pub point_hex: String,
}

impl Commitment {
    pub fn commit(amount: u64, blinding: &BlindingFactor) -> Self {
        let amount_scalar = Scalar::from(amount);
        let point = blinding.0 * g_point() + amount_scalar * h_point();
        Commitment {
            point_hex: hex::encode(point.compress().as_bytes()),
        }
    }

    pub fn verify(&self, amount: u64, blinding: &BlindingFactor) -> bool {
        let expected = Self::commit(amount, blinding);
        self.point_hex == expected.point_hex
    }

    pub fn zero() -> Self {
        let blinding = BlindingFactor(Scalar::ZERO);
        Self::commit(0, &blinding)
    }

    fn to_point(&self) -> Option<RistrettoPoint> {
        let bytes = hex::decode(&self.point_hex).ok()?;
        if bytes.len() != 32 { return None; }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        CompressedRistretto(arr).decompress()
    }

    pub fn add(&self, other: &Commitment) -> Option<Commitment> {
        let p1 = self.to_point()?;
        let p2 = other.to_point()?;
        let sum = p1 + p2;
        Some(Commitment {
            point_hex: hex::encode(sum.compress().as_bytes()),
        })
    }

    pub fn sub(&self, other: &Commitment) -> Option<Commitment> {
        let p1 = self.to_point()?;
        let p2 = other.to_point()?;
        let diff = p1 - p2;
        Some(Commitment {
            point_hex: hex::encode(diff.compress().as_bytes()),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceProof {
    pub excess_commitment_hex: String,
    pub excess_signature_hex: String,
}

impl BalanceProof {
    pub fn create(
        input_blindings: &[BlindingFactor],
        output_blindings: &[BlindingFactor],
    ) -> Self {
        let sum_inputs: Scalar = input_blindings.iter().map(|b| b.0).sum();
        let sum_outputs: Scalar = output_blindings.iter().map(|b| b.0).sum();
        let excess = sum_inputs - sum_outputs;

        let excess_point = excess * g_point();
        let excess_hex = hex::encode(excess_point.compress().as_bytes());

        let mut hasher = Sha256::new();
        hasher.update(excess_point.compress().as_bytes());
        let challenge_bytes = hasher.finalize();
        let mut challenge_arr = [0u8; 32];
        challenge_arr.copy_from_slice(&challenge_bytes);
        let challenge = Scalar::from_bytes_mod_order(challenge_arr);
        let signature = challenge * excess;

        BalanceProof {
            excess_commitment_hex: excess_hex,
            excess_signature_hex: hex::encode(signature.to_bytes()),
        }
    }

    pub fn verify(
        &self,
        input_commitments: &[Commitment],
        output_commitments: &[Commitment],
    ) -> bool {
        let sum_inputs: Option<RistrettoPoint> = input_commitments.iter()
            .try_fold(RistrettoPoint::default(), |acc, c| {
                c.to_point().map(|p| acc + p)
            });

        let sum_outputs: Option<RistrettoPoint> = output_commitments.iter()
            .try_fold(RistrettoPoint::default(), |acc, c| {
                c.to_point().map(|p| acc + p)
            });

        let (sum_in, sum_out) = match (sum_inputs, sum_outputs) {
            (Some(i), Some(o)) => (i, o),
            _ => return false,
        };

        let excess_from_commitments = sum_in - sum_out;

        let excess_bytes = match hex::decode(&self.excess_commitment_hex) {
            Ok(b) if b.len() == 32 => b,
            _ => return false,
        };
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&excess_bytes);
        let excess_point = match CompressedRistretto(arr).decompress() {
            Some(p) => p,
            None => return false,
        };

        excess_from_commitments == excess_point
    }
}

pub struct PrivateTxBuilder {
    pub input_amount: u64,
    pub output_amount: u64,
    pub input_blinding: BlindingFactor,
    pub output_blinding: BlindingFactor,
    pub change_blinding: BlindingFactor,
}

impl PrivateTxBuilder {
    pub fn new(input_amount: u64, output_amount: u64) -> Option<Self> {
        if output_amount > input_amount {
            return None;
        }
        Some(PrivateTxBuilder {
            input_amount,
            output_amount,
            input_blinding: BlindingFactor::random(),
            output_blinding: BlindingFactor::random(),
            change_blinding: BlindingFactor::random(),
        })
    }

    pub fn input_commitment(&self) -> Commitment {
        Commitment::commit(self.input_amount, &self.input_blinding)
    }

    pub fn output_commitment(&self) -> Commitment {
        Commitment::commit(self.output_amount, &self.output_blinding)
    }

    pub fn change_commitment(&self) -> Commitment {
        let change = self.input_amount - self.output_amount;
        Commitment::commit(change, &self.change_blinding)
    }

    pub fn balance_proof(&self) -> BalanceProof {
        BalanceProof::create(
            &[self.input_blinding.clone()],
            &[self.output_blinding.clone(), self.change_blinding.clone()],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commitment_verify_correct() {
        let blinding = BlindingFactor::random();
        let c = Commitment::commit(1000, &blinding);
        assert!(c.verify(1000, &blinding));
    }

    #[test]
    fn test_commitment_verify_wrong_amount() {
        let blinding = BlindingFactor::random();
        let c = Commitment::commit(1000, &blinding);
        assert!(!c.verify(999, &blinding));
    }

    #[test]
    fn test_commitment_verify_wrong_blinding() {
        let b1 = BlindingFactor::random();
        let b2 = BlindingFactor::random();
        let c = Commitment::commit(1000, &b1);
        assert!(!c.verify(1000, &b2));
    }

    #[test]
    fn test_two_commitments_same_amount_different_blinding() {
        let b1 = BlindingFactor::random();
        let b2 = BlindingFactor::random();
        let c1 = Commitment::commit(500, &b1);
        let c2 = Commitment::commit(500, &b2);
        assert_ne!(c1.point_hex, c2.point_hex);
    }

    #[test]
    fn test_homomorphic_addition() {
        let b1 = BlindingFactor::random();
        let b2 = BlindingFactor::random();

        let c1 = Commitment::commit(300, &b1);
        let c2 = Commitment::commit(700, &b2);

        let sum_blinding = BlindingFactor(b1.0 + b2.0);
        let c_sum = Commitment::commit(1000, &sum_blinding);

        let c_added = c1.add(&c2).unwrap();
        assert_eq!(c_added.point_hex, c_sum.point_hex);
    }

    #[test]
    fn test_balance_proof_valid() {
        let input_amount = 1000u64;
        let output_amount = 700u64;
        let change = 300u64;

        let b_in = BlindingFactor::random();
        let b_out = BlindingFactor::random();
        let b_change = BlindingFactor::random();

        let c_in = Commitment::commit(input_amount, &b_in);
        let c_out = Commitment::commit(output_amount, &b_out);
        let c_change = Commitment::commit(change, &b_change);

        let proof = BalanceProof::create(
            &[b_in],
            &[b_out, b_change],
        );

        assert!(proof.verify(&[c_in], &[c_out, c_change]));
    }

    #[test]
    fn test_balance_proof_invalid_wrong_amounts() {
        let b_in = BlindingFactor::random();
        let b_out = BlindingFactor::random();

        let c_in = Commitment::commit(1000, &b_in);
        let c_out = Commitment::commit(999, &b_out);

        let proof = BalanceProof::create(&[b_in], &[b_out]);

        assert!(!proof.verify(&[c_in], &[c_out]));
    }

    #[test]
    fn test_private_tx_builder() {
        let builder = PrivateTxBuilder::new(1000, 700).unwrap();

        let c_in = builder.input_commitment();
        let c_out = builder.output_commitment();
        let c_change = builder.change_commitment();
        let proof = builder.balance_proof();

        assert!(proof.verify(&[c_in], &[c_out, c_change]));
    }

    #[test]
    fn test_private_tx_builder_full_amount() {
        let builder = PrivateTxBuilder::new(500, 500).unwrap();
        let c_in = builder.input_commitment();
        let c_out = builder.output_commitment();
        let c_change = builder.change_commitment();
        let proof = builder.balance_proof();
        assert!(proof.verify(&[c_in], &[c_out, c_change]));
    }

    #[test]
    fn test_private_tx_builder_insufficient_returns_none() {
        assert!(PrivateTxBuilder::new(100, 200).is_none());
    }

    #[test]
    fn test_blinding_factor_hex_roundtrip() {
        let b = BlindingFactor::random();
        let hex = b.to_hex();
        let restored = BlindingFactor::from_hex(&hex).unwrap();
        assert_eq!(b.to_bytes(), restored.to_bytes());
    }

    #[test]
    fn test_commitment_is_32_bytes_hex() {
        let b = BlindingFactor::random();
        let c = Commitment::commit(42, &b);
        assert_eq!(c.point_hex.len(), 64);
    }
}