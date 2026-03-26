# sim/run_simulation.py
import random
import argparse
import csv
import sys
import types
from dataclasses import dataclass

from sim.dag import DAG, Tx
from sim.parent_selection import Policy, select_parents
from sim.conflict_resolver import compute_losers, try_resolve
from sim.metrics import TrialMetrics

# ── Simulation parameters ─────────────────────────────────────────────────────

N_NODES         = 6
N_TX            = 150
CONFLICT_EVERY  = 30
N_TRIALS        = 50
DECOY_POOL_MAX  = 30

BETA_VALUES    = [0.0, 0.3, 0.5, 0.7, 0.9, 1.0]
EPSILON_VALUES = [0.0, 0.05, 0.10, 0.15, 0.20, 0.30]


# ── Single trial ──────────────────────────────────────────────────────────────

def run_trial(policy: Policy, seed: int) -> TrialMetrics:
    rng = random.Random(seed)
    dag = DAG()
    metrics = TrialMetrics(beta=policy.beta, epsilon=policy.epsilon)

    conflict_sets: dict[str, list[str]] = {}
    conflict_open_at: dict[str, int] = {}
    resolved: dict[str, str] = {}
    decoy_pool: list[str] = []

    tx_counter = 0
    conflict_counter = 0

    genesis = Tx(tx_id="genesis", sender="system", nonce=0, parents=[], weight=1)
    dag.add(genesis)
    decoy_pool.append("genesis")

    for step in range(N_TX):

        if step > 10 and step % CONFLICT_EVERY == 0:
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
            metrics.record_parents(node_parents[node_id])

            tx = Tx(
                tx_id=f"t{tx_counter}",
                sender=f"n{node_id}",
                nonce=tx_counter,
                parents=node_parents[node_id],
            )
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


# ── Sweep ─────────────────────────────────────────────────────────────────────

@dataclass
class AggResult:
    beta: float
    epsilon: float
    median_closure: float
    p90_closure: float
    closure_rate: float
    mean_dag_width: float
    decoy_rate: float
    parent_diversity: float   
    graph_entropy: float    
    origin_recovery_risk: float 


def aggregate(trials: list[TrialMetrics]) -> AggResult:
    def avg(vals):
        return sum(vals) / len(vals) if vals else 0.0

    return AggResult(
        beta=trials[0].beta,
        epsilon=trials[0].epsilon,
        median_closure=avg([t.median_closure_time for t in trials]),
        p90_closure=avg([t.p90_closure_time for t in trials]),
        closure_rate=avg([t.closure_rate for t in trials]),
        mean_dag_width=avg([t.mean_dag_width for t in trials]),
        decoy_rate=avg([t.decoy_rate for t in trials]),
        parent_diversity=avg([t.parent_diversity for t in trials]),   
        graph_entropy=avg([t.graph_entropy for t in trials]),         
        origin_recovery_risk=avg([t.origin_recovery_risk for t in trials]), 

    )


def run_sweep() -> list[AggResult]:
    results = []
    total = len(BETA_VALUES) * len(EPSILON_VALUES)
    done = 0

    for beta in BETA_VALUES:
        for epsilon in EPSILON_VALUES:
            policy = Policy(beta=beta, epsilon=epsilon)
            trials = [run_trial(policy, seed=i * 137 + 42) for i in range(N_TRIALS)]
            agg = aggregate(trials)
            results.append(agg)
            done += 1
            print(f"  [{done}/{total}] β={beta:.1f} ε={epsilon:.2f} "
                  f"→ closure={agg.median_closure:.1f}steps "
                  f"rate={agg.closure_rate:.2f} "
                  f"width={agg.mean_dag_width:.1f}",
                  flush=True)

    return results


# ── Output ────────────────────────────────────────────────────────────────────

HEADER = ["beta", "epsilon", "median_closure", "p90_closure",
          "closure_rate", "mean_dag_width", "decoy_rate"]


