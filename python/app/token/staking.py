# app/token/staking.py

from __future__ import annotations

import time
from dataclasses import dataclass, field
from enum import Enum

from app.config import(
MIN_STAKE,
SLASH_PERCENT,
SLASH_BURN_RATIO,
SLASH_POOL_RATIO,
MAX_VIOLATIONS
)

class StakeStatus(Enum):
    ACTIVE = "active"       
    SLASHED = "slashed"   
    EJECTED = "ejected"      
    WITHDRAWN = "withdrawn"  


class ViolationType(Enum):
    DOUBLE_VOTE = "double_vote"          
    CONFLICTING_TX = "conflicting_tx"    
    REPUTATION_PENALTY = "reputation_penalty" 
    INVALID_STATE = "invalid_state"      


@dataclass
class StakeRecord:
    """Запись о стейке одного узла."""
    address: str
    amount: int                      
    original_amount: int                
    staked_at: float = field(default_factory=time.time)
    status: StakeStatus = StakeStatus.ACTIVE
    violations: list[dict] = field(default_factory=list)
    total_slashed: int = 0
    last_active: float = field(default_factory=time.time)

    def is_active(self) -> bool:
        return self.status == StakeStatus.ACTIVE

    def violation_count(self) -> int:
        return len(self.violations)

    def stake_ratio(self) -> float:
        """Какая доля от начального стейка осталась."""
        if self.original_amount == 0:
            return 0.0
        return self.amount / self.original_amount


@dataclass
class SlashResult:
    slashed_amount: int
    burned: int
    to_pool: int
    ejected: bool
    reason: str


class StakingManager:
    def __init__(self) -> None:
        self.stakes: dict[str, StakeRecord] = {}
        self.slash_pool: int = 0       
        self.total_burned: int = 0   

    def stake(self, address: str, amount: int, token_balances: dict) -> tuple[bool, str]:
        if amount < MIN_STAKE:
            return False, f"minimum stake is {MIN_STAKE} GHOST"

        if address in self.stakes and self.stakes[address].is_active():
            return False, "already staking"

        current_balance = token_balances.get(address, 0)
        if current_balance < amount:
            return False, "insufficient balance"

        token_balances[address] = current_balance - amount

        self.stakes[address] = StakeRecord(
            address=address,
            amount=amount,
            original_amount=amount,
        )
        return True, "staked"

    def slash(
        self,
        address: str,
        violation: ViolationType,
        evidence: str = "",
    ) -> SlashResult | None:
        if address not in self.stakes:
            return None

        record = self.stakes[address]
        if record.status == StakeStatus.EJECTED or record.status == StakeStatus.WITHDRAWN:
            return None

        record.violations.append({
            "type": violation.value,
            "evidence": evidence,
            "timestamp": time.time(),
        })

        slash_amount = int(record.amount * SLASH_PERCENT)
        slash_amount = min(slash_amount, record.amount)

        burned = int(slash_amount * SLASH_BURN_RATIO)
        to_pool = slash_amount - burned

        record.amount -= slash_amount
        record.total_slashed += slash_amount
        record.status = StakeStatus.SLASHED
        self.total_burned += burned
        self.slash_pool += to_pool

        ejected = False

        if record.violation_count() >= MAX_VIOLATIONS:
            remaining = record.amount
            burned_remaining = int(remaining * SLASH_BURN_RATIO)
            pool_remaining = remaining - burned_remaining

            self.total_burned += burned_remaining
            self.slash_pool += pool_remaining
            record.amount = 0
            record.status = StakeStatus.EJECTED
            ejected = True
        else:
            if record.amount >= MIN_STAKE:
                record.status = StakeStatus.ACTIVE

        return SlashResult(
            slashed_amount=slash_amount,
            burned=burned,
            to_pool=to_pool,
            ejected=ejected,
            reason=violation.value,
        )

    def withdraw(
        self,
        address: str,
        token_balances: dict,
    ) -> tuple[bool, str, int]:
        if address not in self.stakes:
            return False, "not staking", 0

        record = self.stakes[address]

        if record.status == StakeStatus.EJECTED:
            return False, "ejected nodes cannot withdraw", 0

        if record.status == StakeStatus.WITHDRAWN:
            return False, "already withdrawn", 0

        amount = record.amount
        token_balances[address] = token_balances.get(address, 0) + amount
        record.amount = 0
        record.status = StakeStatus.WITHDRAWN

        return True, "withdrawn", amount

    def distribute_slash_pool(self, token_balances: dict) -> int:
        if self.slash_pool == 0:
            return 0

        active_nodes = [
            addr for addr, record in self.stakes.items()
            if record.is_active() and record.violation_count() == 0
        ]

        if not active_nodes:
            return 0

        per_node = self.slash_pool // len(active_nodes)
        if per_node == 0:
            return 0

        distributed = 0
        for addr in active_nodes:
            token_balances[addr] = token_balances.get(addr, 0) + per_node
            distributed += per_node

        self.slash_pool -= distributed
        return distributed

    def is_eligible(self, address: str) -> bool:
        if address not in self.stakes:
            return False
        record = self.stakes[address]
        return record.is_active() and record.amount >= MIN_STAKE

    def get_stake_weight(self, address: str) -> float:
        if not self.is_eligible(address):
            return 0.0
        record = self.stakes[address]
        return record.stake_ratio()

    def stats(self) -> dict:
        total_staked = sum(r.amount for r in self.stakes.values())
        active = sum(1 for r in self.stakes.values() if r.is_active())
        ejected = sum(1 for r in self.stakes.values() if r.status == StakeStatus.EJECTED)
        return {
            "total_stakers": len(self.stakes),
            "active_stakers": active,
            "ejected": ejected,
            "total_staked": total_staked,
            "slash_pool": self.slash_pool,
            "total_burned": self.total_burned,
            "min_stake": MIN_STAKE,
        }