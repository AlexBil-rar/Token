# GhostLedger Network Protocol Specification

Version: 0.1  
Status: Draft

---

## 1. Overview

GhostLedger uses a WebSocket-based P2P protocol. All messages are JSON-encoded at the application layer. Binary wire format (bincode) is available via `ghost-wire` crate for high-performance paths.

---

## 2. Wire Format

Every binary-encoded message starts with a 5-byte header:
```
[0..4] MAGIC = 0x47 0x48 0x53 0x54 ("GHST")
[4]    VERSION = 0x01
[5..]  bincode-serialized payload
```

JSON fallback is used for WebSocket messages during the current alpha phase.

---

## 3. Message Types

| Type | Direction | Description |
|------|-----------|-------------|
| `ping` | any → any | Liveness check |
| `pong` | any → any | Liveness response |
| `transaction` | any → any | Submit or relay transaction |
| `state_request` | peer → node | Request ledger state |
| `state_response` | node → peer | Ledger balances + state root |
| `peer_list` | any → any | Exchange known peers |
| `difficulty_request` | peer → node | Request current PoW difficulty |
| `difficulty_response` | node → peer | Current difficulty value |
| `explorer_request` | client → node | Request DAG stats + recent txs |
| `explorer_response` | node → client | DAG stats + recent txs |
| `checkpoint_request` | peer → node | Request latest checkpoint |
| `checkpoint_response` | node → peer | Checkpoint metadata |
| `partition_handshake` | node → node | PHA Step 1: announce checkpoint |
| `partition_handshake_ack` | node → node | PHA Step 2: agree on cp* |
| `partition_sync_request` | node → node | PHA Step 3: request txs above cp* |
| `partition_sync_response` | node → node | PHA Step 4: send txs above cp* |

---

## 4. Message Envelope
```json
{
  "type": "<message_type>",
  "payload": { ... },
  "timestamp": 1700000000.0,
  "sender": "<node_address>"
}
```

---

## 5. Transaction Format
```json
{
  "sender": "<40-char hex address>",
  "receiver": "<40-char hex address>",
  "amount": 100,
  "nonce": 1,
  "timestamp": 1700000000,
  "public_key": "<64-char hex ed25519 pubkey>",
  "parents": ["<tx_id>", "<tx_id>"],
  "signature": "<128-char hex ed25519 signature>",
  "anti_spam_nonce": 12345,
  "anti_spam_hash": "<64-char hex sha256>",
  "commitment": "<64-char hex Ristretto point> | null",
  "balance_proof": "<JSON serialized BalanceProof> | null",
  "range_proof": "<JSON serialized RangeProof> | null",
  "excess_commitment": "<64-char hex> | null",
  "excess_signature": "<64-char hex> | null",
  "stem_ttl": 0,
  "range_proof_status": "Missing | Experimental | Verified"
}
```

---

## 6. Peer Handshake
```
Client                          Node
  |                              |
  |------- ping ---------------→|
  |←------ pong ----------------|
  |                              |
  |------- peer_list {req} ----→|
  |←------ peer_list {peers} ---|
  |                              |
  |------- checkpoint_request -→|
  |←------ checkpoint_response -|
```

---

## 7. Transaction Submission
```
Client                          Node
  |                              |
  |------- transaction --------→|  validate → DAG → gossip
  |←------ {ok, code, reason} --|
```

**Validation steps:**
1. Structure (sender, receiver, amount, nonce, parents)
2. Duplicate check
3. Parent existence in DAG
4. Ed25519 signature
5. Anti-spam PoW
6. Balance check (readonly state)
7. Privacy mode (commitment required if privacy_by_default)
8. Balance proof (if confidential)
9. Excess commitment (if confidential)
10. Range proof (if confidential)

---

## 8. Dandelion++ Routing

Transactions are relayed using Dandelion++ protocol:

- **Stem phase** (≈20% of txs): forward to single random peer, decrement `stem_ttl`
- **Fluff phase** (≈80% of txs): broadcast to all peers
- `stem_ttl` starts at 10, decrements each hop
- TTL exhaustion → automatic fallback to fluff
```
stem_ttl > 1:   tx → single_peer (stem)
stem_ttl == 1:  tx → all_peers  (fluff fallback)
stem_ttl == 0:  phase determined by tx_id entropy
```

---

## 9. Partition Healing Algorithm (PHA)

Used when network partitions are detected:
```
Node A                          Node B
  |                              |
  |-- partition_handshake -----→|  announce my latest finalized cp
  |←- partition_handshake_ack --|  agree on common cp* 
  |                              |
  |-- partition_sync_request --→|  request txs above cp*
  |←- partition_sync_response --|  receive txs
  |                              |
  [re-evaluate conflicts above cp* using frozen stake]
  [globally closed → ClosedGlobal]
  [not dominant → back to Ready]
```

**Invariant G:** Conflicts below cp* are never downgraded.  
**Theorem P:** Both nodes converge to same global winner.

---

## 10. Consensus Parameters

| Parameter | Value | Description |
|-----------|-------|-------------|
| `BETA` | 0.7 | Parent selection bias toward heavy tips |
| `EPSILON` | 0.10 | Decoy parent probability (default) |
| `EPSILON_PRIVACY` | 0.20 | Decoy parent probability (privacy mode) |
| `SIGMA` | 2.0 | Conflict closure dominance threshold |
| `THETA` | 6 | Min weight for checkpoint finalization |
| `RESOLVE_MIN_WEIGHT` | 3 | Min weight for conflict resolution |
| `MIN_STAKE` | 1000 | Minimum validator stake (GHOST) |
| `STEM_MAX_TTL` | 10 | Maximum Dandelion stem hops |

---

## 11. Cryptography

| Primitive | Usage |
|-----------|-------|
| Ed25519 | Transaction signing |
| X25519 | Stealth address ECDH |
| Ristretto255 | Pedersen commitments |
| SHA-256 | tx_id, anti-spam PoW, Merkle tree |
| SHA-512 | H point derivation for commitments |
| Bulletproofs | Range proofs (64-bit) |

---

## 12. Anti-Spam PoW

SHA-256 based proof of work:
```
hash = SHA256(sender || receiver || amount || nonce || timestamp || 
              public_key || parents || anti_spam_nonce || ephemeral_pubkey)
```

Hash must start with `difficulty` leading zero hex chars.  
Difficulty adjusts automatically: increases above 10 TPS, decreases below 2 TPS.  
Range: `[2, 6]` leading zeros.

---

## 13. State Root

Merkle tree over ledger state:
```
leaf = SHA256(address || ":" || balance || ":" || nonce)
tree = binary Merkle tree, leaves sorted by address
root = hex(tree_root)
```

Empty state root: `SHA256("ghostledger:empty_state")`