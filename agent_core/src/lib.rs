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
pub mod chronicle;
pub mod cohorts;
pub mod data;
pub mod dialogue;
pub mod director;
pub mod events;
pub mod factions;
pub mod fauna;
pub mod features;
pub mod fields;
pub mod goals;
pub mod gossip;
pub mod norms;
pub mod observe;
pub mod people;
pub mod perception;
pub mod plan;
pub mod player;
pub mod scalar;
pub mod sift;

pub use ai::{Consideration, Curve, Input};
pub use beats::{Beat, BeatBook, Effect, Phase, Pre, Role};
pub use chronicle::{Chronicle, Episode, EpisodeKind, Provenance};
pub use cohorts::{
    Cohort, CohortConfig, CohortMember, CohortRng, EconomyMaps, Regions, seed_regions,
};
pub use data::{
    Casting, GoodDef, GoodId, MoodDef, MoodId, Recipe, RegisterDef, RegisterId, Registry,
    ResourceKind, SkillId, TraitDef, TraitId,
};
pub use dialogue::{Dialogue, DialogueConfig, IntentBook, Utterance};
pub use director::{Cadence, Director, DirectorConfig, Protagonist, Thread};
pub use events::{AgentEvent, Appraisals, EventQueue};
pub use factions::{
    Allegiance, Bond, Detained, Faction, FactionConfig, Factions, Government, Law, Opinion,
};
pub use fauna::{
    Bestiary, Carnivore, Diet, Energy, FaunaConfig, FaunaRng, Form, Herbivore, Species, SpeciesId,
};
pub use features::{
    AffordanceDef, Category, Discovery, EffectDef, Feature, FeatureCatalog, FeatureConfig,
    FeatureDef, FeatureId, Features, FindState, NeedKind,
};
pub use fields::FieldsConfig;
pub use game_sim::{Coord, Topology};
pub use goals::{Goal, Goals};
pub use player::{
    Player, PlayerKnowledge, PlayerState, PlayerView, Rumor, SearchOutcome, Terrain, TileInfo,
};
pub use scalar::Fx;
pub use sift::{
    Axis, InterestAxis, Sift, SiftBook, SiftPattern, SiftPatternId, SiftStatus, ThreadCandidate,
};
// Only `Gossip` is re-exported at the root; `gossip::Rumor` would clash with `player::Rumor`
// (a different concept — a place heard-of, not a beat overheard), so reach it via the module.
pub use gossip::Gossip;
pub use norms::{Modality, Norm, Norms};
pub use observe::{Census, Retelling, RetoldThread, Violation, check};
pub use people::{
    AffordanceSite, EconConfig, Grievance, Inventory, Known, Liege, Market, Mood, Needs,
    NeedsConfig, Npc, Patron, Personality, Plan, Skills, Throne, WorldAffordances, price,
};
pub use perception::{
    Anchor, GrammarRealizer, Perception, PlaceMood, PlaceRealizer, ReadTier, RealizeCtx,
    RealizeHints, Realizer, ScanLine, ScanRowRealizer, Surface, Tell, TellKind, When,
};
pub use plan::{
    AffordEffect, Affordance, Condition, Deed, GoodSel, MarketSnapshot, Need, PlanCtx, PlanState,
    Step, plan,
};

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

/// Marks an NPC **dormant for this tick** under level-of-detail. A distant NPC runs on a *coarse
/// clock* — simulated only one tick in every [`SimRadius::far_stride`], staggered across NPCs — so on
/// the other ticks it carries this marker and is skipped by planning, execution, and metabolism (so
/// it can't starve on its idle ticks). It still **lives, just slowly**; it is not frozen. Toggled each
/// tick by [`lod_dormancy`]; **absent by default, so a full-detail world is byte-identical**. It does
/// not remove `Npc`, so the director, factions, and mood always see every soul — drama stays intact.
#[derive(Component, Clone, Copy, Debug)]
pub struct Dormant;

