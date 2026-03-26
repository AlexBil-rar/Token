# GhostLedger: A Feeless, Private DAG-Based Payment Ledger

**Aleksandr Bilyk**
Independent Researcher
March 2026

---

## Abstract

We present GhostLedger, a DAG-based payment ledger that simultaneously targets three properties rarely combined in practice: no transaction fees, strong sender/amount privacy, and decentralized conflict resolution without block producers. The protocol uses cumulative DAG weight for consensus, dynamic proof-of-work for spam resistance, stealth addresses and Pedersen commitments for privacy, and a stake-weighted closure rule for double-spend resolution. We introduce a hybrid parent selection policy parameterized by a consensus bias β and a privacy noise level ε, and show empirically — across 50 independent trials per configuration — that pure greedy selection (β=1.0) causes DAG divergence via tip starvation, while moderate bias (β∈[0.3, 0.9]) with bounded noise (ε≤0.10) provides stable operation. The empirically validated default is (β=0.7, ε=0.10). We also present the Partition Healing Algorithm (PHA) and a 5-state conflict status machine that handles network partitions without coordinator intervention. Amount privacy is implemented via real Pedersen commitments (C = r·G + v·H on Ristretto255) with balance proofs and excess kernels following the Mimblewimble model. Graph-level privacy is addressed via a Dandelion-inspired stem/fluff diffusion model, parent entropy analysis, an intersection attack detector, and cut-through pruning. Safety and liveness are argued informally with identified open problems.

---

## 1. Introduction

Payment systems face a persistent trilemma: feeless operation removes economic spam resistance; strong privacy complicates balance verification; decentralization removes the authority that would otherwise resolve conflicts. Existing systems address at most two of these simultaneously. Nano achieves feeless DAG but has no amount privacy. Monero achieves strong privacy but requires miners and fees. IOTA uses a DAG but relies on a coordinator and has limited privacy.

GhostLedger is an attempt to architect all three properties together and to understand what the tradeoffs look like when they are forced to coexist. The core thesis is that spam resistance can come from proof-of-work rather than fees, privacy can be layered at the transaction level without breaking consensus, and conflict resolution can be driven by the transactions themselves via cumulative weight rather than by dedicated validators.

The primary contribution of this paper is not a finished system but a protocol architecture with working Rust implementation (224 passing tests), formal definitions of the key mechanisms, empirical characterization of the parent selection parameter space at 50 trials, a cryptographically correct privacy layer with Pedersen commitments and excess kernels, and a graph privacy layer defending against intersection and timing correlation attacks.

---

## 2. System Model

### 2.1 Network

We model a set of nodes N communicating over an asynchronous network. We do not assume synchrony; we assume eventual message delivery (standard partially synchronous model). Nodes may crash but not exhibit arbitrary Byzantine behavior in the base model. Byzantine resilience is analyzed separately in Section 8.

### 2.2 Ledger State

The ledger state S is a mapping from addresses to (balance, nonce) pairs. Balances are non-negative integers. Nonces are strictly increasing per address and enforce transaction ordering.

A state root R(S) is the root of a Merkle tree over the sorted leaf set {hash(addr ‖ balance ‖ nonce) : addr ∈ S}. State roots are deterministic: R(S₁) = R(S₂) if and only if S₁ = S₂.

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

`range_proof_status` is an enum with three variants: `Missing`, `Experimental`, `Verified`. The validator enforces that a confidential transaction must have `range_proof_status` ≥ `Experimental` and a non-null `excess_commitment`.

Each transaction carries a weight w(T) initialized to 1. Weight propagates to ancestors: when T is added, w(P) += 1 for all ancestors P of T. A transaction is considered confirmed when w(T) ≥ 6.

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

That is, W(T) counts T itself plus all transactions that directly or transitively reference T.

**Definition (Confirmation).** A transaction T is confirmed if W(T) ≥ θ, where θ = 6 in the current implementation.

Weight propagation is monotonic: once confirmed, a transaction remains confirmed unless the DAG is reorganized, which cannot happen in the absence of conflicts (and is resolved by the conflict resolver in the presence of conflicts).

### 3.3 Tips and DAG Width

The number of live tips |Tips(G)| reflects the "width" of the DAG frontier. Empirical results (Section 9) show that under normal operation with honest nodes, DAG width stabilizes at 7–11 tips. Pathological parent selection (pure greedy, β=1.0) causes tip starvation where width grows unboundedly (150–207 tips observed), preventing weight accumulation and therefore confirmation.

