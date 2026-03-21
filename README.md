# GhostLedger

**Feeless. Anonymous. User-powered.**

GhostLedger is an experimental DAG-based ledger where transactions are free, anonymous by design, and validated by the users themselves — no miners, no gas fees, no middlemen.

---

## The Problem

Every major cryptocurrency today forces you to pay to move your own money.

- Bitcoin charges fees that spike during congestion
- Ethereum gas prices can exceed the value of the transaction itself
- Even "cheap" chains have fees that add up over time

Beyond fees, most blockchains are pseudonymous at best. Your address is public. Your transaction graph is public. With enough data, you can be identified.

**You shouldn't have to pay to send your own money. And no one should be able to trace where it goes.**

---

## The Solution

GhostLedger is built around one hypothesis: *can you build a crypto network where users create, validate, and propagate transactions themselves — and privacy and spam protection are solved architecturally, not by fees?*

The answer is yes. Here's how.

---

## How It Works

### No miners. No fees.

Instead of a blockchain where miners compete to add blocks, GhostLedger uses a **DAG (Directed Acyclic Graph)**. Every transaction is a vertex in the graph. To submit a transaction, you must reference and implicitly confirm two previous transactions. The network validates itself.

Spam is prevented not by fees but by **lightweight Proof of Work** — each transaction requires a small computation that makes mass spam expensive without making normal use costly.

```
Your transaction → lightweight PoW → references 2 parents → confirmed by future transactions
```

### Anonymous by design.

GhostLedger implements **stealth addresses** — a cryptographic technique where every payment goes to a unique one-time address. The sender generates a fresh address for each payment using the recipient's public key. Only the recipient can discover that the payment belongs to them.

```
Bob publishes:    spend_pubkey  (once, publicly)

Alice pays Bob:
  1. Generates random ephemeral key r
  2. Computes stealth_address = ECDH(r, spend_pubkey)
  3. Publishes ephemeral_pubkey R in the transaction

Bob scans DAG:
  stealth_address = ECDH(spend_privkey, R)
  If match → this payment is mine
```

An observer sees only a random address and an ephemeral key. Without Bob's private key, the payment cannot be linked to him.

### Cumulative weight consensus.

There are no block confirmations. Instead, each transaction accumulates **cumulative weight** as future transactions reference it. The more transactions built on top of yours, the more confirmed it is. This is O(1) per transaction and scales naturally with network activity.

### Branch architecture with quorum voting.

The network is split into independent **branches**, each processing its own subset of transactions in parallel. A **quorum-based coordinator** periodically merges branch states — a value is accepted only if the majority of branches agree on it. No single point of failure.

---

## Key Features

| Feature | Description |
|---|---|
| Feeless | No gas, no mining fees. Ever. |
| Stealth addresses | One-time addresses per payment. Untrackable. |
| DAG structure | Transactions confirm each other. No blocks. |
| Anti-spam PoW | Lightweight — costs milliseconds, not money |
| Ed25519 signatures | Production-grade asymmetric cryptography |
| P2P gossip network | Transactions broadcast to all peers automatically |
| Quorum consensus | Majority voting. No central authority. |
| Pruning | Nodes store only recent transactions + current state. Fixed memory footprint. |
| Persistent snapshots | Node state survives restarts. No history loss. |
| GHOST token | 21M supply. Proof of Uptime rewards. Anti-whale mechanics. |
| Sybil resistance | PoW registration + reputation + behaviour analysis |

---

## GHOST Token

The native token of the network is **GHOST**.

- **Total supply:** 21,000,000 GHOST (fixed, like Bitcoin)
- **Distribution:** Proof of Uptime — nodes earn GHOST for being online
- **Anti-whale:** Diminishing returns on continuous uptime. Address cap at 0.1% of supply (21,000 GHOST max per address)
- **Halvening:** Rewards halve every 4 years
- **Purpose:** Governance and network participation. Transfers are always free regardless of token holdings.

