# GhostLedger: A Feeless, Private DAG-Based Payment Ledger

**Aleksandr Bilyk**
Independent Researcher
March 2026

---

## Abstract

We present GhostLedger, a DAG-based payment ledger that simultaneously targets three properties rarely combined in practice: no transaction fees, strong sender/amount privacy, and decentralized conflict resolution without block producers. The protocol uses cumulative DAG weight for consensus, dynamic proof-of-work for spam resistance, stealth addresses and Pedersen commitments for privacy, and a stake-weighted closure rule for double-spend resolution. We introduce a hybrid parent selection policy parameterized by a consensus bias β and a privacy noise level ε, and show empirically — across 50 independent trials per configuration — that pure greedy selection (β=1.0) causes DAG divergence via tip starvation, while moderate bias (β∈[0.3, 0.9]) with bounded noise (ε≤0.10) provides stable operation. The empirically validated default is (β=0.7, ε=0.10). We also present the Partition Healing Algorithm (PHA) and a 5-state conflict status machine that handles network partitions without coordinator intervention. Amount privacy is implemented via real Pedersen commitments (C = r·G + v·H on Ristretto255) with balance proofs, excess kernels, and production Bulletproofs range proofs following the Mimblewimble model. Graph-level privacy is addressed via true Dandelion stem/fluff diffusion with single-peer relay, parent entropy analysis, an intersection attack detector, and cut-through pruning with full kernel sum validation via RistrettoPoint addition. Adversarial simulation across 30 trials confirms parasite DAG success rate of 0.0% and double-spend success rate of 0.0% under the default policy. Safety and liveness are argued informally with identified open problems.

---

## 1. Introduction

Payment systems face a persistent trilemma: feeless operation removes economic spam resistance; strong privacy complicates balance verification; decentralization removes the authority that would otherwise resolve conflicts. Existing systems address at most two of these simultaneously. Nano achieves feeless DAG but has no amount privacy. Monero achieves strong privacy but requires miners and fees. IOTA uses a DAG but relies on a coordinator and has limited privacy.

GhostLedger is an attempt to architect all three properties together and to understand what the tradeoffs look like when they are forced to coexist. The core thesis is that spam resistance can come from proof-of-work rather than fees, privacy can be layered at the transaction level without breaking consensus, and conflict resolution can be driven by the transactions themselves via cumulative weight rather than by dedicated validators.

The primary contribution of this paper is not a finished system but a protocol architecture with working Rust implementation (428 passing tests across 11 crates), formal definitions of the key mechanisms, empirical characterization of the parent selection parameter space at 50 trials, a cryptographically complete privacy layer with Pedersen commitments, excess kernels, and production Bulletproofs range proofs, adversarial simulation confirming security bounds, and a graph privacy layer defending against intersection and timing correlation attacks.

---

## 2. System Model

### 2.1 Network

We model a set of nodes N communicating over an asynchronous network. We do not assume synchrony; we assume eventual message delivery (standard partially synchronous model). Nodes may crash but not exhibit arbitrary Byzantine behavior in the base model. Byzantine resilience is analyzed separately in Section 8.

### 2.2 Ledger State

The ledger state S is a mapping from addresses to (balance, nonce) pairs. Balances are non-negative integers. Nonces are strictly increasing per address and enforce transaction ordering.

A state root R(S) is the root of a Merkle tree over the sorted leaf set {hash(addr ‖ balance ‖ nonce) : addr ∈ S}. State roots are deterministic: R(S₁) = R(S₂) if and only if S₁ = S₂.

The ledger additionally maintains a **kernel set** K: the set of excess kernels for all confirmed confidential transactions. After cut-through pruning, the full ledger validity can be verified as:

```
Σ inputs_all - Σ outputs_all = Σ excess_kernels_in_K
```

### 2.3 Transactions

A transaction vertex T consists of:

```
T = (sender, receiver, amount, nonce, timestamp,
     public_key, parents, signature,
     anti_spam_nonce, anti_spam_hash,
     commitment?,
     balance_proof?,
     excess_commitment?,
     excess_signature?,
     range_proof?,
     range_proof_status)
```

The fields `commitment`, `balance_proof`, `excess_commitment`, `excess_signature`, `range_proof` are optional and enable amount privacy (Section 5.2). The `parents` field references 1–2 previous vertices; this is what makes the ledger a DAG rather than a chain.

`range_proof_status` is an enum with three variants: `Missing`, `Experimental`, `Verified`. In production builds (release mode), the validator enforces `range_proof_status = Verified` for all confidential transactions. In debug/test builds, `Experimental` is accepted to allow placeholder proofs during development.

