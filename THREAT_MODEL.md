# GhostLedger Threat Model

**Version:** 0.2  
**Date:** March 2026  
**Author:** Aleksandr Bilyk  
**Status:** Research prototype — no security guarantees implied

---

## 1. Scope

This document describes the threat model for GhostLedger v0.1: a feeless, private, DAG-based payment ledger. It covers the attack surface across four layers — network, consensus, privacy, and implementation — and maps each threat to the current mitigation status.

This is not a formal security proof. Where mitigations are partial or absent, that is stated explicitly.

---

## 2. Trust Assumptions

### 2.1 What we assume

- **Honest majority by stake.** The protocol assumes that adversarial nodes control less than 1/6 of total staked weight (Conjecture F). This is more conservative than the standard 1/3 Byzantine bound, justified by the stake-amplification factor α=3.0 in the conflict resolver.
- **Eventual message delivery.** The network model is partially synchronous. Messages are eventually delivered but may be arbitrarily delayed. The protocol does not assume a global clock.
- **Honest genesis.** The genesis state is trusted. There is no trustless bootstrap mechanism yet.
- **No key compromise.** Ed25519 signing keys are assumed secure. Private key management is out of scope.

### 2.2 What we do not assume

- Synchronous message delivery.
- Honest behavior from nodes that do not hold stake.
- Privacy of the network layer (IP addresses, connection topology).
- Security of the host operating system or hardware.

---

## 3. Attacker Model

We consider two classes of adversary:

**Class A — Rational adversary.** Wants to double-spend or gain economic advantage. Does not act in ways that harm themselves without corresponding benefit. Bounded stake fraction f < 1/6.

**Class B — Disruptive adversary.** Wants to degrade protocol availability or deanonymize users. May act irrationally (i.e., incur cost without economic benefit). Bounded by network resource constraints and PoW cost.

In the privacy threat model (Section 6), we additionally consider a **passive observer** who sees all network traffic and all on-chain data but cannot break cryptographic primitives.

---

## 4. Network-Layer Threats

### 4.1 Eclipse Attack

**Description.** An adversary monopolizes all of a target node's peer connections, isolating it from the honest network. The node then receives a false view of the DAG and can be fed a fraudulent chain of transactions.

**Attack vector.** Fill the target's peer list (max 128 entries) with adversary-controlled addresses. Requires controlling enough IPs to saturate the list, or exploiting the peer discovery mechanism.

**Current mitigation.**
- `PeerList::check_eclipse()` detects when ≥80% of peers share the same /16 subnet and logs a warning.
- Peer list is capped at 128 entries.
- Random gossip sampling (`gossip_sample()` size 8) distributes messages across diverse peers.

**Gaps.**
- Eclipse detection triggers a warning only — there is no automatic peer rotation or connection rejection.
- No minimum diversity requirement is enforced when adding peers.
- No QUIC or transport-level authentication; adversary can trivially spoof peer addresses at the application layer.

**Residual risk.** Medium. The detection exists but the response does not. A targeted eclipse still requires controlling many IPs, but the mitigation is incomplete.

---

### 4.2 Sybil Attack

**Description.** An adversary creates many fake identities (nodes) to disproportionately influence peer discovery, gossip, or consensus weight.

**Attack vector.** Register many node addresses; flood peer discovery with adversary addresses; attempt to dilute honest weight.

**Current mitigation.**
- Consensus weight is stake-weighted. Sybil nodes with no stake have multiplier 1.0 — they contribute at most the same as unstaked honest nodes.
- Stake requires locking GHOST tokens, imposing a real economic cost per identity.
- Eclipse detection limits subnet concentration.

**Gaps.**
- Staking is not yet a hard gate: a node with zero stake can still participate in gossip and submit transactions, contributing weight=1 per transaction.
- There is no identity binding between a node's network address and its stake record; a node can stake and then run many unstaked copies.

**Residual risk.** Low for consensus (stake cost), medium for gossip flooding.

---

