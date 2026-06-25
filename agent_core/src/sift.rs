//! The **Story Sifter** — Gamma's missing perception organ (`docs/narrative_sifter.md`).
//!
//! The narrative [`director`](crate::director) is a *top-down* drama manager: it reads casting
//! salience from a **snapshot** of the present and manufactures an arc. What it cannot see is a
//! **trajectory** — that three souls have been escalating a feud across the last forty ticks. The
//! sifter is that perception: a deterministic, in-tick reader of the bounded
//! [`Chronicle`](crate::chronicle) ring that pattern-matches **forming** stories bottom-up, ranks
//! them by interest, and (in a later phase) hands the ranked list to the director so its beats
//! land on situations the world *already leans toward* — strengthening the deniability thesis.
//!
//! Patterns are **data** (`assets/data/sift.ron`), in the `beats.ron` idiom: an ordered window of
//! episode-kind predicates that **bind a cast** (later predicates reuse earlier bindings), plus the
//! interest axes a match scores on — **surprise** (base-rate rarity) + **dissonance** (grounded on
//! the real social state: grievance convergence, soured opinion, a coiled norm, a mood reversal,
//! bloodshed). Scoring reuses [`ai::Curve`](crate::ai::Curve), so axes are authored exactly like
//! goal/intent considerations.
//!
//! **Deterministic & off-by-default.** [`Sift`] and [`SiftBook`] are their own resources, inserted
//! only when the sift layer is woken; the matcher is pure arithmetic over the deterministic ring
//! and a read-only world snapshot, so it changes no sim state — a sift-off world is byte-identical.
//! This phase ships the **retrospective** matcher (run the patterns over the whole ring — the
//! oracle, trivially testable against a saved run); the incremental matcher (asserted to agree) and
//! the director graft follow.

// coupling-lint:allow string_ids: the vengeance-pressure axis refers to the "vengeance" trait and
// "alive" predicate by name — necessary semantic references.
use crate::ai::Curve;
use crate::chronicle::{Chronicle, Episode, EpisodeKind};
use crate::data::{RegisterId, Registry};
use crate::director::MoodIds;
use crate::factions::{Factions, Opinion};
use crate::people::{Grievance, Mood, Npc, Personality};
use bevy_ecs::prelude::*;
use config::{Asset, Bundled};
use game_sim::Coord;
use serde::Deserialize;
use smallvec::SmallVec;
use std::collections::HashMap;

// --- The pattern (RON-authored) ---

/// Which party of an [`Episode`] a window step reads — an index into its `parties` array
/// (`actor`, `target`, `third`). A step binds a cast variable to one of these slots.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Actor,
    Target,
    Third,
}

impl Slot {
    fn idx(self) -> usize {
        self as usize
    }
}

/// A direction constraint on an episode's `detail` sign — for [`EpisodeKind::OpinionCrossed`],
/// `Cold` requires the edge ended sour (`detail < 0`), `Warm` requires it warmed (`detail > 0`).
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Cold,
    Warm,
}

/// One step in a pattern's ordered window: the episode kind it matches, the cast variables it
/// **binds** (a variable's *first* mention) or **checks** (a later mention must resolve to the same
/// soul), and an optional direction constraint. Unifying bind/check into one list means a pattern
/// reads top-to-bottom and a variable can never be "used before bound".
#[derive(Deserialize, Clone, Debug)]
pub struct WindowStep {
    /// The episode kind this step matches.
    pub kind: EpisodeKind,
    /// `(variable, slot)` pairs: the first time a variable appears it binds to that party; later
    /// appearances require the party to equal the earlier binding (the "where" reuse).
    #[serde(default)]
    pub binds: Vec<(String, Slot)>,
    /// An optional `detail`-sign constraint (e.g. an opinion crossing that ended cold).
    #[serde(default)]
    pub dir: Option<Dir>,
}

/// Which interest signal an axis reads (the curve then shapes it to `0..1`). `Rarity` is the
/// surprise term; the rest are the dissonance terms grounded on real social state.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// Base-rate rarity of this pattern (rarer patterns rank higher) — the statistical surprise.
    Rarity,
    /// How much pressure stands behind a grudge in the cast: convergence (souls grudging one
    /// target) × recency. The director counts the same convergence as `grudges_at_proto`.
    GrievancePressure,
    /// How far a directed opinion within the cast has soured — an ally turned foe reads high.
    OpinionReversal,
    /// A cast member primed toward a forbidden act — vengeance under a no-kill taboo (a coiled
    /// spring), mirroring the deontic `Input::Sanction` axis.
    NormTension,
    /// The depth of a low mood among the cast (anger/sorrow/fear/despair/dread/foreboding) — the
    /// same `MoodIds::low` the director reads for its reversal term.
    MoodReversal,
    /// A killing or death among the cast — the apex stakes a forming thread can reach.
    Bloodshed,
}

/// One interest axis: which signal, shaped by which [`Curve`] (authored exactly like an
/// `ai::Consideration`). A match's interest is the sum of its axes' curve outputs.
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct InterestAxis {
    pub axis: Axis,
    pub curve: Curve,
}

/// A story pattern as authored in `sift.ron` (its `register` name not yet resolved).
#[derive(Deserialize, Clone, Debug)]
struct SiftPatternDef {
    id: String,
    tension: String,
    register: String,
    window: Vec<WindowStep>,
    #[serde(default)]
    interest: Vec<InterestAxis>,
    #[serde(default = "one")]
    emerging_at: usize,
    #[serde(default = "two")]
    active_at: usize,
    #[serde(default = "default_window_ticks")]
    window_ticks: u64,
}

