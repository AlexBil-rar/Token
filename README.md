# GhostLedger

A solo research project exploring whether feeless, private, and decentralized can coexist in one DAG-based protocol.

---

## What this is

GhostLedger is an attempt to build a payment ledger with three properties at once: no transaction fees, meaningful privacy, and no central authority. Each of these exists somewhere in the wild — Nano has feeless DAG, Monero has privacy, IOTA has user-validated DAG. None of them have all three together. This project explores whether the combination is architecturally viable, and what the tradeoffs look like.

This is not a production system. It is a research prototype with a working Rust core, a Python proof-of-concept, a P2P gossip network, and a growing test suite. The goal right now is to get the protocol core right before worrying about scale.

Built by one person. No team, no funding, no whitepaper yet.

---

## The core idea

No miners. No fees. Every transaction does a small proof-of-work and references two previous transactions, implicitly confirming them. The network validates itself. Spam is controlled by dynamic PoW difficulty that adjusts based on load — not by making transactions expensive.

Privacy comes from two layers. Stealth addresses give every payment a unique one-time destination that only the recipient can identify. Pedersen commitments on Ristretto255 hide transaction amounts while still allowing the network to verify balance integrity. These two together hide who paid whom and how much, but they do not hide the graph itself — that is an open problem and the next privacy milestone.

Conflict resolution uses stake-weighted DAG weight with a hard cap on stake influence (max 3x multiplier, normalized against total network stake). This prevents the richest node from having unbounded power over disputes, while still giving honest long-term participants more weight than fresh Sybil nodes. Staking is influence, not a gate — transactions from unstaked nodes are valid; they simply carry less weight in disputes.

Parent selection uses a hybrid policy with two explicit parameters: `beta` (consensus bias, `0.0=random .. 1.0=greedy`) and `epsilon` (privacy noise, probability of replacing one parent with a decoy). This unifies what were previously two competing selection strategies and makes the privacy/convergence tradeoff explicit.

---

## What is actually built

**Rust workspace** (`ghost_core/`) — the production-path core:

```
crypto/           Ed25519, SHA-256, X25519 stealth addresses, Pedersen commitments
ledger/           DAG, state, mempool, validator, pruner, anti-spam, batch transactions,
                  Merkle state roots, checkpoint registry, privacy (decoy pool, diffusion),
                  parent_selection (ParentSelectionPolicy — unified β/ε hybrid)
consensus/        Stake-weighted conflict resolver (capped), tip selector, byzantine sim
branches/         Parallel DAG branches with quorum merge  [experimental, not on critical path]
network/          WebSocket P2P, gossip broadcast, peer discovery, eclipse attack detection
storage/          JSON snapshot persistence
token/            GHOST token, Proof of Uptime, StakingManager (stake/slash/eject/pool)
ghost-node/       Binary node — CLI, genesis, bootstrap, gossip, peer health
ghost-explorer    TUI block explorer (ratatui)
```

**Python workspace** (`app/`) — the original proof-of-concept. Slower but easier to reason about. Useful for testing protocol logic before porting to Rust.

**Test count:** 184 tests in the Rust workspace, all passing.

---

## How a transaction moves through the system

```
Wallet
  → dynamic PoW (difficulty auto-adjusts with network load)
  → stealth address generation (optional)
  → Pedersen commitment (optional, for amount privacy)
  → Ed25519 signature
  → Validator: structure, nonce, PoW, signature, state, commitment
  → Per-address rate limiter (anti-spam second layer)
  → Mempool
  → Consensus: stake-weighted conflict resolution, capped at 3x influence
  → DAG: cumulative weight propagation
  → Parent selection: ParentSelectionPolicy (β/ε hybrid — conflict-aware + privacy noise)
  → Merkle state root update
  → Snapshot persistence
  → Pruner: sliding window, fixed memory footprint
  → Gossip broadcast to random peer sample (diffusion delay for timing privacy)
```

---

## What is not done yet

Being honest about the gaps:

**Staking influences consensus but does not hard-gate it.** `token::StakingManager` is the single source of truth for stake. Staked nodes get a weight multiplier of `1.0..3.0` applied during conflict resolution and parent selection. There is no hard eligibility gate — transactions from unstaked nodes are valid; they simply carry less weight in disputes. This is intentional: a hard gate would break the `transactions produce consensus` model. Genesis nodes auto-stake `MIN_STAKE` at startup.

**Merkle roots exist but are not anchored to finality.** `merkle.rs` computes state roots and can verify snapshots. But the DAG does not yet store checkpoint hashes, and light sync still trusts the snapshot rather than verifying against a root chain. This is the next infrastructure milestone.

