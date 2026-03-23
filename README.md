# GhostLedger

**Feeless. Anonymous. User-powered.**

> An experimental DAG-based ledger exploring an architecture where transactions are free, private by design, and validated by the users themselves — no miners, no gas fees, no middlemen.

---

## Why this exists

Every major cryptocurrency forces you to pay to move your own money.

Bitcoin fees spike during congestion. Ethereum gas can exceed the value of the transaction. Even "cheap" chains have fees that add up. And beyond fees, most blockchains are pseudonymous at best — your address is public, your transaction graph is public, and with enough data analysis you can be identified.

GhostLedger is built around one question: *can feeless + anonymous + decentralized coexist in a single architecture?*

This project is my attempt to find out.

---

## How it works

### No miners. No fees.

Instead of a blockchain where miners compete to add blocks, GhostLedger uses a **DAG (Directed Acyclic Graph)**. Every transaction is a vertex. To submit one, you must reference and implicitly confirm two previous transactions. The network validates itself.

Spam is handled not by fees but by **dynamic Proof of Work** — a lightweight computation that adjusts difficulty in real time based on network load. High traffic → higher difficulty. Low traffic → near-instant. The cost is milliseconds of CPU, not money.

```
Your transaction
  → lightweight PoW (auto-difficulty)
  → references 2 parent transactions
  → confirmed by future transactions building on top
```

### Stealth addresses

Every payment goes to a unique one-time address. The sender generates a fresh address per payment using the recipient's public key. Only the recipient can discover that the payment belongs to them.

```
Bob publishes:    spend_pubkey  (once, publicly)

Alice pays Bob:
  1. Generates ephemeral key r
  2. Computes stealth_address = ECDH(r, spend_pubkey)
  3. Publishes ephemeral_pubkey R in the transaction

Bob scans DAG:
  stealth_address = ECDH(spend_privkey, R)
  If match → this payment is mine
```

An outside observer sees only a random address and an ephemeral key. Without Bob's private key, the payment cannot be linked to him.

### Pedersen Commitments (amount privacy)

Transaction amounts can be hidden using **Pedersen commitments** on Ristretto255:

```
C = r·G + amount·H
```

The network can verify that inputs balance outputs without learning the actual amounts. Two modes are supported — transparent (amount visible) and private (commitment only), similar to how Zcash handles t/z addresses.

### Stake-weighted conflict resolution

Double-spends and conflicts are resolved by **stake-weighted DAG weight**:

```
score = dag_weight × (1 + stake)
```

Nodes with more stake have more say in conflict resolution. Ties are broken deterministically by transaction ID — no randomness, no timestamp manipulation.

### Branch architecture *(experimental)*

The network is split into independent **branches**, each processing its own subset of transactions in parallel. A quorum-based coordinator periodically merges branch states — a value is accepted only if the majority agree. This is an experimental feature; global state consistency across branches is an open research problem and the hardest part of this design.

---

## Architecture

```
Rust workspace (production core)
ghost_core/
├── crypto/        Ed25519 wallets, SHA-256, stealth addresses, Pedersen commitments
├── ledger/        Transaction model, DAG, state, mempool, validator, pruner, anti-spam
├── consensus/     Stake-weighted conflict resolver, tip selector
├── branches/      Branch, quorum coordinator, branch manager  [experimental]
├── network/       WebSocket P2P, gossip broadcast, peer discovery, peer reputation
├── storage/       JSON snapshot persistence
├── token/         GHOST token, Proof of Uptime, halvening, staking
├── ghost-node/    Binary node — CLI, genesis, bootstrap, node runner
└── ghost-explorer TUI block explorer (ratatui)

Python workspace (research prototype)
app/
├── crypto/        wallet.py, hashing.py, stealth.py
├── ledger/        transaction.py, dag.py, state.py, mempool.py, validator.py, node.py, pruner.py
├── consensus/     engine.py, conflict_resolver.py, tip_selector.py
├── branches/      branch.py, coordinator.py, branch_manager.py
├── network/       server.py, client.py, peer_list.py, peer_reputation.py, ws_*.py
├── storage/       snapshot.py
└── token/         ghost.py, staking.py
```

---

## Transaction pipeline