/// A story pattern, resolved: an ordered window of episode predicates that bind a cast, the spine it
/// would let the director amplify (a [`RegisterId`]), and the interest axes it scores on.
#[derive(Clone, Debug)]
pub struct SiftPattern {
    /// Stable id (and the log line), e.g. `"feud_escalating"`.
    pub id: String,
    /// Machine-readable tension label — surfaced only to the dev overlay / eval, never the player.
    pub tension: String,
    /// The spine a candidate of this pattern would feed the director.
    pub register: RegisterId,
    /// The ordered episode window. Non-empty.
    pub window: Vec<WindowStep>,
    /// How a match's interest is computed (surprise + dissonance).
    pub interest: Vec<InterestAxis>,
    /// Steps matched to reach [`SiftStatus::Emerging`] (≥ 1).
    pub emerging_at: usize,
    /// Steps matched to reach [`SiftStatus::Active`] (the director-graft threshold).
    pub active_at: usize,
    /// A partial match whose last episode is older than this (in ticks) has stalled — `Abandoned`.
    pub window_ticks: u64,
}

fn one() -> usize {
    1
}
fn two() -> usize {
    2
}
fn default_window_ticks() -> u64 {
    60
}

/// The director's whole pattern repertoire — the parsed `sift.ron`.
#[derive(Resource, Clone, Debug, Default)]
pub struct SiftBook(pub Vec<SiftPattern>);

impl SiftBook {
    /// The patterns shipped with the crate — resolved against the bundled registry.
    pub fn bundled() -> Self {
        let book = Self::from_ron(Bundled::get(Asset::Sift), &Registry::bundled())
            .expect("bundled sift patterns are valid RON");
        book.validate()
            .expect("bundled sift patterns are structurally sound");
        book
    }

    /// Parse a sift document and resolve each pattern's register name against `reg` (a typo in a
    /// register name is a load error, not a silent phantom).
    pub fn from_ron(ron: &str, reg: &Registry) -> Result<Self, SiftError> {
        let defs: Vec<SiftPatternDef> = config::parse(ron)?;
        let mut pats = Vec::with_capacity(defs.len());
        for d in defs {
            let register =
                reg.register_id(&d.register)
                    .ok_or_else(|| SiftError::UnknownRegister {
                        pattern: d.id.clone(),
                        register: d.register.clone(),
                    })?;
            pats.push(SiftPattern {
                id: d.id,
                tension: d.tension,
                register,
                window: d.window,
                interest: d.interest,
                emerging_at: d.emerging_at,
                active_at: d.active_at,
                window_ticks: d.window_ticks,
            });
        }
        Ok(SiftBook(pats))
    }

    /// Fail fast on a structurally unsound pattern (empty window, or thresholds out of the
    /// `1 <= emerging_at <= active_at <= window.len()` ordering) — so a typo is caught at load.
    pub fn validate(&self) -> Result<(), SiftError> {
        for p in &self.0 {
            if p.window.is_empty() {
                return Err(SiftError::EmptyWindow {
                    pattern: p.id.clone(),
                });
            }
            let n = p.window.len();
            if !(1..=n).contains(&p.emerging_at) || !(p.emerging_at..=n).contains(&p.active_at) {
                return Err(SiftError::BadThresholds {
                    pattern: p.id.clone(),
                    emerging_at: p.emerging_at,
                    active_at: p.active_at,
                    window_len: n,
                });
            }
        }
        Ok(())
    }
}

/// Why loading sift patterns failed — parse error, an unknown register, or a structurally
/// unsound pattern.
#[derive(Debug)]
pub enum SiftError {
    Config(config::ConfigError),
    /// A pattern names a register the registry doesn't define.
    UnknownRegister {
        pattern: String,
        register: String,
    },
    /// A pattern's match window is empty — it could never match.
    EmptyWindow {
        pattern: String,
    },
    /// A pattern's thresholds break the `1 <= emerging_at <= active_at <= window_len` ordering.
    BadThresholds {
        pattern: String,
        emerging_at: usize,
        active_at: usize,
        window_len: usize,
    },
}

impl std::fmt::Display for SiftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SiftError::Config(e) => write!(f, "loading sift patterns: {e}"),
            SiftError::UnknownRegister { pattern, register } => {
                write!(f, "sift '{pattern}': unknown register '{register}'")
            }
            SiftError::EmptyWindow { pattern } => write!(f, "sift '{pattern}': empty window"),
            SiftError::BadThresholds {
                pattern,
                emerging_at,
                active_at,
                window_len,
            } => write!(
                f,
                "sift '{pattern}': need 1 <= emerging_at ({emerging_at}) <= active_at ({active_at}) <= window len ({window_len})",
            ),
        }
    }
}
impl std::error::Error for SiftError {}
impl From<config::ConfigError> for SiftError {
    fn from(e: config::ConfigError) -> Self {
        SiftError::Config(e)
    }
}

// --- Candidates ---

/// A pattern's index in its [`SiftBook`] — a cheap interned handle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SiftPatternId(pub usize);