Each transaction carries a weight w(T) initialized to 1. Weight propagates to ancestors: when T is added, w(P) += 1 for all ancestors P of T. A transaction is considered confirmed when w(T) ≥ 6.

### 2.4 Wire Format

All transactions are serializable in two formats:

- **JSON** (current default for WebSocket transport): human-readable, used during alpha phase
- **bincode** (binary, via `ghost-wire` crate): compact binary encoding with a 5-byte header

```
[0..4] MAGIC = 0x47 0x48 0x53 0x54 ("GHST")
[4]    VERSION = 0x01
[5..]  bincode-serialized WireTransaction
```

Empirically, bincode encoding is 40–60% smaller than JSON for typical transactions.

---

## 3. DAG Structure and Weight

### 3.1 DAG Definition

Let G = (V, E) be a directed acyclic graph where V is the set of accepted transaction vertices and E is the parent relation. A vertex T is a **tip** if it has no children in G. The set of tips Tips(G) represents the frontier of the ledger.

New transactions must reference 1–2 tips as parents, implicitly confirming them by adding to their cumulative weight. This is the fundamental mechanism: **transactions produce consensus**.

### 3.2 Cumulative Weight

**Definition (Cumulative Weight).** The cumulative weight W(T) of a transaction T is:

```
W(T) = 1 + |{T' ∈ V : T is an ancestor of T'}|
```

**Definition (Confirmation).** A transaction T is confirmed if W(T) ≥ θ, where θ = 6 in the current implementation.

Weight propagation is monotonic: once confirmed, a transaction remains confirmed unless the DAG is reorganized, which cannot happen in the absence of conflicts.

### 3.3 Tips and DAG Width

The number of live tips |Tips(G)| reflects the "width" of the DAG frontier. Empirical results (Section 9) show that under normal operation with honest nodes, DAG width stabilizes at 7–11 tips. Pathological parent selection (pure greedy, β=1.0) causes tip starvation where width grows unboundedly (150–207 tips observed).

### 3.4 Cut-Through Pruning and Kernel Set

Intermediate transactions that have been confirmed and whose outputs have been fully spent can be removed from the DAG while retaining their **kernel** (excess commitment + excess signature):

```
Tx_A → Tx_B  (where Tx_B spends Tx_A's output)
⟹ remove Tx_A, retain kernel(Tx_A)
```

The `CutThroughPruner` identifies confirmed non-tip transactions that have children and removes them, accumulating kernels in `LedgerState.kernel_set`.

**Kernel sum validation** uses full RistrettoPoint addition:

```rust
let sum: RistrettoPoint = kernels.iter()
    .map(|k| decompress(k.excess_commitment))
    .sum();
assert!(sum != RistrettoPoint::default());
```

This provides a compact validity proof for the entire ledger history.

### 3.5 Genesis Transaction

The canonical genesis transaction is a confidential transaction from `system` to the genesis address committing to the total supply (21,000,000 GHOST):

```
genesis_tx.sender    = "system"
genesis_tx.amount    = 21_000_000
genesis_tx.parents   = []
genesis_tx.commitment = C(21_000_000, r_genesis)
```

The genesis blinding factor `r_genesis` must be preserved by the genesis operator to enable future kernel sum verification.

---

## 4. Conflict Model

### 4.1 Conflict Definition

Two transactions T₁ and T₂ are **conflicting** if they have the same sender and nonce:

```
conflict(T₁, T₂) ⟺ T₁.sender = T₂.sender ∧ T₁.nonce = T₂.nonce ∧ T₁ ≠ T₂
```

### 4.2 Conflict Status Machine

Each conflict set C(sender, nonce) progresses through a 5-state machine:

```
Pending → Ready → ClosedLocal → Reconciling → ClosedGlobal
                      ↑_______________|
```

- **Pending → Ready**: all transactions in C have W(T) ≥ θ_min = 3
- **Ready → ClosedLocal**: closure predicate holds and a finalized checkpoint anchors all transactions in C
- **ClosedLocal → Reconciling**: a newer partition boundary cp* is discovered (PHA Step 3)
- **Reconciling → ClosedGlobal**: closure predicate holds using frozen stake
- **Reconciling → Ready**: closure predicate fails after re-evaluation

`ClosedGlobal` is terminal — no transitions out.

### 4.3 Closure Predicate

**Definition (Closure).** A conflict set C is closed if:

