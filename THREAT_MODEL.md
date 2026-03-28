# GhostLedger Threat Model

**Version:** 0.3
**Date:** March 2026
**Author:** Aleksandr Bilyk
**Status:** Research prototype — no security guarantees implied

---

## 1. Scope

This document describes the threat model for GhostLedger v0.2: a feeless, private, DAG-based payment ledger. It covers the attack surface across four layers — network, consensus, privacy, and implementation — and maps each threat to the current mitigation status.

This is not a formal security proof. Where mitigations are partial or absent, that is stated explicitly.

---

## 2. Trust Assumptions

### 2.1 What we assume

- **Honest majority by stake.** The protocol assumes that adversarial nodes control less than 1/6 of total staked weight (Conjecture F). This is more conservative than the standard 1/3 Byzantine bound, justified by the stake-amplification factor α=3.0 in the conflict resolver.
- **Eventual message delivery.** The network model is partially synchronous. Messages are eventually delivered but may be arbitrarily delayed.
- **Honest genesis.** The genesis state is trusted. The genesis blinding factor must be preserved by the genesis operator for kernel sum verification.
- **No key compromise.** Ed25519 signing keys are assumed secure. Private key management is out of scope.

### 2.2 What we do not assume

- Synchronous message delivery.
- Honest behavior from nodes that do not hold stake.
- Privacy of the network layer (IP addresses, connection topology).
- Security of the host operating system or hardware.

---

## 3. Attacker Model

**Class A — Rational adversary.** Wants to double-spend or gain economic advantage. Bounded stake fraction f < 1/6. Subject to automatic slashing on detected double-spend attempts.

**Class B — Disruptive adversary.** Wants to degrade protocol availability or deanonymize users. Bounded by network resource constraints and PoW cost.

**Class C — Passive observer.** Sees all network traffic and all on-chain data but cannot break cryptographic primitives. Primary threat to sender privacy.

---

## 4. Network-Layer Threats

### 4.1 Eclipse Attack

**Description.** Adversary monopolizes all peer connections of a target node, isolating it from the honest network.

**Current mitigation.**
- `PeerList::check_eclipse()` detects when ≥80% of peers share the same /16 subnet (among ≥10 peers) and logs a warning.
- Peer list capped at 128 entries.
- Random gossip sampling (size 8) distributes messages across diverse peers.

**Gaps.**
- Detection triggers warning only — no automatic peer rotation.
- No minimum diversity requirement enforced when adding peers.
- No transport-level authentication.

**Residual risk.** Medium. Detection exists; response does not.

---

### 4.2 Sybil Attack

**Description.** Adversary creates many fake identities to disproportionately influence peer discovery or consensus weight.

**Current mitigation.**
- Consensus weight is stake-weighted. Zero-stake nodes contribute multiplier 1.0.
- Staking requires locking GHOST tokens — real economic cost per identity.
- Eclipse detection limits subnet concentration.

**Gaps.**
- Zero-stake nodes can still participate in gossip and submit transactions.
- No identity binding between network address and stake record.

**Residual risk.** Low for consensus, medium for gossip flooding.

---

### 4.3 DoS via Message Flooding

**Description.** Adversary sends high volume of messages to exhaust target node resources.

**Current mitigation.**
- Dynamic PoW (difficulty 2–6, auto-adjusting with TPS).
- Per-address rate limiting: 5 tx / 10s soft, 10 tx / 10s burst.
- `SeenSet` deduplicates gossiped transactions (10,000 entries, LRU eviction).
- WebSocket message size cap: 1MB.

**Adversarial simulation result.** With spam_rate_limit=2 (simulating PoW cost), spam weight share is 20.6% of total graph weight (proportional to 25% tx share), diluted below 10% within 1 step. The spam weight share is bounded by the attacker's tx share, not unbounded.

**Gaps.**
- Dynamic PoW penalizes all senders during an attack.
- SeenSet eviction under sustained flood may cause re-processing of seen transactions.

**Residual risk.** Medium. PoW provides meaningful friction; spam is self-diluting.

---

### 4.4 Timing Correlation Attack

