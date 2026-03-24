// consensus/src/conflict_resolver.rs  — v4

use std::collections::{HashMap, HashSet};
use ledger::transaction::{TransactionVertex, TxStatus};
use ledger::dag::DAG;

pub const RESOLUTION_MIN_WEIGHT: u64 = 3;
pub const MAX_STAKE_INFLUENCE: f64   = 3.0;
pub const CLOSURE_SIGMA: f64         = 2.0;
pub const CHECKPOINT_MIN_WEIGHT: u64 = 6;

// ═══════════════════════════════════════════════════════════════════════════
// ConflictStatus — 5-state machine
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictStatus {
    Pending,
    Ready,
    ClosedLocal { winner: String },
    Reconciling,
    ClosedGlobal { winner: String },
}

impl ConflictStatus {
    pub fn is_globally_final(&self) -> bool {
        matches!(self, ConflictStatus::ClosedGlobal { .. })
    }

    pub fn is_any_closed(&self) -> bool {
        matches!(self,
            ConflictStatus::ClosedLocal { .. } |
            ConflictStatus::ClosedGlobal { .. }
        )
    }

    pub fn winner(&self) -> Option<&str> {
        match self {
            ConflictStatus::ClosedLocal  { winner } => Some(winner.as_str()),
            ConflictStatus::ClosedGlobal { winner } => Some(winner.as_str()),
            _ => None,
        }
    }

    pub fn can_transition_to(&self, next: &ConflictStatus) -> bool {
        match (self, next) {
            (ConflictStatus::Pending,              ConflictStatus::Ready)              => true,
            (ConflictStatus::Ready,                ConflictStatus::ClosedLocal { .. }) => true,
            (ConflictStatus::ClosedLocal { .. },   ConflictStatus::Reconciling)        => true,
            (ConflictStatus::Reconciling,          ConflictStatus::ClosedGlobal { .. })=> true,
            (ConflictStatus::Reconciling,          ConflictStatus::Ready)              => true,
            (ConflictStatus::ClosedGlobal { .. },  _)                                 => false,
            (ConflictStatus::Pending,              ConflictStatus::ClosedLocal { .. }) => true,
            _ => false,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PartitionState — per-conflict partition metadata
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct PartitionState {
    pub status: ConflictStatus,
    pub local_anchor_id: Option<String>,
    pub global_anchor_id: Option<String>,
    pub frozen_stake: Option<HashMap<String, f64>>,
    pub frozen_total_stake: f64,
}

impl PartitionState {
    pub fn new() -> Self {
        PartitionState {
            status: ConflictStatus::Pending,
            local_anchor_id: None,
            global_anchor_id: None,
            frozen_stake: None,
            frozen_total_stake: 0.0,
        }
    }

    pub fn set_closed_local(
        &mut self,
        winner: String,
        anchor_id: String,
        stake_at_cp: HashMap<String, f64>,
        total_at_cp: f64,
    ) {
        let next = ConflictStatus::ClosedLocal { winner };
        debug_assert!(
            self.status.can_transition_to(&next),
            "Invalid transition: {:?} → ClosedLocal", self.status
        );
        self.status = next;
        self.local_anchor_id = Some(anchor_id);
        self.frozen_stake = Some(stake_at_cp);
        self.frozen_total_stake = total_at_cp;
    }

    pub fn downgrade_to_reconciling(&mut self) {
        debug_assert!(
            matches!(self.status, ConflictStatus::ClosedLocal { .. }),
            "downgrade_to_reconciling called on non-ClosedLocal: {:?}", self.status
        );
        self.status = ConflictStatus::Reconciling;
    }

    pub fn set_closed_global(&mut self, winner: String, global_anchor_id: String) {
        debug_assert!(
            matches!(self.status, ConflictStatus::Reconciling),
            "set_closed_global called on non-Reconciling: {:?}", self.status
        );
        self.global_anchor_id = Some(global_anchor_id);
        self.status = ConflictStatus::ClosedGlobal { winner };
    }

    pub fn reconciling_to_ready(&mut self) {
        debug_assert!(
            matches!(self.status, ConflictStatus::Reconciling),
            "reconciling_to_ready called on non-Reconciling: {:?}", self.status
        );
        self.status = ConflictStatus::Ready;
    }
}

impl Default for PartitionState {
    fn default() -> Self { Self::new() }
}

// ═══════════════════════════════════════════════════════════════════════════
// CheckpointAnchor
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct CheckpointAnchor {
    pub checkpoint_id: String,
    pub dag_height: u64,
    pub weight: u64,
    descendant_tx_ids: HashSet<String>,
}

impl CheckpointAnchor {
    pub fn new(checkpoint_id: String, dag_height: u64, weight: u64) -> Self {
        CheckpointAnchor {
            checkpoint_id, dag_height, weight,
            descendant_tx_ids: HashSet::new(),
        }
    }

    pub fn from_dag(checkpoint_id: String, dag_height: u64, weight: u64, dag: &DAG) -> Self {
        let descendants = dag.descendants_of(&checkpoint_id);
        CheckpointAnchor { checkpoint_id, dag_height, weight, descendant_tx_ids: descendants }
    }

    pub fn refresh(&mut self, dag: &DAG) {
        self.descendant_tx_ids = dag.descendants_of(&self.checkpoint_id);
    }

    pub fn is_finalized(&self) -> bool { self.weight >= CHECKPOINT_MIN_WEIGHT }

    pub fn is_ancestor_of(&self, tx_id: &str) -> bool {
        self.descendant_tx_ids.contains(tx_id)
    }

    pub fn register_descendant(&mut self, tx_id: String) {
        self.descendant_tx_ids.insert(tx_id);
    }

    pub fn descendant_count(&self) -> usize { self.descendant_tx_ids.len() }
}

// ═══════════════════════════════════════════════════════════════════════════
// ClosureResult
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub enum ClosureResult {
    NotConflict,
    AlreadyResolved { winner: String },
    NotReady { pending_ids: Vec<String> },
    NotAnchored,
    NotDominant { leader: String, leader_score: f64, second_score: f64, required_ratio: f64 },
    Closed { winner_id: String, winner_score: f64, second_score: f64 },
    InsufficientData,
}

impl ClosureResult {
    pub fn is_closed(&self) -> bool { matches!(self, ClosureResult::Closed { .. }) }

    pub fn winner(&self) -> Option<&str> {
        match self {
            ClosureResult::Closed { winner_id, .. }      => Some(winner_id.as_str()),
            ClosureResult::AlreadyResolved { winner }    => Some(winner.as_str()),
            ClosureResult::NotDominant { leader, .. }    => Some(leader.as_str()),
            _ => None,
        }
    }

    // constructors
    pub fn not_conflict()                    -> Self { ClosureResult::NotConflict }
    pub fn already_resolved(w: String)       -> Self { ClosureResult::AlreadyResolved { winner: w } }
    pub fn not_ready(ids: Vec<String>)       -> Self { ClosureResult::NotReady { pending_ids: ids } }
    pub fn not_anchored()                    -> Self { ClosureResult::NotAnchored }
    pub fn not_dominant(l: String, ls: f64, ss: f64, sigma: f64) -> Self {
        ClosureResult::NotDominant { leader: l, leader_score: ls, second_score: ss, required_ratio: sigma }
    }
    pub fn closed(w: String, ws: f64, ss: f64) -> Self {
        ClosureResult::Closed { winner_id: w, winner_score: ws, second_score: ss }
    }
    pub fn insufficient_data()               -> Self { ClosureResult::InsufficientData }
}

// ═══════════════════════════════════════════════════════════════════════════
// ConflictResolver
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Default)]
pub struct ConflictResolver {
    conflict_sets: HashMap<(String, u64), Vec<String>>,
    partition_states: HashMap<(String, u64), PartitionState>,
    resolved: HashMap<(String, u64), String>,
}

impl ConflictResolver {
    pub fn new() -> Self { ConflictResolver::default() }

