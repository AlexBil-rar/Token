# GhostLedger Token Economics

**Version:** 0.1  
**Date:** March 2026  
**Author:** Aleksandr Bilyk  
**Status:** Research prototype — subject to change

---

## 1. Overview

GhostLedger uses a single native token, **GHOST**, for three purposes:

1. **Validator collateral** — stake to participate in consensus weight
2. **Economic deterrent** — slashing penalizes malicious behavior
3. **Network reward** — emission incentivizes honest operation

The model is intentionally minimal. No governance tokens, no LP incentives, no multi-tier reward classes. The goal is a formally defined, auditable baseline.

---

## 2. Supply

| Parameter | Value |
|-----------|-------|
| Total supply (hard cap) | 21,000,000 GHOST |
| Genesis allocation | 2,100,000 GHOST (10% of total) |
| Emission pool | 18,900,000 GHOST (90% of total) |
| Smallest unit | 1 (integer, no decimals in v0.1) |

The genesis allocation is distributed to the genesis address at launch. The emission pool is distributed over time via validator rewards.

---

## 3. Emission Curve

Emission follows a halvening schedule modeled after Bitcoin, adjusted for the DAG-based continuous reward model.

### 3.1 Base reward

```
base_reward_per_hour = 10 GHOST
```

### 3.2 Halvening

```
halvening_interval = 4 years (= 4 × 365 × 24 × 3600 seconds)

halvening_multiplier(t) = 1 / 2^floor((t - t_genesis) / halvening_interval)
```

| Period | Multiplier | Reward/hour |
|--------|-----------|-------------|
| Year 0–4 | 1.0 | 10 GHOST |
| Year 4–8 | 0.5 | 5 GHOST |
| Year 8–12 | 0.25 | 2.5 GHOST |
| Year 12–16 | 0.125 | 1.25 GHOST |
| ... | ... | ... |

### 3.3 Uptime multiplier

Continuous uptime increases reward rate:

| Continuous uptime | Multiplier |
|-------------------|-----------|
| ≤ 24 hours | 1.00 |
| 24–72 hours | 0.50 |
| 72–168 hours | 0.25 |
| > 168 hours | 0.10 |

*Note: the uptime multiplier decreases with time to incentivize regular node restarts (software updates) rather than indefinitely running stale nodes.*

### 3.4 Per-address cap

```
address_cap = 0.1% of total supply = 21,000 GHOST
```

No single address can accumulate more than 21,000 GHOST via emission rewards. This prevents whale accumulation through uptime alone.

### 3.5 Total emission bound

```
Σ rewards ≤ 18,900,000 GHOST
```

Emission stops when total minted reaches 21,000,000 GHOST (genesis + rewards).

---

## 4. Validator Economy

### 4.1 Eligibility

A node is eligible for validator status if:

```
stake(node) ≥ MIN_STAKE = 1,000 GHOST
status = Active (not Slashed or Ejected)
```

Nodes below `MIN_STAKE / 2 = 500 GHOST` are relay-only (can gossip but not influence consensus weight).

### 4.2 Consensus influence

Validator influence on conflict resolution is stake-weighted:

```
score(tx) = W(tx) × multiplier(sender)

multiplier(addr) = 1 + (stake(addr) / total_stake) × 2
```

Maximum multiplier: **3×** (at 100% stake concentration — theoretical).  
Typical multiplier for a node with 1% of stake: **1.02×**.

### 4.3 Reward eligibility

To receive epoch rewards, a node must:

1. Be online and responding to pings
2. Have `stake ≥ MIN_STAKE / 2 = 500 GHOST`
3. Not be in `Ejected` or `Withdrawn` status
4. Participate in transaction relay (submit or forward at least 1 tx per epoch)

*Epoch length: 1 hour (approximate, based on uptime tracking).*

---

## 5. Slashing

### 5.1 Slash conditions