def print_table(results: list[AggResult]):
    print()
    print(f"{'β':>4}  {'ε':>5}  {'median_cls':>10}  {'p90_cls':>8}  "
          f"{'close_rate':>10}  {'dag_width':>9}  {'decoy_rt':>8}  "
          f"{'p_divers':>8}  {'entropy':>7}  {'orig_risk':>9}")
    print("-" * 95)
    for r in results:
        inf_str = lambda v: "         ∞" if v == float('inf') else f"{v:10.1f}"
        print(f"{r.beta:4.1f}  {r.epsilon:5.2f}  {inf_str(r.median_closure)}  "
              f"{inf_str(r.p90_closure):>8}  "
              f"{r.closure_rate:10.3f}  {r.mean_dag_width:9.1f}  {r.decoy_rate:8.3f}  "
              f"{r.parent_diversity:8.3f}  {r.graph_entropy:7.3f}  {r.origin_recovery_risk:9.3f}")


def write_csv(results: list[AggResult], path: str):
    with open(path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(HEADER)
        for r in results:
            w.writerow([r.beta, r.epsilon, r.median_closure, r.p90_closure,
                        r.closure_rate, r.mean_dag_width, r.decoy_rate])
    print(f"CSV written to {path}")


def plot_heatmaps(results: list[AggResult]):
    try:
        import matplotlib.pyplot as plt
        import numpy as np
    except ImportError:
        print("matplotlib not installed — skipping plots (pip install matplotlib)")
        return

    metrics_to_plot = [
        ("median_closure",  "Median closure time (steps) — lower = faster consensus"),
        ("closure_rate",    "Closure rate — higher = more conflicts resolved"),
        ("mean_dag_width",  "Mean DAG width (tips) — lower = less sprawl"),
        ("decoy_rate",      "Actual decoy injection rate"),
    ]

    nb = len(BETA_VALUES)
    ne = len(EPSILON_VALUES)

    fig, axes = plt.subplots(2, 2, figsize=(14, 10))
    fig.suptitle("GhostLedger β/ε Parent Selection — Convergence Experiments", fontsize=14)

    for ax, (attr, title) in zip(axes.flat, metrics_to_plot):
        matrix = np.zeros((nb, ne))
        for r in results:
            bi = BETA_VALUES.index(r.beta)
            ei = EPSILON_VALUES.index(r.epsilon)
            val = getattr(r, attr)
            matrix[bi][ei] = val if val != float('inf') else 0

        im = ax.imshow(matrix, aspect='auto', origin='lower',
                       cmap='RdYlGn_r' if 'closure' in attr and attr != 'closure_rate' else 'RdYlGn')
        ax.set_xticks(range(ne))
        ax.set_xticklabels([f"{e:.2f}" for e in EPSILON_VALUES])
        ax.set_yticks(range(nb))
        ax.set_yticklabels([f"{b:.1f}" for b in BETA_VALUES])
        ax.set_xlabel("ε (privacy noise)")
        ax.set_ylabel("β (consensus bias)")
        ax.set_title(title, fontsize=10)
        plt.colorbar(im, ax=ax)

        for bi in range(nb):
            for ei in range(ne):
                ax.text(ei, bi, f"{matrix[bi][ei]:.2f}",
                        ha='center', va='center', fontsize=7, color='black')

    plt.tight_layout()
    plt.savefig("beta_epsilon_heatmaps.png", dpi=150)
    print("Plot saved to beta_epsilon_heatmaps.png")
    plt.show()


# ── Entry point ───────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="GhostLedger β/ε convergence simulation")
    parser.add_argument("--plot", action="store_true", help="Generate heatmap plots")
    parser.add_argument("--csv",  type=str, default="", help="Write results to CSV file")
    args = parser.parse_args()

    print(f"Running sweep: {len(BETA_VALUES)} β × {len(EPSILON_VALUES)} ε × {N_TRIALS} trials")
    print(f"  N_TX={N_TX}, CONFLICT_EVERY={CONFLICT_EVERY}, DECOY_POOL={DECOY_POOL_MAX}")
    print()

    results = run_sweep()
    print_table(results)

    if args.csv:
        write_csv(results, args.csv)

    if args.plot:
        plot_heatmaps(results)


if __name__ == "__main__":
    main()