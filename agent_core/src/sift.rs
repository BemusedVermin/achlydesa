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

use crate::ai::Curve;
use crate::beats::Register;
use crate::chronicle::{Chronicle, Episode, EpisodeKind};
use crate::data::Registry;
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

/// A story pattern, as data: an ordered window of episode predicates that bind a cast, the spine it
/// would let the director amplify, and the interest axes it scores on.
#[derive(Deserialize, Clone, Debug)]
pub struct SiftPattern {
    /// Stable id (and the log line), e.g. `"feud_escalating"`.
    pub id: String,
    /// Machine-readable tension label — surfaced only to the dev overlay / eval, never the player.
    pub tension: String,
    /// The spine a candidate of this pattern would feed the director.
    pub register: Register,
    /// The ordered episode window. Non-empty.
    pub window: Vec<WindowStep>,
    /// How a match's interest is computed (surprise + dissonance).
    #[serde(default)]
    pub interest: Vec<InterestAxis>,
    /// Steps matched to reach [`SiftStatus::Emerging`] (≥ 1).
    #[serde(default = "one")]
    pub emerging_at: usize,
    /// Steps matched to reach [`SiftStatus::Active`] (the director-graft threshold).
    #[serde(default = "two")]
    pub active_at: usize,
    /// A partial match whose last episode is older than this (in ticks) has stalled — `Abandoned`.
    #[serde(default = "default_window_ticks")]
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
    /// The patterns shipped with the crate.
    pub fn bundled() -> Self {
        let book = Self::from_ron(Bundled::get(Asset::Sift)).expect("bundled sift patterns are valid RON");
        book.validate().expect("bundled sift patterns are structurally sound");
        book
    }

    /// Parse a sift document.
    pub fn from_ron(ron: &str) -> Result<Self, config::ConfigError> {
        Ok(SiftBook(config::parse(ron)?))
    }

