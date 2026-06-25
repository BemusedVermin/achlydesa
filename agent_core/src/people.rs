//! People and the market they trade in — built over the authored [`Registry`] and
//! [`Goals`], so goods, recipes, skills, and the very things people *want* are
//! data, not code.
//!
//! ## Two layers of mind
//! Each person decides in two stages:
//! - **What do I want?** The utility layer ranks the authored [`Goals`] by appeal
//!   (how far each is from satisfied) and picks the most pressing one it can make
//!   progress on — see [`Goals::ranked`].
//! - **How do I get it?** The planner ([`plan`]) searches the actions available
//!   and returns a sequence that makes the chosen goal's condition true. The
//!   person caches that plan and performs one step per tick.
//!
//! So "bake then eat", "walk to market, buy bread, walk home", "haul grain to a
//! dearer market", "restock the larder while not even hungry", and "save toward a
//! cushion" are all *found by search* against *authored wants* — none of it is
//! hand-written behaviour. Adding a good, recipe, or goal is a data edit.
//!
//! Goods are counted in whole units and money in whole coins; needs and skills are
//! continuous. Production scales a recipe's whole-unit output by skill and the
//! natural-resource level, then rounds.

// coupling-lint:allow string_ids: the throne/feud machinery refers to named traits/predicates
// (ambition, vengeance, enthroned, alive) — necessary semantic references, not an instance table.
use crate::chronicle::EpisodeKind;
use crate::data::{GoodId, PredicateId, ROLE_COUNT, Registry, ResourceKind, fact_slot};
use crate::events::{AgentEvent, EventQueue};
use crate::factions::{Allegiance, Detained, Factions, Law, Opinion};
use crate::features::{Discovery, EffectDef, FeatureCatalog, Features, NeedKind};
use crate::goals::Goals;
use crate::norms::{Modality, Norm, Norms};
use crate::plan::{
    AffordEffect, Affordance, Deed, Facts, MarketSnapshot, Need, PlanCtx, PlanState, Step, Stock,
    plan,
};
use crate::scalar::Fx;
use crate::{Position, Substrate};
use bevy_ecs::prelude::*;
use game_sim::{Coord, SplitMix64, Topology, World as GameWorld};
use sim::Rng;
use std::collections::{HashMap, HashSet, VecDeque};

// --- Components ---

/// A non-player person.
#[derive(Component, Clone, Copy, Debug)]
pub struct Npc;

/// Short-term drives, `0` (desperate) to `100` (sated). Continuous meters.
#[derive(Component, Clone, Copy, Debug)]
pub struct Needs {
    pub sustenance: f32,
    pub rest: f32,
}

/// Per-skill proficiency, indexed by the registry's skill ids. **Doubles as a
/// person's calling:** a skill at `0` is one they were never taught and cannot
/// practise (a baker has `farming == 0`), so the recipes they can run are gated by
/// where they have any proficiency, while the same number scales the yield of the
/// recipes they *can* run. Born with their calling(s) seeded and the rest at `0`,
/// then grows by doing — so production splits across people and trade for what you
/// cannot make yourself becomes a necessity, not a convenience.
#[derive(Component, Clone, Debug, Default)]
pub struct Skills(pub Vec<Fx>);

/// Coins plus a whole-unit count of every good (indexed by [`GoodId`]).
#[derive(Component, Clone, Debug)]
pub struct Inventory {
    pub money: i64,
    pub stock: Vec<u32>,
}

/// The home market a person was settled at. (Trade is by physical proximity now,
/// so this is a spawn anchor rather than a hard tie.)
#[derive(Component, Clone, Copy, Debug)]
pub struct Patron(pub Entity);

/// A person's current intention: the goal it is pursuing (an index into [`Goals`])
/// and the steps it has left planned to reach it. Empty between plans.
#[derive(Component, Clone, Debug, Default)]
pub struct Plan {
    pub goal: Option<usize>,
    pub steps: VecDeque<Step>,
    /// Whether the act being pursued is one the prevailing norms *forbid* (and don't
    /// excuse) — judged when the goal was chosen. Carries the social verdict from
    /// deliberation to the deed, so [`people_execute`] can mark a taboo broken as a
    /// transgression (see [`Norms::forbids`](crate::norms::Norms::forbids)).
    pub illicit: bool,
}

/// A market: a whole-unit stock of every good and a coin pool to pay sellers
/// from. Sets prices, never yields.
#[derive(Component, Clone, Debug)]
pub struct Market {
    pub stock: Vec<u32>,
    pub money: i64,
    /// The **smoothed** stock prices are read from — an exponential moving average
    /// of `stock` that lags it ([`smooth_prices`]). Real `stock` still governs what
    /// can be bought or sold; this only governs *price*, so a glut or a run doesn't
    /// whipsaw the price within a tick and the cobweb boom-bust is damped. Agents
    /// plan and trade against the same lagged price, so a plan's expected payoff
    /// matches what it actually gets.
    pub price_basis: Vec<f32>,
}

/// The tiles a person has personally **discovered** — the topology indices whose
/// Hidden/Secret features it has found by being there. Landmarks need no entry (they
/// are seen by everyone); this is what stops an agent acting on a hidden place it has
/// never visited. Each person carries its own map, so knowledge is private and
/// spreads only by exploration (not yet by word of mouth).
#[derive(Component, Clone, Debug, Default)]
pub struct Known(pub HashSet<usize>);

/// A person's personality: a value per authored trait (indexed by the registry's
/// trait ids — ambition, greed, vengeance, …). Stable and innate; changes only
/// through appraised events (see [`events`](crate::events)).
#[derive(Component, Clone, Debug)]
pub struct Personality(pub Vec<f32>);

/// A person's current mood: a value per authored emotion (anger, fear, joy, …),
/// resting at zero. Appraised events spike it; [`mood_decay`] fades it back. Goals'
/// appeal can read it to weight behaviour by how the agent feels right now.
#[derive(Component, Clone, Debug)]
pub struct Mood(pub Vec<f32>);

/// A grudge against a specific other person — the one this agent would see dead.
/// Binds the `?foe` role of an `avenge`-style goal (`alive(foe) = false`), so "kill
/// THIS foe" is an entity-targeted instance of the relational goal grammar.
#[derive(Component, Clone, Copy, Debug)]
pub struct Grievance(pub Entity);

/// The lord a vassal serves. A bond that carries *duty*: when the liege is slain,
/// the vassal takes up the quarrel — inheriting a [`Grievance`] against the killer
/// — and a society that *obliges* vengeance (an `Obliged` norm on `avenge`) will
/// move even a mild vassal to hunt them down. The lord's cause outlives the lord.
#[derive(Component, Clone, Copy, Debug)]
pub struct Liege(pub Entity);

