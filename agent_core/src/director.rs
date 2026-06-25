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

use crate::beats::{Beat, BeatBook, Effect, Phase, Pre, Role, SLOTS};
use crate::chronicle::EpisodeKind;
use crate::data::{Casting, RegisterId, Registry};
use crate::dialogue::Dialogue;
use crate::factions::{Allegiance, Detained, Factions, Law, Opinion};
use crate::features::{Discovery, FeatureCatalog, Features};
use crate::people::{Bond, Grievance, Mood, Needs, Npc, Personality, Throne};
use crate::scalar::Fx;
use crate::sift::Sift;
use crate::{Position, Substrate};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use game_sim::{Coord, SplitMix64, Topology};
use sim::Rng;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

/// The director's optional **out-of-band sinks**, each absent unless its layer is awake: the
/// dialogue queue it forces `Voice` lines into, and the gossip store it seeds rumours into.
/// Bundled into one `SystemParam` so `director_step` stays within bevy's system-param limit
/// (and so the Chronicle can join them in a later phase); each is `None` when its layer is off,
/// so the director stays byte-identical.
#[derive(SystemParam)]
pub(crate) struct Sinks<'w> {
    dialogue: Option<ResMut<'w, Dialogue>>,
    gossip: Option<ResMut<'w, crate::gossip::Gossip>>,
    chronicle: Option<ResMut<'w, crate::chronicle::Chronicle>>,
    /// The sifter's ranked story candidates (read-only): the **graft** consults these to seed
    /// threads on, and lower resistance toward, the stories the world is already forming. `None`
    /// (layer off) or its graft flag unset => the director runs byte-identically. Lives here, with
    /// the other optional-layer connections, so `director_step` needs no extra system param.
    sift: Option<Res<'w, crate::sift::Sift>>,
}

/// The **stage** the director manipulates: the land's features (and their catalog), the factions,
/// and the throne. Bundled into one `SystemParam` to keep `director_step` well within bevy's
/// system-param limit and to group the world-state the beats' effects reach. The throne is optional
/// (a thronesless world has none).
#[derive(SystemParam)]
pub(crate) struct Stage<'w> {
    features: ResMut<'w, Features>,
    catalog: Res<'w, FeatureCatalog>,
    factions: ResMut<'w, Factions>,
    throne: Option<Res<'w, Throne>>,
}

/// The **avatar and the bodies**: the player (if any) and every body's position, read so the
/// director can draw its spotlight toward the player. All inert in a headless run — no avatar, no
/// draw — so a player-less world is byte-identical. Bundled to trim the param list.
#[derive(SystemParam)]
pub(crate) struct Cast<'w, 's> {
    player: Option<Res<'w, crate::player::PlayerState>>,
    positions: Query<'w, 's, &'static Position>,
    protagonist: Query<'w, 's, Entity, With<Protagonist>>,
}

/// The NPC the director stages its drama for — the audience of one. `Γ`'s threads weave
/// around the *player's* accumulated investments (its [`prominence`](Director::prominence)
/// map), of which this avatar is the central, but not the only, figure. On its death the
/// director promotes another and tells on — but the prominence (the audience's
/// attachment) **persists**: the player outlives the avatar.
#[derive(Component, Clone, Copy, Debug)]
pub struct Protagonist;

// The registers a thread can take as its **spine** are now *data*: the spine-eligible registers,
// in rotation order, are [`Registry::spines`] (authored in `registers.ron`, `spine: true`). Relief
// and the other Fall flavours carry `spine: false`; Betrayal/Vengeance carry `trunk: true`.

// Drama-manager knobs ([`DirectorConfig`]) live Bevy-free in the `config` crate;
// re-exported here and wrapped in an ECS-resource newtype.
pub use config::DirectorConfig;

/// ECS-resource handle for the [`DirectorConfig`] knobs. Derefs to the config.
#[derive(Resource, Clone, Debug)]
pub struct DirectorRes(pub DirectorConfig);

impl std::ops::Deref for DirectorRes {
    type Target = DirectorConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Sustenance the protagonist is never dropped below by the director's *own* staged
/// disasters — so a famine threatens the lead but doesn't end their story outright (the
/// world's own hunger, and the drama of a foe's knife, still can).
const PROTAGONIST_FLOOR: Fx = Fx::lit("18");

/// Sustenance a [`Slay`](crate::beats::Effect::Slay) drains — far past any larder, so the
/// metabolism finishes the kill next tick regardless of the victim's reserves (interim, until
/// combat lands). The protagonist's floor still shields the avatar from a staged death.
const SLAY_SEVERITY: Fx = Fx::lit("999");

/// Ticks a [`Bind`](crate::beats::Effect::Bind) holds a soul captive (via [`Detained`]), unless
/// a [`Free`](crate::beats::Effect::Free) beat strikes the chains first. Counted down by
/// `detention_countdown` like any detention.
const BIND_TICKS: u32 = 60;

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
    pub spine: RegisterId,
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
    pub register: RegisterId,
    pub phase: Phase,
    pub thread: u64,
    pub lead_prominence: f32,
    /// This climax was timed onto another thread's high.
    pub collision: bool,
}

/// A beat the director lately staged, kept so the world can **gossip** about it: where it fell and
/// who it turned on. The player overhears these from nearby souls — sharp or vague by how near and
/// how recent the event is (the fidelity veil, `docs/narrative_surfacing.md` §3). Append-only and
/// read-only to the tick: recording it changes no decision, so a director run is byte-identical with
/// or without anyone listening.
#[derive(Clone, Debug)]
pub struct BeatEvent {
    /// Stable id of the originating beat — shared with the [`gossip`](crate::gossip) rumour it seeds.
    pub id: u64,
    pub tick: u64,
    pub register: RegisterId,
    /// Where it fell — the protagonist's tile when it was staged.
    pub place: Coord,
    /// The figure the beat turned on, and its key counterpart (the friend who turned, the foe).
    pub lead: Entity,
    pub other: Option<Entity>,
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
    reg_heat: HashMap<RegisterId, f32>,
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
    /// The recent beats, as gossip material (place + cast) — what the world murmurs about. A short
    /// ring; append-only, read-only to the tick (see [`BeatEvent`]).
    pub events: Vec<BeatEvent>,
    /// Monotonic id stamped on each staged beat — ties a [`BeatEvent`] to the rumour it seeds.
    next_event: u64,
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
            events: Vec::new(),
            next_event: 0,
        }
    }

    /// How hot (recently-told) a beat is right now — high for a beat, tone, or register
    /// just used, decaying back toward 0. Drives the novelty penalty / register rotation.
    fn heat(&self, beat: &Beat) -> f32 {
        let id = self.id_heat.get(&beat.id).copied().unwrap_or(0.0);
        let tags: f32 = beat
            .tags
            .iter()
            .map(|t| self.tag_heat.get(t).copied().unwrap_or(0.0))
            .sum();
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

    /// The arc-aware **epithet** a soul has earned by being cast in a live thread — "the Betrayed",
    /// "the Faithless" — or `None` for a soul the director has not yet woven into a story. The
    /// register of the thread it stars in, and whether it is the lead or the pinned counterpart,
    /// name it: this is how a prominent soul becomes *legible in the world*, met not as "a villager"
    /// but as the figure a story turns on. Surface only — never read by the tick.
    pub fn epithet_of<'a>(&self, reg: &'a Registry, e: Entity) -> Option<&'a str> {
        let t = self
            .threads
            .iter()
            .find(|t| t.lead == e || t.other == Some(e))?;
        Some(epithet_for(reg, t.spine, t.lead == e))
    }

    /// A short, present-tense **opener** naming a thread figure's plight by its register — the
    /// soul's situation in a single line, for a conversation to open on. `None` for a soul not in a
    /// live thread. Surface flavour only; moves no state.
    pub fn situation_of<'a>(&self, reg: &'a Registry, e: Entity) -> Option<&'a str> {
        let t = self
            .threads
            .iter()
            .find(|t| t.lead == e || t.other == Some(e))?;
        Some(situation_for(reg, t.spine, t.lead == e))
    }

    /// The beats the director has lately staged, oldest first — the raw material the world gossips
    /// about ([`BeatEvent`]). Capped to the recent few.
    pub fn recent_events(&self) -> &[BeatEvent] {
        &self.events
    }
}

/// The authored epithet lexicon (the "authored" half of the naming): a hand-tuned honorific per
/// register, split by whether the soul is the thread's **lead** or its pinned **other**. Registers
/// without a tuned pair fall through to a generic "the Storied" — the generated half being the
/// register/role composition itself.
fn epithet_for(reg: &Registry, spine: RegisterId, is_lead: bool) -> &str {
    reg.register_def(spine).epithet(is_lead)
}