    // ── Registration ──────────────────────────────────────────────────────

    pub fn register_transaction(&mut self, tx: &TransactionVertex) {
        let key = (tx.sender.clone(), tx.nonce);
        let set = self.conflict_sets.entry(key.clone()).or_default();
        if !set.contains(&tx.tx_id) {
            set.push(tx.tx_id.clone());
        }
        self.partition_states.entry(key).or_default();
    }

    pub fn get_conflicts(&self, tx: &TransactionVertex) -> Vec<String> {
        let key = (tx.sender.clone(), tx.nonce);
        self.conflict_sets.get(&key).cloned().unwrap_or_default()
            .into_iter().filter(|id| id != &tx.tx_id).collect()
    }

    // ── Status accessors ──────────────────────────────────────────────────

    pub fn partition_status(&self, sender: &str, nonce: u64) -> ConflictStatus {
        let key = (sender.to_string(), nonce);
        self.partition_states
            .get(&key)
            .map(|s| s.status.clone())
            .unwrap_or(ConflictStatus::Pending)
    }

    pub fn conflict_status(&self, tx: &TransactionVertex, dag: &DAG) -> ConflictStatus {
        let key = (tx.sender.clone(), tx.nonce);

        if let Some(ps) = self.partition_states.get(&key) {
            if ps.status.is_any_closed() {
                return ps.status.clone();
            }
        }

        let ids = match self.conflict_sets.get(&key) {
            Some(v) if v.len() > 1 => v,
            _ => return ConflictStatus::Pending,
        };
        let all_ready = ids.iter().all(|id| {
            dag.get_transaction(id)
                .map(|t| t.weight >= RESOLUTION_MIN_WEIGHT)
                .unwrap_or(false)
        });
        if all_ready { ConflictStatus::Ready } else { ConflictStatus::Pending }
    }

    pub fn winner_of(&self, sender: &str, nonce: u64) -> Option<&String> {
        self.resolved.get(&(sender.to_string(), nonce))
    }

    pub fn resolved_count(&self) -> usize { self.resolved.len() }

    // ── Closure predicate: is_closed() ────────────────────────────────────

