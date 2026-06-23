//! Actors and their live state (spec §5). An `Actor` is the per-combatant component; the data is
//! deliberately flat and `Copy` so it maps cleanly onto an ECS later.

use crate::ids::{ActorId, FactionId, InstanceId};
use crate::tick::Tick;
use serde::{Deserialize, Serialize};

/// The minimal spatial coordinate (spec §11/§18): a small set of abstract zones. The continuous
/// lane+offset model is a future module that slots in behind `ZoneId`/`ZoneReq`.
pub type ZoneId = u8;

/// Hit points.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Vitals {
    pub hp: i32,
    pub max_hp: i32,
}

impl Vitals {
    pub fn new(max_hp: i32) -> Self {
        Self { hp: max_hp, max_hp }
    }
}

/// What an actor is doing right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ActorState {
    /// Ready to act; a decision point.
    Idle,
    /// Currently executing a scheduled action.
    Committed(InstanceId),
    /// Cannot act or edit until `until`.
    Staggered { until: Tick },
    /// Defeated.
    Down,
}

/// One combatant.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Actor {
    pub id: ActorId,
    pub faction: FactionId,
    pub vitals: Vitals,
    /// Tempo pool; `0` for mooks (spec §9).
    pub tempo: i32,
    /// When this actor may next take an `Idle` decision.
    pub next_ready_tick: Tick,
    pub state: ActorState,
    /// How many ticks ahead this actor can see (a stat; falls back to `Config`).
    pub foresight_horizon: u32,
    pub zone: ZoneId,
}

impl Actor {
    /// Not defeated.
    #[inline]
    pub fn alive(&self) -> bool {
        !matches!(self.state, ActorState::Down)
    }

    /// A legal target: exists (caller), alive, and not already down.
    #[inline]
    pub fn targetable(&self) -> bool {
        self.alive()
    }

    /// Can this actor take a readiness decision at `tick`?
    #[inline]
    pub fn ready_at(&self, tick: Tick) -> bool {
        matches!(self.state, ActorState::Idle) && self.next_ready_tick <= tick
    }
}