/// Marks an NPC as a **Tier-1 "drifter"** under level-of-detail (`docs/scaling.md`, Track 2): too
/// far from the avatar to be worth a full GOAP brain, so instead of planning it follows the
/// stigmergic field gradient and meets its needs by local rules ([`fields::drift`]) — cheap
/// (`O(1)`/tick) but still alive (it moves, produces, trades, and eats, just without search). The
/// generalisation of [`Dormant`] from "asleep" to "awake on a cheaper brain": where the coarse
/// clock runs a *distant* full brain occasionally, a drifter runs a *cheap* brain every tick.
/// Toggled by [`lod_dormancy`] only when the fields layer is on; **absent by default (and whenever
/// the layer is off), so a world without it is byte-identical**. Skipped by the full-brain
/// `people_plan`/`people_execute` via `Without<Drifter>`, and like `Dormant` it keeps `Npc`, so the
/// director, factions, and mood still see every soul.
#[derive(Component, Clone, Copy, Debug)]
pub struct Drifter;

/// Level-of-detail config. NPCs within `radius` hexes of the avatar run at full detail every tick;
/// farther NPCs run on a coarse clock (once per `far_stride` ticks). `radius = None` = full detail
/// everywhere — the default, byte-identical to a build without LOD. Set by the assembler from
/// `Setup::{sim_radius, sim_far_stride}`.
#[derive(Resource, Clone, Copy)]
pub struct SimRadius {
    pub radius: Option<i32>,
    pub far_stride: u32,
}

impl Default for SimRadius {
    fn default() -> Self {
        Self {
            radius: None,
            far_stride: 1,
        }
    }
}

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

/// Level-of-detail: each tick, decide which NPCs run *this* tick. Those near the avatar always do;
/// distant ones only on their turn of a coarse, staggered clock (one tick in `far_stride`) — the
/// rest carry [`Dormant`] and are skipped, so the distant world still lives but costs ~1/`far_stride`
/// as much. A no-op (byte-identical) when the radius is unset or there is no avatar. Runs first, so
/// the active set is settled before anyone plans. Dormant souls keep `Npc`, so the director, factions,
/// and mood still see them every tick — drama intact.
#[allow(clippy::type_complexity)]
pub(crate) fn lod_dormancy(
    mut commands: Commands,
    cfg: Res<SimRadius>,
    // Present only when the Track-2 stigmergic-fields layer is on. Its presence flips LOD from
    // "coarse-clock the distant full brain" to "give the distant brain the cheap gradient one"
    // ([`Drifter`]). Absent → the original behaviour, byte-identical.
    fields: Option<Res<fields::FieldsConfig>>,
    player: Res<PlayerState>,
    substrate: Res<Substrate>,
    positions: Query<&Position>,
    npcs: Query<(Entity, &Position, Has<Dormant>, Has<Drifter>), With<people::Npc>>,
) {
    let Some(r) = cfg.radius else { return };
    let Some(avatar) = player.avatar() else {
        return;
    };
    let Ok(&Position(ac)) = positions.get(avatar) else {
        return;
    };
    let width = substrate.0.topology().width();
    let tick = substrate.0.tick();
    let stride = cfg.far_stride.max(1) as u64;
    let tiered = fields.is_some();
    for (e, &Position(p), dormant, drifter) in &npcs {
        let near = within(ac, p, r, width);
        if tiered {
            // Tier 0 (full brain) within the radius; Tier 1 ([`Drifter`], gradient-follower)
            // beyond. No coarse clock — a drifter is cheap enough to run every tick.
            match (near, drifter) {
                (true, true) => {
                    commands.entity(e).remove::<Drifter>();
                }
                (false, false) => {
                    commands.entity(e).insert(Drifter);
                }
                _ => {}
            }
        } else {
            // Original LOD: a distant NPC keeps its full brain but on a staggered coarse clock —
            // active when near, or on its turn (one tick in `far_stride`, staggered by entity id).
            let active = near || stride <= 1 || tick % stride == e.to_bits() % stride;
            match (active, dormant) {
                (true, true) => {
                    commands.entity(e).remove::<Dormant>();
                }
                (false, false) => {
                    commands.entity(e).insert(Dormant);
                }
                _ => {}
            }
        }
    }
}