    pub fn is_closed(
        &self,
        sender: &str,
        nonce: u64,
        dag: &DAG,
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
        anchor: Option<&CheckpointAnchor>,
    ) -> ClosureResult {
        let key = (sender.to_string(), nonce);

        if let Some(ps) = self.partition_states.get(&key) {
            if let ConflictStatus::ClosedGlobal { winner } = &ps.status {
                return ClosureResult::already_resolved(winner.clone());
            }
            if let ConflictStatus::ClosedLocal { winner } = &ps.status {
                return ClosureResult::already_resolved(winner.clone());
            }
        }

        let ids = match self.conflict_sets.get(&key) {
            Some(v) if v.len() > 1 => v.clone(),
            _ => return ClosureResult::not_conflict(),
        };

        // ── (1) READY ──────────────────────────────────────────────────────
        let not_ready: Vec<String> = ids.iter()
            .filter(|id| dag.get_transaction(id)
                .map(|t| t.weight < RESOLUTION_MIN_WEIGHT)
                .unwrap_or(true))
            .cloned().collect();
        if !not_ready.is_empty() {
            return ClosureResult::not_ready(not_ready);
        }

        // ── (2) ANCHORED ───────────────────────────────────────────────────
        let anchored = match anchor {
            None     => false,
            Some(cp) => cp.is_finalized() && ids.iter().all(|id| cp.is_ancestor_of(id)),
        };
        if !anchored {
            return ClosureResult::not_anchored();
        }

        // ── (3) DOMINANT ───────────────────────────────────────────────────
        let scores = Self::compute_scores(dag, &ids, stake_weights, total_stake);
        if scores.is_empty() {
            return ClosureResult::insufficient_data();
        }

        let (winner_id, winner_score) = match Self::pick_winner_with_score(&scores) {
            Some(w) => w,
            None    => return ClosureResult::insufficient_data(),
        };

        let second_score = scores.iter()
            .filter(|(id, _)| id != &winner_id)
            .map(|(_, s)| *s)
            .fold(0.0_f64, f64::max);

        let dominant = second_score == 0.0 || winner_score >= second_score * CLOSURE_SIGMA;
        if !dominant {
            return ClosureResult::not_dominant(winner_id, winner_score, second_score, CLOSURE_SIGMA);
        }

        ClosureResult::closed(winner_id, winner_score, second_score)
    }

    // ── Local closure ─────────────────────────────────────────────────────

    pub fn try_close_local(
        &mut self,
        sender: &str,
        nonce: u64,
        dag: &DAG,
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
        anchor: &CheckpointAnchor,
    ) -> Option<String> {
        let key = (sender.to_string(), nonce);

        if let Some(ps) = self.partition_states.get(&key) {
            if ps.status.is_globally_final() { return None; }
            if ps.status.is_any_closed()     { return ps.status.winner().map(str::to_string); }
        }

        let result = self.is_closed(sender, nonce, dag, stake_weights, total_stake, Some(anchor));
        if let ClosureResult::Closed { winner_id, .. } = result {
            let ps = self.partition_states.entry(key.clone()).or_default();
            ps.set_closed_local(
                winner_id.clone(),
                anchor.checkpoint_id.clone(),
                stake_weights.clone(),
                total_stake,
            );
            self.resolved.insert(key, winner_id.clone());
            return Some(winner_id);
        }
        None
    }

    // ── Partition Healing Algorithm (PHA) ─────────────────────────────────

    pub fn pha_downgrade_above(
        &mut self,
        cp_star: &CheckpointAnchor,
    ) -> Vec<(String, u64)> {
        let keys: Vec<(String, u64)> = self.conflict_sets.keys().cloned().collect();
        let mut downgraded = Vec::new();

        for key in keys {
            let ids = match self.conflict_sets.get(&key) {
                Some(v) => v.clone(),
                None    => continue,
            };

            let all_above = ids.iter().all(|id| cp_star.is_ancestor_of(id));
            if !all_above { continue; }

            let ps = self.partition_states.entry(key.clone()).or_default();
            if matches!(ps.status, ConflictStatus::ClosedLocal { .. }) {
                ps.downgrade_to_reconciling();
                downgraded.push(key);
            }
        }
        downgraded
    }

    pub fn pha_re_evaluate(
        &mut self,
        dag: &DAG,
        cp_star: &CheckpointAnchor,
    ) -> (usize, usize) {
        let keys: Vec<(String, u64)> = self.partition_states.keys()
            .filter(|k| matches!(
                self.partition_states.get(*k).map(|ps| &ps.status),
                Some(ConflictStatus::Reconciling)
            ))
            .cloned()
            .collect();

        let mut globally_closed = 0;
        let mut still_pending   = 0;

        for key in keys {
            let (frozen_stake, frozen_total) = {
                let ps = self.partition_states.get(&key).unwrap();
                (
                    ps.frozen_stake.clone().unwrap_or_default(),
                    ps.frozen_total_stake,
                )
            };

            let result = self.is_closed_raw(
                &key.0, key.1, dag, &frozen_stake, frozen_total, Some(cp_star),
            );

            let ps = self.partition_states.get_mut(&key).unwrap();
            match result {
                ClosureResult::Closed { winner_id, .. } => {
                    ps.set_closed_global(winner_id.clone(), cp_star.checkpoint_id.clone());
                    self.resolved.insert(key, winner_id);
                    globally_closed += 1;
                }
                _ => {
                    ps.reconciling_to_ready();
                    still_pending += 1;
                }
            }
        }

        (globally_closed, still_pending)
    }

