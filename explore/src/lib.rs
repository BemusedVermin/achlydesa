//! The **exploration layer**'s data: the procedurally-laid road network, the gear a body carries,
//! and the cost/gate config. The heavy lifting — the cost model, weighted routing, road-laying,
//! edge gates — lives in the pure [`travel`] crate; the orchestration (building the network at
//! world-gen, routing the avatar with its party's capabilities, POI interaction) lives in the
//! `agents` assembler, which has the world, the party and the RPG layer in scope.

use bevy_ecs::prelude::*;
use std::collections::HashSet;

pub use travel::{Caps, CostModel};

/// The road network — topology indices carrying road. Built once at world-gen from the settlement
/// tiles; feeds the travel cost field (a road hex is the fast lane). Present only when the layer
/// is on, so a world without it routes over raw terrain exactly as before.
#[derive(Resource, Clone, Debug, Default)]
pub struct Roads(pub HashSet<usize>);

impl Roads {
    pub fn has(&self, i: usize) -> bool {
        self.0.contains(&i)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Gear a body carries — capability flags like `"climbing_gear"`, `"boat"`, `"warm_gear"`, `"cart"`.
/// Steep edges need climbing gear (and a proficient party share); deep water needs a boat.
#[derive(Component, Clone, Debug, Default)]
pub struct Gear(pub HashSet<String>);

impl Gear {
    pub fn has(&self, g: &str) -> bool {
        self.0.contains(g)
    }
    pub fn with(items: impl IntoIterator<Item = &'static str>) -> Self {
        Self(items.into_iter().map(String::from).collect())
    }
}

/// Knobs for the exploration layer: the travel [`CostModel`] and the party climbing gate.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ExploreConfig {
    pub cost: CostModel,
    /// Fraction of the party that must be climbing-proficient (carry the `"climbing_proficient"`
    /// flag) — on top of holding climbing gear — to scale a steep edge.
    pub climb_share: f32,
}

impl Default for ExploreConfig {
    fn default() -> Self {
        Self { cost: CostModel::default(), climb_share: 0.5 }
    }
}
