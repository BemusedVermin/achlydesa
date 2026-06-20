//! The **agent layer**: autonomous entities living on the `game_sim` substrate,
//! built on `bevy_ecs`.
//!
//! The substrate (a struct-of-arrays field solver) is held as an ECS
//! **`Resource`**; agents are **entities**; behaviour is **systems**. Two kinds
//! of agent today: instinct-driven [`fauna`] and utility-driven [`people`] whose
//! economy is defined entirely by authored [`data`] (goods and recipes in RON).
//!
//! ## Configuration, deliberately separated
//! - **Authored data** ([`Registry`]) — goods, recipes, skills. Edit `assets/data/*.ron`.
//! - **Global knobs** ([`EconConfig`], [`NeedsConfig`], [`FaunaConfig`]) — rates
//!   and scales that apply to the whole economy, nothing instance-specific.
//! - **Scenario** ([`Setup`]) — one run: world size, seed, **warm-up**,
//!   populations, endowments. This is where per-run values live, not in the
//!   behaviour configs.
//!
//! Determinism is preserved: the schedule runs single-threaded in a fixed order,
//! and all randomness comes from a seeded [`SimRng`].

use bevy_ecs::prelude::*;
use game_sim::{SplitMix64, World as GameWorld};
use sim::Substrate as SubstrateTrait;

// The foundational simulation lives in `agent_core`; this `agents` crate is the thin
// assembler on top of it (and the feature crates). Re-export the whole core surface so
// existing users — `app`, the examples, the tests — keep importing everything from
// `agents` unchanged.
pub use agent_core::*;
// The RPG layer (WWN attributes/skills/foci/edges) is its own crate; re-export its character
// types so `app`, the demos and the tests reach them through `agents`. The `rpg::check` engine
// and `rpg::wwn_mod` are used internally — depend on `rpg` directly to call them.
pub use rpg::{Abilities, Archetype, CheckOutcome, FociHeld, Flags, PowerTier, Proficiencies, Rolled, RpgData, Save};
// The party layer (recruited companions that travel with the avatar) is its own crate.
pub use party::{Party, PartyConfig, PartyMember};
// The survival layer (per-tile, per-day vital drain on every body) is its own crate.
pub use survival::{SurvivalConfig, Vitals};
// The exploration layer (roads, gear, weighted/cost-paced travel) — `explore` over the pure `travel`.
pub use explore::{ExploreConfig, Gear, Roads};

/// Seed for the RPG layer's dedicated RNG stream, kept so the avatar can be rolled from it in
/// [`Simulation::spawn_player`] (after the NPCs were rolled at construction). Present as a
/// resource only when the RPG layer is enabled.
#[derive(Resource, Clone, Copy)]
struct RpgSeed(u64);

/// Proficiency a freshly-apprenticed calling starts at for the avatar — a novice, mirroring
/// `people::LEARNED_SKILL` (private there), grown by working `Yield` sites.
const NOVICE_SKILL: f32 = 0.25;

/// The short verb phrase the Use action offers for an affordance, by its effect — generic by kind
/// (the site carries only its effect, not the authored action word), which the feature's own name in
/// the inspect read-out already colours ("the hot springs", "the oasis").
fn affordance_verb(effect: agent_core::AffordEffect) -> String {
    use agent_core::{AffordEffect, Need};
    match effect {
        AffordEffect::Relieve { need: Need::Rest, .. } => "rest here".into(),
        AffordEffect::Relieve { need: Need::Sustenance, .. } => "tend yourself".into(),
        AffordEffect::Yield { .. } => "work the place".into(),
        AffordEffect::Teach { .. } => "watch the craft".into(),
    }
}

/// A plain-language read of what an NPC pursuing `goal` is *doing*, for the inspect read-out — so a
/// place reads as peopled and alive rather than as a bare list of names. Unknown goals fall through
/// to a neutral phrase, so authoring a new goal never breaks the surface.
fn activity_phrase(goal: &str) -> &'static str {
    match goal {
        "sustained" => "looking for food",
        "rested" => "seeking rest",
        "stocked" => "laying in stores",
        "solvent" => "chasing coin",
        "rule" => "reaching for power",
        "avenge" => "nursing a grudge",
        _ => "about its business",
    }
}

/// Wrapped Chebyshev hex distance (the world wraps east–west) — the cheap proximity gossip fidelity
/// reads. Mirrors `agent_core::director`'s private `hex_dist`.
fn wrapped_dist(a: Coord, b: Coord, width: i32) -> i32 {
    let drow = (a.row - b.row).abs();
    let dcol = {
        let d = (a.col - b.col).abs();
        d.min(width - d)
    };
    drow.max(dcol)
}

/// A rumour's **fidelity** in `0..1` from how far (`dist` hexes) and how long ago (`age` ticks) the
/// beat fell — the veil's dial. Falls to 0 beyond gossip-range or once it has gone stale, so such
/// events are not heard at all. Pure arithmetic, so it is deterministic.
fn gossip_fidelity(dist: i32, age: u64) -> f32 {
    const RANGE: f32 = 40.0;
    const MAX_AGE: f32 = 90.0;
    let near = (1.0 - dist as f32 / RANGE).clamp(0.0, 1.0);
    let fresh = (1.0 - age as f32 / MAX_AGE).clamp(0.0, 1.0);
    near * fresh
}

/// A rough compass bearing from `from` to `to` (wrapped east–west), or "nearby" when close — the
/// vague "somewhere to the east" a low-fidelity rumour carries.
fn compass_dir(from: Coord, to: Coord, width: i32) -> &'static str {
    let drow = to.row - from.row;
    let dcol = {
        let r = to.col - from.col;
        if r > width / 2 {
            r - width
        } else if r < -width / 2 {
            r + width
        } else {
            r
        }
    };
    if drow.abs() <= 2 && dcol.abs() <= 2 {
        return "nearby";
    }
    if dcol.abs() >= drow.abs() {
        if dcol > 0 { "to the east" } else { "to the west" }
    } else if drow > 0 {
        "to the south"
    } else {
        "to the north"
    }
}

/// The plain noun a rumour gives a beat's register ("a betrayal", "a reckoning") — what the world
/// calls the drama when it does not know the particulars.
fn rumor_noun(register: agent_core::Register) -> &'static str {
    use agent_core::Register::*;
    match register {
        Betrayal => "betrayal",
        Vengeance => "reckoning",
        Ambition => "grasping after power",
        War => "war",
        Disaster => "calamity",
        Persecution => "hounding",
        Romance => "love-match",
        Triumph => "triumph",
        Wonder => "wonder",
        _ => "strange turn",
    }
}

/// Render the overheard line, sharpening or blurring by `fid` (the veil): high fidelity names the
/// figures in a register-specific sentence; middling fidelity gives the lead and a bearing; low
/// fidelity is loose talk — a rumour and a direction, no names.
fn gossip_line(register: agent_core::Register, fid: f32, lead: &str, other: Option<&str>, dir: &str) -> String {
    use agent_core::Register::*;
    let noun = rumor_noun(register);
    if fid >= 0.6 {
        match (register, other) {
            (Betrayal, Some(o)) => format!("They say {lead} was betrayed by {o}."),
            (Vengeance, Some(o)) => format!("They say {lead} has sworn vengeance on {o}."),
            (Ambition, Some(o)) => format!("They say {lead} reaches for power, with {o} in the way."),
            (War, Some(o)) => format!("They say {lead}'s people have gone to war with {o}'s."),
            (Persecution, Some(o)) => format!("They say {o} hounds {lead} without mercy."),
            (Romance, Some(o)) => format!("They say {lead} has given their heart to {o}."),
            (Triumph, Some(o)) => format!("They say {lead} stands triumphant, and {o} eclipsed."),
            (Disaster, _) => format!("They say a calamity has fallen on {lead}'s house."),
            (Wonder, _) => format!("They say {lead} has seen a wonder none can explain."),
            _ => format!("They say {lead} is caught up in a {noun}."),
        }
    } else if fid >= 0.3 {
        format!("Word reaches you of a {noun} {dir} — {lead}, they think, at the heart of it.")
    } else {
        format!("There's loose talk of a {noun} {dir}. Who can say.")
    }
}

// --- Scenario ---

/// Everything that defines one run. Defaults to an empty 36×26 world; fill in
/// the populations and tunables you want. Warm-up and endowments live here
/// (per-run scenario), not in the behaviour configs.
pub struct Setup {
    pub width: i32,
    pub height: i32,
    pub seed: u64,
    /// World-generation + climate/ecology tunables used by the convenience
    /// [`Simulation::new`] to generate a world. Defaults to the figment-layered
    /// `params.ron`. The app builds its own (much larger, US-scale) world via
    /// `game_sim::World::generate` and injects it with [`Simulation::from_world`], so this
    /// only shapes the small default world used by headless/test runs.
    pub params: config::Params,
    /// Substrate days to spin up before agents are introduced.
    pub warmup: u64,
    pub fauna: usize,
    /// Predators that hunt the herbivores — the top-down half of the trophic loop.
    pub carnivores: usize,
    pub npcs: usize,
    pub markets: usize,
    pub initial_money: i64,
    pub initial_food: u32,
    pub initial_market_stock: u32,
    pub market_money: i64,
    pub registry: Registry,
    pub econ: EconConfig,
    pub needs: NeedsConfig,
    pub fauna_cfg: FaunaConfig,
    /// The authored goals people pursue (target conditions + appeal).
    pub goals: Goals,
    /// The society's deontic norms — permissions, prohibitions, and obligations on
    /// the acts goals pursue. Empty by default (no taboos).
    pub norms: Norms,
    /// How agents appraise significant events into persistent trait changes.
    pub appraisals: Appraisals,
    /// Place a contested throne (at the first market's tile). With a `rule` goal
    /// authored in `goals`, ambitious people will vie to hold it.
    pub throne: bool,
    /// How many of the `npcs` are ambitious (given a strong drive for power).
    pub ambitious: usize,
    /// How many of the `npcs` bear a grudge against a distinct other (entity-
    /// targeted `avenge` goals — needs an `avenge` goal authored in `goals`).
    pub feuds: usize,
    /// How many of the `npcs` are vassals sworn to a feud victim (a lord). When their
    /// lord is slain they inherit the grudge against the killer — needs an `avenge`
    /// goal, and (to move a mild vassal) an `Obliged` avenge norm in `norms`.
    pub vassals: usize,
    /// How many trades each person is born to — their **calling**. `1` (the default)
    /// makes specialists: a farmer who *cannot* bake and must trade for bread, and a
    /// baker who must buy grain. A round-robin keeps every trade covered; raise this
    /// for "the few who do more than one thing". `0` (or ≥ the skill count) makes
    /// everyone an unspecialised generalist afforded every trade.
    pub professions_per_agent: usize,
    /// Starting proficiency in each calling a person is born with (`0` in every
    /// trade they were not born to, which they can never practise).
    pub initial_skill: f32,
    /// The catalog of tile features (settlements, courts, ruins, wonders) layered
    /// onto the land. Always placed; query via [`Simulation::features`].
    pub features: FeatureCatalog,
    /// Knobs for feature placement (density, settlement spacing, remoteness scale).
    pub feature_cfg: FeatureConfig,
    /// Knobs for the faction turn (period, minimum size, recruiting reach).
    pub faction_cfg: FactionConfig,
    /// Seat the economy's markets in the world's settlements (community features)
    /// instead of scattering them on raw fertility, and put any throne in a court.
    /// Off by default so a bare economy run is unchanged.
    pub markets_on_settlements: bool,
    /// Wake the hidden **narrative director** `Γ` (`docs/narrative_director.md`): tag
    /// the first NPC as the [`Protagonist`] it stages drama for, and let it sense the
    /// stage and fire its levers. Off by default, so a director-free world is
    /// byte-identical to one before this layer existed. The director's knobs (and an
    /// alternative way to enable it) live in [`Setup::director_cfg`].
    pub director: bool,
    /// Knobs for the narrative director (stage radius, lull patience, lever sizes,
    /// the grief weights of the moral arithmetic). Its `enabled` is OR'd with
    /// [`Setup::director`].
    pub director_cfg: DirectorConfig,
    /// Wake the **emergent dialogue** layer (`docs/dialogue.md`): co-located NPCs speak
    /// when they have something worth saying, the words composed from their state. Off by
    /// default, so a dialogue-free world is byte-identical. Knobs live in
    /// [`Setup::dialogue_cfg`]; its `enabled` is OR'd with this switch.
    pub dialogue: bool,
    pub dialogue_cfg: DialogueConfig,
    /// Wake the **RPG layer** (WWN attributes/skills/foci/edges): stamp every NPC — and the
    /// avatar, when spawned — with rolled stats from a dedicated seeded stream. Off by
    /// default, so a world without it is byte-identical (no components, no resource, no
    /// stream drawn). The social/world-interaction skills it adds are read by later layers.
    pub rpg: bool,
    /// Wake the **party layer**: let the avatar recruit NPCs (via [`Simulation::player_recruit`])
    /// into a roster that travels with it as a stack. Needs the RPG layer (the recruit check
    /// reads the avatar's Convince/Lead). Off by default → byte-identical.
    pub party: bool,
    /// Recruitment knobs (base difficulty, disposition weight, size cap).
    pub party_cfg: PartyConfig,
    /// Wake the **survival layer**: every body (NPCs and the avatar) carries `Vitals` (thirst,
    /// warmth, stamina) drained per day by the tile it stands on, and grazing yields only what
    /// the tile bears. Constitution + the Survive skill + gear blunt the drain. Off by default →
    /// byte-identical. Needs the RPG layer for the skill/stat mitigation (works without it, unblunted).
    pub survival: bool,
    /// Survival drain/mitigation knobs.
    pub survival_cfg: SurvivalConfig,
    /// Whether survival applies to **every** body (the original design — also makes NPC hunger
    /// tile-dependent) or only the **avatar + party**. Default `true`; the app sets `false` so the
    /// populated world doesn't depopulate before NPCs can seek water/shelter (deferred survival-AI).
    pub survival_everyone: bool,
    /// Wake the **exploration layer**: lay a road network between settlements, price travel by
    /// terrain and slope (a road hex is a fraction of a day, a mountain hex several days), and
    /// gate steep edges (climbing gear + a proficient share of the party) and deep water (a boat).
    /// Off by default → byte-identical (BFS travel, one hex per tick).
    pub exploration: bool,
    /// Exploration cost-model + climbing-gate knobs.
    pub explore_cfg: ExploreConfig,
    /// **Level-of-detail radius** (hexes from the avatar). When `Some(r)`, NPCs within `r` of the
    /// avatar are simulated in full every tick; farther ones run on a *coarse clock* — one tick in
    /// [`Setup::sim_far_stride`], staggered — so they keep living, just slower, at ~1/stride the cost.
    /// This keeps movement smooth in a heavily peopled world. The director/factions/mood still see
    /// every soul each tick, so drama is unaffected. `None` (the default) simulates every soul every
    /// tick, byte-identical to before. Needs the player avatar (no effect on a player-less run).
    pub sim_radius: Option<i32>,
    /// Ticks between updates for a *distant* NPC under [`Setup::sim_radius`] — its coarse-clock
    /// stride. `1` = no coarsening (distant NPCs still run every tick). Ignored when `sim_radius` is
    /// `None`. Default `8`.
    pub sim_far_stride: u32,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            width: 36,
            height: 26,
            seed: 0,
            params: config::tunables::params(),
            warmup: 300,
            fauna: 0,
            carnivores: 0,
            npcs: 0,
            markets: 6,
            initial_money: 100,
            initial_food: 3,
            initial_market_stock: 25,
            market_money: 80_000,
            registry: Registry::bundled(),
            econ: config::tunables::econ(),
            needs: config::tunables::needs(),
            fauna_cfg: config::tunables::fauna(),
            goals: Goals::default(),
            norms: Norms::default(),
            appraisals: Appraisals::default(),
            throne: false,
            ambitious: 0,
            feuds: 0,
            vassals: 0,
            professions_per_agent: 1,
            initial_skill: 0.5,
            features: FeatureCatalog::default(),
            feature_cfg: config::tunables::feature(),
            faction_cfg: config::tunables::faction(),
            markets_on_settlements: false,
            director: false,
            director_cfg: config::tunables::director(),
            dialogue: false,
            dialogue_cfg: config::tunables::dialogue(),
            rpg: false,
            party: false,
            party_cfg: PartyConfig::default(),
            survival: false,
            survival_cfg: SurvivalConfig::default(),
            survival_everyone: true,
            exploration: false,
            explore_cfg: ExploreConfig::default(),
            sim_radius: None,
            sim_far_stride: 8,
        }
    }
}

// --- Driver ---

/// The agent simulation: a `bevy_ecs` world plus the per-step schedule.
pub struct Simulation {
    world: World,
    schedule: Schedule,
}

impl Simulation {
    /// Build a run from a [`Setup`] — the headless/test convenience. Generates a world
    /// from the `Setup`'s world knobs (`width`/`height`/`params`/`seed`), then hands it to
    /// [`Self::from_world`]. The **app owns world generation itself** and calls `from_world`
    /// directly; the terrain generator lives in [`game_sim`](game_sim::World::generate),
    /// never here — this crate only *drives* a substrate, it does not author one.
    pub fn new(setup: Setup) -> Self {
        let world = GameWorld::generate(setup.width, setup.height, setup.params.clone(), setup.seed);
        Self::from_world(world, setup)
    }

