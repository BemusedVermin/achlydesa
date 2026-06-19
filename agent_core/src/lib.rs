//! The **agent core**: the foundational layer of the agent simulation — every shared
//! component, resource, and type, plus the existing subsystems (economy, factions,
//! dialogue, features, the avatar, fauna, the narrative director) and the fixed-order
//! per-step [`build_schedule`].
//!
//! Extracted from the original `agents` crate so the higher feature layers (the RPG,
//! survival, party, and exploration crates) can build *on top of* this core without a
//! dependency cycle. The thin `agents` crate sits above everything and assembles a
//! [`Simulation`](../agents/struct.Simulation.html) from these pieces.
//!
//! The substrate (a struct-of-arrays field solver) is held as an ECS **`Resource`**;
//! agents are **entities**; behaviour is **systems**. Two kinds of agent: instinct-driven
//! [`fauna`] and utility-driven [`people`] whose economy is authored [`data`] (RON).
//!
//! Determinism is preserved: the schedule runs single-threaded in a fixed order, and all
//! randomness comes from a seeded [`SimRng`].

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ExecutorKind;
use game_sim::{SplitMix64, World as GameWorld};
use sim::Substrate as SubstrateTrait;

pub mod ai;
pub mod beats;
pub mod data;
pub mod dialogue;
pub mod director;
pub mod events;
pub mod factions;
pub mod fauna;
pub mod features;
pub mod goals;
pub mod norms;
pub mod observe;
pub mod people;
pub mod plan;
pub mod player;

pub use ai::{Consideration, Curve, Input};
pub use data::{GoodDef, GoodId, MoodDef, MoodId, Recipe, Registry, ResourceKind, SkillId, TraitDef, TraitId};
pub use beats::{Beat, BeatBook, Effect, Phase, Pre, Register, Role};
pub use dialogue::{Dialogue, DialogueConfig, IntentBook, SlmRealizer, SpeechAct, TextGen, Utterance};
pub use player::{Player, PlayerKnowledge, PlayerState, PlayerView, Rumor, SearchOutcome, Terrain, TileInfo};
pub use game_sim::{Coord, Topology};
pub use director::{Cadence, Director, DirectorConfig, Protagonist, Thread};
pub use events::{AgentEvent, Appraisals, EventQueue};
pub use factions::{Allegiance, Bond, Detained, Faction, FactionConfig, Factions, Government, Law, Opinion};
pub use fauna::{Bestiary, Carnivore, Diet, Energy, FaunaConfig, FaunaRng, Form, Herbivore, Species, SpeciesId};
pub use features::{
    AffordanceDef, Category, Discovery, EffectDef, Feature, FeatureCatalog, FeatureConfig, FeatureDef, FeatureId,
    Features, FindState, NeedKind,
};
pub use goals::{Goal, Goals};
pub use norms::{Modality, Norm, Norms};
pub use observe::{Census, Violation, check};
pub use people::{
    AffordanceSite, EconConfig, Grievance, Inventory, Known, Liege, Market, Mood, Needs, NeedsConfig, Npc, Patron,
    Personality, Plan, Skills, Throne, WorldAffordances, price,
};
pub use plan::{Affordance, AffordEffect, Condition, Deed, GoodSel, MarketSnapshot, Need, PlanCtx, PlanState, Step, plan};

// --- Shared components / resources ---

/// Which hex an agent stands on.
#[derive(Component, Clone, Copy, Debug)]
pub struct Position(pub Coord);

/// The climate/ecosystem substrate, owned by the ECS world as a resource.
#[derive(Resource)]
pub struct Substrate(pub GameWorld);

/// Seeded randomness shared by substrate `evolve` and agent placement.
#[derive(Resource)]
pub struct SimRng(pub SplitMix64);

// --- Integration seams (inert by default; populated by the feature crates above) ---

/// Marks an agent whose autonomous planning is **suspended** — it neither plans nor
/// executes its own steps this tick (e.g. a recruited party member following the avatar).
/// A generalisation of [`Detained`]; absent on every agent by default, so a world without
/// the party layer is byte-identical. Checked by `people_plan`/`people_execute`.
#[derive(Component, Clone, Copy, Debug)]
pub struct Suspended;

/// Marks an agent that moves *with* the avatar — `player_travel` snaps it to the avatar's
/// tile each step the avatar travels. Used by recruited party members (a stack that follows
/// the hero). Absent by default, so a world without the party layer is byte-identical.
#[derive(Component, Clone, Copy, Debug)]
pub struct Follower;

/// Per-tile **entry cost in days** (≈ a day's forest walk = 1.0), indexed by topology index —
/// the field the avatar's day-budget travel reads so a road hex is crossed in a fraction of a day
/// and a mountain hex takes several. Inserted only by the exploration layer; **absent → every hex
/// costs one day (one hex per tick), so a world without it is byte-identical.**
#[derive(Resource)]
pub struct TravelCost(pub Vec<f32>);

/// How hunger drains in `people_metabolism`. `Flat` (the default) is the original
/// constant rate, so an economy run is byte-identical; the survival layer flips this to
/// `TileBiomass` to make sustenance spatial. Held as a resource so the assembler can set
/// it once at build time.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HungerModel {
    #[default]
    Flat,
    TileBiomass,
}

/// Advance the substrate one day (`Φ`).
fn advance_substrate(mut substrate: ResMut<Substrate>, mut rng: ResMut<SimRng>) {
    substrate.0.evolve(&mut rng.0);
}

/// Build the simulation's fixed-order, single-threaded per-step schedule (`Φ` then the
/// agent layers). The order is load-bearing for determinism; the thin `agents` crate runs
/// this every [`Simulation::step`](../agents/struct.Simulation.html#method.step).
pub fn build_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.set_executor_kind(ExecutorKind::SingleThreaded);
    schedule.add_systems(
        (
            advance_substrate,
            fauna::forage,
            fauna::lifecycle,
            fauna::hunt,
            fauna::carnivore_lifecycle,
            people::people_plan,
            people::people_execute,
            people::smooth_prices,
            people::discover_features,
            // The player walks its route, revealing the map and finding what it passes.
            player::player_travel,
            events::appraise,
            people::mood_shapes_traits,
            people::mood_decay,
            people::people_metabolism,
            people::regen_affordances,
            factions::faction_turn,
            factions::detention_countdown,
            // Γ runs late: it charges itself for this tick's deaths inside its
            // footprints, reads the stage, and may manufacture an escalation.
            director::director_step,
            // Dialogue runs last: it voices the social state the rest of the tick (and
            // the director) just shaped — emergent intents, and Γ's forced `Voice` beats.
            dialogue::converse,
        )
            .chain(),
    );
    schedule
}