### 4.3 DoS via Message Flooding

**Description.** An adversary sends a high volume of invalid or valid-but-expensive messages to exhaust target node resources.

**Attack vector.**
1. Send many invalid transactions (no valid PoW).
2. Send many valid transactions from many addresses just under the rate limit.
3. Send oversized WebSocket messages.

**Current mitigation.**
- All transactions require PoW with dynamic difficulty (2–6 leading zeros, auto-adjusting with TPS). High flood rate raises difficulty for all.
- Per-address rate limiting: soft limit 5 tx per 10 seconds, burst limit 10 tx per 10 seconds.
- `SeenSet` deduplicates gossiped transactions (max 10,000 entries with LRU eviction).
- WebSocket message size cap enforced at 1MB per message.

**Gaps.**
- Dynamic PoW penalizes all senders including honest ones during an attack.
- `SeenSet` eviction under sustained flood may cause re-processing of seen transactions.

**Residual risk.** Medium. PoW provides meaningful friction but does not prevent sustained low-rate flooding.

---

### 4.4 Timing Correlation Attack

**Description.** A passive observer correlates the time a transaction appears at different nodes to identify the originating node.

**Attack vector.** Monitor multiple network vantage points; use arrival time differences to triangulate transaction origin.

**Current mitigation.**
- `DiffusionConfig` applies a deterministic random relay delay (50–500ms) per transaction.
- Dandelion-style stem/fluff phases. ~20% of transactions enter a stem phase (500–1000ms delay, single-peer relay) before fluff broadcast, breaking naive timing triangulation.

**Gaps.**
- Stem phase is implemented at the delay level only. True Dandelion requires selecting a random stem relay path and switching to broadcast only at the fluff node. Current implementation applies stem delay locally but still broadcasts to all peers.
- Delay is deterministic per tx_id — a global adversary who knows the tx_id can predict the delay.
- No cover traffic or dummy transactions to obscure absence of activity.

**Residual risk.** Medium-high. Timing protection is partial. Stem phase reduces but does not eliminate timing correlation.

---

### 4.5 Parasite DAG Attack

**Description.** An adversary builds a private branch of the DAG that conflicts with the honest chain, then releases it to attempt a double-spend.

**Attack vector.** Submit transaction T₁ to the honest network. Simultaneously mine a private DAG branch containing T₂ (conflicting with T₁). Release the private branch when it has sufficient weight to dominate T₁.

**Current mitigation.**
- Closure requires σ-dominance: `score(winner) ≥ 2.0 · score(second)`. A parasite branch must accumulate more than 2× the score of the honest winner.
- Score is stake-weighted: a node with stake fraction f can accumulate at most multiplier = 1 + f·2 per transaction. A rational adversary with f < 1/6 faces a 7:1 uphill battle per honest transaction.
- State root anchoring: syncing nodes verify incoming state against the local trusted checkpoint root. A parasite state diverging from the honest checkpoint is rejected.

**Formal bound (Conjecture F).** The simulation in `byzantine_sim.rs` shows that with f < 1/6, the probability of a successful revert is < 1% when the honest winner has weight gap ≥ 3× the loser. This is empirically validated but not formally proven.

**Gaps.**
- The σ=2.0 threshold is chosen empirically. No closed-form bound on the required threshold as a function of adversary stake is derived.
- Private branch mining is not detected until release.
- Long-range attacks (building a branch from an old checkpoint) are not explicitly analyzed.

**Residual risk.** Low for rational adversary with f < 1/6. Uncharacterized for long-range attacks.

---

## 5. Consensus-Layer Threats

### 5.1 Double-Spend Attack

**Description.** An adversary submits two conflicting transactions T₁ and T₂ (same sender, same nonce) hoping both will be accepted by different nodes.

**Current mitigation.**
- `ConflictResolver` detects conflicts via `(sender, nonce)` key. Conflicting transactions enter the 5-state machine.
- Closure predicate requires both Ready (weight ≥ 3) and Dominant (σ=2.0) conditions. Only one transaction can satisfy dominance.
- PHA (Partition Healing Algorithm) ensures that after network reconnection, nodes converge to the same global winner (Theorem P).

