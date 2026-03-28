# GhostLedger Partition Healing Algorithm — Protocol Specification

**Version:** 0.1  
**Date:** March 2026  
**Status:** Executable specification — not a formal proof

---

## 1. Purpose

This document specifies the Partition Healing Algorithm (PHA) as an **executable protocol**, not a mathematical sketch. It defines inputs, preconditions, state transitions, invariants, and failure cases with enough precision to drive both implementation and test generation.

The PHA solves the following problem: two nodes A and B have been partitioned and independently resolved the same conflict C with potentially different winners. When the partition heals, they must converge to a single globally consistent winner — deterministically, without a coordinator.

---

## 2. Definitions

```
Node        := a GhostLedger full node with a local DAG and conflict resolver
DAG(N)      := the DAG held by node N at time t
CR(N)       := the ConflictResolver state of node N
CP(N)       := the latest finalized checkpoint known to node N
cp*         := the common checkpoint agreed upon in Step 1
```

**Finalized checkpoint:** a `CheckpointVertex` with `weight ≥ THETA = 6`.

**Above cp*:** a transaction T is "above cp*" if T ∈ descendants(cp*) in DAG(N).

**Below cp*:** a transaction T is "below cp*" if T is an ancestor of cp* or is cp* itself.

**ConflictStatus states:**
```
Pending → Ready → ClosedLocal → Reconciling → ClosedGlobal
                      ↑________________|
```

---

## 3. Preconditions

PHA may only be initiated when ALL of the following hold:

1. Node A and node B have re-established a network connection after a partition.
2. Both nodes have at least one finalized checkpoint (`CP(A) ≠ None`, `CP(B) ≠ None`).
3. Neither node is currently executing PHA with a third node for the same cp*.

If any precondition fails, PHA is deferred until conditions are met.

---

## 4. Algorithm

### Step 1 — Handshake

**Inputs:**
- `A.checkpoint_id`: latest finalized checkpoint ID known to A
- `A.sequence`: sequence number of A's latest finalized checkpoint
- `B.checkpoint_id`: latest finalized checkpoint ID known to B
- `B.sequence`: sequence number of B's latest finalized checkpoint

**Process:**
```
if B.sequence ≤ A.sequence:
    if B.checkpoint_id ∈ A.checkpoint_registry AND is_finalized:
        cp* = B.checkpoint_id
    else:
        cp* = A.latest_finalized.checkpoint_id
else:
    cp* = A.latest_finalized.checkpoint_id
```

**Output:** `cp*` — the agreed common checkpoint

**Invariant (Step 1):** `cp*` is finalized on both A and B before proceeding.

**Failure case:** If neither node has a common finalized checkpoint, PHA cannot proceed. Nodes exchange their full checkpoint registries and retry after the next checkpoint finalizes.

---

### Step 2 — Invariant G Enforcement

**Statement of Invariant G:**
> Any conflict whose all transactions are ancestors of cp* (i.e., the entire conflict set is below cp*) must NOT be modified by PHA. Its current status is preserved as-is.

**Process:**
```
for each conflict C in CR(A):
    if all(tx ∈ ancestors(cp*) for tx in C.tx_ids):
        skip C  // below cp* — immutable
    else:
        C is eligible for PHA processing
```

**Why this is safe:** Transactions below cp* have accumulated at least THETA=6 confirmations from both sides of the partition. Their resolution is already stable and globally consistent by construction of the checkpoint finalization rule.

---

### Step 3 — Downgrade

**Inputs:** all conflicts in CR(A) with status `ClosedLocal` that are above cp*

**Process:**
```
for each conflict C in CR(A):
    if C.status == ClosedLocal:
        if any(tx ∈ descendants(cp*) for tx in C.tx_ids):
            C.status = Reconciling
            // frozen_stake preserved from original ClosedLocal transition
```

**Output:** set of conflicts now in `Reconciling` state

**Invariant (Step 3):** `frozen_stake` is the stake snapshot at the time of the original `ClosedLocal` transition — it is NOT updated to current stake. This prevents stake manipulation attacks where an adversary moves stake after the partition to influence PHA outcome.

