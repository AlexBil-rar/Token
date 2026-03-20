# app/branches/coordinator.py

from __future__ import annotations

from collections import Counter
from dataclasses import dataclass, field

from app.branches.branch import Branch
from app.ledger.state import LedgerState


@dataclass
class Coordinator:
    root_state: LedgerState = field(default_factory=LedgerState)
    merge_count: int = 0

    def _quorum_size(self, total: int) -> int:
        return total // 2 + 1

    def merge(self, branches: list[Branch]) -> LedgerState:
        if not branches:
            return self.root_state

        total = len(branches)
        quorum = self._quorum_size(total)

        all_addresses: set[str] = set()
        for branch in branches:
            all_addresses.update(branch.state.balances.keys())

        new_state = LedgerState()

        for address in all_addresses:
            balance_votes = [
                branch.state.balances.get(address, 0)
                for branch in branches
                if address in branch.state.balances
            ]
            nonce_votes = [
                branch.state.nonces.get(address, 0)
                for branch in branches
                if address in branch.state.nonces
            ]

            if balance_votes:
                new_state.balances[address] = self._quorum_value(balance_votes, quorum)
            if nonce_votes:
                new_state.nonces[address] = self._quorum_value(nonce_votes, quorum)

        for branch in branches:
            new_state.applied_txs.update(branch.state.applied_txs)

        self.root_state = new_state
        self.merge_count += 1
        return self.root_state

    def _quorum_value(self, votes: list[int], quorum: int) -> int:
        counter = Counter(votes)
        for value, count in counter.most_common():
            if count >= quorum:
                return value
        return min(votes)

    def get_balance(self, address: str) -> int:
        return self.root_state.get_balance(address)

    def has_quorum(self, branches: list[Branch], address: str) -> bool:
        total = len(branches)
        quorum = self._quorum_size(total)
        votes = [
            branch.state.balances.get(address, 0)
            for branch in branches
            if address in branch.state.balances
        ]
        if not votes:
            return False
        counter = Counter(votes)
        _, top_count = counter.most_common(1)[0]
        return top_count >= quorum

    def stats(self) -> dict:
        return {
            "merge_count": self.merge_count,
            "total_addresses": len(self.root_state.balances),
            "root_balances": dict(self.root_state.balances),
        }