/// How far a forming story has progressed. The director graft (a later phase) amplifies `Active`
/// candidates — the world is *forming* such a story and resistance is genuinely low.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SiftStatus {
    /// Enough of the window matched to notice (≥ `emerging_at`).
    Emerging,
    /// Substantially matched and still forming (≥ `active_at`, not yet complete).
    Active,
    /// The whole window matched — the arc has already played out.
    Resolved,
    /// A partial match that stalled (its last episode aged past `window_ticks`).
    Abandoned,
}

impl SiftStatus {
    /// Whether this story is still **forming** — worth the director amplifying. `Emerging` and
    /// `Active` both qualify (a high-interest single grudge against a widely-hated figure is a
    /// genuine forming story); `Resolved` (already played out) and `Abandoned` (stalled) do not.
    /// The interest floor, not the step count, is what separates signal from noise.
    pub fn is_forming(self) -> bool {
        matches!(self, SiftStatus::Emerging | SiftStatus::Active)
    }
}

/// A forming (or formed) story the sifter perceived: the pattern, how far it got, the cast it bound,
/// the episodes that constitute it, and its interest. `PartialEq` is what the incremental ==
/// retrospective oracle test compares (both paths emit candidates in one canonical order).
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadCandidate {
    pub pattern: SiftPatternId,
    pub status: SiftStatus,
    /// The bound cast, in the pattern's binding order.
    pub cast: SmallVec<[Entity; 4]>,
    /// The tension label (dev / eval only).
    pub tension: String,
    /// The spine it would feed the director.
    pub register: RegisterId,
    /// Where it is centered (the latest supporting episode's place) — for casting + markers.
    pub place: Coord,
    /// The [`Episode::id`]s that constitute it.
    pub support: SmallVec<[u64; 8]>,
    pub interest: f32,
    pub first_seen: u64,
    pub last_updated: u64,
}

/// The **director-graft** knobs (`docs/narrative_sifter.md` S5), lifted from
/// [`config::SiftConfig`] into the live [`Sift`] so `director_step` can read them without another
/// system param. Default = the graft **off**: a sift-on world still only *observes*.
#[derive(Clone, Copy, Debug, Default)]
pub struct GraftCfg {
    /// Whether the director consults the sifter at all.
    pub enabled: bool,
    /// The ceiling on the resistance bias a live forming story lends a beat (`1.0` = none).
    pub max_bias: f32,
    /// The interest a candidate must reach to seed a thread or bias a beat.
    pub min_interest: f32,
    /// How many threads stay director-authored (never sift-seeded) — the manufactured floor.
    pub floor: usize,
}

/// The sifter's output (and base-rate memory). A resource, so a sift-free world is byte-identical.
#[derive(Resource, Clone, Debug, Default)]
pub struct Sift {
    candidates: Vec<ThreadCandidate>,
    /// Base-rate counters per pattern — how often each has been seen (the `Rarity` axis).
    seen: HashMap<usize, u64>,
    /// The **incremental** matcher's running state: every partial match opened so far (one per
    /// episode that started a pattern), advanced greedily as later episodes arrive. Unused by the
    /// retrospective path. Folding the ring's episodes in tick order leaves this identical to the
    /// retrospective per-start matches (the oracle equality, verified in tests).
    open: Vec<(usize, RawMatch)>,
    /// High-water mark of episode ids the live [`sift_step`] has folded in (so it ingests each
    /// episode once). `None` until the first ingest.
    last_ingested: Option<u64>,
    /// The director-graft configuration (set when the layer is woken; default off).
    graft: GraftCfg,
}

