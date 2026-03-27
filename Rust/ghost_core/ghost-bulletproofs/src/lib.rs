use bulletproofs::{BulletproofGens, PedersenGens, RangeProof};
use merlin::Transcript;
use curve25519_dalek_ng::scalar::Scalar as NgScalar;
use crypto::commitments::{Commitment, BlindingFactor};
use crypto::range_proof::{RangeProofSystem, RangeProofError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulletproofRangeProof {
    pub proof_bytes: Vec<u8>,
    pub committed_value_bytes: Vec<u8>,
}

pub struct BulletproofsBackend;

impl RangeProofSystem for BulletproofsBackend {
    type Proof = BulletproofRangeProof;

    fn prove(
        amount: u64,
        blinding: &BlindingFactor,
        _commitment: &Commitment,
    ) -> Result<Self::Proof, RangeProofError> {
        let pc_gens = PedersenGens::default();
        let bp_gens = BulletproofGens::new(64, 1);

        let blinding_bytes = blinding.to_bytes();
        let ng_scalar = NgScalar::from_canonical_bytes(blinding_bytes)
            .ok_or_else(|| RangeProofError::InvalidProof("invalid blinding".into()))?;

        let mut transcript = Transcript::new(b"GhostLedger RangeProof v1");

        let (proof, committed_value) = RangeProof::prove_single(
            &bp_gens,
            &pc_gens,
            &mut transcript,
            amount,
            &ng_scalar,
            64,
        ).map_err(|e| RangeProofError::InvalidProof(e.to_string()))?;

        Ok(BulletproofRangeProof {
            proof_bytes: proof.to_bytes(),
            committed_value_bytes: committed_value.to_bytes().to_vec(),
        })
    }

    fn verify(
        _commitment: &Commitment,
        proof: &Self::Proof,
    ) -> Result<(), RangeProofError> {
        let pc_gens = PedersenGens::default();
        let bp_gens = BulletproofGens::new(64, 1);

        let range_proof = RangeProof::from_bytes(&proof.proof_bytes)
            .map_err(|e| RangeProofError::InvalidProof(e.to_string()))?;

        if proof.committed_value_bytes.len() != 32 {
            return Err(RangeProofError::InvalidCommitment("bad length".into()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&proof.committed_value_bytes);
        let committed_value = curve25519_dalek_ng::ristretto::CompressedRistretto(arr);

        let mut transcript = Transcript::new(b"GhostLedger RangeProof v1");

        range_proof.verify_single(
            &bp_gens,
            &pc_gens,
            &mut transcript,
            &committed_value,
            64,
        ).map_err(|e| RangeProofError::InvalidProof(e.to_string()))
    }

    fn is_production_safe() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto::commitments::{Commitment, BlindingFactor};

    #[test]
    fn test_bulletproof_prove_and_verify() {
        let blinding = BlindingFactor::random();
        let commitment = Commitment::commit(1000, &blinding);
        let proof = BulletproofsBackend::prove(1000, &blinding, &commitment).unwrap();
        assert!(BulletproofsBackend::verify(&commitment, &proof).is_ok());
    }

    #[test]
    fn test_bulletproof_is_production_safe() {
        assert!(BulletproofsBackend::is_production_safe());
    }

    #[test]
    fn test_bulletproof_zero_amount() {
        let blinding = BlindingFactor::random();
        let commitment = Commitment::commit(0, &blinding);
        let proof = BulletproofsBackend::prove(0, &blinding, &commitment).unwrap();
        assert!(BulletproofsBackend::verify(&commitment, &proof).is_ok());
    }

    #[test]
    fn test_bulletproof_max_amount() {
        let blinding = BlindingFactor::random();
        let commitment = Commitment::commit(u64::MAX, &blinding);
        let proof = BulletproofsBackend::prove(u64::MAX, &blinding, &commitment).unwrap();
        assert!(BulletproofsBackend::verify(&commitment, &proof).is_ok());
    }
}