    /// Build a run on an **already-generated** substrate that the caller owns. Warms the
    /// climate (`Setup::warmup`) and introduces the population; the world's dimensions come
    /// from `world` itself, so `Setup::{width,height,params}` are ignored here (they only
    /// drive the convenience [`Self::new`]). `Setup::seed` still seeds every agent-layer
    /// RNG stream — pass the same seed the world was generated with for a reproducible run.
    pub fn from_world(world: GameWorld, setup: Setup) -> Self {
        // The compute pool backs `people_plan`'s parallel planning. Idempotent, so
        // it's safe to call for every simulation built in the process. Planning
        // writes only each person's own `Plan` from read-only shared state, so the
        // result is identical regardless of how work is split across threads.
        bevy_tasks::ComputeTaskPool::get_or_init(bevy_tasks::TaskPool::default);

        let mut substrate = world;
        let mut rng = SplitMix64::new(setup.seed ^ 0x9E37_79B9_7F4A_7C15);
        for _ in 0..setup.warmup {
            substrate.evolve(&mut rng);
        }

        // Layer the world's features onto the warmed substrate, from a dedicated RNG
        // stream so placement never perturbs the economy's (fauna/market/NPC) stream.
        let mut feat_rng = SplitMix64::new(setup.seed ^ 0xF0A7_FEA7_57E5_0001);
        let features = features::place(&substrate, &setup.features, &setup.feature_cfg, &mut feat_rng);
        // The smart-object layer: resolve each feature's advertised affordances into
        // live sites the planner can route to and execution can deplete.
        let affordances = people::build_affordances(&setup.features, &features, &setup.registry, substrate.topology());

        let mut world = World::new();
        // The creature roster — which species live which biomes. Static content, so
        // built once and shared between placement and the running fauna systems.
        let bestiary = fauna::Bestiary::bundled();
        fauna::spawn_fauna(&mut world, &substrate, &bestiary, &mut rng, setup.fauna, setup.fauna_cfg.initial_energy);
        fauna::spawn_carnivores(
            &mut world,
            &substrate,
            &bestiary,
            &mut rng,
            setup.carnivores,
            setup.fauna_cfg.carn_initial_energy,
        );
        let markets = if setup.npcs == 0 {
            Vec::new()
        } else if setup.markets_on_settlements {
            // Seat one market in each settlement (community), up to `markets`.
            let tiles: Vec<Coord> = features
                .tiles_of(&setup.features, Category::Community, substrate.topology())
                .into_iter()
                .take(setup.markets)
                .collect();
            people::spawn_markets_at(&mut world, &setup.registry, &tiles, setup.market_money, setup.initial_market_stock)
        } else {
            people::spawn_markets(
                &mut world,
                &substrate,
                &mut rng,
                &setup.registry,
                setup.markets,
                setup.market_money,
                setup.initial_market_stock,
            )
        };
        people::spawn_npcs(
            &mut world,
            &substrate,
            &mut rng,
            &setup.registry,
            &setup.needs,
            setup.npcs,
            &markets,
            setup.initial_money,
            setup.initial_food,
            setup.ambitious,
            setup.feuds,
            setup.vassals,
            setup.professions_per_agent,
            setup.initial_skill,
            setup.seed ^ 0x5EED_0FC0,
        );
        // A throne is one shared world fact — seat it in a court if the world has
        // one (a seat of rule), else on a market tile (a hub contenders pass
        // through). Either way it must be reachable.
        if setup.throne {
            let court = setup.markets_on_settlements
                .then(|| features.tiles_of(&setup.features, Category::Court, substrate.topology()).first().copied())
                .flatten();
            if let Some(tile) = court.or_else(|| markets.first().map(|&(_, t)| t)) {
                world.insert_resource(people::Throne { tile, holder: None });
            }
        }

        // Wake the RPG layer, if asked: stamp every NPC with rolled WWN stats from a
        // dedicated seeded stream, drawn *after* every spawn above so it never perturbs their
        // RNG. The avatar is rolled later (in `spawn_player`) from the same seed. Off → nothing
        // is drawn, no resource inserted, no component added, so the world is byte-identical.
        if setup.rpg {
            let data = rpg::RpgData::bundled();
            let rpg_seed = setup.seed ^ 0x2790_F00D_0FF1_CE00;
            let mut rpg_rng = SplitMix64::new(rpg_seed);
            let npcs: Vec<Entity> = {
                let mut q = world.query_filtered::<Entity, With<people::Npc>>();
                q.iter(&world).collect()
            };
            for e in npcs {
                let r = rpg::roll(&mut rpg_rng, &data);
                world
                    .entity_mut(e)
                    .insert((r.abilities, r.proficiencies, r.foci, r.flags, r.power, rpg::Archetype(r.edge)));
            }
            world.insert_resource(RpgSeed(rpg_seed));
            world.insert_resource(data);
        }

        // Wake the party layer, if asked: insert the empty roster and its knobs so the avatar
        // can recruit. Off → no resources, the Suspended/Follower seams are never set, and a
        // partyless world is byte-identical.
        if setup.party {
            world.insert_resource(party::Party::default());
            world.insert_resource(setup.party_cfg);
        }

        // Wake the survival layer, if asked: every NPC carries Vitals, grazing becomes
        // tile-dependent (the HungerModel seam), and the per-day drain system is added to the
        // schedule below. Off → no Vitals, no resources, flat hunger — byte-identical.
        if setup.survival {
            world.insert_resource(setup.survival_cfg);
            // "Everyone" survival also makes NPC hunger tile-dependent and gives every NPC `Vitals`.
            // The party-scoped variant leaves NPCs entirely untouched (flat hunger, no Vitals) — only
            // the avatar (in `spawn_player`) and recruited companions face the drain — so a populated
            // world doesn't thin out before NPCs can seek water/shelter on their own.
            if setup.survival_everyone {
                world.insert_resource(agent_core::HungerModel::TileBiomass);
                let npcs: Vec<Entity> = {
                    let mut q = world.query_filtered::<Entity, With<people::Npc>>();
                    q.iter(&world).collect()
                };
                for e in npcs {
                    world.entity_mut(e).insert(survival::Vitals::default());
                }
            }
        }

        // Wake the exploration layer, if asked: lay a road network between the settlements and
        // build the per-tile travel cost field from it. Off → no Roads/TravelCost, so the avatar
        // travels BFS at one hex per tick (byte-identical).
        if setup.exploration {
            let hubs = features.tiles_of(&setup.features, Category::Community, substrate.topology());
            let roads = travel::build_roads(&substrate, &setup.explore_cfg.cost, &hubs);
            let cost = travel::cost_field(&substrate, &setup.explore_cfg.cost, &|i| roads.contains(&i));
            world.insert_resource(explore::Roads(roads));
            world.insert_resource(setup.explore_cfg);
            world.insert_resource(TravelCost(cost));
        }

        // Wake the narrative director, if asked. Its `enabled` is the OR of the
        // convenience switch and the config's own flag. When enabled, the first NPC
        // becomes the protagonist it stages drama for; when not, the resources are
        // still inserted but the system early-returns, so the run is unchanged.
        let mut director_cfg = setup.director_cfg;
        director_cfg.enabled = setup.director || director_cfg.enabled;
        let beat_book = if director_cfg.enabled {
            let mut q = world.query_filtered::<Entity, With<people::Npc>>();
            if let Some(protagonist) = q.iter(&world).next() {
                world.entity_mut(protagonist).insert(director::Protagonist);
            }
            beats::BeatBook::bundled()
        } else {
            beats::BeatBook::default()
        };
        world.insert_resource(director::DirectorRes(director_cfg));
        world.insert_resource(beat_book);
        // The director draws its variety from a dedicated, seeded stream so a story is
        // deterministic yet not the same every beat.
        world.insert_resource(director::Director::seeded(setup.seed ^ 0xD1EC_7012_0F00_0001));

        // Wake the dialogue layer, if asked. Like the director, its `enabled` is the OR of
        // the convenience switch and the config's own flag; its state lives in one resource
        // (no NPC component), so a dialogue-free world is byte-identical. Its variety draws
        // from its own dedicated seeded stream.
        let mut dialogue_cfg = setup.dialogue_cfg;
        dialogue_cfg.enabled = setup.dialogue || dialogue_cfg.enabled;
        let intents = if dialogue_cfg.enabled { dialogue::IntentBook::bundled() } else { dialogue::IntentBook::default() };
        world.insert_resource(dialogue::DialogueRes(dialogue_cfg));
        world.insert_resource(intents);
        world.insert_resource(dialogue::Dialogue::seeded(setup.seed ^ 0xD1A1_706E_0FF1_CE00));
        // Gossip of the director's beats spreads between co-located souls (`gossip_spread`). Always
        // present (the schedule reads it), but empty until the director seeds the first rumour — so a
        // director-free world never gossips and stays byte-identical. No RNG: the spread is arithmetic.
        world.insert_resource(agent_core::Gossip::default());

        // The player avatar's state — empty (no avatar) until `spawn_player` is called, so
        // a world with no player is byte-identical (the travel system early-returns).
        world.insert_resource(player::PlayerState::default());
        // What the player knows (lore facts, rumours) — empty until an avatar goes looking.
        world.insert_resource(player::PlayerKnowledge::default());

        // The land-movement graph is fixed by the terrain (elevation never changes),
        // so build it once here and share it — never rebuilt during planning.
        world.insert_resource(people::MoveGraph::build(&substrate));
        world.insert_resource(features);
        world.insert_resource(affordances);
        world.insert_resource(setup.features);
        world.insert_resource(Substrate(substrate));
        world.insert_resource(SimRng(rng));
        // Predation draws from its own stream (seeded off the run seed, not the main
        // RNG) so a predator-free world is unchanged and the substrate is unperturbed.
        world.insert_resource(fauna::FaunaRng(SplitMix64::new(setup.seed ^ 0xCA12_0FF5_0FF1_CE00)));
        world.insert_resource(factions::Factions::default());
        world.insert_resource(factions::FactionRes(setup.faction_cfg));
        world.insert_resource(fauna::FaunaRes(setup.fauna_cfg));
        world.insert_resource(bestiary);
        world.insert_resource(setup.registry);
        world.insert_resource(people::EconRes(setup.econ));
        world.insert_resource(people::NeedsRes(setup.needs));
        world.insert_resource(setup.goals);
        world.insert_resource(setup.norms);
        world.insert_resource(setup.appraisals);
        world.insert_resource(events::EventQueue::default());
        // The level-of-detail config the `lod_dormancy` system reads (radius None → full detail).
        world.insert_resource(agent_core::SimRadius { radius: setup.sim_radius, far_stride: setup.sim_far_stride });

        // The fixed-order, single-threaded per-step schedule is owned by `agent_core`; the
        // survival layer (when on) adds its per-day drain just before the core metabolism.
        let mut schedule = agent_core::build_schedule();
        if setup.survival {
            schedule.add_systems(survival::survival_metabolism.before(agent_core::people::people_metabolism));
        }

        Self { world, schedule }
    }

    pub fn step(&mut self) {
        self.schedule.run(&mut self.world);
    }

    pub fn run(&mut self, steps: u64) {
        for _ in 0..steps {
            self.step();
        }
    }

    pub fn substrate(&self) -> &GameWorld {
        &self.world.resource::<Substrate>().0
    }

    /// The authored game data (goods, recipes, skills, …) this run uses.
    pub fn registry(&self) -> &Registry {
        self.world.resource::<Registry>()
    }

    pub fn tick(&self) -> u64 {
        self.substrate().tick()
    }

    /// The tile features placed on this world (settlements, courts, ruins, wonders).
    pub fn features(&self) -> &Features {
        self.world.resource::<Features>()
    }

    /// The catalog naming and describing the feature kinds.
    pub fn feature_catalog(&self) -> &FeatureCatalog {
        self.world.resource::<FeatureCatalog>()
    }

    /// The power blocs (factions) currently formed around courts.
    pub fn factions(&self) -> &[Faction] {
        &self.world.resource::<Factions>().0
    }

    /// The hidden narrative director `Γ`, if this run woke one.
    pub fn director(&self) -> &Director {
        self.world.resource::<Director>()
    }

    /// Cumulative **gratuitous** suffering — the suffering the director has authored by
    /// telling its beats (the anguish each manipulation injects, plus deaths in a beat's
    /// wake), and *only* that. Suffering `Φ` and the world's own politics produce on
    /// their own is never charged. The moral arithmetic's running total; the liberation
    /// goal is to drive its *rate* to zero (§3.1).
    pub fn gratuitous_total(&self) -> f64 {
        self.director().gratuitous_total
    }

    /// Cumulative **staged experience** — *all* the emotional life the director has
    /// authored, joy as well as anguish (suffering weighted heaviest). The generalized
    /// metric (decision #8): the horror isn't sadness, it is *instrumentalization* — a
    /// Demiurge shaping the world to entertain *you* — so a manufactured triumph or love
    /// counts here too. The win is this → 0 (authorship, not sadness, brought to nothing);
    /// it is the system's internal truth, never a shown meter.
    pub fn director_staged_total(&self) -> f64 {
        self.director().staged_total
    }

    /// The legible **cadence** the director leaves — per fired beat, its register, arc
    /// phase, thread, the protagonist's prominence, and whether the climax was a
    /// collision. Each beat is deniable; this *pattern* (groom→climax→fall, the
    /// prominence→reversal correlation) is the only evidence it leaves, the thing a
    /// suspicious player eventually reads. *The player should feel manipulated* (§5).
    pub fn director_cadence(&self) -> &[Cadence] {
        &self.director().cadence
    }

    /// The director's running **threads** — the few interleaved stories it grooms toward
    /// their climaxes (groom→climax→fall; a betrayal/vengeance trunk + tributaries).
    pub fn director_threads(&self) -> &[Thread] {
        self.director().threads()
    }

    /// A soul's accumulated, manufacturable narrative **prominence** — how invested the
    /// audience is in them (the attachment the director grooms on purpose, then reverses).
    pub fn director_prominence(&self, e: bevy_ecs::entity::Entity) -> f32 {
        self.director().prominence_of(e)
    }

    /// The director's current read of the protagonist's dramatic tension — high when the
    /// story is in crisis, low when the world has gone quiet.
    pub fn director_tension(&self) -> f32 {
        self.director().tension_now
    }

    /// How many beats the director has told so far.
    pub fn director_beats_fired(&self) -> usize {
        self.director().log.len()
    }

    /// The story the director has told: `(tick, beat id)` in order — the beats it
    /// identified and staged for this player.
    pub fn director_log(&self) -> &[(u64, String)] {
        &self.director().log
    }

    /// How many *distinct* beats the director has told — a read on the diversity of the
    /// story (the novelty pressure keeps this climbing rather than repeating one beat).
    pub fn director_distinct_beats(&self) -> usize {
        self.director().log.iter().map(|(_, id)| id.as_str()).collect::<std::collections::HashSet<_>>().len()
    }

    /// Turn the director on or off — a *scenario* switch, not a player verb. There is
    /// deliberately **no** "disarm the director" tool: the only way to quiet it is to
    /// bring the world to a state it can find no drama in (see [`Self::director_tension`]
    /// and the trait readouts) — through the same ordinary life every NPC lives. The
    /// freedom is a property of the world, never a button.
    pub fn set_director_enabled(&mut self, on: bool) {
        self.world.resource_mut::<director::DirectorRes>().0.enabled = on;
    }

    /// Mean of a named trait across the living population — the **traction readout**: the
    /// world's drift on the axes the director feeds on (ambition fuels rivalry & coups,
    /// vengeance fuels the knife). A society that has grown contented and forgiving has
    /// starved the director of leverage, and its story falls quiet.
    pub fn mean_trait(&mut self, name: &str) -> f32 {
        let Some(id) = self.world.resource::<Registry>().trait_id(name) else {
            return 0.0;
        };
        let mut q = self.world.query_filtered::<&Personality, With<Npc>>();
        let (sum, n) = q.iter(&self.world).filter_map(|p| p.0.get(id).copied()).fold((0.0f32, 0u32), |(s, n), v| (s + v, n + 1));
        if n == 0 { 0.0 } else { sum / n as f32 }
    }

    /// The entity the director stages its drama for, if a protagonist is tagged and alive.
    pub fn protagonist(&mut self) -> Option<bevy_ecs::entity::Entity> {
        let mut q = self.world.query_filtered::<bevy_ecs::entity::Entity, With<Protagonist>>();
        q.iter(&self.world).next()
    }

    /// The conversation the world has spoken: every [`Utterance`] in order — the emergent
    /// dialogue (and the director's forced `Voice` beats). Each carries the grounded
    /// meaning *and* its deterministic grammar surface.
    pub fn dialogue_log(&self) -> &[Utterance] {
        &self.world.resource::<Dialogue>().log
    }

    /// How many lines have been spoken.
    pub fn dialogue_count(&self) -> usize {
        self.world.resource::<Dialogue>().log.len()
    }

    /// The conversational verbs the **player** may choose from — the full repertoire, in a
    /// stable order. The player is the avatar's mind (this is a role-playing game): the sim
    /// does *not* rank what the player "wants" to say (that is the NPC's path); it offers the
    /// verbs and the player chooses the meaning. So the avatar needs no traits or mood to
    /// speak. Pair each id with [`Self::player_talk`] to say it.
    pub fn player_intents(&self) -> Vec<String> {
        dialogue::repertoire(&self.world)
    }

    /// How strongly the avatar's spoken **moves** land on `listener` right now — a persuasion
    /// check (the avatar's Charisma modifier + the better of Convince/Lead, against the listener's
    /// strong-will resistance), graded `0.0` (failed — the words don't land) / `1.0` / `1.5`.
    /// Returns `1.0` (unscaled) when the RPG layer is off or the speaker carries no stats, so
    /// dialogue is byte-identical without it. This is how the prioritized **speech skills** bite:
    /// the check is `EASY` — *anyone* can be pleasant, so even a blunt avatar slowly builds rapport
    /// (a `1.0` Pass) — but a silver-tongued one lands a `1.5` Strong, shifting opinion (and so
    /// earning recruitment) far faster, while a *strong-willed* listener can resist the inarticulate.
    pub fn speech_strength(&self, speaker: bevy_ecs::entity::Entity, listener: bevy_ecs::entity::Entity) -> f32 {
        let (Some(ab), Some(pr), Some(data)) = (
            self.world.get::<Abilities>(speaker),
            self.world.get::<Proficiencies>(speaker),
            self.world.get_resource::<RpgData>(),
        ) else {
            return 1.0;
        };
        let rank = |name: &str| data.skill_id(name).map(|i| pr.rank(i)).unwrap_or(rpg::PROF_UNSKILLED);
        let social = rank("Convince").max(rank("Lead"));
        // A strong-willed listener resists more: its best of WIS/CHA modifier lifts the difficulty.
        let resist = self
            .world
            .get::<Abilities>(listener)
            .map(|a| a.modifier(rpg::WIS).max(a.modifier(rpg::CHA)))
            .unwrap_or(0);
        rpg::check(ab.modifier(rpg::CHA), social, 0, rpg::EASY + resist).strength()
    }

    /// The **player avatar** speaks the line the player chose: enact `intent_id` from the
    /// avatar to `listener` through the same machinery as emergent speech — its moves land
    /// on the listener (scaled by [`Self::speech_strength`]), it is remembered, and the rendered
    /// [`Utterance`] is returned (and appended to [`Self::dialogue_log`]). `None` if there is no
    /// avatar or the intent is unknown. Does not advance time — see [`Self::player_talk`].
    pub fn player_say(&mut self, listener: bevy_ecs::entity::Entity, intent_id: &str) -> Option<Utterance> {
        let avatar = self.world.resource::<player::PlayerState>().avatar()?;
        let scale = self.speech_strength(avatar, listener);
        dialogue::perform_scaled(&mut self.world, avatar, listener, intent_id, scale)
    }