impl Sift {
    /// The candidates with interest ≥ `min_interest`, **highest interest first** (ties broken
    /// deterministically by pattern, then first-seen, then the leading episode id) — what the
    /// director and the eval harness read.
    pub fn ranked(&self, min_interest: f32) -> Vec<&ThreadCandidate> {
        let mut out: Vec<&ThreadCandidate> = self
            .candidates
            .iter()
            .filter(|c| c.interest >= min_interest)
            .collect();
        out.sort_by(|a, b| {
            b.interest
                .partial_cmp(&a.interest)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.pattern.0.cmp(&b.pattern.0))
                .then(a.first_seen.cmp(&b.first_seen))
                .then(a.support.first().cmp(&b.support.first()))
        });
        out
    }

    /// All candidates, unranked (eval / debugging).
    pub fn candidates(&self) -> &[ThreadCandidate] {
        &self.candidates
    }

    /// Configure the director graft from [`config::SiftConfig`] (called once when the layer wakes).
    pub fn set_graft(&mut self, cfg: &config::SiftConfig) {
        self.graft = GraftCfg {
            enabled: cfg.graft,
            max_bias: cfg.max_bias.max(1.0),
            min_interest: cfg.min_interest,
            floor: cfg.manufactured_floor,
        };
    }

    /// The director-graft configuration — read by `director_step` to decide whether (and how hard)
    /// to consult the sifter.
    pub fn graft(&self) -> GraftCfg {
        self.graft
    }

    /// **Retrospective** matcher (the oracle): recompute every candidate from the whole `ring`
    /// against `book`, scoring each on the read-only world snapshot `reads`. Pure — it touches no
    /// sim state. The episodes must be in tick order (the ring's natural order).
    pub(crate) fn resift(&mut self, ring: &[Episode], book: &SiftBook, reads: &SiftReads) {
        let mut raws: Vec<(usize, RawMatch)> = Vec::new();
        for (pi, pat) in book.0.iter().enumerate() {
            for m in find_matches(pat, ring, reads.now) {
                raws.push((pi, m));
            }
        }
        self.finalize(raws, book, reads);
    }

    /// Fold one episode into the **incremental** matcher's [`open`](Self::open) state: advance every
    /// open match whose next step this episode satisfies (binding-consistent, still inside its
    /// window), then open a fresh match for every pattern this episode can start. Advance *then*
    /// spawn, so an episode never advances the match it just opened — mirroring the retrospective
    /// scan, which begins each start at the *next* episode. Pure structural matching (no scoring).
    /// Feed episodes in tick order.
    pub(crate) fn ingest(&mut self, ep: &Episode, book: &SiftBook) {
        for (pi, m) in self.open.iter_mut() {
            let pat = &book.0[*pi];
            if m.matched >= pat.window.len() {
                continue; // already complete
            }
            if ep.tick.saturating_sub(m.first_tick) > pat.window_ticks {
                continue; // its window has closed
            }
            if try_step(&mut m.env, &pat.window[m.matched], ep) {
                m.support.push(ep.id);
                m.last_tick = ep.tick;
                m.place = ep.place;
                m.matched += 1;
            }
        }
        for (pi, pat) in book.0.iter().enumerate() {
            let mut env: Vec<(String, Entity)> = Vec::new();
            if try_step(&mut env, &pat.window[0], ep) {
                self.open.push((
                    pi,
                    RawMatch {
                        env,
                        support: vec![ep.id],
                        matched: 1,
                        first_tick: ep.tick,
                        last_tick: ep.tick,
                        place: ep.place,
                    },
                ));
            }
        }
    }

    /// Recompute the ranked candidates from the accumulated [`open`](Self::open) matches against the
    /// live `reads`. With the same episode sequence fed to [`ingest`](Self::ingest), this yields the
    /// **same** candidates the retrospective [`resift`](Self::resift) would (the S8.2 oracle).
    pub(crate) fn recompute(&mut self, book: &SiftBook, reads: &SiftReads) {
        let raws: Vec<(usize, RawMatch)> = self
            .open
            .iter()
            .filter(|(pi, m)| m.matched >= book.0[*pi].emerging_at)
            .map(|(pi, m)| (*pi, m.clone()))
            .collect();
        self.finalize(raws, book, reads);
    }

    /// Shared back end for both matchers: dedup the raw matches to one per `(pattern, cast)` (keeping
    /// the most-advanced, then earliest), tally the base rates the `Rarity` axis reads, classify +
    /// score each, and emit the candidates in **one canonical order** — so the retrospective and
    /// incremental paths, given the same raw matches, produce byte-identical candidate lists.
    fn finalize(&mut self, mut raws: Vec<(usize, RawMatch)>, book: &SiftBook, reads: &SiftReads) {
        self.candidates.clear();
        self.seen.clear();

        raws.sort_by(|x, y| {
            x.0.cmp(&y.0)
                .then_with(|| cast_key(&x.1).cmp(&cast_key(&y.1)))
                .then(y.1.matched.cmp(&x.1.matched))
                .then(x.1.first_tick.cmp(&y.1.first_tick))
        });
        raws.dedup_by(|x, y| x.0 == y.0 && cast_key(&x.1) == cast_key(&y.1));

        for (pi, _) in &raws {
            *self.seen.entry(*pi).or_insert(0) += 1;
        }
        for (pi, m) in &raws {
            let pat = &book.0[*pi];
            let n = pat.window.len();
            let status = if m.matched >= n {
                SiftStatus::Resolved
            } else if reads.now.saturating_sub(m.last_tick) > pat.window_ticks {
                SiftStatus::Abandoned
            } else if m.matched >= pat.active_at {
                SiftStatus::Active
            } else {
                SiftStatus::Emerging
            };
            let interest = self.score(pat, *pi, m, reads);
            self.candidates.push(ThreadCandidate {
                pattern: SiftPatternId(*pi),
                status,
                cast: m.env.iter().map(|(_, e)| *e).collect(),
                tension: pat.tension.clone(),
                register: pat.register,
                place: m.place,
                support: m.support.iter().copied().collect(),
                interest,
                first_seen: m.first_tick,
                last_updated: m.last_tick,
            });
        }
        // Canonical order: (pattern, cast) is unique per surviving candidate, so this is a total,
        // input-order-independent sort — the key to the two paths agreeing byte-for-byte.
        self.candidates.sort_by(|a, b| {
            a.pattern
                .0
                .cmp(&b.pattern.0)
                .then_with(|| a.cast.cmp(&b.cast))
                .then(a.first_seen.cmp(&b.first_seen))
                .then(a.support.first().cmp(&b.support.first()))
        });
    }

    /// Sum the pattern's interest axes (each curve output is `0..1`), grounded on `reads`.
    fn score(&self, pat: &SiftPattern, pi: usize, m: &RawMatch, reads: &SiftReads) -> f32 {
        pat.interest
            .iter()
            .map(|ax| {
                let x = axis_input(ax.axis, m, reads, *self.seen.get(&pi).unwrap_or(&1));
                ax.curve.eval(x)
            })
            .sum()
    }
}

// --- The matcher ---

