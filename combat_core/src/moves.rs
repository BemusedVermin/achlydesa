//! Moves: the authored, static definition of what an action does (spec §5). v1 moves are
//! hand-built `MoveDef`s; a future RPG stat layer compiles into the same shape through the
//! [`MoveDef::builder`] seam (spec §18), so the bridge has a single construction surface.

use crate::actor::ZoneId;
use crate::ids::MoveId;
use crate::tick::Fixed;
use crate::windows::WindowTag;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The three classic phases of an action, in ticks. `startup` must be ≥ 1 (an action enters
/// startup the tick it is committed, so a zero-startup move could never have its contact resolved
/// — the builder clamps it).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct FrameData {
    pub startup: u32,
    pub active: u32,
    pub recovery: u32,
}

impl FrameData {
    pub fn new(startup: u32, active: u32, recovery: u32) -> Self {
        Self {
            startup: startup.max(1),
            active: active.max(1),
            recovery,
        }
    }

    /// Total occupancy of the action on the line.
    pub fn total(&self) -> u32 {
        self.startup + self.active + self.recovery
    }
}

/// What an active frame does to its target, applied in listed order (spec §7).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Effect {
    Damage {
        amount: i32,
    },
    /// Shoves the target along the timeline (the reversal mechanic, spec §7).
    LineKnockback {
        ticks: u32,
    },
    OpenWindow {
        tag: WindowTag,
        duration: u32,
        magnitude: Fixed,
    },
    Stagger {
        ticks: u32,
    },
    /// Move the *acting* actor to another zone — the minimal positioning verb (spec §11). Unlike
    /// the other effects this targets self, not the move's target.
    Reposition {
        zone: ZoneId,
    },
}

/// Range requirement, against the minimal zone model (spec §11).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ZoneReq {
    /// Target must share the attacker's zone.
    SameZone,
    /// Any zone is fine.
    #[default]
    AnyZone,
}

/// One authored move.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct MoveDef {
    pub id: MoveId,
    pub name: String,
    pub frames: FrameData,
    /// Contact ordering — higher wins when two actions activate on the same tick.
    pub priority_class: u8,
    pub effects: Vec<Effect>,
    /// If set, the target must currently carry this window tag or the move fizzles.
    pub requires_tag: Option<WindowTag>,
    /// If true, the move cannot be interrupted during its startup.
    pub has_armor: bool,
    pub range: ZoneReq,
    /// Tempo deducted when committed (normally 0 for a reactive readiness action).
    pub tempo_cost: i32,
}

impl MoveDef {
    /// Begin building a move. Chains set the optional fields; [`MoveBuilder::build`] finishes.
    pub fn builder(id: MoveId, name: impl Into<String>) -> MoveBuilder {
        MoveBuilder {
            def: MoveDef {
                id,
                name: name.into(),
                frames: FrameData::new(1, 1, 0),
                priority_class: 0,
                effects: Vec::new(),
                requires_tag: None,
                has_armor: false,
                range: ZoneReq::AnyZone,
                tempo_cost: 0,
            },
        }
    }
}

/// Fluent builder for [`MoveDef`] — the single construction seam the RPG bridge will target.
pub struct MoveBuilder {
    def: MoveDef,
}

impl MoveBuilder {
    pub fn frames(mut self, startup: u32, active: u32, recovery: u32) -> Self {
        self.def.frames = FrameData::new(startup, active, recovery);
        self
    }
    pub fn priority(mut self, class: u8) -> Self {
        self.def.priority_class = class;
        self
    }
    pub fn effect(mut self, e: Effect) -> Self {
        self.def.effects.push(e);
        self
    }
    pub fn damage(self, amount: i32) -> Self {
        self.effect(Effect::Damage { amount })
    }
    pub fn reposition(self, zone: crate::actor::ZoneId) -> Self {
        self.effect(Effect::Reposition { zone })
    }
    pub fn requires(mut self, tag: WindowTag) -> Self {
        self.def.requires_tag = Some(tag);
        self
    }
    pub fn armored(mut self) -> Self {
        self.def.has_armor = true;
        self
    }
    pub fn range(mut self, range: ZoneReq) -> Self {
        self.def.range = range;
        self
    }
    pub fn tempo_cost(mut self, cost: i32) -> Self {
        self.def.tempo_cost = cost;
        self
    }
    pub fn build(self) -> MoveDef {
        self.def
    }
}

/// `MoveId → MoveDef`, a `BTreeMap` so iteration (when needed) is deterministic.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MoveLibrary {
    moves: BTreeMap<MoveId, MoveDef>,
}

impl MoveLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, def: MoveDef) {
        self.moves.insert(def.id, def);
    }

    pub fn get(&self, id: MoveId) -> Option<&MoveDef> {
        self.moves.get(&id)
    }

    pub fn from_defs(defs: impl IntoIterator<Item = MoveDef>) -> Self {
        let mut lib = Self::new();
        for d in defs {
            lib.insert(d);
        }
        lib
    }

    pub fn len(&self) -> usize {
        self.moves.len()
    }

    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }
}