/// A durable, positive bond to a specific soul — the structural inverse of [`Grievance`]:
/// the one this agent holds dear (a vow kept, oath-kin, a love). Set by a `Bond` beat and
/// read by the `Bonded` precondition, so a love the director built can later be the thing it
/// breaks. Directed and single-target like `Grievance`; a reciprocal tie is two components.
#[derive(Component, Clone, Copy, Debug)]
pub struct Bond(pub Entity);

/// The throne: a single, *shared* world fact — who, if anyone, rules. Unlike a
/// larder or purse this is one global thing many agents contend over, so seizing
/// it changes the world for everyone (the coherence test for shared facts). When
/// no throne exists in a scenario, this resource is simply absent.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Throne {
    pub tile: Coord,
    pub holder: Option<Entity>,
}

// --- Feature affordances (the smart-object layer) ---

/// A live feature affordance: a place-based action (gather or relieve) plus the
/// depletion state that lets a worked site run dry and slowly refill — the
/// stigmergic mark that working a resource leaves on the world. The order of these
/// is stable, so [`Step::Use`] indices line up between planning and execution
/// within a tick even as `remaining` changes.
#[derive(Clone, Copy, Debug)]
pub struct AffordanceSite {
    pub at: Coord,
    /// Topology index of `at` — the key into a person's [`Known`] set.
    pub tile: usize,
    pub effect: AffordEffect,
    /// On a Hidden/Secret feature, so usable only by someone who has discovered it.
    pub needs_discovery: bool,
    /// Uses left before the site is worked out; `capacity == 0` is inexhaustible.
    pub remaining: f32,
    pub capacity: u32,
    pub regen: f32,
    /// Cumulative times this site has been used — instrumentation for the observer,
    /// so we can *see* that agents actually use the POIs rather than assume it.
    pub uses: u64,
}

impl AffordanceSite {
    /// Is there a use to be had here right now? Inexhaustible sites always; a
    /// depletable one only while it still holds a whole use.
    pub fn available(&self) -> bool {
        self.capacity == 0 || self.remaining >= 1.0
    }
}

/// Every feature affordance in the world, resolved against the registry. A shared
/// resource the planner reads (which sites afford what) and execution mutates
/// (depleting a worked site).
#[derive(Resource, Clone, Debug, Default)]
pub struct WorldAffordances(pub Vec<AffordanceSite>);

/// Resolve the placed features' authored affordances into live sites. Unknown
/// goods/skills are skipped — a scenario whose economy lacks them simply has no such
/// affordance — so the feature catalog and the registry stay independently editable.
pub fn build_affordances(
    catalog: &FeatureCatalog,
    features: &Features,
    reg: &Registry,
    topo: &Topology,
) -> WorldAffordances {
    let mut sites = Vec::new();
    for (i, f) in features.iter() {
        let at = topo.coord(i);
        let needs_discovery = catalog.def(f.kind).discovery != Discovery::Landmark;
        for ad in &catalog.def(f.kind).affordances {
            let Some(effect) = resolve_effect(&ad.effect, reg) else {
                continue;
            };
            sites.push(AffordanceSite {
                at,
                tile: i,
                effect,
                needs_discovery,
                remaining: ad.capacity as f32,
                capacity: ad.capacity,
                regen: ad.regen,
                uses: 0,
            });
        }
    }
    WorldAffordances(sites)
}

fn resolve_effect(e: &EffectDef, reg: &Registry) -> Option<AffordEffect> {
    Some(match e {
        EffectDef::Relieve { need, amount } => AffordEffect::Relieve {
            need: need_of(*need),
            amount: *amount,
        },
        EffectDef::Yield { good, units, skill } => {
            let g = reg.good_id(good)?;
            let sk = match skill {
                Some(n) => Some(reg.skill_id(n)?),
                None => None,
            };
            AffordEffect::Yield {
                good: g,
                units: *units,
                skill: sk,
            }
        }
        EffectDef::Teach { skill } => AffordEffect::Teach {
            skill: reg.skill_id(skill)?,
        },
    })
}

/// Proficiency a freshly-taught trade starts at — a novice, below a born
/// specialist's endowment, then grown by doing.
const LEARNED_SKILL: f32 = 0.25;

fn need_of(n: NeedKind) -> Need {
    match n {
        NeedKind::Sustenance => Need::Sustenance,
        NeedKind::Rest => Need::Rest,
    }
}

/// Worked sites recover toward capacity each tick — the land replenishing what was
/// taken, so a foraged grove or a fished cove stays a renewable destination rather
/// than a one-shot. Inexhaustible sites (`capacity == 0`) need nothing.
pub(crate) fn regen_affordances(mut affordances: ResMut<WorldAffordances>) {
    for s in &mut affordances.0 {
        if s.capacity > 0 && s.remaining < s.capacity as f32 {
            s.remaining = (s.remaining + s.regen).min(s.capacity as f32);
        }
    }
}

// --- Global config (knobs only — no per-good/recipe/skill data) ---
// The config *data* ([`EconConfig`]/[`NeedsConfig`]) lives Bevy-free in the
// `config` crate; here we re-export it and wrap the two systems read as ECS
// resources in thin newtypes (so the engine never touches the config types).

pub use config::{EconConfig, NeedsConfig};

/// ECS-resource handle for the [`EconConfig`] knobs. Derefs to the config, so
/// systems read `econ.price_floor_frac` exactly as before.
#[derive(Resource, Clone, Debug)]
pub struct EconRes(pub EconConfig);