/// One raw match before classification: the bound environment (in binding order), the supporting
/// episode ids, how many window steps matched, and its temporal/spatial extent.
#[derive(Clone, Debug)]
struct RawMatch {
    env: Vec<(String, Entity)>,
    support: Vec<u64>,
    matched: usize,
    first_tick: u64,
    last_tick: u64,
    place: Coord,
}

/// A canonical key for "the same story": the pattern's bound cast, sorted by variable name. Two
/// matches that bind the same variables to the same souls are one candidate.
fn cast_key(m: &RawMatch) -> Vec<(String, Entity)> {
    let mut k = m.env.clone();
    k.sort();
    k
}

/// Whether `ep` satisfies `step`'s kind and direction (independent of bindings).
fn step_shape_ok(step: &WindowStep, ep: &Episode) -> bool {
    if ep.kind != step.kind {
        return false;
    }
    match step.dir {
        Some(Dir::Cold) => ep.detail < 0,
        Some(Dir::Warm) => ep.detail > 0,
        None => true,
    }
}

/// Try to advance `env` by `step` against `ep`: every `(var, slot)` either binds (first mention) or
/// must match the prior binding. A missing party for a referenced slot fails the step. Returns
/// whether the step matched (and only mutates `env` when it did).
fn try_step(env: &mut Vec<(String, Entity)>, step: &WindowStep, ep: &Episode) -> bool {
    if !step_shape_ok(step, ep) {
        return false;
    }
    let mut staged: Vec<(String, Entity)> = Vec::new();
    for (var, slot) in &step.binds {
        let Some(e) = ep.parties[slot.idx()] else {
            return false;
        };
        let bound = env
            .iter()
            .chain(staged.iter())
            .find(|(v, _)| v == var)
            .map(|(_, b)| *b);
        match bound {
            Some(b) if b != e => return false, // reuse mismatch
            Some(_) => {}                      // consistent reuse
            None => staged.push((var.clone(), e)),
        }
    }
    env.extend(staged);
    true
}

/// Greedily find the matches of `pat` in `episodes` (tick-ordered). For each episode that can open
/// the window, extend forward through the remaining steps, requiring binding consistency and that
/// the whole match fall within `window_ticks` of its first episode.
fn find_matches(pat: &SiftPattern, episodes: &[Episode], _now: u64) -> Vec<RawMatch> {
    let mut out = Vec::new();
    for (i, start) in episodes.iter().enumerate() {
        let mut env: Vec<(String, Entity)> = Vec::new();
        if !try_step(&mut env, &pat.window[0], start) {
            continue;
        }
        let mut support = vec![start.id];
        let mut wi = 1;
        let mut last = start;
        for ep in &episodes[i + 1..] {
            if wi >= pat.window.len() {
                break;
            }
            if ep.tick.saturating_sub(start.tick) > pat.window_ticks {
                break;
            }
            if try_step(&mut env, &pat.window[wi], ep) {
                support.push(ep.id);
                last = ep;
                wi += 1;
            }
        }
        if wi >= pat.emerging_at {
            out.push(RawMatch {
                env,
                support,
                matched: wi,
                first_tick: start.tick,
                last_tick: last.tick,
                place: last.place,
            });
        }
    }
    out
}

// --- Interest inputs (grounded on the live snapshot) ---

/// The convergence scale: a target grudged by this many souls saturates the `GrievancePressure`
/// convergence term.
const CONVERGENCE_SAT: f32 = 4.0;

fn axis_input(axis: Axis, m: &RawMatch, reads: &SiftReads, seen: u64) -> f32 {
    let cast: Vec<Entity> = m.env.iter().map(|(_, e)| *e).collect();
    match axis {
        // Rarer pattern -> higher surprise. seen counts this pattern's matches in the ring.
        Axis::Rarity => 1.0 / (seen.max(1) as f32),
        Axis::GrievancePressure => {
            // The most-grudged target among the supporting GrievanceFormed episodes, weighted by
            // how recently the grudge formed.
            let mut best = 0.0f32;
            for &id in &m.support {
                if let Some(ep) = reads.episode(id)
                    && ep.kind == EpisodeKind::GrievanceFormed
                    && let Some(target) = ep.parties[1]
                {
                    let conv = *reads.grudge_convergence.get(&target).unwrap_or(&0) as f32;
                    let age = reads.now.saturating_sub(ep.tick) as f32;
                    let recency = (1.0 - age / 60.0).clamp(0.0, 1.0);
                    best = best.max((conv / CONVERGENCE_SAT * recency).clamp(0.0, 1.0));
                }
            }
            best
        }
        Axis::OpinionReversal => {
            // The most soured directed edge within the cast (an ally turned foe reads ~1).
            let mut coldest = 0.0f32;
            for &a in &cast {
                for &b in &cast {
                    if a != b
                        && let Some(edges) = reads.opinion.get(&a)
                        && let Some(&v) = edges.get(&b)
                    {
                        coldest = coldest.max((-v).clamp(0.0, 1.0));
                    }
                }
            }
            coldest
        }
        Axis::NormTension => {
            // A coiled spring: a vengeful cast member under a no-kill taboo.
            if !reads.taboo_active {
                return 0.0;
            }
            cast.iter()
                .map(|e| *reads.vengeance.get(e).unwrap_or(&0.0))
                .fold(0.0f32, f32::max)
        }
        Axis::MoodReversal => {
            // The deepest low felt by any cast member (the same low the director reads).
            cast.iter()
                .map(|e| reads.mood.get(e).map_or(0.0, |mv| reads.mood_ids.low(mv)))
                .fold(0.0f32, f32::max)
                .clamp(0.0, 1.0)
        }
        Axis::Bloodshed => {
            let any = m.support.iter().any(|&id| {
                reads
                    .episode(id)
                    .is_some_and(|ep| matches!(ep.kind, EpisodeKind::Killed | EpisodeKind::Death))
            });
            if any { 1.0 } else { 0.0 }
        }
    }
}