/// The matching one-line situational opener — the soul's plight, present-tense, for a conversation
/// to begin on. Short by design (never a wall of text): the player learns the story by *meeting* it.
fn situation_for(reg: &Registry, spine: RegisterId, is_lead: bool) -> &str {
    reg.register_def(spine).situation(is_lead)
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
    /// The soul this one holds a durable [`Bond`] to (for `Bonded`), if any.
    bond_target: Option<Entity>,
    /// Whether this soul is held captive ([`Detained`]) — for `Bound`.
    detained: bool,
}

// coupling-lint:allow string_ids: the drama director's affect/casting model refers to specific
// named moods/traits/predicates (joy, hope, ambition, alive, …) — necessary *semantic references*,
// resolved once into MoodIds (not in a loop), not an enum-of-instances table.
/// Resolved mood ids the objective reads, looked up once.
#[derive(Clone, Copy)]
pub(crate) struct MoodIds {
    joy: Option<usize>,
    hope: Option<usize>,
    love: Option<usize>,
    calm: Option<usize>,
    awe: Option<usize>,
    anger: Option<usize>,
    sorrow: Option<usize>,
    fear: Option<usize>,
    // The achlydesan affect registers (assets/data/moods.ron): `rapture` is the cult's
    // *manufactured* bliss — the quintessential high the director grooms a soul on, only to
    // reverse it — and `despair`/`dread` are depths a relief beat lifts. The affect model
    // must know them, or the director cannot time a climax onto the very high it staged.
    // Optional, so a registry without these moods reads them as 0 (behaviour unchanged).
    rapture: Option<usize>,
    despair: Option<usize>,
    dread: Option<usize>,
    /// `elation` is a bright high (the soaring joy the director stages, then breaks);
    /// `foreboding` a dark low (the cold sense of a hand about to fall). Like the others,
    /// optional — absent moods read as 0, so the affect model degrades gracefully.
    elation: Option<usize>,
    foreboding: Option<usize>,
}

impl MoodIds {
    pub(crate) fn resolve(reg: &Registry) -> Self {
        Self {
            joy: reg.mood_id("joy"),
            hope: reg.mood_id("hope"),
            love: reg.mood_id("love"),
            calm: reg.mood_id("calm"),
            awe: reg.mood_id("awe"),
            anger: reg.mood_id("anger"),
            sorrow: reg.mood_id("sorrow"),
            fear: reg.mood_id("fear"),
            rapture: reg.mood_id("rapture"),
            despair: reg.mood_id("despair"),
            dread: reg.mood_id("dread"),
            elation: reg.mood_id("elation"),
            foreboding: reg.mood_id("foreboding"),
        }
    }
    /// How *up* the protagonist currently feels — the height a dark beat reverses. Counts
    /// the manufactured highs `rapture` and `elation` (the bliss/soaring the cult stages, the
    /// ones it most loves to break).
    pub(crate) fn high(&self, m: &[f32]) -> f32 {
        let g = |i: Option<usize>| i.and_then(|i| m.get(i)).copied().unwrap_or(0.0);
        g(self.joy)
            + g(self.hope)
            + g(self.love)
            + g(self.awe)
            + g(self.rapture)
            + g(self.elation)
            + 0.5 * g(self.calm)
    }
    /// How *down* the protagonist currently feels — the depth a relief beat reverses.
    pub(crate) fn low(&self, m: &[f32]) -> f32 {
        let g = |i: Option<usize>| i.and_then(|i| m.get(i)).copied().unwrap_or(0.0);
        g(self.anger)
            + g(self.sorrow)
            + g(self.fear)
            + g(self.despair)
            + g(self.dread)
            + g(self.foreboding)
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

/// Wrapped Chebyshev hex distance (the world wraps east–west) — the cheap proximity the
/// avatar-draw bias reads to keep the staged drama within the player's reach.
fn hex_dist(a: Coord, b: Coord, width: i32) -> i32 {
    let drow = (a.row - b.row).abs();
    let dcol = {
        let d = (a.col - b.col).abs();
        d.min(width - d)
    };
    drow.max(dcol)
}

/// How strongly the director prefers casting near the **avatar** (when one is in the world): a
/// soul within the stage radius gets this added to its selection score, so the protagonist and the
/// thread leads — and thus the whole staged season — gather where the player can *encounter* it.
/// Large enough to dominate prominence/ambition (proximity wins), but ties among the near still
/// break on prominence. **Avatar-gated**: with no avatar (every headless run) the bonus is never
/// added, so a player-less run is byte-identical and the director's V&V baseline is untouched.
const AVATAR_DRAW: f32 = 1000.0;

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
            matches!(
                r,
                Role::Ally | Role::Foe | Role::Rival | Role::Lover | Role::Bystander
            ) && slots[r.slot()].is_none()
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
                .max_by(|a, b| {
                    a.op_of_proto
                        .partial_cmp(&b.op_of_proto)
                        .unwrap()
                        .then(a.e.cmp(&b.e))
                })
                .map(|c| (c.e, c.op_of_proto.clamp(0.0, 1.0))),
            Role::Rival => cands
                .iter()
                .filter(|c| !used.contains(&c.e) && c.ambition > 0.01)
                .max_by(|a, b| {
                    a.ambition
                        .partial_cmp(&b.ambition)
                        .unwrap()
                        .then(a.e.cmp(&b.e))
                })
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
                        .min_by(|a, b| {
                            a.op_of_proto
                                .partial_cmp(&b.op_of_proto)
                                .unwrap()
                                .then(a.e.cmp(&b.e))
                        })
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
                .map(|c| {
                    (
                        c.e,
                        ((c.sociability + c.op_of_proto.max(0.0)) * 0.5).clamp(0.2, 1.0),
                    )
                }),
            Role::Mentor => cands
                .iter()
                .filter(|c| !used.contains(&c.e))
                .max_by(|a, b| a.piety.partial_cmp(&b.piety).unwrap().then(a.e.cmp(&b.e)))
                .map(|c| (c.e, c.piety.clamp(0.2, 1.0))),
            Role::Bystander => cands
                .iter()
                .filter(|c| !used.contains(&c.e))
                .min_by(|a, b| a.e.cmp(&b.e))
                .map(|c| (c.e, 0.5)),
        };
        let (e, fit) = chosen?;
        slots[role.slot()] = Some(e);
        used.insert(e);
        fit_sum += fit;
        fit_n += 1;
    }
    let salience = if fit_n > 0 {
        fit_sum / fit_n as f32
    } else {
        1.0
    };
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
    /// Whether a discovered, unspoilt marvel lies in reach (for `DiscoveredMarvelNearby`).
    marvel_nearby: bool,
}