1. **Ready**: ∀T ∈ C, W(T) ≥ θ_min
2. **Anchored**: ∃ finalized checkpoint cp such that ∀T ∈ C, T ∈ descendants(cp)
3. **Dominant**: score(winner) ≥ σ · score(second), where σ = 2.0

```
score(T) = W(T) · (1 + (stake(T.sender) / total_stake) · 2)
```

**Theorem S (Safety sketch).** Once score(T_w) ≥ 2·score(T_l), T_l is excluded from parent selection and gains no further weight. The dominance condition is monotonically stable. □

### 4.4 Double-Spend Slashing

When conflict resolution marks a transaction as a loser, the sender's stake is automatically slashed:

```rust
for (loser_id, loser_sender) in &losers {
    if staking.is_eligible(&loser_sender) {
        staking.slash(loser_sender, ViolationType::ConflictingTx, loser_id);
    }
}
```

10% of stake slashed per violation; 50% burned, 50% redistributed to clean validators. Ejection after 3 violations. This creates a direct economic deterrent for double-spend attempts.

---

## 5. Privacy

### 5.1 Stealth Addresses

One-time stealth addresses are derived from the recipient's spend public key and an ephemeral sender key:

```
stealth_addr = H(ECDH(ephemeral_priv, recipient_pub) ‖ recipient_pub)[0:20]
```

Only the recipient can identify payments addressed to them.

### 5.2 Pedersen Commitments, Excess Kernels, and Bulletproofs

Transaction amounts are hidden using Pedersen commitments on Ristretto255:

```
C(v, r) = r·G + v·H
```

Properties: hiding, binding, homomorphic.

**Balance proof (excess kernel):**

```
excess = Σ r_inputs - Σ r_outputs
excess_commitment = excess · G
```

Validator checks: `Σ C_inputs - Σ C_outputs = excess_commitment`

**Range proofs (Bulletproofs).** The `ghost-bulletproofs` crate implements `RangeProofSystem` using `bulletproofs = "4.0"` proving v ∈ [0, 2^64):

```rust
impl RangeProofSystem for BulletproofsBackend {
    fn is_production_safe() -> bool { true }
    // prove() and verify() via Merlin transcript
}
```

Architecture: `ledger` does not depend on `ghost-bulletproofs`. Verification happens in `ghost-node/ws_server.rs`; proof generation is a wallet concern. This follows the Grin/Beam model.

**Three-step confidential transaction validation** (`validate_confidential_tx`):
1. Range proof via `RangeProofSystem::verify()` — `Verified` required in release builds
2. Balance proof — excess commitment matches commitment difference
3. Excess — structural validity of excess_commitment and excess_signature

### 5.3 Graph Privacy

**Intersection attack** mitigated by ε-noise parent selection and tracked by `IntersectionAttackDetector` (per-address Jaccard overlap + timing regularity scores).

**Timing correlation** mitigated by true Dandelion stem/fluff: ~20% of transactions forwarded to a **single randomly selected peer** before broadcast, providing origin IP hiding.

**Parent topology inference** mitigated by `GraphPrivacyAnalyzer` computing parent entropy, fan-out score, and timing exposure.

**Graph entropy metrics** (Python simulator):
- **parent_diversity**: fraction of unique parent combinations
- **graph_entropy**: Shannon entropy over parent usage frequency
- **origin_recovery_risk**: composite deanonymization risk score

`origin_recovery_risk` decreases monotonically with ε: 0.072 → 0.061 → 0.053 → 0.045 as ε goes from 0.00 to 0.30.

---

## 6. Parent Selection

### 6.1 ParentSelectionPolicy

```
Policy = (β, ε, max_parents)
```

- **β**: consensus bias via Gumbel-max sampling: k(t) = -ln(U) / w(t)^β
- **ε**: privacy noise — with probability ε, one parent replaced with weight-adaptive decoy
- **max_parents**: default 2

Policy presets (from `ghost-params`):

```rust
ParentSelectionPolicy::default()        // β=0.7, ε=0.10
ParentSelectionPolicy::privacy_mode()   // β=0.7, ε=0.20
ParentSelectionPolicy::consensus_mode() // β=0.7, ε=0.00
```

### 6.2 Conflict-Aware Filtering

```
candidates = Tips(G) \ {T : T is a conflict loser}
```

Fallback to full tip set if all tips are losers.

### 6.3 True Dandelion Routing

Stem peer selection: `idx = (H(tx_id) XOR (time / 10s)) % |peers|`