impl std::ops::Deref for EconRes {
    type Target = EconConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// ECS-resource handle for the [`NeedsConfig`] knobs.
#[derive(Resource, Clone, Debug)]
pub struct NeedsRes(pub NeedsConfig);

impl std::ops::Deref for NeedsRes {
    type Target = NeedsConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Hard cap on A\* expansions per plan — keeps replanning real-time safe. The default the
/// search runs at; [`PlanConfig`] can lower it for cheaper, shorter-horizon planning.
const NODE_BUDGET: usize = 600;

/// Tunable cap on A\* expansions per plan — the bulk knob behind `docs/scaling.md`'s "lower
/// `NODE_BUDGET` for the masses". A smaller budget makes planning cheaper (fewer node
/// expansions, so fewer `PlanState` clones — the dominant per-tick allocation) at the cost
/// of a shorter planning horizon. **Absent, or set to [`NODE_BUDGET`], is the original
/// 600-node search — byte-identical**, so a run that doesn't touch this knob is unchanged.
/// The assembler sets it from `Setup::plan_budget`.
#[derive(Resource, Clone, Copy, Debug)]
pub struct PlanConfig {
    pub node_budget: usize,
}

impl Default for PlanConfig {
    fn default() -> Self {
        Self {
            node_budget: NODE_BUDGET,
        }
    }
}

/// Weight of a faction's taboo as a deontic prohibition on its members — how strongly
/// a faction's law suppresses a member's appetite to break it (a unit taboo, like the
/// society-wide ones).
const FACTION_LAW_WEIGHT: f32 = 1.0;

// --- Pricing ---

/// Market price (coins per unit) of a good given its stock: `base · target /
/// stock`, rounded and clamped to a band around the base price.
pub fn price(reg: &Registry, econ: &EconConfig, good: GoodId, stock: u32) -> i64 {
    let g = reg.good(good);
    let raw = (g.base_price as f64 * g.target_stock as f64) / stock.max(1) as f64;
    let floor = (g.base_price as f64 * econ.price_floor_frac as f64).max(1.0);
    let ceil = g.base_price as f64 * econ.price_ceil_frac as f64;
    raw.clamp(floor, ceil).round() as i64
}

// --- World reads (for the planner's closures) ---

/// Read every natural-resource level at a tile from the substrate.
pub fn read_resources(world: &GameWorld, c: Coord) -> [f32; ResourceKind::COUNT] {
    let mut r = [0.0; ResourceKind::COUNT];
    r[ResourceKind::Fertility.idx()] = world.carrying_capacity(c);
    r[ResourceKind::Vegetation.idx()] = world.plant_biomass(c);
    r[ResourceKind::Minerals.idx()] = world.minerals(c);
    r[ResourceKind::Water.idx()] = world.surface_water(c);
    r
}

/// Draw a resource down at a tile (for recipes that deplete what they harvest).
pub(crate) fn deplete_resource(world: &mut GameWorld, c: Coord, kind: ResourceKind, amount: f32) {
    match kind {
        ResourceKind::Vegetation => {
            world.graze(c, amount);
        }
        ResourceKind::Minerals => {
            world.mine(c, amount);
        }
        ResourceKind::Fertility | ResourceKind::Water => {} // renewable / not depleted
    }
}

/// The planner's **movement graph**: the land tiles reachable in one step from each
/// tile. It depends only on elevation and sea level — both fixed at worldgen, never
/// touched by `evolve` — so it is *static for the whole run*. Built once (in
/// `Simulation::new`) and shared as a resource, rather than
/// rebuilt every tick. Stored CSR-style — one flat `neighbors` buffer plus per-tile
/// `offsets` — so a lookup hands back a borrowed slice with no allocation, and the
/// whole graph is two allocations instead of one small `Vec` per tile per tick.
#[derive(Resource)]
pub struct MoveGraph {
    /// `offsets[t]..offsets[t+1]` is tile `t`'s slice of `neighbors`.
    offsets: Vec<u32>,
    neighbors: Vec<Coord>,
}

impl MoveGraph {
    /// Derive the land-movement graph from the (final) terrain.
    pub fn build(world: &GameWorld) -> Self {
        let topo = world.topology();
        let sea = world.params().sea_level;
        let mut offsets = Vec::with_capacity(topo.len() + 1);
        let mut neighbors = Vec::new();
        offsets.push(0);
        for i in 0..topo.len() {
            // Same order as the old per-tile build (topology order, land-filtered), so
            // the planner's successors — and thus every plan — are bit-for-bit identical.
            for l in topo.neighbors(i) {
                let n = topo.coord(l.to);
                if world.elevation(n) >= sea {
                    neighbors.push(n);
                }
            }
            offsets.push(neighbors.len() as u32);
        }
        Self { offsets, neighbors }
    }