    fn is_closed_raw(
        &self,
        sender: &str,
        nonce: u64,
        dag: &DAG,
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
        anchor: Option<&CheckpointAnchor>,
    ) -> ClosureResult {
        let key = (sender.to_string(), nonce);
        let ids = match self.conflict_sets.get(&key) {
            Some(v) if v.len() > 1 => v.clone(),
            _ => return ClosureResult::not_conflict(),
        };

        // READY
        let not_ready: Vec<String> = ids.iter()
            .filter(|id| dag.get_transaction(id)
                .map(|t| t.weight < RESOLUTION_MIN_WEIGHT)
                .unwrap_or(true))
            .cloned().collect();
        if !not_ready.is_empty() { return ClosureResult::not_ready(not_ready); }

        // ANCHORED
        let anchored = match anchor {
            None     => false,
            Some(cp) => cp.is_finalized() && ids.iter().all(|id| cp.is_ancestor_of(id)),
        };
        if !anchored { return ClosureResult::not_anchored(); }

        // DOMINANT
        let scores = Self::compute_scores(dag, &ids, stake_weights, total_stake);
        if scores.is_empty() { return ClosureResult::insufficient_data(); }

        let (winner_id, winner_score) = match Self::pick_winner_with_score(&scores) {
            Some(w) => w,
            None    => return ClosureResult::insufficient_data(),
        };

        let second_score = scores.iter()
            .filter(|(id, _)| id != &winner_id)
            .map(|(_, s)| *s)
            .fold(0.0_f64, f64::max);

        if second_score != 0.0 && winner_score < second_score * CLOSURE_SIGMA {
            return ClosureResult::not_dominant(winner_id, winner_score, second_score, CLOSURE_SIGMA);
        }

        ClosureResult::closed(winner_id, winner_score, second_score)
    }

    // ── Batch resolution (legacy + updated) ───────────────────────────────

    pub fn resolve_closed(
        &mut self,
        dag: &mut DAG,
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
        anchor: Option<&CheckpointAnchor>,
    ) -> Vec<(String, String)> {
        let candidates: Vec<(String, u64)> = self.conflict_sets.keys()
            .filter(|k| {
                self.partition_states.get(*k)
                    .map(|ps| !ps.status.is_any_closed())
                    .unwrap_or(true)
            })
            .cloned()
            .collect();

        let mut losers = Vec::new();

        for (sender, nonce) in candidates {
            let anchor_ref = match anchor {
                Some(a) => a,
                None    => continue,
            };
            if let Some(winner_id) = self.try_close_local(
                &sender, nonce, dag, stake_weights, total_stake, anchor_ref,
            ) {
                let ids = self.conflict_sets
                    .get(&(sender.clone(), nonce))
                    .cloned()
                    .unwrap_or_default();
                for id in &ids {
                    if id != &winner_id {
                        if let Some(t) = dag.get_transaction_mut(id) {
                            t.status = TxStatus::Conflict;
                            losers.push((id.clone(), t.sender.clone()));
                        }
                    }
                }
            }
        }
        losers
    }

    pub fn resolve(&self, dag: &mut DAG, tx: &TransactionVertex) {
        self.resolve_with_stake(dag, tx, &HashMap::new(), 0.0);
    }

    pub fn resolve_with_stake(
        &self,
        dag: &mut DAG,
        tx: &TransactionVertex,
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
    ) {
        let conflicts = self.get_conflicts(tx);
        if conflicts.is_empty() { return; }
        let all_ids: Vec<String> = conflicts.iter()
            .chain(std::iter::once(&tx.tx_id))
            .cloned().collect();
        let scores = Self::compute_scores(dag, &all_ids, stake_weights, total_stake);
        if scores.is_empty() { return; }
        if let Some((winner, _)) = Self::pick_winner_with_score(&scores) {
            for id in &all_ids {
                if id != &winner {
                    if let Some(t) = dag.get_transaction_mut(id) {
                        t.status = TxStatus::Conflict;
                    }
                }
            }
        }
    }

    pub fn resolve_ready(
        &mut self,
        dag: &mut DAG,
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
    ) -> Vec<String> {
        let ready_keys: Vec<(String, u64)> = self.conflict_sets.iter()
            .filter(|(key, ids)| {
                let not_closed = self.partition_states.get(*key)
                    .map(|ps| !ps.status.is_any_closed())
                    .unwrap_or(true);
                not_closed && ids.len() > 1 &&
                ids.iter().all(|id| dag.get_transaction(id)
                    .map(|t| t.weight >= RESOLUTION_MIN_WEIGHT)
                    .unwrap_or(false))
            })
            .map(|(k, _)| k.clone())
            .collect();

        let mut resolved_winners = Vec::new();
        for key in ready_keys {
            let ids = match self.conflict_sets.get(&key) {
                Some(v) => v.clone(), None => continue,
            };
            let scores = Self::compute_scores(dag, &ids, stake_weights, total_stake);
            if scores.is_empty() { continue; }
            if let Some((winner_id, _)) = Self::pick_winner_with_score(&scores) {
                for id in &ids {
                    if id != &winner_id {
                        if let Some(t) = dag.get_transaction_mut(id) {
                            t.status = TxStatus::Conflict;
                        }
                    }
                }
                self.resolved.insert(key, winner_id.clone());
                resolved_winners.push(winner_id);
            }
        }
        resolved_winners
    }