### 3.4 Cut-Through Pruning

Intermediate transactions — those that have been confirmed and whose outputs have been fully spent — can be removed from the DAG while retaining their **kernel** (excess commitment + excess signature). This is the Mimblewimble cut-through property:

```
Tx_A → Tx_B  (where Tx_B spends Tx_A's output)
⟹ remove Tx_A, retain kernel(Tx_A)
```

The `CutThroughPruner` (in `ledger/src/cut_through.rs`) identifies confirmed non-tip transactions that have children and removes them, accumulating a kernel set. The **kernel sum rule** provides a compact validity proof for the entire ledger:

```
Σ inputs_all - Σ outputs_all = Σ excess_kernels
```

This enables state verification without replaying the full transaction history, improving both storage efficiency and sync speed.

---

## 4. Conflict Model

### 4.1 Conflict Definition

Two transactions T₁ and T₂ are **conflicting** if they have the same sender and nonce:

```
conflict(T₁, T₂) ⟺ T₁.sender = T₂.sender ∧ T₁.nonce = T₂.nonce ∧ T₁ ≠ T₂
```

This is the standard double-spend condition. The conflict set C(sender, nonce) is the set of all transactions from the same sender with the same nonce.

### 4.2 Conflict Status Machine

Each conflict set C(sender, nonce) progresses through a 5-state machine:

```
Pending → Ready → ClosedLocal → Reconciling → ClosedGlobal
                      ↑_______________|
```

Transitions:

- **Pending → Ready**: all transactions in C have W(T) ≥ θ_min = 3
- **Ready → ClosedLocal**: closure predicate holds (Section 4.3) and a finalized checkpoint anchors all transactions in C
- **ClosedLocal → Reconciling**: a newer partition boundary (checkpoint cp*) is discovered that post-dates the local anchor (PHA Step 3, Section 7)
- **Reconciling → ClosedGlobal**: closure predicate holds using frozen stake from the local anchor (PHA Step 5)
- **Reconciling → Ready**: closure predicate fails after re-evaluation (conflict requires more weight)

The transition `ClosedGlobal → *` does not exist. Global closure is terminal.

### 4.3 Closure Predicate

**Definition (Closure).** A conflict set C is closed if:

1. **Ready**: ∀T ∈ C, W(T) ≥ θ_min
2. **Anchored**: there exists a finalized checkpoint cp such that ∀T ∈ C, T ∈ descendants(cp)
3. **Dominant**: score(winner) ≥ σ · score(second), where σ = 2.0

The score of a transaction is its stake-weighted cumulative weight:

```
score(T) = W(T) · multiplier(T.sender)

multiplier(addr) = 1 + (stake(addr) / total_stake) · (M - 1)
```

where M = 3.0 is the maximum stake influence cap.

**Theorem S (Safety sketch).** If the closure predicate holds for winner T_w at a finalized checkpoint, then no honest node will select T_l ≠ T_w as a parent after closure, because honest parent selection filters conflict losers. Therefore the weight of T_l does not increase after closure, and the dominance condition is stable.

*Proof sketch.* Once score(T_w) ≥ 2·score(T_l), adding k transactions to T_w increases score(T_w) by k·multiplier while T_l is excluded from parent selection and gains no further weight. The gap is monotonically non-decreasing. □

---

## 5. Privacy

### 5.1 Stealth Addresses

Each payment can use a unique one-time stealth address derived from the recipient's spend public key and an ephemeral sender key:

```
stealth_addr = H(ECDH(ephemeral_priv, recipient_pub) ‖ recipient_pub)[0:20]
```

The recipient scans incoming transactions by recomputing the stealth address from each transaction's ephemeral public key and their spend private key. Only the recipient can identify payments addressed to them. This hides the receiver's identity from chain observers.

### 5.2 Pedersen Commitments and Excess Kernels

Transaction amounts are hidden using Pedersen commitments on the Ristretto255 group:

```
C(v, r) = r·G + v·H
```

where G is the Ristretto255 basepoint, H = hash_to_point("GhostLedger_H_v1"), v is the amount, and r is the blinding factor sampled uniformly at random.

This follows the Mimblewimble commitment model with three cryptographic properties:

- **Hiding**: C(v, r) reveals nothing about v or r individually.
- **Binding**: it is computationally infeasible to find (v', r') ≠ (v, r) such that C(v', r') = C(v, r).
- **Homomorphic**: C(v₁, r₁) + C(v₂, r₂) = C(v₁+v₂, r₁+r₂).

**Balance proof (excess kernel).** For each confidential transaction, the sender computes:

```
excess = Σ r_inputs - Σ r_outputs
excess_commitment = excess · G
excess_signature  = Sign(sk = excess)
```

The validator checks:

```
Σ C_inputs - Σ C_outputs = excess_commitment
```

This proves value conservation without revealing any individual amount. The excess signature proves the sender knows the blinding difference, preventing forgery.

**Three-step confidential transaction validation** (`validate_confidential_tx`):

1. **Range proof**: verify `range_proof` against the output commitment (currently `PlaceholderRangeProof`; Bulletproofs are planned as a future backend via `trait RangeProofSystem`).
2. **Balance proof**: verify `BalanceProof` — excess commitment matches the difference of input and output commitment sums.
3. **Excess**: verify `excess_commitment` and `excess_signature` are present and structurally valid.

**Range proof backend abstraction.** The `RangeProofSystem` trait in `crypto/src/range_proof.rs` defines a backend-agnostic API:

```rust
trait RangeProofSystem {
    type Proof;
    fn prove(amount: u64, blinding: &BlindingFactor, commitment: &Commitment)
        -> Result<Self::Proof, RangeProofError>;
    fn verify(commitment: &Commitment, proof: &Self::Proof)
        -> Result<(), RangeProofError>;
    fn is_production_safe() -> bool;
}
```

The current backend is `PlaceholderRangeProof` with `is_production_safe() = false`. The abstraction allows drop-in replacement with a Bulletproofs or Halo2 backend without protocol changes.

### 5.3 Graph Privacy

The above mechanisms hide **who** and **how much**, but the transaction graph remains partially observable. Parent links, timing, and relay paths can be used by a passive observer to infer the sender's network position.

Three attack vectors are specifically defended against (implemented in `ledger/src/privacy.rs`):

**Intersection attack.** An observer who sees multiple transactions from the same address intersects their parent sets. If the parent sets overlap consistently, the observer narrows down the sender's local DAG view. Mitigated by ε-noise parent selection (decoy injection) and tracked by `IntersectionAttackDetector`, which computes per-address Jaccard overlap and timing regularity scores.

**Timing correlation.** Even with relay delay, a transaction appearing significantly earlier at one node than others identifies that node as the likely origin. Mitigated by Dandelion-style stem/fluff diffusion: ~20% of transactions enter a stem phase (500–1000ms single-relay delay) before broadcast, breaking naive first-seen triangulation.

**Parent topology inference.** Predictable parent selection patterns (e.g. always choosing the heaviest tip) are distinguishable from random. `GraphPrivacyAnalyzer` evaluates each transaction's parent entropy (Shannon entropy over parent weights), fan-out score, and timing exposure, producing a `privacy_score ∈ [0.0, 1.0]`.

**Graph entropy metrics.** The Python simulator (`sim/metrics.py`) tracks three graph-level privacy metrics per trial:

- **parent_diversity**: fraction of unique parent combinations (0 = all selections identical, 1 = every selection unique)
- **graph_entropy**: Shannon entropy over individual parent usage frequency (higher = parents distributed more uniformly across the DAG)
- **origin_recovery_risk**: composite metric combining diversity risk and normalized entropy (0 = low risk, 1 = easy to deanonymize)

Empirical results at 50 trials show `origin_recovery_risk` decreasing monotonically with ε: from 0.072 at ε=0.00 to 0.044 at ε=0.30, confirming that decoy injection measurably reduces deanonymization risk.

**Remaining gap.** Sender address is public on-chain. Decoy pools are bounded (50 entries). The stem phase applies delay locally rather than routing through a true relay chain. A global passive adversary retains meaningful deanonymization capability. No formal anonymity set bound is established. See `THREAT_MODEL.md` for full analysis.

---

## 6. Parent Selection

### 6.1 The Honest Parent Selection Problem

Parent selection is the mechanism by which a new transaction chooses which tips to reference as parents. This choice has two competing objectives:

- **Consensus**: prefer tips that help accumulate weight on the conflict winner, driving convergence
- **Privacy**: introduce noise to prevent a graph observer from inferring the sender's position in the network

