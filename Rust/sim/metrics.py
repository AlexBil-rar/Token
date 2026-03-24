# sim/metrics.py

from dataclasses import dataclass, field


@dataclass
class TrialMetrics:
    beta: float
    epsilon: float

    # Convergence
    closure_times: list[int] = field(default_factory=list)   # steps to resolve each conflict
    unresolved: int = 0                                        # conflicts not closed by end

    # DAG shape
    dag_widths: list[int] = field(default_factory=list)       # tips count over time

    # Privacy
    decoy_injections: int = 0
    total_selections: int = 0

    def record_width(self, width: int):
        self.dag_widths.append(width)

    def record_closure(self, steps: int):
        self.closure_times.append(steps)

    def record_selection(self, used_decoy: bool):
        self.total_selections += 1
        if used_decoy:
            self.decoy_injections += 1

    # ── Derived ──────────────────────────────────────────────────────────────

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