```
Fresh node (first 24h):    100% reward rate
24–72 hours continuous:     50% reward rate
72h–1 week continuous:      25% reward rate
Over 1 week continuous:     10% reward rate
```

This makes it economically rational to have many small nodes rather than one large always-on server.

---

## Architecture

```
app/
├── crypto/
│   ├── wallet.py          Ed25519 key generation and signing
│   ├── hashing.py         SHA-256 utilities
│   └── stealth.py         Stealth address generation and scanning
├── ledger/
│   ├── transaction.py     Transaction vertex model
│   ├── dag.py             DAG graph, tips, cumulative weight
│   ├── state.py           Balances, nonces, applied transactions
│   ├── mempool.py         Pre-commit transaction buffer
│   ├── validator.py       Structure, signature, PoW, state validation
│   ├── node.py            Node orchestrator
│   └── pruner.py          DAG pruning — recent window strategy
├── consensus/
│   ├── engine.py          Mempool commit, conflict resolution
│   ├── conflict_resolver.py  Double-spend detection by sender+nonce
│   └── tip_selector.py    Weighted random tip selection
├── branches/
│   ├── branch.py          Independent DAG branch
│   ├── coordinator.py     Quorum-based state merge
│   └── branch_manager.py  Transaction routing across branches
├── network/
│   ├── server.py          FastAPI HTTP node server
│   ├── client.py          Gossip broadcast client
│   ├── peer_list.py       Known peers registry
│   └── peer_reputation.py Sybil resistance — PoW + reputation + behaviour
├── storage/
│   └── snapshot.py        JSON snapshot persistence
└── token/
    └── ghost.py           GHOST token — Proof of Uptime, halvening, anti-whale
```

---

## Transaction Pipeline

```
Wallet
  → anti-spam PoW
  → stealth address (optional)
  → Ed25519 signature
  → Validator (structure, nonce, PoW, signature, state)
  → Mempool
  → Consensus Engine (conflict check, state apply)
  → DAG (cumulative weight propagation)
  → Snapshot (persistent storage)
  → Pruner (memory management)
  → Gossip broadcast to peers
```

---

## Getting Started

**Requirements:** Python 3.11+

```bash
git clone https://github.com/AlexBil-rar/Token
cd Token
pip install fastapi uvicorn httpx PyNaCl cryptography pytest
```

**Run two local nodes:**

```bash
# Terminal 1
python3 run_node.py 8000

# Terminal 2
python3 run_node.py 8001
```

**Send a transaction:**

```bash
python3 send_tx.py <address> <private_key>
```

**Run tests:**

```bash
pytest tests/ -v
```

124 tests covering DAG, validator, consensus, stealth addresses, quorum, pruning, token, and P2P reputation.

---

## Compared to Existing Projects

| Project | Feeless | Anonymous | DAG | No miners |
|---|---|---|---|---|
| Bitcoin | ✗ | Partial | ✗ | ✗ |
| Monero | ✗ | ✓ | ✗ | ✗ |
| Nano | ✓ | ✗ | ✓ | ✓ |
| IOTA | ✓ | ✗ | ✓ | ✓ |
| **GhostLedger** | **✓** | **✓** | **✓** | **✓** |

GhostLedger is the only project combining all four properties in a single architecture.

---

## Technology Stack

- **Python 3.11+** — research and prototyping
- **PyNaCl** — Ed25519 signatures (same primitive as Solana, Cardano)
- **cryptography** — X25519 ECDH for stealth addresses
- **FastAPI + uvicorn** — P2P HTTP node server
- **httpx** — async gossip broadcast
- **pytest** — 124 tests

Future: core rewrite in **Rust** for production performance.

---

## Status

This is an **experimental research project**. The goal is to prove the hypothesis that feeless + anonymous + decentralized can coexist in one architecture.

Current state: working MVP with P2P network, stealth addresses, quorum consensus, and token economics. Not audited. Not production-ready. Contributions and ideas welcome.

---

## License

MIT
