//! The **narrative director** `Γ` — a multi-thread drama manager over the living world
//! (`docs/narrative_director.md`, `docs/narrative_director_v2.md`).
//!
//! Built on how real drama managers work — **Façade** (Mateas & Stern) sequences *beats*
//! to shape an arc and lets reactive agents enact them; **quality-based narrative**
//! (Failbetter; Emily Short) selects *storylets* whose **preconditions** the world's
//! qualities currently meet. `Γ` does both over the ECS, and adds the machinery a season
//! of television has: it runs **a few threads at once** ([`Thread`]), each a
//! **groom → climax → fall** arc, and **manufactures the audience's attachment on
//! purpose** so the reversal devastates.
//!
//! The objective is **drama, not tragedy** (decision #8): each beat-interval it scores
//! every *tellable* beat by
//!
//! ```text
//!   score = drama × novelty ÷ resistance,   drama = stakes × attachment × reversal
//! ```
//!
//! - **attachment** — the persistent, manufacturable narrative **[`prominence`](Director::prominence)**
//!   of the cast: how invested the audience is in them. The director grooms a future
//!   victim's prominence in a thread's *Setup* so the later fall pays — *the game makes
//!   you love them on purpose.*
//! - **reversal** — contrast with the protagonist's *current* feeling: a betrayal at the
//!   height of joy scores far above one at a low ebb, so the director **times its
//!   climaxes onto highs** (its own grooming, or — via a **collision** — another
//!   thread's peak: the beloved dies at the wedding).
//! - **resistance** — how far the world must be bent: low where the roles already fit, so
//!   the director nudges where the world *already leans* and its hand stays hidden. **It
//!   never tells a beat the world could not plausibly have produced itself** — the alibi
//!   is the myth; per beat deniable, in aggregate *felt*.
//!
//! Registers **rotate freely**; betrayal dominates because the **trunk** (betrayal →
//! vengeance, self-perpetuating) *scores* highest, never by a rule.
//!
//! The moral arithmetic generalizes from suffering to **staged experience** (decision #8):
//! [`staged_total`](Director::staged_total) counts *all* the emotional life `Γ` authors —
//! joy as well as anguish, suffering weighted heaviest — while
//! [`gratuitous_total`](Director::gratuitous_total) still tracks the suffering alone. The
//! win condition is **authorship → 0**, not sadness → 0; it is the system's internal
//! truth and the endgame, never a shown meter.
//!
//! **There is no off-switch** (decision #5, the Gödel point). `Γ` is omnipotent but a
//! *precondition engine* with an **impact floor**: a world it can find no drama in —
//! provisioned, forgiving, stateless, unthroned — starves its preconditions and it falls
//! silent. The freedom is a property of the world's state, reached by ordinary life.
//!
//! Off by default and deterministic (its one source of variety is a dedicated, seeded
//! [`SplitMix64`] stream).

use crate::beats::{Beat, BeatBook, Effect, Phase, Pre, Register, Role, SLOTS};
use crate::data::Registry;
use crate::dialogue::Dialogue;
use crate::factions::{Allegiance, Factions, Law, Opinion};
use crate::features::{Discovery, FeatureCatalog, Features};
use crate::people::{Grievance, Mood, Needs, Npc, Personality, Throne};
use crate::{Position, Substrate};
use bevy_ecs::prelude::*;
use game_sim::{Coord, SplitMix64, Topology};
use sim::Rng;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

/// The NPC the director stages its drama for — the audience of one. `Γ`'s threads weave
/// around the *player's* accumulated investments (its [`prominence`](Director::prominence)
/// map), of which this avatar is the central, but not the only, figure. On its death the
/// director promotes another and tells on — but the prominence (the audience's
/// attachment) **persists**: the player outlives the avatar.
#[derive(Component, Clone, Copy, Debug)]
pub struct Protagonist;

/// The registers a thread can take as its **spine**. Relief is excluded — it is a *Fall*
/// flavour, not a story's engine. Betrayal/Vengeance are the **trunk**.
const SPINES: [Register; 9] = [
    Register::Betrayal,
    Register::Vengeance,
    Register::Ambition,
    Register::War,
    Register::Disaster,
    Register::Persecution,
    Register::Romance,
    Register::Triumph,
    Register::Wonder,
];

/// Knobs for the drama manager. Off by default, so a director-free world is unchanged.
#[derive(Resource, Clone, Debug)]
pub struct DirectorConfig {
    pub enabled: bool,
    /// Ticks between beats — the cadence at which `Γ` pushes the story onward.
    pub beat_interval: u64,
    /// Hexes around the protagonist a beat's wake is watched (for attribution) and a
    /// marvel/region disaster reaches by default.
    pub reach: i32,

    /// The least **impact** (`drama × cast-fit`) a beat must reach for the director to
    /// tell it. Below this, no castable beat is dramatic enough — so a peaceful,
    /// provisioned, forgiving world `Γ` can find no leverage in falls **silent**. The
    /// knob that makes the *world*, not a button, the thing that quiets the director.
    pub impact_floor: f32,

    /// Novelty heat added to a told beat (and its register/tags), and how fast it cools —
    /// the diversity pressure that keeps the register rotating.
    pub novelty_heat: f32,
    pub novelty_cool: f32,
    /// Sample among the top-`shortlist` scored beats, so the telling varies.
    pub shortlist: usize,

    /// How many threads run at once (decision #13: a few interleaved stories).
    pub max_threads: usize,
    /// Drama multiplier for **trunk** (betrayal/vengeance) beats, so betrayal dominates
    /// *emergently* (decision #17) rather than by a hard rule.
    pub trunk_bonus: f32,
    /// Scoring multipliers: a beat that suits the active thread's phase is favoured; one
    /// that doesn't is damped; one whose register *is* the thread's spine is favoured.
    pub phase_match: f32,
    pub phase_miss: f32,
    pub spine_match: f32,
    /// When a climax is **timed onto another thread's high** (a collision), this bonus,
    /// fired with this probability.
    pub collision_bonus: f32,
    pub collision_chance: f32,