    /// The land tiles reachable in one step from tile index `t`.
    #[inline]
    pub fn neighbors(&self, t: usize) -> &[Coord] {
        &self.neighbors[self.offsets[t] as usize..self.offsets[t + 1] as usize]
    }
}

/// Is `pos` on, or next to, the market tile `market`? (A market's catchment is
/// its own tile plus its neighbours.)
fn adjacent_or_on(world: &GameWorld, pos: Coord, market: Coord) -> bool {
    pos == market || {
        let topo = world.topology();
        topo.neighbors(topo.index_of(pos))
            .iter()
            .any(|l| topo.coord(l.to) == market)
    }
}

// --- The act systems (decide, then do) ---
//
// Acting is split in two so the expensive part is cheap to parallelize and the
// cheap part stays deterministic:
//   * `people_plan` is read-only over a start-of-tick world (a market snapshot and
//     per-tile caches) and writes only each person's own `Plan` — so it is
//     order-independent and runs in parallel.
//   * `people_execute` performs one planned step against the live world, in a
//     fixed order, so market/substrate mutations stay deterministic and money is
//     exactly conserved.

/// Decide phase: every person ranks its authored goals by appeal and (re)plans
/// toward the most pressing one it can make progress on, when the cached plan is
/// spent or the goal changed. Walking goals by appeal and taking the first that
/// yields a plan means a goal it can't progress on is skipped for the next best —
/// opportunity-gating falls out of "is there a plan?", nothing is hardcoded.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn people_plan(
    mut npcs: Query<
        (
            Entity,
            &Needs,
            &Skills,
            &Inventory,
            &mut Plan,
            &Position,
            &Personality,
            &Mood,
            Option<&Grievance>,
            &Known,
            Option<&Detained>,
            &Allegiance,
        ),
        (
            With<Npc>,
            Without<crate::Suspended>,
            Without<crate::Dormant>,
            // Tier-1 drifters run the cheap gradient brain (`fields::drift`), not the full
            // GOAP plan/execute. Absent by default, so a non-tiered world is byte-identical.
            Without<crate::Drifter>,
        ),
    >,
    markets: Query<(&Position, &Market), Without<Npc>>,
    substrate: Res<Substrate>,
    reg: Res<Registry>,
    econ: Res<EconRes>,
    needs_cfg: Res<NeedsRes>,
    goals: Res<Goals>,
    norms: Res<Norms>,
    move_graph: Res<MoveGraph>,
    world_affordances: Res<WorldAffordances>,
    factions: Res<Factions>,
    throne: Option<Res<Throne>>,
    plan_cfg: Res<PlanConfig>,
) {
    // The A* node budget for this run — the configured cap (default 600, so an untouched run is
    // byte-identical). The assembler inserts `PlanConfig` unconditionally, so this is a required
    // resource: a missing insertion fails loudly here rather than silently reverting to the
    // constant. A plain `usize`, copied into the parallel planning closure below.
    let node_budget = plan_cfg.node_budget;
    // One start-of-tick snapshot of every market; everyone plans against the same
    // world, and live trades in `people_execute` keep money exactly conserved.
    let snapshots: Vec<MarketSnapshot> = markets
        .iter()
        .map(|(p, m)| MarketSnapshot {
            pos: p.0,
            stock: m.stock.clone(),
            price_basis: m
                .price_basis
                .iter()
                .map(|&x| x.round().max(0.0) as u32)
                .collect(),
            money: m.money,
        })
        .collect();
    // The feature affordances on offer this tick, as the planner sees them — a
    // worked-out site reads unavailable so no one routes to it.
    let affordances: Vec<Affordance> = world_affordances
        .0
        .iter()
        .map(|s| Affordance {
            at: s.at,
            tile: s.tile,
            effect: s.effect,
            available: s.available(),
            needs_discovery: s.needs_discovery,
        })
        .collect();
    // Where everyone stands — so an aggrieved agent can locate (and a foe present
    // means: alive) its target. Absent from this map = dead.
    let people_pos: HashMap<Entity, Coord> = npcs.iter().map(|q| (q.0, q.5.0)).collect();
    let throne = throne.as_deref();
    // Only the genuinely ambitious scheme for the throne (the demo's rule for who
    // may seize it); everyone else ignores it whatever their idle whims.
    let ambition = reg.trait_id("ambition");
    // Flat fact slots the relational goals ground to.
    let enthroned = reg.predicate_id("enthroned");
    let alive = reg.predicate_id("alive");
    // The trait that resists a faction's law (as it resists the society's taboos), so a
    // vengeful soul feels even its faction's no-kill law lightly.
    let law_defiance = reg.trait_id("vengeance");

    // Per-tick resource cache: levels change every tick (climate, grazing), so it's
    // rebuilt — but as one contiguous buffer for allocation-free O(1) lookups. The
    // movement graph is *static*, so it lives in `move_graph` and is never rebuilt.
    let topo = substrate.0.topology();
    let resources_cache: Vec<[f32; ResourceKind::COUNT]> = (0..topo.len())
        .map(|i| read_resources(&substrate.0, topo.coord(i)))
        .collect();

    // Planning touches no shared mutable state (only this person's `Plan`), so it
    // is order-independent — safe to run across threads.
    npcs.par_iter_mut().for_each(
        |(
            entity,
            needs,
            skills,
            inv,
            mut plan_c,
            pos,
            personality,
            mood,
            grievance,
            known,
            detained,
            allegiance,
        )| {
            // A person in a faction's cells does nothing — its plan is suspended.
            if detained.is_some() {
                plan_c.goal = None;
                plan_c.steps.clear();
                return;
            }
            // Ground the relational facts this agent reasons about into the planner's
            // flat fact vector (predicate × role). `enthroned(self)` reflects whether
            // this agent rules; `alive(foe)` whether its grudge still draws breath. Each
            // gets the deed that would change it, if the agent is in a position to.
            let mut facts = Facts::from_elem(0i64, reg.predicate_count() * ROLE_COUNT);
            let mut deeds = Vec::new();
            if let (Some(t), Some(e)) = (throne, enthroned) {
                facts[fact_slot(e, 0)] = i64::from(t.holder == Some(entity));
                if ambition.is_some_and(|a| personality.0.get(a).is_some_and(|&v| v > 0.6)) {
                    deeds.push(Deed {
                        at: t.tile,
                        fact: fact_slot(e, 0),
                        value: 1,
                    });
                }
            }
            if let (Some(g), Some(a)) = (grievance, alive)
                && let Some(&foe_pos) = people_pos.get(&g.0)
            {
                facts[fact_slot(a, 1)] = 1; // the foe lives
                deeds.push(Deed {
                    at: foe_pos,
                    fact: fact_slot(a, 1),
                    value: 0,
                });
            }

            let start = PlanState {
                sustenance: needs.sustenance.round() as i32,
                rest: needs.rest.round() as i32,
                money: inv.money,
                stock: Stock::from_slice(&inv.stock),
                pos: pos.0,
                facts,
                learned: Stock::from_elem(0u32, reg.skill_count()),
            };

            // This person's *effective* norms: the society's, plus a prohibition for each
            // of its factions' taboos — so a member is reluctant to break its faction's law
            // (its appeal suppressed), not merely policed for breaking it. Built only when
            // the agent has faction laws; otherwise the society's norms stand unchanged.
            let faction_taboos: Vec<(PredicateId, i64)> = allegiance
                .0
                .iter()
                .filter_map(|b| factions.at(b.seat))
                .flat_map(|f| f.laws.iter())
                .filter_map(|l| {
                    if let Law::Taboo(p, v) = l {
                        Some((*p, *v))
                    } else {
                        None
                    }
                })
                .collect();
            let extra: Norms;
            let effective: &Norms = if faction_taboos.is_empty() {
                &norms
            } else {
                let mut e = (*norms).clone();
                for (p, v) in faction_taboos {
                    if !e
                        .0
                        .iter()
                        .any(|n| n.act == (p, v) && n.modality == Modality::Forbidden)
                    {
                        e.0.push(Norm {
                            act: (p, v),
                            modality: Modality::Forbidden,
                            weight: FACTION_LAW_WEIGHT,
                            when: None,
                            defiance: law_defiance,
                        });
                    }
                }
                extra = e;
                &extra
            };

            // The goals worth pursuing now, best first — already excluding satisfied and
            // appeal-vetoed (e.g. taboo) wants, so the planner never falls through to one.
            let agenda = goals.agenda(&start, &reg, &personality.0, &mood.0, effective);
            let chosen = agenda.first().copied();

            if plan_c.goal != chosen || plan_c.steps.is_empty() {
                let resources = |c: Coord| resources_cache[topo.index_of(c)];
                let neighbors = |c: Coord| move_graph.neighbors(topo.index_of(c));
                let ctx = PlanCtx {
                    reg: &reg,
                    econ: &econ,
                    needs_cfg: &needs_cfg,
                    skills: &skills.0,
                    markets: &snapshots,
                    deeds: &deeds,
                    affordances: &affordances,
                    known: &|i| known.0.contains(&i),
                    resources: &resources,
                    neighbors: &neighbors,
                    node_budget,
                };
                let mut goal = None;
                let mut steps = Vec::new();
                for &i in &agenda {
                    // Plan the next reachable leg of the goal, not the whole distance.
                    let target = goals.0[i].condition.planning_target(&start, &reg);
                    let s = plan(&target, &start, &ctx);
                    if !s.is_empty() {
                        goal = Some(i);
                        steps = s;
                        break;
                    }
                }
                plan_c.goal = goal;
                plan_c.steps = steps.into_iter().collect();
                // Record whether the act now being pursued is a forbidden one, so a kill
                // carried through despite the taboo is marked a transgression at the deed.
                plan_c.illicit =
                    goal.is_some_and(|i| effective.forbids(goals.0[i].act, &start, &reg));
            }
        },
    );
}

/// Act phase: every person performs the next step of its plan against the live
/// world. Trades move whole units and coins; production consumes/creates inventory
/// and trains skill; grazing and harvesting mutate the substrate; moving changes
/// the person's hex. A step whose precondition has since broken (a drained market,
/// a foe gone) is abandoned and the person replans next tick.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn people_execute(
    mut commands: Commands,
    mut npcs: Query<
        (
            Entity,
            &mut Needs,
            &mut Skills,
            &mut Inventory,
            &mut Plan,
            &mut Position,
            Option<&Grievance>,
            &Known,
            Option<&Detained>,
        ),
        (
            With<Npc>,
            Without<crate::Suspended>,
            Without<crate::Dormant>,
            // Tier-1 drifters run the cheap gradient brain (`fields::drift`), not the full
            // GOAP plan/execute. Absent by default, so a non-tiered world is byte-identical.
            Without<crate::Drifter>,
        ),
    >,
    lieges: Query<(Entity, &Liege), With<Npc>>,
    mut markets: Query<(Entity, &Position, &mut Market), Without<Npc>>,
    mut substrate: ResMut<Substrate>,
    reg: Res<Registry>,
    econ: Res<EconRes>,
    needs_cfg: Res<NeedsRes>,
    hunger: Option<Res<crate::HungerModel>>,
    mut world_affordances: ResMut<WorldAffordances>,
    mut throne: Option<ResMut<Throne>>,
    mut events: ResMut<EventQueue>,
    // The Chronicle (present only when the sift layer is woken) hears the emergent deeds done here —
    // a killing, a crowning, a deposing, a transgression, an inherited grudge. Off => `None` => no-op.
    mut chronicle: Option<ResMut<crate::chronicle::Chronicle>>,
) {
    let tick = substrate.0.tick();
    // Market index (as referenced by `Step::Buy/Sell`) -> entity and tile. Same
    // query filter and order as `people_plan`'s snapshot, so the indices align.
    let market_entities: Vec<(Entity, Coord)> = markets.iter().map(|(e, p, _)| (e, p.0)).collect();
    // Where everyone stands, to resolve a strike against a foe.
    let people_pos: HashMap<Entity, Coord> = npcs.iter().map(|q| (q.0, q.5.0)).collect();
    // Vassals indexed by the lord they serve, so a slain lord's followers can take
    // up the quarrel against whoever struck him down.
    let mut vassals_of: HashMap<Entity, Vec<Entity>> = HashMap::new();
    for (vassal, liege) in &lieges {
        vassals_of.entry(liege.0).or_default().push(vassal);
    }

    for (entity, mut needs, mut skills, mut inv, mut plan_c, mut pos, grievance, known, detained) in
        &mut npcs
    {
        if detained.is_some() {
            continue; // held by enforcers — cannot act
        }
        let Some(step) = plan_c.steps.pop_front() else {
            continue;
        };

        let done = match step {
            Step::Eat(g) => {
                if inv.stock[g] == 0 {
                    false
                } else {
                    inv.stock[g] -= 1;
                    needs.sustenance = (needs.sustenance + reg.good(g).nutrition).min(100.0);
                    true
                }
            }
            Step::Graze => {
                // Spatial subsistence under the survival layer: you graze only what the tile bears
                // (barren wastes give nothing, lush land sustains). Flat relief otherwise — the
                // unchanged economy default, so a world without survival is byte-identical.
                let relief = match hunger.as_deref().copied().unwrap_or_default() {
                    crate::HungerModel::Flat => needs_cfg.eat_grass_relief,
                    crate::HungerModel::TileBiomass => {
                        let frac = (substrate.0.plant_biomass(pos.0)
                            / substrate.0.params().biomass_max)
                            .clamp(0.0, 1.0);
                        needs_cfg.eat_grass_relief * frac
                    }
                };
                substrate.0.graze(pos.0, 1.0);
                needs.sustenance = (needs.sustenance + relief).min(100.0);
                true
            }
            Step::Rest => {
                needs.rest = (needs.rest + needs_cfg.rest_recovery).min(100.0);
                true
            }
            Step::Make(i) => {
                let r = &reg.recipes()[i];
                let level = r
                    .resource
                    .map(|k| read_resources(&substrate.0, pos.0)[k.idx()]);
                let resource_ok = level.is_none_or(|l| l >= r.min_resource);
                let inputs_ok = !r.inputs.iter().any(|&(g, qty)| inv.stock[g] < qty);
                let can_practise = skills.0.get(r.skill).is_some_and(|&s| s > 0.0);
                if !(can_practise && resource_ok && inputs_ok) {
                    false
                } else {
                    let scale = level.unwrap_or(1.0);
                    for &(g, qty) in &r.inputs {
                        inv.stock[g] -= qty;
                    }
                    let skill = skills.0[r.skill];
                    for &(g, qty) in &r.outputs {
                        inv.stock[g] += (Fx::saturating_from_num(qty)
                            * (Fx::ONE + skill)
                            * Fx::saturating_from_num(scale))
                        .round()
                        .saturating_to_num::<u32>();
                    }
                    let sd = reg.skill(r.skill);
                    skills.0[r.skill] = (skill + Fx::saturating_from_num(sd.gain))
                        .min(Fx::saturating_from_num(sd.cap));
                    if let Some(kind) = r.resource
                        && r.deplete > 0.0
                    {
                        deplete_resource(&mut substrate.0, pos.0, kind, r.deplete);
                    }
                    true
                }
            }
            Step::Buy {
                good,
                units,
                market,
            } => {
                let (entity, mpos) = market_entities[market];
                if !adjacent_or_on(&substrate.0, pos.0, mpos) {
                    false
                } else {
                    let (_, _, mut m) = markets.get_mut(entity).unwrap();
                    let p = price(
                        &reg,
                        &econ,
                        good,
                        m.price_basis[good].round().max(0.0) as u32,
                    );
                    let bought = units.min(m.stock[good]).min((inv.money / p.max(1)) as u32);
                    if bought > 0 {
                        inv.money -= bought as i64 * p;
                        inv.stock[good] += bought;
                        m.stock[good] -= bought;
                        m.money += bought as i64 * p;
                        true
                    } else {
                        false
                    }
                }
            }
            Step::Sell {
                good,
                units,
                market,
            } => {
                let (entity, mpos) = market_entities[market];
                if !adjacent_or_on(&substrate.0, pos.0, mpos) {
                    false
                } else {
                    let (_, _, mut m) = markets.get_mut(entity).unwrap();
                    let p = price(
                        &reg,
                        &econ,
                        good,
                        m.price_basis[good].round().max(0.0) as u32,
                    );
                    let sold = units.min((m.money / p.max(1)) as u32).min(inv.stock[good]);
                    if sold > 0 {
                        inv.stock[good] -= sold;
                        inv.money += sold as i64 * p;
                        m.stock[good] += sold;
                        m.money -= sold as i64 * p;
                        true
                    } else {
                        false
                    }
                }
            }
            Step::Move(to) => {
                pos.0 = to;
                true
            }
            // A deed's real effect is re-derived from where the agent stands (the
            // planner only knows it sets a fact). On the throne tile → seize it
            // (usurping the holder, who is deposed; both live the event). On a
            // foe's tile → strike the foe down; if the foe ruled, the throne falls
            // vacant. (Today these are the only two deeds.)
            Step::Do(_) => {
                if throne.as_deref().is_some_and(|t| pos.0 == t.tile) {
                    let t = throne.as_deref_mut().unwrap();
                    if let Some(prev) = t.holder
                        && prev != entity
                    {
                        events.0.push((prev, AgentEvent::Deposed));
                        if let Some(c) = chronicle.as_deref_mut() {
                            c.record(
                                tick,
                                EpisodeKind::Deposed,
                                [Some(prev), Some(entity), None],
                                pos.0,
                                None,
                                0,
                            );
                        }
                    }
                    t.holder = Some(entity);
                    events.0.push((entity, AgentEvent::Crowned));
                    if let Some(c) = chronicle.as_deref_mut() {
                        c.record(
                            tick,
                            EpisodeKind::Crowned,
                            [Some(entity), None, None],
                            pos.0,
                            None,
                            0,
                        );
                    }
                    true
                } else if let Some(foe) = grievance.map(|g| g.0)
                    && people_pos.get(&foe) == Some(&pos.0)
                {
                    if let Some(t) = throne.as_deref_mut()
                        && t.holder == Some(foe)
                    {
                        t.holder = None;
                    }
                    // The apex narratable deed — recorded with its true cast (slayer, victim)
                    // before the body leaves the world.
                    if let Some(c) = chronicle.as_deref_mut() {
                        c.record(
                            tick,
                            EpisodeKind::Killed,
                            [Some(entity), Some(foe), None],
                            pos.0,
                            None,
                            0,
                        );
                    }
                    commands.entity(foe).despawn();
                    // A killing done in the teeth of a taboo is a transgression — it
                    // marks the killer (appraised into a lasting change of character).
                    if plan_c.illicit {
                        events.0.push((entity, AgentEvent::Transgressed));
                        if let Some(c) = chronicle.as_deref_mut() {
                            c.record(
                                tick,
                                EpisodeKind::Transgressed,
                                [Some(entity), None, None],
                                pos.0,
                                None,
                                0,
                            );
                        }
                    }
                    // The slain lord's vassals inherit his quarrel: each takes up a
                    // grudge against the killer, to be driven home by a duty to avenge.
                    if let Some(vassals) = vassals_of.get(&foe) {
                        for &v in vassals {
                            if v != entity {
                                commands.entity(v).insert(Grievance(entity));
                                if let Some(c) = chronicle.as_deref_mut() {
                                    c.record(
                                        tick,
                                        EpisodeKind::GrievanceFormed,
                                        [Some(v), Some(entity), None],
                                        pos.0,
                                        None,
                                        0,
                                    );
                                }
                            }
                        }
                    }
                    true
                } else {
                    false
                }
            }
            // Use a feature affordance: the planner only knew "this gathers/relieves
            // here"; the real effect is re-applied from the live site (so it honours
            // depletion). Working a depletable site draws it down — the stigmergic mark.
            Step::Use(i) => match world_affordances.0.get_mut(i) {
                Some(site)
                    if site.at == pos.0
                        && site.available()
                        && (!site.needs_discovery || known.0.contains(&site.tile)) =>
                {
                    let applied = match site.effect {
                        AffordEffect::Relieve { need, amount } => {
                            match need {
                                Need::Sustenance => {
                                    needs.sustenance = (needs.sustenance + amount as f32).min(100.0)
                                }
                                Need::Rest => needs.rest = (needs.rest + amount as f32).min(100.0),
                            }
                            true
                        }
                        AffordEffect::Yield { good, units, skill } => {
                            if skill.is_some_and(|sk| skills.0.get(sk).is_none_or(|&v| v <= 0.0)) {
                                false // takes a calling this agent doesn't have
                            } else {
                                inv.stock[good] += units;
                                if let Some(sk) = skill {
                                    let sd = reg.skill(sk);
                                    skills.0[sk] = (skills.0[sk]
                                        + Fx::saturating_from_num(sd.gain))
                                    .min(Fx::saturating_from_num(sd.cap));
                                }
                                true
                            }
                        }
                        AffordEffect::Teach { skill } => match skills.0.get_mut(skill) {
                            // Learn the trade: lift the calling above zero (a novice),
                            // so its recipes become runnable from here on.
                            Some(s) if *s <= Fx::ZERO => {
                                *s = Fx::from_num(LEARNED_SKILL);
                                true
                            }
                            _ => false, // already has it, or no such skill
                        },
                    };
                    if applied {
                        site.uses += 1;
                        if site.capacity > 0 {
                            site.remaining -= 1.0;
                        }
                    }
                    applied
                }
                _ => false,
            },
        };

        if !done {
            plan_c.goal = None;
            plan_c.steps.clear();
        }
    }
}