    pub fn resolve_all_with_stake(
        &mut self,
        dag: &mut DAG,
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
    ) {
        let keys: Vec<(String, u64)> = self.conflict_sets.iter()
            .filter(|(_, ids)| ids.len() > 1)
            .map(|(k, _)| k.clone()).collect();
        for key in keys {
            if self.partition_states.get(&key)
                .map(|ps| ps.status.is_any_closed())
                .unwrap_or(false) { continue; }
            let ids = match self.conflict_sets.get(&key) {
                Some(v) => v.clone(), None => continue,
            };
            let scores = Self::compute_scores(dag, &ids, stake_weights, total_stake);
            if scores.is_empty() { continue; }
            if let Some((winner_id, _)) = Self::pick_winner_with_score(&scores) {
                for id in &ids {
                    if id != &winner_id {
                        if let Some(t) = dag.get_transaction_mut(id) {
                            t.status = TxStatus::Conflict;
                        }
                    }
                }
                self.resolved.insert(key, winner_id);
            }
        }
    }

    // ── Scoring ───────────────────────────────────────────────────────────

    pub fn compute_scores(
        dag: &DAG,
        ids: &[String],
        stake_weights: &HashMap<String, f64>,
        total_stake: f64,
    ) -> Vec<(String, f64)> {
        ids.iter().filter_map(|id| {
            let tx = dag.get_transaction(id)?;
            let stake = stake_weights.get(&tx.sender).copied().unwrap_or(0.0);
            let ratio = if total_stake > 0.0 {
                (stake / total_stake).clamp(0.0, 1.0)
            } else { 0.0 };
            let multiplier = 1.0 + ratio * (MAX_STAKE_INFLUENCE - 1.0);
            Some((id.clone(), tx.weight as f64 * multiplier))
        }).collect()
    }