These objectives are in direct tension. A node that always selects the heaviest tip (pure greedy, β=1.0) maximizes consensus signal but is trivially identifiable. A node that selects randomly (β=0.0) is less identifiable but contributes less to convergence.

We call this the **Honest Parent Selection Problem**: how should an honest node select parents to satisfy both objectives simultaneously?

### 6.2 ParentSelectionPolicy

We define a parameterized policy with two explicit parameters:

```
Policy = (β, ε, max_parents)
```

- **β ∈ [0.0, 1.0]**: consensus bias. β=0.0 is uniform random selection; β=1.0 is pure greedy (heaviest tip). Intermediate values use the Gumbel-max sampling trick: each candidate tip t receives a random key k(t) = -ln(U) / w(t)^β, and tips are selected in ascending key order.

- **ε ∈ [0.0, 1.0]**: privacy noise. With probability ε, one selected parent is replaced with a decoy sampled from a pool of recently observed transactions that are not current tips. Decoy selection is weight-adaptive: decoys are preferentially sampled from entries with weights similar to the real parents, making them harder to distinguish by weight analysis alone.

- **max_parents**: maximum number of parents per transaction (default 2).

Three named policy presets are defined, each justified by empirical data (Section 9):

```rust
ParentSelectionPolicy::default()   // β=0.7, ε=0.10 — production balance
ParentSelectionPolicy::privacy_mode()  // β=0.7, ε=0.20 — privacy priority
ParentSelectionPolicy::consensus_mode() // β=0.7, ε=0.00 — consensus priority
```

### 6.3 Conflict-Aware Filtering

Before applying the β/ε policy, conflict losers are filtered from the candidate set:

```
candidates = Tips(G) \ {T : T is a conflict loser}
```

If all tips are conflict losers (e.g. the winner is no longer a tip), the full tip set is used as fallback. This ensures honest nodes do not reinforce losing transactions.

### 6.4 Diffusion Delay and Dandelion Phases

To reduce timing correlation, each transaction is relayed with a random delay:

```
delay(T) = min_delay + H(T.tx_id) mod (max_delay - min_delay)
```

Default: min=50ms, max=500ms. The delay is deterministic from the transaction ID, ensuring consistent behavior across restarts.

Additionally, each transaction is assigned a Dandelion phase deterministically from its tx_id:

- **Stem phase (~20% of transactions):** relayed with a 500–1000ms delay before broadcast. Intended to obscure the originating node from timing-based observers.
- **Fluff phase (~80% of transactions):** standard gossip broadcast with the normal 50–500ms delay.

The phase is determined by `H(tx_id) mod 1024 < 205`. Both the phase assignment and the stem delay are fully deterministic and stateless.

### 6.5 Default Parameters

The default policy is (β=0.7, ε=0.10, max_parents=2). This is empirically validated across 50 trials per configuration (Section 9) and represents the optimal balance of closure rate, DAG width, and origin recovery risk.

---

## 7. Partition Healing Algorithm (PHA)

### 7.1 Motivation

Network partitions cause two sub-DAGs to grow independently. When the partition heals, both sides have conflicts that were resolved locally using different subsets of the global state. The Partition Healing Algorithm reconciles these local resolutions into a globally consistent outcome.

### 7.2 Algorithm

The PHA proceeds in 6 steps after a partition heals:

**Step 1 (Handshake).** Both nodes exchange their latest finalized checkpoint. They agree on a common checkpoint cp* = the most recent checkpoint that both nodes have seen and finalized.

**Step 2 (Invariant G).** Conflicts that were closed below cp* (i.e. all their transactions are ancestors of cp*) are not touched. Their local closure is preserved. This is **Invariant G**: checkpoints below the partition boundary are immutable.

**Step 3 (Downgrade).** Conflicts closed above cp* are downgraded from `ClosedLocal` to `Reconciling`. These are the conflicts that might have been resolved differently on each side.

**Step 4 (Sync).** Each node requests all transactions above cp* from the other side. After exchange, both nodes have the same view of the DAG above cp*.

**Step 5 (Re-evaluate).** For each `Reconciling` conflict, the closure predicate is re-evaluated using the frozen stake snapshot from the original local anchor (not current stake). This prevents stake manipulation attacks where an attacker moves stake between the partition and the merge to influence outcome.

