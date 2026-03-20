# app/branches/coordinator.py

from __future__ import annotations

from dataclasses import dataclass, field

from app.branches.branch import Branch
from app.ledger.state import LedgerState


@dataclass
class Coordinator:
    # TODO Phase 6: заменить на quorum voting
    root_state: LedgerState = field(default_factory=LedgerState)
    merge_count: int = 0

    def merge(self, branches: list[Branch]) -> LedgerState:
        if not branches:
            return self.root_state

        all_addresses: set[str] = set()
        for branch in branches:
            all_addresses.update(branch.state.balances.keys())

        new_state = LedgerState()

        for address in all_addresses:
            balances = [
                branch.state.balances.get(address, 0)
                for branch in branches
                if address in branch.state.balances
            ]
            if balances:
                new_state.balances[address] = min(balances)
                new_state.nonces[address] = 0

            nonces = [
                branch.state.nonces.get(address, 0)
                for branch in branches
                if address in branch.state.nonces
            ]
            if nonces:
                new_state.nonces[address] = max(nonces)

        for branch in branches:
            new_state.applied_txs.update(branch.state.applied_txs)

        self.root_state = new_state
        self.merge_count += 1

        return self.root_state

    def get_balance(self, address: str) -> int:
        return self.root_state.get_balance(address)

    def stats(self) -> dict:
        return {
            "merge_count": self.merge_count,
            "total_addresses": len(self.root_state.balances),
            "root_balances": dict(self.root_state.balances),
        }