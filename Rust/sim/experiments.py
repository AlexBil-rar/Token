# sim/experiments.py

import argparse
import types
import random
import csv
from dataclasses import dataclass

from sim.dag import DAG, Tx
from sim.parent_selection import Policy, select_parents
from sim.conflict_resolver import compute_losers, try_resolve
from sim.metrics import TrialMetrics

SWEET_SPOT = Policy(beta=0.7, epsilon=0.10)

N_NODES       = 6
N_TRIALS      = 10
DECOY_POOL_MAX = 30

SCALE_VALUES         = [150, 500, 1000]
SCALE_CONFLICT_EVERY = 50   # fixed

CONFLICT_RATE_VALUES = [10, 30, 50, 100]
CONFLICT_RATE_N_TX   = 500  # fixed

BETA_VALUES    = [0.0, 0.3, 0.5, 0.7, 0.9, 1.0]
EPSILON_VALUES = [0.0, 0.05, 0.10, 0.20, 0.30]
HEATMAP_N_TX   = 150
HEATMAP_CONFLICT_EVERY = 30



def run_trial(policy: Policy, n_tx: int, conflict_every: int, seed: int) -> TrialMetrics:
    rng = random.Random(seed)
    dag = DAG()
    metrics = TrialMetrics(beta=policy.beta, epsilon=policy.epsilon)

    conflict_sets: dict = {}
    conflict_open_at: dict = {}
    resolved: dict = {}
    decoy_pool: list = []

    tx_counter = 0
    conflict_counter = 0

    genesis = Tx("genesis", "system", 0, [], weight=1)
    dag.add(genesis)
    decoy_pool.append("genesis")

    for step in range(n_tx):
        if step > 10 and step % conflict_every == 0:
            cid = f"c{conflict_counter}"
            conflict_counter += 1
            sender = f"user_{conflict_counter}"
            losers = compute_losers(dag, conflict_sets)
            parents_a, _ = select_parents(dag, losers, decoy_pool, policy, rng)
            parents_b, _ = select_parents(dag, losers, decoy_pool, policy, rng)
            tx_a = Tx(f"{cid}a", sender, 1, parents_a, weight=1, conflict_id=cid)
            tx_b = Tx(f"{cid}b", sender, 1, parents_b, weight=1, conflict_id=cid)
            dag.add(tx_a); dag.propagate_weight(tx_a.tx_id)
            dag.add(tx_b); dag.propagate_weight(tx_b.tx_id)
            conflict_sets[cid] = [tx_a.tx_id, tx_b.tx_id]
            conflict_open_at[cid] = step

        losers = compute_losers(dag, conflict_sets)
        node_parents = []
        node_decoys = []

        for node_id in range(N_NODES):
            view = types.SimpleNamespace(
                get_tips=lambda nid=node_id: dag.get_tips_partial(15 + nid * 2),
                vertices=dag.vertices,
            )
            p, used = select_parents(view, losers, decoy_pool, policy, rng)
            node_parents.append(p)
            node_decoys.append(used)

        for node_id in range(N_NODES):
            tx_counter += 1
            metrics.record_selection(node_decoys[node_id])
            tx = Tx(f"t{tx_counter}", f"n{node_id}", tx_counter, node_parents[node_id])
            dag.add(tx)
            dag.propagate_weight(tx.tx_id)
            decoy_pool.append(tx.tx_id)
            if len(decoy_pool) > DECOY_POOL_MAX:
                decoy_pool.pop(0)

        newly = try_resolve(dag, conflict_sets, resolved)
        for cid in newly:
            metrics.record_closure(step - conflict_open_at[cid])

        metrics.record_width(len(dag.get_tips()))

    metrics.unresolved = len(conflict_sets) - len(resolved)
    return metrics


def avg(vals):
    return sum(vals) / len(vals) if vals else 0.0


# ── Experiment A: Scale ───────────────────────────────────────────────────────

@dataclass
class ScaleResult:
    n_tx: int
    median_closure: float
    closure_rate: float
    mean_dag_width: float


def run_scale_experiment() -> list[ScaleResult]:
    print("\n=== Experiment A: Scale (β=0.5, ε=0.10) ===")
    print(f"{'n_tx':>6}  {'median_cls':>10}  {'close_rate':>10}  {'dag_width':>9}")
    print("-" * 45)

    results = []
    for n_tx in SCALE_VALUES:
        trials = [run_trial(SWEET_SPOT, n_tx, SCALE_CONFLICT_EVERY, seed=i*137+7)
                  for i in range(N_TRIALS)]

        mc = avg([t.median_closure_time for t in trials])
        cr = avg([t.closure_rate for t in trials])
        wd = avg([t.mean_dag_width for t in trials])

        inf_s = lambda v: "         ∞" if v == float('inf') else f"{v:10.1f}"
        print(f"{n_tx:6}  {inf_s(mc)}  {cr:10.3f}  {wd:9.1f}")
        results.append(ScaleResult(n_tx, mc, cr, wd))

    return results


# ── Experiment B: Conflict rate ───────────────────────────────────────────────

@dataclass
class ConflictRateResult:
    conflict_every: int
    conflicts_total: float   # avg number of conflicts injected
    median_closure: float
    closure_rate: float
    mean_dag_width: float


