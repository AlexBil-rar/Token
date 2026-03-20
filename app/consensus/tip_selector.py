# app/consensus/tip_selector.py

from __future__ import annotations

import random

from app.config import MAX_PARENTS
from app.ledger.dag import DAG


class TipSelector:
    def select(self, dag: DAG, max_parents: int = MAX_PARENTS) -> list[str]:
        tips = dag.get_tips()

        if not tips:
            return []

        if len(tips) == 1:
            return [tips[0]]

        weights = [dag.vertices[tip_id].weight for tip_id in tips]

        count = min(max_parents, len(tips))
        selected = random.choices(tips, weights=weights, k=count)

        seen = set()
        unique = []
        for tip_id in selected:
            if tip_id not in seen:
                seen.add(tip_id)
                unique.append(tip_id)

        if len(unique) < count:
            remaining = [t for t in tips if t not in seen]
            unique.extend(remaining[: count - len(unique)])

        return unique