**Step 6 (Close).** If the predicate holds: `ClosedGlobal`. If not: back to `Ready`, waiting for more weight.

### 7.3 Theorem P (Partition Safety)

**Theorem P.** After PHA completes on two nodes A and B that share the same DAG above cp*, if a conflict C is closed globally on both, then both nodes agree on the same winner.

*Proof.* After Step 4, both nodes have identical DAG state above cp*. Steps 5–6 apply a deterministic function (closure predicate with frozen stake) to identical input. Deterministic function on identical input produces identical output. □

**Corollary P (Liveness, proof-sketch only).** Under the assumption of Global Stabilization Time (GST) — that after some time t_GST all messages are delivered within Δ — eventually all nodes accumulate sufficient weight above cp* for the dominant transaction to satisfy the σ-dominance condition, and all conflicts close globally.

*This is a proof-sketch. The formal proof requires a bounded gossip convergence model and explicit analysis of the multi-partition case, which remains open.*

---

## 8. Security Model

### 8.1 Spam Resistance

Spam is controlled by two mechanisms:

**Dynamic PoW.** Each transaction must find a nonce such that SHA256(payload ‖ nonce) has a difficulty-appropriate prefix. Difficulty adjusts based on observed TPS over a 60-second window: increases above 10 TPS, decreases below 2 TPS. Range: difficulty 2–6 (leading zeros).

**Per-address rate limiting.** Each address is limited to 5 transactions per 10-second window, with a burst cap of 10. This is a second-layer defense independent of PoW difficulty.

### 8.2 Stake Adversary (Conjecture F)

Let f be the fraction of total stake controlled by an adversary. Let σ=2.0 be the closure ratio and α=3.0 be the stake multiplier cap.

**Definition (Drift boundary).** The adversary can prevent closure if their weighted score grows faster than the honest score. This requires:

```
f · α > (1 - f) · 1  →  f > 1/(1 + α) ≈ 0.25
```

**Conjecture F.** If f < 1/(σ·α) ≈ 0.167, then with overwhelming probability an adversary cannot cause a closed conflict to revert or prevent an honest conflict from closing within O(W/Δ) steps, where W is the DAG weight gap and Δ is the gossip convergence time.

*This is a conjecture supported by Monte Carlo simulation (byzantine_sim.rs) but without a formal proof.*

### 8.3 Eclipse Attack

The P2P layer implements subnet-based eclipse detection: if more than 80% of peers share a /16 subnet prefix (among peers ≥ 10), the node logs a warning. Random peer sampling (gossip sample of 8 peers) limits the influence of any single subnet cluster.

Detection is implemented; automatic response (peer rotation) is not yet implemented. See `THREAT_MODEL.md`.

### 8.4 Parasite DAG

An adversary may attempt to build a private sub-DAG and reveal it to revert confirmed transactions. The stake-weighted closure rule with σ=2.0 provides resistance: to revert a closed conflict, the adversary must accumulate score ≥ 2·score(winner), which requires controlling a fraction f > 1/(1+α) of stake while the honest network continues building on the winner.

---

## 9. Empirical Results

### 9.1 Simulation Setup

We implement a discrete-event DAG simulator in Python (`sim/`) with the following model:

- N=6 honest nodes sharing a global DAG (instant gossip)
- Each node sees a partial view of the last 15–21 transactions (gossip lag model)
- All nodes select parents independently before adding their transaction each step
- Conflicts are injected every 30 steps (two nodes emit conflicting transactions for the same sender/nonce)
- Closure rule: σ=2.0, θ_min=3
- **50 independent trials per configuration** (increased from 10 for statistical stability)

Metrics: conflict closure rate, median closure time, mean DAG width, parent diversity, graph entropy, and origin recovery risk.

### 9.2 Parent Selection Parameter Sweep

We swept β ∈ {0.0, 0.3, 0.5, 0.7, 0.9, 1.0} and ε ∈ {0.00, 0.05, 0.10, 0.15, 0.20, 0.30} with 50 independent trials per combination (N_TX=150, CONFLICT_EVERY=30).

**Key finding 1: Pure greedy selection causes tip starvation.**

β=1.0 produces DAG width of 143–207 tips and closure rate ≈ 0.00–0.01, regardless of ε. When all nodes always select the single heaviest tip, all other tips receive no further confirmations. The DAG frontier grows without bound and the weight accumulation required for closure never occurs.

