//! `Sim` — the top-level state and the `step` API (spec §6/§16). A pure state machine driven by
//! an external loop: it runs forward until it needs a decision (then yields one, fogged) or the
//! fight ends. The caller routes each decision to a `Controller` and feeds the command back via
//! [`Sim::submit`]. All ordering is strict and deterministic (see `PORTING.md`).

use crate::actor::{Actor, ActorState};
use crate::config::{Config, TempoModel};
use crate::controller::{Command, Decision, DecisionKind};
use crate::events::{Event, Outcome};
use crate::foresight::{self, ForesightView};
use crate::ids::{ActorId, FactionId, InstanceId, MoveId};
use crate::moves::{FrameData, MoveLibrary};
use crate::resolve;
use crate::rng::Rng;
use crate::tick::Tick;
use crate::timeline::{InstanceStatus, Timeline};
use crate::verbs::{EditVerb, VerbError};
use crate::windows::WindowStore;
use std::collections::{BTreeMap, BTreeSet};

/// What one `run_until_decision_or_end` returns.
pub enum StepResult {
    /// The engine needs a command for `decision`; `view` is what that observer may see.
    Decision {
        decision: Decision,
        view: ForesightView,
    },
    /// The fight is over.
    Ended(Outcome),
}

/// The whole combat state.
pub struct Sim {
    pub(crate) config: Config,
    pub(crate) lib: MoveLibrary,
    pub(crate) actors: BTreeMap<ActorId, Actor>,
    pub(crate) kits: BTreeMap<ActorId, Vec<MoveId>>,
    pub(crate) timeline: Timeline,
    pub(crate) windows: WindowStore,
    // Seeded variance stream — part of `Sim` so any randomized effect draws from one fixed-order
    // source (spec §4). v1 has no randomized effects, so it is not yet drawn from.
    #[allow(dead_code)]
    pub(crate) rng: Rng,
    pub(crate) current_tick: Tick,
    pub(crate) events: Vec<Event>,
    pub(crate) last_emit_tick: Option<Tick>,
    pub(crate) decided_this_tick: BTreeSet<ActorId>,
    pub(crate) pending: Option<Decision>,
    pub(crate) tick_resolved: bool,
    pub(crate) ended: bool,
    pub(crate) outcome: Option<Outcome>,
    /// Determinism harness: force a tick-by-tick advance instead of jumping (spec §17.3).
    pub(crate) force_tick_by_tick: bool,
}

impl Sim {
    /// A fresh sim with the given config, move library, and RNG seed. Add actors with
    /// [`Sim::add_actor`] before running.
    pub fn new(config: Config, lib: MoveLibrary, seed: u64) -> Self {
        Self {
            config,
            lib,
            actors: BTreeMap::new(),
            kits: BTreeMap::new(),
            timeline: Timeline::new(),
            windows: WindowStore::new(),
            rng: Rng::new(seed),
            current_tick: Tick::ZERO,
            events: Vec::new(),
            last_emit_tick: None,
            decided_this_tick: BTreeSet::new(),
            pending: None,
            tick_resolved: false,
            ended: false,
            outcome: None,
            force_tick_by_tick: false,
        }
    }

    /// Register a combatant and the moves it may commit.
    pub fn add_actor(&mut self, actor: Actor, kit: Vec<MoveId>) {
        self.kits.insert(actor.id, kit);
        self.actors.insert(actor.id, actor);
    }

    /// Force a forced tick-by-tick advance (the determinism test compares this against the
    /// default event-driven tick-jump — the two must produce identical traces).
    pub fn set_force_tick_by_tick(&mut self, on: bool) {
        self.force_tick_by_tick = on;
    }

