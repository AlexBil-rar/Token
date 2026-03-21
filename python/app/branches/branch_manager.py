# app/branches/branch_manager.py

from __future__ import annotations

from dataclasses import dataclass, field

from app.branches.branch import Branch
from app.branches.coordinator import Coordinator
from app.ledger.transaction import TransactionVertex
from app.ledger.validator import ValidationResult


@dataclass
class BranchManager:
    branches: dict[str, Branch] = field(default_factory=dict)
    coordinator: Coordinator = field(default_factory=Coordinator)
    

    def create_branch(self, branch_id: str) -> Branch:
        branch = Branch(branch_id=branch_id)
        self.branches[branch_id] = branch
        return branch


    def get_least_loaded_branch(self) -> Branch:
        return min(
            self.branches.values(),
            key=lambda b: b.mempool.size()
        )

    def submit_transaction(self, tx: TransactionVertex) -> ValidationResult:
        branch = self.get_least_loaded_branch()
        result = branch.submit_transaction(tx)

        if result.ok:
            self.coordinator.merge(list(self.branches.values()))

        return result

    def credit(self, address: str, amount: int) -> None:
        for branch in self.branches.values():
            branch.state.credit(address, amount)

    def get_stats(self) -> dict:
        return {
            "branches": [b.get_stats() for b in self.branches.values()],
            "coordinator": self.coordinator.stats(),
        }