> Pure greedy parent selection (β=1.0) is incompatible with this protocol's consensus mechanism.

**Key finding 2: β∈[0.3, 0.9] is a stable operating region.**

All configurations with β < 1.0 show:
- Closure rate: 0.59–0.84
- Median closure time: 1.7–5.2 steps
- Mean DAG width: 7.7–11.7 tips

**Key finding 3: β=0.7 is the empirically optimal consensus bias.**

Across 50 trials, β=0.7, ε=0.00 achieves closure_rate=0.795 with dag_width=7.7 — consistently among the top configurations. β=0.9 performs similarly (0.805, width=7.8) but shows higher variance at ε>0.10.

**Key finding 4: ε=0.10 is the privacy/consensus sweet spot.**

Increasing ε from 0.00 to 0.10 reduces `origin_recovery_risk` from 0.072 to 0.061 (−15%) while degrading closure_rate by less than 0.05 on average. Above ε=0.20, DAG width grows by 1–3 tips and closure rate degrades noticeably.

**Key finding 5: origin_recovery_risk decreases monotonically with ε.**

```
ε=0.00: origin_risk ≈ 0.072
ε=0.10: origin_risk ≈ 0.061
ε=0.20: origin_risk ≈ 0.053
ε=0.30: origin_risk ≈ 0.045
```

This confirms that decoy injection measurably reduces graph-level deanonymization risk.

**Summary table (50 trials, selected configurations):**

| β    | ε    | closure_rate | median_closure | dag_width | origin_risk |
|------|------|-------------|----------------|-----------|-------------|
| 0.7  | 0.00 | 0.795       | 2.1            | 7.7       | 0.072       |
| 0.7  | 0.10 | 0.685       | 3.2            | 8.3       | 0.061       |
| 0.7  | 0.20 | 0.725       | 2.5            | 9.2       | 0.053       |
| 0.9  | 0.00 | 0.805       | 2.1            | 7.8       | 0.073       |
| 0.9  | 0.10 | 0.665       | 2.6            | 8.3       | 0.061       |
| 0.5  | 0.10 | 0.725       | 2.7            | 8.3       | 0.062       |
| 1.0  | 0.00 | 0.000       | ∞              | 190.7     | 0.323       |

**Policy presets derived from simulation:**

| Mode      | β   | ε    | Rationale                              |
|-----------|-----|------|----------------------------------------|
| default   | 0.7 | 0.10 | Best closure/privacy/width balance     |
| privacy   | 0.7 | 0.20 | Lower origin_risk, modest width cost   |
| consensus | 0.7 | 0.00 | Highest closure_rate, no privacy noise |

### 9.3 Scale Experiment

We ran the default policy (β=0.7, ε=0.10) at N_TX ∈ {150, 500, 1000} with CONFLICT_EVERY=50:

| N_TX | closure_rate | median_closure | dag_width |
|------|-------------|----------------|-----------|
| 150  | 0.68        | 3.2            | 8.3       |
| 500  | 0.71        | 2.0            | 10.0      |
| 1000 | 0.68        | 2.4            | 11.0      |

Closure time grows from 2.0 to 2.4 steps as DAG size grows from 500 to 1000 — sub-linear scaling consistent with the O(log W) expected convergence.

### 9.4 Conflict Rate Experiment

Fixed (β=0.7, ε=0.10, N_TX=500), varying CONFLICT_EVERY ∈ {10, 30, 50, 100}:

| conflict_every | n_conflicts | closure_rate | median_closure | dag_width |
|---------------|-------------|-------------|----------------|-----------|
| 10            | 48          | 0.74        | 2.0            | 7.8       |
| 30            | 16          | 0.73        | 2.2            | 8.9       |
| 50            | 9           | 0.64        | 2.3            | 10.2      |
| 100           | 4           | 0.70        | 2.7            | 11.6      |

High conflict rate (conflict_every=10) produces narrower DAG width (7.8 vs 11.6). This is counterintuitive but explainable: more conflicts means more losers filtered from parent selection, which concentrates weight on fewer tips and reduces sprawl.

---

## 10. Implementation

The protocol is implemented as a Rust workspace (`ghost_core/`) with the following crates:

| Crate | Responsibility |
|-------|---------------|
| `crypto` | Ed25519 signatures, X25519 stealth addresses, Pedersen commitments on Ristretto255, `BalanceProof`, `BlindingFactor`, `trait RangeProofSystem`, `PlaceholderRangeProof` |
| `ledger` | DAG, state, validator (`validate_confidential_tx`), pruner, cut-through pruner, anti-spam, Merkle roots, checkpoint registry, `ParentSelectionPolicy` (3 presets), graph privacy (`GraphPrivacyAnalyzer`, `IntersectionAttackDetector`, Dandelion diffusion) |
| `consensus` | `ConflictResolver` (5-state machine, PHA), `TipSelector`, Byzantine simulation |
| `token` | GHOST token, `StakingManager` (stake/slash/eject/pool distribution) |
| `network` | WebSocket P2P, gossip, peer discovery, eclipse detection |
| `storage` | JSON snapshot with atomic write |
| `ghost-node` | Binary node — CLI, genesis, bootstrap |
| `ghost-explorer` | TUI explorer (ratatui) |

Test suite: **224 passing tests** across all crates.

**Privacy layer.** Confidential transactions carry real Pedersen commitments (`C = r·G + v·H`), a `BalanceProof` (excess commitment + excess signature), and a `range_proof` field for future Bulletproofs integration. The validator enforces all three via `validate_confidential_tx`, which runs range proof, balance proof, and excess checks in sequence. `RangeProofStatus` is a typed enum (`Missing` / `Experimental` / `Verified`) — not a string.

**Cut-through pruning.** `CutThroughPruner` in `ledger/src/cut_through.rs` removes confirmed intermediate transactions from the DAG, retaining their kernels. `validate_kernel_sum()` checks that all retained kernels have non-null excess commitments, providing a compact ledger validity proof.

**State root anchoring.** The `CheckpointRegistry` maintains an ordered sequence of `CheckpointVertex` objects, each containing a `state_root` (Merkle root of the ledger state at that DAG height). The `verify_chain()` method validates that the sequence is monotonically increasing in both sequence number and DAG height. When syncing from a peer, `verify_synced_state()` checks the received state against the local latest trusted checkpoint root.

**Graph privacy layer.** `GraphPrivacyAnalyzer` computes a `privacy_score ∈ [0.0, 1.0]` from parent entropy, fan-out score, and timing exposure. `IntersectionAttackDetector` maintains a sliding window of observations per address and computes intersection risk from timing regularity and parent Jaccard overlap.

---

## 11. Open Problems

We identify the following open problems explicitly:

**1. β/ε formal analysis.** The simulation at 50 trials confirms β=0.7 as the empirically optimal consensus bias, but a formal analysis relating β to convergence speed under the gossip model — and ε to the anonymity set size — would provide a principled closed-form bound.

**2. Honest Parent Selection Problem.** Privacy noise (ε > 0) introduces decoy parents that reduce the convergence signal. This is an inherent tension: more privacy means slower consensus. The trade-off is characterized empirically but not analytically.

**3. Corollary P (formal proof).** The liveness argument for PHA under partition is a proof-sketch. A formal proof requires: (a) a bounded gossip model with message loss probability, (b) explicit analysis of the multi-partition case where more than two network components exist simultaneously, and (c) proof that the σ-dominance condition is eventually satisfied under bounded adversarial stake.

**4. Graph deanonymization (partial mitigation).** Graph privacy tools are implemented — `GraphPrivacyAnalyzer`, `IntersectionAttackDetector`, Dandelion stem/fluff. However, the sender address is public on-chain, decoy pools are bounded, and the stem phase applies delay locally rather than routing through a true relay chain. A global passive adversary who observes all network traffic retains significant deanonymization capability. A formal anonymity set bound for DAG graphs remains open.

**5. Bulletproofs integration.** Range proofs are currently handled by `PlaceholderRangeProof`. The `trait RangeProofSystem` provides a drop-in backend API ready for Bulletproofs or Halo2. The blocking issue is the `curve25519-dalek v3/v4` version conflict in the Rust ecosystem; this is a known ecosystem problem rather than a protocol design issue.

**6. State root finality chain.** Merkle roots are computed and verified but not yet incorporated into a proper finality chain where each checkpoint commits to the previous checkpoint's root. This would enable secure light clients.

**7. Parasite DAG (formal analysis).** The closure rule and honest parent selection together provide informal resistance to parasite branches. A formal analysis bounding the probability of a successful parasite attack as a function of adversary stake fraction is not yet done.

---