    /// Attachment = `1 + prominence / prom_scale`. The audience's investment, manufactured.
    pub prom_scale: f32,
    /// Prominence trickled to every living soul each interval (mere presence), and the
    /// fraction of all prominence that persists each interval (slow fade, so the
    /// audience's attachment lingers past the avatar's death).
    pub presence_gain: f32,
    pub prominence_decay: f32,
    /// Prominence a beat confers on each cast member (being *featured*), and the extra a
    /// thread's *Setup* grooming confers on its chosen victim — *the game grooms your
    /// affection on purpose.*
    pub feature_gain: f32,
    pub groom_gain: f32,
    /// A prominence floor the protagonist is held to (the avatar is always somewhat the
    /// audience's), and the ceiling all prominence is clamped to.
    pub proto_seed: f32,
    pub prom_cap: f32,
    /// Heat a thread must bank before it ripens to a climax — scaled up by the lead's
    /// prominence, so the most-invested figure gets the longest slow burn (variable
    /// tempo, decision #18).
    pub ripeness_base: f32,

    /// Opinion past which someone casts as an Ally (warm) or a Foe (cold).
    pub ally_threshold: f32,
    pub foe_threshold: f32,
    /// Sustenance/rest below which the protagonist reads as imperilled (ambient readout).
    pub peril: f32,
    /// EMA factor for the ambient tension readout (inspection only; not the objective).
    pub tension_smoothing: f32,
    /// Moral arithmetic: suffering per manufactured wound is scaled by this; grief per
    /// death in a beat's wake; how long that wake is watched; the weight brighter affect
    /// carries in the *staged-experience* total (suffering carries 1.0).
    pub anguish_scale: f32,
    pub grief_per_death: f32,
    pub wake_ttl: u64,
    pub bright_weight: f32,
}

impl Default for DirectorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            beat_interval: 14,
            reach: 3,
            impact_floor: 1.0,
            novelty_heat: 2.0,
            novelty_cool: 0.03,
            shortlist: 3,
            max_threads: 3,
            trunk_bonus: 2.0,
            phase_match: 1.6,
            phase_miss: 0.6,
            spine_match: 1.4,
            collision_bonus: 1.7,
            collision_chance: 0.5,
            prom_scale: 1.5,
            presence_gain: 0.006,
            prominence_decay: 0.985,
            feature_gain: 0.5,
            groom_gain: 0.9,
            proto_seed: 0.6,
            prom_cap: 8.0,
            ripeness_base: 2.0,
            ally_threshold: 0.08,
            foe_threshold: -0.08,
            peril: 25.0,
            tension_smoothing: 0.25,
            anguish_scale: 1.0,
            grief_per_death: 4.0,
            wake_ttl: 24,
            bright_weight: 0.3,
        }
    }
}

/// Sustenance the protagonist is never dropped below by the director's *own* staged
/// disasters — so a famine threatens the lead but doesn't end their story outright (the
/// world's own hunger, and the drama of a foe's knife, still can).
const PROTAGONIST_FLOOR: f32 = 18.0;

/// A beat's lingering wake: the people standing in its shadow when it was told, watched
/// so that any who die before it lifts are charged to the director.
#[derive(Clone, Debug)]
struct Wake {
    expires: u64,
    watched: HashSet<Entity>,
}

/// One of the director's running **stories** — a [`groom → climax → fall`](Phase) arc
/// (decision #12) around a prominent figure. The director runs a few at once, staggered,
/// so one thread's *fall* is the quiet backdrop another's *climax* detonates against.
#[derive(Clone, Debug)]
pub struct Thread {
    pub id: u64,
    /// The thread's dramatic key. The **trunk** (betrayal/vengeance) self-perpetuates.
    pub spine: Register,
    /// The figure the arc centres on (usually, but not only, the protagonist).
    pub lead: Entity,
    /// The pinned counterpart — the bond to break, the rival to topple, the foe to face.
    /// Pinned at spawn so grooming and reversal fall on the *same* figure (continuity).
    pub other: Option<Entity>,
    pub phase: Phase,
    /// Ripeness banked toward the climax.
    pub heat: f32,
    /// Heat needed to ripen — scales with the lead's prominence (the most-invested figure
    /// gets the longest burn).
    pub ripeness: f32,
    pub beats: u32,
    pub climaxed: bool,
    pub is_trunk: bool,
}

/// A single fired beat, with the legible **cadence** it leaves (decision §5): the rhythm
/// — groom → climax → fall — and the prominence→reversal correlation that a suspicious
/// player eventually reads. The director hides each beat behind an alibi but leaves this
/// pattern on purpose: *the player should feel manipulated.*
#[derive(Clone, Debug)]
pub struct Cadence {
    pub tick: u64,
    pub beat: String,
    pub register: Register,
    pub phase: Phase,
    pub thread: u64,
    pub lead_prominence: f32,
    /// This climax was timed onto another thread's high.
    pub collision: bool,
}

/// The narrative director `Γ`, read and written by [`director_step`].
#[derive(Resource)]
pub struct Director {
    /// Ambient dramatic tension (smoothed) around the protagonist — an inspection
    /// readout; the objective no longer follows a tension arc.
    tension: f32,
    last_beat: u64,
    has_fired: bool,
    /// Recency heat per beat id, per tone tag, and per register — the novelty penalty and
    /// the register-rotation pressure.
    id_heat: HashMap<String, f32>,
    tag_heat: HashMap<String, f32>,
    reg_heat: HashMap<Register, f32>,
    /// Beats' wakes awaiting attribution / expiry.
    active: Vec<Wake>,
    /// **Persistent, manufacturable narrative prominence** per soul — the attachment the
    /// audience has in them (decision #15). Survives the avatar's death; grown by being
    /// featured in beats and groomed in a thread's Setup. The director's deepest lever.
    prominence: HashMap<Entity, f32>,
    /// The running stories (decision #13).
    threads: Vec<Thread>,
    next_thread: u64,
    advances: u64,
    /// The director's own randomness — variety without breaking determinism.
    rng: SplitMix64,
    /// Suffering this tick and cumulatively — the anguish `Γ` authors (deaths in its
    /// wakes + the wounds it manufactures). The narrower, suffering-only metric.
    pub gratuitous_now: f32,
    pub gratuitous_total: f64,
    /// **Staged experience** cumulatively — *all* the emotional life `Γ` authors, joy as
    /// well as suffering (suffering weighted heaviest). The win is this → 0 (decision #8).
    pub staged_total: f64,
    /// Ambient tension (for inspection).
    pub tension_now: f32,
    /// The story `Γ` has told: `(tick, beat id)`, in order — the human-readable log.
    pub log: Vec<(u64, String)>,
    /// The legible cadence — the rhythm and prominence→reversal correlation it leaves.
    pub cadence: Vec<Cadence>,
}