**Privacy stops at amount and address.** Stealth addresses and commitments are implemented and tested. But the transaction graph is still observable — parent links, timing, interaction patterns, and relay paths are all visible. The `parent_selection` module adds decoy parents and diffusion delay, but this is partial mitigation, not a full solution. Defending against graph-level deanonymization is the next privacy milestone.

**P2P is a working prototype.** WebSocket gossip, peer discovery, health checks, eclipse detection, and random sampling are all there. What is not there: QUIC transport, NAT traversal, and a real bootstrap network. "Working P2P prototype" is the accurate description, not "production P2P".

**No external security audit.** The internal review found and fixed several issues (DoS via message size, peer flooding, integer overflow, rate limiting). An external adversarial audit has not happened.

**Branches are a research module.** Parallel DAG branches with quorum merge are implemented and tested, but they are explicitly not on the canonical network path. Global state consistency across branches is an unsolved problem here and in the broader field. Branches are frozen as a future scaling paper, not a near-term feature.

---

## Known open problems

- **β/ε tradeoff is a design knob, not a theorem.** `ParentSelectionPolicy` has explicit `beta` (consensus bias) and `epsilon` (privacy noise) parameters. The tradeoff between convergence speed and graph privacy is real and measurable, but there is no formal proof of the optimal values. This is an open research question.
- **Honest Parent Selection Problem.** Privacy-motivated decoy parents weaken the convergence signal. This is inherent tension in the protocol, not a bug. Documented as an open problem for the whitepaper.
- **Corollary P is a proof-sketch only.** Liveness under network partition (Theorem P / PHA) has a working implementation and passing tests, but the formal liveness proof under arbitrary multi-partition scenarios is not complete.
- **Graph deanonymization.** Current privacy does not protect against timing and topology analysis. Decoy parents and diffusion delay reduce the signal but do not eliminate it.
- **State root anchoring.** Merkle roots need to be part of the DAG finality model.
- **Threat model.** No formal adversary model document yet — this is being written.

---

## GHOST token

Fixed supply of 21,000,000. Nodes earn GHOST for uptime (Proof of Uptime), with diminishing returns for continuous operation to discourage always-on server farming. Address cap at 21,000 GHOST (0.1% of supply). Halvening every 4 years.

Staking is required for full validator participation. The minimum stake is 1,000 GHOST. Slashing applies on provable misbehavior (double vote, conflicting transactions, reputation penalty). The goal is that token holdings represent economic commitment to the network, not just accumulated reward.

The staking framework is implemented and wired into conflict resolution. Genesis nodes automatically stake at startup. The remaining gap is anchoring stake state to the DAG finality model.

---

## Running it

Requires Rust 1.75+.

```bash
git clone https://github.com/AlexBil-rar/Token
cd Token/Rust/ghost_core

# Start a genesis node
cargo run --bin ghostledger -- --genesis --genesis-address <address> --port 9000

# Connect a second node
cargo run --bin ghostledger -- --port 9001 --peers ws://127.0.0.1:9000

# TUI explorer
cargo run --bin ghost-explorer -- ws://127.0.0.1:9000
```

Web explorer: open `explorer.html` and point it at any running node.

---

## Roadmap

- [x] Phase 1 — Rust binary node
- [x] Phase 2 — P2P prototype (WebSocket gossip, peer discovery, health checks, eclipse detection)
- [x] Phase 3 — Dynamic anti-spam (PoW difficulty + per-address rate limits)
- [x] Phase 4 — Stake-weighted conflict resolution with cap
- [ ] Phase 5 — Wallet app (Tauri + React)
- [x] Phase 6 — Block explorer (web + TUI)
- [x] Phase 7 — Amount privacy (Pedersen commitments on Ristretto255)
- [x] Phase 8 — Internal security review (rate limits, peer flooding, overflow, DoS)
- [x] Phase 8.5 — Merkle state roots, transaction batching, staking framework
- [x] Phase 9 — Wire staking into consensus path (multiplier model, genesis auto-stake)
- [x] Phase 9.5 — Unified parent selection (ParentSelectionPolicy β/ε hybrid)
- [ ] Phase 10 — Graph privacy (decoy parents formalized, diffusion smoothing)
- [ ] Phase 11 — Threat model document + whitepaper v0.3
- [ ] Phase 12 — Testnet (3 nodes, different regions)
- [ ] Phase 13 — External security audit
- [ ] Phase 14 — Mainnet

---

## Why one person

Not because a team would be worse, but because working alone forces every architectural decision to be explainable to yourself. There is no one to hand-wave at. If something is vague, it stays broken until it is understood. This has been a useful constraint.

The downside is that blind spots are harder to catch. That is why the code is public and criticism is genuinely welcome.

---

## License

MIT
