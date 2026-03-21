# app/token/ghost.py

from __future__ import annotations

import time
from dataclasses import dataclass, field

from app.config import (
    TOTAL_SUPPLY,
    GENESIS_SHARE,            
    ADDRESS_CAP,          
    BASE_REWARD_PER_HOUR,      
    HALVENING_INTERVAL,
    UPTIME_TIERS)

@dataclass
class NodeUptime:
    address: str
    first_seen: float = field(default_factory=time.time)
    last_seen: float = field(default_factory=time.time)
    continuous_since: float = field(default_factory=time.time)
    total_earned: int = 0

    def ping(self, now: float | None = None) -> None:
        now = now or time.time()
        if now - self.last_seen > 2 * 3600:
            self.continuous_since = now
        self.last_seen = now

    def continuous_uptime(self, now: float | None = None) -> float:
        now = now or time.time()
        return now - self.continuous_since


@dataclass
class GhostToken:
    network_start: float = field(default_factory=time.time)
    balances: dict[str, int] = field(default_factory=dict)
    total_minted: int = 0
    nodes: dict[str, NodeUptime] = field(default_factory=dict)

    def __post_init__(self) -> None:
        self.address_cap = int(TOTAL_SUPPLY * ADDRESS_CAP)
        self.genesis_supply = int(TOTAL_SUPPLY * GENESIS_SHARE)

    def genesis(self, founder_address: str) -> int:
        if self.total_minted > 0:
            return 0

        amount = min(self.genesis_supply, self.address_cap)
        self.balances[founder_address] = amount
        self.total_minted += amount
        return amount

    def register_node(self, address: str, now: float | None = None) -> None:
        now = now or time.time()
        if address not in self.nodes:
            self.nodes[address] = NodeUptime(address=address, first_seen=now,
                                             last_seen=now, continuous_since=now)

    def ping_node(self, address: str, now: float | None = None) -> None:
        now = now or time.time()
        if address not in self.nodes:
            self.register_node(address, now)
        self.nodes[address].ping(now)

    def claim_reward(self, address: str, now: float | None = None) -> int:
        now = now or time.time()

        if address not in self.nodes:
            return 0

        if self.total_minted >= TOTAL_SUPPLY:
            return 0

        node = self.nodes[address]
        continuous = node.continuous_uptime(now)

        multiplier = self._uptime_multiplier(continuous)

        halvening_multiplier = self._halvening_multiplier(now)

        reward = int(BASE_REWARD_PER_HOUR * multiplier * halvening_multiplier)

        if reward <= 0:
            return 0

        current_balance = self.balances.get(address, 0)
        available_cap = self.address_cap - current_balance
        if available_cap <= 0:
            return 0

        reward = min(reward, available_cap)

        remaining_supply = TOTAL_SUPPLY - self.total_minted
        reward = min(reward, remaining_supply)

        if reward <= 0:
            return 0

        self.balances[address] = current_balance + reward
        self.total_minted += reward
        node.total_earned += reward

        return reward

    def get_balance(self, address: str) -> int:
        return self.balances.get(address, 0)

    def supply_remaining(self) -> int:
        return TOTAL_SUPPLY - self.total_minted

    def supply_percentage(self) -> float:
        return round(self.total_minted / TOTAL_SUPPLY * 100, 4)

    def _uptime_multiplier(self, continuous_seconds: float) -> float:
        for threshold, multiplier in UPTIME_TIERS:
            if continuous_seconds <= threshold:
                return multiplier
        return UPTIME_TIERS[-1][1]

    def _halvening_multiplier(self, now: float) -> float:
        elapsed = now - self.network_start
        halvings = int(elapsed / HALVENING_INTERVAL)
        return 1.0 / (2 ** halvings)

    def stats(self) -> dict:
        return {
            "total_supply": TOTAL_SUPPLY,
            "total_minted": self.total_minted,
            "supply_remaining": self.supply_remaining(),
            "supply_percentage": self.supply_percentage(),
            "active_nodes": len(self.nodes),
            "address_cap": self.address_cap,
        }