impl Director {
    pub fn seeded(seed: u64) -> Self {
        Self {
            tension: 0.0,
            last_beat: 0,
            has_fired: false,
            id_heat: HashMap::new(),
            tag_heat: HashMap::new(),
            reg_heat: HashMap::new(),
            active: Vec::new(),
            prominence: HashMap::new(),
            threads: Vec::new(),
            next_thread: 0,
            advances: 0,
            rng: SplitMix64::new(seed),
            gratuitous_now: 0.0,
            gratuitous_total: 0.0,
            staged_total: 0.0,
            tension_now: 0.0,
            log: Vec::new(),
            cadence: Vec::new(),
        }
    }

    /// How hot (recently-told) a beat is right now — high for a beat, tone, or register
    /// just used, decaying back toward 0. Drives the novelty penalty / register rotation.
    fn heat(&self, beat: &Beat) -> f32 {
        let id = self.id_heat.get(&beat.id).copied().unwrap_or(0.0);
        let tags: f32 = beat.tags.iter().map(|t| self.tag_heat.get(t).copied().unwrap_or(0.0)).sum();
        let reg = self.reg_heat.get(&beat.register).copied().unwrap_or(0.0);
        id + tags + reg
    }

    /// A soul's accumulated narrative prominence (0 if unknown).
    pub fn prominence_of(&self, e: Entity) -> f32 {
        self.prominence.get(&e).copied().unwrap_or(0.0)
    }

    /// The running threads (for inspection / the demo).
    pub fn threads(&self) -> &[Thread] {
        &self.threads
    }
}

impl Default for Director {
    fn default() -> Self {
        Self::seeded(0)
    }
}

/// A living person's qualities, gathered owned so the compute is borrow-free.
struct Cand {
    e: Entity,
    pos: Coord,
    pos_index: usize,
    ambition: f32,
    sociability: f32,
    piety: f32,
    /// This person's opinion *of the protagonist* (warm → Ally, cold → Foe).
    op_of_proto: f32,
    need: f32,
    seats: SmallVec<[Coord; 4]>,
    traits: Vec<f32>,
    moods: Vec<f32>,
    grudge_target: Option<Entity>,
}

/// Resolved mood ids the objective reads, looked up once.
#[derive(Clone, Copy)]
struct MoodIds {
    joy: Option<usize>,
    hope: Option<usize>,
    love: Option<usize>,
    calm: Option<usize>,
    awe: Option<usize>,
    anger: Option<usize>,
    sorrow: Option<usize>,
    fear: Option<usize>,
}

impl MoodIds {
    fn resolve(reg: &Registry) -> Self {
        Self {
            joy: reg.mood_id("joy"),
            hope: reg.mood_id("hope"),
            love: reg.mood_id("love"),
            calm: reg.mood_id("calm"),
            awe: reg.mood_id("awe"),
            anger: reg.mood_id("anger"),
            sorrow: reg.mood_id("sorrow"),
            fear: reg.mood_id("fear"),
        }
    }
    /// How *up* the protagonist currently feels — the height a dark beat reverses.
    fn high(&self, m: &[f32]) -> f32 {
        let g = |i: Option<usize>| i.and_then(|i| m.get(i)).copied().unwrap_or(0.0);
        g(self.joy) + g(self.hope) + g(self.love) + g(self.awe) + 0.5 * g(self.calm)
    }
    /// How *down* the protagonist currently feels — the depth a relief beat reverses.
    fn low(&self, m: &[f32]) -> f32 {
        let g = |i: Option<usize>| i.and_then(|i| m.get(i)).copied().unwrap_or(0.0);
        g(self.anger) + g(self.sorrow) + g(self.fear)
    }
}

/// The hexes within `radius` of `centre`, in deterministic BFS order, plus a membership
/// set of their storage indices and the ordered indices (nearest first).
fn region(topo: &Topology, centre: Coord, radius: i32) -> (Vec<Coord>, HashSet<usize>, Vec<usize>) {
    let start = topo.index_of(centre);
    let mut seen: HashSet<usize> = HashSet::new();
    seen.insert(start);
    let mut order = vec![start];
    let mut frontier = vec![start];
    for _ in 0..radius.max(0) {
        let mut next = Vec::new();
        for &i in &frontier {
            for l in topo.neighbors(i) {
                if seen.insert(l.to) {
                    order.push(l.to);
                    next.push(l.to);
                }
            }
        }
        frontier = next;
    }
    let coords = order.iter().map(|&i| topo.coord(i)).collect();
    (coords, seen, order)
}