// --- The read-only world snapshot the dissonance axes score on ---

/// Everything the interest scorer needs from the live world, gathered once. Read-only: the sifter
/// never mutates sim state. (Dead souls simply drop out of these maps and read as neutral.)
pub(crate) struct SiftReads {
    pub now: u64,
    /// How many souls hold a [`Grievance`] against each target — the director's `grudges_at_proto`.
    pub grudge_convergence: HashMap<Entity, u32>,
    /// `a -> (b -> opinion)`: live directed [`Opinion`] edges.
    pub opinion: HashMap<Entity, HashMap<Entity, f32>>,
    /// Each soul's live mood vector (for the `MoodReversal` low).
    pub mood: HashMap<Entity, Vec<f32>>,
    /// Each soul's `vengeance` trait (for `NormTension`).
    pub vengeance: HashMap<Entity, f32>,
    /// Whether any faction forbids the avenge act (the no-kill taboo gating `NormTension`).
    pub taboo_active: bool,
    /// Resolved mood ids (shared with the director's reversal reading).
    pub mood_ids: MoodIds,
    /// The episodes by id, so an axis can re-read a supporting episode's parties/tick.
    by_id: HashMap<u64, Episode>,
}

impl SiftReads {
    fn episode(&self, id: u64) -> Option<&Episode> {
        self.by_id.get(&id)
    }
}

/// Whether any faction forbids the avenge act (`alive(foe) = 0`) — the no-kill taboo that gates the
/// `NormTension` axis (a vengeful soul under it is a coiled spring).
fn taboo_in_force(reg: &Registry, factions: Option<&Factions>) -> bool {
    let avenge = reg.predicate_id("alive").map(|p| (p, 0i64));
    matches!((avenge, factions), (Some(act), Some(f)) if f.0.iter().any(|fac| fac.forbids(act)))
}

/// Assemble the read-only snapshot from already-gathered parts — the shared back end for the
/// `&mut World` eval path ([`gather_reads`]) and the in-schedule system path ([`sift_step`]). It
/// only *reads* each soul's components (cloning what it keeps), so it perturbs nothing.
fn assemble_reads<'a>(
    now: u64,
    mood_ids: MoodIds,
    vengeance_id: Option<usize>,
    taboo_active: bool,
    rows: impl Iterator<
        Item = (
            Entity,
            Option<&'a Grievance>,
            &'a Opinion,
            &'a Mood,
            &'a Personality,
        ),
    >,
    ring: &[Episode],
) -> SiftReads {
    let mut grudge_convergence: HashMap<Entity, u32> = HashMap::new();
    let mut opinion: HashMap<Entity, HashMap<Entity, f32>> = HashMap::new();
    let mut mood: HashMap<Entity, Vec<f32>> = HashMap::new();
    let mut vengeance: HashMap<Entity, f32> = HashMap::new();
    for (e, gr, op, md, pers) in rows {
        if let Some(g) = gr {
            *grudge_convergence.entry(g.0).or_insert(0) += 1;
        }
        if !op.0.is_empty() {
            // Opinion is fixed-point now; the Sifter's f32 read of it converts at this boundary.
            opinion.insert(
                e,
                op.0.iter().map(|(&k, v)| (k, v.to_num::<f32>())).collect(),
            );
        }
        // The Story Sifter's pattern scoring stays `f32` (its axes are raw narrative signals, not
        // the IAUS appraisal); mood/vengeance are converted out of fixed-point at this read boundary.
        mood.insert(e, md.0.iter().map(|v| v.to_num::<f32>()).collect());
        if let Some(id) = vengeance_id
            && let Some(&v) = pers.0.get(id)
        {
            vengeance.insert(e, v.to_num::<f32>());
        }
    }
    let by_id: HashMap<u64, Episode> = ring.iter().map(|e| (e.id, *e)).collect();
    SiftReads {
        now,
        grudge_convergence,
        opinion,
        mood,
        vengeance,
        taboo_active,
        mood_ids,
        by_id,
    }
}

/// Gather the read-only world snapshot the scorer needs (live grievance convergence, opinion edges,
/// moods, the vengeance trait, and whether a no-kill taboo is in force), plus an id index over the
/// ring. Needs `&mut World` only because ECS queries build their state, as every accessor does.
pub(crate) fn gather_reads(world: &mut World) -> SiftReads {
    let now = world.resource::<crate::Substrate>().0.tick();
    let mood_ids = MoodIds::resolve(world.resource::<Registry>());
    let vengeance_id = world.resource::<Registry>().trait_id("vengeance");
    let taboo_active = taboo_in_force(
        world.resource::<Registry>(),
        world.get_resource::<Factions>(),
    );
    let ring: Vec<Episode> = world
        .get_resource::<Chronicle>()
        .map(|c| c.recent().copied().collect())
        .unwrap_or_default();
    let mut q = world
        .query_filtered::<(Entity, Option<&Grievance>, &Opinion, &Mood, &Personality), With<Npc>>();
    assemble_reads(
        now,
        mood_ids,
        vengeance_id,
        taboo_active,
        q.iter(world),
        &ring,
    )
}

