# GhostLedger: A Feeless, Private DAG-Based Payment Ledger

**Aleksandr Bilyk**
Independent Researcher
March 2026

---

## Abstract

We present GhostLedger, a DAG-based payment ledger that simultaneously targets three properties rarely combined in practice: no transaction fees, strong sender/amount privacy, and decentralized conflict resolution without block producers. The protocol uses cumulative DAG weight for consensus, dynamic proof-of-work for spam resistance, stealth addresses and Pedersen commitments for privacy, and a stake-weighted closure rule for double-spend resolution. We introduce a hybrid parent selection policy parameterized by a consensus bias β and a privacy noise level ε, and show empirically that pure greedy selection (β=1.0) causes DAG divergence via tip starvation, while moderate bias (β∈[0.3, 0.9]) with bounded noise (ε≤0.10) provides stable operation. We also present the Partition Healing Algorithm (PHA) and a 5-state conflict status machine that handles network partitions without coordinator intervention. Safety and liveness are argued informally with identified open problems.

---

## 1. Introduction

Payment systems face a persistent trilemma: feeless operation removes economic spam resistance; strong privacy complicates balance verification; decentralization removes the authority that would otherwise resolve conflicts. Existing systems address at most two of these simultaneously. Nano achieves feeless DAG but has no amount privacy. Monero achieves strong privacy but requires miners and fees. IOTA uses a DAG but relies on a coordinator and has limited privacy.

GhostLedger is an attempt to architect all three properties together and to understand what the tradeoffs look like when they are forced to coexist. The core thesis is that spam resistance can come from proof-of-work rather than fees, privacy can be layered at the transaction level without breaking consensus, and conflict resolution can be driven by the transactions themselves via cumulative weight rather than by dedicated validators.

The primary contribution of this paper is not a finished system but a protocol architecture with working Rust implementation (184 passing tests), formal definitions of the key mechanisms, and empirical characterization of the parent selection parameter space.

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
     commitment?, balance_proof?)