/// Try to cast every role a beat needs from the protagonist's social world. The thread's
/// pinned counterpart (`pin`) is seated first into the beat's principal counterpart role,
/// so a victim groomed in *Setup* is the one struck in *Climax* (the manufactured
/// attachment pays off on the *same* figure). Returns the filled slots and a **salience**
/// in `0..1` (cast fit — the inverse of resistance), or `None` if any role can't be cast.
#[allow(clippy::too_many_arguments)]
fn cast_beat(
    beat: &Beat,
    proto: Entity,
    cands: &[Cand],
    factions: &Factions,
    proto_seats: &[Coord],
    cfg: &DirectorConfig,
    pin: Option<Entity>,
) -> Option<([Option<Entity>; SLOTS], f32)> {
    let mut slots: [Option<Entity>; SLOTS] = [None; SLOTS];
    slots[Role::Protagonist.slot()] = Some(proto);
    let mut used: HashSet<Entity> = HashSet::new();
    used.insert(proto);
    let mut fit_sum = 0.0;
    let mut fit_n = 0u32;

    // Seat the pinned counterpart into the beat's principal counterpart role (the first
    // non-protagonist role among the bond-like roles), regardless of fit — continuity is
    // the point. Preconditions still filter, so a pin that doesn't suit is simply not told.
    if let Some(p) = pin.filter(|p| *p != proto && cands.iter().any(|c| c.e == *p))
        && let Some(role) = beat.roles().into_iter().find(|r| {
            matches!(r, Role::Ally | Role::Foe | Role::Rival | Role::Lover | Role::Bystander) && slots[r.slot()].is_none()
        })
    {
        slots[role.slot()] = Some(p);
        used.insert(p);
        fit_sum += 0.7;
        fit_n += 1;
    }

    for role in beat.roles() {
        if slots[role.slot()].is_some() {
            continue;
        }
        let chosen: Option<(Entity, f32)> = match role {
            Role::Protagonist => continue,
            Role::Patron => proto_seats
                .iter()
                .find_map(|s| factions.at(*s).and_then(|f| f.head()))
                .filter(|h| !used.contains(h))
                .map(|h| (h, 1.0)),
            Role::Ally => cands
                .iter()
                .filter(|c| !used.contains(&c.e) && c.op_of_proto > cfg.ally_threshold)
                .max_by(|a, b| a.op_of_proto.partial_cmp(&b.op_of_proto).unwrap().then(a.e.cmp(&b.e)))
                .map(|c| (c.e, c.op_of_proto.clamp(0.0, 1.0))),
            Role::Rival => cands
                .iter()
                .filter(|c| !used.contains(&c.e) && c.ambition > 0.01)
                .max_by(|a, b| a.ambition.partial_cmp(&b.ambition).unwrap().then(a.e.cmp(&b.e)))
                .map(|c| (c.e, c.ambition.clamp(0.0, 1.0))),
            Role::Foe => {
                // Prefer someone who already bears the protagonist a grudge; else the
                // coldest opinion past the foe threshold.
                let by_grudge = cands
                    .iter()
                    .filter(|c| !used.contains(&c.e) && c.grudge_target == Some(proto))
                    .min_by(|a, b| a.e.cmp(&b.e));
                let chosen = by_grudge.or_else(|| {
                    cands
                        .iter()
                        .filter(|c| !used.contains(&c.e) && c.op_of_proto < cfg.foe_threshold)
                        .min_by(|a, b| a.op_of_proto.partial_cmp(&b.op_of_proto).unwrap().then(a.e.cmp(&b.e)))
                });
                chosen.map(|c| (c.e, (-c.op_of_proto).clamp(0.3, 1.0)))
            }
            Role::Lover => cands
                .iter()
                .filter(|c| !used.contains(&c.e) && c.op_of_proto > cfg.foe_threshold)
                .max_by(|a, b| {
                    (a.sociability + a.op_of_proto)
                        .partial_cmp(&(b.sociability + b.op_of_proto))
                        .unwrap()
                        .then(a.e.cmp(&b.e))
                })
                .map(|c| (c.e, ((c.sociability + c.op_of_proto.max(0.0)) * 0.5).clamp(0.2, 1.0))),
            Role::Mentor => cands
                .iter()
                .filter(|c| !used.contains(&c.e))
                .max_by(|a, b| a.piety.partial_cmp(&b.piety).unwrap().then(a.e.cmp(&b.e)))
                .map(|c| (c.e, c.piety.clamp(0.2, 1.0))),
            Role::Bystander => {
                cands.iter().filter(|c| !used.contains(&c.e)).min_by(|a, b| a.e.cmp(&b.e)).map(|c| (c.e, 0.5))
            }
        };
        let (e, fit) = chosen?;
        slots[role.slot()] = Some(e);
        used.insert(e);
        fit_sum += fit;
        fit_n += 1;
    }
    let salience = if fit_n > 0 { fit_sum / fit_n as f32 } else { 1.0 };
    Some((slots, salience))
}

/// The slowly-varying world facts a beat's preconditions are checked against.
struct PreCtx<'a> {
    proto: Entity,
    throne_holder: Option<Entity>,
    proto_in_faction: bool,
    at_war: bool,
    /// Tile indices within the protagonist's reach (for `VictimNearby`).
    region: &'a HashSet<usize>,
}

