# sim/conflict_resolver.py

from sim.dag import DAG

SIGMA = 2.0           # closure threshold: winner_score >= SIGMA * loser_score
MIN_WEIGHT = 3        # minimum weight before resolution


def compute_losers(dag: DAG, conflict_sets: dict[str, list[str]]) -> set[str]:
    """Return set of loser tx_ids across all conflict sets."""
    losers = set()
    for ids in conflict_sets.values():
        tips = dag.get_tips()
        conflict_tips = [t for t in ids if t in tips]
        if len(conflict_tips) < 2:
            continue
        scores = {t: dag.vertices[t].weight for t in conflict_tips}
        winner = max(scores, key=lambda t: (scores[t], t))  # tiebreak: lex min id
        for t in conflict_tips:
            if t != winner:
                losers.add(t)
    return losers


def try_resolve(
    dag: DAG,
    conflict_sets: dict[str, list[str]],
    resolved: dict[str, str],
) -> list[str]:
    newly_resolved = []

    for cid, ids in conflict_sets.items():
        if cid in resolved:
            continue
        # All must have weight >= MIN_WEIGHT
        if not all(dag.vertices[t].weight >= MIN_WEIGHT for t in ids if t in dag.vertices):
            continue

        scores = {
            t: dag.vertices[t].weight
            for t in ids
            if t in dag.vertices
        }
        if not scores:
            continue

        winner = max(scores, key=lambda t: (scores[t], t))
        winner_score = scores[winner]
        others = [s for t, s in scores.items() if t != winner]
        second = max(others) if others else 0

        if second == 0 or winner_score >= SIGMA * second:
            resolved[cid] = winner
            newly_resolved.append(cid)

    return newly_resolved