**Failure case:** If `frozen_stake` is None (conflict was closed without a checkpoint anchor — should not happen), treat as `Pending` and reset to `Ready` after sync.

---

### Step 4 — Sync

**Process:**
```
A → B: send all transactions T where T ∈ descendants(cp*) in DAG(A)
B → A: send all transactions T where T ∈ descendants(cp*) in DAG(B)
```

For each received transaction T:
```
if T.tx_id ∉ local_dag:
    validate_structure(T)  // structural validation only, no state checks
    if valid:
        dag.add(T)
        dag.propagate_weight(T.tx_id)
        cr.register(T)
```

**Output:** Both DAG(A) and DAG(B) now contain the union of all transactions above cp*.

**Invariant (Step 4):** After sync, for all transactions T above cp*:
```
T ∈ DAG(A) ⟺ T ∈ DAG(B)
```

**Failure case:** If a received transaction fails structural validation, it is discarded (not added). A missing transaction does not block PHA — the conflict will fall back to `Ready` in Step 6 if insufficient weight.

**Note on Invariant G:** Step 4 only syncs transactions above cp*. Transactions below cp* are not requested and not sent.

---

### Step 5 — Re-evaluate

For each conflict C in `Reconciling` state:

**Inputs:**
- `C.tx_ids`: the set of conflicting transaction IDs
- `C.frozen_stake`: the stake snapshot at original ClosedLocal time
- `C.frozen_total_stake`: total stake at original ClosedLocal time
- Current DAG weights (updated by Step 4 sync)

**Closure predicate (applied with frozen stake):**
```
scores = { tx_id: W(tx) × multiplier(tx.sender, frozen_stake, frozen_total_stake)
           for tx_id in C.tx_ids }

winner = argmax(scores)
winner_score = scores[winner]
second_score = max(scores[tx] for tx in C.tx_ids if tx ≠ winner)

dominant = (second_score == 0) OR (winner_score ≥ SIGMA × second_score)
ready    = all(W(tx) ≥ RESOLVE_MIN_WEIGHT for tx in C.tx_ids)
```

**Transition:**
```
if ready AND dominant:
    C.status = ClosedGlobal
    C.winner = winner
    C.global_anchor = cp*
else:
    C.status = Ready  // wait for more weight
```

---

### Step 6 — Close

After Step 5:
- Conflicts that transitioned to `ClosedGlobal`: their winner is final. No further modification.
- Conflicts that fell back to `Ready`: normal conflict resolution resumes. The conflict will close locally again when sufficient weight accumulates.

**Post-condition:**
```
∀ conflict C: C.status ∈ {ClosedGlobal, Ready, Pending}
// No conflicts remain in Reconciling after PHA completes
```

---

## 5. Theorem P (Safety)

**Theorem P.** If nodes A and B complete PHA with the same cp*, then for any conflict C that transitions to `ClosedGlobal` on both nodes, `winner(C, A) = winner(C, B)`.

**Proof.**
- After Step 4: `DAG(A) above cp* = DAG(B) above cp*` (same transaction set, same weights)
- Step 5 applies a deterministic function `f(DAG, frozen_stake, C.tx_ids)` to identical inputs
- Deterministic function on identical inputs produces identical output
- Therefore `winner(C, A) = winner(C, B)` □

---

## 6. Invariants Summary

| # | Invariant | Statement |
|---|-----------|-----------|
| G | Immutability below cp* | Conflicts fully below cp* are never touched by PHA |
| S | Frozen stake | Re-evaluation uses stake at ClosedLocal time, not current stake |
| D | DAG equality | After Step 4, both nodes have identical DAG above cp* |
| T | Terminal state | ClosedGlobal is a terminal state — no transitions out |
| P | Safety | Same DAG + same frozen_stake → same winner (Theorem P) |

---

## 7. Failure Cases and Edge Conditions

### 7.1 No common finalized checkpoint

**Condition:** CP(A) and CP(B) have no checkpoint in common.

**Handling:** PHA deferred. Nodes continue normal operation and retry PHA after the next checkpoint finalizes on both sides.