**Gaps.**
- During the Ready phase (before closure), both T₁ and T₂ are in the DAG simultaneously. A node that receives only one side of the conflict may temporarily apply a transaction that will later be marked Conflict.
- The state machine applies the transaction optimistically; reversal on conflict marking requires re-application of state, which is not currently implemented.

**Residual risk.** Low for closed conflicts. Present but bounded for the Ready→ClosedLocal window.

---

### 5.2 Stake Grinding / Weight Manipulation

**Description.** An adversary manipulates stake distribution or weight propagation to influence conflict resolution outcomes.

**Attack vector.**
1. Accumulate stake just below the detection threshold to maximize multiplier without visibility.
2. Time transaction submission to exploit temporary tip configurations.

**Current mitigation.**
- Stake influence is capped at 3× (MAX_STAKE_INFLUENCE = 3.0). A node with 100% of stake has at most 3× score amplification.
- Stake is a public on-chain record.

**Gaps.**
- No slashing for submitting conflicting transactions. The economic disincentive for double-spend attempts is only the staked amount (which is not lost unless a separate slashing event is triggered by a different violation).
- Weight manipulation via selective parent choice: an adversary with β=1.0 (greedy) can concentrate weight on specific tips. This is partially mitigated by the β/ε policy but not enforced.

**Residual risk.** Low-medium. Cap limits impact; slashing gap reduces disincentive.

---

### 5.3 Partition Attack (Split-Brain)

**Description.** A network partition causes two honest subsets to independently close the same conflict differently, then reconnect with inconsistent global state.

**Current mitigation.**
- PHA (Partition Healing Algorithm) handles this explicitly. Steps: (1) handshake to find common checkpoint cp*, (2) exchange transactions above cp*, (3) downgrade ClosedLocal conflicts to Reconciling, (4) re-evaluate using frozen stake at cp*, (5) converge to ClosedGlobal.
- Theorem P: if both subsets use the same DAG state above cp* and the same stake frozen at cp*, they compute the same global winner.

**Gaps.**
- Theorem P holds under GST (Global Stabilization Time) — after the partition heals and all messages are eventually delivered. During the partition, no liveness guarantee holds.
- Multi-way partitions (>2 components) are not formally analyzed.
- PHA assumes both nodes have the same cp* as a common ancestor. If cp* itself is not yet finalized on one side, the handshake falls back to no-verification mode (bootstrap path), which bypasses the safety guarantee.

**Residual risk.** Low for two-way partitions post-GST. Uncharacterized for multi-way or prolonged partitions.

---

## 6. Privacy-Layer Threats

### 6.1 Amount Linkability

**Description.** An observer traces transaction amounts to link senders and receivers.

**Current mitigation.**
- Pedersen commitments on Ristretto255 hide transaction amounts: `C(v, r) = r·G + v·H` where r is a cryptographically random blinding factor. This is a real commitment with hiding and binding properties — not a hash of the amount.
- `BalanceProof` verifies no value creation via excess commitment: `Σ C_inputs - Σ C_outputs = excess · G`, where `excess = Σ r_inputs - Σ r_outputs`.
- `excess_signature` proves the sender knows the blinding difference, preventing forgery.
- Three-step validator (`validate_confidential_tx`): range proof → balance proof → excess check.
- Commitments are homomorphically additive: cut-through pruning is safe because `Σ excess_kernels` encodes the full value conservation proof for the pruned history.

**Gaps.**
- Commitments are optional. Transparent transactions leak amounts in full.
- Range proofs are currently `PlaceholderRangeProof` (`is_production_safe() = false`). The `trait RangeProofSystem` provides the abstraction; a Bulletproofs backend is planned. Without range proofs, a commitment could theoretically hide a negative amount. The excess-zero proof prevents inflation but does not prove non-negativity of individual outputs.