**Description.** Passive observer correlates transaction arrival times across nodes to identify the originating node.

**Current mitigation.**
- Deterministic random relay delay (50–500ms) per transaction.
- **True Dandelion routing**: ~20% of transactions enter stem phase, forwarded to a **single randomly selected peer** (not broadcast). Stem peer selection uses `tx_id` entropy XOR time component (rotates every 10 seconds), ensuring different nodes choose different stem peers.
- Stem TTL: max 10 hops, then fluff fallback.

**Gaps.**
- Stem phase provides origin IP hiding, not full anonymity. A global passive adversary observing all network links can still perform timing analysis over stem hops.
- No cover traffic or dummy transactions.
- Delay is deterministic per tx_id — adversary knowing the tx_id can predict the relay delay.

**Residual risk.** Medium. True Dandelion routing significantly reduces timing correlation vs. broadcast-only, but a powerful passive adversary retains capability.

---

### 4.5 Parasite DAG Attack

**Description.** Adversary builds a private DAG branch from an old anchor and releases it to attempt a double-spend.

**Current mitigation.**
- σ-dominance closure: winner must score ≥ 2× second. Parasite must accumulate 2× the honest weight.
- Stake-weighted scoring: adversary with f < 1/6 faces a 7:1 uphill battle.
- State root anchoring: syncing nodes verify incoming state against trusted checkpoint root.

**Adversarial simulation result (30 trials).** Parasite accepted: 0/30 (0.0%). Avg weight ratio (parasite/honest): 0.003. Parasite tips surviving: 0.000.

**Residual risk.** Low. Empirically 0% success rate under default policy.

---

## 5. Consensus-Layer Threats

### 5.1 Double-Spend Attack

**Description.** Sender submits two conflicting transactions T₁ and T₂ (same sender, same nonce) to different network partitions.

**Current mitigation.**
- 5-state conflict machine with σ=2.0 dominance threshold.
- Partition Healing Algorithm (PHA) ensures global consensus on winner after partition heals.
- **Automatic slashing**: when T_loser is resolved, sender's stake is slashed via `ViolationType::ConflictingTx`. 10% per violation, ejection after 3.
- `SeenSet` deduplicates in-flight transactions.

**Adversarial simulation result (30 trials).** Attacker won: 0/30 (0.0%). Avg weight gap (honest − attacker): 180.0.

**Residual risk.** Negligible. 0% success rate; economic deterrent via auto-slashing.

---

### 5.2 Stake Grinding

**Description.** Validator repeatedly resamples transaction timing or parameters to maximize stake-weighted score for their own transactions.

**Current mitigation.**
- Stake multiplier capped at 3× regardless of stake fraction.
- Score is W(T) · multiplier — weight must be accumulated via honest transaction referencing, not just stake.

**Gaps.**
- No explicit grinding detection.
- A validator with large stake can still derive disproportionate benefit from timing their transactions optimally.

**Residual risk.** Low. 3× cap limits amplification.

---

### 5.3 Network Partition / Split-Brain

**Description.** Network splits into two or more components that resolve the same conflicts differently, leading to inconsistent state when the partition heals.

**Current mitigation.**
- PHA handles 2-way partitions deterministically (Theorem P).
- Frozen stake prevents stake manipulation across partition boundary.
- Invariant G: below-boundary conflicts are immutable.

**Gaps.**
- Multi-way partition (>2 components simultaneously) is not formally analyzed. PHA is defined for pairwise reconciliation; cascading reconciliations may not preserve Theorem P in the multi-way case.

**Residual risk.** Low for 2-way partitions (mitigated). Medium for multi-way (uncharacterized).

---

## 6. Privacy-Layer Threats

### 6.1 Amount Linkability

**Description.** Observer learns transaction amounts by reading on-chain data.

**Current mitigation.**
- Pedersen commitments `C = r·G + v·H` hide amounts.
- Balance proof (excess kernel) proves conservation without revealing values.
- Bulletproofs range proofs prove v ∈ [0, 2^64) without revealing v.

