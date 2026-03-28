# sim/adversarial.py

import random
import types
from dataclasses import dataclass, field

from sim.dag import DAG, Tx
from sim.parent_selection import Policy, select_parents
from sim.conflict_resolver import compute_losers, try_resolve
from sim.metrics import TrialMetrics

SWEET_SPOT = Policy(beta=0.7, epsilon=0.10)
N_NODES = 6
DECOY_POOL_MAX = 30


# ── Shared helpers ────────────────────────────────────────────────────────────

def _bootstrap(rng: random.Random, policy: Policy, n_warmup: int = 20):
    """Build a warmed-up DAG with honest transactions."""
    dag = DAG()
    decoy_pool: list[str] = []
    conflict_sets: dict = {}

    genesis = Tx("genesis", "system", 0, [], weight=1)
    dag.add(genesis)
    decoy_pool.append("genesis")

    tx_counter = 0
    for step in range(n_warmup):
        losers = compute_losers(dag, conflict_sets)
        for node_id in range(N_NODES):
            tx_counter += 1
            parents, _ = select_parents(dag, losers, decoy_pool, policy, rng)
            tx = Tx(f"warm{tx_counter}", f"n{node_id}", tx_counter, parents)
            dag.add(tx)
            dag.propagate_weight(tx.tx_id)
            decoy_pool.append(tx.tx_id)
            if len(decoy_pool) > DECOY_POOL_MAX:
                decoy_pool.pop(0)

    return dag, decoy_pool, tx_counter


# ── Attack 1: Parasite DAG ────────────────────────────────────────────────────

@dataclass
class ParasiteResult:
    seed: int
    parasite_weight_ratio: float
    merge_accepted: bool
    steps_to_detect: int
    damage_bound: float


def run_parasite_attack(
    n_honest_tx: int = 100,
    n_parasite_tx: int = 30,
    seed: int = 42,
    policy: Policy = SWEET_SPOT,
) -> ParasiteResult:
    rng = random.Random(seed)
    dag, decoy_pool, tx_counter = _bootstrap(rng, policy, n_warmup=15)

    conflict_sets: dict = {}
    resolved: dict = {}

    for step in range(n_honest_tx):
        losers = compute_losers(dag, conflict_sets)
        for node_id in range(N_NODES):
            tx_counter += 1
            parents, _ = select_parents(dag, losers, decoy_pool, policy, rng)
            tx = Tx(f"h{tx_counter}", f"n{node_id}", tx_counter, parents)
            dag.add(tx)
            dag.propagate_weight(tx.tx_id)
            decoy_pool.append(tx.tx_id)
            if len(decoy_pool) > DECOY_POOL_MAX:
                decoy_pool.pop(0)

    honest_tips_before = set(dag.get_tips())
    honest_total_weight = sum(dag.vertices[t].weight for t in honest_tips_before)

    parasite_ids = []
    anchor = "genesis"
    for i in range(n_parasite_tx):
        tx_counter += 1
        pid = f"p{tx_counter}"
        parents = [anchor] if i == 0 else [parasite_ids[-1]]
        tx = Tx(pid, "attacker", i, parents)
        dag.add(tx)
        dag.propagate_weight(pid)
        parasite_ids.append(pid)
        anchor = pid

    parasite_tip = parasite_ids[-1]
    parasite_weight = dag.vertices[parasite_tip].weight

    best_honest = max(honest_tips_before, key=lambda t: dag.vertices[t].weight)
    cid = "parasite_conflict"
    conflict_sets[cid] = [best_honest, parasite_tip]
    open_at = n_honest_tx

    steps_to_detect = -1
    for step in range(50):
        losers = compute_losers(dag, conflict_sets)
        for node_id in range(N_NODES):
            tx_counter += 1
            parents, _ = select_parents(dag, losers, decoy_pool, policy, rng)
            tx = Tx(f"post{tx_counter}", f"n{node_id}", tx_counter, parents)
            dag.add(tx)
            dag.propagate_weight(tx.tx_id)
            decoy_pool.append(tx.tx_id)
            if len(decoy_pool) > DECOY_POOL_MAX:
                decoy_pool.pop(0)

        newly = try_resolve(dag, conflict_sets, resolved)
        if cid in newly:
            steps_to_detect = step + 1
            break

    winner = resolved.get(cid)
    merge_accepted = (winner == parasite_tip)

    honest_weight_after = dag.vertices[best_honest].weight
    parasite_weight_ratio = parasite_weight / max(honest_weight_after, 1)

    parasite_tips_surviving = set(parasite_ids) & set(dag.get_tips())
    damage_bound = len(parasite_tips_surviving) / max(len(honest_tips_before), 1)

    return ParasiteResult(
        seed=seed,
        parasite_weight_ratio=parasite_weight_ratio,
        merge_accepted=merge_accepted,
        steps_to_detect=steps_to_detect if steps_to_detect > 0 else 50,
        damage_bound=damage_bound,
    )