```

The fields `commitment` and `balance_proof` are optional and enable amount privacy (Section 5.2). The `parents` field references 1–2 previous vertices; this is what makes the ledger a DAG rather than a chain.

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

The number of live tips |Tips(G)| reflects the "width" of the DAG frontier. Empirical results (Section 9) show that under normal operation with honest nodes, DAG width stabilizes at 7–11 tips. Pathological parent selection (pure greedy, β=1.0) causes tip starvation where width grows unboundedly (150–200 tips observed), preventing weight accumulation and therefore confirmation.

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

### 5.2 Pedersen Commitments

Transaction amounts are hidden using Pedersen commitments on the Ristretto255 group:

```
C(amount, r) = r·G + amount·H
```

where G is the Ristretto255 basepoint and H = hash_to_point("GhostLedger_H_v1").

Commitments are additively homomorphic: C(a, r₁) + C(b, r₂) = C(a+b, r₁+r₂). This allows balance verification without revealing amounts: a transaction is balance-preserving if and only if the sum of input commitments equals the sum of output commitments plus a fee commitment (zero in this protocol).

A `BalanceProof` accompanies private transactions to prove that the excess commitment (sum_inputs - sum_outputs) commits to zero, without revealing the individual amounts.

### 5.3 Graph Privacy

The above mechanisms hide **who** and **how much**, but the transaction graph remains observable. Parent links, timing, interaction patterns, and relay paths are visible to a network observer. This is an open problem addressed partially by the parent selection policy (Section 6.3) and diffusion delay (Section 6.4).

**Open Problem (Graph Deanonymization).** Current privacy does not protect against intersection attacks, timing correlation, or parent topology inference. Decoy parents and diffusion delay reduce the signal but do not eliminate it. A formal privacy model for DAG graphs is future work.

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

- **ε ∈ [0.0, 1.0]**: privacy noise. With probability ε, one selected parent is replaced with a decoy sampled from a pool of recently observed transactions that are not current tips.

- **max_parents**: maximum number of parents per transaction (default 2).

### 6.3 Conflict-Aware Filtering

Before applying the β/ε policy, conflict losers are filtered from the candidate set:

```
candidates = Tips(G) \ {T : T is a conflict loser}
```

If all tips are conflict losers (e.g. the winner is no longer a tip), the full tip set is used as fallback. This ensures honest nodes do not reinforce losing transactions.

### 6.4 Diffusion Delay

To reduce timing correlation, each transaction is relayed with a random delay:

```
delay(T) = min_delay + H(T.tx_id)[0:8] mod (max_delay - min_delay)
```

Default: min=50ms, max=500ms. The delay is deterministic from the transaction ID, ensuring consistent behavior across restarts.

### 6.5 Default Parameters

The default policy is (β=0.7, ε=0.10, max_parents=2). Empirical validation (Section 9) confirms this lies within the stable operating region.

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

Metrics: conflict closure rate (fraction of conflicts resolved before end of simulation), median closure time (steps from injection to resolution), and mean DAG width (live tips).

### 9.2 Parent Selection Parameter Sweep

We swept β ∈ {0.0, 0.3, 0.5, 0.7, 0.9, 1.0} and ε ∈ {0.00, 0.05, 0.10, 0.20, 0.30} with 10 independent trials per combination (N_TX=150, CONFLICT_EVERY=30).

**Key finding 1: Pure greedy selection causes tip starvation.**

β=1.0 produces DAG width of 150–200 tips and closure rate ≈ 0.00–0.25, regardless of ε. When all nodes always select the single heaviest tip, all other tips receive no further confirmations. The DAG frontier grows without bound and the weight accumulation required for closure never occurs.

> Pure greedy parent selection (β=1.0) is incompatible with this protocol's consensus mechanism.

**Key finding 2: β∈[0.3, 0.9] is a stable operating region.**

All configurations with β < 1.0 show:
- Closure rate: 0.55–0.88
- Median closure time: 1.8–5.4 steps
- Mean DAG width: 7.6–11.8 tips

There is no single optimal β in this range; the differences are within the noise of 10 trials.

**Key finding 3: ε≤0.10 introduces negligible consensus degradation.**

Increasing ε from 0.00 to 0.10 changes DAG width by at most +1.0 tip and closure rate by less than 0.05 in most configurations. At ε=0.20–0.30, width increases by 1–3 additional tips and closure rate degrades by 0.05–0.15.

**Summary table (selected configurations):**

| β    | ε    | closure_rate | median_closure | dag_width |
|------|------|-------------|----------------|-----------|
| 0.0  | 0.00 | 0.72–0.82   | 1.7–2.4        | 7.6       |
| 0.3  | 0.00 | 0.75–0.85   | 1.8–2.1        | 7.8       |
| 0.5  | 0.10 | 0.65–0.78   | 1.9–3.6        | 8.3–8.7   |
| 0.7  | 0.10 | 0.70–0.88   | 2.5            | 8.4–8.8   |
| 0.9  | 0.30 | 0.62        | 5.4            | 10.6      |
| 1.0  | 0.00 | 0.00        | ∞              | 197–204   |

### 9.3 Scale Experiment

We ran the sweet-spot policy (β=0.5, ε=0.10) at N_TX ∈ {150, 500, 1000} with CONFLICT_EVERY=50:

| N_TX | closure_rate | median_closure | dag_width |
|------|-------------|----------------|-----------|
| 150  | 0.55        | ∞              | 8.7       |
| 500  | 0.71        | 2.0            | 10.0      |
| 1000 | 0.68        | 2.4            | 11.0      |

N_TX=150 is insufficient for stable results — conflicts do not accumulate enough weight before the simulation ends. At N_TX ≥ 500, behavior is stable. Closure time grows from 2.0 to 2.4 steps as DAG size grows from 500 to 1000 — sub-linear scaling consistent with the O(log W) expected convergence.

### 9.4 Conflict Rate Experiment

Fixed (β=0.5, ε=0.10, N_TX=500), varying CONFLICT_EVERY ∈ {10, 30, 50, 100}:

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
| `crypto` | Ed25519 signatures, X25519 stealth addresses, Pedersen commitments on Ristretto255 |
| `ledger` | DAG, state, validator, pruner, anti-spam, Merkle roots, checkpoint registry, `ParentSelectionPolicy` |
| `consensus` | `ConflictResolver` (5-state machine, PHA), `TipSelector`, Byzantine simulation |
| `token` | GHOST token, `StakingManager` (stake/slash/eject/pool distribution) |
| `network` | WebSocket P2P, gossip, peer discovery, eclipse detection |
| `storage` | JSON snapshot with atomic write |
| `ghost-node` | Binary node — CLI, genesis, bootstrap |
| `ghost-explorer` | TUI explorer (ratatui) |

Test suite: 184 passing tests across all crates.

**State root anchoring.** The `CheckpointRegistry` maintains an ordered sequence of `CheckpointVertex` objects, each containing a `state_root` (Merkle root of the ledger state at that DAG height). The `verify_chain()` method validates that the sequence is monotonically increasing in both sequence number and DAG height, and that no checkpoint has an empty state root. When syncing from a peer, `verify_synced_state()` checks the received state against the local latest trusted checkpoint root, rejecting state that does not match.

---

## 11. Open Problems

We identify the following open problems explicitly:

**1. β/ε optimal values.** The simulation shows a stable operating region but not a unique optimum. With only 10 trials per configuration, variance is high. A formal analysis relating β to convergence speed under the gossip model, and ε to the privacy gain (measured by parent entropy or graph clustering), would provide a principled basis for parameter selection.

**2. Honest Parent Selection Problem.** Privacy noise (ε > 0) introduces decoy parents that reduce the convergence signal. This is an inherent tension: more privacy means slower consensus. There is no known closed-form characterization of the optimal β/ε frontier.

**3. Corollary P (formal proof).** The liveness argument for PHA under partition is a proof-sketch. A formal proof requires: (a) a bounded gossip model with message loss probability, (b) explicit analysis of the multi-partition case where more than two network components exist simultaneously, and (c) proof that the σ-dominance condition is eventually satisfied under bounded adversarial stake.

**4. Graph privacy.** Current privacy protects amounts and receiver identities but not the graph itself. Timing, parent topology, and relay path analysis can deanonymize senders. A formal privacy model for DAG transaction graphs — analogous to Dandelion for blockchains — is future work.

**5. State root finality chain.** Merkle roots are computed and verified but not yet incorporated into a proper finality chain where each checkpoint commits to the previous checkpoint's root. This would enable secure light clients.

**6. Parasite DAG (formal analysis).** The closure rule and honest parent selection together provide informal resistance to parasite branches. A formal analysis bounding the probability of a successful parasite attack as a function of adversary stake fraction is not yet done.

---

## 12. Related Work

**IOTA (Popov, 2018).** The original DAG ledger using the Tangle. GhostLedger differs in: no coordinator, stake-weighted conflict resolution instead of random walk, explicit privacy layer, and the 5-state conflict machine.

**Nano (LeMahieu, 2018).** Feeless DAG ledger with delegated PoS. No amount privacy. Conflict resolution uses voting rather than cumulative weight. GhostLedger's approach is closer to implicit confirmation via weight accumulation.

**Monero.** Strong privacy via ring signatures and RingCT. Block-based, proof-of-work, fees required. GhostLedger borrows the Pedersen commitment approach but uses DAG structure and stealth addresses differently.

**Avalanche (Rocket et al., 2020).** DAG-based metastable consensus via repeated sampling. Provides probabilistic finality. GhostLedger's closure rule is deterministic once the σ-threshold is met, which is stronger but requires more weight accumulation.

**Dandelion (Fanti et al., 2018).** Diffusion relay protocol for blockchain privacy. GhostLedger implements an analogous mechanism (random relay delay) but for DAG broadcast.

---

## 13. Conclusion

GhostLedger demonstrates that a feeless, private, decentralized DAG ledger is architecturally viable. The key mechanisms — cumulative weight consensus, stake-weighted conflict closure with σ-dominance, stealth addresses, Pedersen commitments, hybrid parent selection, and the Partition Healing Algorithm — form a coherent whole that has been implemented and tested.

The empirical results confirm two non-obvious findings: pure greedy parent selection causes DAG divergence regardless of privacy noise level, and high conflict rates produce narrower (healthier) DAGs rather than wider ones.

The protocol has significant open problems — particularly around formal liveness proofs, graph privacy, and optimal parameter selection — and should be considered a research prototype rather than a production system. The implementation is public at https://github.com/AlexBil-rar/Token.

---

## References

1. S. Popov. "The Tangle." IOTA Foundation, 2018.
2. C. LeMahieu. "Nano: A Feeless Distributed Cryptocurrency Network." 2018.
3. T. Rocket, M. Yin, K. Sekniqi, R. van Renesse, E. G. Sirer. "Scalable and Probabilistic Leaderless BFT Consensus through Metastability." arXiv:1906.08936, 2020.
4. G. Fanti, S. B. Venkatakrishnan, S. Bakshi, B. Bhatt, S. Bhatt, P. Viswanath. "Dandelion++: Lightweight Cryptocurrency Networking with Formal Anonymity Guarantees." ACM SIGMETRICS, 2018.
5. N. van Saberhagen. "CryptoNote v2.0." 2013. (Stealth addresses)
6. T. P. Pedersen. "Non-Interactive and Information-Theoretic Secure Verifiable Secret Sharing." CRYPTO 1991. (Pedersen commitments)
7. M. Hamburg. "Decaf: Eliminating Cofactors Through Point Compression." CRYPTO 2015. (Ristretto255 construction basis)

---

*GhostLedger v0.1 — March 2026*
*Source: https://github.com/AlexBil-rar/Token*
*This document describes a research prototype. No security guarantees are implied.*