**Note:** This can happen if the partition occurred before either node had finalized a checkpoint. In this case, all conflicts are treated as `Pending` and resolved normally via weight accumulation.

---

### 7.2 Transaction validation failure during sync

**Condition:** A transaction received in Step 4 fails structural validation.

**Handling:** Discard the transaction. Do not abort PHA. If this causes a conflict to lack sufficient weight for closure in Step 5, it falls back to `Ready`.

---

### 7.3 Stale peer — one node is far behind

**Condition:** Node B's latest checkpoint has sequence N; Node A's latest is sequence N+10.

**Handling:** cp* = A's checkpoint at sequence N (the most recent one B has finalized). A sends B all transactions above sequence-N checkpoint. B must process a large sync payload.

**Limit:** If the sync payload exceeds `MAX_WIRE_PAYLOAD = 1MB`, B requests transactions in batches. PHA completes in multiple rounds.

---

### 7.4 Three-way partition (A, B, C)

**Condition:** Network splits into three components A, B, C simultaneously.

**Handling:** PHA is defined for **pairwise** reconciliation. When the partition heals:
1. A and B run PHA → both reach consistent state A∪B
2. (A∪B) and C run PHA → all three reach consistent state A∪B∪C

**Known limitation:** The order of pairwise reconciliations can affect intermediate states. The final state is guaranteed consistent only if Theorem P holds for each pairwise execution. The multi-way case is not formally analyzed and is an open problem.

---

### 7.5 Repeated split/merge

**Condition:** Network splits, heals (PHA runs), splits again before all conflicts close globally.

**Handling:** Each partition cycle runs PHA independently. A conflict that was downgraded to `Reconciling` in one cycle and fell back to `Ready` can be downgraded again in the next cycle. There is no bound on the number of times a conflict can cycle through `ClosedLocal → Reconciling → Ready`. The conflict closes globally when:
1. A stable cp* is agreed upon
2. Sufficient weight accumulates
3. σ-dominance is achieved

---

### 7.6 Delayed checkpoint arrival

**Condition:** A checkpoint has been broadcast but not yet received by one node when PHA initiates.

**Handling:** PHA uses only **locally finalized** checkpoints (weight ≥ THETA=6 in the local registry). A checkpoint that is "in transit" is not used as cp*. The handshake in Step 1 uses the most recent locally confirmed cp.

---

## 8. Test Requirements

The following test scenarios must pass for PHA to be considered correct:

| # | Scenario | Expected outcome |
|---|----------|-----------------|
| T1 | 2-way partition, 1 conflict, A wins | Both nodes: winner = A's tx |
| T2 | 2-way partition, 1 conflict, different local winners | Both nodes: same global winner after re-eval |
| T3 | 2-way partition, conflict not dominant after sync | Both nodes: conflict → Ready |
| T4 | Partition below cp* — conflict already ClosedLocal below boundary | Not touched by PHA |
| T5 | Stale peer — B has no checkpoints | PHA deferred; normal resolution continues |
| T6 | Large sync payload (>100 transactions) | PHA completes correctly |
| T7 | Repeated split/merge (3 cycles) | Eventual global closure |
| T8 | 3-way partition (pairwise) | After A∪B and (A∪B)∪C: same winner on all |

*T8 is currently not formally verified — see Section 7.4.*

---

## 9. Wire Messages

PHA uses the following message types (defined in `network/src/ws_message.rs`):

| Message | Direction | Content |
|---------|-----------|---------|
| `partition_handshake` | A → B | `{checkpoint_id, dag_height, sequence}` |
| `partition_handshake_ack` | B → A | `{common_checkpoint_id, common_sequence, ready_to_sync}` |
| `partition_sync_request` | A → B | `{above_checkpoint_id}` |
| `partition_sync_response` | B → A | `{checkpoint_id, transactions[], tx_count}` |

All messages use the standard `WsMessage` envelope with `timestamp` and `sender` fields.

---

*GhostLedger PHA Protocol Specification v0.1 — March 2026*  
*Executable specification — not a formal proof.*