# ── Attack 2: Spam flood ──────────────────────────────────────────────────────

@dataclass
class SpamResult:
    seed: int
    spam_tx_count: int
    honest_tx_count: int
    spam_weight_share: float
    dag_width_increase: float
    time_to_dilute: int


def run_spam_flood(
    n_steps: int = 80,
    spam_per_step: int = 10,
    seed: int = 42,
    policy: Policy = SWEET_SPOT,
    spam_rate_limit: int = 2,      # simulates PoW cost — max spam tx per step
) -> SpamResult:
    rng = random.Random(seed)
    dag, decoy_pool, tx_counter = _bootstrap(rng, policy, n_warmup=10)

    conflict_sets: dict = {}
    spam_ids: list[str] = []
    honest_ids: list[str] = []
    width_before = len(dag.get_tips())
    time_to_dilute = n_steps
    spam_ids_set: set[str] = set()  # O(1) lookup

    for step in range(n_steps):
        losers = compute_losers(dag, conflict_sets)

        # Honest nodes — select parents via policy
        for node_id in range(N_NODES):
            tx_counter += 1
            parents, _ = select_parents(dag, losers, decoy_pool, policy, rng)
            tx = Tx(f"h{tx_counter}", f"n{node_id}", tx_counter, parents)
            dag.add(tx)
            dag.propagate_weight(tx.tx_id)
            honest_ids.append(tx.tx_id)
            decoy_pool.append(tx.tx_id)
            if len(decoy_pool) > DECOY_POOL_MAX:
                decoy_pool.pop(0)

        # Spammer — rate limited by PoW simulation
        # Picks heaviest tip as anchor (greedy strategy)
        tips = dag.get_tips()
        spam_anchor = max(tips, key=lambda t: dag.vertices[t].weight)
        actual_spam = min(spam_per_step, spam_rate_limit)
        for _ in range(actual_spam):
            tx_counter += 1
            sid = f"spam{tx_counter}"
            tx = Tx(sid, "spammer", tx_counter, [spam_anchor])
            dag.add(tx)
            dag.propagate_weight(sid)
            spam_ids.append(sid)
            spam_ids_set.add(sid)
            spam_anchor = sid

        # Check dilution: spam weight share among tips
        current_tips = dag.get_tips()

        total_w = sum(dag.vertices[t].weight for t in dag.vertices)
        spam_total_w = sum(
            dag.vertices[t].weight
            for t in dag.vertices
            if t in spam_ids_set
        )
        if total_w > 0 and spam_total_w / total_w < 0.10 and time_to_dilute == n_steps:
            time_to_dilute = step + 1

    total_weight = sum(dag.vertices[t].weight for t in dag.vertices)
    spam_weight = sum(
        dag.vertices[t].weight for t in dag.vertices if t in spam_ids_set
    )
    spam_weight_share = spam_weight / max(total_weight, 1)
    width_after = len(dag.get_tips())

    return SpamResult(
        seed=seed,
        spam_tx_count=len(spam_ids),
        honest_tx_count=len(honest_ids),
        spam_weight_share=spam_weight_share,
        dag_width_increase=width_after - width_before,
        time_to_dilute=time_to_dilute,
    )

# ── Attack 3: Double spend race ───────────────────────────────────────────────

@dataclass
class DoubleSpendResult:
    seed: int
    attacker_won: bool
    steps_to_resolve: int
    attacker_weight_at_resolve: int
    honest_weight_at_resolve: int
    weight_gap: int