/// Sustained feeling slowly settles into character: each step a mood nudges the
/// trait it `shapes` (scaled by how strongly it's felt), persistently — so a life
/// of anger hardens into vengeance, a life of joy into contentment. This is nurture
/// shaping disposition; it does not revert. Opposed traits move the other way.
/// Per-agent and order-free — runs in parallel.
pub(crate) fn mood_shapes_traits(
    mut people: Query<(&mut Personality, &Mood), With<Npc>>,
    reg: Res<Registry>,
) {
    people.par_iter_mut().for_each(|(mut p, mood)| {
        for m in 0..mood.0.len() {
            let Some((t, rate)) = reg.mood_shapes(m) else {
                continue;
            };
            let delta = mood.0[m] * rate;
            if delta > 0.0 {
                p.0[t] = (p.0[t] + delta).min(1.0);
                if let Some(o) = reg.opposes(t) {
                    p.0[o] = (p.0[o] - delta).max(0.0);
                }
            }
        }
    });
}

/// Moods fade toward rest (zero) each step at their authored rate, so a feeling
/// spiked by an event cools off over time while the underlying trait endures.
/// Per-agent and order-free — runs in parallel.
pub(crate) fn mood_decay(mut people: Query<&mut Mood, With<Npc>>, reg: Res<Registry>) {
    people.par_iter_mut().for_each(|mut mood| {
        for m in 0..mood.0.len() {
            mood.0[m] *= 1.0 - reg.mood_def(m).decay;
        }
    });
}