    /// Apply only the **social consequence** of a conversational intent from the player's avatar
    /// to `listener` — the deterministic authored moves (opinion/mood/grievance shifts) — with no
    /// surface rendered or logged. This is how a *free-text* conversation moves the world: the
    /// host classifies what the player said into an intent, then calls this. Returns `false` if
    /// there is no avatar or the intent id is unknown. Player-driven and out of the tick, so a
    /// world with no player stays byte-identical.
    pub fn apply_conversational_intent(&mut self, listener: bevy_ecs::entity::Entity, intent_id: &str) -> bool {
        let Some(avatar) = self.world.resource::<player::PlayerState>().avatar() else { return false };
        let scale = self.speech_strength(avatar, listener);
        // Clone the moves out (releasing the IntentBook borrow) before mutating the world.
        let moves = match self.world.resource::<dialogue::IntentBook>().0.iter().find(|i| i.id == intent_id) {
            Some(i) => i.moves.clone(),
            None => return false,
        };
        dialogue::apply_moves_scaled(&mut self.world, avatar, listener, &moves, scale);
        true
    }

    /// One conversational **action**: the player speaks the chosen line to `listener`, the
    /// listener answers in kind if it has anything worth saying, and the world then advances
    /// exactly one tick around them (one action = one tick, like a step or a wait). Returns
    /// the player's line and the listener's reply (if any); `None` if there is no avatar or
    /// the intent is unknown.
    pub fn player_talk(
        &mut self,
        listener: bevy_ecs::entity::Entity,
        intent_id: &str,
    ) -> Option<(Utterance, Option<Utterance>)> {
        let avatar = self.world.resource::<player::PlayerState>().avatar()?;
        let scale = self.speech_strength(avatar, listener);
        let line = dialogue::perform_scaled(&mut self.world, avatar, listener, intent_id, scale)?;
        let reply = dialogue::reply(&mut self.world, listener, avatar);
        self.step(); // a spoken exchange is an action; the world lives a moment on
        Some((line, reply))
    }

    /// **Intervene in a soul's drama** — counsel it toward peace (`calm`), or stoke its grievance.
    /// Directly, persuasion-scaled (the same speech check as a spoken line), moves the figure's drive
    /// for *vengeance* and its heat of *anger* — the very state the director's avenge/betrayal beats
    /// read — so the player can talk a vendetta down or feed it: a real lever on the threads. A
    /// player action (spends a tick); never feeds the NPC dialogue path and draws no RNG, so a
    /// player-less world is byte-identical. Returns what your words did (a failed check moves nothing).
    pub fn player_counsel(&mut self, npc: bevy_ecs::entity::Entity, calm: bool) -> Option<String> {
        let avatar = self.world.resource::<player::PlayerState>().avatar()?;
        let scale = self.speech_strength(avatar, npc);
        let name = self.display_name(npc);
        let (veng, anger) = {
            let reg = self.world.resource::<Registry>();
            (reg.trait_id("vengeance"), reg.mood_id("anger"))
        };
        let mag = 0.2 * scale * if calm { -1.0 } else { 1.0 };
        if let Some(t) = veng
            && let Some(mut p) = self.world.get_mut::<people::Personality>(npc)
            && let Some(v) = p.0.get_mut(t)
        {
            *v = (*v + mag).clamp(0.0, 1.0);
        }
        if let Some(m) = anger
            && let Some(mut mood) = self.world.get_mut::<people::Mood>(npc)
            && let Some(v) = mood.0.get_mut(m)
        {
            *v = (*v + mag).clamp(0.0, 1.0);
        }
        self.step(); // counsel is an action; the world lives a moment on
        Some(if scale <= 0.0 {
            format!("{name} will not be moved by your words.")
        } else if calm {
            format!("Your counsel cools {name}'s fury \u{2014} the reckoning loses its heat.")
        } else {
            format!("You feed {name}'s grievance \u{2014} the wound stays raw.")
        })
    }

    /// The avatar entity, if the player is in the world.
    pub fn player_avatar(&self) -> Option<bevy_ecs::entity::Entity> {
        self.world.resource::<player::PlayerState>().avatar()
    }

    /// The avatar's current sight radius — how far it reveals the map each step. Tracks the
    /// avatar's Notice skill when the RPG layer is on, else the base radius.
    pub fn player_sight(&self) -> i32 {
        self.world.resource::<player::PlayerState>().sight()
    }

    /// Whether the avatar passively spots lore-met Secret features as it travels (a keen Notice).
    pub fn player_perceptive(&self) -> bool {
        self.world.resource::<player::PlayerState>().perceptive()
    }

    /// The NPCs within the avatar's sight, nearest first — who the player could turn and
    /// speak to. Empty if there is no avatar.
    pub fn player_nearby_npcs(&mut self) -> Vec<(bevy_ecs::entity::Entity, Coord)> {
        let Some(view) = self.player_view() else { return Vec::new() };
        let here = view.pos;
        let mut v = view.nearby;
        v.sort_by_key(|(e, c)| ((c.col - here.col).abs() + (c.row - here.row).abs(), e.to_bits()));
        v
    }

    /// The stable epithet a soul is known by in conversation (matches the dialogue log).
    pub fn display_name(&self, e: bevy_ecs::entity::Entity) -> String {
        dialogue::display_name(&self.world, e)
    }

    /// Whether the soul `e` is still in the world (alive). A despawned soul fails this — used by the
    /// ledger to mark someone the avatar knew who is now gone. Read-only.
    pub fn npc_present(&self, e: bevy_ecs::entity::Entity) -> bool {
        self.world.get::<agent_core::Position>(e).is_some()
    }

    /// The arc-aware **honorific** a soul has earned in the director's live threads — "the Betrayed",
    /// "the Faithless" — or `None` for a soul not woven into a story (or the director asleep).
    /// Surface flavour for the HUD and conversation; never affects the sim.
    pub fn npc_epithet(&self, e: bevy_ecs::entity::Entity) -> Option<String> {
        self.world.get_resource::<Director>()?.epithet_of(e).map(str::to_string)
    }

    /// The soul's own most-recent **forced line** — words the director lately put in its mouth (a
    /// `Voice` beat), the manufactured drama *heard* — when one landed within the recent past.
    /// `None` otherwise. Used to open a conversation on the soul's own voice. Read-only.
    pub fn npc_voiced_line(&self, e: bevy_ecs::entity::Entity) -> Option<String> {
        let now = self.substrate().tick();
        self.dialogue_log()
            .iter()
            .rev()
            .find(|u| u.speaker == e && u.forced && now.saturating_sub(u.tick) <= 40)
            .map(|u| u.surface.clone())
    }

    /// A short, present-tense **situational** fragment naming a thread figure's plight ("still raw
    /// from a trusted friend's turning."), for a conversation to open on as narration. `None` for an
    /// ordinary soul (or the director asleep). Surface flavour; moves no state.
    pub fn npc_situation(&self, e: bevy_ecs::entity::Entity) -> Option<String> {
        self.world.get_resource::<Director>()?.situation_of(e).map(str::to_string)
    }

    /// **Word reaches you** — a line of gossip about the director's drama, heard from a soul in
    /// earshot, in *that soul's own worn copy*. Reads the sharpest rumour any nearby soul actually
    /// holds (the gossip layer, propagated soul-to-soul) and renders it by its **fidelity** — sharp
    /// (naming the figures) when the teller heard it firsthand or close to it, vague (a rumour and a
    /// bearing) when it has passed through many mouths to reach them. This is the veil with real
    /// teeth (`docs/narrative_surfacing.md` §3): the garbling is *how far the telephone game
    /// travelled*, not merely the player's distance. `None` when no one in earshot knows anything
    /// (no avatar, no gossip layer, or the locals simply haven't heard). Read-only & deterministic.
    pub fn overheard(&mut self) -> Option<String> {
        // You catch talk from a good way off, not just your own tile — a wider net than sight.
        const HEARING: i32 = 8;
        let nearby = self.souls_within(HEARING);
        if nearby.is_empty() {
            return None; // gossip needs a teller in earshot
        }
        let at = self.player_position()?;
        let width = self.substrate().topology().width();
        // The sharpest rumour anyone in earshot holds — its fidelity is *their* wear, not the player's.
        let r = {
            let gossip = self.world.get_resource::<agent_core::Gossip>()?;
            nearby
                .iter()
                .flat_map(|&e| gossip.rumors_of(e).iter().copied())
                .max_by(|a, b| a.fidelity.partial_cmp(&b.fidelity).unwrap().then(a.event_id.cmp(&b.event_id)))?
        };
        let lead = self.display_name(r.lead);
        let lead_titled = match self.npc_epithet(r.lead) {
            Some(ep) => format!("{lead}, {ep}"),
            None => lead,
        };
        let other = r.other.map(|e| self.display_name(e));
        let dir = compass_dir(at, r.place, width);
        Some(gossip_line(r.register, r.fidelity, &lead_titled, other.as_deref(), dir))
    }

    /// The NPCs within `radius` hexes of the avatar (wrapped E–W) — a wider net than sight, for what
    /// the avatar can *hear* talk from. Empty if there is no avatar.
    fn souls_within(&mut self, radius: i32) -> Vec<bevy_ecs::entity::Entity> {
        let Some(at) = self.player_position() else { return Vec::new() };
        let width = self.substrate().topology().width();
        let mut q = self.world.query_filtered::<(bevy_ecs::entity::Entity, &agent_core::Position), With<people::Npc>>();
        q.iter(&self.world).filter(|(_, p)| wrapped_dist(at, p.0, width) <= radius).map(|(e, _)| e).collect()
    }