```
Wallet
  → anti-spam PoW (dynamic difficulty)
  → stealth address or Pedersen commitment (optional)
  → Ed25519 signature
  → Validator (structure, nonce, PoW, signature, state, commitment)
  → Mempool
  → Consensus engine (stake-weighted conflict resolution)
  → DAG (cumulative weight propagation)
  → Snapshot (persistent storage)
  → Pruner (memory management)
  → Gossip broadcast to peers
```

---

## Security model

| Threat | Mitigation |
|--------|------------|
| Replay attacks | Per-sender nonce, enforced in validator |
| Spam / DoS | Dynamic PoW, 1MB WS message limit |
| Sybil attacks | PoW registration + peer reputation + behaviour analysis |
| Peer flooding | MAX_PEERS=128 cap in peer list |
| Double-spend | Stake-weighted conflict resolution, deterministic tiebreaker |
| Arithmetic overflow | `saturating_sub/add` throughout state.rs |
| Amount tracing | Pedersen commitments (optional private mode) |
| Address linkability | Stealth addresses (one-time per payment) |

---

## GHOST Token

- **Total supply:** 21,000,000 GHOST (fixed)
- **Distribution:** Proof of Uptime — nodes earn GHOST for being online and serving the network
- **Anti-whale:** Address cap at 21,000 GHOST (0.1% of supply). Diminishing returns on continuous uptime
- **Halvening:** Rewards halve every 4 years
- **Transfers:** Always free, regardless of token holdings

```
First 24h online:       100% reward rate
24–72h continuous:       50% reward rate
72h–1 week continuous:   25% reward rate
Over 1 week continuous:  10% reward rate
```

This makes it rational to run many small nodes rather than one always-on server — better decentralization by design.

---

## Running a node

**Requirements:** Rust 1.75+

```bash
git clone https://github.com/AlexBil-rar/Token
cd Token/Rust/ghost_core
```

```bash
# Genesis node
cargo run --bin ghostledger -- --genesis --genesis-address <address> --port 9000

# Connect a second node
cargo run --bin ghostledger -- --port 9001 --peers ws://127.0.0.1:9000

# TUI block explorer
cargo run --bin ghost-explorer -- ws://127.0.0.1:9000
```

The web explorer (`explorer.html`) connects to any running node via WebSocket.

---

## Tests

```
Python:  158 tests  ✅
Rust:    174 tests  ✅
─────────────────────
Total:   332 tests
```

Coverage: DAG, validator, consensus, stealth addresses, Pedersen commitments, quorum, pruner, token, P2P reputation, anti-spam, branches.

```bash
# Python
pytest tests/ -v

# Rust
cargo test --workspace
```

---

## Compared to existing work

| Project | Feeless | Privacy | DAG | No miners |
|---------|---------|---------|-----|-----------|
| Bitcoin | ✗ | Partial | ✗ | ✗ |
| Monero | ✗ | ✓ | ✗ | ✗ |
| Nano | ✓ | ✗ | ✓ | ✓ |
| IOTA | ✓ | ✗ | ✓ | ✓ |
| **GhostLedger** | **✓** | **✓** | **✓** | **✓** |

Nano and IOTA solve feeless DAG. Monero solves privacy. GhostLedger explores combining all of them in one architecture — and whether the tradeoffs are manageable.

---

## Roadmap

- [x] Phase 1 — Rust binary node
- [x] Phase 2 — Real P2P (gossip, peer discovery, health checks)
- [x] Phase 3 — Dynamic anti-spam (auto-adjusting PoW difficulty)
- [x] Phase 4 — Stake-weighted conflict resolution
- [ ] Phase 5 — Wallet app (Tauri + React)
- [x] Phase 6 — Block explorer (web + TUI)
- [x] Phase 7 — Amount privacy (Pedersen commitments)
- [x] Phase 8 — Security audit
- [ ] Phase 9 — Testnet (3 nodes, different regions)
- [ ] Phase 10 — Mainnet

---

## Status

Experimental research project. Working MVP with P2P network, stealth addresses, Pedersen commitments, stake-weighted consensus, and token economics. Not audited. Not production-ready.

Built solo. Contributions and criticism welcome.

---

## License

MIT