**Status.** Mitigated when privacy mode is used. Transparent transactions (no commitment) remain linkable — privacy is opt-in.

**Residual risk.** Low for confidential transactions. Medium for transparent transactions (user choice).

---

### 6.2 Receiver Linkability

**Description.** Observer links multiple payments to the same receiver by observing repeated addresses.

**Current mitigation.**
- X25519 stealth addresses: each payment generates a unique one-time address.
- Only the recipient can identify their payments via ECDH scan.

**Residual risk.** Low when stealth addresses are used (opt-in).

---

### 6.3 Sender Linkability via Graph Analysis

**Description.** Observer infers the sender's network position by analyzing parent selection patterns, timing, and relay paths.

**Current mitigation.**
- ε-noise parent selection (decoy injection).
- True Dandelion stem/fluff routing.
- `IntersectionAttackDetector`: per-address Jaccard overlap and timing regularity scores.
- `GraphPrivacyAnalyzer`: parent entropy, fan-out score, timing exposure.
- Adaptive ε: node automatically increases ε when high intersection risk is detected.

**Empirical bounds (50 trials).**

| ε | origin_recovery_risk |
|---|---------------------|
| 0.00 | 0.072 |
| 0.10 | 0.061 |
| 0.20 | 0.053 |
| 0.30 | 0.045 |

**Gaps.**
- Sender address is **public on-chain**. This is the dominant linkability vector and is not mitigated at the protocol level.
- Decoy pools are bounded (50 entries). Small pools reduce anonymity set size.
- No formal anonymity set bound for DAG graphs.
- A global passive adversary with full network observation retains meaningful deanonymization capability.

**Residual risk.** High for sender address linkability (structural). Medium for graph-level linkability (partially mitigated by decoys + Dandelion).

---

### 6.4 Negative-Amount Inflation (Range Proof Attack)

**Description.** Adversary constructs a confidential transaction committing to a negative amount that satisfies the balance proof sum but creates coins from nothing.

**Current mitigation.**
- Bulletproofs range proofs (`ghost-bulletproofs` crate) prove v ∈ [0, 2^64) for each output commitment.
- `is_production_safe() = true` for `BulletproofsBackend`.
- In release builds, `RangeProofStatus::Verified` is required — `Experimental` is rejected.
- Validator enforces all three steps in sequence via `validate_confidential_tx`.

**Status.** Mitigated. The Bulletproofs backend is production-safe and enforced in release builds.

**Residual risk.** Low. Bulletproofs are cryptographically binding on Ristretto255.

---

### 6.5 Commitment Forgery (Balance Inflation)

**Description.** Adversary constructs `excess_commitment` such that the balance proof passes but `Σ outputs > Σ inputs`.

**Current mitigation.**
- `validate_balance_proof()` verifies `Σ C_inputs - Σ C_outputs = excess_commitment` using homomorphic Pedersen property. Forgery requires breaking discrete log on Ristretto255.
- `validate_excess()` verifies structural validity of excess fields.

**Residual risk.** Negligible (discrete log hardness).

---

## 7. Implementation-Layer Threats

### 7.1 Integer Overflow

All balance arithmetic uses `saturating_sub` and `saturating_add`. **Residual risk: Low.**

### 7.2 PoW Bypass

`validate_anti_spam_with_difficulty()` recomputes hash and checks prefix. Hash covers full payload. **Residual risk: Negligible.**

### 7.3 State Inconsistency via Snapshot Corruption

Atomic write (`.tmp` → `rename`). `verify_synced_state()` checks against latest checkpoint root on sync.

**Gaps.** Local snapshot not independently signed. No checkpoint chain validation on initial load.

**Residual risk.** Low for network attacks. Medium for compromised host.

### 7.4 Signature Malleability

Ed25519 is non-malleable (RFC 8032). `tx_id` computed over full transaction including signature. **Residual risk: Negligible.**

### 7.5 Wire Format Tampering

`ghost-wire` validates GHST magic bytes and version on decode. Bad magic or unsupported version → immediate rejection. **Residual risk: Negligible.**

---

## 8. Threat Summary Table