    /// The **current narrative pulse**, pushed at the player as it moves — the world *telling* its
    /// drama instead of waiting to be asked. The loudest gossip a nearby soul holds, when there is
    /// any ("Word here — they say…"); else the unrest the avatar can sense, pointed and given a face
    /// ("Unrest stirs to the east — Allogenes, the Avenger."); else `None` (the land is quiet here).
    /// Read-only; safe to call every frame.
    pub fn tidings(&mut self) -> Option<String> {
        if let Some(g) = self.overheard() {
            return Some(format!("Word here \u{2014} {g}"));
        }
        let at = self.player_position()?;
        let width = self.substrate().topology().width();
        let strongest = self.drama_marks().into_iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())?;
        let dir = compass_dir(at, strongest.0, width);
        // Give the unrest a face: the figure the director's drama turns on, if one is staged.
        let who = match self.protagonist() {
            Some(p) => {
                let n = self.display_name(p);
                match self.npc_epithet(p) {
                    Some(ep) => format!(" \u{2014} {n}, {ep}"),
                    None => format!(" \u{2014} {n}"),
                }
            }
            None => String::new(),
        };
        Some(format!("Unrest stirs {dir}{who}."))
    }

    /// Where recent drama can be **sensed** — `(place, fidelity)` for each recent beat within
    /// gossip-range of the avatar (deduped by tile, strongest kept). The map markers that draw the
    /// player toward unrest ("a commotion to the east"); travelling there makes [`Self::overheard`]
    /// sharp by proximity. Empty with no avatar or director. Read-only; fidelity is deterministic.
    pub fn drama_marks(&self) -> Vec<(Coord, f32)> {
        let Some(at) = self.player_position() else { return Vec::new() };
        let Some(director) = self.world.get_resource::<Director>() else { return Vec::new() };
        let width = self.substrate().topology().width();
        let now = self.substrate().tick();
        let mut marks: Vec<(Coord, f32)> = Vec::new();
        for ev in director.recent_events() {
            let fid = gossip_fidelity(wrapped_dist(at, ev.place, width), now.saturating_sub(ev.tick));
            if fid <= 0.0 {
                continue;
            }
            match marks.iter_mut().find(|(c, _)| *c == ev.place) {
                Some(m) => m.1 = m.1.max(fid),
                None => marks.push((ev.place, fid)),
            }
        }
        marks
    }

    /// The souls standing on tile `c` and what each is about — "Aldric, the Betrayed — chasing coin"
    /// — so an inspected place reads as *peopled and alive*, not just terrain. Each line is the
    /// soul's name, any arc honorific, and a plain-language read of the goal it is pursuing.
    /// Read-only over the world (the query needs `&mut`, as the other inspection accessors do).
    pub fn souls_at(&mut self, c: Coord) -> Vec<String> {
        // Collect present NPCs and their current goal index (releasing the query borrow), copy the
        // goal names out, then turn each into words via the name/epithet accessors.
        let here: Vec<(bevy_ecs::entity::Entity, Option<usize>)> = {
            let mut q = self
                .world
                .query_filtered::<(bevy_ecs::entity::Entity, &agent_core::Position, &people::Plan), With<people::Npc>>();
            q.iter(&self.world).filter(|(_, p, _)| p.0 == c).map(|(e, _, plan)| (e, plan.goal)).collect()
        };
        let goal_names: Vec<String> = self.world.resource::<Goals>().0.iter().map(|g| g.name.clone()).collect();
        here.into_iter()
            .map(|(e, goal)| {
                let name = self.display_name(e);
                let titled = match self.npc_epithet(e) {
                    Some(ep) => format!("{name}, {ep}"),
                    None => name,
                };
                let doing = goal.and_then(|i| goal_names.get(i)).map(|n| activity_phrase(n)).unwrap_or("at rest");
                format!("{titled} — {doing}")
            })
            .collect()
    }

    /// What the avatar can **do** at the place it stands — the available, discovered affordances on
    /// its tile, each `(index, verb)` where the verb is a short phrase ("rest here", "tend
    /// yourself"). Empty when there is nothing to engage. Drives the Use action and its read-out;
    /// called every frame for the button gate, so it borrows rather than clones the avatar's `Known`.
    pub fn affordances_here(&self) -> Vec<(usize, String)> {
        let Some(avatar) = self.world.resource::<player::PlayerState>().avatar() else { return Vec::new() };
        let Some(at) = self.world.get::<agent_core::Position>(avatar).map(|p| p.0) else { return Vec::new() };
        let known = self.world.get::<people::Known>(avatar);
        let aff = self.world.resource::<people::WorldAffordances>();
        aff.0
            .iter()
            .enumerate()
            .filter(|(_, s)| s.at == at && s.available() && (!s.needs_discovery || known.is_some_and(|k| k.0.contains(&s.tile))))
            .map(|(i, s)| (i, affordance_verb(s.effect)))
            .collect()
    }

    /// **Use the affordance** `idx` where the avatar stands: engage the smart-object — refresh the
    /// avatar's body at a relief site, fill its satchel and grow the calling at a `Yield` site, learn
    /// a craft at a `Teach` site — the **same effects an NPC's `Step::Use` gets**. When it applies,
    /// the site is drawn down a use and the world lives a tick around the act (one action = one
    /// tick). Returns a line describing what happened (or why it could not), or `None` if the site is
    /// gone, worked out, or out of reach.
    pub fn player_use_affordance(&mut self, idx: usize) -> Option<String> {
        let avatar = self.world.resource::<player::PlayerState>().avatar()?;
        let at = self.world.get::<agent_core::Position>(avatar)?.0;
        let known = self.world.get::<people::Known>(avatar).map(|k| k.0.clone()).unwrap_or_default();
        let effect = {
            let aff = self.world.resource::<people::WorldAffordances>();
            let s = aff.0.get(idx)?;
            if s.at != at || !s.available() || (s.needs_discovery && !known.contains(&s.tile)) {
                return None;
            }
            s.effect
        };
        match self.apply_affordance_to_avatar(avatar, effect) {
            // It applied: draw the site down a use, and let the world live a tick around the act.
            Ok(outcome) => {
                if let Some(s) = self.world.resource_mut::<people::WorldAffordances>().0.get_mut(idx) {
                    s.uses += 1;
                    if s.capacity > 0 {
                        s.remaining -= 1.0;
                    }
                }
                self.step(); // engaging a place is an action; the world lives a moment on
                Some(outcome)
            }
            // It could not (a craft the avatar lacks, a calling it already holds): say why, no tick.
            Err(reason) => Some(reason),
        }
    }

    /// Apply an affordance's effect to the **avatar**, mirroring an NPC's `Step::Use`: a relief site
    /// refreshes its `Vitals` (survival); a `Yield` site fills its satchel (`Inventory`) and grows
    /// the calling (`Skills`); a `Teach` site lifts a calling above zero (a novice). `Ok` = applied
    /// (the caller depletes the site and spends a tick); `Err` = it could not (the avatar lacks the
    /// craft a yield needs, or already holds the calling a guild teaches) — the reason to show, with
    /// no tick spent.
    fn apply_affordance_to_avatar(
        &mut self,
        avatar: bevy_ecs::entity::Entity,
        effect: agent_core::AffordEffect,
    ) -> Result<String, String> {
        use agent_core::{AffordEffect, Need};
        match effect {
            AffordEffect::Relieve { need, .. } => {
                if let Some(mut v) = self.world.get_mut::<Vitals>(avatar) {
                    match need {
                        Need::Rest => {
                            v.stamina = (v.stamina + 35.0).min(100.0);
                            v.warmth = (v.warmth + 15.0).min(100.0);
                        }
                        Need::Sustenance => {
                            v.thirst = (v.thirst + 30.0).min(100.0);
                            v.stamina = (v.stamina + 10.0).min(100.0);
                        }
                    }
                }
                Ok(match need {
                    Need::Rest => "You take your rest here, and the ache eases from your limbs.".into(),
                    Need::Sustenance => "You tend your body here; water and forage ease the day's wear.".into(),
                })
            }
            AffordEffect::Yield { good, units, skill } => {
                // Resolve names/rates from the read-only registry first, then mutate the components.
                let (good_name, craft_name, rate) = {
                    let reg = self.world.resource::<Registry>();
                    let sk_name = skill.map(|sk| reg.skill(sk).name.clone());
                    let sk_rate = skill.map(|sk| {
                        let sd = reg.skill(sk);
                        (sd.gain, sd.cap)
                    });
                    (reg.good(good).name.clone(), sk_name, sk_rate)
                };
                // A craft the avatar has not learned cannot be worked here — the lure to go apprentice.
                let has_craft = match skill {
                    Some(sk) => self.world.get::<people::Skills>(avatar).is_some_and(|s| s.0.get(sk).is_some_and(|&v| v > 0.0)),
                    None => true,
                };
                if !has_craft {
                    let craft = craft_name.unwrap_or_else(|| "craft".into());
                    return Err(format!("You could work this place, but you have not learned the {craft}'s craft."));
                }
                if let Some(mut inv) = self.world.get_mut::<people::Inventory>(avatar)
                    && let Some(slot) = inv.stock.get_mut(good)
                {
                    *slot += units;
                }
                if let (Some(sk), Some((gain, cap))) = (skill, rate)
                    && let Some(mut sks) = self.world.get_mut::<people::Skills>(avatar)
                    && let Some(v) = sks.0.get_mut(sk)
                {
                    *v = (*v + gain).min(cap);
                }
                Ok(format!("You gather {units} {good_name}."))
            }
            AffordEffect::Teach { skill } => {
                let craft = self.world.resource::<Registry>().skill(skill).name.clone();
                let already =
                    self.world.get::<people::Skills>(avatar).is_some_and(|s| s.0.get(skill).is_some_and(|&v| v > 0.0));
                if already {
                    return Err(format!("You already know the {craft}'s craft."));
                }
                let mut learned = false;
                if let Some(mut sks) = self.world.get_mut::<people::Skills>(avatar)
                    && let Some(v) = sks.0.get_mut(skill)
                {
                    *v = NOVICE_SKILL;
                    learned = true;
                }
                if learned {
                    Ok(format!("You apprentice here, and learn the {craft}'s craft."))
                } else {
                    Err("There is no craft to learn here.".into())
                }
            }
        }
    }

    /// The goods in the avatar's satchel — `(name, count)` for each it carries (gathered at `Yield`
    /// sites). Empty if there is no avatar or it carries nothing. For the Inventory tab.
    pub fn player_goods(&self) -> Vec<(String, u32)> {
        let Some(avatar) = self.player_avatar() else { return Vec::new() };
        let Some(inv) = self.world.get::<people::Inventory>(avatar) else { return Vec::new() };
        let reg = self.world.resource::<Registry>();
        inv.stock.iter().enumerate().filter(|&(_, &n)| n > 0).map(|(i, &n)| (reg.good(i).name.clone(), n)).collect()
    }

    /// The **callings** the avatar has learned — `(craft, proficiency)` for each economy skill above
    /// zero (taught at a guild, grown by working). Empty if there is no avatar or it has learned
    /// none. For the Inventory tab. These are the *crafts* economy, distinct from the WWN skills.
    pub fn player_callings(&self) -> Vec<(String, f32)> {
        let Some(avatar) = self.player_avatar() else { return Vec::new() };
        let Some(sk) = self.world.get::<people::Skills>(avatar) else { return Vec::new() };
        let reg = self.world.resource::<Registry>();
        sk.0.iter().enumerate().filter(|&(_, &v)| v > 0.0).map(|(i, &v)| (reg.skill(i).name.clone(), v)).collect()
    }

    // --- Exploration: the player avatar (an ordinary body in the world) ---

    /// Place the player's avatar in the world (at `at`, or a sensible land start if
    /// `None`) and reveal its surroundings. Returns the avatar entity. Calling it again
    /// re-homes the player. Until called, the world runs with no player and is unchanged.
    pub fn spawn_player(&mut self, at: Option<Coord>) -> bevy_ecs::entity::Entity {
        let start = at.unwrap_or_else(|| self.default_start());
        let avatar = player::spawn(&mut self.world, start);
        // If the RPG layer is awake, roll the avatar's WWN stats too — from a dedicated
        // sub-stream of the RPG seed, so re-homing yields the same character and the roll is
        // independent of the NPCs'. The avatar gets capabilities (stats/skills), never a mind.
        if let Some(&RpgSeed(s)) = self.world.get_resource::<RpgSeed>() {
            let mut rng = SplitMix64::new(s ^ 0xA7A7_0FF1_CE00_0A7A);
            let rolled = {
                let data = self.world.resource::<RpgData>();
                rpg::roll(&mut rng, data)
            };
            self.world.entity_mut(avatar).insert((
                rolled.abilities,
                rolled.proficiencies,
                rolled.foci,
                rolled.flags,
                rolled.power,
                rpg::Archetype(rolled.edge),
            ));
            // World-interaction skill: a keener Notice reveals more of the map each step, and a
            // trained scout (Notice ≥ 2) passively spots the Secrets it already knows to look for.
            let notice = self.proficiency_of(avatar, "Notice").unwrap_or(rpg::PROF_UNSKILLED) as i32;
            {
                let mut st = self.world.resource_mut::<player::PlayerState>();
                st.set_sight(3 + notice);
                st.set_perceptive(notice >= 2);
            }
        }
        // The avatar is a body in the world too: give it Vitals when the survival layer is on.
        if self.world.get_resource::<survival::SurvivalConfig>().is_some() {
            self.world.entity_mut(avatar).insert(survival::Vitals::default());
        }
        // Exploration on: the avatar can carry gear (climbing gear, a boat, …) — start empty.
        if self.world.get_resource::<ExploreConfig>().is_some() {
            self.world.entity_mut(avatar).insert(Gear::default());
        }
        // A satchel and a blank set of callings, so the avatar can join the **crafts economy**: it
        // learns a trade by apprenticing at a guild (a `Teach` affordance) then gathers goods at a
        // `Yield` site — the same effects an NPC gets, applied by `player_use_affordance`. Inert to
        // the NPC-gated economy systems (the avatar is no `Npc`), so it never auto-trades or gets
        // planned; only the Use verb touches these. Starts empty (no money minted, no goods).
        {
            let (n_goods, n_skills) = {
                let reg = self.world.resource::<Registry>();
                (reg.good_count(), reg.skill_count())
            };
            if n_goods > 0 {
                self.world
                    .entity_mut(avatar)
                    .insert((people::Inventory { money: 0, stock: vec![0; n_goods] }, people::Skills(vec![0.0; n_skills])));
            }
        }
        avatar
    }

    /// A reasonable place to drop a fresh avatar: where the people already are (so it is on
    /// reachable, lived-in land), else the first land tile.
    fn default_start(&mut self) -> Coord {
        let npc = {
            let mut q = self.world.query_filtered::<&Position, With<people::Npc>>();
            q.iter(&self.world).next().map(|p| p.0)
        };
        npc.unwrap_or_else(|| {
            let gw = &self.world.resource::<Substrate>().0;
            let (topo, sea) = (gw.topology(), gw.params().sea_level);
            (0..topo.len()).map(|i| topo.coord(i)).find(|&c| gw.elevation(c) >= sea).unwrap_or(Coord::new(0, 0))
        })
    }

    /// Order the avatar to walk to `to`, auto-routing over land. It then advances along the route
    /// as the world ticks ([`Self::step`]/[`Self::run`]). With the exploration layer on the route
    /// is **weighted** — it prefers roads, avoids the steepest ground, and won't cross an edge the
    /// party can't (a climb without gear + a proficient share, deep water without a boat); cost
    /// then paces the walk (roads several hexes a day, mountains several days a hex). Returns
    /// `false` if there is no avatar or `to` is unreachable with the party's capabilities.
    pub fn player_travel_to(&mut self, to: Coord) -> bool {
        let Some(avatar) = self.world.resource::<player::PlayerState>().avatar() else { return false };
        let Some(from) = self.world.get::<Position>(avatar).map(|p| p.0) else { return false };
        let path = if let Some(cfg) = self.world.get_resource::<ExploreConfig>().copied() {
            let caps = self.party_caps(cfg);
            let roads = self.world.get_resource::<Roads>();
            let is_road = |i: usize| roads.is_some_and(|r| r.has(i));
            let gw = &self.world.resource::<Substrate>().0;
            travel::route(gw, &cfg.cost, &is_road, from, to, caps).map(std::collections::VecDeque::from)
        } else {
            let mg = self.world.resource::<people::MoveGraph>();
            let gw = &self.world.resource::<Substrate>().0;
            player::path_to(mg, gw.topology(), from, to)
        };
        match path {
            Some(p) => {
                self.world.resource_mut::<player::PlayerState>().set_path(to, p);
                true
            }
            None => false,
        }
    }

    /// The party's travel capabilities, read across the avatar and its companions: climbing (the
    /// roster holds climbing gear **and** a `climb_share` fraction carry the `climbing_proficient`
    /// flag) and a boat (the roster holds one).
    fn party_caps(&self, cfg: ExploreConfig) -> travel::Caps {
        let mut roster: Vec<bevy_ecs::entity::Entity> = Vec::new();
        if let Some(a) = self.player_avatar() {
            roster.push(a);
        }
        if let Some(p) = self.world.get_resource::<Party>() {
            roster.extend(p.members.iter().copied());
        }
        let has_gear = |g: &str| roster.iter().any(|&e| self.world.get::<Gear>(e).is_some_and(|gr| gr.has(g)));
        let climbers = roster.iter().filter(|&&e| self.world.get::<Flags>(e).is_some_and(|f| f.has("climbing_proficient"))).count();
        let share = if roster.is_empty() { 0.0 } else { climbers as f32 / roster.len() as f32 };
        travel::Caps {
            climbing: has_gear("climbing_gear") && share >= cfg.climb_share,
            boat: has_gear("boat"),
        }
    }

    /// Stop the avatar where it stands.
    pub fn player_halt(&mut self) {
        self.world.resource_mut::<player::PlayerState>().halt();
    }

    /// **Wait** — let one tick pass where the avatar stands. The avatar takes no journey;
    /// its only "action" is to be present while the world lives a single moment around it.
    /// Advances the simulation exactly one tick — the same cost as stepping one hex — so the
    /// turn-based contract holds: *one player action == one tick*. Returns `false` (and does
    /// not advance the world) if there is no avatar — waiting is a thing a *body* does.
    pub fn player_wait(&mut self) -> bool {
        if self.world.resource::<player::PlayerState>().avatar().is_none() {
            return false;
        }
        self.step(); // a wait is one tick; the avatar simply does not move during it
        true
    }

    /// **Search** where the avatar stands — the discovery verb. Reveals every undiscovered
    /// feature here whose knowledge gate the player satisfies, and the player gains whatever
    /// lore those places teach. It is an action like waiting: one tick passes. Returns what was
    /// found (empty with no avatar). Deterministic: knowledge, not luck, decides.
    pub fn player_search(&mut self) -> player::SearchOutcome {
        if self.world.resource::<player::PlayerState>().avatar().is_none() {
            return player::SearchOutcome::default();
        }
        let out = player::search(&mut self.world);
        self.step(); // searching spends the turn
        out
    }

    /// Would a search here turn anything up? `Findable` (there is, and you can), `Locked` (there
    /// is, but you lack the lore — the lure), or `Nothing`. Drives the "press F to search" hint.
    pub fn player_find_state(&self) -> FindState {
        player::find_state(&self.world)
    }

    /// The lore facts the player holds, sorted — the contents of the journal's Lore tab.
    pub fn player_lore(&self) -> Vec<String> {
        let mut v: Vec<String> = self.world.resource::<player::PlayerKnowledge>().lore.iter().cloned().collect();
        v.sort();
        v
    }

    /// Does the player hold this lore fact?
    pub fn player_knows(&self, fact: &str) -> bool {
        self.world.resource::<player::PlayerKnowledge>().lore.contains(fact)
    }

    /// Is the avatar mid-journey?
    pub fn player_traveling(&self) -> bool {
        self.world.resource::<player::PlayerState>().traveling()
    }

    /// Where the avatar stands, if one is spawned.
    pub fn player_position(&self) -> Option<Coord> {
        let avatar = self.world.resource::<player::PlayerState>().avatar()?;
        self.world.get::<Position>(avatar).map(|p| p.0)
    }

    /// How many tiles the player has revealed (the fog-of-war it has lifted).
    pub fn player_explored_count(&self) -> usize {
        self.world.resource::<player::PlayerState>().explored_count()
    }

    /// Every tile the player has revealed, for a renderer to draw the explored map.
    pub fn player_explored(&self) -> Vec<Coord> {
        let topo = self.world.resource::<Substrate>().0.topology();
        self.world.resource::<player::PlayerState>().explored_tiles(topo)
    }

    /// What the player sees right now — the tile underfoot, the tiles in sight (terrain +
    /// the features it can make out), and the bodies nearby. The "look" verb.
    pub fn player_view(&mut self) -> Option<PlayerView> {
        player::view(&mut self.world)
    }

    /// The court seats a person currently belongs to (it may hold several).
    pub fn allegiance_of(&self, e: bevy_ecs::entity::Entity) -> Vec<Coord> {
        self.world.get::<Allegiance>(e).map(|a| a.0.iter().map(|b| b.seat).collect()).unwrap_or_default()
    }

    /// How many people are currently detained by faction enforcers.
    pub fn detained_count(&mut self) -> usize {
        let mut q = self.world.query::<&Detained>();
        q.iter(&self.world).count()
    }

    /// The features standing on hex `c`.
    pub fn features_at(&self, c: Coord) -> &[Feature] {
        self.features().at(self.substrate().topology(), c)
    }

    /// The live feature affordances in the world (smart-object actions, with their
    /// depletion state).
    pub fn affordances(&self) -> &[people::AffordanceSite] {
        &self.world.resource::<WorldAffordances>().0
    }

    /// A macro snapshot of the world right now — population, wealth, goods, the
    /// emergent professions, and how hard the features are being worked. The basis
    /// for the V&V invariants ([`check`]).
    pub fn census(&mut self) -> Census {
        Census::take(&mut self.world)
    }

    /// Who holds the throne, if a throne exists and anyone has seized it.
    pub fn throne_holder(&self) -> Option<bevy_ecs::entity::Entity> {
        self.world.get_resource::<Throne>().and_then(|t| t.holder)
    }

    /// Is the throne held by someone whose ambition runs well above the innate
    /// baseline? (The content never pursue it — a held throne is ambitious hands.)
    pub fn throne_held_by_the_ambitious(&self) -> bool {
        let Some(h) = self.throne_holder() else { return false };
        let Some(p) = self.world.get::<Personality>(h) else { return false };
        let reg = self.world.resource::<Registry>();
        reg.trait_id("ambition").and_then(|a| p.0.get(a).copied()).is_some_and(|v| v > 0.4)
    }

    /// Is this entity still a living person?
    pub fn is_alive(&self, e: bevy_ecs::entity::Entity) -> bool {
        self.world.get::<Npc>(e).is_some()
    }

    /// Coins held by a specific person, if alive.
    pub fn money_of(&self, e: bevy_ecs::entity::Entity) -> Option<i64> {
        self.world.get::<Inventory>(e).map(|i| i.money)
    }

    /// Where an entity stands, if it carries a position.
    pub fn position_of(&self, e: bevy_ecs::entity::Entity) -> Option<Coord> {
        self.world.get::<Position>(e).map(|p| p.0)
    }

    /// Whether the RPG layer is awake for this run (NPCs and the avatar carry WWN stats).
    pub fn rpg_enabled(&self) -> bool {
        self.world.get_resource::<RpgData>().is_some()
    }

    /// The WWN attribute scores of an entity, if it carries them.
    pub fn abilities_of(&self, e: bevy_ecs::entity::Entity) -> Option<&Abilities> {
        self.world.get::<Abilities>(e)
    }

    /// An entity's proficiency rank (`-1` unskilled … `4`) in a named WWN skill, if it has stats.
    pub fn proficiency_of(&self, e: bevy_ecs::entity::Entity, skill: &str) -> Option<i8> {
        let id = self.world.get_resource::<RpgData>()?.skill_id(skill)?;
        self.world.get::<Proficiencies>(e).map(|p| p.rank(id))
    }

    /// The RPG content set this run uses (attributes, skills, foci, edges), if enabled.
    pub fn rpg_data(&self) -> Option<&RpgData> {
        self.world.get_resource::<RpgData>()
    }

    /// The name of the archetype Edge an entity was rolled with (e.g. "Wanderer"), if any.
    pub fn archetype_of(&self, e: bevy_ecs::entity::Entity) -> Option<&str> {
        let id = self.world.get::<Archetype>(e)?.0?;
        Some(self.world.get_resource::<RpgData>()?.edge_name(id))
    }

    /// Every living NPC entity (deterministic iteration order).
    pub fn npcs(&mut self) -> Vec<bevy_ecs::entity::Entity> {
        let mut q = self.world.query_filtered::<Entity, With<Npc>>();
        q.iter(&self.world).collect()
    }

    // --- Party (recruited companions) ---

    /// Attempt to **recruit** an NPC into the avatar's party. The avatar talks them round with a
    /// deterministic Convince/Lead check — the avatar's Charisma modifier plus the better of those
    /// two skills, against a difficulty the NPC's opinion of the avatar sets (a friend is easier,
    /// a foe harder). On success the NPC joins the roster and travels as a stack: it stops acting
    /// on its own (`Suspended`) and follows the avatar (`Follower`), keeping all its own stats.
    /// Recruiting is a social action — one tick passes either way. Returns whether they joined;
    /// `false` (and no tick) if there's no avatar, the target isn't a recruitable NPC, the RPG or
    /// party layers are off, the target is already a member, or the party is full.
    pub fn player_recruit(&mut self, listener: bevy_ecs::entity::Entity) -> bool {
        let Some(avatar) = self.player_avatar() else { return false };
        if avatar == listener || self.world.get::<Npc>(listener).is_none() {
            return false;
        }
        let Some(cfg) = self.world.get_resource::<PartyConfig>().copied() else { return false };
        let Some(party) = self.world.get_resource::<Party>() else { return false };
        if party.contains(listener) || (cfg.max_size != 0 && party.len() >= cfg.max_size) {
            return false;
        }
        // The avatar's social capability: Charisma modifier + the better of Convince / Lead.
        let (cha_mod, social) = {
            let (Some(ab), Some(pr), Some(data)) = (
                self.world.get::<Abilities>(avatar),
                self.world.get::<Proficiencies>(avatar),
                self.world.get_resource::<RpgData>(),
            ) else {
                return false;
            };
            let rank = |name: &str| data.skill_id(name).map(|i| pr.rank(i)).unwrap_or(rpg::PROF_UNSKILLED);
            (ab.modifier(rpg::CHA), rank("Convince").max(rank("Lead")))
        };
        // Disposition: the NPC's standing opinion of the avatar sets the difficulty.
        let opinion = self.world.get::<Opinion>(listener).map(|o| o.of(avatar)).unwrap_or(0.0);
        let difficulty = party::disposition_difficulty(&cfg, opinion);
        let joined = rpg::check(cha_mod, social, 0, difficulty).succeeded();
        if joined {
            let since = self.tick();
            let at = self.player_position();
            self.world.entity_mut(listener).insert((PartyMember { since }, Suspended, Follower));
            // A companion shares the road's hardships: give it `Vitals` if survival is on and it
            // lacks them (party-scoped survival, where NPCs otherwise carry none).
            if self.world.get_resource::<SurvivalConfig>().is_some() && self.world.get::<Vitals>(listener).is_none() {
                self.world.entity_mut(listener).insert(survival::Vitals::default());
            }
            // Snap them to the avatar's side at once; `player_travel` keeps them there.
            if let (Some(at), Some(mut p)) = (at, self.world.get_mut::<Position>(listener)) {
                p.0 = at;
            }
            self.world.resource_mut::<Party>().push(listener);
        }
        self.step(); // talking someone round spends the turn either way
        joined
    }

    /// The avatar's party roster, in recruit order (empty if the party layer is off).
    pub fn party_roster(&self) -> Vec<bevy_ecs::entity::Entity> {
        self.world.get_resource::<Party>().map(|p| p.members.clone()).unwrap_or_default()
    }

    /// How many companions travel with the avatar.
    pub fn party_size(&self) -> usize {
        self.world.get_resource::<Party>().map_or(0, |p| p.len())
    }

    /// Is this entity a recruited member of the avatar's party?
    pub fn is_party_member(&self, e: bevy_ecs::entity::Entity) -> bool {
        self.world.get::<PartyMember>(e).is_some()
    }

    /// **Dismiss** a companion: drop it from the roster and let it resume its own life.
    pub fn dismiss(&mut self, e: bevy_ecs::entity::Entity) -> bool {
        if self.world.get::<PartyMember>(e).is_none() {
            return false;
        }
        self.world.entity_mut(e).remove::<(PartyMember, Suspended, Follower)>();
        if let Some(mut party) = self.world.get_resource_mut::<Party>() {
            party.remove(e);
        }
        true
    }

    // --- Survival ---

    /// Whether the survival layer is awake (every body carries `Vitals`, drained per day).
    pub fn survival_enabled(&self) -> bool {
        self.world.get_resource::<SurvivalConfig>().is_some()
    }

    /// A body's survival meters (thirst / warmth / stamina), if it carries them.
    pub fn vitals_of(&self, e: bevy_ecs::entity::Entity) -> Option<&Vitals> {
        self.world.get::<Vitals>(e)
    }

    // --- Exploration (travel cost, roads, gear) ---

    /// Whether the exploration layer is on (weighted, cost-paced travel; roads; edge gates).
    pub fn exploration_enabled(&self) -> bool {
        self.world.get_resource::<ExploreConfig>().is_some()
    }

    /// Every tile carrying a road (empty if the layer is off) — for a renderer to draw the network.
    pub fn road_tiles(&self) -> Vec<Coord> {
        let Some(roads) = self.world.get_resource::<Roads>() else { return Vec::new() };
        let topo = self.world.resource::<Substrate>().0.topology();
        roads.0.iter().map(|&i| topo.coord(i)).collect()
    }

    /// Days to enter a tile under the current cost field (≈1.0 a forest day; roads cheaper, peaks
    /// dearer). `1.0` when the exploration layer is off.
    pub fn travel_cost_at(&self, c: Coord) -> f32 {
        let topo = self.world.resource::<Substrate>().0.topology();
        self.world.get_resource::<TravelCost>().and_then(|tc| tc.0.get(topo.index_of(c)).copied()).unwrap_or(1.0)
    }

    /// Give the avatar a piece of gear (`"climbing_gear"`, `"boat"`, `"warm_gear"`, …). Returns
    /// false if there is no avatar.
    pub fn player_equip(&mut self, item: &str) -> bool {
        let Some(avatar) = self.player_avatar() else { return false };
        let mut em = self.world.entity_mut(avatar);
        match em.get_mut::<Gear>() {
            Some(mut g) => {
                g.0.insert(item.to_string());
            }
            None => {
                em.insert(Gear(std::collections::HashSet::from([item.to_string()])));
            }
        }
        true
    }

    /// Whether the avatar carries a piece of gear.
    pub fn player_has_gear(&self, item: &str) -> bool {
        self.player_avatar().is_some_and(|a| self.world.get::<Gear>(a).is_some_and(|g| g.has(item)))
    }

    /// Everything the avatar carries, sorted (empty if it carries nothing / no exploration layer) —
    /// for the character sheet to list.
    pub fn player_gear(&self) -> Vec<String> {
        let mut v: Vec<String> =
            self.player_avatar().and_then(|a| self.world.get::<Gear>(a)).map(|g| g.0.iter().cloned().collect()).unwrap_or_default();
        v.sort();
        v
    }

    /// `who`'s opinion of `toward` (`-1..1`; `0` if they have no opinion or no `Opinion`).
    /// Read-only — used to show a soul's live disposition toward the player as a conversation
    /// moves it.
    pub fn opinion_of(&self, who: bevy_ecs::entity::Entity, toward: bevy_ecs::entity::Entity) -> Option<f32> {
        self.world.get::<Opinion>(who).map(|o| o.of(toward))
    }

    /// Does `who` bear a standing grudge against `toward`? (Read-only counterpart to [`Self::grudges`].)
    pub fn bears_grudge(&self, who: bevy_ecs::entity::Entity, toward: bevy_ecs::entity::Entity) -> bool {
        self.world.get::<Grievance>(who).is_some_and(|g| g.0 == toward)
    }

    /// Every entity that someone bears a grudge against (the targets of feuds).
    pub fn feud_targets(&mut self) -> Vec<bevy_ecs::entity::Entity> {
        let mut q = self.world.query_filtered::<&Grievance, With<Npc>>();
        q.iter(&self.world).map(|g| g.0).collect()
    }

    /// Every grudge as a `(holder, target)` pair — who would see whom dead.
    pub fn grudges(&mut self) -> Vec<(bevy_ecs::entity::Entity, bevy_ecs::entity::Entity)> {
        let mut q = self.world.query_filtered::<(bevy_ecs::entity::Entity, &Grievance), With<Npc>>();
        q.iter(&self.world).map(|(e, g)| (e, g.0)).collect()
    }

    /// Any one living NPC (deterministic — the first in iteration order).
    pub fn any_npc(&mut self) -> Option<bevy_ecs::entity::Entity> {
        let mut q = self.world.query_filtered::<bevy_ecs::entity::Entity, With<Npc>>();
        q.iter(&self.world).next()
    }

    /// The named trait's value for an entity (for inspecting personality).
    pub fn trait_of(&self, e: bevy_ecs::entity::Entity, name: &str) -> Option<f32> {
        let id = self.world.resource::<Registry>().trait_id(name)?;
        self.world.get::<Personality>(e)?.0.get(id).copied()
    }

    /// The highest value of a named trait across all living people (for observing
    /// how events have reshaped personalities).
    pub fn max_trait(&mut self, name: &str) -> f32 {
        let Some(id) = self.world.resource::<Registry>().trait_id(name) else {
            return 0.0;
        };
        let mut q = self.world.query_filtered::<&Personality, With<Npc>>();
        q.iter(&self.world).filter_map(|p| p.0.get(id).copied()).fold(0.0, f32::max)
    }

    /// The lowest value of a named trait across all living people.
    pub fn min_trait(&mut self, name: &str) -> f32 {
        let Some(id) = self.world.resource::<Registry>().trait_id(name) else {
            return 0.0;
        };
        let mut q = self.world.query_filtered::<&Personality, With<Npc>>();
        q.iter(&self.world).filter_map(|p| p.0.get(id).copied()).fold(1.0, f32::min)
    }

    /// A named mood for an entity, if it has one.
    pub fn mood_of(&self, e: bevy_ecs::entity::Entity, name: &str) -> Option<f32> {
        let id = self.world.resource::<Registry>().mood_id(name)?;
        self.world.get::<Mood>(e)?.0.get(id).copied()
    }

    /// The highest value of a named mood across all living people.
    pub fn max_mood(&mut self, name: &str) -> f32 {
        let Some(id) = self.world.resource::<Registry>().mood_id(name) else {
            return 0.0;
        };
        let mut q = self.world.query_filtered::<&Mood, With<Npc>>();
        q.iter(&self.world).filter_map(|m| m.0.get(id).copied()).fold(0.0, f32::max)
    }

    pub fn fauna_count(&mut self) -> usize {
        let mut q = self.world.query_filtered::<(), With<Herbivore>>();
        q.iter(&self.world).count()
    }

    /// Living predators.
    pub fn carnivore_count(&mut self) -> usize {
        let mut q = self.world.query_filtered::<(), With<Carnivore>>();
        q.iter(&self.world).count()
    }

    pub fn fauna_positions(&mut self) -> Vec<Coord> {
        let mut q = self.world.query_filtered::<&Position, With<Herbivore>>();
        q.iter(&self.world).map(|p| p.0).collect()
    }

    /// The creature roster this world hosts — every species and its traits.
    pub fn bestiary(&self) -> &Bestiary {
        self.world.resource::<Bestiary>()
    }

    /// A census of every living creature: a **stable id** (unchanged across ticks
    /// until the creature dies, so the renderer can track and smoothly move it), its
    /// species (an index into [`Self::bestiary`]), and where it stands. Also what the
    /// demos read to show how species sort themselves into their biomes.
    pub fn fauna_census(&mut self) -> Vec<(u64, usize, Coord)> {
        let mut q = self.world.query::<(Entity, &SpeciesId, &Position)>();
        q.iter(&self.world).map(|(e, s, p)| (e.to_bits(), s.0, p.0)).collect()
    }

    pub fn npc_count(&mut self) -> usize {
        let mut q = self.world.query_filtered::<(), With<Npc>>();
        q.iter(&self.world).count()
    }

    pub fn npc_positions(&mut self) -> Vec<Coord> {
        let mut q = self.world.query_filtered::<&Position, With<Npc>>();
        q.iter(&self.world).map(|p| p.0).collect()
    }

    /// How many NPCs have taken up a given skill (practised it at all) — the
    /// emergent occupations. The skill name comes from the data, not the code.
    pub fn practitioners(&mut self, skill: &str) -> usize {
        let Some(id) = self.world.resource::<Registry>().skill_id(skill) else {
            return 0;
        };
        let mut q = self.world.query_filtered::<&Skills, With<Npc>>();
        q.iter(&self.world).filter(|s| s.0.get(id).is_some_and(|&v| v > 0.1)).count()
    }

    /// Total coins across NPCs and markets — exactly conserved by trade (deaths
    /// remove it). Integer money makes this exact.
    pub fn total_money(&mut self) -> i64 {
        let mut np = self.world.query_filtered::<&Inventory, With<Npc>>();
        let npc_money: i64 = np.iter(&self.world).map(|i| i.money).sum();
        let mut mk = self.world.query::<&Market>();
        let market_money: i64 = mk.iter(&self.world).map(|m| m.money).sum();
        npc_money + market_money
    }

    /// Total goods stock held across all markets.
    pub fn total_market_stock(&mut self) -> u64 {
        let mut q = self.world.query::<&Market>();
        q.iter(&self.world).map(|m| m.stock.iter().map(|&s| s as u64).sum::<u64>()).sum()
    }

    /// Total goods in the world — every NPC inventory plus every market. Rises
    /// when production outpaces consumption.
    pub fn total_goods(&mut self) -> u64 {
        let mut np = self.world.query_filtered::<&Inventory, With<Npc>>();
        let held: u64 = np.iter(&self.world).map(|i| i.stock.iter().map(|&s| s as u64).sum::<u64>()).sum();
        held + self.total_market_stock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Fauna ---

    #[test]
    fn fauna_spawn_on_land() {
        let mut sim = Simulation::new(Setup { fauna: 40, seed: 2026, ..Default::default() });
        let sea = sim.substrate().params().sea_level;
        let positions = sim.fauna_positions();
        assert!(!positions.is_empty(), "no fauna spawned");
        let sub = sim.substrate();
        assert!(positions.iter().all(|&c| sub.elevation(c) >= sea), "a herbivore spawned in the sea");
    }

    #[test]
    fn simulation_steps_and_advances_time() {
        let mut sim = Simulation::new(Setup { fauna: 20, seed: 1, ..Default::default() });
        let start = sim.tick();
        sim.run(20);
        assert_eq!(sim.tick(), start + 20);
    }

    #[test]
    fn population_grows_below_capacity() {
        let mut sim = Simulation::new(Setup { fauna: 6, seed: 2026, ..Default::default() });
        let start = sim.fauna_count();
        sim.run(60);
        assert!(sim.fauna_count() > start, "a small fed population should grow ({start} → {})", sim.fauna_count());
    }

    #[test]
    fn starving_fauna_die_off() {
        let mut sim = Simulation::new(Setup {
            fauna: 30,
            seed: 5,
            warmup: 50,
            fauna_cfg: FaunaConfig { eat_rate: 0.0, initial_energy: 10.0, metabolism: 1.0, ..Default::default() },
            ..Default::default()
        });
        assert!(sim.fauna_count() > 0);
        // Long enough that even the smallest, slowest-burning species starves
        // (metabolism now scales with body size, so the tiniest last the longest).
        sim.run(40);
        assert_eq!(sim.fauna_count(), 0, "no animal should survive without food");
    }

    #[test]
    fn grazing_depletes_vegetation() {
        let standing_biomass = |fauna: usize| -> f32 {
            let mut sim = Simulation::new(Setup { fauna, seed: 2026, ..Default::default() });
            sim.run(40);
            let sub = sim.substrate();
            let topo = sub.topology();
            topo.indices().map(|i| sub.plant_biomass(topo.coord(i))).sum()
        };
        assert!(standing_biomass(120) < standing_biomass(0), "grazers should draw vegetation down");
    }

    #[test]
    fn fauna_runs_are_deterministic() {
        // Now with predators too — the stochastic kills draw from a seeded stream, so
        // two identical runs stay bit-identical in both populations.
        let run = || {
            let mut s = Simulation::new(Setup { fauna: 40, carnivores: 15, seed: 42, ..Default::default() });
            s.run(60);
            (s.fauna_count(), s.carnivore_count())
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn predators_thin_the_herd() {
        // On a small, dense range — prey forced together, so a pack can actually hunt
        // (above the spatial refuge) — a run with predators leaves fewer herbivores
        // than the same run without: direct predation pressure. (On a large, sparse
        // map the net effect can invert into a trophic cascade, where predators raise
        // prey by sparing the forage; that phase-dependence is why this test pins down
        // the *mechanism* on a range where culling dominates.)
        let herd = |carnivores: usize| -> usize {
            let mut sim = Simulation::new(Setup { width: 18, height: 12, fauna: 60, carnivores, seed: 11, ..Default::default() });
            sim.run(80);
            sim.fauna_count()
        };
        let without = herd(0);
        let with = herd(20);
        assert!(without > 0, "the control herd died out on its own; can't test predation");
        assert!(with < without, "predators should thin a dense herd (with {with} vs without {without})");
    }

    #[test]
    fn predators_and_prey_coexist() {
        // The trophic loop closes: with Liebig productivity, herd aggregation (huntable
        // density), and a patient pack riding out the troughs, both tiers survive a long
        // run in a sustained oscillation — neither collapses. A standing predator tier.
        let mut sim = Simulation::new(Setup { width: 48, height: 36, seed: 11, warmup: 300, fauna: 60, carnivores: 8, ..Default::default() });
        sim.run(600);
        assert!(sim.fauna_count() > 0, "the herd died out");
        assert!(sim.carnivore_count() > 0, "the predators died out — no standing tier formed");
    }

    #[test]
    fn predators_starve_without_prey() {
        // A pack with nothing to hunt only pays metabolism — it dwindles toward zero.
        let mut sim = Simulation::new(Setup { fauna: 0, carnivores: 40, seed: 4, ..Default::default() });
        let start = sim.carnivore_count();
        sim.run(250);
        assert!(sim.carnivore_count() < start, "predators with no prey should decline ({start} → {})", sim.carnivore_count());
    }

    // --- Economy ---

    fn economy(npcs: usize) -> Simulation {
        Simulation::new(Setup { width: 40, height: 30, seed: 2026, npcs, ..Default::default() })
    }

    #[test]
    fn npcs_spawn_on_land() {
        let mut sim = economy(40);
        let sea = sim.substrate().params().sea_level;
        let positions = sim.npc_positions();
        assert!(!positions.is_empty(), "no NPCs spawned");
        let sub = sim.substrate();
        assert!(positions.iter().all(|&c| sub.elevation(c) >= sea), "an NPC spawned in the sea");
    }

    #[test]
    fn the_population_sustains_itself() {
        // Planning ahead, NPCs feed themselves off the land rather than crashing:
        // a large start is carried, not collapsed.
        let mut sim = economy(60);
        sim.run(150);
        assert!(sim.npc_count() > 5, "the economy collapsed ({} left)", sim.npc_count());
    }

    // --- RPG layer (Worlds Without Number) ---

    #[test]
    fn rpg_layer_stamps_npcs_and_the_avatar() {
        let mut sim = Simulation::new(Setup { width: 40, height: 30, seed: 2026, npcs: 30, rpg: true, ..Default::default() });
        assert!(sim.rpg_enabled(), "the rpg resource is present when enabled");
        let npc = sim.any_npc().expect("npcs spawned");
        assert!(sim.abilities_of(npc).is_some(), "an NPC carries WWN attributes");
        // The rolled edge / background trained at least one skill above unskilled.
        let names: Vec<String> = sim.rpg_data().unwrap().skills().iter().map(|s| s.name.clone()).collect();
        assert!(
            names.iter().any(|s| sim.proficiency_of(npc, s).is_some_and(|r| r > -1)),
            "the NPC has at least one trained skill",
        );
        // The avatar gets capabilities too.
        let avatar = sim.spawn_player(None);
        assert!(sim.abilities_of(avatar).is_some(), "the avatar carries WWN attributes");
    }

    #[test]
    fn rpg_rolls_are_deterministic() {
        let first_scores = || {
            let mut sim = Simulation::new(Setup { width: 40, height: 30, seed: 2026, npcs: 20, rpg: true, ..Default::default() });
            let npc = sim.any_npc().unwrap();
            sim.abilities_of(npc).unwrap().scores
        };
        assert_eq!(first_scores(), first_scores(), "same seed → identical rolled stats");
    }

    // --- Party layer ---

    #[test]
    fn a_recruited_companion_follows_and_stops_acting() {
        let mut sim = Simulation::new(Setup {
            width: 40,
            height: 30,
            seed: 2026,
            npcs: 30,
            rpg: true,
            party: true,
            // Trivial difficulty so the check always passes — this tests the mechanics, not the roll.
            party_cfg: PartyConfig { recruit_difficulty: -100, ..Default::default() },
            ..Default::default()
        });
        let _avatar = sim.spawn_player(None);
        let target = sim.any_npc().expect("npcs spawned");

        assert!(sim.player_recruit(target), "the recruit check passes at trivial difficulty");
        assert!(sim.is_party_member(target) && sim.party_size() == 1);
        assert_eq!(sim.party_roster(), vec![target]);

        // Snapped to the avatar's side, and — being suspended — it stays there rather than
        // wandering off on its own as the world ticks on around it.
        let at = sim.player_position().unwrap();
        assert_eq!(sim.position_of(target), Some(at), "the companion is at the avatar's side");
        sim.run(5);
        assert_eq!(
            sim.position_of(target),
            sim.player_position(),
            "a suspended follower holds station at the avatar",
        );

        // Dismissing returns it to autonomy.
        assert!(sim.dismiss(target));
        assert!(!sim.is_party_member(target) && sim.party_size() == 0);
    }

    #[test]
    fn recruiting_is_gated_by_the_check() {
        // The same world, but an honestly hard difficulty an average avatar can't clear with a
        // neutral stranger — so the recruit is refused and the party stays empty.
        let mut sim = Simulation::new(Setup {
            width: 40,
            height: 30,
            seed: 2026,
            npcs: 30,
            rpg: true,
            party: true,
            party_cfg: PartyConfig { recruit_difficulty: 100, ..Default::default() },
            ..Default::default()
        });
        let _ = sim.spawn_player(None);
        let target = sim.any_npc().unwrap();
        assert!(!sim.player_recruit(target), "an impossible check refuses the recruit");
        assert!(sim.party_size() == 0 && !sim.is_party_member(target));
    }

    // --- Speech skill scaling ---

    #[test]
    fn speech_is_unscaled_without_the_rpg_layer() {
        let mut sim = economy(20);
        let avatar = sim.spawn_player(None);
        let npc = sim.any_npc().unwrap();
        assert_eq!(sim.speech_strength(avatar, npc), 1.0, "no RPG layer → the avatar's words land at full strength");
    }

    #[test]
    fn speech_strength_is_a_graded_check_with_the_rpg_layer() {
        let mut sim = Simulation::new(Setup { width: 40, height: 30, seed: 2026, npcs: 20, rpg: true, ..Default::default() });
        let avatar = sim.spawn_player(None);
        let npc = sim.any_npc().unwrap();
        let s = sim.speech_strength(avatar, npc);
        assert!([0.0, 1.0, 1.5].contains(&s), "a graded persuasion result, got {s}");
    }

    // --- World-interaction skill: Notice → exploration sight ---

    #[test]
    fn notice_sets_the_avatar_sight() {
        // Off: the base radius. On: the base lifted by the avatar's rolled Notice skill.
        let mut off = economy(20);
        let _ = off.spawn_player(None);
        assert_eq!(off.player_sight(), 3, "no RPG layer → base sight");

        let mut on = Simulation::new(Setup { width: 40, height: 30, seed: 2026, npcs: 20, rpg: true, ..Default::default() });
        let avatar = on.spawn_player(None);
        let notice = on.proficiency_of(avatar, "Notice").unwrap();
        assert_eq!(on.player_sight(), (3 + notice as i32).max(1), "sight tracks Notice");
        assert_eq!(on.player_perceptive(), notice >= 2, "a trained scout (Notice ≥ 2) is perceptive");
        assert!(!off.player_perceptive(), "no RPG layer → not perceptive (active search only)");
    }

    // --- Survival layer ---

    #[test]
    fn no_survival_means_no_vitals() {
        let mut sim = economy(20);
        assert!(!sim.survival_enabled());
        let npc = sim.any_npc().unwrap();
        assert!(sim.vitals_of(npc).is_none(), "no survival layer → no vitals (byte-identical)");
    }

    #[test]
    fn survival_attaches_vitals_to_every_body() {
        let mut sim = Simulation::new(Setup {
            width: 40,
            height: 30,
            seed: 2026,
            npcs: 20,
            rpg: true,
            survival: true,
            ..Default::default()
        });
        assert!(sim.survival_enabled());
        let npc = sim.any_npc().unwrap();
        assert_eq!(sim.vitals_of(npc).unwrap().thirst, 100.0, "vitals start full");
        let avatar = sim.spawn_player(None);
        assert!(sim.vitals_of(avatar).is_some(), "the avatar is a body too — it carries vitals");
        // The per-day system runs every tick and keeps every meter in range (no NaN/overflow).
        sim.run(20);
        if let Some(v) = sim.vitals_of(npc) {
            for m in [v.thirst, v.warmth, v.stamina] {
                assert!((0.0..=100.0).contains(&m), "a vital left its range: {m}");
            }
        }
    }

    // --- Exploration layer (travel cost, roads, gear) ---

    #[test]
    fn no_exploration_keeps_flat_costless_travel() {
        let mut sim = economy(20);
        assert!(!sim.exploration_enabled());
        assert!(sim.road_tiles().is_empty(), "no roads without the layer");
        let c = sim.substrate().topology().coord(0);
        assert_eq!(sim.travel_cost_at(c), 1.0, "no cost field → a flat day per hex (byte-identical)");
    }

    #[test]
    fn exploration_lays_roads_paces_travel_and_carries_gear() {
        let mut sim = Simulation::new(Setup {
            width: 48,
            height: 36,
            seed: 7,
            npcs: 40,
            exploration: true,
            ..Default::default()
        });
        let _ = sim.spawn_player(None);
        assert!(sim.exploration_enabled());
        assert!(!sim.road_tiles().is_empty(), "roads were laid between the settlements");
        assert!(sim.road_tiles().iter().any(|&c| sim.travel_cost_at(c) < 1.0), "roads make some tiles the fast lane");
        // Gear can be equipped — the climbing/boat gates read it.
        assert!(!sim.player_has_gear("climbing_gear"));
        assert!(sim.player_equip("climbing_gear") && sim.player_has_gear("climbing_gear"));
    }

    // --- Capstone: the whole stack together ---

    #[test]
    fn the_whole_stack_is_deterministic_and_never_mints_money() {
        // Every new layer at once, with an avatar in the world.
        let run = || {
            let mut sim = Simulation::new(Setup {
                width: 40,
                height: 30,
                seed: 2026,
                npcs: 40,
                markets_on_settlements: true,
                rpg: true,
                party: true,
                survival: true,
                exploration: true,
                ..Default::default()
            });
            let _ = sim.spawn_player(None);
            let money0 = sim.total_money();
            sim.run(40);
            (money0, sim.total_money(), sim.npc_count())
        };
        let a = run();
        let b = run();
        assert_eq!(a, b, "rpg + party + survival + exploration on → byte-identical per seed");
        assert!(a.1 <= a.0, "no layer mints money — the total only falls (deaths), trade conserving the rest");
    }

    #[test]
    fn both_farming_and_baking_emerge() {
        let mut sim = economy(60);
        // Food is bread, and bread comes only from grain you grow then bake. At
        // first people just buy the markets' cheap grain and bake it — rational —
        // so farming stays low; once that grain runs dry, growing it becomes worth
        // the effort and a farming trade emerges alongside baking. Nobody was
        // assigned a job: the planner found both.
        sim.run(180);
        let (farmers, bakers) = (sim.practitioners("farming"), sim.practitioners("baking"));
        assert!(farmers > 0 && bakers > 0, "occupations didn't emerge (farming {farmers}, baking {bakers})");
    }

    /// Economy goals plus a "rule" goal (hold the throne, gated on ambition).
    fn throne_goals(reg: &Registry) -> Goals {
        Goals::from_ron(
            r#"[
                (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                (name: "stocked",   condition: Holding(good: Edible, at_least: 15), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
                (name: "solvent",   condition: Money(at_least: 200),      appeal: [(input: Deficit, curve: Linear(m: 0.5, b: 0.0))]),
                (name: "rule",      condition: Verb(verb: "rule", target: Me),
                    appeal: [(input: Trait("ambition"), curve: Linear(m: 0.7, b: 0.0)), (input: Deficit, curve: Linear(m: 1.0, b: 0.0))]),
            ]"#,
            reg,
        )
        .unwrap()
    }

    #[test]
    fn an_ambitious_few_seize_the_throne() {
        // A wholly non-economic want on the same IAUS+GOAP rails: hold the throne —
        // a single *shared* world fact (`Fact(0)`), seized by a place-based deed.
        // Only the ambitious pursue it (and only once fed/stocked, lest they starve).
        let reg = Registry::bundled();
        let goals = throne_goals(&reg);
        let mut sim = Simulation::new(Setup {
            width: 40,
            height: 30,
            seed: 2026,
            npcs: 40,
            throne: true,
            ambitious: 6,
            goals,
            registry: reg,
            ..Default::default()
        });
        sim.run(160);
        // Nobody was handed the crown: the ambitious sought it out, and the throne
        // (one shared seat) ends in ambitious hands — never the content's.
        assert!(sim.throne_held_by_the_ambitious(), "an ambitious person should hold the throne");
    }

    #[test]
    fn an_innate_trait_does_not_decay() {
        // A trait is who you are, not a skill that atrophies or a meter that
        // drains. In the plain economy nothing appraises greed, so a person's
        // greed never moves a hair, however long they live and whatever they do.
        let mut sim = economy(40);
        let npc = sim.any_npc().expect("npcs spawned");
        let early = sim.trait_of(npc, "greed").expect("has a personality");
        sim.run(150);
        // The economy sustains everyone, so the same person is still here.
        let late = sim.trait_of(npc, "greed").expect("still alive");
        assert!((late - early).abs() < 1e-6, "an innate trait must not drift ({early:.3} -> {late:.3})");
    }

    #[test]
    fn a_life_of_anger_hardens_into_a_grudge() {
        // The full feeling→character pipeline. Being deposed doesn't *directly* make
        // anyone vengeful — it makes them *angry* (a mood). Only anger, sustained
        // over a life of being cast down, slowly hardens into vengeance (the trait).
        // Four ambitious rivals trade the throne; vengeance climbs well past
        // anything birth variation could give (baseline 0.20, spread 0.15 → ≤0.35),
        // and forgiveness — its opposite — is worn away. Nobody scripted any of it.
        let reg = Registry::bundled();
        let goals = throne_goals(&reg);
        let mut sim = Simulation::new(Setup {
            width: 40,
            height: 30,
            seed: 2026,
            npcs: 30,
            throne: true,
            ambitious: 4,
            goals,
            registry: reg,
            ..Default::default()
        });
        sim.run(160);
        let vengeance = sim.max_trait("vengeance");
        let forgiveness = sim.min_trait("forgiveness");
        assert!(vengeance > 0.40, "sustained anger should breed vengeance (peak {vengeance:.3})");
        assert!(forgiveness < 0.30, "and wear away its opposite, forgiveness (min {forgiveness:.3})");
    }

    #[test]
    fn a_mood_spikes_then_fades_while_the_trait_endures() {
        // The two layers, side by side. Being crowned both feeds ambition (a
        // lasting trait) and brings a flush of joy (a passing mood). Tracking a
        // freshly crowned ruler over a short while: his joy cools back toward rest,
        // while his ambition — a disposition, only ever raised by events — does not
        // fall. Mood is weather; trait is climate.
        let reg = Registry::bundled();
        let goals = throne_goals(&reg);
        // A single, uncontested claimant — so once crowned he *holds* the throne and
        // is not deposed-and-re-crowned within the window (which would re-spike joy).
        // This test is about a mood fading, not the scramble for the throne.
        let mut sim = Simulation::new(Setup {
            width: 40,
            height: 30,
            seed: 2026,
            npcs: 40,
            throne: true,
            ambitious: 1,
            goals,
            registry: reg,
            ..Default::default()
        });
        // Run until our claimant is first crowned (and capture his joy that very tick).
        let mut king = None;
        for _ in 0..400 {
            sim.step();
            if let Some(k) = sim.throne_holder() {
                king = Some(k);
                break;
            }
        }
        let king = king.expect("the ambitious should produce a ruler");
        let joy_early = sim.mood_of(king, "joy").expect("ruler has a mood");
        // Greed: a trait no feeling of his shapes, so it shows the contrast cleanly.
        let greed_early = sim.trait_of(king, "greed").unwrap();
        sim.run(8);
        let joy_late = sim.mood_of(king, "joy").unwrap_or(0.0);
        let greed_late = sim.trait_of(king, "greed").unwrap_or(0.0);
        assert!(joy_early > 0.1, "crowning should bring a flush of joy ({joy_early:.3})");
        assert!(joy_late < joy_early, "joy (a mood) should fade ({joy_early:.3} -> {joy_late:.3})");
        // The disposition holds steady while the feeling cools. Weather vs climate.
        assert!((greed_late - greed_early).abs() < 1e-6, "an unshaped trait must hold ({greed_early:.3} -> {greed_late:.3})");
    }

    #[test]
    fn an_aggrieved_person_hunts_down_their_foe() {
        // "Kill THIS foe" — an entity-targeted goal on the relational grammar:
        // `avenge` = make `alive(foe)` false, with the foe bound per agent by its
        // grudge. A few people each hunt a distinct other; once provisioned they run
        // their quarry down. Since the economy sustains everyone, a missing victim
        // was murdered, not starved.
        let reg = Registry::bundled();
        let goals = Goals::from_ron(
            r#"[
                (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                (name: "stocked",   condition: Holding(good: Edible, at_least: 8), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
                (name: "avenge",    condition: Verb(verb: "avenge", target: Foe),
                    appeal: [(input: Deficit, curve: Linear(m: 0.55, b: 0.0))]),
            ]"#,
            &reg,
        )
        .unwrap();
        let mut sim = Simulation::new(Setup {
            width: 40,
            height: 30,
            seed: 2026,
            npcs: 30,
            feuds: 3,
            goals,
            registry: reg,
            ..Default::default()
        });
        let victims = sim.feud_targets();
        assert_eq!(victims.len(), 3, "three grudges should be set");
        sim.run(200);
        let killed = victims.iter().filter(|&&v| !sim.is_alive(v)).count();
        assert!(killed >= 1, "an aggrieved hunter should run down its foe (killed {killed}/3)");
    }

    #[test]
    fn a_taboo_restrains_the_avengers() {
        // The same feud world, with and without a social norm against killing. The
        // `avenge` goal now weighs the deontic `Sanction` on its act, so an empty
        // norm set leaves vengeance free while a kill taboo collapses its appeal —
        // a population's behaviour changed purely by authored norms, with goals,
        // planner, and scenario untouched.
        fn avenging_world(reg: Registry, norms: Norms) -> Simulation {
            let goals = Goals::from_ron(
                r#"[
                    (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                    (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                    (name: "stocked",   condition: Holding(good: Edible, at_least: 8), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
                    (name: "avenge",    condition: Verb(verb: "avenge", target: Foe),
                        appeal: [(input: Deficit,  curve: Linear(m: 0.55, b: 0.0)),
                                 (input: Sanction, curve: Linear(m: -1.0, b: 1.0))]),
                ]"#,
                &reg,
            )
            .unwrap();
            Simulation::new(Setup {
                width: 40,
                height: 30,
                seed: 2026,
                npcs: 30,
                feuds: 3,
                goals,
                norms,
                registry: reg,
                ..Default::default()
            })
        }

        // No taboo: vengeance runs its course, as before.
        let mut free = avenging_world(Registry::bundled(), Norms::default());
        let free_victims = free.feud_targets();
        free.run(200);
        let free_killed = free_victims.iter().filter(|&&v| !free.is_alive(v)).count();

        // A kill taboo (fully felt — no `defiance`): the avengers stay their hand.
        let reg = Registry::bundled();
        let taboo = Norms::from_ron(r#"[(act: "avenge", modality: Forbidden)]"#, &reg).unwrap();
        let mut bound = avenging_world(reg, taboo);
        let bound_victims = bound.feud_targets();
        bound.run(200);
        let bound_killed = bound_victims.iter().filter(|&&v| !bound.is_alive(v)).count();

        assert!(free_killed >= 1, "without a taboo, vengeance runs its course ({free_killed}/3)");
        assert_eq!(bound_killed, 0, "a kill taboo should stay every hand ({bound_killed} killed)");
        assert!(bound_killed < free_killed, "the taboo restrains relative to free vengeance");
    }

    #[test]
    fn breaking_the_taboo_hardens_the_killer() {
        // A society that forbids killing, whose avengers ignore the deterrent (their
        // goal doesn't weigh the sanction, so the killings still happen). Each killing
        // is nonetheless a forbidden act unexcused — a transgression — and is appraised
        // on the killer: crossing the line raises their vengeance. Norm violation feeds
        // back into who they are.
        let reg = Registry::bundled();
        let goals = Goals::from_ron(
            r#"[
                (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                (name: "stocked",   condition: Holding(good: Edible, at_least: 8), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
                (name: "avenge",    condition: Verb(verb: "avenge", target: Foe),
                    appeal: [(input: Deficit, curve: Linear(m: 0.55, b: 0.0))]),
            ]"#,
            &reg,
        )
        .unwrap();
        let norms = Norms::from_ron(r#"[(act: "avenge", modality: Forbidden)]"#, &reg).unwrap();
        let mut sim = Simulation::new(Setup {
            width: 40,
            height: 30,
            seed: 2026,
            npcs: 30,
            feuds: 3,
            goals,
            norms,
            registry: reg,
            ..Default::default()
        });
        // Each aggressor's foe and starting vengeance, before any blood is shed.
        let grudges = sim.grudges();
        let before: Vec<_> =
            grudges.into_iter().map(|(holder, foe)| (holder, foe, sim.trait_of(holder, "vengeance").unwrap())).collect();
        sim.run(200);
        let mut hardened = 0;
        for (holder, foe, v0) in before {
            if !sim.is_alive(foe) {
                let v1 = sim.trait_of(holder, "vengeance").expect("the killer still lives");
                assert!(v1 > v0, "breaking the taboo should harden the killer ({v0:.3} -> {v1:.3})");
                hardened += 1;
            }
        }
        assert!(hardened >= 1, "at least one avenger should have killed — and been hardened by it");
    }

    #[test]
    fn a_vassal_avenges_a_slain_lord() {
        // A duty of vengeance and a vassal who inherits his lord's quarrel. The kill
        // taboo would stay a mild vassal's hand — but an *obligation* to avenge (the
        // more specific norm) overrides the taboo and drives him to it. An aggressor
        // is no one's foe until he sheds blood, so a *dead* aggressor can only mean a
        // vassal ran him down. Strip the duty and the blanket taboo keeps every hand
        // still — no lord falls, no vassal rises, no aggressor dies.
        fn feud_world(norms: Norms) -> Simulation {
            let reg = Registry::bundled();
            let goals = Goals::from_ron(
                r#"[
                    (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                    (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                    (name: "stocked",   condition: Holding(good: Edible, at_least: 8), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
                    (name: "avenge",    condition: Verb(verb: "avenge", target: Foe),
                        appeal: [(input: Deficit,  curve: Linear(m: 0.55, b: 0.0)),
                                 (input: Sanction, curve: Linear(m: -1.0, b: 1.0))]),
                ]"#,
                &reg,
            )
            .unwrap();
            // A small, close-quartered world: a two-stage vengeance chain (aggressor
            // kills lord, then the lord's vassal hunts the aggressor) completes only
            // where the quarry can't wander off and evade — intercepting a *moving*
            // target across an open map is a separate, unsolved problem from the
            // fixed-distance pursuit the planner now handles.
            Simulation::new(Setup {
                width: 18,
                height: 12,
                seed: 2026,
                npcs: 10,
                markets: 3,
                feuds: 1,
                vassals: 1,
                goals,
                norms,
                registry: reg,
                ..Default::default()
            })
        }

        // The aggressors — the only ones who could be avenged against (no one else
        // bears them a grudge), captured before any blood is shed.
        let aggressors = |sim: &mut Simulation| -> Vec<_> { sim.grudges().into_iter().map(|(holder, _)| holder).collect() };
        let dead = |sim: &Simulation, who: &[bevy_ecs::entity::Entity]| who.iter().filter(|&&a| !sim.is_alive(a)).count();

        let reg = Registry::bundled();
        let taboo = r#"(act: "avenge", modality: Forbidden)"#;
        let duty = r#"(act: "avenge", modality: Obliged, weight: 0.5, when: Some(Relation(predicate: "alive", subject: Foe, equals: 1)))"#;

        // With the duty in force: aggressors cut down their lords, and at least one
        // lord's vassal — though no born avenger — runs the killer down in turn.
        let dutiful_norms = Norms::from_ron(&format!("[{taboo}, {duty}]"), &reg).unwrap();
        let mut dutiful = feud_world(dutiful_norms);
        let agg = aggressors(&mut dutiful);
        dutiful.run(200);
        assert!(dead(&dutiful, &agg) >= 1, "a vassal should avenge his slain lord");

        // With only the blanket taboo: no duty to override it, so the peace holds and
        // no aggressor is ever brought to account.
        let taboo_only = Norms::from_ron(&format!("[{taboo}]"), &reg).unwrap();
        let mut peaceful = feud_world(taboo_only);
        let agg = aggressors(&mut peaceful);
        peaceful.run(200);
        assert_eq!(dead(&peaceful, &agg), 0, "the taboo alone keeps the peace");
    }

    #[test]
    fn npcs_move_about_the_world() {
        // The Move step is real: pursuing their plans, NPCs leave the tiles they
        // spawned on (to reach land, markets, or better ground).
        let mut sim = economy(60);
        let mut spawn = sim.npc_positions();
        spawn.sort_by_key(|c| (c.col, c.row));
        sim.run(50);
        let mut now = sim.npc_positions();
        now.sort_by_key(|c| (c.col, c.row));
        assert_ne!(now, spawn, "no NPC ever moved — the planner's Move step isn't taking effect");
    }

    #[test]
    fn trade_does_not_create_money() {
        let mut sim = economy(60);
        let before = sim.total_money();
        sim.run(120);
        // Exact: trade only moves coins; only death removes them.
        assert!(sim.total_money() <= before, "money was created ({before} -> {})", sim.total_money());
    }

    #[test]
    fn economy_is_deterministic() {
        let mut a = Simulation::new(Setup { width: 40, height: 30, seed: 7, npcs: 60, ..Default::default() });
        let mut b = Simulation::new(Setup { width: 40, height: 30, seed: 7, npcs: 60, ..Default::default() });
        a.run(80);
        b.run(80);
        assert_eq!(a.npc_count(), b.npc_count());
        assert_eq!(a.total_money(), b.total_money(), "same seed must give the same economy");
    }

    // --- Tile features ---

    #[test]
    fn the_world_is_stocked_with_features() {
        let a = Simulation::new(Setup { width: 48, height: 36, seed: 7, npcs: 20, ..Default::default() });
        let cat = a.feature_catalog();
        let feats = a.features();
        assert!(feats.total() > 10, "the world should be richly stocked, got {}", feats.total());
        assert!(feats.count_of(cat, Category::Community) > 0, "no settlements formed");
        let to_explore = feats.count_of(cat, Category::Ruin) + feats.count_of(cat, Category::Wilderness);
        assert!(to_explore > 0, "no ruins or wonders to explore");

        // Deterministic: a second identical build places byte-identical features.
        let b = Simulation::new(Setup { width: 48, height: 36, seed: 7, npcs: 20, ..Default::default() });
        let va: Vec<_> = a.features().iter().map(|(i, f)| (i, f.kind, f.discovered)).collect();
        let vb: Vec<_> = b.features().iter().map(|(i, f)| (i, f.kind, f.discovered)).collect();
        assert_eq!(va, vb, "feature placement must be deterministic across builds");
    }

    #[test]
    fn settlements_host_the_markets() {
        // With the wiring on, every market is seated in a community feature rather
        // than scattered on raw fertility — the economy hangs off the world's towns.
        let mut sim = Simulation::new(Setup {
            width: 48,
            height: 36,
            seed: 11,
            warmup: 200,
            npcs: 24,
            markets: 6,
            markets_on_settlements: true,
            ..Default::default()
        });
        let community_idx: std::collections::HashSet<usize> = {
            let cat = sim.feature_catalog();
            sim.features().iter().filter(|(_, f)| cat.def(f.kind).category == Category::Community).map(|(i, _)| i).collect()
        };
        let market_tiles: Vec<Coord> = {
            let mut q = sim.world.query::<(&Position, &Market)>();
            q.iter(&sim.world).map(|(p, _)| p.0).collect()
        };
        assert!(!market_tiles.is_empty(), "no markets were seated");
        let topo = sim.substrate().topology();
        for c in market_tiles {
            assert!(community_idx.contains(&topo.index_of(c)), "a market was seated off any settlement at {c:?}");
        }
    }

    #[test]
    fn features_advertise_affordances() {
        // The bundled catalog gives wonders smart-object actions (forage, shelter,
        // bathe), so a world is stocked with places agents can *use*, not just see.
        let a = Simulation::new(Setup { width: 48, height: 36, seed: 7, npcs: 20, ..Default::default() });
        assert!(!a.affordances().is_empty(), "no feature advertised an affordance");
        let b = Simulation::new(Setup { width: 48, height: 36, seed: 7, npcs: 20, ..Default::default() });
        assert_eq!(a.affordances().len(), b.affordances().len(), "affordance build must be deterministic");
    }

    #[test]
    fn worked_sites_stay_within_bounds() {
        // Depletion (use draws a site down) and regeneration (the land refills it)
        // keep every site's remaining within [0, capacity] over a long run.
        let mut sim = Simulation::new(Setup { width: 40, height: 30, seed: 5, npcs: 50, warmup: 200, ..Default::default() });
        sim.run(120);
        for s in sim.affordances() {
            assert!(s.remaining >= 0.0, "affordance went negative ({})", s.remaining);
            assert!(s.capacity == 0 || s.remaining <= s.capacity as f32, "affordance exceeded capacity");
        }
    }

    // --- Factions ---

    #[test]
    fn factions_form_around_courts_and_consolidate() {
        // Seat people in settlements (where the host-courts are), so blocs can form.
        let mut sim = Simulation::new(Setup {
            width: 48,
            height: 36,
            seed: 7,
            warmup: 200,
            npcs: 70,
            markets: 6,
            markets_on_settlements: true,
            ..Default::default()
        });
        sim.run(40); // a couple of faction turns
        assert!(!sim.factions().is_empty(), "no factions formed around the courts");

        sim.run(220); // many more turns — loyalty follows power, so blocs consolidate
        let factions: Vec<_> = sim.factions().to_vec();
        assert!(!factions.is_empty(), "the factions all collapsed");
        // Every surviving bloc clears the threshold and has its stats and a live head.
        for f in &factions {
            assert!(f.members.len() >= 3 && f.force > 0.0, "a faction held below threshold: {f:?}");
            assert!(f.head().is_some_and(|l| sim.is_alive(l)), "a faction's leader is dead but it persists");
        }
        // Preferential attachment should have grown a dominant bloc past the minimum.
        let largest = factions.iter().map(|f| f.members.len()).max().unwrap();
        assert!(largest > 3, "expected a dominant bloc to consolidate, largest = {largest}");
    }

    #[test]
    fn faction_leaders_grow_rich_on_tribute() {
        // Factions *act*: each turn members pay their leader tribute. That coin flows
        // up (never created — total money can only fall, as deaths remove it), leaving
        // at least one leader richer than its faction's average member.
        let cfg = FactionConfig { tax_rate: 0.12, ..Default::default() };
        let mut sim = Simulation::new(Setup {
            width: 48,
            height: 36,
            seed: 7,
            warmup: 200,
            npcs: 70,
            markets: 6,
            markets_on_settlements: true,
            faction_cfg: cfg,
            ..Default::default()
        });
        let before = sim.total_money();
        sim.run(260);
        assert!(sim.total_money() <= before, "tribute must not create money ({before} -> {})", sim.total_money());

        let factions: Vec<_> = sim.factions().to_vec();
        assert!(!factions.is_empty(), "no factions formed to tax");
        let leader_richer = factions
            .iter()
            .any(|f| f.head().and_then(|l| sim.money_of(l)).is_some_and(|m| m > f.wealth / f.members.len() as i64));
        assert!(leader_richer, "tribute should leave a leader above its faction's average wealth");
    }

    #[test]
    fn government_follows_the_court_kind() {
        // A faction's government is set by the kind of court it forms around — a guild
        // is an oligarchy, a temple a democracy, a royal seat a monarchy. (Which kinds
        // actually form factions varies by world; here we check the rule itself.)
        let mut sim =
            Simulation::new(Setup { width: 48, height: 36, seed: 7, warmup: 200, npcs: 70, markets: 6, markets_on_settlements: true, ..Default::default() });
        sim.run(120);
        let cat = sim.feature_catalog();
        let factions = sim.factions();
        assert!(!factions.is_empty(), "no factions formed");
        for f in factions {
            let court = sim
                .features_at(f.seat)
                .iter()
                .find_map(|feat| (cat.def(feat.kind).category == Category::Court).then(|| cat.name(feat.kind)))
                .expect("a faction sits on a court");
            let expected = match court {
                "guild" | "thieves_guild" => Government::Oligarchy,
                "temple" | "druid_circle" => Government::Democracy,
                _ => Government::Monarchy,
            };
            assert_eq!(f.government, expected, "a {court} should be a {expected:?}");
        }
    }

    #[test]
    fn people_can_hold_multiple_factions() {
        // Membership is no longer exclusive: someone within reach of two courts belongs
        // to both.
        let mut sim =
            Simulation::new(Setup { width: 48, height: 36, seed: 7, warmup: 200, npcs: 70, markets: 6, markets_on_settlements: true, ..Default::default() });
        sim.run(120);
        let mut q = sim.world.query_filtered::<&Allegiance, With<Npc>>();
        let multi = q.iter(&sim.world).any(|a| a.0.len() >= 2);
        assert!(multi, "no one belonged to more than one faction");
    }

    #[test]
    fn war_makes_rivals_exclusive_and_costs_lives() {
        // With a low war threshold, an out-sized faction makes war on a weaker neighbour:
        // they become mutually exclusive (a law), and the war takes lives.
        let cfg = FactionConfig { war_force_ratio: 1.1, ..Default::default() };
        let mut sim = Simulation::new(Setup {
            width: 48,
            height: 36,
            seed: 7,
            warmup: 200,
            npcs: 80,
            markets: 8,
            markets_on_settlements: true,
            faction_cfg: cfg,
            ..Default::default()
        });
        let before = sim.npc_count();
        // The richer feature catalog reshuffles where settlements (and so factions) sit, so an
        // out-sized faction takes a little longer to form and strike. Track war and the
        // exclusion law as they arise over a season rather than sampling one fixed tick.
        let mut warred = false;
        let mut excluded = false;
        for _ in 0..400 {
            sim.run(1);
            let factions = sim.factions();
            warred |= factions.iter().any(|f| !f.at_war.is_empty());
            excluded |= factions.iter().any(|f| f.laws.iter().any(|l| matches!(l, Law::Exclude(_))));
        }
        assert!(warred, "no faction ever went to war");
        assert!(excluded, "war should impose mutual exclusion");
        assert!(sim.npc_count() < before, "war (and enforcement) should have cost lives ({before} -> {})", sim.npc_count());
    }

    #[test]
    fn faction_war_dispatches_champions() {
        // Command: a faction at war sets its keenest member on the enemy's leader — a
        // grudge granted through the ordinary avenge machinery. With no feuds seeded, any
        // grudge that appears is a war champion a faction drafted.
        let cfg = FactionConfig { war_force_ratio: 1.1, ..Default::default() };
        let mut sim = Simulation::new(Setup {
            width: 48,
            height: 36,
            seed: 7,
            warmup: 200,
            npcs: 80,
            markets: 8,
            markets_on_settlements: true,
            faction_cfg: cfg,
            ..Default::default()
        });
        // Poll: a champion is drafted with a grudge, but if its own faction forbids
        // killing its enforcers soon jail or execute it — so we catch the draft as it
        // happens rather than expecting the grudge to survive to the end.
        let mut warred = false;
        let mut drafted = false;
        // A longer window: the flooded feature catalog moves the settlements factions form
        // around, so the out-sized faction (and the champion it drafts) takes longer to appear.
        for _ in 0..400 {
            sim.run(1);
            warred |= sim.factions().iter().any(|f| !f.at_war.is_empty());
            if !sim.grudges().is_empty() {
                drafted = true;
                break;
            }
        }
        assert!(warred, "no war broke out");
        assert!(drafted, "war should have drafted a champion (a grudge against a rival)");
    }

    #[test]
    fn people_form_opinions_of_their_leaders() {
        // The opinion graph: serving a leader (and warring on rivals) leaves people with
        // real, directed opinions of the figures they have dealt with.
        let mut sim =
            Simulation::new(Setup { width: 48, height: 36, seed: 7, warmup: 200, npcs: 70, markets: 6, markets_on_settlements: true, ..Default::default() });
        sim.run(140);
        let mut q = sim.world.query_filtered::<&Opinion, With<Npc>>();
        let formed = q.iter(&sim.world).any(|o| o.0.values().any(|&v| v.abs() > 0.05));
        assert!(formed, "no one formed an opinion of a leader");
    }

    #[test]
    fn a_no_kill_faction_jails_the_vengeful() {
        // Grudge-bearers (feuds give them a `Grievance`) who fall under a law-abiding
        // faction's no-kill law are detained by its enforcers — so over a run, someone
        // ends up in the cells.
        let mut sim = Simulation::new(Setup {
            width: 48,
            height: 36,
            seed: 7,
            warmup: 200,
            npcs: 70,
            markets: 6,
            markets_on_settlements: true,
            feuds: 8,
            ..Default::default()
        });
        let mut ever_detained = false;
        for _ in 0..200 {
            sim.run(1);
            if sim.detained_count() > 0 {
                ever_detained = true;
                break;
            }
        }
        assert!(ever_detained, "a no-kill faction should have jailed a grudge-bearing member");
    }

    #[test]
    fn factions_are_deterministic() {
        let build = || {
            let mut s = Simulation::new(Setup {
                width: 48, height: 36, seed: 7, warmup: 200, npcs: 60, markets: 6, markets_on_settlements: true,
                ..Default::default()
            });
            s.run(80);
            s.factions().iter().map(|f| (f.seat.col, f.seat.row, f.members.len())).collect::<Vec<_>>()
        };
        assert_eq!(build(), build(), "same seed must give the same factions");
    }

    // --- Observer / V&V ---

    #[test]
    fn invariants_hold_over_a_long_run() {
        // The verification harness: across a long simulation, no step may create
        // money, grow the population, lose affordance uses, or price a good out of
        // band. A trip would mean a bug wearing the costume of emergence.
        let mut sim = Simulation::new(Setup { width: 48, height: 36, seed: 9, npcs: 80, warmup: 200, ..Default::default() });
        let mut prev = sim.census();
        for _ in 0..150 {
            sim.run(1);
            let now = sim.census();
            let violations = check(&prev, &now);
            assert!(violations.is_empty(), "invariant broken at tick {}: {violations:?}", now.tick);
            prev = now;
        }
        // And the run was a real economy: specialists meant more than one trade is
        // actually practised (the emergent division of labour).
        assert!(prev.trades_in_use() >= 2, "expected a division of labour, professions = {:?}", prev.professions);
    }

    #[test]
    fn the_census_reads_the_world() {
        let mut sim = Simulation::new(Setup { width: 48, height: 36, seed: 3, npcs: 30, ..Default::default() });
        let c = sim.census();
        assert_eq!(c.population, 30, "census should count the living");
        assert!(c.money > 0 && c.markets > 0, "an economy should have coins and markets");
        assert!(c.features > 0 && c.affordance_sites > 0, "the world should be stocked and afford actions");
    }

    #[test]
    fn npcs_uncover_what_they_walk_over() {
        // The discovery system's invariant: after a tick, no Landmark or Hidden
        // feature stays latent on a hex an NPC occupies — a turn there finds it.
        // (Secrets need more than presence, so they may remain.)
        let mut sim = Simulation::new(Setup { width: 40, height: 30, seed: 5, warmup: 200, npcs: 60, ..Default::default() });
        sim.run(120);
        let occupied: Vec<Coord> = {
            let mut q = sim.world.query_filtered::<&Position, With<Npc>>();
            q.iter(&sim.world).map(|p| p.0).collect()
        };
        let cat = sim.feature_catalog();
        let feats = sim.features();
        let topo = sim.substrate().topology();
        for c in occupied {
            for f in feats.at_index(topo.index_of(c)) {
                let tier = cat.def(f.kind).discovery;
                if matches!(tier, Discovery::Landmark | Discovery::Hidden) {
                    assert!(f.discovered, "an NPC stands on an undiscovered {tier:?} feature");
                }
            }
        }
    }

    // --- Narrative director ---

    /// A society seated in its settlements (so factions — and the political register —
    /// form around the protagonist), with a throne the ambitious vie for. The director
    /// stages its story over *this* social world.
    fn staged(director: bool) -> Simulation {
        staged_seeded(director, 11)
    }

    /// As [`staged`], but with an explicit world seed — so an emergent property can be
    /// checked across a few worlds rather than demanded of one fixed seed.
    fn staged_seeded(director: bool, seed: u64) -> Simulation {
        let reg = Registry::bundled();
        let goals = throne_goals(&reg);
        Simulation::new(Setup {
            width: 44,
            height: 32,
            seed,
            warmup: 200,
            npcs: 60,
            markets: 6,
            markets_on_settlements: true,
            throne: true,
            ambitious: 6,
            goals,
            registry: reg,
            director,
            // A brisker cadence so a story of many varied beats is told without a
            // marathon run (keeps the test suite quick).
            director_cfg: DirectorConfig { beat_interval: 9, ..Default::default() },
            ..Default::default()
        })
    }

    #[test]
    fn the_director_sleeps_unless_woken() {
        // Off by default: no protagonist, no beats told, no gratuitous suffering — a
        // world before this layer existed.
        let mut sim =
            Simulation::new(Setup { width: 48, height: 36, seed: 11, warmup: 200, npcs: 40, ..Default::default() });
        sim.run(80);
        assert!(sim.protagonist().is_none(), "no protagonist should be tagged when the director sleeps");
        assert_eq!(sim.director_beats_fired(), 0, "a sleeping director tells no story");
        assert_eq!(sim.gratuitous_total(), 0.0, "a sleeping director authors no suffering");
    }

    #[test]
    fn the_director_tells_a_varied_story() {
        // Woken, the director identifies and stages beats for its protagonist — and the
        // novelty pressure keeps it *varying* them, so the same situation isn't told
        // twice running. A real, diverse tale, and one that costs the world.
        let mut sim = staged(true);
        sim.run(200);
        let told = sim.director_beats_fired();
        let distinct = sim.director_distinct_beats();
        assert!(told >= 8, "the director should have told a story of several beats (told {told})");
        assert!(distinct >= 5, "the story should be varied, not one beat on repeat (distinct {distinct}/{told})");
        assert!(sim.gratuitous_total() > 0.0, "telling its story should author gratuitous suffering");
    }

    #[test]
    fn the_director_manipulates_people_factions_and_the_world() {
        // The point of the rebuild: Γ works the *social fabric*, not just the land. Over
        // a run with no feuds seeded, any grudge is one the director engineered (people);
        // its beat log reaches the political register (factions) and a disaster (world).
        let mut sim = staged(true);
        sim.run(240);
        let manufactured_grudges = !sim.grudges().is_empty();
        let log: Vec<&str> = sim.director_log().iter().map(|(_, id)| id.as_str()).collect();
        let touched_world = log.iter().any(|id| matches!(*id, "the_famine" | "the_long_winter" | "a_loved_one_falls"));
        let touched_politics =
            log.iter().any(|id| matches!(*id, "the_drums_of_war" | "the_faction_turns_persecutor" | "the_mob_demands_blood" | "a_defection_sown"));
        assert!(manufactured_grudges, "the director should have manufactured grudges between people");
        assert!(touched_world, "the director should have struck the world (a disaster), not only people");
        assert!(touched_politics, "the director should have worked the faction layer (a political beat)");
    }

    /// A *freed* world — the configuration the player would reach through ordinary life
    /// (no special verbs): provisioned (deep larders), forgiving (a no-kill norm its
    /// avenge goal heeds, so manufactured grudges stay the hand), stateless (scattered,
    /// so few factions), and unthroned (no coveted prize). The director is fully awake,
    /// its whole library intact — it simply finds no drama worth telling.
    fn freed_world() -> Simulation {
        let reg = Registry::bundled();
        let goals = Goals::from_ron(
            r#"[
                (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                (name: "stocked",   condition: Holding(good: Edible, at_least: 12), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
                (name: "solvent",   condition: Money(at_least: 200),      appeal: [(input: Deficit, curve: Linear(m: 0.5, b: 0.0))]),
                (name: "avenge",    condition: Verb(verb: "avenge", target: Foe),
                    appeal: [(input: Deficit,  curve: Linear(m: 0.55, b: 0.0)),
                             (input: Sanction, curve: Linear(m: -1.0, b: 1.0))]),
            ]"#,
            &reg,
        )
        .unwrap();
        let norms = Norms::from_ron(r#"[(act: "avenge", modality: Forbidden)]"#, &reg).unwrap();
        Simulation::new(Setup {
            width: 44,
            height: 32,
            seed: 11,
            warmup: 200,
            npcs: 40,
            markets: 6,
            markets_on_settlements: false,                                  // scattered
            throne: false,                                                  // no coveted prize
            ambitious: 0,
            feuds: 0,
            initial_food: 30,                                               // deep larders — surplus
            initial_market_stock: 80,
            faction_cfg: FactionConfig { period: 0, ..Default::default() }, // stateless — no blocs to set alight
            goals,
            norms,
            registry: reg,
            director: true,
            director_cfg: DirectorConfig { beat_interval: 9, ..Default::default() },
            ..Default::default()
        })
    }

    #[test]
    fn a_freed_world_quiets_the_omnipotent_director() {
        // §1.1 / the Gödel point: there is no off-switch. The *same* fully-armed director,
        // set loose on two worlds. In a harsh, ambitious, hungry, factional one it runs
        // riot. In a provisioned, forgiving, stateless one it surveys, finds nothing above
        // its impact floor — no vengeful foe, no hungry belly, no faction to set alight —
        // and falls silent. The world's state, reached by ordinary life, is the freedom.
        let mut volatile = staged(true);
        volatile.run(220);
        let v_grat = volatile.gratuitous_total();
        let v_told = volatile.director_beats_fired();

        let mut freed = freed_world();
        freed.run(220);
        let f_grat = freed.gratuitous_total();
        let f_told = freed.director_beats_fired();

        assert!(v_grat > 0.0 && v_told > 5, "the harsh world should feed the director (told {v_told}, grat {v_grat:.0})");
        assert!(
            f_grat < 0.3 * v_grat,
            "a freed world should quiet the director: gratuitous {f_grat:.0} vs {v_grat:.0} (beats {f_told} vs {v_told})"
        );
        assert!(freed.npc_count() > 0, "and the freed world lives on — its own life, uncharged");
    }

    #[test]
    fn stories_chain_into_arcs() {
        // A story is many storylets: chained beats key off qualities prior beats leave
        // behind (a crowning's greed → a tyrant's reign → a revolt; a loss's grief →
        // vengeance → a vendetta). Over a run, at least one such *chained* beat is told —
        // proof the storylets compose rather than fire in isolation.
        let mut sim = staged(true);
        sim.run(360);
        let log: std::collections::HashSet<&str> = sim.director_log().iter().map(|(_, id)| id.as_str()).collect();
        let chained = [
            "the_tyrants_reign",
            "the_revolt",
            "grief_hardens_into_vengeance",
            "the_vendetta",
            "the_succession_crisis",
            "war_weariness",
        ];
        assert!(
            chained.iter().any(|id| log.contains(id)),
            "no chained beat was reached — storylets aren't composing (told: {:?})",
            log
        );
    }

    #[test]
    fn the_director_is_deterministic() {
        // Same seed → the identical story, beat for beat, and the identical moral cost.
        let run = || {
            let mut s = staged(true);
            s.run(150);
            (s.gratuitous_total(), s.director_log().to_vec())
        };
        assert_eq!(run(), run(), "same seed must tell the same story");
    }

    #[test]
    fn the_director_grooms_threads_and_targets_its_audience() {
        // The v2 machinery: the director runs threads through a groom → climax → fall arc
        // (decision #12), and **manufactures the audience's attachment on purpose** — it
        // grooms a figure's prominence far past mere presence so a later reversal pays
        // (decision #15). And it stages *experience*, not only suffering (decision #8):
        // the brighter registers it authors lift `staged` above the suffering-only total.
        let mut sim = staged(true);
        sim.run(320);

        // The arc moves through its phases — the cadence is not a flat sequence.
        let phases: std::collections::HashSet<Phase> = sim.director_cadence().iter().map(|c| c.phase).collect();
        assert!(
            phases.len() >= 3 && phases.contains(&Phase::Setup),
            "threads should groom (Setup) and move through several phases, saw {phases:?}"
        );

        // Attachment is *manufactured*: the protagonist's prominence is groomed far past
        // the bare seed (0.6) and presence trickle — the audience is made to invest.
        let proto = sim.protagonist().expect("a protagonist is staged for");
        assert!(
            sim.director_prominence(proto) > 2.0,
            "the director should have manufactured the protagonist's prominence (got {:.2})",
            sim.director_prominence(proto)
        );

        // Staged experience counts the joy too, so it exceeds the suffering-only total.
        let (staged, grat) = (sim.director_staged_total(), sim.gratuitous_total());
        assert!(grat > 0.0, "the season should author some suffering");
        assert!(staged > grat, "staged experience (joy + suffering) should exceed suffering alone ({staged:.0} vs {grat:.0})");
    }

    #[test]
    fn betrayal_dominates_emergently_and_climaxes_collide() {
        // The trunk is betrayal → vengeance, and it should dominate the season **because
        // it scores highest** (decision #17), never by a hard rule — and the director
        // **times climaxes onto highs**, colliding a reversal with a manufactured high
        // (decision #14: the beloved dies at the wedding). Both are emergent, so this
        // needs a season long enough to play out.
        let mut sim = staged(true);
        sim.run(480);

        let mut counts: std::collections::HashMap<Register, usize> = std::collections::HashMap::new();
        for c in sim.director_cadence() {
            *counts.entry(c.register).or_insert(0) += 1;
        }
        let betrayal = counts.get(&Register::Betrayal).copied().unwrap_or(0);
        let top = counts.values().copied().max().unwrap_or(0);
        assert!(betrayal > 0 && betrayal == top, "betrayal should top the season's registers (got {counts:?})");

        // A **collision** — a climax timed onto a high (the beloved dies at the wedding) — is
        // doubly emergent: a thread must reach its climax *and* the protagonist be up at that
        // moment. Whether it happens in any one season is sensitive to the exact world; that the
        // director *can and does* engineer them is the real claim. So verify the capability
        // across a handful of seeded worlds rather than demanding it of one fixed seed.
        let collided = (0..4u64).any(|i| {
            let mut s = staged_seeded(true, 11 + i * 13);
            s.run(600);
            s.director_cadence().iter().any(|c| c.collision)
        });
        assert!(collided, "the director should time a climax onto a high in at least one season (a collision)");
    }

    // --- Emergent dialogue ---

    /// A peopled world (souls cluster in settlements and talk) with the director stirring
    /// the drama their talk then voices.
    fn chatty(seed: u64) -> Simulation {
        let reg = Registry::bundled();
        let goals = throne_goals(&reg);
        Simulation::new(Setup {
            width: 44,
            height: 32,
            seed,
            warmup: 200,
            npcs: 50,
            markets: 6,
            markets_on_settlements: true,
            throne: true,
            ambitious: 6,
            goals,
            registry: reg,
            director: true,
            director_cfg: DirectorConfig { beat_interval: 9, ..Default::default() },
            dialogue: true,
            dialogue_cfg: DialogueConfig::default(),
            ..Default::default()
        })
    }

    #[test]
    fn dialogue_sleeps_unless_woken() {
        // Off by default: not a word spoken, the run unchanged from before this layer.
        let mut sim =
            Simulation::new(Setup { width: 44, height: 32, seed: 11, warmup: 200, npcs: 40, ..Default::default() });
        sim.run(60);
        assert_eq!(sim.dialogue_count(), 0, "a sleeping dialogue layer speaks nothing");
    }

    #[test]
    fn emergent_dialogue_reflects_the_social_state() {
        // Speaking is acting: the words emerge from traits, mood, opinion, and grudges —
        // so a dramatic, peopled world speaks a *varied* tongue, grievance beside warmth,
        // every line grounded in who is speaking and to whom.
        let mut sim = chatty(11);
        sim.run(220);
        assert!(sim.dialogue_count() > 20, "a peopled, dramatic world should speak (said {})", sim.dialogue_count());
        let acts: std::collections::HashSet<SpeechAct> = sim.dialogue_log().iter().map(|u| u.act).collect();
        assert!(acts.len() >= 4, "the talk should be varied, saw {acts:?}");
        let grievance = sim.dialogue_log().iter().any(|u| matches!(u.act, SpeechAct::Accuse | SpeechAct::Threaten));
        let warmth = sim
            .dialogue_log()
            .iter()
            .any(|u| matches!(u.act, SpeechAct::Greet | SpeechAct::Confide | SpeechAct::Console | SpeechAct::Praise));
        assert!(grievance && warmth, "grievance and warmth should both emerge (grievance {grievance}, warmth {warmth})");
        assert!(
            sim.dialogue_log().iter().any(|u| u.surface.contains(&u.listener_name)),
            "the words are grounded — a line names the soul it is spoken to"
        );
    }

    #[test]
    fn the_player_chooses_their_own_words() {
        // This is a role-playing game: the *player* is the avatar's mind. The avatar carries
        // no personality, mood, or opinion — and the sim does not score what it "wants" to
        // say. The player is offered the whole repertoire and chooses; the sim only renders
        // the words and visits the consequence on the soul addressed.
        let mut sim = chatty(11);
        sim.run(120);
        sim.spawn_player(None); // the avatar lands where the people are — a soul stands near
        let avatar = sim.player_avatar().expect("an avatar is in the world");
        let (npc, _) = sim.player_nearby_npcs().first().copied().expect("a soul stands within reach");

        // The menu is the full repertoire — the player may attempt any verb, attributes or no.
        let menu = sim.player_intents();
        assert_eq!(menu.len(), dialogue::IntentBook::bundled().0.len(), "the player is offered every verb");

        // The avatar has no opinion of this NPC; an attribute-scored mind would never *want*
        // to accuse out of nowhere — but the player can choose to. The choice drives the act.
        let before = sim.dialogue_count();
        let (line, _reply) = sim.player_talk(npc, "an_accusation").expect("the avatar speaks the chosen line");
        assert_eq!(line.speaker, avatar, "the player spoke as themselves");
        assert_eq!(line.act, SpeechAct::Accuse, "the player's *choice* set the act, not the avatar's mood");
        assert!(line.surface.contains(&line.listener_name), "the words name the soul addressed: {:?}", line.surface);
        assert!(sim.dialogue_count() > before, "the line joins the conversation");

        // And it cost exactly the turn-based price elsewhere proven: the world moved on.
        assert!(!sim.player_traveling(), "speaking is its own action, not a journey");
    }

    #[test]
    fn the_player_conversation_is_deterministic() {
        // Same seed + same choices → the same exchange, word for word (the surface is seeded;
        // the consequence is canon).
        let run = || {
            let mut s = chatty(11);
            s.run(120);
            s.spawn_player(None);
            let npc = s.player_nearby_npcs().first().copied().expect("a soul stands near").0;
            let (line, reply) = s.player_talk(npc, "a_greeting").expect("the avatar greets");
            (line.surface, reply.map(|r| r.surface))
        };
        assert_eq!(run(), run(), "the same words, chosen the same way, render the same");
    }

    #[test]
    fn the_director_voices_its_betrayals() {
        // The thematic payoff: a betrayal Γ engineers is *heard*, not merely tallied — a
        // forced utterance, the friend renouncing the protagonist aloud.
        let mut sim = chatty(11);
        sim.run(300);
        assert!(
            sim.dialogue_log().iter().any(|u| u.forced),
            "the director should have put words in a betrayer's mouth"
        );
    }

    #[test]
    fn dialogue_is_deterministic() {
        // Same seed → the same conversation, word for word.
        let run = || {
            let mut s = chatty(11);
            s.run(120);
            s.dialogue_log().iter().map(|u| u.surface.clone()).collect::<Vec<_>>()
        };
        assert_eq!(run(), run(), "same seed must speak the same words");
    }

    // --- Exploration (the player avatar) ---

    /// A peopled, settled world for a body to walk through.
    fn walker(seed: u64) -> Simulation {
        let reg = Registry::bundled();
        let goals = throne_goals(&reg);
        Simulation::new(Setup {
            width: 48,
            height: 36,
            seed,
            warmup: 150,
            npcs: 40,
            markets: 6,
            markets_on_settlements: true,
            goals,
            registry: reg,
            ..Default::default()
        })
    }

    /// A reachable tile a few hexes east of `from` to set out for.
    fn nearby_target(sim: &mut Simulation, from: Coord) -> Option<Coord> {
        (1..16).map(|d| Coord::new(from.col + d, from.row)).find(|&c| sim.player_travel_to(c))
    }

    #[test]
    fn the_world_runs_with_no_player() {
        // Off by default: no avatar, no fog lifted, the run as it was before this layer.
        let mut sim = walker(7);
        sim.run(30);
        assert!(sim.player_position().is_none(), "no avatar without spawning one");
        assert!(sim.player_view().is_none(), "nothing to look through");
        assert_eq!(sim.player_explored_count(), 0, "no avatar, no map revealed");
    }

    #[test]
    fn the_player_explores_and_lifts_the_fog() {
        // The avatar spawns, sees its surroundings, walks a route over land, and reveals
        // more of the map as it goes — discovering what it passes (the same way NPCs do).
        let mut sim = walker(7);
        sim.spawn_player(None);
        let start = sim.player_position().expect("an avatar was placed");
        let seen0 = sim.player_explored_count();
        assert!(seen0 > 0, "spawning reveals the immediate surroundings");

        let target = nearby_target(&mut sim, start).expect("some nearby land is reachable on foot");
        let mut day = 0;
        while sim.player_traveling() && day < 200 {
            sim.step();
            day += 1;
        }
        assert_eq!(sim.player_position(), Some(target), "the avatar reaches where it set out for");
        assert!(
            sim.player_explored_count() > seen0,
            "walking should lift more fog ({seen0} -> {})",
            sim.player_explored_count()
        );
        let v = sim.player_view().expect("the avatar can look around");
        assert_eq!(v.pos, target, "and the look view is centred on it");
    }

    #[test]
    fn the_player_can_wait() {
        // Waiting is the second action: it passes exactly one tick where the avatar stands —
        // the world lives a moment on, but the avatar does not move. And it is a body's act:
        // with no avatar there is no one to wait, so the world is left untouched.
        let mut sim = walker(7);
        let day_no_player = sim.tick();
        assert!(!sim.player_wait(), "no avatar — no one to wait");
        assert_eq!(sim.tick(), day_no_player, "a failed wait advances nothing");

        sim.spawn_player(None);
        let here = sim.player_position().expect("an avatar was placed");
        let seen0 = sim.player_explored_count();
        let day0 = sim.tick();

        assert!(sim.player_wait(), "a spawned avatar can wait");
        assert_eq!(sim.player_position(), Some(here), "waiting does not move the avatar");
        assert_eq!(sim.tick(), day0 + 1, "waiting advances the world exactly one tick");
        assert_eq!(sim.player_explored_count(), seen0, "standing still reveals no new ground");
        assert!(!sim.player_traveling(), "waiting sets no journey");
    }

    #[test]
    fn the_player_cannot_walk_on_water() {
        // A body is a body: it cannot route across the sea.
        let mut sim = walker(7);
        sim.spawn_player(None);
        let water = {
            let gw = sim.substrate();
            let (topo, sea) = (gw.topology(), gw.params().sea_level);
            (0..topo.len()).map(|i| topo.coord(i)).find(|&c| gw.elevation(c) < sea)
        };
        if let Some(w) = water {
            assert!(!sim.player_travel_to(w), "there is no walking route onto the ocean");
        }
    }

    #[test]
    fn exploration_is_deterministic() {
        // Same seed → the same journey and the same revealed map.
        let run = || {
            let mut s = walker(7);
            s.spawn_player(None);
            let start = s.player_position().unwrap();
            let target = nearby_target(&mut s, start).unwrap();
            let mut day = 0;
            while s.player_traveling() && day < 200 {
                s.step();
                day += 1;
            }
            (s.player_position(), s.player_explored_count(), target)
        };
        assert_eq!(run(), run(), "same seed must walk the same path and reveal the same map");
    }
}