| Violation | Code | Slash % |
|-----------|------|---------|
| Double spend (conflicting tx) | `ConflictingTx` | 10% |
| Double vote (equivocation) | `DoubleVote` | 10% |
| Invalid state submission | `InvalidState` | 10% |
| Reputation penalty (future) | `ReputationPenalty` | 10% |

All violations currently slash at the same rate. Graduated slashing (first offense lighter) is planned for v0.2.

### 5.2 Slash mechanics

```
slash_amount = floor(stake × 0.10)
burned       = floor(slash_amount × 0.50)
to_pool      = slash_amount - burned
stake       -= slash_amount
```

- 50% of slashed amount is **burned** (permanently removed from supply)
- 50% goes to the **slash pool** for redistribution

### 5.3 Slash pool redistribution

The slash pool is periodically distributed to clean validators (zero violations):

```
per_node = floor(slash_pool / count(clean_validators))
```

This creates a direct economic incentive for honest behavior: honest validators receive a share of the penalty paid by malicious ones.

### 5.4 Ejection

After **3 violations**, the node is ejected:

```
remaining_stake → 50% burned, 50% to slash pool
node.status = Ejected
```

Ejected nodes cannot withdraw remaining stake and cannot re-stake. This is a terminal state.

### 5.5 Withdrawal

A non-ejected node can withdraw stake at any time:

```
stake → returned to balance
node.status = Withdrawn
```

*Note: no lock period is implemented in v0.1. A lock period (e.g. 72 hours) is planned for v0.2 to prevent stake manipulation around conflict resolution events.*

---

## 6. Stake Lock Period (Planned — v0.2)

A stake lock period prevents an adversary from:
1. Staking to influence a conflict resolution
2. Immediately withdrawing after the conflict closes

Planned lock: **72 hours** from stake registration before withdrawal is permitted.

This closes a known attack vector where an adversary moves stake between partitions to influence PHA re-evaluation.

---

## 7. Supply Schedule (Projection)

Approximate cumulative emission under continuous operation with 100 validators:

| Year | Approx cumulative emission | % of cap |
|------|---------------------------|----------|
| 1 | ~87,600 GHOST | 0.46% |
| 4 | ~350,400 GHOST | 1.85% |
| 8 | ~525,600 GHOST | 2.78% |
| 20 | ~613,200 GHOST | 3.24% |

*Note: actual emission depends on validator count, uptime, and address cap enforcement. The per-address cap (21,000 GHOST) is the binding constraint for individual nodes, not the global cap. The global emission cap (18.9M GHOST) is unlikely to be reached within the first decade.*

---

## 8. Economic Security Bound

The minimum cost of a double-spend attack is bounded by the slashing model:

```
cost_of_attack ≥ stake × 0.10 per attempt
               ≥ 1,000 × 0.10 = 100 GHOST minimum (at MIN_STAKE)
```

After 3 failed attempts, the attacker's entire stake is confiscated:

```
max_cost = full stake (ejection)
```

For an attacker with stake fraction f of total_stake S:

```
economic_barrier = f × S × 0.10 per attempt
                 = f × S (ejection after 3 attempts)
```

Combined with the consensus security bound (Conjecture F: f < 1/6), an attacker controlling less than 1/6 of stake faces both a cryptographic barrier (σ=2.0 dominance) and an economic barrier (stake confiscation) simultaneously.

---

## 9. Summary

| Parameter | Value |
|-----------|-------|
| Total supply | 21,000,000 GHOST |
| Genesis allocation | 2,100,000 GHOST (10%) |
| Emission pool | 18,900,000 GHOST (90%) |
| Base reward | 10 GHOST/hour |
| Halvening | Every 4 years |
| MIN_STAKE | 1,000 GHOST |
| Slash rate | 10% per violation |
| Slash burn ratio | 50% burned, 50% to pool |
| Max violations | 3 (ejection) |
| Address emission cap | 21,000 GHOST |
| Stake lock period | Not implemented (v0.1) |

---

*GhostLedger Economics v0.1 — March 2026*  
*This document describes a research prototype. Economic parameters are subject to change.*