    // ── Read-only accessors (used by foresight + the bridge/CLI) ────────────────────────────

    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn current_tick(&self) -> Tick {
        self.current_tick
    }
    pub fn actor(&self, id: ActorId) -> Option<&Actor> {
        self.actors.get(&id)
    }
    pub fn actors(&self) -> impl Iterator<Item = &Actor> {
        self.actors.values()
    }
    pub(crate) fn timeline(&self) -> &Timeline {
        &self.timeline
    }
    pub(crate) fn windows(&self) -> &WindowStore {
        &self.windows
    }
    pub(crate) fn kit(&self, id: ActorId) -> &[MoveId] {
        self.kits.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }
    pub(crate) fn effective_horizon(&self, a: &Actor) -> u32 {
        if a.foresight_horizon == 0 {
            self.config.default_foresight_horizon
        } else {
            a.foresight_horizon
        }
    }
    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }
    pub fn is_ended(&self) -> bool {
        self.ended
    }
    pub fn library(&self) -> &MoveLibrary {
        &self.lib
    }

    // ── Event emission ──────────────────────────────────────────────────────────────────────

    /// Emit one event, lazily prepending a `TickAdvanced` the first time a tick produces output.
    /// Boring ticks emit nothing, which is what keeps tick-jump and tick-by-tick traces equal.
    pub(crate) fn emit(&mut self, ev: Event) {
        if self.last_emit_tick != Some(self.current_tick) {
            self.events.push(Event::TickAdvanced {
                to: self.current_tick,
            });
            self.last_emit_tick = Some(self.current_tick);
        }
        self.events.push(ev);
    }

    /// Take the accumulated events, leaving the buffer empty.
    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    // ── Tempo ───────────────────────────────────────────────────────────────────────────────

    /// Change `actor`'s Tempo by `delta`, clamped at 0 (never negative), emitting `TempoChanged`
    /// when it actually moves.
    pub(crate) fn change_tempo(&mut self, actor: ActorId, delta: i32) {
        let Some(a) = self.actors.get_mut(&actor) else {
            return;
        };
        let new = (a.tempo + delta).max(0);
        let real_delta = new - a.tempo;
        if real_delta == 0 {
            return;
        }
        a.tempo = new;
        self.emit(Event::TempoChanged {
            actor,
            delta: real_delta,
            new_total: new,
        });
    }

    /// Spend `amount` Tempo if affordable; returns whether the spend happened.
    pub(crate) fn spend_tempo(&mut self, actor: ActorId, amount: i32) -> bool {
        if amount <= 0 {
            return true;
        }
        let have = self.actors.get(&actor).map(|a| a.tempo).unwrap_or(0);
        if have < amount {
            return false;
        }
        self.change_tempo(actor, -amount);
        true
    }

    fn can_dilate_actor(&self, a: &Actor) -> bool {
        match self.config.tempo_model {
            TempoModel::PerActorOpposed => a.tempo > 0,
            TempoModel::SharedPlayerPool => a.tempo > 0 && a.faction == FactionId::PLAYER,
        }
    }

    // ── Scheduling & actions ────────────────────────────────────────────────────────────────

    /// Schedule a committed action starting now, set the actor `Committed`, and emit the
    /// schedule + startup-entry events. (Tempo accounting is the caller's job.)
    pub(crate) fn schedule_committed(
        &mut self,
        actor: ActorId,
        mv: MoveId,
        target: Option<ActorId>,
        frames: FrameData,
    ) -> InstanceId {
        let start = self.current_tick;
        let id = self.timeline.schedule(actor, mv, target, start, frames);
        if let Some(inst) = self.timeline.get_mut(id) {
            inst.started = true;
        }
        if let Some(a) = self.actors.get_mut(&actor) {
            a.state = ActorState::Committed(id);
        }
        self.emit(Event::ActionScheduled {
            instance: id,
            actor,
            mv,
            target,
            start,
        });
        self.emit(Event::ActionStarted {
            instance: id,
            tick: start,
        });
        id
    }

    /// A readiness commit: deduct the move's (usually zero) `tempo_cost`, then schedule. If the
    /// actor can't afford it, falls back to a `Hold`.
    pub(crate) fn commit_action(&mut self, actor: ActorId, mv: MoveId, target: Option<ActorId>) {
        let Some(def) = self.lib.get(mv) else {
            self.hold(actor);
            return;
        };
        let (frames, cost) = (def.frames, def.tempo_cost);
        if !self.spend_tempo(actor, cost) {
            self.hold(actor);
            return;
        }
        self.schedule_committed(actor, mv, target, frames);
    }

    /// Park a ready actor for `hold_quantum` ticks so it cannot stall the sim forever.
    pub(crate) fn hold(&mut self, actor: ActorId) {
        let q = self.config.hold_quantum as u64;
        if let Some(a) = self.actors.get_mut(&actor) {
            a.next_ready_tick = self.current_tick + q;
        }
    }

    /// Re-anchor an instance to a new start tick and emit `Rescheduled`.
    pub(crate) fn reschedule(&mut self, instance: InstanceId, new_start: Tick) {
        if let Some(inst) = self.timeline.get_mut(instance) {
            inst.start_tick = new_start;
        }
        self.emit(Event::Rescheduled {
            instance,
            start: new_start,
        });
    }

    /// Defeat an actor: set `Down`, cancel its in-flight action, drop its windows, emit
    /// `ActorDowned`.
    pub(crate) fn down_actor(&mut self, actor: ActorId, tick: Tick) {
        if let Some(live) = self.timeline.live_of(actor).map(|i| i.id)
            && let Some(inst) = self.timeline.get_mut(live)
        {
            inst.status = InstanceStatus::Cancelled;
        }
        self.windows.drop_actor(actor);
        if let Some(a) = self.actors.get_mut(&actor) {
            a.state = ActorState::Down;
        }
        self.emit(Event::ActorDowned { actor, tick });
    }

    // ── Edit verbs (spec §8) ────────────────────────────────────────────────────────────────

    /// Apply an edit verb on behalf of `actor`. A rejected verb costs nothing and mutates nothing.
    pub(crate) fn apply_verb(&mut self, actor: ActorId, verb: EditVerb) -> Result<(), VerbError> {
        match verb {
            EditVerb::Slow { instance, ticks } => self.verb_shift(actor, instance, ticks as i64),
            EditVerb::Haste { instance, ticks } => {
                self.verb_shift(actor, instance, -(ticks as i64))
            }
            EditVerb::Interrupt { instance } => self.verb_interrupt(actor, instance),
            EditVerb::Insert {
                actor: who,
                mv,
                target,
            } => self.verb_insert(actor, who, mv, target),
        }
    }

    /// May `actor` edit `instance` under the lock policy?
    fn edit_allowed(&self, actor: ActorId, instance: InstanceId) -> Result<(), VerbError> {
        use crate::config::EditLockPolicy::*;
        use crate::timeline::Phase;
        let inst = self
            .timeline
            .get(instance)
            .ok_or(VerbError::NoSuchInstance)?;
        if !inst.live() {
            return Err(VerbError::InstanceNotEditable);
        }
        let owner_faction = self
            .actors
            .get(&inst.actor)
            .map(|a| a.faction)
            .ok_or(VerbError::NoSuchInstance)?;
        let actor_faction = self
            .actors
            .get(&actor)
            .map(|a| a.faction)
            .ok_or(VerbError::ActorIneligible)?;
        let same_side = owner_faction == actor_faction;
        match self.config.edit_lock_policy {
            LockedOnCommit if same_side => Err(VerbError::EditLocked),
            EditableUntilActive if same_side && inst.phase(self.current_tick) != Phase::Startup => {
                Err(VerbError::EditLocked)
            }
            _ => Ok(()),
        }
    }

    fn verb_shift(
        &mut self,
        actor: ActorId,
        instance: InstanceId,
        delta: i64,
    ) -> Result<(), VerbError> {
        self.edit_allowed(actor, instance)?;
        let cost = self.config.verb_base_cost
            + self.config.verb_per_tick_cost * delta.unsigned_abs() as i32;
        let inst = self
            .timeline
            .get(instance)
            .ok_or(VerbError::NoSuchInstance)?;
        let cur = inst.start_tick.0 as i64;
        // Haste clamps so the action still activates strictly after the current tick.
        let floor = self.current_tick.0 as i64;
        let new_start = (cur + delta).max(floor) as u64;
        if self.actors.get(&actor).map(|a| a.tempo).unwrap_or(0) < cost {
            return Err(VerbError::InsufficientTempo);
        }
        self.change_tempo(actor, -cost);
        self.reschedule(instance, Tick(new_start));
        Ok(())
    }

    fn verb_interrupt(&mut self, actor: ActorId, instance: InstanceId) -> Result<(), VerbError> {
        use crate::timeline::Phase;
        self.edit_allowed(actor, instance)?;
        let inst = *self
            .timeline
            .get(instance)
            .ok_or(VerbError::NoSuchInstance)?;
        let armored = self.lib.get(inst.mv).map(|d| d.has_armor).unwrap_or(true);
        if inst.phase(self.current_tick) != Phase::Startup || armored {
            return Err(VerbError::NotInterruptible);
        }
        let cost = self.config.verb_base_cost + self.config.verb_per_tick_cost;
        if self.actors.get(&actor).map(|a| a.tempo).unwrap_or(0) < cost {
            return Err(VerbError::InsufficientTempo);
        }
        self.change_tempo(actor, -cost);
        self.cancel_instance(instance, actor);
        Ok(())
    }

    fn verb_insert(
        &mut self,
        actor: ActorId,
        who: ActorId,
        mv: MoveId,
        target: Option<ActorId>,
    ) -> Result<(), VerbError> {
        if who != actor {
            return Err(VerbError::ActorIneligible);
        }
        // Inserting is for an actor not currently mid-action.
        let state = self
            .actors
            .get(&actor)
            .map(|a| a.state)
            .ok_or(VerbError::ActorIneligible)?;
        if !matches!(state, ActorState::Idle) {
            return Err(VerbError::ActorIneligible);
        }
        let def = self.lib.get(mv).ok_or(VerbError::NoSuchMove)?;
        let frames = def.frames;
        let cost = self.config.verb_base_cost + self.config.verb_per_tick_cost;
        if self.actors.get(&actor).map(|a| a.tempo).unwrap_or(0) < cost {
            return Err(VerbError::InsufficientTempo);
        }
        self.change_tempo(actor, -cost);
        self.schedule_committed(actor, mv, target, frames);
        Ok(())
    }

    /// Cancel an instance and free its owner (the shared interrupt path).
    pub(crate) fn cancel_instance(&mut self, instance: InstanceId, by: ActorId) {
        let Some(inst) = self.timeline.get_mut(instance) else {
            return;
        };
        inst.status = InstanceStatus::Cancelled;
        let owner = inst.actor;
        let tick = self.current_tick;
        if let Some(a) = self.actors.get_mut(&owner)
            && a.state == ActorState::Committed(instance)
        {
            a.state = ActorState::Idle;
            a.next_ready_tick = tick;
        }
        self.emit(Event::Interrupted {
            interrupted: instance,
            by,
        });
    }

    // ── The tick loop (spec §6) ─────────────────────────────────────────────────────────────

    /// Run forward until a decision is required or the fight ends.
    pub fn run_until_decision_or_end(&mut self) -> StepResult {
        loop {
            if self.ended {
                return StepResult::Ended(self.outcome.unwrap_or(Outcome::Stalemate));
            }
            if !self.tick_resolved {
                resolve::resolve_tick(self, self.current_tick);
                self.tick_resolved = true;
                if self.check_and_maybe_end() {
                    return StepResult::Ended(self.outcome.unwrap());
                }
            }
            if let Some(decision) = self.next_actionable() {
                self.pending = Some(decision);
                // NB: the pending decision is surfaced to the caller via `StepResult`, not the
                // event stream — emitting it would flood the trace with every Pass/Hold and break
                // the "boring ticks emit nothing" property. `Event::DecisionRequired` remains in
                // the contract for a downstream layer that wants to synthesize it.
                let view = foresight::project(self, decision.actor);
                return StepResult::Decision { decision, view };
            }
            if self.check_and_maybe_end() {
                return StepResult::Ended(self.outcome.unwrap());
            }
            match self.next_interesting_tick() {
                Some(t) => {
                    self.current_tick = t;
                    self.tick_resolved = false;
                    self.decided_this_tick.clear();
                }
                None => {
                    self.finish(Outcome::Stalemate);
                    return StepResult::Ended(Outcome::Stalemate);
                }
            }
            if self.current_tick.0 > self.config.max_ticks {
                self.finish(Outcome::Stalemate);
                return StepResult::Ended(Outcome::Stalemate);
            }
        }
    }

    /// Apply the controller's command for the pending decision, then mark that actor decided for
    /// this tick. Panics if no decision is pending (a harness contract violation).
    pub fn submit(&mut self, cmd: Command) {
        let d = self
            .pending
            .take()
            .expect("submit with no pending decision");
        self.decided_this_tick.insert(d.actor);
        match d.kind {
            DecisionKind::Readiness => match cmd {
                Command::CommitAction { mv, target } => self.commit_action(d.actor, mv, target),
                // Hold, or anything illegal for a readiness decision → Hold (deterministic).
                _ => self.hold(d.actor),
            },
            DecisionKind::Dilation => {
                if let Command::EditVerb(v) = cmd {
                    // A rejected verb is a no-op; the actor is still decided for this tick.
                    let _ = self.apply_verb(d.actor, v);
                }
                // Pass / Dilate / anything else: decline to edit.
            }
        }
    }

    // ── Decision selection & advance ────────────────────────────────────────────────────────

    fn next_actionable(&self) -> Option<Decision> {
        let mut best: Option<(u32, u32, DecisionKind)> = None;
        for a in self.actors.values() {
            if self.decided_this_tick.contains(&a.id) || !a.alive() {
                continue;
            }
            let kind = if a.ready_at(self.current_tick) {
                DecisionKind::Readiness
            } else if self.dilation_eligible(a) {
                DecisionKind::Dilation
            } else {
                continue;
            };
            let key = (a.faction.0, a.id.0);
            if best.is_none_or(|(f, id, _)| key < (f, id)) {
                best = Some((a.faction.0, a.id.0, kind));
            }
        }
        best.map(|(f, id, kind)| Decision {
            actor: ActorId(id),
            faction: FactionId(f),
            tick: self.current_tick,
            kind,
        })
    }

    /// A Tempo-holder may dilate when it is busy or idle-but-not-ready and there is at least one
    /// opposing action live on the line to react to.
    fn dilation_eligible(&self, a: &Actor) -> bool {
        if !self.can_dilate_actor(a) {
            return false;
        }
        let busy_or_waiting = match a.state {
            ActorState::Committed(_) => true,
            ActorState::Idle => a.next_ready_tick > self.current_tick,
            _ => false,
        };
        if !busy_or_waiting {
            return false;
        }
        self.timeline.iter().any(|i| {
            i.live()
                && self
                    .actors
                    .get(&i.actor)
                    .is_some_and(|o| o.faction != a.faction)
        })
    }

    /// The minimum future tick where anything interesting happens (or `current + 1` in the
    /// forced-tick-by-tick determinism mode).
    fn next_interesting_tick(&self) -> Option<Tick> {
        if self.force_tick_by_tick {
            return Some(self.current_tick + 1);
        }
        let now = self.current_tick;
        let mut best: Option<Tick> = None;
        let mut consider = |t: Tick| {
            if t > now {
                best = Some(best.map_or(t, |b| b.min(t)));
            }
        };
        for inst in self.timeline.iter() {
            if !inst.live() {
                continue;
            }
            consider(inst.start_tick);
            consider(inst.active_start());
            consider(inst.recovery_end());
        }
        for w in self.windows.iter() {
            consider(w.end);
        }
        for a in self.actors.values() {
            match a.state {
                ActorState::Idle if a.alive() => consider(a.next_ready_tick),
                ActorState::Staggered { until } => consider(until),
                _ => {}
            }
        }
        best
    }

    /// Are we down to one (or zero) standing factions? If so, end and emit `CombatEnded`.
    fn check_and_maybe_end(&mut self) -> bool {
        if self.ended {
            return true;
        }
        let mut factions: BTreeSet<u32> = BTreeSet::new();
        for a in self.actors.values() {
            if a.alive() {
                factions.insert(a.faction.0);
            }
        }
        if factions.len() <= 1 {
            let outcome = match factions.iter().next() {
                Some(&f) => Outcome::Victory {
                    faction: FactionId(f),
                },
                None => Outcome::MutualDefeat,
            };
            self.finish(outcome);
            true
        } else {
            false
        }
    }

    fn finish(&mut self, outcome: Outcome) {
        if self.ended {
            return;
        }
        self.ended = true;
        self.outcome = Some(outcome);
        self.emit(Event::CombatEnded { outcome });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::Vitals;
    use crate::config::EditLockPolicy;
    use crate::moves::MoveDef;

    /// A two-actor sim with one move (id 1), both actors with plenty of Tempo, idle and ready.
    fn rig(policy: EditLockPolicy) -> Sim {
        let cfg = Config {
            edit_lock_policy: policy,
            ..Config::default()
        };
        let lib = MoveLibrary::from_defs([MoveDef::builder(MoveId(1), "Strike")
            .frames(4, 1, 2)
            .damage(3)
            .build()]);
        let mut sim = Sim::new(cfg, lib, 7);
        for (id, faction) in [(1u32, 0u32), (2, 1)] {
            sim.add_actor(
                Actor {
                    id: ActorId(id),
                    faction: FactionId(faction),
                    vitals: Vitals::new(20),
                    tempo: 50,
                    next_ready_tick: Tick(0),
                    state: ActorState::Idle,
                    foresight_horizon: 0,
                    zone: 0,
                },
                vec![MoveId(1)],
            );
        }
        sim
    }

    /// Commit a Strike for `actor` and return its instance id.
    fn commit(sim: &mut Sim, actor: u32, target: u32) -> InstanceId {
        sim.commit_action(ActorId(actor), MoveId(1), Some(ActorId(target)));
        sim.timeline.live_of(ActorId(actor)).unwrap().id
    }

    #[test]
    fn locked_on_commit_blocks_the_owner_but_not_the_opponent() {
        let mut sim = rig(EditLockPolicy::LockedOnCommit);
        let inst = commit(&mut sim, 1, 2);
        let start = sim.timeline.get(inst).unwrap().start_tick;

        // The owner (faction 0) cannot Slow its own committed action.
        let owner_try = sim.apply_verb(
            ActorId(1),
            EditVerb::Slow {
                instance: inst,
                ticks: 3,
            },
        );
        assert_eq!(owner_try, Err(VerbError::EditLocked));
        assert_eq!(sim.timeline.get(inst).unwrap().start_tick, start);

        // The opponent (faction 1) can.
        let foe_try = sim.apply_verb(
            ActorId(2),
            EditVerb::Slow {
                instance: inst,
                ticks: 3,
            },
        );
        assert_eq!(foe_try, Ok(()));
        assert_eq!(sim.timeline.get(inst).unwrap().start_tick, start + 3);
    }

    #[test]
    fn editable_until_active_lets_the_owner_edit_in_startup() {
        let mut sim = rig(EditLockPolicy::EditableUntilActive);
        let inst = commit(&mut sim, 1, 2);
        let start = sim.timeline.get(inst).unwrap().start_tick;
        // Still in startup (current_tick 0 < active_start 4) → owner may slow it.
        let r = sim.apply_verb(
            ActorId(1),
            EditVerb::Slow {
                instance: inst,
                ticks: 2,
            },
        );
        assert_eq!(r, Ok(()));
        assert_eq!(sim.timeline.get(inst).unwrap().start_tick, start + 2);
    }

    #[test]
    fn tempo_never_goes_negative() {
        let mut sim = rig(EditLockPolicy::LockedOnCommit);
        // Drain actor 2's Tempo to a known small amount, then overspend.
        let id = ActorId(2);
        sim.actors.get_mut(&id).unwrap().tempo = 1;
        assert!(!sim.spend_tempo(id, 5), "overspend must be refused");
        assert_eq!(
            sim.actor(id).unwrap().tempo,
            1,
            "refused spend leaves Tempo intact"
        );
        assert!(sim.spend_tempo(id, 1));
        assert_eq!(sim.actor(id).unwrap().tempo, 0);
    }

    #[test]
    fn an_unaffordable_verb_is_a_no_op() {
        let mut sim = rig(EditLockPolicy::LockedOnCommit);
        let inst = commit(&mut sim, 1, 2);
        // Opponent with no Tempo cannot Slow.
        sim.actors.get_mut(&ActorId(2)).unwrap().tempo = 0;
        let start = sim.timeline.get(inst).unwrap().start_tick;
        let r = sim.apply_verb(
            ActorId(2),
            EditVerb::Slow {
                instance: inst,
                ticks: 3,
            },
        );
        assert_eq!(r, Err(VerbError::InsufficientTempo));
        assert_eq!(sim.timeline.get(inst).unwrap().start_tick, start);
    }
}