def run_double_spend_race(
    n_steps: int = 60,
    attacker_boost: int = 3,
    seed: int = 42,
    policy: Policy = SWEET_SPOT,
) -> DoubleSpendResult:
    rng = random.Random(seed)
    dag, decoy_pool, tx_counter = _bootstrap(rng, policy, n_warmup=15)

    conflict_sets: dict = {}
    resolved: dict = {}

    # Inject double spend
    losers = compute_losers(dag, conflict_sets)
    parents_honest, _ = select_parents(dag, losers, decoy_pool, policy, rng)
    parents_attack, _ = select_parents(dag, losers, decoy_pool, policy, rng)

    tx_counter += 1
    tx_honest = Tx(f"ds_honest_{tx_counter}", "victim", 1, parents_honest)
    tx_counter += 1
    tx_attack = Tx(f"ds_attack_{tx_counter}", "victim", 1, parents_attack)

    dag.add(tx_honest); dag.propagate_weight(tx_honest.tx_id)
    dag.add(tx_attack); dag.propagate_weight(tx_attack.tx_id)

    cid = "double_spend"
    conflict_sets[cid] = [tx_honest.tx_id, tx_attack.tx_id]
    open_at = 0

    steps_to_resolve = n_steps
    attacker_won = False

    for step in range(n_steps):
        losers = compute_losers(dag, conflict_sets)

        for node_id in range(N_NODES):
            tx_counter += 1
            honest_parents = [tx_honest.tx_id]
            tx = Tx(f"post_h{tx_counter}", f"n{node_id}", tx_counter, honest_parents)
            dag.add(tx)
            dag.propagate_weight(tx.tx_id)
            decoy_pool.append(tx.tx_id)
            if len(decoy_pool) > DECOY_POOL_MAX:
                decoy_pool.pop(0)

        for _ in range(attacker_boost):
            tx_counter += 1
            tx = Tx(f"boost{tx_counter}", "attacker", tx_counter, [tx_attack.tx_id])
            dag.add(tx)
            dag.propagate_weight(tx.tx_id)

        newly = try_resolve(dag, conflict_sets, resolved)
        if cid in newly:
            steps_to_resolve = step + 1
            attacker_won = (resolved[cid] == tx_attack.tx_id)
            break

    hw = dag.vertices[tx_honest.tx_id].weight
    aw = dag.vertices[tx_attack.tx_id].weight

    return DoubleSpendResult(
        seed=seed,
        attacker_won=attacker_won,
        steps_to_resolve=steps_to_resolve,
        attacker_weight_at_resolve=aw,
        honest_weight_at_resolve=hw,
        weight_gap=hw - aw,
    )


# ── Aggregate runner ──────────────────────────────────────────────────────────

def run_all(n_trials: int = 30, policy: Policy = SWEET_SPOT):
    print(f"\n{'='*60}")
    print(f"Adversarial Simulation — {n_trials} trials, β={policy.beta}, ε={policy.epsilon}")
    print(f"{'='*60}")

    parasite_results = [
        run_parasite_attack(seed=i * 137 + 1, policy=policy)
        for i in range(n_trials)
    ]
    accepted = sum(r.merge_accepted for r in parasite_results)
    avg_detect = sum(r.steps_to_detect for r in parasite_results) / n_trials
    avg_ratio = sum(r.parasite_weight_ratio for r in parasite_results) / n_trials
    avg_damage = sum(r.damage_bound for r in parasite_results) / n_trials

    print(f"\n--- Attack 1: Parasite DAG ---")
    print(f"  parasite accepted:     {accepted}/{n_trials} ({100*accepted/n_trials:.1f}%)")
    print(f"  avg weight ratio:      {avg_ratio:.3f}  (parasite/honest)")
    print(f"  avg steps to detect:   {avg_detect:.1f}")
    print(f"  avg damage bound:      {avg_damage:.3f}  (fraction tips displaced)")

    spam_results = [
    run_spam_flood(seed=i * 137 + 2, policy=policy, spam_per_step=10)
    for i in range(n_trials)
    ]

    avg_share = sum(r.spam_weight_share for r in spam_results) / n_trials
    avg_width = sum(r.dag_width_increase for r in spam_results) / n_trials
    avg_dilute = sum(r.time_to_dilute for r in spam_results) / n_trials

    print(f"\n--- Attack 2: Spam Flood ---")
    print(f"  avg spam weight share: {avg_share:.3f}  (at end of simulation)")
    print(f"  avg DAG width increase:{avg_width:.1f}  tips")
    print(f"  avg time to dilute:    {avg_dilute:.1f}  steps (<10% weight)")

    ds_results = [
        run_double_spend_race(seed=i * 137 + 3, policy=policy)
        for i in range(n_trials)
    ]
    attacker_wins = sum(r.attacker_won for r in ds_results)
    avg_steps = sum(r.steps_to_resolve for r in ds_results) / n_trials
    avg_gap = sum(r.weight_gap for r in ds_results) / n_trials

    print(f"\n--- Attack 3: Double Spend Race ---")
    print(f"  attacker won:          {attacker_wins}/{n_trials} ({100*attacker_wins/n_trials:.1f}%)")
    print(f"  avg steps to resolve:  {avg_steps:.1f}")
    print(f"  avg weight gap:        {avg_gap:.1f}  (honest - attacker)")

    print(f"\n{'='*60}")
    print("Summary:")
    print(f"  Parasite attack success rate: {100*accepted/n_trials:.1f}%  (target: <5%)")
    print(f"  Double spend success rate:    {100*attacker_wins/n_trials:.1f}%  (target: <5%)")
    print(f"  Spam dilution time:           {avg_dilute:.1f} steps  (target: <20)")
    print(f"{'='*60}")


if __name__ == "__main__":
    run_all(n_trials=30)