def run_conflict_rate_experiment() -> list[ConflictRateResult]:
    print("\n=== Experiment B: Conflict rate (β=0.5, ε=0.10, N_TX=500) ===")
    print(f"{'conf_every':>10}  {'n_conflicts':>11}  {'median_cls':>10}  {'close_rate':>10}  {'dag_width':>9}")
    print("-" * 60)

    results = []
    for ce in CONFLICT_RATE_VALUES:
        trials = [run_trial(SWEET_SPOT, CONFLICT_RATE_N_TX, ce, seed=i*137+13)
                  for i in range(N_TRIALS)]

        n_conflicts_approx = max(0, (CONFLICT_RATE_N_TX - 10) // ce)
        mc = avg([t.median_closure_time for t in trials])
        cr = avg([t.closure_rate for t in trials])
        wd = avg([t.mean_dag_width for t in trials])
        nc = avg([len(t.closure_times) + t.unresolved for t in trials])

        inf_s = lambda v: "         ∞" if v == float('inf') else f"{v:10.1f}"
        print(f"{ce:10}  {nc:11.1f}  {inf_s(mc)}  {cr:10.3f}  {wd:9.1f}")
        results.append(ConflictRateResult(ce, nc, mc, cr, wd))

    return results


# ── Heatmap (closure_rate and dag_width) ─────────────────────────────────────

@dataclass
class HeatmapResult:
    beta: float
    epsilon: float
    closure_rate: float
    median_closure: float
    mean_dag_width: float


def run_heatmap_sweep() -> list[HeatmapResult]:
    print(f"\n=== Heatmap sweep: {len(BETA_VALUES)}β × {len(EPSILON_VALUES)}ε × {N_TRIALS} trials ===")
    total = len(BETA_VALUES) * len(EPSILON_VALUES)
    done = 0
    results = []

    for beta in BETA_VALUES:
        for epsilon in EPSILON_VALUES:
            policy = Policy(beta=beta, epsilon=epsilon)
            trials = [run_trial(policy, HEATMAP_N_TX, HEATMAP_CONFLICT_EVERY, seed=i*137+42)
                      for i in range(N_TRIALS)]
            mc = avg([t.median_closure_time for t in trials])
            cr = avg([t.closure_rate for t in trials])
            wd = avg([t.mean_dag_width for t in trials])
            done += 1
            print(f"  [{done}/{total}] β={beta:.1f} ε={epsilon:.2f} "
                  f"→ rate={cr:.2f} width={wd:.1f}", flush=True)
            results.append(HeatmapResult(beta, epsilon, cr, mc, wd))

    return results


def plot_heatmaps(results: list[HeatmapResult]):
    try:
        import matplotlib.pyplot as plt
        import matplotlib.colors as mcolors
        import numpy as np
    except ImportError:
        print("pip install matplotlib numpy")
        return

    nb, ne = len(BETA_VALUES), len(EPSILON_VALUES)

    fig, axes = plt.subplots(1, 3, figsize=(16, 5))
    fig.suptitle("GhostLedger β/ε Parent Selection — Empirical Results", fontsize=13)

    panels = [
        ("closure_rate",    "Conflict closure rate\n(higher = better)",    "RdYlGn"),
        ("median_closure",  "Median closure time (steps)\n(lower = better, ∞ = never)", "RdYlGn_r"),
        ("mean_dag_width",  "Mean DAG width (tips)\n(lower = less sprawl)",  "RdYlGn_r"),
    ]

    for ax, (attr, title, cmap) in zip(axes, panels):
        matrix = np.zeros((nb, ne))
        for r in results:
            bi = BETA_VALUES.index(r.beta)
            ei = EPSILON_VALUES.index(r.epsilon)
            val = getattr(r, attr)
            matrix[bi][ei] = 0 if val == float('inf') else val

        im = ax.imshow(matrix, aspect='auto', origin='lower', cmap=cmap)
        ax.set_xticks(range(ne))
        ax.set_xticklabels([f"{e:.2f}" for e in EPSILON_VALUES], fontsize=9)
        ax.set_yticks(range(nb))
        ax.set_yticklabels([f"{b:.1f}" for b in BETA_VALUES], fontsize=9)
        ax.set_xlabel("ε (privacy noise)", fontsize=10)
        ax.set_ylabel("β (consensus bias)", fontsize=10)
        ax.set_title(title, fontsize=10)
        plt.colorbar(im, ax=ax, shrink=0.8)

        for bi in range(nb):
            for ei in range(ne):
                v = matrix[bi][ei]
                label = "∞" if getattr(results[bi*ne+ei], attr) == float('inf') else f"{v:.2f}"
                ax.text(ei, bi, label, ha='center', va='center',
                        fontsize=7, color='black',
                        fontweight='bold' if bi == 4 and ei == 1 else 'normal')

        sweet_bi = BETA_VALUES.index(0.5)
        sweet_ei = EPSILON_VALUES.index(0.10)
        ax.add_patch(plt.Rectangle(
            (sweet_ei - 0.5, sweet_bi - 0.5), 1, 1,
            fill=False, edgecolor='blue', linewidth=2, label='default (β=0.5, ε=0.10)'
        ))

    axes[0].legend(loc='upper right', fontsize=8)
    plt.tight_layout()
    plt.savefig("beta_epsilon_heatmaps.png", dpi=150, bbox_inches='tight')
    print("\nPlot saved → beta_epsilon_heatmaps.png")
    plt.show()


# ── Entry point ───────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--scale",         action="store_true")
    parser.add_argument("--conflict-rate", action="store_true")
    parser.add_argument("--heatmap",       action="store_true")
    parser.add_argument("--all",           action="store_true")
    parser.add_argument("--plot",          action="store_true")
    args = parser.parse_args()

    if not any([args.scale, args.conflict_rate, args.heatmap, args.all]):
        parser.print_help()
        return

    if args.scale or args.all:
        run_scale_experiment()

    if args.conflict_rate or args.all:
        run_conflict_rate_experiment()

    if args.heatmap or args.all:
        heatmap_results = run_heatmap_sweep()
        if args.plot:
            plot_heatmaps(heatmap_results)


if __name__ == "__main__":
    main()