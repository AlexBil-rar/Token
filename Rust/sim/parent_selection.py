# sim/parent_selection.py

import math
import random
from dataclasses import dataclass
from sim.dag import DAG, Tx


@dataclass
class Policy:
    beta: float    # 0.0 = random, 1.0 = greedy
    epsilon: float # 0.0 = no noise, 0.3 = 30% decoy chance
    max_parents: int = 2


def select_parents(
    dag: DAG,
    losers: set[str],
    decoy_pool: list[str],
    policy: Policy,
    rng: random.Random,
) -> tuple[list[str], bool]:
    """
    Returns (selected_parents, used_decoy).
    """
    tips = dag.get_tips()
    if not tips:
        return [], False

    # 1. Filter conflict losers
    candidates = [t for t in tips if t not in losers]
    if not candidates:
        candidates = tips

    # 2. Weighted selection with bias β
    if policy.beta >= 1.0:
        # pure greedy — pick heaviest
        candidates.sort(key=lambda t: -dag.vertices[t].weight)
        selected = candidates[:policy.max_parents]

    elif policy.beta <= 0.0:
        # pure random
        rng.shuffle(candidates)
        selected = candidates[:policy.max_parents]

    else:
        # Gumbel-max trick: key = -ln(U) / w^β
        keyed = []
        for t in candidates:
            u = max(rng.random(), 1e-15)
            w = dag.vertices[t].weight ** policy.beta
            keyed.append((-math.log(u) / w, t))
        keyed.sort()
        selected = [t for _, t in keyed[:policy.max_parents]]

    # 3. Privacy noise: with prob ε replace last parent with decoy
    used_decoy = False
    available = [d for d in decoy_pool if d not in selected]

    if policy.epsilon > 0.0 and available:
        if rng.random() < policy.epsilon:
            decoy = rng.choice(available)
            if len(selected) < policy.max_parents:
                selected.append(decoy)
            else:
                selected[-1] = decoy
            used_decoy = True

    return selected, used_decoy