/// Drain needs each step; remove anyone who runs out of sustenance (vacating the
/// throne if the one who starved was holding it).
#[allow(clippy::type_complexity)]
pub fn people_metabolism(
    mut commands: Commands,
    // `&Position` is read-only and present on every NPC, so adding it changes neither which entities
    // match nor the iteration order — the run stays byte-identical; it only gives a starvation a place.
    // A distant (LOD) NPC is `Dormant` on its idle ticks and skipped here, so it only drains on the
    // coarse-clock ticks it actually runs — it ages slower, but never starves while you're away.
    mut npcs: Query<(Entity, &mut Needs, &Position), (With<Npc>, Without<crate::Dormant>)>,
    cfg: Res<NeedsRes>,
    mut throne: Option<ResMut<Throne>>,
    // The tick (for stamping a death) and the off-by-default Chronicle that records it. `Res<Substrate>`
    // is read-only here; `None` chronicle => the tap is a no-op and the world is unchanged.
    substrate: Res<Substrate>,
    mut chronicle: Option<ResMut<crate::chronicle::Chronicle>>,
) {
    let tick = substrate.0.tick();
    for (entity, mut needs, pos) in &mut npcs {
        needs.sustenance -= cfg.hunger_rate;
        needs.rest -= cfg.fatigue_rate;
        if needs.sustenance <= 0.0 {
            if let Some(t) = throne.as_deref_mut()
                && t.holder == Some(entity)
            {
                t.holder = None;
            }
            // An unattributed death (starvation) — `parties[0]` the dead, before despawn.
            if let Some(c) = chronicle.as_deref_mut() {
                c.record(
                    tick,
                    EpisodeKind::Death,
                    [Some(entity), None, None],
                    pos.0,
                    None,
                    0,
                );
            }
            commands.entity(entity).despawn();
        }
    }
}