    fn pick_winner_with_score(scores: &[(String, f64)]) -> Option<(String, f64)> {
        scores.iter()
            .max_by(|(id_a, sa), (id_b, sb)| {
                sa.partial_cmp(sb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| id_b.cmp(id_a))
            })
            .map(|(id, score)| (id.clone(), *score))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx(tx_id: &str, sender: &str, nonce: u64) -> TransactionVertex {
        let mut tx = TransactionVertex::new(
            sender.to_string(), "bob".to_string(),
            100, nonce, 1000, "pk".to_string(), vec![],
        );
        tx.tx_id = tx_id.to_string();
        tx
    }

    fn make_tx_w(tx_id: &str, sender: &str, nonce: u64, weight: u64) -> TransactionVertex {
        let mut tx = make_tx(tx_id, sender, nonce);
        tx.weight = weight;
        tx
    }

    fn build_dag_with_cp(cp_id: &str, txs: Vec<(&str, &str, u64, u64)>) -> DAG {
        let mut dag = DAG::new();
        let mut cp = make_tx(cp_id, "system", 0);
        cp.weight = CHECKPOINT_MIN_WEIGHT;
        dag.add_transaction(cp).unwrap();
        for (tx_id, sender, nonce, weight) in txs {
            let mut tx = make_tx_w(tx_id, sender, nonce, weight);
            tx.parents = vec![cp_id.to_string()];
            dag.add_transaction(tx).unwrap();
            dag.children_map.entry(cp_id.to_string()).or_default()
                .insert(tx_id.to_string());
        }
        dag
    }

    fn make_anchor(cp_id: &str, descendants: Vec<&str>) -> CheckpointAnchor {
        let mut a = CheckpointAnchor::new(cp_id.to_string(), 0, CHECKPOINT_MIN_WEIGHT);
        for d in descendants { a.register_descendant(d.to_string()); }
        a
    }

    // ── ConflictStatus state machine ──────────────────────────────────────

    #[test]
    fn test_status_transitions_valid() {
        let pending    = ConflictStatus::Pending;
        let ready      = ConflictStatus::Ready;
        let local      = ConflictStatus::ClosedLocal { winner: "tx1".into() };
        let recon      = ConflictStatus::Reconciling;
        let global     = ConflictStatus::ClosedGlobal { winner: "tx1".into() };

        assert!(pending.can_transition_to(&ready));
        assert!(ready.can_transition_to(&local));
        assert!(local.can_transition_to(&recon));
        assert!(recon.can_transition_to(&global));
        assert!(recon.can_transition_to(&ready));
    }

    #[test]
    fn test_status_global_final_no_transitions() {
        let global = ConflictStatus::ClosedGlobal { winner: "tx1".into() };
        let any_next = [
            ConflictStatus::Pending,
            ConflictStatus::Ready,
            ConflictStatus::ClosedLocal { winner: "tx2".into() },
            ConflictStatus::Reconciling,
            ConflictStatus::ClosedGlobal { winner: "tx2".into() },
        ];
        for next in &any_next {
            assert!(!global.can_transition_to(next),
                "ClosedGlobal should not transition to {:?}", next);
        }
    }

    #[test]
    fn test_status_is_globally_final() {
        assert!(!ConflictStatus::Pending.is_globally_final());
        assert!(!ConflictStatus::Ready.is_globally_final());
        assert!(!ConflictStatus::ClosedLocal { winner: "t".into() }.is_globally_final());
        assert!(!ConflictStatus::Reconciling.is_globally_final());
        assert!(ConflictStatus::ClosedGlobal { winner: "t".into() }.is_globally_final());
    }

    #[test]
    fn test_winner_accessor() {
        assert_eq!(ConflictStatus::ClosedLocal { winner: "tx1".into() }.winner(), Some("tx1"));
        assert_eq!(ConflictStatus::ClosedGlobal { winner: "tx2".into() }.winner(), Some("tx2"));
        assert_eq!(ConflictStatus::Pending.winner(), None);
        assert_eq!(ConflictStatus::Reconciling.winner(), None);
    }

    // ── PartitionState transitions ────────────────────────────────────────

    #[test]
    fn test_partition_state_happy_path() {
        let mut ps = PartitionState::new();
        assert_eq!(ps.status, ConflictStatus::Pending);

        ps.set_closed_local(
            "tx1".into(), "cp1".into(),
            HashMap::new(), 0.0,
        );
        assert!(matches!(ps.status, ConflictStatus::ClosedLocal { .. }));
        assert_eq!(ps.local_anchor_id, Some("cp1".into()));

        ps.downgrade_to_reconciling();
        assert_eq!(ps.status, ConflictStatus::Reconciling);

        ps.set_closed_global("tx1".into(), "cp1".into());
        assert!(ps.status.is_globally_final());
        assert_eq!(ps.global_anchor_id, Some("cp1".into()));
    }

    #[test]
    fn test_partition_state_reconciling_to_ready() {
        let mut ps = PartitionState::new();
        ps.set_closed_local("tx1".into(), "cp".into(), HashMap::new(), 0.0);
        ps.downgrade_to_reconciling();
        ps.reconciling_to_ready();
        assert_eq!(ps.status, ConflictStatus::Ready);
    }

    #[test]
    fn test_frozen_stake_preserved() {
        let mut stake = HashMap::new();
        stake.insert("alice".to_string(), 1000.0);

        let mut ps = PartitionState::new();
        ps.set_closed_local("tx1".into(), "cp".into(), stake.clone(), 1000.0);

        let frozen = ps.frozen_stake.as_ref().unwrap();
        assert_eq!(frozen.get("alice"), Some(&1000.0));
        assert_eq!(ps.frozen_total_stake, 1000.0);
    }

    // ── try_close_local ───────────────────────────────────────────────────

    #[test]
    fn test_try_close_local_succeeds() {
        let dag = build_dag_with_cp("cp", vec![
            ("tx1", "alice", 1, 8),
            ("tx2", "alice", 1, 3),
        ]);
        let anchor = CheckpointAnchor::from_dag(
            "cp".into(), 0, CHECKPOINT_MIN_WEIGHT, &dag
        );

        let mut r = ConflictResolver::new();
        r.register_transaction(dag.get_transaction("tx1").unwrap());
        r.register_transaction(dag.get_transaction("tx2").unwrap());

        let winner = r.try_close_local(
            "alice", 1, &dag, &HashMap::new(), 0.0, &anchor
        );
        assert_eq!(winner, Some("tx1".into()));
        assert!(matches!(
            r.partition_status("alice", 1),
            ConflictStatus::ClosedLocal { winner } if winner == "tx1"
        ));
    }

    #[test]
    fn test_try_close_local_not_dominant() {
        let dag = build_dag_with_cp("cp", vec![
            ("tx1", "alice", 1, 4),
            ("tx2", "alice", 1, 3),
        ]);
        let anchor = CheckpointAnchor::from_dag(
            "cp".into(), 0, CHECKPOINT_MIN_WEIGHT, &dag
        );

        let mut r = ConflictResolver::new();
        r.register_transaction(dag.get_transaction("tx1").unwrap());
        r.register_transaction(dag.get_transaction("tx2").unwrap());

        let winner = r.try_close_local(
            "alice", 1, &dag, &HashMap::new(), 0.0, &anchor
        );
        assert_eq!(winner, None);
        assert_eq!(r.partition_status("alice", 1), ConflictStatus::Pending);
    }

    #[test]
    fn test_try_close_local_idempotent() {
        let dag = build_dag_with_cp("cp", vec![
            ("tx1", "alice", 1, 8),
            ("tx2", "alice", 1, 3),
        ]);
        let anchor = CheckpointAnchor::from_dag(
            "cp".into(), 0, CHECKPOINT_MIN_WEIGHT, &dag
        );

        let mut r = ConflictResolver::new();
        r.register_transaction(dag.get_transaction("tx1").unwrap());
        r.register_transaction(dag.get_transaction("tx2").unwrap());

        let w1 = r.try_close_local("alice", 1, &dag, &HashMap::new(), 0.0, &anchor);
        let w2 = r.try_close_local("alice", 1, &dag, &HashMap::new(), 0.0, &anchor);
        assert_eq!(w1, w2);
        assert!(matches!(r.partition_status("alice", 1), ConflictStatus::ClosedLocal { .. }));
    }

    // ── PHA: pha_downgrade_above ──────────────────────────────────────────

    #[test]
    fn test_pha_downgrade_above_only_above_cp() {
        let mut dag = DAG::new();
        let mut cp = make_tx("cp_star", "system", 0);
        cp.weight = CHECKPOINT_MIN_WEIGHT;
        dag.add_transaction(cp).unwrap();

        for (id, w) in [("tx1", 8u64), ("tx2", 3u64)] {
            let mut tx = make_tx_w(id, "alice", 1, w);
            tx.parents = vec!["cp_star".into()];
            dag.add_transaction(tx).unwrap();
            dag.children_map.entry("cp_star".into()).or_default().insert(id.into());
        }

        dag.add_transaction(make_tx_w("tx3", "bob", 2, 8)).unwrap();
        dag.add_transaction(make_tx_w("tx4", "bob", 2, 3)).unwrap();

        let cp_star_anchor = CheckpointAnchor::from_dag(
            "cp_star".into(), 0, CHECKPOINT_MIN_WEIGHT, &dag
        );

        let mut r = ConflictResolver::new();
        for id in ["tx1", "tx2", "tx3", "tx4"] {
            r.register_transaction(dag.get_transaction(id).unwrap());
        }

        let a1 = CheckpointAnchor::from_dag("cp_star".into(), 0, CHECKPOINT_MIN_WEIGHT, &dag);
        r.try_close_local("alice", 1, &dag, &HashMap::new(), 0.0, &a1);

        let mut a2 = CheckpointAnchor::new("other_cp".into(), 0, CHECKPOINT_MIN_WEIGHT);
        a2.register_descendant("tx3".into());
        a2.register_descendant("tx4".into());
        r.try_close_local("bob", 2, &dag, &HashMap::new(), 0.0, &a2);

        let downgraded = r.pha_downgrade_above(&cp_star_anchor);

        assert_eq!(downgraded.len(), 1);
        assert!(downgraded.contains(&("alice".into(), 1)));

        assert_eq!(r.partition_status("alice", 1), ConflictStatus::Reconciling);
        assert!(matches!(r.partition_status("bob", 2), ConflictStatus::ClosedLocal { .. }));
    }

    // ── PHA: pha_re_evaluate ──────────────────────────────────────────────

    #[test]
    fn test_pha_re_evaluate_dominant_becomes_global() {
        let dag = build_dag_with_cp("cp", vec![
            ("tx1", "alice", 1, 10),
            ("tx2", "alice", 1, 3),
        ]);
        let cp_anchor = CheckpointAnchor::from_dag(
            "cp".into(), 0, CHECKPOINT_MIN_WEIGHT, &dag
        );

        let mut r = ConflictResolver::new();
        r.register_transaction(dag.get_transaction("tx1").unwrap());
        r.register_transaction(dag.get_transaction("tx2").unwrap());

        // Force into Reconciling
        r.try_close_local("alice", 1, &dag, &HashMap::new(), 0.0, &cp_anchor);
        r.pha_downgrade_above(&cp_anchor);
        assert_eq!(r.partition_status("alice", 1), ConflictStatus::Reconciling);

        let (closed, pending) = r.pha_re_evaluate(&dag, &cp_anchor);
        assert_eq!(closed, 1);
        assert_eq!(pending, 0);
        assert!(matches!(
            r.partition_status("alice", 1),
            ConflictStatus::ClosedGlobal { winner } if winner == "tx1"
        ));
    }

    #[test]
    fn test_pha_re_evaluate_not_dominant_falls_to_ready() {
        let dag = build_dag_with_cp("cp", vec![
            ("tx1", "alice", 1, 4),
            ("tx2", "alice", 1, 3),
        ]);
        let cp_anchor = CheckpointAnchor::from_dag(
            "cp".into(), 0, CHECKPOINT_MIN_WEIGHT, &dag
        );

        let mut r = ConflictResolver::new();
        r.register_transaction(dag.get_transaction("tx1").unwrap());
        r.register_transaction(dag.get_transaction("tx2").unwrap());

        let ps = r.partition_states
            .entry(("alice".into(), 1))
            .or_default();
        ps.status = ConflictStatus::ClosedLocal { winner: "tx1".into() };
        ps.downgrade_to_reconciling();
        r.conflict_sets.entry(("alice".into(), 1)).or_default()
            .extend(["tx1".into(), "tx2".into()]);

        let (closed, pending) = r.pha_re_evaluate(&dag, &cp_anchor);
        assert_eq!(closed, 0);
        assert_eq!(pending, 1);
        assert_eq!(r.partition_status("alice", 1), ConflictStatus::Ready);
    }

    #[test]
    fn test_pha_uses_frozen_stake_not_current() {
        let dag = build_dag_with_cp("cp", vec![
            ("tx1", "alice", 1, 8),
            ("tx2", "alice", 1, 3),
        ]);
        let cp_anchor = CheckpointAnchor::from_dag(
            "cp".into(), 0, CHECKPOINT_MIN_WEIGHT, &dag
        );

        let mut r = ConflictResolver::new();
        r.register_transaction(dag.get_transaction("tx1").unwrap());
        r.register_transaction(dag.get_transaction("tx2").unwrap());

        r.try_close_local("alice", 1, &dag, &HashMap::new(), 0.0, &cp_anchor);
        r.pha_downgrade_above(&cp_anchor);

        let (closed, _) = r.pha_re_evaluate(&dag, &cp_anchor);
        assert_eq!(closed, 1);
        assert!(matches!(
            r.partition_status("alice", 1),
            ConflictStatus::ClosedGlobal { winner } if winner == "tx1"
        ));
    }

    // ── Theorem P: same input → same global winner ────────────────────────

    #[test]
    fn test_theorem_p_two_nodes_same_global_winner() {
        let build = || {
            let dag = build_dag_with_cp("cp_star", vec![
                ("tx1", "alice", 1, 10),
                ("tx2", "alice", 1, 4),
            ]);
            let anchor = CheckpointAnchor::from_dag(
                "cp_star".into(), 0, CHECKPOINT_MIN_WEIGHT, &dag
            );
            let mut r = ConflictResolver::new();
            r.register_transaction(dag.get_transaction("tx1").unwrap());
            r.register_transaction(dag.get_transaction("tx2").unwrap());
            (dag, anchor, r)
        };

        let (dag_a, anchor_a, mut r_a) = build();
        let (dag_b, anchor_b, mut r_b) = build();

        r_a.try_close_local("alice", 1, &dag_a, &HashMap::new(), 0.0, &anchor_a);
        {
            let ps = r_b.partition_states.entry(("alice".into(), 1)).or_default();
            ps.set_closed_local("tx2".into(), "cp_star".into(), HashMap::new(), 0.0);
        }

        r_a.pha_downgrade_above(&anchor_a);
        r_b.pha_downgrade_above(&anchor_b);

        r_a.pha_re_evaluate(&dag_a, &anchor_a);
        r_b.pha_re_evaluate(&dag_b, &anchor_b);

        let winner_a = r_a.partition_status("alice", 1);
        let winner_b = r_b.partition_status("alice", 1);
        assert_eq!(winner_a, winner_b,
            "Theorem P violated: A={:?}, B={:?}", winner_a, winner_b);
        assert!(winner_a.is_globally_final());
    }

    #[test]
    fn test_invariant_g_below_cp_not_touched() {
        let mut dag = DAG::new();
        let mut old_cp = make_tx("old_cp", "system", 0);
        old_cp.weight = CHECKPOINT_MIN_WEIGHT;
        dag.add_transaction(old_cp).unwrap();

        for (id, w) in [("below_tx1", 8u64), ("below_tx2", 3u64)] {
            let mut tx = make_tx_w(id, "carol", 5, w);
            tx.parents = vec!["old_cp".into()];
            dag.add_transaction(tx).unwrap();
            dag.children_map.entry("old_cp".into()).or_default().insert(id.into());
        }

        let mut cp_star_tx = make_tx("cp_star", "system", 1);
        cp_star_tx.weight = CHECKPOINT_MIN_WEIGHT;
        dag.add_transaction(cp_star_tx).unwrap();

        let cp_star = CheckpointAnchor::from_dag(
            "cp_star".into(), 1, CHECKPOINT_MIN_WEIGHT, &dag
        );

        let mut r = ConflictResolver::new();
        r.register_transaction(dag.get_transaction("below_tx1").unwrap());
        r.register_transaction(dag.get_transaction("below_tx2").unwrap());

        let mut old_anchor = CheckpointAnchor::new("old_cp".into(), 0, CHECKPOINT_MIN_WEIGHT);
        old_anchor.register_descendant("below_tx1".into());
        old_anchor.register_descendant("below_tx2".into());
        r.try_close_local("carol", 5, &dag, &HashMap::new(), 0.0, &old_anchor);
        assert!(matches!(r.partition_status("carol", 5), ConflictStatus::ClosedLocal { .. }));

        let downgraded = r.pha_downgrade_above(&cp_star);
        assert!(downgraded.is_empty(), "Invariant G violated: below-cp* conflict was downgraded");
        assert!(matches!(r.partition_status("carol", 5), ConflictStatus::ClosedLocal { .. }),
            "Invariant G violated: carol/5 status changed");
    }

    // ── Legacy compatibility ───────────────────────────────────────────────

    #[test]
    fn test_register_and_get_conflicts() {
        let mut r = ConflictResolver::new();
        let tx1 = make_tx("tx1", "alice", 1);
        let tx2 = make_tx("tx2", "alice", 1);
        r.register_transaction(&tx1);
        r.register_transaction(&tx2);
        let conflicts = r.get_conflicts(&tx1);
        assert!(conflicts.contains(&"tx2".to_string()));
        assert!(!conflicts.contains(&"tx1".to_string()));
    }

    #[test]
    fn test_tiebreaker_min_tx_id() {
        let mut r = ConflictResolver::new();
        let tx_a = make_tx_w("aaa", "alice", 1, 1);
        let tx_b = make_tx_w("bbb", "alice", 1, 1);
        r.register_transaction(&tx_a);
        r.register_transaction(&tx_b);
        let mut dag = DAG::new();
        dag.add_transaction(tx_a.clone()).unwrap();
        dag.add_transaction(tx_b.clone()).unwrap();
        r.resolve_with_stake(&mut dag, &tx_b, &HashMap::new(), 0.0);
        assert!(!matches!(dag.get_transaction("aaa").unwrap().status, TxStatus::Conflict));
        assert!(matches!(dag.get_transaction("bbb").unwrap().status, TxStatus::Conflict));
    }

    #[test]
    fn test_is_closed_full_closure() {
        let dag = build_dag_with_cp("cp", vec![
            ("tx1", "alice", 1, 8),
            ("tx2", "alice", 1, 3),
        ]);
        let anchor = CheckpointAnchor::from_dag(
            "cp".into(), 0, CHECKPOINT_MIN_WEIGHT, &dag
        );
        let mut r = ConflictResolver::new();
        r.register_transaction(dag.get_transaction("tx1").unwrap());
        r.register_transaction(dag.get_transaction("tx2").unwrap());
        let result = r.is_closed("alice", 1, &dag, &HashMap::new(), 0.0, Some(&anchor));
        assert!(result.is_closed());
        assert_eq!(result.winner(), Some("tx1"));
    }
}