    /// Fail fast on a structurally unsound pattern (empty window, or thresholds out of the
    /// `1 ≤ emerging_at ≤ active_at ≤ window.len()` ordering) — so a typo is caught at load.
    pub fn validate(&self) -> Result<(), String> {
        for p in &self.0 {
            if p.window.is_empty() {
                return Err(format!("sift '{}': empty window", p.id));
            }
            let n = p.window.len();
            if !(1..=n).contains(&p.emerging_at) || !(p.emerging_at..=n).contains(&p.active_at) {
                return Err(format!(
                    "sift '{}': need 1 <= emerging_at ({}) <= active_at ({}) <= window len ({n})",
                    p.id, p.emerging_at, p.active_at,
                ));
            }
        }
        Ok(())
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

/// A forming (or formed) story the sifter perceived: the pattern, how far it got, the cast it bound,
/// the episodes that constitute it, and its interest.
#[derive(Clone, Debug)]
pub struct ThreadCandidate {
    pub pattern: SiftPatternId,
    pub status: SiftStatus,
    /// The bound cast, in the pattern's binding order.
    pub cast: SmallVec<[Entity; 4]>,
    /// The tension label (dev / eval only).
    pub tension: String,
    /// The spine it would feed the director.
    pub register: Register,
    /// Where it is centered (the latest supporting episode's place) — for casting + markers.
    pub place: Coord,
    /// The [`Episode::id`]s that constitute it.
    pub support: SmallVec<[u64; 8]>,
    pub interest: f32,
    pub first_seen: u64,
    pub last_updated: u64,
}

/// The sifter's output (and base-rate memory). A resource, so a sift-free world is byte-identical.
#[derive(Resource, Clone, Debug, Default)]
pub struct Sift {
    candidates: Vec<ThreadCandidate>,
    /// Base-rate counters per pattern — how often each has been seen (the `Rarity` axis).
    seen: HashMap<usize, u64>,
}

impl Sift {
    /// The candidates with interest ≥ `min_interest`, **highest interest first** (ties broken
    /// deterministically by pattern, then first-seen, then the leading episode id) — what the
    /// director and the eval harness read.
    pub fn ranked(&self, min_interest: f32) -> Vec<&ThreadCandidate> {
        let mut out: Vec<&ThreadCandidate> = self.candidates.iter().filter(|c| c.interest >= min_interest).collect();
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

    /// **Retrospective** matcher (the oracle): recompute every candidate from the whole `ring`
    /// against `book`, scoring each on the read-only world snapshot `reads`. Pure — it touches no
    /// sim state. The episodes must be in tick order (the ring's natural order).
    pub(crate) fn resift(&mut self, ring: &[Episode], book: &SiftBook, reads: &SiftReads) {
        self.candidates.clear();
        self.seen.clear();

        // First pass: collect every raw match, and tally base rates per pattern.
        let mut raws: Vec<(usize, RawMatch)> = Vec::new();
        for (pi, pat) in book.0.iter().enumerate() {
            let mut matches = find_matches(pat, ring, reads.now);
            // Dedup: the same (pattern, bound cast) found from many starts is one story — keep the
            // best (most matched, then earliest), so a candidate is reported once.
            matches.sort_by(|a, b| {
                cast_key(a).cmp(&cast_key(b)).then(b.matched.cmp(&a.matched)).then(a.first_tick.cmp(&b.first_tick))
            });
            matches.dedup_by(|a, b| cast_key(a) == cast_key(b));
            for m in matches {
                *self.seen.entry(pi).or_insert(0) += 1;
                raws.push((pi, m));
            }
        }

        // Second pass: classify + score (rarity reads the tallies from the first pass).
        for (pi, m) in raws {
            let pat = &book.0[pi];
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
            let interest = self.score(pat, pi, &m, reads);
            self.candidates.push(ThreadCandidate {
                pattern: SiftPatternId(pi),
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
        let Some(e) = ep.parties[slot.idx()] else { return false };
        let bound = env.iter().chain(staged.iter()).find(|(v, _)| v == var).map(|(_, b)| *b);
        match bound {
            Some(b) if b != e => return false, // reuse mismatch
            Some(_) => {}                       // consistent reuse
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
            cast.iter().map(|e| *reads.vengeance.get(e).unwrap_or(&0.0)).fold(0.0f32, f32::max)
        }
        Axis::MoodReversal => {
            // The deepest low felt by any cast member (the same low the director reads).
            cast.iter().map(|e| reads.mood.get(e).map_or(0.0, |mv| reads.mood_ids.low(mv))).fold(0.0f32, f32::max).clamp(0.0, 1.0)
        }
        Axis::Bloodshed => {
            let any = m.support.iter().any(|&id| {
                reads.episode(id).is_some_and(|ep| matches!(ep.kind, EpisodeKind::Killed | EpisodeKind::Death))
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

/// Gather the read-only world snapshot the scorer needs (live grievance convergence, opinion edges,
/// moods, the vengeance trait, and whether a no-kill taboo is in force), plus an id index over the
/// ring. Needs `&mut World` only because ECS queries build their state, as every accessor does.
pub(crate) fn gather_reads(world: &mut World) -> SiftReads {
    let now = world.resource::<crate::Substrate>().0.tick();
    let mood_ids = MoodIds::resolve(world.resource::<Registry>());
    let vengeance_id = world.resource::<Registry>().trait_id("vengeance");

    // Whether any faction lays a no-kill taboo on the avenge act (`alive(foe) = 0`).
    let taboo_active = {
        let avenge = world.resource::<Registry>().predicate_id("alive").map(|p| (p, 0i64));
        match (avenge, world.get_resource::<Factions>()) {
            (Some(act), Some(f)) => f.0.iter().any(|fac| fac.forbids(act)),
            _ => false,
        }
    };

    let mut grudge_convergence: HashMap<Entity, u32> = HashMap::new();
    let mut opinion: HashMap<Entity, HashMap<Entity, f32>> = HashMap::new();
    let mut mood: HashMap<Entity, Vec<f32>> = HashMap::new();
    let mut vengeance: HashMap<Entity, f32> = HashMap::new();
    {
        let mut q = world.query_filtered::<(Entity, Option<&Grievance>, &Opinion, &Mood, &Personality), With<Npc>>();
        for (e, gr, op, md, pers) in q.iter(world) {
            if let Some(g) = gr {
                *grudge_convergence.entry(g.0).or_insert(0) += 1;
            }
            if !op.0.is_empty() {
                opinion.insert(e, op.0.clone());
            }
            mood.insert(e, md.0.clone());
            if let Some(id) = vengeance_id
                && let Some(&v) = pers.0.get(id)
            {
                vengeance.insert(e, v);
            }
        }
    }

    let by_id: HashMap<u64, Episode> = world
        .get_resource::<Chronicle>()
        .map(|c| c.recent().map(|e| (e.id, *e)).collect())
        .unwrap_or_default();

    SiftReads { now, grudge_convergence, opinion, mood, vengeance, taboo_active, mood_ids, by_id }
}

/// Run the **retrospective** sift against the live world and return the updated [`Sift`] (the
/// pattern book and the Chronicle ring must both be present, i.e. the sift layer is woken). Pure /
/// dev-and-eval only — it reads the world and changes no sim state, so calling it never perturbs a
/// run. The result is also written back into the world's [`Sift`] resource.
pub fn run_retrospective(world: &mut World) -> Option<Sift> {
    let book = world.get_resource::<SiftBook>()?.clone();
    let ring: Vec<Episode> = world.get_resource::<Chronicle>()?.recent().copied().collect();
    let reads = gather_reads(world);
    let mut sift = world.get_resource::<Sift>().cloned().unwrap_or_default();
    sift.resift(&ring, &book, &reads);
    if let Some(mut res) = world.get_resource_mut::<Sift>() {
        *res = sift.clone();
    }
    Some(sift)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_sift_patterns_load_and_validate() {
        let book = SiftBook::bundled();
        assert!(!book.0.is_empty(), "the sift book should ship patterns");
        assert!(book.0.iter().any(|p| p.id == "feud_escalating"), "the canonical feud pattern is present");
        book.validate().expect("bundled patterns validate");
    }

    #[test]
    fn an_unsound_pattern_is_rejected() {
        // active_at past the window length is a structural error.
        let ron = r#"[(
            id: "bad", tension: "bad", register: Vengeance,
            window: [(kind: GrievanceFormed, binds: [("A", Actor)])],
            emerging_at: 1, active_at: 2,
        )]"#;
        let book = SiftBook::from_ron(ron).unwrap();
        assert!(book.validate().is_err());
    }

    /// A hand-built ring exercising the matcher without the full sim: A grudges B, then A's opinion
    /// of B crosses cold, then A kills B — the feud_escalating arc, end to end.
    #[test]
    fn the_matcher_finds_a_feud_that_escalates_to_a_kill() {
        let pat = SiftPattern {
            id: "feud".into(),
            tension: "feud".into(),
            register: Register::Vengeance,
            window: vec![
                WindowStep { kind: EpisodeKind::GrievanceFormed, binds: vec![("A".into(), Slot::Actor), ("B".into(), Slot::Target)], dir: None },
                WindowStep { kind: EpisodeKind::OpinionCrossed, binds: vec![("A".into(), Slot::Actor), ("B".into(), Slot::Target)], dir: Some(Dir::Cold) },
                WindowStep { kind: EpisodeKind::Killed, binds: vec![("A".into(), Slot::Actor), ("B".into(), Slot::Target)], dir: None },
            ],
            interest: vec![InterestAxis { axis: Axis::Bloodshed, curve: Curve::Power { exp: 1.0 } }],
            emerging_at: 1,
            active_at: 2,
            window_ticks: 60,
        };
        let mut w = World::new();
        let a = w.spawn_empty().id();
        let b = w.spawn_empty().id();
        let stranger = w.spawn_empty().id();
        let at = Coord::new(3, 4);
        let ep = |id: u64, tick: u64, kind: EpisodeKind, p0: Entity, p1: Entity, detail: i32| Episode {
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
        assert_eq!(feud.status, SiftStatus::Resolved, "all three steps matched -> the arc resolved");
        assert_eq!(feud.support.as_slice(), &[0, 2, 3], "the stranger's grudge (id 1) is not part of it");
        assert!(feud.interest > 0.0, "a feud that reaches a killing carries bloodshed interest");
    }
}