/// Whether a beat's preconditions hold for a tentative cast.
fn pre_ok(
    beat: &Beat,
    slots: &[Option<Entity>; SLOTS],
    cands: &[Cand],
    idx_of: &HashMap<Entity, usize>,
    reg: &Registry,
    ctx: &PreCtx,
) -> bool {
    let cand = |e: Entity| idx_of.get(&e).map(|&i| &cands[i]);
    for p in &beat.pre {
        let ok = match p {
            Pre::Exists { who } => slots[who.slot()].is_some(),
            Pre::TraitAtLeast { who, trait_name, v } => slots[who.slot()]
                .and_then(cand)
                .and_then(|c| {
                    reg.trait_id(trait_name)
                        .and_then(|t| c.traits.get(t).copied())
                })
                .is_some_and(|val| val >= *v),
            Pre::TraitAtMost { who, trait_name, v } => slots[who.slot()]
                .and_then(cand)
                .and_then(|c| {
                    reg.trait_id(trait_name)
                        .and_then(|t| c.traits.get(t).copied())
                })
                .is_some_and(|val| val <= *v),
            Pre::MoodAtLeast { who, mood, v } => slots[who.slot()]
                .and_then(cand)
                .and_then(|c| reg.mood_id(mood).and_then(|m| c.moods.get(m).copied()))
                .is_some_and(|val| val >= *v),
            Pre::HasGrudge { who, yes } => slots[who.slot()]
                .and_then(cand)
                .is_some_and(|c| c.grudge_target.is_some() == *yes),
            Pre::HoldsThrone { yes } => (ctx.throne_holder == Some(ctx.proto)) == *yes,
            Pre::InFaction { yes } => ctx.proto_in_faction == *yes,
            Pre::AtWar { yes } => ctx.at_war == *yes,
            Pre::VictimNearby { need_below } => cands
                .iter()
                .any(|c| ctx.region.contains(&c.pos_index) && c.need < *need_below),
            Pre::Bonded { who, yes } => slots[who.slot()]
                .and_then(cand)
                .is_some_and(|c| c.bond_target.is_some() == *yes),
            Pre::Bound { who, yes } => slots[who.slot()]
                .and_then(cand)
                .is_some_and(|c| c.detained == *yes),
            Pre::DiscoveredMarvelNearby { yes } => ctx.marvel_nearby == *yes,
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Pick a spine for a new thread: rotate registers (recency-penalised), bias the trunk so
/// betrayal recurs *emergently*, and don't duplicate a spine already running.
fn pick_spine(
    director: &mut Director,
    cfg: &DirectorConfig,
    reg: &Registry,
    taken: &[RegisterId],
    force_trunk: bool,
) -> RegisterId {
    if force_trunk {
        // The first thread always takes the trunk root — the first trunk spine in rotation order
        // (Betrayal). RNG-free, so the draw stream is byte-identical to the old `return Betrayal`.
        return reg
            .spines()
            .iter()
            .copied()
            .find(|&r| reg.register_def(r).trunk)
            .unwrap_or_else(|| {
                reg.spines()
                    .first()
                    .copied()
                    .expect("registers.ron has no spine registers")
            });
    }
    let mut best = reg
        .spines()
        .first()
        .copied()
        .expect("registers.ron has no spine registers");
    let mut best_score = f32::MIN;
    for &r in reg.spines() {
        if taken.contains(&r) {
            continue;
        }
        let heat = director.reg_heat.get(&r).copied().unwrap_or(0.0);
        let trunk = if reg.register_def(r).trunk {
            cfg.trunk_bonus
        } else {
            1.0
        };
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
fn pick_other(
    spine: RegisterId,
    proto: Entity,
    cands: &[Cand],
    cfg: &DirectorConfig,
    reg: &Registry,
) -> Option<Entity> {
    let warmest = || {
        cands
            .iter()
            .filter(|c| c.e != proto && c.op_of_proto > cfg.foe_threshold)
            .max_by(|a, b| {
                (a.sociability + a.op_of_proto)
                    .partial_cmp(&(b.sociability + b.op_of_proto))
                    .unwrap()
                    .then(a.e.cmp(&b.e))
            })
            .map(|c| c.e)
    };
    let coldest = || {
        cands
            .iter()
            .filter(|c| c.e != proto)
            .min_by(|a, b| {
                a.op_of_proto
                    .partial_cmp(&b.op_of_proto)
                    .unwrap()
                    .then(a.e.cmp(&b.e))
            })
            .map(|c| c.e)
    };
    let ambitious = || {
        cands
            .iter()
            .filter(|c| c.e != proto)
            .max_by(|a, b| {
                a.ambition
                    .partial_cmp(&b.ambition)
                    .unwrap()
                    .then(a.e.cmp(&b.e))
            })
            .map(|c| c.e)
    };
    let pious = || {
        cands
            .iter()
            .filter(|c| c.e != proto)
            .max_by(|a, b| a.piety.partial_cmp(&b.piety).unwrap().then(a.e.cmp(&b.e)))
            .map(|c| c.e)
    };
    match reg.register_def(spine).casting {
        Casting::Warmest => warmest(),
        Casting::Coldest => coldest(),
        Casting::Ambitious => ambitious(),
        Casting::Pious => pious(),
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
    cfg: Res<DirectorRes>,
    book: Res<BeatBook>,
    reg: Res<Registry>,
    // The world stage the director manipulates (land features + catalog, factions, throne), bundled.
    mut stage: Stage,
    mut director: ResMut<Director>,
    // The optional out-of-band sinks (dialogue queue + gossip store + Chronicle), bundled (see `Sinks`).
    mut sinks: Sinks,
    // The avatar and the bodies — for the player-draw bias; inert (byte-identical) in a headless run.
    cast: Cast,
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
    // Per-soul Bond/Detained reads for the `Bonded`/`Bound` preconditions — a separate
    // read-only query, so the big mutating query above keeps its shape (and its destructurings).
    // Both components are absent until a Bond/Bind beat creates them, so this is empty until then.
    bonds: Query<(Entity, Option<&Bond>, Option<&Detained>), With<Npc>>,
) {
    director.gratuitous_now = 0.0;
    if !cfg.enabled {
        return;
    }
    let tick = substrate.0.tick();
    let proto = cast.protagonist.iter().next();
    let moods = MoodIds::resolve(&reg);

    // Where the player stands, and the per-tile draw toward it (0 with no avatar). This is the
    // whole avatar-bias: a soul within the stage radius scores `AVATAR_DRAW` higher when the
    // director picks a protagonist or a thread's lead, so the spotlight gathers near the player.
    let width = substrate.0.topology().width();
    let reach = cfg.reach;
    let avatar_pos: Option<Coord> = cast
        .player
        .as_ref()
        .and_then(|p| p.avatar())
        .and_then(|a| cast.positions.get(a).ok())
        .map(|p| p.0);
    let draw = |c: Coord| match avatar_pos {
        Some(ap) if hex_dist(ap, c, width) <= reach => AVATAR_DRAW,
        _ => 0.0,
    };

    // --- Read pass: every living person's qualities, owned.
    let amb = reg.trait_id("ambition");
    let soc = reg.trait_id("sociability");
    let pie = reg.trait_id("piety");
    // Per-soul Bond/Detained, for the Cand fields the `Bonded`/`Bound` preconditions read
    // (empty until a Bond/Bind beat creates a component — so off-by-default is byte-identical).
    let bond_det: HashMap<Entity, (Option<Entity>, bool)> = bonds
        .iter()
        .map(|(e, b, d)| (e, (b.map(|x| x.0), d.is_some())))
        .collect();
    let mut alive: HashSet<Entity> = HashSet::new();
    let mut cands: Vec<Cand> = Vec::new();
    {
        let topo = substrate.0.topology();
        for (e, pos, pers, mood, op, alleg, needs, gr) in people.iter() {
            alive.insert(e);
            // The director's casting heuristics stay `f32`; personality/mood are read out of their
            // fixed-point storage and converted at this boundary (like the UI — a value read for a
            // selection decision, never written back into the appraised state).
            let t = |id: Option<usize>| {
                id.and_then(|i| pers.0.get(i).copied())
                    .map_or(0.0, |v| v.to_num::<f32>())
            };
            cands.push(Cand {
                e,
                pos: pos.0,
                pos_index: topo.index_of(pos.0),
                ambition: t(amb),
                sociability: t(soc),
                piety: t(pie),
                op_of_proto: proto.map_or(0.0, |p| op.of(p).to_num::<f32>()),
                need: needs.sustenance.min(needs.rest).to_num::<f32>(),
                seats: alleg.0.iter().map(|b| b.seat).collect(),
                traits: pers.0.iter().map(|v| v.to_num::<f32>()).collect(),
                moods: mood.0.iter().map(|v| v.to_num::<f32>()).collect(),
                grudge_target: gr.map(|g| g.0),
                bond_target: bond_det.get(&e).and_then(|x| x.0),
                detained: bond_det.get(&e).is_some_and(|x| x.1),
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
    director
        .id_heat
        .values_mut()
        .for_each(|h| *h = (*h - cool).max(0.0));
    director
        .tag_heat
        .values_mut()
        .for_each(|h| *h = (*h - cool).max(0.0));
    director
        .reg_heat
        .values_mut()
        .for_each(|h| *h = (*h - cool).max(0.0));

    // The story must go on: if the protagonist has died, promote the most *prominent*
    // soul left (the audience's standing investment — not merely the most ambitious),
    // and tell on. Prominence persists across the death: the player outlives the avatar.
    let proto = match proto.filter(|p| alive.contains(p)) {
        Some(p) => Some(p),
        None => {
            let next = cands
                .iter()
                .max_by(|a, b| {
                    // Prominence + ambition, plus the avatar draw, so the heir to a dead lead is
                    // chosen near the player when one is about (no avatar → draw 0 → unchanged).
                    let pa = director.prominence_of(a.e) + a.ambition + draw(a.pos);
                    let pb = director.prominence_of(b.e) + b.ambition + draw(b.pos);
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

    // Draw the *stage* to the player: if the protagonist has wandered well beyond the avatar's
    // reach and a prominent soul stands near it, move the spotlight there so the headline drama is
    // encountered rather than unfolding off-map. The 2× hysteresis keeps it from flitting as the
    // avatar steps about; gated on an avatar, so a player-less run never re-seats (byte-identical).
    let proto = match (avatar_pos, idx_of.get(&proto)) {
        (Some(ap), Some(&pi0)) if hex_dist(ap, cands[pi0].pos, width) > 2 * reach.max(1) => {
            let near = cands
                .iter()
                .filter(|c| hex_dist(ap, c.pos, width) <= reach)
                .max_by(|a, b| {
                    director
                        .prominence_of(a.e)
                        .partial_cmp(&director.prominence_of(b.e))
                        .unwrap()
                        .then(a.e.cmp(&b.e))
                })
                .map(|c| c.e);
            match near {
                Some(n) if n != proto => {
                    commands.entity(proto).remove::<Protagonist>();
                    commands.entity(n).insert(Protagonist);
                    n
                }
                _ => proto,
            }
        }
        _ => proto,
    };

    let Some(&pi) = idx_of.get(&proto) else {
        director.gratuitous_total += director.gratuitous_now as f64;
        director.staged_total += director.gratuitous_now as f64;
        return;
    };
    let proto_pos = cands[pi].pos;
    let proto_seats: SmallVec<[Coord; 4]> = cands[pi].seats.clone();
    let proto_in_faction = proto_seats.iter().any(|s| stage.factions.at(*s).is_some());
    let throne_holder = stage.throne.as_ref().and_then(|t| t.holder);

    // --- Ambient tension readout (inspection only; the objective is drama, not this).
    let grudges_at_proto = cands
        .iter()
        .filter(|c| c.grudge_target == Some(proto))
        .count() as f32;
    let cold = cands
        .iter()
        .filter(|c| c.op_of_proto < cfg.foe_threshold)
        .count() as f32;
    let at_war = proto_seats
        .iter()
        .any(|s| stage.factions.at(*s).is_some_and(|f| !f.at_war.is_empty()));
    let peril = if cands[pi].need < cfg.peril { 1.0 } else { 0.0 };
    let heat =
        grudges_at_proto + 0.5 * cold + if at_war { 2.0 } else { 0.0 } + peril + deaths as f32;
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

    // --- The graft (`docs/narrative_sifter.md` S5): consult the forming stories the sifter
    // perceived — but only when the sift layer is woken AND its graft flag is set; else `sift_on` is
    // false and every branch below is skipped, so the director runs byte-identically. The candidates
    // are taken as an owned snapshot (Active stories above the interest floor), so the later mutable
    // `sinks` use is unconflicted. The graft only overrides *deterministic, RNG-free* selections —
    // `pick_spine` is still always called below, so `director.rng` advances identically on vs. off.
    let graft = sinks.sift.as_deref().map(Sift::graft).unwrap_or_default();
    let sift_on = graft.enabled;
    let sift_threads: Vec<(RegisterId, SmallVec<[Entity; 4]>, f32)> = if sift_on {
        sinks
            .sift
            .as_deref()
            .map(|s| {
                s.ranked(graft.min_interest)
                    .into_iter()
                    .filter(|c| c.status.is_forming())
                    .map(|c| (c.register, c.cast.clone(), c.interest))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // --- Maintain the threads: drop spent ones, spawn up to the cap (the first anchored
    // on the protagonist), and choose which to advance this beat (round-robin → staggered).
    director
        .threads
        .retain(|t| t.lead == proto || alive.contains(&t.lead));
    while director.threads.len() < cfg.max_threads {
        let taken: Vec<RegisterId> = director.threads.iter().map(|t| t.spine).collect();
        let force_trunk = director.threads.is_empty();
        let spine = pick_spine(&mut director, &cfg, &reg, &taken, force_trunk);
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
                    // The tributary threads, too, gather their leads near the player (draw 0 when
                    // there is no avatar, so the headless thread-spawn order is unchanged).
                    (director.prominence_of(a.e) + draw(a.pos))
                        .partial_cmp(&(director.prominence_of(b.e) + draw(b.pos)))
                        .unwrap()
                        .then(a.e.cmp(&b.e))
                })
                .map(|c| c.e)
                .unwrap_or(proto)
        };
        // The graft: past the manufactured floor, if this lead is already in a forming story, re-key
        // the thread to *that* story's spine and counterpart (the world demonstrably leans here, so
        // resistance is genuinely low and the alibi genuinely strong). RNG-free — `pick_spine` was
        // already called above (its draws consumed); these overrides only change deterministic
        // values. Below the floor (the protagonist's own thread is the first), Γ authors as before.
        let (spine, other) = if sift_on
            && director.threads.len() >= graft.floor
            && let Some((reg_c, cast_c, _)) = sift_threads
                .iter()
                .filter(|(_, cast, _)| cast.contains(&lead))
                .max_by(|a, b| {
                    a.2.partial_cmp(&b.2)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(a.1.cmp(&b.1))
                }) {
            let other_c = cast_c
                .iter()
                .copied()
                .find(|&e| e != lead && idx_of.contains_key(&e) && alive.contains(&e));
            (
                *reg_c,
                other_c.or_else(|| pick_other(*reg_c, lead, &cands, &cfg, &reg)),
            )
        } else {
            (spine, pick_other(spine, lead, &cands, &cfg, &reg))
        };
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
            is_trunk: reg.register_def(spine).trunk,
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
                    && idx_of
                        .get(&t.lead)
                        .is_some_and(|&j| moods.high(&cands[j].moods) > 0.3)
            }));
    let collide_roll = director.rng.next_f64() as f32;
    let collision = near_peak && collide_roll < cfg.collision_chance;

    // The world facts the preconditions read, and the ordered reach (for marvels).
    let (_region_coords, region_set, region_tiles) = {
        let topo = substrate.0.topology();
        region(topo, proto_pos, cfg.reach)
    };
    let marvel_nearby = region_tiles.iter().any(|&i| {
        stage
            .features
            .at_index(i)
            .iter()
            .any(|f| f.discovered && !f.defiled)
    });
    let ctx = PreCtx {
        proto,
        throne_holder,
        proto_in_faction,
        at_war,
        region: &region_set,
        marvel_nearby,
    };

    // --- Score every *tellable* beat by drama × novelty ÷ resistance, biased toward the
    // active thread's phase and spine, and track the most *impactful* the world supports.
    let mut scored: Vec<(f32, usize, [Option<Entity>; SLOTS])> = Vec::new();
    let mut max_impact = 0.0f32;
    for (bi, beat) in book.0.iter().enumerate() {
        let Some((slots, salience)) = cast_beat(
            beat,
            proto,
            &cands,
            &stage.factions,
            &proto_seats,
            &cfg,
            active.other,
        ) else {
            continue;
        };
        if !pre_ok(beat, &slots, &cands, &idx_of, &reg, &ctx) {
            continue;
        }
        // attachment — the audience's manufactured investment in the cast (lead-weighted).
        let lead_p = director.prominence_of(proto);
        let mut other_sum = 0.0;
        let mut other_n = 0.0;
        for s in slots
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != Role::Protagonist.slot())
            .filter_map(|(_, s)| *s)
        {
            other_sum += director.prominence_of(s);
            other_n += 1.0;
        }
        let other_p = if other_n > 0.0 {
            other_sum / other_n
        } else {
            0.0
        };
        let attachment = 1.0 + (lead_p + 0.5 * other_p) / cfg.prom_scale;
        // reversal — contrast with the protagonist's current feeling (times climaxes onto highs).
        let reversal = if beat.tension >= 0.0 {
            1.0 + proto_high
        } else {
            1.0 + proto_low
        };
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
        let beat_trunk = reg.register_def(beat.register).trunk;
        let spine_bias = if beat.register == active.spine {
            cfg.spine_match
        } else if beat_trunk && active.is_trunk {
            1.2
        } else {
            1.0
        };
        let trunk_bias = if beat_trunk { cfg.trunk_bonus } else { 1.0 };
        let collide_bias = if collision && beat.phases.contains(&Phase::Climax) {
            cfg.collision_bonus
        } else {
            1.0
        };
        // score = drama × novelty ÷ resistance, with the thread/rotation biases.
        let mut score = beat.weight
            * drama
            * salience
            * novelty
            * phase_bias
            * spine_bias
            * trunk_bias
            * collide_bias;
        // The graft's trajectory bias (S5): a beat whose cast rides a live forming story is likelier
        // to be told — layered atop the snapshot `salience`. Branch-gated, so an ungrafted run never
        // multiplies by a computed value (byte-identical); RNG-free, so the draw stream is unchanged.
        if sift_on {
            let best = sift_threads
                .iter()
                .filter(|(_, cast, _)| cast.iter().any(|e| slots.iter().flatten().any(|s| s == e)))
                .map(|(_, _, i)| *i)
                .fold(0.0f32, f32::max);
            if best > 0.0 {
                score *= 1.0 + (graft.max_bias - 1.0) * (best / (best + 1.0));
            }
        }
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
    let recent: HashSet<&str> = director
        .cadence
        .iter()
        .rev()
        .take(2)
        .map(|c| c.beat.as_str())
        .collect();
    let fresh: Vec<(f32, usize, [Option<Entity>; SLOTS])> = scored
        .iter()
        .copied()
        .filter(|(_, bi, _)| !recent.contains(book.0[*bi].id.as_str()))
        .collect();
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
    // `rapture` is bright (the cult's staged ecstasy); `despair`/`dread`/`guilt`/`longing`
    // fall through as authored anguish, which is what the director should be charged for.
    let bright_mood =
        |name: &str| matches!(name, "joy" | "calm" | "hope" | "love" | "awe" | "rapture");

    for effect in &beat.effects {
        match effect {
            Effect::Grudge { who, against } => {
                if let (Some(w), Some(a)) = (role_entity(*who), role_entity(*against))
                    && w != a
                {
                    commands.entity(w).insert(Grievance(a));
                    suffering += 2.0;
                    if let Some(c) = sinks.chronicle.as_deref_mut() {
                        c.record(
                            tick,
                            EpisodeKind::GrievanceFormed,
                            [Some(w), Some(a), None],
                            proto_pos,
                            None,
                            0,
                        );
                    }
                }
            }
            Effect::Sway {
                who,
                trait_name,
                delta,
            } => {
                if let Some(w) = role_entity(*who)
                    && let Some(tid) = reg.trait_id(trait_name)
                    && let Ok((.., mut pers, _, _, _, _, _)) = people.get_mut(w)
                    && let Some(v) = pers.0.get_mut(tid)
                {
                    *v = (*v + Fx::from_num(*delta)).clamp(Fx::ZERO, Fx::ONE);
                    if matches!(trait_name.as_str(), "vengeance" | "greed" | "ambition")
                        && *delta > 0.0
                    {
                        suffering += delta * 2.0;
                    } else if matches!(
                        trait_name.as_str(),
                        "forgiveness" | "contentment" | "sociability"
                    ) && *delta > 0.0
                    {
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
                    *v = (*v + Fx::from_num(*delta)).clamp(Fx::ZERO, Fx::ONE);
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
                    let e = op.0.entry(tw).or_insert(Fx::ZERO);
                    *e = (*e + Fx::from_num(*delta)).clamp(-Fx::ONE, Fx::ONE);
                    let crossed = e.to_num::<f32>();
                    if *delta < 0.0 {
                        suffering += -delta * 3.0;
                    } else {
                        brightness += delta * 0.5;
                    }
                    if let Some(c) = sinks.chronicle.as_deref_mut() {
                        // Record a meaningful swing: the edge ending cold (a foe made) or warm (an
                        // ally made). detail = -1 cold / +1 warm; a middling shift is left unrecorded.
                        let dir = if crossed < cfg.foe_threshold {
                            -1
                        } else if crossed > cfg.ally_threshold {
                            1
                        } else {
                            0
                        };
                        if dir != 0 {
                            c.record(
                                tick,
                                EpisodeKind::OpinionCrossed,
                                [Some(w), Some(tw), None],
                                proto_pos,
                                None,
                                dir,
                            );
                        }
                    }
                }
            }
            Effect::Afflict { who, severity } => {
                if let Some(w) = role_entity(*who)
                    && let Ok((.., mut needs, _)) = people.get_mut(w)
                {
                    needs.sustenance -= Fx::from_num(*severity);
                    if w == proto {
                        needs.sustenance = needs.sustenance.max(PROTAGONIST_FLOOR);
                    }
                    suffering += severity / 15.0;
                }
            }
            Effect::Decree => {
                if let Some(p) = reg.predicate_id("alive") {
                    let law = Law::Taboo(p, 0);
                    if let Some(f) = stage
                        .factions
                        .0
                        .iter_mut()
                        .find(|f| proto_seats.contains(&f.seat))
                        && !f.forbids((p, 0))
                    {
                        f.laws.push(law);
                        suffering += 1.0;
                    }
                }
            }
            Effect::War => {
                let mine = proto_seats
                    .iter()
                    .find(|s| stage.factions.at(**s).is_some())
                    .copied();
                if let Some(mine) = mine {
                    let rival = stage.factions.0.iter().map(|f| f.seat).find(|s| *s != mine);
                    if let Some(rival) = rival {
                        for f in stage.factions.0.iter_mut() {
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
                        needs.sustenance -= Fx::from_num(*severity);
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
                    if stage.features.at_index(i).iter().any(|f| !f.discovered) {
                        stage
                            .features
                            .discover_at_index(&stage.catalog, i, Discovery::Secret);
                        break;
                    }
                }
                if let Some(awe) = moods.awe {
                    for s in slots.into_iter().flatten() {
                        if let Ok((.., mut m, _, _, _, _)) = people.get_mut(s)
                            && let Some(v) = m.0.get_mut(awe)
                        {
                            *v = (*v + Fx::from_num(0.3)).clamp(Fx::ZERO, Fx::ONE);
                        }
                    }
                }
                brightness += 0.5;
            }
            Effect::Voice { who, intent } => {
                // Put words in their mouth — the manufactured drama *heard*. Forced onto
                // the dialogue layer (a no-op if it is asleep); the protagonist is the ear.
                if let (Some(w), Some(dlg)) = (role_entity(*who), sinks.dialogue.as_deref_mut())
                    && w != proto
                {
                    dlg.force(w, proto, intent.clone());
                }
            }
            Effect::Relieve { who, need, amount } => {
                // The bright twin of Afflict: restore a need (heal a struck body, feed the
                // starving), mirroring the affordance Relieve write to Needs. Material grace.
                if let Some(w) = role_entity(*who)
                    && let Ok((.., mut needs, _)) = people.get_mut(w)
                {
                    let a = *amount as f32;
                    let af = Fx::from_num(*amount);
                    match need {
                        crate::features::NeedKind::Sustenance => {
                            needs.sustenance = (needs.sustenance + af).min(Fx::from_num(100))
                        }
                        crate::features::NeedKind::Rest => {
                            needs.rest = (needs.rest + af).min(Fx::from_num(100))
                        }
                    }
                    brightness += (a / 100.0).clamp(0.0, 0.5);
                }
            }
            Effect::Slay { who, by } => {
                // Interim: a mortal wound drains the body past saving; `people_metabolism`
                // finishes it next tick (a plausible in-world death preserving the deniability
                // rule; true Slay routes through combat later). The death is charged to the
                // director by its wake; the killing is recorded as an attributed `Killed`
                // episode here (slayer `by`), so the sifter can see a consummated grudge.
                if let Some(w) = role_entity(*who)
                    && let Ok((.., mut needs, _)) = people.get_mut(w)
                {
                    needs.sustenance -= SLAY_SEVERITY;
                    if w == proto {
                        needs.sustenance = needs.sustenance.max(PROTAGONIST_FLOOR);
                    }
                    suffering += 5.0;
                    if let Some(c) = sinks.chronicle.as_deref_mut() {
                        c.record(
                            tick,
                            EpisodeKind::Killed,
                            [role_entity(*by), Some(w), None],
                            proto_pos,
                            None,
                            0,
                        );
                    }
                }
            }
            Effect::Exalt { who } => {
                // The heavenly apex (interim): raise the soul's standing — narrative
                // prominence, pride/devotion, and a soaring high. The true ascendant-tier
                // raise awaits the rpg power tier.
                if let Some(w) = role_entity(*who) {
                    let p = director.prominence.entry(w).or_insert(0.0);
                    *p = (*p + cfg.groom_gain).min(cfg.prom_cap);
                    if let Ok((_, _, mut pers, mut m, _, _, _, _)) = people.get_mut(w) {
                        for tname in ["pride", "devotion"] {
                            if let Some(tid) = reg.trait_id(tname)
                                && let Some(v) = pers.0.get_mut(tid)
                            {
                                *v = (*v + Fx::from_num(0.15)).clamp(Fx::ZERO, Fx::ONE);
                            }
                        }
                        for mname in ["awe", "elation"] {
                            if let Some(mid) = reg.mood_id(mname)
                                && let Some(v) = m.0.get_mut(mid)
                            {
                                *v = (*v + Fx::from_num(0.3)).clamp(Fx::ZERO, Fx::ONE);
                            }
                        }
                    }
                    brightness += 0.6;
                }
            }
            Effect::Defile => {
                // Dark twin of Reveal: ruin the nearest discovered marvel in reach, and fill the
                // cast with despair. (Gated by `DiscoveredMarvelNearby`, so there is one to ruin.)
                for &i in &region_tiles {
                    if stage.features.defile_at_index(i).is_some() {
                        break;
                    }
                }
                for s in slots.into_iter().flatten() {
                    if let Ok((_, _, _, mut m, _, _, _, _)) = people.get_mut(s)
                        && let Some(mid) = reg.mood_id("despair")
                        && let Some(v) = m.0.get_mut(mid)
                    {
                        *v = (*v + Fx::from_num(0.3)).clamp(Fx::ZERO, Fx::ONE);
                    }
                }
                suffering += 2.0;
            }
            Effect::Bond { who, to } => {
                // Forge a durable tie — the bright setup a later betrayal can reverse.
                if let (Some(w), Some(t)) = (role_entity(*who), role_entity(*to))
                    && w != t
                {
                    commands.entity(w).insert(Bond(t));
                    brightness += 0.4;
                }
            }
            Effect::Bind { who } => {
                // Captivity made personal — reuse the faction-enforcer Detained machinery (the
                // captive cannot act until freed or the term elapses). Never bind the avatar.
                if let Some(w) = role_entity(*who)
                    && w != proto
                {
                    commands.entity(w).insert(Detained { ticks: BIND_TICKS });
                    suffering += 2.0;
                }
            }
            Effect::Free { who } => {
                // Strike off another's chains — the defiant/heavenly deed.
                if let Some(w) = role_entity(*who) {
                    commands.entity(w).remove::<Detained>();
                    brightness += 0.4;
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
    let mut watched: HashSet<Entity> = cands
        .iter()
        .filter(|c| region_set.contains(&c.pos_index))
        .map(|c| c.e)
        .collect();
    for s in slots.into_iter().flatten() {
        watched.insert(s);
    }

    // This beat's identity and the cast counterpart it turned on — shared by the gossip seed and the
    // event log. Everyone in the wake (the cast + the locale) witnessed it, so they learn it
    // firsthand (fidelity 1.0) and it can begin to spread. A no-op when the gossip layer is absent.
    let event_id = director.next_event;
    director.next_event += 1;
    let gossip_other = slots
        .iter()
        .enumerate()
        .find(|(i, s)| *i != Role::Protagonist.slot() && s.is_some())
        .and_then(|(_, s)| *s);
    if let Some(g) = sinks.gossip.as_deref_mut() {
        let r = crate::gossip::Rumor {
            event_id,
            register: beat.register,
            lead: proto,
            other: gossip_other,
            place: proto_pos,
            fidelity: 1.0,
        };
        for &w in &watched {
            g.witness(w, r);
        }
    }

    director.active.push(Wake {
        expires: tick + cfg.wake_ttl,
        watched,
    });

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
        // A trunk/loss thread's grief seeds the next thread — *which* register it seeds is now data
        // (`registers.ron`'s `seeds:`), replacing the hardcoded `Register::Vengeance`. The seed's
        // own trunk flag carries forward (vengeance is a trunk), so the spine keeps self-perpetuating.
        if let Some(seed) = reg.register_seeds(closed.spine)
            && director.threads.len() < cfg.max_threads
        {
            let id = director.next_thread;
            director.next_thread += 1;
            let other = pick_other(seed, proto, &cands, &cfg, &reg);
            let ripeness =
                cfg.ripeness_base * (1.0 + director.prominence_of(proto) / cfg.prom_scale);
            director.threads.push(Thread {
                id,
                spine: seed,
                lead: proto,
                other,
                phase: Phase::Rising,
                heat: 0.0,
                ripeness,
                beats: 0,
                climaxed: false,
                is_trunk: reg.register_def(seed).trunk,
            });
        }
    }

    // Record the telling and its legible cadence; heat the novelty counters; bank the cost.
    let staged_now = suffering * cfg.anguish_scale
        + brightness * cfg.bright_weight
        + deaths as f32 * cfg.grief_per_death;
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
    // Record it as gossip material — a short append-only ring; perturbs no decision. Reuses the
    // `event_id`/`gossip_other` computed for the rumour seed above, so the log and the rumour agree.
    director.events.push(BeatEvent {
        id: event_id,
        tick,
        register: beat.register,
        place: proto_pos,
        lead: proto,
        other: gossip_other,
    });
    if let Some(c) = sinks.chronicle.as_deref_mut() {
        c.record(
            tick,
            EpisodeKind::BeatFired,
            [Some(proto), gossip_other, None],
            proto_pos,
            Some(beat.register),
            0,
        );
    }
    const EVENT_CAP: usize = 16;
    if director.events.len() > EVENT_CAP {
        let drop = director.events.len() - EVENT_CAP;
        director.events.drain(0..drop);
    }
    *director.id_heat.entry(beat.id.clone()).or_insert(0.0) += cfg.novelty_heat;
    for tag in &beat.tags {
        *director.tag_heat.entry(tag.clone()).or_insert(0.0) += cfg.novelty_heat;
    }
    *director.reg_heat.entry(beat.register).or_insert(0.0) += cfg.novelty_heat;
    director.last_beat = tick;
    director.has_fired = true;
    director.advances += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_sim::World as GameWorld;

    /// A **contrived stage** for the director: the tiny ECS world and the handful of resources
    /// [`director_step`] reads, with helpers to hand-place souls and seed the director's own
    /// threads/prominence. No worldgen warmup and no emergent population — a test seeds the exact
    /// dramatic situation it means to check, then ticks the director directly. This is the whole
    /// point of the harness: the director is a precondition engine, so its decisions can be tested
    /// against *constructed* state in microseconds, where the old whole-season emergence tests in
    /// the `agents` crate ran for seconds to let the same situation arise by chance.
    struct Stage {
        world: World,
        reg: Registry,
        coords: Vec<Coord>,
        next_tile: usize,
        proto: Option<Entity>,
    }

    impl Stage {
        /// A stage with the given director knobs (use [`knobs`] for the `enabled` defaults). The
        /// world is a tiny, unwarmed map: the director reads only topology, positions and features
        /// — never the climate — so terrain detail is irrelevant and no `evolve` passes are needed.
        fn new(cfg: DirectorConfig) -> Self {
            let reg = Registry::bundled();
            let gw = GameWorld::generate(16, 12, config::tunables::params(), 7);
            let coords: Vec<Coord> = {
                let topo = gw.topology();
                (0..topo.len()).map(|i| topo.coord(i)).collect()
            };
            let mut world = World::new();
            world.insert_resource(Substrate(gw));
            world.insert_resource(DirectorRes(cfg));
            world.insert_resource(reg.clone());
            world.insert_resource(Features::default());
            world.insert_resource(FeatureCatalog::default());
            world.insert_resource(Factions(Vec::new()));
            world.insert_resource(Director::seeded(0));
            world.insert_resource(BeatBook(Vec::new()));
            Self {
                world,
                reg,
                coords,
                next_tile: 0,
                proto: None,
            }
        }

        /// Replace the director's repertoire (its action set `L`).
        fn beats(&mut self, beats: Vec<Beat>) {
            self.world.insert_resource(BeatBook(beats));
        }

        /// Spawn a soul — all traits/moods neutral, well-fed — at its own tile. The first soul
        /// spawned becomes the [`Protagonist`].
        fn soul(&mut self) -> Entity {
            let coord = self.coords[self.next_tile % self.coords.len()];
            self.next_tile += 1;
            let e = self
                .world
                .spawn((
                    Npc,
                    Position(coord),
                    Personality(vec![Fx::ZERO; self.reg.trait_count()]),
                    Mood(vec![Fx::ZERO; self.reg.mood_count()]),
                    Opinion::default(),
                    Allegiance::default(),
                    Needs {
                        sustenance: Fx::from_num(100),
                        rest: Fx::from_num(100),
                    },
                ))
                .id();
            if self.proto.is_none() {
                self.world.entity_mut(e).insert(Protagonist);
                self.proto = Some(e);
            }
            e
        }

        fn set_mood(&mut self, e: Entity, mood: &str, v: f32) {
            let id = self.reg.mood_id(mood).expect("mood exists");
            self.world.entity_mut(e).get_mut::<Mood>().unwrap().0[id] = Fx::from_num(v);
        }

        /// Move a soul to a chosen tile (e.g. into the protagonist's blast radius).
        fn place(&mut self, e: Entity, coord: Coord) {
            self.world.entity_mut(e).get_mut::<Position>().unwrap().0 = coord;
        }

        /// Set `who`'s opinion *of* `of` (warm → ally, cold → foe).
        fn set_opinion(&mut self, who: Entity, of: Entity, v: f32) {
            self.world
                .entity_mut(who)
                .get_mut::<Opinion>()
                .unwrap()
                .0
                .insert(of, Fx::from_num(v));
        }

        /// Enrol `who` in the faction seated at `seat` (so the director reads it as `InFaction`).
        fn join_faction(&mut self, who: Entity, seat: Coord) {
            self.world
                .entity_mut(who)
                .get_mut::<Allegiance>()
                .unwrap()
                .0
                .push(crate::factions::Bond { seat, loyalty: 1.0 });
        }

        /// Seat a faction led by `leader` at `seat` — enough for the director's faction levers
        /// (war, decree) to have a bloc to work.
        fn seat_faction(&mut self, seat: Coord, leader: Entity) {
            use crate::factions::{Faction, Government};
            self.world.resource_mut::<Factions>().0.push(Faction {
                seat,
                government: Government::Monarchy,
                leaders: SmallVec::from_slice(&[leader]),
                members: vec![leader],
                laws: SmallVec::new(),
                at_war: SmallVec::new(),
                force: 1.0,
                cunning: 0.0,
                wealth: 0,
            });
        }

        /// Seed a running thread directly, at a chosen point in its groom→climax→fall arc — the
        /// lever the old tests could only reach by waiting a thread to groom and ripen across a
        /// whole season. Reaches `Director`'s private state, which a child test module may touch.
        fn seed_thread(&mut self, spine: &str, lead: Entity, other: Option<Entity>, phase: Phase) {
            let spine = self.reg.register_id(spine).expect("register exists");
            let is_trunk = self.reg.register_def(spine).trunk;
            let mut d = self.world.resource_mut::<Director>();
            let id = d.next_thread;
            d.next_thread += 1;
            d.threads.push(Thread {
                id,
                spine,
                lead,
                other,
                phase,
                heat: 0.0,
                ripeness: 1.0,
                beats: 0,
                climaxed: false,
                is_trunk,
            });
        }

        /// Wake the dialogue sink, so the director's `Voice` lever has a mouth to speak through.
        fn enable_dialogue(&mut self) {
            self.world
                .insert_resource(crate::dialogue::Dialogue::seeded(0));
        }

        fn director(&self) -> &Director {
            self.world.resource::<Director>()
        }

        /// Run the director for one tick. A one-system schedule applies the deferred `Commands`
        /// the director enacts (grudges, bonds, the promoted protagonist) before returning.
        fn tick(&mut self) {
            let mut sched = Schedule::default();
            sched.add_systems(director_step);
            sched.run(&mut self.world);
        }
    }

    /// A minimal beat built in-test, so a mechanism test exercises the director against a
    /// *controlled* storylet rather than the bundled content (which is checked separately). A
    /// positive `tension` (an escalation) keeps the reversal keyed to the protagonist's *high*.
    fn beat(id: &str, reg: &Registry, register: &str, phase: Phase, cast: Vec<Role>) -> Beat {
        Beat {
            id: id.into(),
            register: reg.register_id(register).expect("register exists"),
            tags: Vec::new(),
            phases: vec![phase],
            tension: 1.0,
            stakes: 5.0,
            weight: 1.0,
            cast,
            pre: Vec::new(),
            effects: Vec::new(),
        }
    }

    /// Knobs for a woken director with the impact floor dropped to 0 — a contrived stage offers
    /// little ambient drama, so the floor (which makes a barren world fall silent) is lifted unless
    /// a test sets it. `enabled` is on, since most tests want the director awake.
    fn knobs() -> DirectorConfig {
        DirectorConfig {
            enabled: true,
            impact_floor: 0.0,
            ..Default::default()
        }
    }

    #[test]
    fn a_due_beat_is_told() {
        // The simplest mechanism: a woken director with a tellable beat and a protagonist fires
        // it on its first due tick — no season required.
        let mut s = Stage::new(knobs());
        let reg = s.reg.clone();
        s.beats(vec![beat(
            "a_quiet_moment",
            &reg,
            "wonder",
            Phase::Setup,
            vec![Role::Protagonist],
        )]);
        s.soul();
        s.tick();
        assert_eq!(s.director().log.len(), 1, "a due beat should be told");
        assert_eq!(s.director().cadence[0].beat, "a_quiet_moment");
    }

    #[test]
    fn a_climax_is_timed_onto_a_high() {
        // The collision the old `betrayal_dominates_…` test hunted across four 600-tick seasons:
        // a climax timed onto the protagonist's manufactured high (the beloved dies at the
        // wedding). Seed a thread already at its climax and a protagonist riding a high, and the
        // director marks the telling a collision — directly, in one tick.
        let mut cfg = knobs();
        cfg.max_threads = 1; // only our seeded thread runs
        cfg.collision_chance = 1.0; // a near-peak climax always collides (no RNG flake)
        let mut s = Stage::new(cfg);
        let reg = s.reg.clone();
        s.beats(vec![beat(
            "the_reversal",
            &reg,
            "betrayal",
            Phase::Climax,
            vec![Role::Protagonist],
        )]);
        let proto = s.soul();
        s.set_mood(proto, "joy", 0.5); // a high to reverse
        s.seed_thread("betrayal", proto, None, Phase::Climax);
        s.tick();
        assert!(
            s.director().cadence.iter().any(|c| c.collision),
            "a climax on a high should be recorded as a collision",
        );
    }

    #[test]
    fn betrayal_is_the_trunk_spine() {
        // Betrayal dominates a season **because it is the trunk** the first thread always takes
        // and the spine that self-perpetuates — not by a hard rule in the director. The old test
        // inferred this from a 480-tick register tally; here we read it off `pick_spine` directly.
        let reg = Registry::bundled();
        let cfg = knobs();
        let mut d = Director::seeded(0);
        let spine = pick_spine(&mut d, &cfg, &reg, &[], true);
        assert_eq!(
            spine,
            reg.register_id("betrayal").expect("betrayal register"),
            "the first thread should take the betrayal trunk",
        );
        assert!(
            reg.register_def(spine).trunk,
            "the trunk spine should be flagged trunk",
        );
    }

    #[test]
    fn the_director_falls_silent_below_the_impact_floor() {
        // The Gödel point (§5): the director is omnipotent yet a precondition engine with an
        // **impact floor** — a world offering no drama worth telling starves it and it falls
        // silent. With a real floor and only a toothless (low-stakes) beat, nothing is told;
        // raise the stakes past the floor and the same director speaks. No "freed world" season
        // needed — the floor is the whole mechanism.
        let mut quiet = Stage::new(DirectorConfig {
            impact_floor: 1.0,
            ..knobs()
        });
        let reg = quiet.reg.clone();
        let mut toothless = beat(
            "a_toothless_beat",
            &reg,
            "wonder",
            Phase::Setup,
            vec![Role::Protagonist],
        );
        toothless.stakes = 0.0; // no impact — below the floor
        quiet.beats(vec![toothless]);
        quiet.soul();
        quiet.tick();
        assert!(
            quiet.director().log.is_empty(),
            "a world below the impact floor should quiet the director",
        );

        let mut loud = Stage::new(DirectorConfig {
            impact_floor: 1.0,
            ..knobs()
        });
        loud.beats(vec![beat(
            "real_drama",
            &reg,
            "betrayal",
            Phase::Setup,
            vec![Role::Protagonist],
        )]);
        loud.soul();
        loud.tick();
        assert_eq!(
            loud.director().log.len(),
            1,
            "drama above the floor should be told by the same director",
        );
    }

    #[test]
    fn the_same_seed_tells_the_same_story() {
        // Determinism: same seed, same world, same story — beat for beat, and the same moral cost.
        let run = || {
            let mut s = Stage::new(knobs());
            s.beats(BeatBook::bundled().0);
            s.soul();
            s.soul();
            for _ in 0..8 {
                s.tick();
            }
            let d = s.director();
            (d.gratuitous_total, d.log.clone())
        };
        assert_eq!(run(), run(), "same seed must tell the same story");
    }

    #[test]
    fn featuring_a_soul_grooms_its_prominence() {
        // *The game makes you love them on purpose* (decision #15): being featured in beats
        // grooms a soul's prominence far past the bare presence trickle, so a later reversal
        // pays. A handful of fires lifts the protagonist well past the seed floor.
        let mut s = Stage::new(DirectorConfig {
            beat_interval: 0, // due every tick — fire a beat each step
            max_threads: 1,
            ..knobs()
        });
        let reg = s.reg.clone();
        s.beats(vec![beat(
            "featured",
            &reg,
            "wonder",
            Phase::Setup,
            vec![Role::Protagonist],
        )]);
        let proto = s.soul();
        for _ in 0..6 {
            s.tick();
        }
        assert!(
            s.director().prominence_of(proto) > 2.0,
            "featuring should manufacture prominence (got {:.2})",
            s.director().prominence_of(proto),
        );
    }

    #[test]
    fn staged_experience_counts_joy_not_only_suffering() {
        // The moral arithmetic is *staged experience*, not tragedy (decision #8): a beat that
        // authors both anguish and joy lifts `staged_total` above the suffering-only
        // `gratuitous_total`. A single mixed beat shows both totals diverge.
        let mut s = Stage::new(DirectorConfig {
            max_threads: 1,
            ..knobs()
        });
        let reg = s.reg.clone();
        let mut mixed = beat(
            "a_bittersweet_turn",
            &reg,
            "betrayal",
            Phase::Setup,
            vec![Role::Protagonist],
        );
        mixed.effects = vec![
            Effect::Stir {
                who: Role::Protagonist,
                mood: "anger".into(),
                delta: 0.5,
            },
            Effect::Stir {
                who: Role::Protagonist,
                mood: "joy".into(),
                delta: 0.5,
            },
        ];
        s.beats(vec![mixed]);
        s.soul();
        s.tick();
        let d = s.director();
        assert!(d.gratuitous_total > 0.0, "the beat should author suffering");
        assert!(
            d.staged_total > d.gratuitous_total,
            "staged experience (joy + suffering) should exceed suffering alone ({:.3} vs {:.3})",
            d.staged_total,
            d.gratuitous_total,
        );
    }

    #[test]
    fn a_trunk_threads_fall_seeds_the_next_vengeance() {
        // Storylets chain into arcs: a trunk (betrayal/loss) thread's *fall* seeds the next
        // thread — its `seeds:` register (vengeance) — so the spine self-perpetuates. The old
        // test waited a 360-tick season for a chained beat; here we close one trunk thread and
        // read the seeded successor directly.
        let mut s = Stage::new(DirectorConfig {
            max_threads: 1,
            ..knobs()
        });
        let reg = s.reg.clone();
        let mut fall = beat(
            "the_aftermath",
            &reg,
            "betrayal",
            Phase::Fall,
            vec![Role::Protagonist],
        );
        fall.tension = -1.0; // a fall is relief-keyed
        s.beats(vec![fall]);
        let proto = s.soul();
        // A betrayal thread that has already climaxed and is now falling — closing it seeds vengeance.
        s.seed_thread("betrayal", proto, None, Phase::Fall);
        s.world.resource_mut::<Director>().threads[0].climaxed = true;
        s.tick();
        let vengeance = reg.register_id("vengeance").expect("vengeance register");
        assert!(
            s.director().threads.iter().any(|t| t.spine == vengeance),
            "a fallen betrayal thread should seed a vengeance thread (threads: {:?})",
            s.director()
                .threads
                .iter()
                .map(|t| t.spine)
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn a_thread_advances_through_its_arc() {
        // A thread moves groom → climax: heat banked by each beat ripens it Setup → Rising →
        // Climax, so the cadence is an arc, not a flat sequence. (Decision #12.)
        let mut s = Stage::new(DirectorConfig {
            beat_interval: 0,
            max_threads: 1,
            ..knobs()
        });
        let reg = s.reg.clone();
        // A phase-agnostic, high-tension beat: it fits any phase (so it always casts) and banks
        // enough heat each telling to ripen the thread quickly.
        let mut driver = beat(
            "a_turn",
            &reg,
            "betrayal",
            Phase::Setup,
            vec![Role::Protagonist],
        );
        driver.phases = Vec::new(); // any phase
        driver.tension = 2.0;
        s.beats(vec![driver]);
        let proto = s.soul();
        s.seed_thread("betrayal", proto, None, Phase::Setup);
        for _ in 0..3 {
            s.tick();
        }
        let phases: std::collections::HashSet<Phase> =
            s.director().cadence.iter().map(|c| c.phase).collect();
        assert!(
            phases.len() >= 3 && phases.contains(&Phase::Setup),
            "the thread should groom (Setup) and move through several phases, saw {phases:?}",
        );
    }

    #[test]
    fn an_asleep_director_does_nothing() {
        // Off by default = byte-identical: a sleeping director (its switch off) tells no beat and
        // authors no suffering, however rich the world. The integration that a director-free run
        // is *bit-for-bit* unchanged stays an `agents` smoke test; here we pin the unit behaviour.
        let mut s = Stage::new(DirectorConfig {
            enabled: false,
            ..knobs()
        });
        s.beats(BeatBook::bundled().0);
        s.soul();
        s.soul();
        for _ in 0..5 {
            s.tick();
        }
        let d = s.director();
        assert!(d.log.is_empty(), "a sleeping director tells no story");
        assert!(
            d.cadence.is_empty(),
            "a sleeping director leaves no cadence"
        );
        assert_eq!(
            d.gratuitous_total, 0.0,
            "a sleeping director authors no suffering"
        );
        assert_eq!(d.staged_total, 0.0, "a sleeping director stages nothing");
    }

    #[test]
    fn the_director_works_people_factions_and_the_world() {
        // The point of the rebuild: `Γ` reaches the *social fabric*, not just the land — and the
        // land too. A single beat carrying Grudge, War and Disaster levers, told over a seated
        // faction, manufactures a grudge between people (people layer), sets that faction at war
        // with its rival (faction layer), and scours a soul caught in the blast (world layer). The
        // old test inferred all three from which beats happened to fire across a 240-tick, 56-soul
        // season; here the levers are exercised directly, in one tick.
        let mut s = Stage::new(DirectorConfig {
            max_threads: 1,
            ..knobs()
        });
        let reg = s.reg.clone();
        let proto = s.soul();
        let foe = s.soul();
        let proto_pos = s.world.get::<Position>(proto).unwrap().0;
        s.place(foe, proto_pos); // stand the foe in the protagonist's tile — inside the blast
        s.set_opinion(foe, proto, -1.0); // a foe: opinion of the protagonist well past cold
        let seat = s.coords[0];
        let rival_seat = s.coords[6];
        s.join_faction(proto, seat);
        s.seat_faction(seat, proto);
        s.seat_faction(rival_seat, foe);
        let mut strike = beat(
            "the_director_strikes",
            &reg,
            "betrayal",
            Phase::Climax,
            vec![Role::Protagonist, Role::Foe],
        );
        strike.effects = vec![
            Effect::Grudge {
                who: Role::Protagonist,
                against: Role::Foe,
            },
            Effect::War,
            Effect::Disaster {
                radius: 1,
                severity: 40.0,
            },
        ];
        s.beats(vec![strike]);
        s.tick();
        assert!(
            s.world.get::<Grievance>(proto).is_some(),
            "the director should have manufactured a grudge (the people layer)",
        );
        assert!(
            s.world
                .resource::<Factions>()
                .at(seat)
                .is_some_and(|f| f.at_war.contains(&rival_seat)),
            "the director should have set its faction at war (the faction layer)",
        );
        assert!(
            s.world.get::<Needs>(foe).unwrap().sustenance < Fx::from_num(100),
            "the director's disaster should have scoured a body in the blast (the world layer)",
        );
    }

    #[test]
    fn the_director_voices_its_betrayals() {
        // The thematic payoff: a betrayal `Γ` engineers is *heard*, not merely tallied — its `Voice`
        // lever forces a line into a soul's mouth (the friend renouncing the protagonist aloud).
        // Here we fire a Voice beat directly and confirm a forced utterance was queued for the
        // dialogue layer; the old test ran a 300-tick peopled, talking season to catch one.
        let mut s = Stage::new(DirectorConfig {
            max_threads: 1,
            ..knobs()
        });
        s.enable_dialogue();
        let reg = s.reg.clone();
        let _proto = s.soul();
        let ally = s.soul();
        s.set_opinion(ally, _proto, 1.0); // a warm soul, cast as the Ally the beat voices
        let mut renounce = beat(
            "the_friend_renounces",
            &reg,
            "betrayal",
            Phase::Climax,
            vec![Role::Protagonist, Role::Ally],
        );
        renounce.effects = vec![Effect::Voice {
            who: Role::Ally,
            intent: "an_accusation".into(),
        }];
        s.beats(vec![renounce]);
        s.tick();
        assert!(
            s.world.resource::<crate::dialogue::Dialogue>().forced_len() > 0,
            "the director should have put words in the betrayer's mouth",
        );
    }
}