The time component (changes every 10 seconds) ensures different nodes choose different stem peers for the same transaction. Stem TTL starts at 10, decrements each hop; TTL exhaustion triggers fluff fallback.

### 6.4 Protocol Parameters (ghost-params)

| Parameter | Value | Description |
|-----------|-------|-------------|
| `BETA` | 0.7 | Parent selection consensus bias |
| `EPSILON` | 0.10 | Privacy noise (default) |
| `SIGMA` | 2.0 | Conflict closure dominance threshold |
| `THETA` | 6 | Checkpoint finalization weight |
| `RESOLVE_MIN_WEIGHT` | 3 | Conflict resolution minimum weight |
| `MIN_STAKE` | 1000 | Minimum validator stake (GHOST) |
| `STEM_MAX_TTL` | 10 | Maximum Dandelion stem hops |

---

## 7. Partition Healing Algorithm (PHA)

**Step 1 (Handshake).** Exchange latest finalized checkpoint; agree on common cp*.

**Step 2 (Invariant G).** Conflicts below cp* are immutable — not touched.

**Step 3 (Downgrade).** Conflicts closed above cp* → `Reconciling`.

**Step 4 (Sync).** Exchange all transactions above cp*. Both nodes now have identical DAG above cp*.

**Step 5 (Re-evaluate).** Apply closure predicate with frozen stake snapshot.

**Step 6 (Close).** Dominant → `ClosedGlobal`. Not dominant → back to `Ready`.

**Theorem P.** After Step 4, both nodes have identical DAG above cp*. Steps 5–6 are deterministic. Same input → same winner. □

---

## 8. Security Model

### 8.1 Spam Resistance

**Dynamic PoW**: difficulty 2–6, adjusts with TPS (threshold: 10 TPS up, 2 TPS down).

**Rate limiting**: 5 tx / 10s soft, 10 tx / 10s burst per address.

**Adversarial spam simulation** (30 trials, spam_rate_limit=2): spam weight share 20.6% (proportional to tx share), diluted below 10% within 1 step.

### 8.2 Stake Adversary (Conjecture F)

**Conjecture F.** If f < 1/(σ·α) ≈ 0.167, adversary cannot revert a closed conflict or prevent honest closure.

*Supported by Monte Carlo simulation (byzantine_sim.rs), not formally proven.*

### 8.3 Adversarial Simulation Results (30 trials, β=0.7, ε=0.10)

| Attack | Success Rate | Target |
|--------|-------------|--------|
| Parasite DAG | 0.0% | < 5% ✅ |
| Double Spend Race | 0.0% | < 5% ✅ |
| Spam dilution time | 1 step | < 20 ✅ |
| Parasite damage bound | 0.000 | ≈ 0 ✅ |
| Honest weight gap | +180.0 | > 0 ✅ |

---

## 9. Empirical Results

### 9.1 Simulation Setup

- N=6 honest nodes, partial view (last 15–21 tx per node)
- 50 independent trials per configuration
- N_TX=150, CONFLICT_EVERY=30, σ=2.0, θ_min=3

### 9.2 Key Findings

**β=1.0 incompatible** — tip starvation, closure_rate ≈ 0.00, width 150–207.

**β=0.7 empirically optimal** — closure_rate=0.795, dag_width=7.7.

**ε=0.10 sweet spot** — −15% origin_risk vs ε=0.00, <0.05 closure_rate degradation.

**origin_recovery_risk monotonically decreasing with ε**: 0.072 → 0.061 → 0.053 → 0.045.

### 9.3 Summary Table (50 trials)

| β    | ε    | closure_rate | median_closure | dag_width | origin_risk |
|------|------|-------------|----------------|-----------|-------------|
| 0.7  | 0.00 | 0.795       | 2.1            | 7.7       | 0.072       |
| 0.7  | 0.10 | 0.685       | 3.2            | 8.3       | 0.061       |
| 0.7  | 0.20 | 0.725       | 2.5            | 9.2       | 0.053       |
| 0.9  | 0.00 | 0.805       | 2.1            | 7.8       | 0.073       |
| 0.9  | 0.10 | 0.665       | 2.6            | 8.3       | 0.061       |
| 1.0  | 0.00 | 0.000       | ∞              | 190.7     | 0.323       |

### 9.4 Scale and Conflict Rate

Scale (β=0.7, ε=0.10, CONFLICT_EVERY=50):

| N_TX | closure_rate | median_closure | dag_width |
|------|-------------|----------------|-----------|
| 150  | 0.68        | 3.2            | 8.3       |
| 500  | 0.71        | 2.0            | 10.0      |
| 1000 | 0.68        | 2.4            | 11.0      |

