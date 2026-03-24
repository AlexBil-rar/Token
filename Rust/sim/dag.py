# sim/dag.py

from dataclasses import dataclass, field
from typing import Optional


@dataclass
class Tx:
    tx_id: str
    sender: str
    nonce: int
    parents: list[str]
    weight: int = 1
    conflict_id: Optional[str] = None


class DAG:
    def __init__(self):
        self.vertices: dict[str, Tx] = {}
        self.tips: set[str] = set()
        self.children: dict[str, set[str]] = {}
        self._order: list[str] = []

    def add(self, tx: Tx):
        for p in tx.parents:
            self.children.setdefault(p, set()).add(tx.tx_id)
            self.tips.discard(p)
        self.tips.add(tx.tx_id)
        self.vertices[tx.tx_id] = tx
        self._order.append(tx.tx_id)

    def get_tips(self) -> list[str]:
        return list(self.tips)

    def get_tips_partial(self, known_last_n: int) -> list[str]:
        if known_last_n >= len(self._order):
            return self.get_tips()

        known = set(self._order[-known_last_n:])
        known.add(self._order[0]) 

        partial_tips = []
        for tx_id in known:
            children_in_known = self.children.get(tx_id, set()) & known
            if not children_in_known:
                partial_tips.append(tx_id)
        return partial_tips if partial_tips else self.get_tips()

    def propagate_weight(self, tx_id: str):
        visited = set()
        stack = list(self.vertices[tx_id].parents)
        while stack:
            pid = stack.pop()
            if pid in visited:
                continue
            visited.add(pid)
            if pid in self.vertices:
                self.vertices[pid].weight += 1
                stack.extend(self.vertices[pid].parents)