**Residual risk.** Low for the inflation vector (excess proof prevents it). Medium for negative-amount outputs until Bulletproofs are integrated. High for transparent transactions.

---

### 6.2 Receiver Linkability

**Description.** An observer links multiple transactions to the same receiver address.

**Current mitigation.**
- Stealth addresses (X25519 ECDH). Each payment generates a fresh stealth address using the recipient's spend public key and an ephemeral key pair. No two payments to the same recipient share an address.
- `scan_for_payment()` allows recipients to scan for incoming payments without revealing their spend key.

**Gaps.**
- Stealth addresses are optional. Transparent receiver addresses are permanently linkable.
- The ephemeral public key is included in the transaction, which is required for scanning but also identifies the transaction as using stealth addressing (metadata leakage).

**Residual risk.** Low when stealth is used. High for transparent addresses.

---

### 6.3 Sender Deanonymization via Graph Analysis

**Description.** A passive observer analyzes parent link patterns, timing, and relay paths to identify the originating node of a transaction.

This is the primary open privacy problem. Even with amount and receiver privacy, the transaction graph reveals:
- Which tips a node chose as parents (reveals the node's local DAG view at submission time)
- When the transaction first appeared at each node (timing correlation)
- Which address sent the transaction (it is public)

**Attack vectors.**
1. **Parent set intersection.** An observer who watches multiple transactions from the same sender can intersect their parent sets. If the sender consistently picks the same tips, the intersection narrows to the sender's local DAG view, revealing network position.
2. **Timing correlation.** Even with relay delay, if a transaction appears at one node significantly earlier than others, that node is a likely origin.
3. **Decoy detection.** If decoy parents are older or have lower weight than real parents, they are distinguishable, reducing the privacy set.

**Current mitigation.**
- `GraphPrivacyAnalyzer`: measures parent entropy (Shannon entropy over parent weights), fan-out score, and timing exposure for each transaction. Flags vulnerable transactions.
- `IntersectionAttackDetector`: tracks parent set overlap (Jaccard similarity) and timing regularity (coefficient of variation) per address. Raises risk score when patterns are detectable.
- `ParentSelectionPolicy` with ε-noise: with probability ε, replaces a real parent with a decoy from the `DecoyPool`. Default ε=0.10. Decoy selection is weight-adaptive — decoys are preferentially sampled from entries with weights similar to real parents, making them harder to distinguish.
- Dandelion stem/fluff: ~20% of transactions enter a stem phase with 2× delay before broadcast.
- `DiffusionConfig`: 50–500ms relay delay, deterministic per tx_id.
- Empirical validation: at 50 trials, `origin_recovery_risk` decreases from 0.072 (ε=0.00) to 0.061 (ε=0.10) to 0.045 (ε=0.30), confirming decoy injection measurably reduces graph-level deanonymization risk.

**Gaps.**
- Sender address is public. All privacy mechanisms protect network origin, not on-chain identity.
- Decoy pool is bounded (50 entries) and contains only recent transactions. An adversary who knows the decoy pool contents (by observing recent DAG tips) can identify decoys.
- Stem phase does not implement true Dandelion routing (single relay chain). The current implementation applies delay locally; a network-level adversary still sees the originating IP.
- `IntersectionAttackDetector` computes risk scores and triggers `auto_adjust_privacy()` to increase ε and reduce β under high-risk conditions — but the adjustment is gradual and does not provide hard anonymity guarantees.
- No formal anonymity set size bound. The privacy guarantee is qualitative and empirical.

**Residual risk.** High for a passive network observer. Medium for an on-chain-only observer who cannot correlate timing.

---

### 6.4 Replay Attack

**Description.** An adversary re-submits a previously valid transaction.

**Current mitigation.**
- Nonces are strictly increasing per address. A replayed transaction has the same nonce as an already-applied transaction, which is rejected by the state machine (`can_apply()` checks for duplicate tx_id and correct nonce).
- `SeenSet` at the gossip layer deduplicates in-flight transactions.

**Residual risk.** Negligible.

---

## 7. Implementation-Layer Threats

### 7.1 Integer Overflow

**Description.** Arithmetic on balances or weights overflows, creating coins from nothing or causing underflow to large values.

**Current mitigation.**
- All balance arithmetic uses `saturating_sub` and `saturating_add`. Overflow silently caps rather than wrapping.
- Weight uses `u64`; at 1 transaction per millisecond for 584 years, weight would reach u64::MAX. Not a practical concern.

**Residual risk.** Low.

---

### 7.2 PoW Bypass

**Description.** An adversary submits transactions without valid PoW, bypassing the anti-spam mechanism.

**Current mitigation.**
- `validate_anti_spam_with_difficulty()` recomputes the hash and checks the prefix. A transaction with invalid PoW is rejected before entering the mempool.
- Hash is computed over the full transaction payload including nonce, preventing pre-computation attacks.

**Residual risk.** Negligible given SHA-256 preimage resistance.

---

### 7.3 State Inconsistency via Snapshot Corruption

**Description.** A corrupted or adversarially crafted snapshot causes a node to load invalid state on restart.

**Current mitigation.**
- Snapshots use atomic write (write to `.tmp`, then `rename`). A crash during write leaves the previous snapshot intact.
- `verify_synced_state()` checks incoming state against the latest trusted checkpoint root. A node loading a corrupted snapshot would fail root verification when syncing.

**Gaps.**
- Local snapshot is not independently signed. A compromised host could modify the snapshot file without detection until sync.
- No checkpoint chain validation on initial load (only on sync from peers).

**Residual risk.** Low for network attacks. Medium for compromised host.

---

### 7.4 Signature Malleability

**Description.** An adversary modifies a valid signature to produce a different valid signature for the same message, creating a second valid transaction ID.

**Current mitigation.**
- Ed25519 signatures are non-malleable by construction (RFC 8032).
- `tx_id` is computed over the full transaction including signature, so a modified signature produces a different tx_id.

**Residual risk.** Negligible.

---

### 7.5 Commitment Forgery (Inflation via Fake Excess)

**Description.** An adversary constructs a confidential transaction with a fake `excess_commitment` that passes validation but creates coins from nothing.

**Attack vector.** Submit a confidential transaction where `Σ C_outputs > Σ C_inputs` but supply a forged `excess_commitment` that makes the validator's balance check pass.

**Current mitigation.**
- `validate_balance_proof()` verifies that `Σ C_inputs - Σ C_outputs = excess_commitment` using the homomorphic property of Pedersen commitments. Forging this requires breaking the discrete log problem on Ristretto255.
- `validate_excess()` verifies that `excess_commitment` and `excess_signature` are present and structurally valid (valid hex, non-null).
- The three-step `validate_confidential_tx` enforces all three checks in sequence; a transaction failing any step is rejected.

**Gaps.**
- Without range proofs (Bulletproofs), a commitment can hide a negative amount. The balance proof prevents the sum from being wrong, but individual output commitments could commit to negative values that sum correctly. This is the negative-amount inflation vector.
- `PlaceholderRangeProof` is not production-safe (`is_production_safe() = false`). The validator allows `Experimental` status through, which means the range proof check is structurally present but not cryptographically binding.

**Residual risk.** Low for sum-level inflation (excess proof prevents it). Medium for negative-amount outputs until Bulletproofs replace the placeholder backend.

---

## 8. Threat Summary Table

| # | Threat | Layer | Severity | Mitigation Status |
|---|--------|-------|----------|-------------------|
| 1 | Eclipse attack | Network | High | Partial — detection without response |
| 2 | Sybil attack | Network | Medium | Partial — stake cost but no hard gate |
| 3 | DoS via flooding | Network | Medium | Partial — PoW + rate limit + 1MB cap |
| 4 | Timing correlation | Network | High | Partial — delay + Dandelion delay, not full routing |
| 5 | Parasite DAG | Consensus | Medium | Mitigated — σ-dominance + state root verification |
| 6 | Double-spend | Consensus | High | Mitigated — 5-state machine + PHA convergence |
| 7 | Stake grinding | Consensus | Low | Partial — 3× cap; no double-spend slashing |
| 8 | Partition / split-brain | Consensus | Medium | Mitigated for 2-way; uncharacterized multi-way |
| 9 | Amount linkability | Privacy | High | Mitigated (real Pedersen + excess kernel) — optional only |
| 10 | Receiver linkability | Privacy | High | Mitigated (stealth) — optional only |
| 11 | Graph deanonymization | Privacy | High | Partial — Phase 10 metrics + Dandelion delay + adaptive ε |
| 12 | Replay attack | Protocol | Low | Mitigated — nonce + SeenSet |
| 13 | Integer overflow | Implementation | Low | Mitigated — saturating arithmetic |
| 14 | PoW bypass | Implementation | Low | Mitigated — hash recomputation |
| 15 | Snapshot corruption | Implementation | Medium | Partial — atomic write; no chain validation on load |
| 16 | Signature malleability | Implementation | Low | Mitigated — Ed25519 non-malleable |
| 17 | Commitment forgery / inflation | Privacy | Medium | Partial — excess proof prevents sum inflation; negative outputs possible until Bulletproofs |

---

## 9. What Is Out of Scope

The following threats are acknowledged but not addressed in v0.1:

- **Key management.** Private key storage, hardware wallet integration, key rotation.
- **Transport security.** No TLS/QUIC. All WebSocket connections are plaintext. Network-level adversaries can read all gossip messages.
- **Supply chain.** Dependency integrity (Cargo.lock is committed; no reproducible build verification).
- **Cryptographic agility.** The protocol is hardcoded to Ed25519 + Ristretto255 + SHA-256. Post-quantum migration is not planned.
- **Regulatory compliance.** Privacy mechanisms may conflict with AML/KYC requirements in some jurisdictions.
- **Long-range attacks.** Formal analysis of attacks that build from an old checkpoint is not done.

---

## 10. Recommended Mitigations for Future Versions

Listed by priority:

**High priority.**
1. **Eclipse response.** On detection, trigger automatic peer rotation: drop 50% of same-subnet peers and attempt connections to diverse addresses from a bootstrap list.
2. **True Dandelion routing.** Implement stem phase as a relay chain: forward stem transactions to a single random peer rather than applying delay locally. This provides origin IP hiding rather than timing obfuscation only.
3. **Bulletproofs range proofs.** Replace `PlaceholderRangeProof` with a real Bulletproofs backend via `trait RangeProofSystem`. This closes the negative-amount inflation vector and makes range proof status `Verified` rather than `Experimental`. Blocked on `curve25519-dalek v3/v4` ecosystem conflict.

**Medium priority.**
4. **Double-spend slashing.** Add slashing for `ConflictingTx` violations in `StakingManager`. Nodes that submit conflicting transactions lose a fraction of stake, increasing the economic cost of double-spend attempts.
5. **Checkpoint chain on load.** On snapshot load, run `verify_chain()` against the loaded checkpoint registry to detect local corruption before syncing.
6. **Kernel sum validation (full).** Implement full `validate_kernel_sum()` using RistrettoPoint addition to verify `Σ excess_kernels = Σ C_inputs_all - Σ C_outputs_all` across the entire ledger. Currently only structural presence is checked.

**Low priority.**
7. **Minimum peer diversity.** When adding peers, reject connections that would push any /16 subnet above 60% of the peer list.
8. **Cover traffic.** Periodically broadcast dummy transactions with valid PoW from nodes with no real activity, to obscure the absence of real transactions.
9. **Light client support.** Incorporate checkpoint state roots into a proper finality chain (each checkpoint commits to the previous checkpoint's root) to enable trustless light client sync.

---

*GhostLedger Threat Model v0.2 — March 2026*  
*This document describes a research prototype. It is not a security audit.*
