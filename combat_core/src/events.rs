//! The typed, ordered event stream — *the contract* (spec §12).
//!
//! This `Vec<Event>` is simultaneously (a) what a downstream presentation layer consumes to
//! drive animation/camera/VFX, and (b) the golden-vector trace the tests diff against. Emission
//! order within a tick follows the resolution order in `resolve` (spec §4/§7), so traces are
//! stable across runs and machines.

use crate::ids::{ActorId, FactionId, InstanceId, MoveId, WindowId};
use crate::tick::Tick;
use crate::windows::WindowTag;
use serde::{Deserialize, Serialize};

/// Why a contact produced no effects.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FizzleReason {
    /// The target was gone, down, or out of zone/range.
    NoValidTarget,
    /// The move required a window tag the target wasn't carrying.
    MissingTag,
}

/// How the fight ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Outcome {
    /// Exactly one faction still has a standing actor.
    Victory { faction: FactionId },
    /// No faction has a standing actor (everyone went down on the same beat).
    MutualDefeat,
    /// The sim ran out of interesting ticks with both sides alive (a guard; should be rare).
    Stalemate,
}

/// One thing that happened, carrying the `tick` it happened at and the relevant ids/magnitudes.
///
/// `TickAdvanced` is emitted lazily — exactly once, just before the first "real" event of any
/// tick that produces events. Boring ticks produce nothing, which is what makes the
/// event-driven tick-jump and a forced tick-by-tick advance yield identical traces (spec §17.3).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Event {
    TickAdvanced {
        to: Tick,
    },
    DecisionRequired {
        actor: ActorId,
        faction: FactionId,
        tick: Tick,
    },
    ActionScheduled {
        instance: InstanceId,
        actor: ActorId,
        mv: MoveId,
        target: Option<ActorId>,
        start: Tick,
    },
    /// The instance entered its startup phase.
    ActionStarted {
        instance: InstanceId,
        tick: Tick,
    },
    /// The instance entered its active phase (the contact beat).
    ActionActive {
        instance: InstanceId,
        tick: Tick,
    },
    Hit {
        instance: InstanceId,
        attacker: ActorId,
        target: ActorId,
        amount: i32,
    },
    /// A `LineKnockback` effect shoved the target's contribution later down the line.
    LineShoved {
        target: ActorId,
        instance: Option<InstanceId>,
        ticks: u32,
    },
    /// A `Slow`/`Haste` edit verb re-anchored an instance to a new start tick.
    Rescheduled {
        instance: InstanceId,
        start: Tick,
    },
    Interrupted {
        interrupted: InstanceId,
        by: ActorId,
    },
    ActionFizzled {
        instance: InstanceId,
        reason: FizzleReason,
    },
    WindowOpened {
        window: WindowId,
        actor: ActorId,
        tag: WindowTag,
        end: Tick,
    },
    WindowClosed {
        window: WindowId,
    },
    ActorStaggered {
        actor: ActorId,
        until: Tick,
    },
    TempoChanged {
        actor: ActorId,
        delta: i32,
        new_total: i32,
    },
    ActionCompleted {
        instance: InstanceId,
        tick: Tick,
    },
    ActorDowned {
        actor: ActorId,
        tick: Tick,
    },
    CombatEnded {
        outcome: Outcome,
    },
}