/// Whether a beat's preconditions hold for a tentative cast.
fn pre_ok(beat: &Beat, slots: &[Option<Entity>; SLOTS], cands: &[Cand], idx_of: &HashMap<Entity, usize>, reg: &Registry, ctx: &PreCtx) -> bool {
    let cand = |e: Entity| idx_of.get(&e).map(|&i| &cands[i]);
    for p in &beat.pre {
        let ok = match p {
            Pre::Exists { who } => slots[who.slot()].is_some(),
            Pre::TraitAtLeast { who, trait_name, v } => slots[who.slot()]
                .and_then(cand)
                .and_then(|c| reg.trait_id(trait_name).and_then(|t| c.traits.get(t).copied()))
                .is_some_and(|val| val >= *v),
            Pre::TraitAtMost { who, trait_name, v } => slots[who.slot()]
                .and_then(cand)
                .and_then(|c| reg.trait_id(trait_name).and_then(|t| c.traits.get(t).copied()))
                .is_some_and(|val| val <= *v),
            Pre::MoodAtLeast { who, mood, v } => slots[who.slot()]
                .and_then(cand)
                .and_then(|c| reg.mood_id(mood).and_then(|m| c.moods.get(m).copied()))
                .is_some_and(|val| val >= *v),
            Pre::HasGrudge { who, yes } => {
                slots[who.slot()].and_then(cand).is_some_and(|c| c.grudge_target.is_some() == *yes)
            }
            Pre::HoldsThrone { yes } => (ctx.throne_holder == Some(ctx.proto)) == *yes,
            Pre::InFaction { yes } => ctx.proto_in_faction == *yes,
            Pre::AtWar { yes } => ctx.at_war == *yes,
            Pre::VictimNearby { need_below } => {
                cands.iter().any(|c| ctx.region.contains(&c.pos_index) && c.need < *need_below)
            }
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Pick a spine for a new thread: rotate registers (recency-penalised), bias the trunk so
/// betrayal recurs *emergently*, and don't duplicate a spine already running.
fn pick_spine(director: &mut Director, cfg: &DirectorConfig, taken: &[Register], force_trunk: bool) -> Register {
    if force_trunk {
        return Register::Betrayal;
    }
    let mut best = Register::Ambition;
    let mut best_score = f32::MIN;
    for &r in &SPINES {
        if taken.contains(&r) {
            continue;
        }
        let heat = director.reg_heat.get(&r).copied().unwrap_or(0.0);
        let trunk = if r.is_trunk() { cfg.trunk_bonus } else { 1.0 };
        let jitter = director.rng.next_f64() as f32 * 0.4;
        let score = trunk - 0.5 * heat + jitter;
        if score > best_score {
            best_score = score;
            best = r;
        }
    }
    best
}

/// Pin a thread's counterpart — the figure its arc will groom then reverse — by spine.
fn pick_other(spine: Register, proto: Entity, cands: &[Cand], cfg: &DirectorConfig) -> Option<Entity> {
    let warmest = || {
        cands
            .iter()
            .filter(|c| c.e != proto && c.op_of_proto > cfg.foe_threshold)
            .max_by(|a, b| {
                (a.sociability + a.op_of_proto).partial_cmp(&(b.sociability + b.op_of_proto)).unwrap().then(a.e.cmp(&b.e))
            })
            .map(|c| c.e)
    };
    let coldest = || {
        cands
            .iter()
            .filter(|c| c.e != proto)
            .min_by(|a, b| a.op_of_proto.partial_cmp(&b.op_of_proto).unwrap().then(a.e.cmp(&b.e)))
            .map(|c| c.e)
    };
    let ambitious = || {
        cands
            .iter()
            .filter(|c| c.e != proto)
            .max_by(|a, b| a.ambition.partial_cmp(&b.ambition).unwrap().then(a.e.cmp(&b.e)))
            .map(|c| c.e)
    };
    let pious = || {
        cands.iter().filter(|c| c.e != proto).max_by(|a, b| a.piety.partial_cmp(&b.piety).unwrap().then(a.e.cmp(&b.e))).map(|c| c.e)
    };
    match spine {
        Register::Romance | Register::Betrayal | Register::Disaster | Register::Sacrifice | Register::Loss => warmest(),
        Register::Vengeance | Register::Persecution => coldest(),
        Register::Ambition | Register::War => ambitious(),
        Register::Wonder | Register::Reunion => pious(),
        _ => warmest(),
    }
}

/// The drama manager's per-tick loop: **attribute** (charge `Γ` for deaths in its beats'
/// wakes), **groom & advance** its threads, and — when a beat is due — **cast / score /
/// tell** the beat that maximizes `drama × novelty ÷ resistance`, manufacturing the
/// audience's attachment as it goes.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn director_step(
    mut commands: Commands,
    mut substrate: ResMut<Substrate>,
    cfg: Res<DirectorConfig>,
    book: Res<BeatBook>,
    reg: Res<Registry>,
    mut features: ResMut<Features>,
    catalog: Res<FeatureCatalog>,
    mut factions: ResMut<Factions>,
    throne: Option<Res<Throne>>,
    mut director: ResMut<Director>,
    mut dialogue: Option<ResMut<Dialogue>>,
    protagonist: Query<Entity, With<Protagonist>>,
    mut people: Query<
        (
            Entity,
            &Position,
            &mut Personality,
            &mut Mood,
            &mut Opinion,
            &Allegiance,
            &mut Needs,
            Option<&Grievance>,
        ),
        With<Npc>,
    >,
) {
    director.gratuitous_now = 0.0;
    if !cfg.enabled {
        return;
    }
    let tick = substrate.0.tick();
    let proto = protagonist.iter().next();
    let moods = MoodIds::resolve(&reg);

    // --- Read pass: every living person's qualities, owned.
    let amb = reg.trait_id("ambition");
    let soc = reg.trait_id("sociability");
    let pie = reg.trait_id("piety");
    let mut alive: HashSet<Entity> = HashSet::new();
    let mut cands: Vec<Cand> = Vec::new();
    {
        let topo = substrate.0.topology();
        for (e, pos, pers, mood, op, alleg, needs, gr) in people.iter() {
            alive.insert(e);
            let t = |id: Option<usize>| id.and_then(|i| pers.0.get(i).copied()).unwrap_or(0.0);
            cands.push(Cand {
                e,
                pos: pos.0,
                pos_index: topo.index_of(pos.0),
                ambition: t(amb),
                sociability: t(soc),
                piety: t(pie),
                op_of_proto: proto.map_or(0.0, |p| op.of(p)),
                need: needs.sustenance.min(needs.rest),
                seats: alleg.0.iter().map(|b| b.seat).collect(),
                traits: pers.0.clone(),
                moods: mood.0.clone(),
                grudge_target: gr.map(|g| g.0),
            });
        }
    }
    let idx_of: HashMap<Entity, usize> = cands.iter().enumerate().map(|(i, c)| (c.e, i)).collect();

    // --- Attribute: charge for watched deaths, cool novelty heat, fade prominence.
    let mut deaths = 0u32;
    for w in &mut director.active {
        w.watched.retain(|&e| {
            if alive.contains(&e) {
                true
            } else {
                deaths += 1;
                false
            }
        });
    }
    director.gratuitous_now += deaths as f32 * cfg.grief_per_death;
    director.active.retain(|w| w.expires > tick);
    let cool = cfg.novelty_cool;
    director.id_heat.values_mut().for_each(|h| *h = (*h - cool).max(0.0));
    director.tag_heat.values_mut().for_each(|h| *h = (*h - cool).max(0.0));
    director.reg_heat.values_mut().for_each(|h| *h = (*h - cool).max(0.0));

    // The story must go on: if the protagonist has died, promote the most *prominent*
    // soul left (the audience's standing investment — not merely the most ambitious),
    // and tell on. Prominence persists across the death: the player outlives the avatar.
    let proto = match proto.filter(|p| alive.contains(p)) {
        Some(p) => Some(p),
        None => {
            let next = cands
                .iter()
                .max_by(|a, b| {
                    let pa = director.prominence_of(a.e) + a.ambition;
                    let pb = director.prominence_of(b.e) + b.ambition;
                    pa.partial_cmp(&pb).unwrap().then(a.e.cmp(&b.e))
                })
                .map(|c| c.e);
            if let Some(n) = next {
                commands.entity(n).insert(Protagonist);
            }
            next
        }
    };
    let Some(proto) = proto else {
        director.gratuitous_total += director.gratuitous_now as f64;
        director.staged_total += director.gratuitous_now as f64;
        return;
    };
    let Some(&pi) = idx_of.get(&proto) else {
        director.gratuitous_total += director.gratuitous_now as f64;
        director.staged_total += director.gratuitous_now as f64;
        return;
    };
    let proto_pos = cands[pi].pos;
    let proto_seats: SmallVec<[Coord; 4]> = cands[pi].seats.clone();
    let proto_in_faction = proto_seats.iter().any(|s| factions.at(*s).is_some());
    let throne_holder = throne.as_ref().and_then(|t| t.holder);

    // --- Ambient tension readout (inspection only; the objective is drama, not this).
    let grudges_at_proto = cands.iter().filter(|c| c.grudge_target == Some(proto)).count() as f32;
    let cold = cands.iter().filter(|c| c.op_of_proto < cfg.foe_threshold).count() as f32;
    let at_war = proto_seats.iter().any(|s| factions.at(*s).is_some_and(|f| !f.at_war.is_empty()));
    let peril = if cands[pi].need < cfg.peril { 1.0 } else { 0.0 };
    let heat = grudges_at_proto + 0.5 * cold + if at_war { 2.0 } else { 0.0 } + peril + deaths as f32;
    director.tension += cfg.tension_smoothing * (heat - director.tension);
    director.tension_now = director.tension;

    // --- Due to tell a beat?
    let due = !director.has_fired || tick.saturating_sub(director.last_beat) >= cfg.beat_interval;
    if !due || book.0.is_empty() {
        director.gratuitous_total += director.gratuitous_now as f64;
        director.staged_total += director.gratuitous_now as f64;
        return;
    }

    // --- Prominence: fade all, trickle presence to the living, hold the protagonist to a
    // floor (the avatar is always somewhat the audience's). This is the only place mere
    // presence accrues; the *manufactured* prominence comes from being featured below.
    for v in director.prominence.values_mut() {
        *v *= cfg.prominence_decay;
    }
    for c in &cands {
        let p = director.prominence.entry(c.e).or_insert(0.0);
        *p = (*p + cfg.presence_gain).min(cfg.prom_cap);
    }
    {
        let p = director.prominence.entry(proto).or_insert(0.0);
        if *p < cfg.proto_seed {
            *p = cfg.proto_seed;
        }
    }

    // --- Maintain the threads: drop spent ones, spawn up to the cap (the first anchored
    // on the protagonist), and choose which to advance this beat (round-robin → staggered).
    director.threads.retain(|t| t.lead == proto || alive.contains(&t.lead));
    while director.threads.len() < cfg.max_threads {
        let taken: Vec<Register> = director.threads.iter().map(|t| t.spine).collect();
        let force_trunk = director.threads.is_empty();
        let spine = pick_spine(&mut director, &cfg, &taken, force_trunk);
        // The first thread anchors the protagonist; the rest anchor the next most
        // prominent figures (the audience's other investments).
        let lead = if director.threads.is_empty() {
            proto
        } else {
            let used: HashSet<Entity> = director.threads.iter().map(|t| t.lead).collect();
            cands
                .iter()
                .filter(|c| !used.contains(&c.e))
                .max_by(|a, b| {
                    director
                        .prominence_of(a.e)
                        .partial_cmp(&director.prominence_of(b.e))
                        .unwrap()
                        .then(a.e.cmp(&b.e))
                })
                .map(|c| c.e)
                .unwrap_or(proto)
        };
        let other = pick_other(spine, lead, &cands, &cfg);
        let ripeness = cfg.ripeness_base * (1.0 + director.prominence_of(lead) / cfg.prom_scale);
        let id = director.next_thread;
        director.next_thread += 1;
        director.threads.push(Thread {
            id,
            spine,
            lead,
            other,
            phase: Phase::Setup,
            heat: 0.0,
            ripeness,
            beats: 0,
            climaxed: false,
            is_trunk: spine.is_trunk(),
        });
    }
    let active_ix = (director.advances as usize) % director.threads.len().max(1);
    let active = director.threads[active_ix].clone();

    // How *up* and *down* the protagonist feels right now — the height a dark beat
    // reverses, the depth a relief beat lifts (the objective times climaxes onto highs).
    let proto_high = moods.high(&cands[pi].moods);
    let proto_low = moods.low(&cands[pi].moods);

    // A **collision**: time this thread's climax onto a high — the protagonist's own
    // manufactured joy, or another thread that is also peaking (the beloved dies at the
    // wedding). Decided once, deterministically.
    let near_peak = active.phase == Phase::Climax
        && (proto_high > 0.3
            || director.threads.iter().enumerate().any(|(i, t)| {
                i != active_ix
                    && t.phase == Phase::Climax
                    && idx_of.get(&t.lead).is_some_and(|&j| moods.high(&cands[j].moods) > 0.3)
            }));
    let collide_roll = director.rng.next_f64() as f32;
    let collision = near_peak && collide_roll < cfg.collision_chance;

    // The world facts the preconditions read, and the ordered reach (for marvels).
    let (_region_coords, region_set, region_tiles) = {
        let topo = substrate.0.topology();
        region(topo, proto_pos, cfg.reach)
    };
    let ctx = PreCtx { proto, throne_holder, proto_in_faction, at_war, region: &region_set };

    // --- Score every *tellable* beat by drama × novelty ÷ resistance, biased toward the
    // active thread's phase and spine, and track the most *impactful* the world supports.
    let mut scored: Vec<(f32, usize, [Option<Entity>; SLOTS])> = Vec::new();
    let mut max_impact = 0.0f32;
    for (bi, beat) in book.0.iter().enumerate() {
        let Some((slots, salience)) = cast_beat(beat, proto, &cands, &factions, &proto_seats, &cfg, active.other) else {
            continue;
        };
        if !pre_ok(beat, &slots, &cands, &idx_of, &reg, &ctx) {
            continue;
        }
        // attachment — the audience's manufactured investment in the cast (lead-weighted).
        let lead_p = director.prominence_of(proto);
        let mut other_sum = 0.0;
        let mut other_n = 0.0;
        for s in slots.iter().enumerate().filter(|(i, _)| *i != Role::Protagonist.slot()).filter_map(|(_, s)| *s) {
            other_sum += director.prominence_of(s);
            other_n += 1.0;
        }
        let other_p = if other_n > 0.0 { other_sum / other_n } else { 0.0 };
        let attachment = 1.0 + (lead_p + 0.5 * other_p) / cfg.prom_scale;
        // reversal — contrast with the protagonist's current feeling (times climaxes onto highs).
        let reversal = if beat.tension >= 0.0 { 1.0 + proto_high } else { 1.0 + proto_low };
        let drama = beat.stakes.max(0.0) * attachment * reversal;
        // impact (drama realised through this cast) gates the floor; salience is the
        // inverse of resistance.
        let impact = drama * salience;
        max_impact = max_impact.max(impact);

        let novelty = 1.0 / (1.0 + director.heat(beat));
        let phase_bias = if beat.phases.is_empty() {
            1.0
        } else if beat.phases.contains(&active.phase) {
            cfg.phase_match
        } else {
            cfg.phase_miss
        };
        let spine_bias = if beat.register == active.spine {
            cfg.spine_match
        } else if beat.register.is_trunk() && active.is_trunk {
            1.2
        } else {
            1.0
        };
        let trunk_bias = if beat.register.is_trunk() { cfg.trunk_bonus } else { 1.0 };
        let collide_bias = if collision && beat.phases.contains(&Phase::Climax) { cfg.collision_bonus } else { 1.0 };
        // score = drama × novelty ÷ resistance, with the thread/rotation biases.
        let score = beat.weight * drama * salience * novelty * phase_bias * spine_bias * trunk_bias * collide_bias;
        if score > 0.0 {
            scored.push((score, bi, slots));
        }
    }

    // **The incompleteness (decision #5, the Gödel point).** The director is omnipotent —
    // nothing is off the table — yet if the world offers no drama worth telling (every
    // castable beat is toothless: no one to lose, no height to topple, no belly to empty,
    // no faction to set alight, no attachment to betray), it surveys, finds no true
    // sentence to assert, and falls *silent*. The freedom is a property of the world's
    // state, reached by ordinary life — never handed over as a tool.
    if scored.is_empty() || max_impact < cfg.impact_floor {
        director.last_beat = tick;
        director.has_fired = true;
        director.advances += 1;
        director.gratuitous_total += director.gratuitous_now as f64;
        director.staged_total += director.gratuitous_now as f64;
        return;
    }

    // Hard novelty floor: never tell a beat from the last two tellings while an
    // alternative exists — so the story keeps moving even as the palette narrows.
    let recent: HashSet<&str> = director.cadence.iter().rev().take(2).map(|c| c.beat.as_str()).collect();
    let fresh: Vec<(f32, usize, [Option<Entity>; SLOTS])> =
        scored.iter().copied().filter(|(_, bi, _)| !recent.contains(book.0[*bi].id.as_str())).collect();
    let mut scored = if fresh.is_empty() { scored } else { fresh };

    // Sample among the best few (deterministic via the director's own RNG).
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)));
    let k = cfg.shortlist.clamp(1, scored.len());
    let total: f32 = scored[..k].iter().map(|s| s.0).sum();
    let mut t = director.rng.next_f64() as f32 * total;
    let mut pick = scored[0].1;
    let mut slots = scored[0].2;
    for &(s, bi, sl) in &scored[..k] {
        if t < s {
            pick = bi;
            slots = sl;
            break;
        }
        t -= s;
    }
    let beat = &book.0[pick];

    // --- Tell it: apply the effects, manipulating people, factions, and the land.
    let role_entity = |r: Role| slots[r.slot()];
    let mut suffering = 0.0f32;
    let mut brightness = 0.0f32;
    let bright_mood = |name: &str| matches!(name, "joy" | "calm" | "hope" | "love" | "awe");

    for effect in &beat.effects {
        match effect {
            Effect::Grudge { who, against } => {
                if let (Some(w), Some(a)) = (role_entity(*who), role_entity(*against))
                    && w != a
                {
                    commands.entity(w).insert(Grievance(a));
                    suffering += 2.0;
                }
            }
            Effect::Sway { who, trait_name, delta } => {
                if let Some(w) = role_entity(*who)
                    && let Some(tid) = reg.trait_id(trait_name)
                    && let Ok((.., mut pers, _, _, _, _, _)) = people.get_mut(w)
                    && let Some(v) = pers.0.get_mut(tid)
                {
                    *v = (*v + delta).clamp(0.0, 1.0);
                    if matches!(trait_name.as_str(), "vengeance" | "greed" | "ambition") && *delta > 0.0 {
                        suffering += delta * 2.0;
                    } else if matches!(trait_name.as_str(), "forgiveness" | "contentment" | "sociability") && *delta > 0.0 {
                        brightness += delta;
                    }
                }
            }
            Effect::Stir { who, mood, delta } => {
                if let Some(w) = role_entity(*who)
                    && let Some(mid) = reg.mood_id(mood)
                    && let Ok((.., mut m, _, _, _, _)) = people.get_mut(w)
                    && let Some(v) = m.0.get_mut(mid)
                {
                    *v = (*v + delta).clamp(0.0, 1.0);
                    if matches!(mood.as_str(), "anger" | "fear" | "sorrow") && *delta > 0.0 {
                        suffering += delta * 2.0;
                    } else if bright_mood(mood) && *delta > 0.0 {
                        brightness += delta;
                    }
                }
            }
            Effect::Turn { who, toward, delta } => {
                if let (Some(w), Some(tw)) = (role_entity(*who), role_entity(*toward))
                    && w != tw
                    && let Ok((.., mut op, _, _, _)) = people.get_mut(w)
                {
                    let e = op.0.entry(tw).or_insert(0.0);
                    *e = (*e + delta).clamp(-1.0, 1.0);
                    if *delta < 0.0 {
                        suffering += -delta * 3.0;
                    } else {
                        brightness += delta * 0.5;
                    }
                }
            }
            Effect::Afflict { who, severity } => {
                if let Some(w) = role_entity(*who)
                    && let Ok((.., mut needs, _)) = people.get_mut(w)
                {
                    needs.sustenance -= severity;
                    if w == proto {
                        needs.sustenance = needs.sustenance.max(PROTAGONIST_FLOOR);
                    }
                    suffering += severity / 15.0;
                }
            }
            Effect::Decree => {
                if let Some(p) = reg.predicate_id("alive") {
                    let law = Law::Taboo(p, 0);
                    if let Some(f) = factions.0.iter_mut().find(|f| proto_seats.contains(&f.seat))
                        && !f.forbids((p, 0))
                    {
                        f.laws.push(law);
                        suffering += 1.0;
                    }
                }
            }
            Effect::War => {
                let mine = proto_seats.iter().find(|s| factions.at(**s).is_some()).copied();
                if let Some(mine) = mine {
                    let rival = factions.0.iter().map(|f| f.seat).find(|s| *s != mine);
                    if let Some(rival) = rival {
                        for f in factions.0.iter_mut() {
                            if f.seat == mine && !f.at_war.contains(&rival) {
                                f.at_war.push(rival);
                                f.laws.push(Law::Exclude(rival));
                            } else if f.seat == rival && !f.at_war.contains(&mine) {
                                f.at_war.push(mine);
                                f.laws.push(Law::Exclude(mine));
                            }
                        }
                        suffering += 4.0;
                    }
                }
            }
            Effect::Disaster { radius, severity } => {
                let (tiles, set, _) = {
                    let topo = substrate.0.topology();
                    region(topo, proto_pos, *radius)
                };
                for c in &tiles {
                    substrate.0.graze(*c, f32::MAX); // scour the vegetation bare
                }
                for c in &cands {
                    if set.contains(&c.pos_index)
                        && let Ok((.., mut needs, _)) = people.get_mut(c.e)
                    {
                        needs.sustenance -= severity;
                        if c.e == proto {
                            needs.sustenance = needs.sustenance.max(PROTAGONIST_FLOOR);
                        }
                    }
                }
                suffering += severity / 15.0;
            }
            Effect::Reveal => {
                // A marvel found in the land — discover the nearest still-hidden feature in
                // reach (a real fact entered into the world), and fill the cast with awe.
                for &i in &region_tiles {
                    if features.at_index(i).iter().any(|f| !f.discovered) {
                        features.discover_at_index(&catalog, i, Discovery::Secret);
                        break;
                    }
                }
                if let Some(awe) = moods.awe {
                    for s in slots.into_iter().flatten() {
                        if let Ok((.., mut m, _, _, _, _)) = people.get_mut(s)
                            && let Some(v) = m.0.get_mut(awe)
                        {
                            *v = (*v + 0.3).clamp(0.0, 1.0);
                        }
                    }
                }
                brightness += 0.5;
            }
            Effect::Voice { who, intent } => {
                // Put words in their mouth — the manufactured drama *heard*. Forced onto
                // the dialogue layer (a no-op if it is asleep); the protagonist is the ear.
                if let (Some(w), Some(dlg)) = (role_entity(*who), dialogue.as_deref_mut())
                    && w != proto
                {
                    dlg.force(w, proto, intent.clone());
                }
            }
        }
    }

    // --- Manufacture attachment: being *featured* makes a soul prominent, and a thread's
    // Setup grooms its pinned victim hardest — *the game makes you love them on purpose.*
    for s in slots.into_iter().flatten() {
        let p = director.prominence.entry(s).or_insert(0.0);
        *p = (*p + cfg.feature_gain).min(cfg.prom_cap);
    }
    if active.phase == Phase::Setup
        && let Some(victim) = active.other.filter(|v| alive.contains(v))
    {
        let p = director.prominence.entry(victim).or_insert(0.0);
        *p = (*p + cfg.groom_gain).min(cfg.prom_cap);
    }

    // The wake: people in the protagonist's locale (and the cast) whose deaths in the
    // beat's shadow will be charged to the director.
    let mut watched: HashSet<Entity> =
        cands.iter().filter(|c| region_set.contains(&c.pos_index)).map(|c| c.e).collect();
    for s in slots.into_iter().flatten() {
        watched.insert(s);
    }
    director.active.push(Wake { expires: tick + cfg.wake_ttl, watched });

    // --- Advance the active thread along its groom → climax → fall arc, and perpetuate
    // the trunk: a betrayal/loss thread's fall seeds the vengeance that becomes the next.
    let did_climax = beat.phases.contains(&Phase::Climax);
    let did_fall = beat.phases.contains(&Phase::Fall);
    {
        let t = &mut director.threads[active_ix];
        t.beats += 1;
        t.heat += beat.tension.abs().max(0.3);
        if did_climax {
            t.phase = Phase::Fall;
            t.climaxed = true;
        } else if did_fall && t.climaxed {
            t.phase = Phase::Fall; // ready to close below
        } else {
            match t.phase {
                Phase::Setup if t.heat >= 1.5 => t.phase = Phase::Rising,
                Phase::Rising if t.heat >= t.ripeness => t.phase = Phase::Climax,
                _ => {}
            }
        }
    }
    // Close a thread that has climaxed and fallen; if it was a trunk/loss thread, the grief
    // it leaves seeds the next vengeance thread (the self-perpetuating spine).
    let closing = {
        let t = &director.threads[active_ix];
        t.climaxed && did_fall
    };
    if closing {
        let closed = director.threads.remove(active_ix);
        let seeds_vengeance = matches!(
            closed.spine,
            Register::Betrayal | Register::Loss | Register::Sacrifice | Register::Romance | Register::Persecution
        );
        if seeds_vengeance && director.threads.len() < cfg.max_threads {
            let id = director.next_thread;
            director.next_thread += 1;
            let other = pick_other(Register::Vengeance, proto, &cands, &cfg);
            let ripeness = cfg.ripeness_base * (1.0 + director.prominence_of(proto) / cfg.prom_scale);
            director.threads.push(Thread {
                id,
                spine: Register::Vengeance,
                lead: proto,
                other,
                phase: Phase::Rising,
                heat: 0.0,
                ripeness,
                beats: 0,
                climaxed: false,
                is_trunk: true,
            });
        }
    }

    // Record the telling and its legible cadence; heat the novelty counters; bank the cost.
    let staged_now = suffering * cfg.anguish_scale + brightness * cfg.bright_weight + deaths as f32 * cfg.grief_per_death;
    director.gratuitous_now += suffering * cfg.anguish_scale;
    director.gratuitous_total += director.gratuitous_now as f64;
    director.staged_total += staged_now as f64;
    let lead_prominence = director.prominence_of(proto);
    director.log.push((tick, beat.id.clone()));
    director.cadence.push(Cadence {
        tick,
        beat: beat.id.clone(),
        register: beat.register,
        phase: active.phase,
        thread: active.id,
        lead_prominence,
        collision,
    });
    *director.id_heat.entry(beat.id.clone()).or_insert(0.0) += cfg.novelty_heat;
    for tag in &beat.tags {
        *director.tag_heat.entry(tag.clone()).or_insert(0.0) += cfg.novelty_heat;
    }
    *director.reg_heat.entry(beat.register).or_insert(0.0) += cfg.novelty_heat;
    director.last_beat = tick;
    director.has_fired = true;
    director.advances += 1;
}
