# sim/metrics.py

from dataclasses import dataclass, field
from collections import Counter
import math


@dataclass
class TrialMetrics:
    beta: float
    epsilon: float
    closure_times: list[int] = field(default_factory=list)
    unresolved: int = 0
    dag_widths: list[int] = field(default_factory=list)
    decoy_injections: int = 0
    total_selections: int = 0
    parent_sets: list = field(default_factory=list)

    def record_width(self, width: int):
        self.dag_widths.append(width)

    def record_closure(self, steps: int):
        self.closure_times.append(steps)

    def record_selection(self, used_decoy: bool):
        self.total_selections += 1
        if used_decoy:
            self.decoy_injections += 1

    def record_parents(self, parents: list):
        self.parent_sets.append(frozenset(parents))

    # ── Existing metrics ──────────────────────────────────────────────────────

    @property
    def median_closure_time(self) -> float:
        if not self.closure_times:
            return float('inf')
        s = sorted(self.closure_times)
        n = len(s)
        return (s[n // 2] + s[(n - 1) // 2]) / 2

    @property
    def p90_closure_time(self) -> float:
        if not self.closure_times:
            return float('inf')
        s = sorted(self.closure_times)
        return s[int(len(s) * 0.9)]

    @property
    def mean_dag_width(self) -> float:
        return sum(self.dag_widths) / len(self.dag_widths) if self.dag_widths else 0.0

    @property
    def decoy_rate(self) -> float:
        if self.total_selections == 0:
            return 0.0
        return self.decoy_injections / self.total_selections

    @property
    def closure_rate(self) -> float:
        total = len(self.closure_times) + self.unresolved
        if total == 0:
            return 1.0
        return len(self.closure_times) / total

    # ── Graph entropy metrics ─────────────────────────────────────────────────

    @property
    def parent_diversity(self) -> float:
        if not self.parent_sets:
            return 0.0
        unique = len(set(self.parent_sets))
        return unique / len(self.parent_sets)

    @property
    def graph_entropy(self) -> float:
        counts = Counter(p for ps in self.parent_sets for p in ps)
        total = sum(counts.values())
        if total == 0:
            return 0.0
        entropy = 0.0
        for c in counts.values():
            p = c / total
            entropy -= p * math.log2(p)
        return entropy

    @property
    def origin_recovery_risk(self) -> float:
        if not self.parent_sets:
            return 0.0

        all_parents = set(p for ps in self.parent_sets for p in ps)
        max_entropy = math.log2(len(all_parents)) if len(all_parents) > 1 else 1.0
        normalized_entropy = self.graph_entropy / max_entropy if max_entropy > 0 else 0.0

        diversity_risk = 1.0 - self.parent_diversity
        entropy_risk = 1.0 - normalized_entropy

        return 0.5 * diversity_risk + 0.5 * entropy_risk