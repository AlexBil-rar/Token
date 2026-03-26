// crypto/src/range_proof.rs

use serde::{Deserialize, Serialize};

pub trait RangeProofSystem {
    type Proof: Serialize + for<'de> Deserialize<'de> + Clone;

    fn prove(amount: u64) -> Result<(String, Self::Proof), RangeProofError>;
    fn verify(commitment_hex: &str, proof: &Self::Proof) -> Result<(), RangeProofError>;
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

    fn prove(amount: u64) -> Result<(String, PlaceholderProof), RangeProofError> {
        let commitment_hex = format!("{:016x}", amount);
        Ok((commitment_hex, PlaceholderProof {
            amount_bits: 64,
            experimental: true,
        }))
    }

    fn verify(commitment_hex: &str, proof: &PlaceholderProof) -> Result<(), RangeProofError> {
        if !proof.experimental {
            return Err(RangeProofError::InvalidProof(
                "non-experimental proof not supported".into()
            ));
        }
        u64::from_str_radix(commitment_hex, 16)
            .map(|_| ())
            .map_err(|e| RangeProofError::InvalidCommitment(e.to_string()))
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

    #[test]
    fn test_placeholder_prove_and_verify() {
        let (commitment, proof) = PlaceholderRangeProof::prove(1000).unwrap();
        assert!(PlaceholderRangeProof::verify(&commitment, &proof).is_ok());
    }

    #[test]
    fn test_placeholder_is_not_production_safe() {
        assert!(!PlaceholderRangeProof::is_production_safe());
    }

    #[test]
    fn test_placeholder_proof_is_experimental() {
        let (_, proof) = PlaceholderRangeProof::prove(42).unwrap();
        assert!(proof.experimental);
    }

    #[test]
    fn test_invalid_commitment_fails_verify() {
        let (_, proof) = PlaceholderRangeProof::prove(100).unwrap();
        let result = PlaceholderRangeProof::verify("not_hex_at_all!!", &proof);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_amount_valid() {
        let (commitment, proof) = PlaceholderRangeProof::prove(0).unwrap();
        assert!(PlaceholderRangeProof::verify(&commitment, &proof).is_ok());
    }

    #[test]
    fn test_max_amount_valid() {
        let (commitment, proof) = PlaceholderRangeProof::prove(u64::MAX).unwrap();
        assert!(PlaceholderRangeProof::verify(&commitment, &proof).is_ok());
    }
}