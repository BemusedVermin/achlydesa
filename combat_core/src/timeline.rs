//! The tick line: scheduled action instances and the boundaries derived from their `start_tick`
//! (spec §5). An instance snapshots its `FrameData` so its phase boundaries can be recomputed
//! whenever an edit verb or a knockback re-anchors `start_tick`, without re-reading the library.

use crate::ids::{ActorId, InstanceId, MoveId};
use crate::moves::FrameData;
use crate::tick::Tick;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Lifecycle of a scheduled action.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum InstanceStatus {
    /// Placed on the line, not yet active.
    Scheduled,
    /// In or past its active frame, not yet completed.
    Resolving,
    /// Finished its recovery cleanly.
    Resolved,
    /// Killed (interrupted, or its owner went down).
    Cancelled,
}

/// Which phase an instance is in at a given tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Before `start_tick` (only reachable if an edit pushed start into the future).
    Pending,
    Startup,
    Active,
    Recovery,
    /// After recovery, or cancelled/resolved.
    Done,
}

/// One scheduled action on the timeline.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ActionInstance {
    pub id: InstanceId,
    pub actor: ActorId,
    pub mv: MoveId,
    pub target: Option<ActorId>,
    /// The tick the startup phase begins. All boundaries derive from this.
    pub start_tick: Tick,
    pub status: InstanceStatus,
    /// Snapshot of the move's frames, so boundaries survive a re-anchor of `start_tick`.
    pub frames: FrameData,
    /// `ActionStarted` already emitted — phase-entry events fire at most once.
    pub started: bool,
    /// `ActionActive` already emitted.
    pub active_emitted: bool,
}

impl ActionInstance {
    #[inline]
    pub fn startup_end(&self) -> Tick {
        self.start_tick + self.frames.startup as u64
    }
    #[inline]
    pub fn active_start(&self) -> Tick {
        self.startup_end()
    }
    #[inline]
    pub fn active_end(&self) -> Tick {
        self.active_start() + self.frames.active as u64
    }
    #[inline]
    pub fn recovery_end(&self) -> Tick {
        self.active_end() + self.frames.recovery as u64
    }

    /// The phase at `tick`, honouring the emission flags so an instance never appears to rewind
    /// past a phase it has already entered.
    pub fn phase(&self, tick: Tick) -> Phase {
        match self.status {
            InstanceStatus::Cancelled | InstanceStatus::Resolved => Phase::Done,
            _ => {
                if tick < self.active_start() {
                    if self.started {
                        Phase::Startup
                    } else {
                        Phase::Pending
                    }
                } else if tick < self.active_end() {
                    Phase::Active
                } else if tick < self.recovery_end() {
                    Phase::Recovery
                } else {
                    Phase::Done
                }
            }
        }
    }

    /// Live = neither resolved nor cancelled.
    #[inline]
    pub fn live(&self) -> bool {
        matches!(
            self.status,
            InstanceStatus::Scheduled | InstanceStatus::Resolving
        )
    }
}

/// All scheduled instances, keyed by id. A `BTreeMap` so any iteration is in id order; resolution
/// never depends on insertion timing (spec §4).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Timeline {
    instances: BTreeMap<InstanceId, ActionInstance>,
    next_id: u32,
}

impl Timeline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedule a fresh instance starting at `start_tick`; returns its id.
    pub fn schedule(
        &mut self,
        actor: ActorId,
        mv: MoveId,
        target: Option<ActorId>,
        start_tick: Tick,
        frames: FrameData,
    ) -> InstanceId {
        let id = InstanceId(self.next_id);
        self.next_id += 1;
        self.instances.insert(
            id,
            ActionInstance {
                id,
                actor,
                mv,
                target,
                start_tick,
                status: InstanceStatus::Scheduled,
                frames,
                started: false,
                active_emitted: false,
            },
        );
        id
    }

    pub fn get(&self, id: InstanceId) -> Option<&ActionInstance> {
        self.instances.get(&id)
    }

    pub fn get_mut(&mut self, id: InstanceId) -> Option<&mut ActionInstance> {
        self.instances.get_mut(&id)
    }

    /// Iterate all instances in id order.
    pub fn iter(&self) -> impl Iterator<Item = &ActionInstance> {
        self.instances.values()
    }

    /// The live instance owned by `actor`, if any (an actor has at most one in flight).
    pub fn live_of(&self, actor: ActorId) -> Option<&ActionInstance> {
        self.instances
            .values()
            .find(|i| i.actor == actor && i.live())
    }
}
