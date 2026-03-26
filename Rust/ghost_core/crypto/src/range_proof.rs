// crypto/src/range_proof.rs

use serde::{Deserialize, Serialize};
use crate::commitments::{Commitment, BlindingFactor};

pub trait RangeProofSystem {
    type Proof: Serialize + for<'de> Deserialize<'de> + Clone;

    fn prove(
        amount: u64,
        blinding: &BlindingFactor,
        commitment: &Commitment,
    ) -> Result<Self::Proof, RangeProofError>;

    fn verify(
        commitment: &Commitment,
        proof: &Self::Proof,
    ) -> Result<(), RangeProofError>;

    fn is_production_safe() -> bool;
}

#[derive(Debug, Clone, PartialEq)]
pub enum RangeProofError {
    NotSupported,
    InvalidProof(String),
    InvalidCommitment(String),
}

impl std::fmt::Display for RangeProofError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSupported => write!(f, "range proof not supported in this build"),
            Self::InvalidProof(s) => write!(f, "invalid proof: {}", s),
            Self::InvalidCommitment(s) => write!(f, "invalid commitment: {}", s),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceholderProof {
    pub amount_bits: u8,  
    pub experimental: bool,
}

pub struct PlaceholderRangeProof;

impl RangeProofSystem for PlaceholderRangeProof {
    type Proof = PlaceholderProof;

    fn prove(
        amount: u64,
        _blinding: &BlindingFactor,
        _commitment: &Commitment,
    ) -> Result<PlaceholderProof, RangeProofError> {
        Ok(PlaceholderProof {
            amount_bits: 64,
            experimental: true,
        })
    }

    fn verify(
        _commitment: &Commitment,
        proof: &PlaceholderProof,
    ) -> Result<(), RangeProofError> {
        if !proof.experimental {
            return Err(RangeProofError::InvalidProof(
                "non-experimental proof not supported".into()
            ));
        }
        Ok(())
    }

    fn is_production_safe() -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RangeProofStatus {
    Verified,
    Experimental,
    Missing,
}

impl RangeProofStatus {
    pub fn is_production_safe(&self) -> bool {
        matches!(self, RangeProofStatus::Verified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commitments::{Commitment, BlindingFactor};

    fn make_commitment(amount: u64) -> (BlindingFactor, Commitment) {
        let blinding = BlindingFactor::random();
        let commitment = Commitment::commit(amount, &blinding);
        (blinding, commitment)
    }

    #[test]
    fn test_placeholder_prove_and_verify() {
        let (blinding, commitment) = make_commitment(1000);
        let proof = PlaceholderRangeProof::prove(1000, &blinding, &commitment).unwrap();
        assert!(PlaceholderRangeProof::verify(&commitment, &proof).is_ok());
    }

    #[test]
    fn test_placeholder_is_not_production_safe() {
        assert!(!PlaceholderRangeProof::is_production_safe());
    }

    #[test]
    fn test_placeholder_proof_is_experimental() {
        let (blinding, commitment) = make_commitment(42);
        let proof = PlaceholderRangeProof::prove(42, &blinding, &commitment).unwrap();
        assert!(proof.experimental);
    }

    #[test]
    fn test_zero_amount_valid() {
        let (blinding, commitment) = make_commitment(0);
        let proof = PlaceholderRangeProof::prove(0, &blinding, &commitment).unwrap();
        assert!(PlaceholderRangeProof::verify(&commitment, &proof).is_ok());
    }

    #[test]
    fn test_max_amount_valid() {
        let (blinding, commitment) = make_commitment(u64::MAX);
        let proof = PlaceholderRangeProof::prove(u64::MAX, &blinding, &commitment).unwrap();
        assert!(PlaceholderRangeProof::verify(&commitment, &proof).is_ok());
    }
}