/// The **live sifter**: each tick, fold the Chronicle's new episodes into the incremental matcher
/// and recompute the ranked candidates (the snapshot the director graft and the surface read).
/// Scheduled before `director_step`. A no-op when the sift layer is off (its resources are absent).
/// It writes **only** its own [`Sift`] resource — it touches no sim state — so a sift-off (or
/// graft-off) world runs byte-identically.
#[allow(clippy::type_complexity)]
pub(crate) fn sift_step(
    chronicle: Option<Res<Chronicle>>,
    book: Option<Res<SiftBook>>,
    sift: Option<ResMut<Sift>>,
    substrate: Res<crate::Substrate>,
    reg: Res<Registry>,
    factions: Option<Res<Factions>>,
    npcs: Query<(Entity, Option<&Grievance>, &Opinion, &Mood, &Personality), With<Npc>>,
) {
    let (Some(chronicle), Some(book), Some(mut sift)) = (chronicle, book, sift) else {
        return;
    };
    let ring: Vec<Episode> = chronicle.recent().copied().collect();
    let now = substrate.0.tick();
    let mood_ids = MoodIds::resolve(&reg);
    let vengeance_id = reg.trait_id("vengeance");
    let taboo_active = taboo_in_force(&reg, factions.as_deref());
    let reads = assemble_reads(
        now,
        mood_ids,
        vengeance_id,
        taboo_active,
        npcs.iter(),
        &ring,
    );

    let last = sift.last_ingested;
    for ep in ring.iter().filter(|e| last.is_none_or(|l| e.id > l)) {
        sift.ingest(ep, &book);
    }
    if let Some(e) = ring.last() {
        sift.last_ingested = Some(e.id);
    }
    sift.recompute(&book, &reads);
}

/// Run the **retrospective** sift against the live world and return the updated [`Sift`] (the
/// pattern book and the Chronicle ring must both be present, i.e. the sift layer is woken). Pure /
/// dev-and-eval only — it reads the world and changes no sim state, so calling it never perturbs a
/// run. The result is also written back into the world's [`Sift`] resource.
pub fn run_retrospective(world: &mut World) -> Option<Sift> {
    let book = world.get_resource::<SiftBook>()?.clone();
    let ring: Vec<Episode> = world
        .get_resource::<Chronicle>()?
        .recent()
        .copied()
        .collect();
    let reads = gather_reads(world);
    let mut sift = world.get_resource::<Sift>().cloned().unwrap_or_default();
    sift.resift(&ring, &book, &reads);
    if let Some(mut res) = world.get_resource_mut::<Sift>() {
        *res = sift.clone();
    }
    Some(sift)
}