| # | Threat | Layer | Severity | Status |
|---|--------|-------|----------|--------|
| 1 | Eclipse attack | Network | High | Partial — detection without automatic response |
| 2 | Sybil attack | Network | Medium | Partial — stake cost, no hard gate for gossip |
| 3 | DoS flooding | Network | Medium | Mitigated — PoW + rate limit + 1MB cap + self-diluting spam |
| 4 | Timing correlation | Network | High | Partial — true Dandelion routing; no cover traffic |
| 5 | Parasite DAG | Consensus | Medium | **Mitigated** — 0.0% success in 30-trial sim |
| 6 | Double-spend | Consensus | High | **Mitigated** — 0.0% success + auto-slashing |
| 7 | Stake grinding | Consensus | Low | Partial — 3× multiplier cap |
| 8 | Partition / split-brain | Consensus | Medium | Mitigated (2-way); uncharacterized (multi-way) |
| 9 | Amount linkability | Privacy | High | Mitigated (Bulletproofs + commitments) — opt-in |
| 10 | Receiver linkability | Privacy | High | Mitigated (stealth) — opt-in |
| 11 | Sender linkability (address) | Privacy | High | **Not mitigated** — sender address is public |
| 12 | Graph deanonymization | Privacy | High | Partial — decoys + Dandelion + IntersectionDetector |
| 13 | Negative-amount inflation | Privacy | High | **Mitigated** — Bulletproofs enforced in release builds |
| 14 | Commitment forgery | Privacy | Medium | Mitigated — discrete log hardness |
| 15 | Replay attack | Protocol | Low | Mitigated — nonce + SeenSet |
| 16 | Integer overflow | Implementation | Low | Mitigated — saturating arithmetic |
| 17 | PoW bypass | Implementation | Low | Mitigated — hash recomputation |
| 18 | Snapshot corruption | Implementation | Medium | Partial — atomic write; no chain validation on load |
| 19 | Signature malleability | Implementation | Low | Mitigated — Ed25519 non-malleable |
| 20 | Wire format tampering | Implementation | Low | Mitigated — magic bytes + version check |

---

## 9. What Is Out of Scope

- **Key management.** Private key storage, hardware wallet integration, key rotation.
- **Transport security.** No TLS/QUIC. All WebSocket connections are plaintext.
- **Supply chain.** No reproducible build verification beyond Cargo.lock.
- **Cryptographic agility.** Protocol is hardcoded to Ed25519 + Ristretto255 + SHA-256. No post-quantum migration.
- **Regulatory compliance.** Privacy mechanisms may conflict with AML/KYC requirements.
- **Long-range attacks.** Formal analysis of attacks building from old checkpoints is not done.
- **Multi-party coordinated attacks.** Adversarial simulation covers single-adversary scenarios only.

---

## 10. Recommended Mitigations for Future Versions

**High priority.**
1. **Eclipse response.** On detection, trigger automatic peer rotation: drop 50% of same-subnet peers and connect to bootstrap-list addresses.
2. **Sender address privacy.** Investigate ring signatures or zero-knowledge proofs for sender anonymity at the protocol level. Currently the dominant open privacy gap.
3. **Multi-partition PHA.** Formally analyze and test PHA under simultaneous 3-way partitions.

**Medium priority.**
4. **Checkpoint chain on load.** Run `verify_chain()` against loaded checkpoint registry on snapshot load to detect local corruption.
5. **Cover traffic.** Periodically broadcast dummy transactions from idle nodes to obscure absence of real activity.
6. **Transport security.** Add TLS or QUIC for WebSocket connections to prevent network-layer passive observation.

**Low priority.**
7. **Minimum peer diversity.** Reject connections that would push any /16 subnet above 60% of peer list.
8. **Light client support.** Incorporate checkpoint roots into a proper finality chain (each checkpoint commits to previous checkpoint root).
9. **Formal anonymity set bound.** Derive a closed-form bound for the anonymity set size as a function of ε, decoy pool size, and DAG topology.

---

*GhostLedger Threat Model v0.3 — March 2026*
*This document describes a research prototype. It is not a security audit.*
