//! Edit verbs — the player's (and elite enemies') power to bend the next few seconds (spec §8).
//!
//! Each verb is a pure timeline transform that costs Tempo scaled by its magnitude, respects the
//! `edit_lock_policy`, and emits events. The application logic lives on `Sim` (it needs the whole
//! state); this module owns the verb vocabulary and its cost/magnitude rules.

use crate::ids::{ActorId, InstanceId, MoveId};
use serde::{Deserialize, Serialize};

/// A timeline edit a dilating actor can spend Tempo on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum EditVerb {
    /// Push an instance's `start_tick` later by `ticks`.
    Slow { instance: InstanceId, ticks: u32 },
    /// Pull an instance's `start_tick` earlier by `ticks` (clamped so it stays in the future).
    Haste { instance: InstanceId, ticks: u32 },
    /// Cancel an instance if it is in startup and unarmored.
    Interrupt { instance: InstanceId },
    /// Schedule a fresh action for a dilating actor, outside its normal readiness.
    Insert {
        actor: ActorId,
        mv: MoveId,
        target: Option<ActorId>,
    },
}

impl EditVerb {
    /// The magnitude that scales the Tempo cost (`verb_per_tick_cost * magnitude`).
    pub fn magnitude(&self) -> u32 {
        match self {
            EditVerb::Slow { ticks, .. } | EditVerb::Haste { ticks, .. } => *ticks,
            EditVerb::Interrupt { .. } | EditVerb::Insert { .. } => 1,
        }
    }
}

/// Why a verb could not be applied. A rejected verb costs nothing and mutates nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum VerbError {
    /// The referenced instance does not exist.
    NoSuchInstance,
    /// The referenced move is not in the library.
    NoSuchMove,
    /// Interrupt was asked of an instance not in interruptible startup.
    NotInterruptible,
    /// The acting actor may not edit this instance under the current lock policy.
    EditLocked,
    /// The acting actor cannot afford the verb's Tempo cost.
    InsufficientTempo,
    /// The acting actor is not in a state that permits this verb (down, staggered, …).
    ActorIneligible,
    /// The instance is already resolved or cancelled.
    InstanceNotEditable,
}