/// Run **both** matchers over the world's current Chronicle (the retrospective oracle, and the
/// incremental matcher fed the ring's episodes in tick order) and report whether they agree
/// byte-for-byte — the S8.2 acceptance check, over a real run. `None` when the sift layer is off.
/// Dev/test only; reads the world and changes no sim state.
pub fn paths_agree(world: &mut World) -> Option<bool> {
    let book = world.get_resource::<SiftBook>()?.clone();
    let ring: Vec<Episode> = world
        .get_resource::<Chronicle>()?
        .recent()
        .copied()
        .collect();
    let reads = gather_reads(world);

    let mut retro = Sift::default();
    retro.resift(&ring, &book, &reads);

    let mut incr = Sift::default();
    for ep in &ring {
        incr.ingest(ep, &book);
    }
    incr.recompute(&book, &reads);

    Some(retro.candidates == incr.candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_sift_patterns_load_and_validate() {
        let book = SiftBook::bundled();
        assert!(!book.0.is_empty(), "the sift book should ship patterns");
        assert!(
            book.0.iter().any(|p| p.id == "feud_escalating"),
            "the canonical feud pattern is present"
        );
        book.validate().expect("bundled patterns validate");
    }

    #[test]
    fn an_unsound_pattern_is_rejected() {
        // active_at past the window length is a structural error.
        let reg = Registry::bundled();
        let ron = r#"[(
            id: "bad", tension: "bad", register: "vengeance",
            window: [(kind: GrievanceFormed, binds: [("A", Actor)])],
            emerging_at: 1, active_at: 2,
        )]"#;
        let book = SiftBook::from_ron(ron, &reg).unwrap();
        assert!(book.validate().is_err());
    }

    /// A hand-built ring exercising the matcher without the full sim: A grudges B, then A's opinion
    /// of B crosses cold, then A kills B — the feud_escalating arc, end to end.
    #[test]
    fn the_matcher_finds_a_feud_that_escalates_to_a_kill() {
        let pat = SiftPattern {
            id: "feud".into(),
            tension: "feud".into(),
            register: Registry::bundled().register_id("vengeance").unwrap(),
            window: vec![
                WindowStep {
                    kind: EpisodeKind::GrievanceFormed,
                    binds: vec![("A".into(), Slot::Actor), ("B".into(), Slot::Target)],
                    dir: None,
                },
                WindowStep {
                    kind: EpisodeKind::OpinionCrossed,
                    binds: vec![("A".into(), Slot::Actor), ("B".into(), Slot::Target)],
                    dir: Some(Dir::Cold),
                },
                WindowStep {
                    kind: EpisodeKind::Killed,
                    binds: vec![("A".into(), Slot::Actor), ("B".into(), Slot::Target)],
                    dir: None,
                },
            ],
            interest: vec![InterestAxis {
                axis: Axis::Bloodshed,
                curve: Curve::Power { exp: 1.0 },
            }],
            emerging_at: 1,
            active_at: 2,
            window_ticks: 60,
        };
        let mut w = World::new();
        let a = w.spawn_empty().id();
        let b = w.spawn_empty().id();
        let stranger = w.spawn_empty().id();
        let at = Coord::new(3, 4);
        let ep =
            |id: u64, tick: u64, kind: EpisodeKind, p0: Entity, p1: Entity, detail: i32| Episode {
                id,
                tick,
                kind,
                parties: [Some(p0), Some(p1), None],
                place: at,
                register: None,
                detail,
            };
        let ring = vec![
            ep(0, 1, EpisodeKind::GrievanceFormed, a, b, 0),
            // a stranger's unrelated grudge — must NOT bind into A/B's story.
            ep(1, 2, EpisodeKind::GrievanceFormed, stranger, b, 0),
            ep(2, 5, EpisodeKind::OpinionCrossed, a, b, -1),
            ep(3, 9, EpisodeKind::Killed, a, b, 0),
        ];
        let reads = SiftReads {
            now: 9,
            grudge_convergence: HashMap::new(),
            opinion: HashMap::new(),
            mood: HashMap::new(),
            vengeance: HashMap::new(),
            taboo_active: false,
            mood_ids: MoodIds::resolve(&Registry::bundled()),
            by_id: ring.iter().map(|e| (e.id, *e)).collect(),
        };
        let book = SiftBook(vec![pat]);
        let mut sift = Sift::default();
        sift.resift(&ring, &book, &reads);

        let ranked = sift.ranked(0.0);
        let feud = ranked
            .iter()
            .find(|c| c.cast.as_slice() == [a, b])
            .expect("the A->B feud was found");
        assert_eq!(
            feud.status,
            SiftStatus::Resolved,
            "all three steps matched -> the arc resolved"
        );
        assert_eq!(
            feud.support.as_slice(),
            &[0, 2, 3],
            "the stranger's grudge (id 1) is not part of it"
        );
        assert!(
            feud.interest > 0.0,
            "a feud that reaches a killing carries bloodshed interest"
        );
    }

    /// The S8.2 oracle: the incremental matcher (fed the ring episode-by-episode in tick order)
    /// produces byte-identical candidates to the retrospective matcher over the whole ring. Built on
    /// a ring exercising several patterns, two casts, a duplicate start (dedup), and a populated
    /// snapshot so the scoring axes (convergence, opinion, mood) all contribute.
    #[test]
    fn the_incremental_matcher_agrees_with_the_retrospective_oracle() {
        let mut w = World::new();
        let (a, b, c, d) = (
            w.spawn_empty().id(),
            w.spawn_empty().id(),
            w.spawn_empty().id(),
            w.spawn_empty().id(),
        );
        let at = Coord::new(2, 3);
        let ep = |id, tick, kind, p0: Entity, p1: Option<Entity>, detail| Episode {
            id,
            tick,
            kind,
            parties: [Some(p0), p1, None],
            place: at,
            register: None,
            detail,
        };
        let ring = vec![
            ep(0, 1, EpisodeKind::GrievanceFormed, a, Some(b), 0),
            ep(1, 3, EpisodeKind::GrievanceFormed, a, Some(b), 0), // duplicate cast -> dedup
            ep(2, 4, EpisodeKind::GrievanceFormed, c, Some(d), 0),
            ep(3, 6, EpisodeKind::OpinionCrossed, a, Some(b), -1), // A->B sours
            ep(4, 7, EpisodeKind::WarDeclared, c, Some(d), 0),
            ep(5, 9, EpisodeKind::OpinionCrossed, c, Some(d), 1), // C->D warms (a_grudge_forgiven)
            ep(6, 11, EpisodeKind::Killed, a, Some(b), 0),        // A kills B (feud consummated)
            ep(7, 12, EpisodeKind::Death, d, None, 0),
            ep(8, 13, EpisodeKind::Death, c, None, 0),
        ];
        let book = SiftBook::bundled();
        let reg = Registry::bundled();
        let reads = SiftReads {
            now: 20,
            grudge_convergence: [(b, 2u32), (d, 1)].into_iter().collect(),
            opinion: [(a, [(b, -0.9f32)].into_iter().collect())]
                .into_iter()
                .collect(),
            mood: HashMap::new(),
            vengeance: [(a, 0.8f32)].into_iter().collect(),
            taboo_active: true,
            mood_ids: MoodIds::resolve(&reg),
            by_id: ring.iter().map(|e| (e.id, *e)).collect(),
        };

        let mut retro = Sift::default();
        retro.resift(&ring, &book, &reads);

        let mut incr = Sift::default();
        for e in &ring {
            incr.ingest(e, &book);
        }
        incr.recompute(&book, &reads);

        assert!(
            !retro.candidates.is_empty(),
            "the ring should produce candidates"
        );
        assert_eq!(
            retro.candidates, incr.candidates,
            "the incremental matcher must agree with the retrospective oracle, candidate for candidate",
        );
    }
}