// --- Spawning ---

/// A newborn's skill vector — their **calling**. Their trade(s) are seeded to
/// `initial` and every other skill left at `0` (untaught, so they can never run its
/// recipes). A round-robin primary calling (`n % skill_count`) guarantees every
/// trade is represented across the population — so there are always farmers to feed
/// the bakers — and any extra callings (when `per_agent > 1`, the few who do more
/// than one thing) are drawn from `rng`. `per_agent == 0` (or ≥ the skill count)
/// makes an unspecialised generalist afforded every trade.
fn birth_skills(
    skill_count: usize,
    per_agent: usize,
    n: usize,
    initial: f32,
    rng: &mut SplitMix64,
) -> Vec<Fx> {
    let initial = Fx::saturating_from_num(initial);
    if skill_count == 0 {
        return Vec::new();
    }
    if per_agent == 0 || per_agent >= skill_count {
        return vec![initial; skill_count];
    }
    let mut sk = vec![Fx::ZERO; skill_count];
    sk[n % skill_count] = initial;
    let mut have = 1;
    let mut guard = 0;
    while have < per_agent && guard < skill_count * 4 {
        let s = rng.gen_range(skill_count);
        if sk[s] == Fx::ZERO {
            sk[s] = initial;
            have += 1;
        }
        guard += 1;
    }
    sk
}

/// Lowest fertility a farming recipe needs — where people can make a living off
/// the land. If nothing draws on fertility, any land will do (`0`).
fn workable_fertility(reg: &Registry) -> f32 {
    let min = reg
        .recipes()
        .iter()
        .filter(|r| r.resource == Some(ResourceKind::Fertility))
        .map(|r| r.min_resource)
        .fold(f32::INFINITY, f32::min);
    if min.is_finite() { min } else { 0.0 }
}

/// Place `count` markets on fertile land; return their entities and tiles.
pub fn spawn_markets(
    world: &mut World,
    substrate: &GameWorld,
    rng: &mut SplitMix64,
    reg: &Registry,
    count: usize,
    money: i64,
    stock: u32,
) -> Vec<(Entity, Coord)> {
    let topo = substrate.topology();
    let sea = substrate.params().sea_level;
    let threshold = workable_fertility(reg);
    let fertile: Vec<Coord> = topo
        .indices()
        .filter(|&i| {
            let c = topo.coord(i);
            substrate.elevation(c) >= sea && substrate.carrying_capacity(c) >= threshold
        })
        .map(|i| topo.coord(i))
        .collect();
    if fertile.is_empty() {
        return Vec::new();
    }
    (0..count)
        .map(|_| {
            let coord = fertile[rng.gen_range(fertile.len())];
            let entity = world
                .spawn((
                    Position(coord),
                    Market {
                        stock: vec![stock; reg.good_count()],
                        money,
                        price_basis: vec![stock as f32; reg.good_count()],
                    },
                ))
                .id();
            (entity, coord)
        })
        .collect()
}

/// Each market's price basis chases its real stock by `price_smoothing` per tick —
/// the lag that turns a sudden glut or run into a gradual price move instead of a
/// whipsaw, damping the synchronized boom-bust (cobweb) the price-from-stock rule
/// is otherwise prone to. Runs after trades settle, so next tick prices the new stock.
pub(crate) fn smooth_prices(mut markets: Query<&mut Market, Without<Npc>>, econ: Res<EconRes>) {
    let alpha = econ.price_smoothing.clamp(0.0, 1.0);
    for mut m in &mut markets {
        let Market {
            stock, price_basis, ..
        } = &mut *m;
        for (basis, &s) in price_basis.iter_mut().zip(stock.iter()) {
            *basis += alpha * (s as f32 - *basis);
        }
    }
}

/// Place one market on each of the given tiles — used to seat markets in
/// settlements (community features) rather than scattering them on raw fertility,
/// so the economy hangs off the world's towns. Deterministic (no RNG): the caller
/// supplies the tiles.
pub fn spawn_markets_at(
    world: &mut World,
    reg: &Registry,
    coords: &[Coord],
    money: i64,
    stock: u32,
) -> Vec<(Entity, Coord)> {
    coords
        .iter()
        .map(|&coord| {
            let entity = world
                .spawn((
                    Position(coord),
                    Market {
                        stock: vec![stock; reg.good_count()],
                        money,
                        price_basis: vec![stock as f32; reg.good_count()],
                    },
                ))
                .id();
            (entity, coord)
        })
        .collect()
}