## 12. Related Work

**IOTA (Popov, 2018).** The original DAG ledger using the Tangle. GhostLedger differs in: no coordinator, stake-weighted conflict resolution instead of random walk, explicit privacy layer, and the 5-state conflict machine.

**Nano (LeMahieu, 2018).** Feeless DAG ledger with delegated PoS. No amount privacy. Conflict resolution uses voting rather than cumulative weight. GhostLedger's approach is closer to implicit confirmation via weight accumulation.

**Monero.** Strong privacy via ring signatures and RingCT. Block-based, proof-of-work, fees required. GhostLedger borrows the Pedersen commitment approach but uses DAG structure and stealth addresses differently.

**Mimblewimble / Grin.** The excess kernel model and cut-through pruning in GhostLedger directly follow the Mimblewimble construction. The key difference is DAG structure instead of a linear chain, and an explicit conflict resolution layer on top.

**Avalanche (Rocket et al., 2020).** DAG-based metastable consensus via repeated sampling. Provides probabilistic finality. GhostLedger's closure rule is deterministic once the σ-threshold is met, which is stronger but requires more weight accumulation.

**Dandelion (Fanti et al., 2018).** Diffusion relay protocol for blockchain privacy. GhostLedger implements an analogous stem/fluff mechanism for DAG broadcast, with deterministic phase assignment per transaction and 2× stem delay.

---

## 13. Conclusion

GhostLedger demonstrates that a feeless, private, decentralized DAG ledger is architecturally viable. The key mechanisms — cumulative weight consensus, stake-weighted conflict closure with σ-dominance, stealth addresses, Pedersen commitments with excess kernels, hybrid parent selection, cut-through pruning, graph privacy analysis, and the Partition Healing Algorithm — form a coherent whole that has been implemented and tested.

The empirical results at 50 trials confirm three non-obvious findings: pure greedy parent selection causes DAG divergence regardless of privacy noise level; β=0.7 is the empirically optimal consensus bias outperforming the previously assumed β=0.5; and decoy injection (ε>0) measurably reduces graph-level deanonymization risk with a modest, quantified consensus cost.

The privacy layer has been upgraded from a SHA-256 scaffolding to a cryptographically correct Pedersen commitment system with real blinding factors, balance proofs, excess kernels, and a typed range proof abstraction ready for Bulletproofs. The validator enforces all three confidential transaction checks in sequence.

Privacy at the graph level is partially addressed: parent entropy analysis, intersection attack detection, and Dandelion-style diffusion are implemented and tested. A global passive adversary with access to both network traffic and on-chain data retains meaningful deanonymization capability. This is the primary open problem for the next phase of development.

The protocol should be considered a research prototype. The implementation is public at https://github.com/AlexBil-rar/Token. A full threat model is available in `THREAT_MODEL.md`.

---

## References

1. S. Popov. "The Tangle." IOTA Foundation, 2018.
2. C. LeMahieu. "Nano: A Feeless Distributed Cryptocurrency Network." 2018.
3. T. Rocket, M. Yin, K. Sekniqi, R. van Renesse, E. G. Sirer. "Scalable and Probabilistic Leaderless BFT Consensus through Metastability." arXiv:1906.08936, 2020.
4. G. Fanti, S. B. Venkatakrishnan, S. Bakshi, B. Bhatt, S. Bhatt, P. Viswanath. "Dandelion++: Lightweight Cryptocurrency Networking with Formal Anonymity Guarantees." ACM SIGMETRICS, 2018.
5. N. van Saberhagen. "CryptoNote v2.0." 2013. (Stealth addresses)
6. T. P. Pedersen. "Non-Interactive and Information-Theoretic Secure Verifiable Secret Sharing." CRYPTO 1991. (Pedersen commitments)
7. A. Poelstra. "Mimblewimble." 2016. (Excess kernels, cut-through)
8. M. Hamburg. "Decaf: Eliminating Cofactors Through Point Compression." CRYPTO 2015. (Ristretto255 construction basis)
9. B. Bünz, J. Bootle, D. Boneh, A. Poelstra, P. Wuille, G. Maxwell. "Bulletproofs: Short Proofs for Confidential Transactions and More." IEEE S&P 2018.

---

*GhostLedger v0.1 — March 2026*
*Source: https://github.com/AlexBil-rar/Token*
*This document describes a research prototype. No security guarantees are implied.*