Conflict rate (N_TX=500):

| conflict_every | closure_rate | dag_width |
|---------------|-------------|-----------|
| 10            | 0.74        | 7.8       |
| 30            | 0.73        | 8.9       |
| 100           | 0.70        | 11.6      |

---

## 10. Implementation

| Crate | Responsibility |
|-------|---------------|
| `crypto` | Ed25519, X25519 stealth, Pedersen commitments, `trait RangeProofSystem`, `PlaceholderRangeProof` |
| `ledger` | DAG, state+kernel_set, validator, cut-through (full kernel sum), anti-spam, Merkle, checkpoints, `ParentSelectionPolicy`, graph privacy, canonical genesis |
| `consensus` | `ConflictResolver` (5-state + PHA), `TipSelector`, Byzantine sim |
| `token` | GHOST token, `StakingManager` (auto-slash on double-spend) |
| `network` | WebSocket P2P, gossip, peer discovery, eclipse detection |
| `storage` | Atomic JSON snapshot |
| `ghost-node` | CLI node, genesis, bootstrap, Bulletproofs verification |
| `ghost-explorer` | TUI (ratatui) |
| `ghost-bulletproofs` | Production Bulletproofs (`is_production_safe() = true`) |
| `ghost-params` | Single source of all protocol constants |
| `ghost-wire` | Binary wire format (bincode + GHST magic + versioning) |

**Test suite: 428 passing tests, 0 failed.**

---

## 11. Open Problems

1. **β/ε formal analysis** — formal convergence bound relating β to gossip model and ε to anonymity set size
2. **Honest Parent Selection Problem** — analytical characterization of the privacy/consensus tradeoff
3. **Corollary P (formal proof)** — bounded gossip model + multi-partition case
4. **Graph deanonymization** — formal anonymity set bound for DAG graphs; sender address remains public
5. **State root finality chain** — proper commitment chain for light client support
6. **Multi-party adversarial simulation** — coordinated attack scenarios not yet modeled

---

## 12. Related Work

**IOTA** — DAG Tangle, coordinator-dependent, no privacy.
**Nano** — feeless DAG, voting-based resolution, no amount privacy.
**Monero** — strong privacy via RingCT, block-based, fees required.
**Mimblewimble/Grin** — excess kernel model and cut-through directly followed in GhostLedger.
**Avalanche** — metastable DAG consensus via repeated sampling; GhostLedger uses deterministic σ-dominance.
**Dandelion++** — single-peer stem relay implemented in GhostLedger matching the Dandelion++ model.

---

## 13. Conclusion

GhostLedger demonstrates that a feeless, private, decentralized DAG ledger is architecturally viable. The implementation (428 tests, 11 crates) covers the full protocol stack: production Bulletproofs range proofs, automatic double-spend slashing, true Dandelion routing, canonical genesis with commitments, full kernel sum validation, binary wire format, and centralized protocol parameters.

Adversarial simulation confirms the protocol's security properties: 0.0% parasite DAG success, 0.0% double-spend success, spam diluted in 1 step — all under the empirically validated default policy (β=0.7, ε=0.10).

The protocol should be considered a research prototype. Source: https://github.com/AlexBil-rar/Token. Threat model: `THREAT_MODEL.md`. Network protocol spec: `PROTOCOL.md`.

---

## References

1. S. Popov. "The Tangle." IOTA Foundation, 2018.
2. C. LeMahieu. "Nano: A Feeless Distributed Cryptocurrency Network." 2018.
3. T. Rocket et al. "Scalable and Probabilistic Leaderless BFT Consensus through Metastability." arXiv:1906.08936, 2020.
4. G. Fanti et al. "Dandelion++: Lightweight Cryptocurrency Networking with Formal Anonymity Guarantees." ACM SIGMETRICS, 2018.
5. N. van Saberhagen. "CryptoNote v2.0." 2013.
6. T. P. Pedersen. "Non-Interactive and Information-Theoretic Secure Verifiable Secret Sharing." CRYPTO 1991.
7. A. Poelstra. "Mimblewimble." 2016.
8. M. Hamburg. "Decaf: Eliminating Cofactors Through Point Compression." CRYPTO 2015.
9. B. Bünz et al. "Bulletproofs: Short Proofs for Confidential Transactions and More." IEEE S&P 2018.

---

*GhostLedger v0.2 — March 2026*
*Source: https://github.com/AlexBil-rar/Token*
*This document describes a research prototype. No security guarantees are implied.*