/// Wrapped Chebyshev "within `r` hexes" — a cheap LOD box (the world wraps east–west).
fn within(a: Coord, b: Coord, r: i32, width: i32) -> bool {
    let drow = (a.row - b.row).abs();
    let dcol = {
        let d = (a.col - b.col).abs();
        d.min(width - d)
    };
    drow <= r && dcol <= r
}

/// Build the simulation's fixed-order, single-threaded per-step schedule (`Φ` then the
/// agent layers). The order is load-bearing for determinism; the thin `agents` crate runs
/// this every [`Simulation::step`](../agents/struct.Simulation.html#method.step).
pub fn build_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.set_executor_kind(ExecutorKind::SingleThreaded);
    // Split into two chained groups (a tuple of systems caps at 20; chaining the outer pair keeps
    // the *total* order intact — group A's last runs strictly before group B's first).
    schedule.add_systems(
        (
            (
                // Tier 2 first: crystallize a region's cohort into real entities when the avatar
                // arrives (and dissolve it when it leaves), so the new cast is spawned before the
                // LOD tiers it and before anyone plans. No-op (byte-identical) when off.
                cohorts::cohort_crystallize,
                // Level-of-detail next: settle which NPCs run a full brain vs. a cheap one this
                // tick (off by default — see `SimRadius`/`FieldsConfig`).
                lod_dormancy,
                // Refresh the stigmergic fields *before* Φ, so this tick's deposits diffuse this
                // tick. Each is a no-op (byte-identical) until the fields layer is woken.
                fields::deposit_food,
                fields::deposit_danger,
                fields::deposit_demand,
                advance_substrate,
                fauna::forage,
                fauna::lifecycle,
                fauna::hunt,
                fauna::carnivore_lifecycle,
                people::people_plan,
                people::people_execute,
                // Tier-1 masses act after the full-brain cast: one cheap gradient-following turn
                // each (move/produce/trade/eat), against the same live world. No-op when off.
                fields::drift,
                // Tier-2 masses advance as aggregate integer flows through their regional markets
                // (produce/consume/migrate), O(regions) regardless of headcount. No-op when off.
                cohorts::cohort_step,
                people::smooth_prices,
                people::discover_features,
                // The player walks its route, revealing the map and finding what it passes.
                player::player_travel,
            )
                .chain(),
            (
                events::appraise,
                people::mood_shapes_traits,
                people::mood_decay,
                people::people_metabolism,
                people::regen_affordances,
                factions::faction_turn,
                factions::detention_countdown,
                // The sifter reads the Chronicle (incrementally) into ranked story candidates just
                // before Γ, so the graft can consult forming stories from *this* tick. A no-op when
                // the sift layer is off; writes only its own resource, so off => byte-identical.
                sift::sift_step,
                // Γ runs late: it charges itself for this tick's deaths inside its
                // footprints, reads the stage, and may manufacture an escalation.
                director::director_step,
                // Dialogue runs last: it voices the social state the rest of the tick (and
                // the director) just shaped — emergent intents, and Γ's forced `Voice` beats.
                dialogue::converse,
                // Gossip spreads after the talk: rumours of Γ's beats pass between co-located souls,
                // decaying each hop. A no-op (byte-identical) until the director seeds the first one.
                gossip::gossip_spread,
                // The Perception pass runs last, reading the freshest Chronicle + Sifter into ranked
                // `Tell`s the player surfaces filter. Writes only its own resource; a no-op (the
                // resource absent) when the layer is off, so off => byte-identical.
                perception::perception_step,
            )
                .chain(),
        )
            .chain(),
    );
    schedule
}