/// Exploration: a hex an NPC stands on has its **Landmark** and **Hidden** features
/// revealed — a turn spent looking finds the lair or the buried door. **Secret**
/// features need more than presence (luck, insight, a skill check) and stay latent.
/// Discovery is world-knowledge for now, not yet tracked per agent. Draws no
/// randomness, so it never perturbs the substrate's RNG stream.
pub fn discover_features(
    mut npcs: Query<(&Position, &mut Known), With<Npc>>,
    substrate: Res<Substrate>,
    catalog: Option<Res<FeatureCatalog>>,
    mut features: Option<ResMut<Features>>,
) {
    let topo = substrate.0.topology();
    for (pos, mut known) in &mut npcs {
        let i = topo.index_of(pos.0);
        // The agent now knows this tile's features (private map)…
        known.0.insert(i);
        // …and the world records that *someone* has found them (the shared map /
        // observer view), when a catalog is present.
        if let (Some(catalog), Some(features)) = (catalog.as_deref(), features.as_deref_mut()) {
            features.discover_at_index(catalog, i, Discovery::Hidden);
        }
    }
}

/// Place `count` people on fertile tiles around the markets, each patron to one.
#[allow(clippy::too_many_arguments)]
pub fn spawn_npcs(
    world: &mut World,
    substrate: &GameWorld,
    rng: &mut SplitMix64,
    reg: &Registry,
    needs_cfg: &NeedsConfig,
    count: usize,
    markets: &[(Entity, Coord)],
    initial_money: i64,
    initial_food: u32,
    ambitious: usize,
    feuds: usize,
    vassals: usize,
    professions_per_agent: usize,
    initial_skill: f32,
    personality_seed: u64,
) {
    if count == 0 || markets.is_empty() {
        return;
    }
    let topo = substrate.topology();
    let sea = substrate.params().sea_level;
    let threshold = workable_fertility(reg);
    let catchments: Vec<Vec<Coord>> = markets
        .iter()
        .map(|&(_, coord)| {
            let mut tiles = vec![coord];
            for l in topo.neighbors(topo.index_of(coord)) {
                let c = topo.coord(l.to);
                if substrate.elevation(c) >= sea && substrate.carrying_capacity(c) >= threshold {
                    tiles.push(c);
                }
            }
            tiles
        })
        .collect();

    // A starting larder of every edible good, to bridge the spin-up before
    // production gets going.
    let mut stock = vec![0u32; reg.good_count()];
    for (g, s) in stock.iter_mut().enumerate() {
        if reg.good(g).nutrition > 0.0 {
            *s = initial_food;
        }
    }

    // Birth personalities are drawn from a *separate* RNG so adding them doesn't
    // perturb the economy's placement stream. Each agent is born near each trait's
    // baseline, varied by its spread; the first `ambitious` are seeded with a
    // strong drive for power.
    let ambition = reg.trait_id("ambition");
    let mut pers_rng = SplitMix64::new(personality_seed);
    // Callings draw from their own stream too, so endowing them perturbs neither the
    // economy's placement nor the personality stream.
    let mut prof_rng = SplitMix64::new(personality_seed ^ 0x5A17_C0DE_5A17_C0DE);

    let mut ids = Vec::with_capacity(count);
    for n in 0..count {
        let m = rng.gen_range(markets.len());
        let coord = catchments[m][rng.gen_range(catchments[m].len())];
        let mut personality: Vec<f32> = (0..reg.trait_count())
            .map(|t| {
                let d = reg.trait_def(t);
                let jitter = pers_rng.gen_range(2001) as f32 / 1000.0 - 1.0; // [-1, 1]
                (d.baseline + jitter * d.spread).clamp(0.0, 1.0)
            })
            .collect();
        if n < ambitious
            && let Some(a) = ambition
        {
            personality[a] = 1.0;
        }
        let skills = birth_skills(
            reg.skill_count(),
            professions_per_agent,
            n,
            initial_skill,
            &mut prof_rng,
        );
        let id = world
            .spawn((
                Npc,
                Position(coord),
                Needs {
                    sustenance: needs_cfg.initial_sustenance,
                    rest: needs_cfg.initial_rest,
                },
                Skills(skills),
                Inventory {
                    money: initial_money,
                    stock: stock.clone(),
                },
                Plan::default(),
                Patron(markets[m].0),
                Personality(personality),
                Mood(vec![0.0; reg.mood_count()]),
                Known::default(),
                Allegiance::default(),
                Opinion::default(),
            ))
            .id();
        ids.push(id);
    }

    // The first `feuds` people each bear a grudge against a distinct other — entity-
    // targeted goals: "see THIS one dead".
    for i in 0..feuds.min(count.saturating_sub(feuds)) {
        world.entity_mut(ids[i]).insert(Grievance(ids[i + feuds]));
    }

    // Then `vassals` people swear to the feud *victims* (the lords, `ids[feuds..]`):
    // vassal `ids[2*feuds + j]` serves lord `ids[feuds + j]`. So when an aggressor
    // strikes a lord down, that lord's vassal inherits the quarrel — a chain of
    // vengeance the duty norm drives home.
    for j in 0..vassals.min(count.saturating_sub(2 * feuds)) {
        world
            .entity_mut(ids[2 * feuds + j])
            .insert(Liege(ids[feuds + j]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_basis_lags_stock() {
        // The basis is an EMA: each tick it closes a `price_smoothing` fraction of the
        // gap to real stock, so a sudden glut/run moves the price gradually, not in a
        // whipsaw — converging without overshoot.
        let mut world = World::new();
        world.insert_resource(EconRes(EconConfig::default())); // smoothing 0.15
        let e = world
            .spawn(Market {
                stock: vec![100],
                money: 0,
                price_basis: vec![0.0],
            })
            .id();
        let mut sched = Schedule::default();
        sched.add_systems(smooth_prices);

        sched.run(&mut world);
        let basis = world.get::<Market>(e).unwrap().price_basis[0];
        assert!(
            (basis - 15.0).abs() < 1e-3,
            "0.15 of the way from 0 to 100 is 15, got {basis}"
        );

        for _ in 0..300 {
            sched.run(&mut world);
        }
        let basis = world.get::<Market>(e).unwrap().price_basis[0];
        assert!(
            basis <= 100.0 && (basis - 100.0).abs() < 0.5,
            "should converge to stock without overshoot, got {basis}"
        );
    }
}
