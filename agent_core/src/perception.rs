//! The **Perception Layer** — the last hop that turns the drama the sim already stages (and the
//! [`sift`](crate::sift)er already perceives) into something the player can *read*
//! (`docs/perception_layer.md`).
//!
//! The simulation stages drama (feuds escalate, the beloved falls, wars are declared, Γ manufactures
//! arcs onto named souls); the [`Sift`] already perceives the **forming** stories bottom-up. What was
//! missing is the last hop: none of it reaches the player as something legible. This layer is that
//! hop. It introduces one atom — a [`Tell`], a structured, prose-free, salience-ranked unit of
//! legible information derived from the [`Chronicle`] + [`Sift`] — and one rendering contract — a
//! [`Realizer`] that turns a `Tell` into a medium (a line of prose, a charged place, a scan readout,
//! a timeline marker). Every player-facing surface is then the *same* ranked set of `Tell`s under a
//! different filter + [`Realizer`].
//!
//! Crucially every `Tell` carries [`Provenance`](crate::chronicle::Provenance) — was the state it
//! reports *grown* by the sim or *written* by Γ — and the deepest reads surface that distinction. The
//! meta-plot is the player learning to see the demiurge's seams, so **legibility is the theme**.
//!
//! **Deterministic & off-by-default.** The whole pass is pure arithmetic over the deterministic
//! `Chronicle` + a read-only `Sift`/world snapshot; it writes **only** its own [`Perception`]
//! resource and touches no sim state. `Tell`s are a resource `Vec`, never spawned entities (spawning
//! would churn archetypes and risk perturbing iteration order — the precise thing `Chronicle`/`Sift`
//! avoid). With the layer off the resource is absent and [`perception_step`] early-returns, so a
//! perception-free world is byte-identical to one before this layer existed.

use crate::chronicle::{Chronicle, Episode, EpisodeKind, Provenance};
use crate::data::{RegisterId, Registry};
use crate::player::PlayerState;
use crate::sift::{Sift, SiftStatus, ThreadCandidate};
use crate::{Position, Substrate};
use bevy_ecs::prelude::*;
use game_sim::Coord;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

// --- The atom ---

/// One renderable unit of legible information — a *tell*: a legible cue you learn to read, the root
/// of *telling a story*, and (at a [`Deep`](ReadTier::Deep) read) Γ's hand. A plain struct held in
/// the [`Perception`] resource's `Vec` — **not** a `Component`. Prose-free: a [`Realizer`] renders it,
/// so the one `Tell` can become a line, a glyph, a scan row, or a marker.
#[derive(Clone, Debug)]
pub struct Tell {
    /// Who/what this is about (a soul, later a POI or faction).
    pub subject: Entity,
    /// The cue the player reads (tension / threat / aftermath / …).
    pub kind: TellKind,
    /// The ranked-filter input — why this `Tell` rises (see [`salience`](Perception)).
    pub salience: f32,
    /// Grown by the sim, a soul's own deed, or written by Γ — the theme hook.
    pub provenance: Provenance,
    /// Spatial + temporal placement; surfaces filter on it.
    pub anchor: Anchor,
    /// The [`Episode::id`]s this derives from — every `Tell` stays traceable to the Chronicle.
    pub source: SmallVec<[u64; 4]>,
    /// Structured payload a [`Realizer`] consumes (never pre-baked prose).
    pub hints: RealizeHints,
}

/// The cue a `Tell` carries. The player never sees this label — a [`Realizer`] renders the *fiction*;
/// the kind is a dev-surface input and the selector the surfaces filter on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TellKind {
    /// A forming pressure not yet broken into violence (a converging feud, a souring bond).
    Tension,
    /// A state the player could exploit (arrives with the scan verb).
    Opportunity,
    /// A forming story that has already drawn blood and may again.
    Threat,
    /// Something that does not follow from what came before — the authorship anomaly's home.
    Mystery,
    /// An arc that has already played out — the prose log / map history.
    Aftermath,
    /// A subject/motif that recurs across stories (Phase 5 apophenia).
    Recurrence,
}

/// Spatial + temporal placement, so a surface filters with a predicate (no marker entity needed).
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub place: Option<Coord>,
    pub when: When,
}

/// *When* a `Tell` sits in time — the surfaces split on this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// Already happened — the prose log, the map history.
    Past(u64),
    /// Current standing state — the read-the-room scan.
    Now,
    /// A committed future intent at a combat tick — the timeline telegraph (Phase 4).
    Scheduled(u32),
}

/// Structured render hints, never pre-baked. One `Tell` renders as prose, a charged place, a scan
/// row, or a marker — the [`Realizer`] decides.
#[derive(Clone, Debug)]
pub struct RealizeHints {
    /// The cast the line/marker may name.
    pub actors: SmallVec<[Entity; 4]>,
    /// The dramatic register (maps into the tagged grammar and, later, glyph/icon tables). `None`
    /// for a `Tell` that has no register (a bare death).
    pub register: Option<RegisterId>,
    /// Emphasis (glow, weight) — a **dev-surface** input; no player Realizer renders it as a number.
    pub magnitude: f32,
    /// The minimum read-tier to reveal this `Tell`'s gated content. A Γ-authored `Tell` gates its
    /// *provenance* behind [`Deep`](ReadTier::Deep) — the recognition beat (S6).
    pub tier_gate: ReadTier,
}

/// Progressive disclosure (S5.3): how deep a read a `Tell`'s gated content needs. `Ord`, so a surface
/// reveals everything at or below its current tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReadTier {
    /// Free / passive — *that* a soul is charged, as demeanour.
    Glance,
    /// A verb / time — what it is pursuing and its salient social fact.
    Read,
    /// Skill / tempo — the **provenance**: whether this charge was grown or authored.
    Deep,
}

// --- The render contract (concrete Realizers land in Phase 1+) ---

/// Everything a [`Realizer`] needs from the live world to render a `Tell` into its medium. Grows as
/// surfaces land; in Phase 1 the [`GrammarRealizer`] needs the registry (for the register's phrasing
/// templates) and a name resolver (entity → the soul's display name).
pub struct RealizeCtx<'a> {
    /// The content registry — register definitions, mood/trait names a line may reach for.
    pub registry: &'a Registry,
    /// Resolve an entity to the name the player knows it by (the dialogue display name).
    pub name: &'a dyn Fn(Entity) -> String,
}

/// Turns one `Tell` into one medium. Each Realizer is small, swappable, independently testable — the
/// reuse that collapses the surfaces into one contract. [`GrammarRealizer`] (prose) lands in Phase 1;
/// `PlaceRealizer` (drama-map), `ScanRowRealizer` (read-the-room), `TimelineRealizer` (combat), and
/// the optional `SlmRealizer` follow, all behind this one trait.
pub trait Realizer {
    type Out;
    fn realize(&self, tell: &Tell, ctx: &RealizeCtx) -> Self::Out;
}

/// The **prose** Realizer (S5.1): render a `Tell` as one terse line of recollection, reusing the
/// authored **register templates** (`RegisterDef::told` / `noun`) the gossip surface already fills —
/// "wiring an existing system to a new input", with zero new prose tech. A register-less `Tell` (a
/// bare death) falls back to a minimal connective line. The optional `SlmRealizer` (Phase 5) re-voices
/// this same `Tell` behind the same trait, with the grammar line as its byte-identical fallback.
pub struct GrammarRealizer;

impl Realizer for GrammarRealizer {
    type Out = String;

    fn realize(&self, tell: &Tell, ctx: &RealizeCtx) -> String {
        let lead = (ctx.name)(tell.subject);
        // The salient counterpart is the first cast member who is not the subject.
        let other = tell
            .hints
            .actors
            .iter()
            .copied()
            .find(|&e| e != tell.subject)
            .map(ctx.name);
        match tell.hints.register {
            Some(reg) => {
                let def = ctx.registry.register_def(reg);
                fill_template(&def.told, &lead, other.as_deref(), &def.noun)
            }
            None => generic_line(tell.kind, &lead, other.as_deref()),
        }
    }
}

/// Fill a register phrasing template's `{lead}` / `{other}` / `{noun}` slots. A missing counterpart
/// reads as "someone" — the lacuna the player's pattern-matching closes (S7).
fn fill_template(tmpl: &str, lead: &str, other: Option<&str>, noun: &str) -> String {
    tmpl.replace("{lead}", lead)
        .replace("{other}", other.unwrap_or("someone"))
        .replace("{noun}", noun)
}

/// The minimal fallback for a `Tell` with no register (a bare death) — terse by design (restraint,
/// S5.1). Most candidate-derived `Tell`s carry a register, so this is rarely reached.
fn generic_line(kind: TellKind, lead: &str, other: Option<&str>) -> String {
    match (kind, other) {
        (TellKind::Aftermath, Some(o)) => {
            format!("Word is that {lead} and {o} have met their end.")
        }
        (TellKind::Aftermath, None) => format!("They say {lead} is gone."),
        (TellKind::Threat, Some(o)) => format!("Blood stands between {lead} and {o}."),
        (TellKind::Threat, None) => format!("Something violent gathers around {lead}."),
        (_, Some(o)) => format!("Something uneasy moves between {lead} and {o}."),
        (_, None) => format!("Something stirs around {lead}."),
    }
}

/// One soul's readout in a **read-the-room** scan (S5.3): what the player learns about a charged soul
/// in the current cell, disclosed only as deep as the scan reached. The app renders these rows; the
/// fields fill in tier by tier, so a `Glance` row carries demeanour alone and a `Deep` row may carry
/// the authorship reveal.
#[derive(Clone, Debug)]
pub struct ScanLine {
    pub subject: Entity,
    /// The soul's name.
    pub name: String,
    /// `Glance`: the charge read as demeanour / an earned epithet — no number.
    pub demeanour: String,
    /// `Read`: what the soul is living through (its register situation). `None` below `Read`.
    pub pursuit: Option<String>,
    /// `Deep`: the authorship reveal — `Some` only for a Γ-authored charge read at the `Deep` tier.
    /// *This is the recognition beat:* the player learns the grief was placed, not grown.
    pub authored: Option<String>,
    /// The tier this row was disclosed at.
    pub tier: ReadTier,
}

/// The **scan** Realizer (S5.3): render a `Tell` as one soul's read-the-room row, disclosed to the
/// configured [`ReadTier`]. `Glance` gives demeanour; `Read` adds what the soul is living through;
/// `Deep` — and only `Deep` — surfaces a Γ-authored charge as *placed, not grown*. Reuses the
/// register's authored epithet / situation / noun, so it bakes no new prose. The tier is held on the
/// realizer (the scan configures it from the verb's cost / a skill check), keeping the one-arg
/// `Realizer::realize` shape.
pub struct ScanRowRealizer {
    pub tier: ReadTier,
}

impl Realizer for ScanRowRealizer {
    type Out = ScanLine;

    fn realize(&self, tell: &Tell, ctx: &RealizeCtx) -> ScanLine {
        let name = (ctx.name)(tell.subject);
        let def = tell.hints.register.map(|r| ctx.registry.register_def(r));
        // Glance: the earned epithet conveys the charge as demeanour; a register-less Tell falls back
        // to a bare charge word.
        let demeanour = match def {
            Some(d) if !d.epithet_lead.is_empty() => d.epithet_lead.clone(),
            _ => charge_word(tell.kind).to_string(),
        };
        // Read: what the soul is living through (its register situation).
        let pursuit = if self.tier >= ReadTier::Read {
            def.map(|d| d.situation_lead.clone())
                .filter(|s| !s.is_empty())
        } else {
            None
        };
        // Deep — and only at/above the Tell's gate — reveals Γ's hand, and only when there *is* a
        // hand: a grown (Sim/Agent) charge has nothing to confess, so a Deep read finds nothing.
        let authored =
            if self.tier >= tell.hints.tier_gate && tell.provenance == Provenance::Director {
                let noun = def.map(|d| d.noun.as_str()).unwrap_or("charge");
                Some(format!("This {noun} did not grow here — it was placed."))
            } else {
                None
            };
        ScanLine {
            subject: tell.subject,
            name,
            demeanour,
            pursuit,
            authored,
            tier: self.tier,
        }
    }
}

/// A bare demeanour word for a register-less `Tell` (the rare fallback) — terse, structural.
fn charge_word(kind: TellKind) -> &'static str {
    match kind {
        TellKind::Threat => "dangerous",
        TellKind::Aftermath => "haunted",
        TellKind::Tension => "uneasy",
        TellKind::Mystery => "unreadable",
        TellKind::Opportunity => "unguarded",
        TellKind::Recurrence => "strangely familiar",
    }
}

/// A player channel: a filter + a [`Realizer`] + a budget over the shared `Tell` set. The budget is
/// not incidental — a surface that shows everything is as illegible as one that shows nothing, so the
/// budget is what makes salience do real work and creates restraint.
pub trait Surface {
    type R: Realizer;
    /// The spatial/temporal/kind predicate this channel selects on.
    fn select<'a>(&self, p: &'a Perception) -> impl Iterator<Item = &'a Tell>;
    fn realizer(&self) -> &Self::R;
    /// Max `Tell`s shown — forces salience to rank.
    fn budget(&self) -> usize;
}

// --- The resource + the pass ---

/// The salience weights + thresholds the pass reads, lifted from [`config::PerceptionConfig`] when the
/// layer wakes (kept on the resource so [`perception_step`] needs no extra system param).
#[derive(Clone, Copy, Debug)]
pub struct PerceptionCfg {
    /// The least salience a `Tell` must reach to be kept — the budget floor that forces restraint.
    pub min_salience: f32,
    /// Hexes within which a forming story counts as "near the avatar" (the proximity term's reach).
    pub reach: i32,
    /// Weight on the Sifter's interest (the surprise + dissonance it already scores on real state).
    pub w_dissonance: f32,
    /// Weight on spatial proximity to the avatar (the term is 0 in any player-less run).
    pub w_proximity: f32,
    /// Weight on authorship anomaly — how much of the story Γ *wrote* vs. the world *grew*.
    pub w_authorship: f32,
    /// Weight on **attachment** — a cast holding a `Bond` to the avatar rises (the emotional-story
    /// hook). Inert (0 contribution) in any player-less / bond-less run.
    pub w_bond: f32,
    /// Weight on recurrence — a subject/motif recurring across stories (Phase 5; inert at 0 for now).
    pub w_recurrence: f32,
}

impl Default for PerceptionCfg {
    fn default() -> Self {
        Self {
            min_salience: 0.0,
            reach: 6,
            w_dissonance: 1.0,
            w_proximity: 1.0,
            w_authorship: 0.75,
            w_bond: 1.5,
            w_recurrence: 0.5,
        }
    }
}

/// The Perception Layer's output: the salience-ranked `Tell`s, recomputed each pass. A resource (no
/// component), so a perception-free world is byte-identical (absent ⇒ the pass early-returns). Every
/// surface is a `filter().take(budget)` over [`tells`](Self::tells).
#[derive(Resource, Default)]
pub struct Perception {
    tells: Vec<Tell>,
    cfg: PerceptionCfg,
}

impl Perception {
    /// A fresh `Perception` configured from the tunables (called once when the layer wakes).
    pub fn from_config(cfg: &config::PerceptionConfig) -> Self {
        Self {
            tells: Vec::new(),
            cfg: PerceptionCfg {
                min_salience: cfg.min_salience,
                reach: cfg.reach,
                w_dissonance: cfg.w_dissonance,
                w_proximity: cfg.w_proximity,
                w_authorship: cfg.w_authorship,
                w_bond: cfg.w_bond,
                w_recurrence: cfg.w_recurrence,
            },
        }
    }

    /// All current `Tell`s, highest-salience first — the set every surface filters.
    pub fn tells(&self) -> &[Tell] {
        &self.tells
    }

    /// The top `n` `Tell`s by salience (the budgeted view a surface takes).
    pub fn top(&self, n: usize) -> impl Iterator<Item = &Tell> {
        self.tells.iter().take(n)
    }

    /// `Tell`s that have already happened — the prose-log / map-history filter.
    pub fn past(&self) -> impl Iterator<Item = &Tell> {
        self.tells
            .iter()
            .filter(|t| matches!(t.anchor.when, When::Past(_)))
    }

    /// `Tell`s anchored on `place` — the drama-map's per-POI filter.
    pub fn at(&self, place: Coord) -> impl Iterator<Item = &Tell> + '_ {
        self.tells
            .iter()
            .filter(move |t| t.anchor.place == Some(place))
    }
}

/// The **Perception pass**: each tick, recompute the ranked `Tell`s from the Chronicle ring and the
/// Sifter's forming stories, scored by salience (the Sifter's interest + proximity to the avatar +
/// how much of the story Γ authored). Stateless — recomputed each pass (`docs/perception_layer.md`
/// S10 Q1). Scheduled last, after the sifter/director/gossip have shaped this tick, so it reads the
/// freshest Chronicle (including this tick's beats). Reads only — it writes its own [`Perception`]
/// resource and touches no sim state — so a perception-off (resource-absent) world is byte-identical.
pub(crate) fn perception_step(
    perception: Option<ResMut<Perception>>,
    chronicle: Option<Res<Chronicle>>,
    sift: Option<Res<Sift>>,
    player: Option<Res<PlayerState>>,
    substrate: Res<Substrate>,
    positions: Query<&Position>,
    bonds: Query<(Entity, &crate::people::Bond)>,
) {
    let (Some(mut perception), Some(chronicle), Some(sift)) = (perception, chronicle, sift) else {
        return;
    };
    let cfg = perception.cfg;
    let width = substrate.0.topology().width();
    // The avatar-weighted terms — both inert (avatar None ⇒ proximity 0, bonded empty) in any
    // headless or player-less run, the precedent the director's `draw` bias sets, so V&V stays
    // byte-identical.
    let avatar = player.as_ref().and_then(|p| p.avatar());
    let avatar_pos = avatar.and_then(|a| positions.get(a).ok()).map(|p| p.0);
    // The souls who hold a `Bond` to the avatar — "your people", whose stories the attachment term
    // lifts above the indifferent crowd (the emotional-story hook).
    let bonded: HashSet<Entity> = match avatar {
        Some(av) => bonds
            .iter()
            .filter(|(_, b)| b.0 == av)
            .map(|(e, _)| e)
            .collect(),
        None => HashSet::new(),
    };
    let by_id: HashMap<u64, &Episode> = chronicle.recent().map(|e| (e.id, e)).collect();

    let mut tells: Vec<Tell> = sift
        .candidates()
        .iter()
        .filter_map(|cand| tell_from_candidate(cand, &by_id, avatar_pos, &bonded, width, &cfg))
        .collect();
    // Highest salience first; deterministic tiebreak by the leading source episode, then subject — a
    // total order so the ranking is reproducible run to run.
    tells.sort_by(|a, b| {
        b.salience
            .partial_cmp(&a.salience)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.source.first().cmp(&b.source.first()))
            .then(a.subject.cmp(&b.subject))
    });
    perception.tells = tells;
}

/// Derive one `Tell` from a forming (or just-resolved) story. `None` for an `Abandoned` stall or a
/// story below the salience floor.
fn tell_from_candidate(
    cand: &ThreadCandidate,
    by_id: &HashMap<u64, &Episode>,
    avatar_pos: Option<Coord>,
    bonded: &HashSet<Entity>,
    width: i32,
    cfg: &PerceptionCfg,
) -> Option<Tell> {
    if !(cand.status.is_forming() || cand.status == SiftStatus::Resolved) {
        return None;
    }
    let &subject = cand.cast.first()?;
    let (provenance, authorship) = provenance_of(&cand.support, by_id);
    let proximity = match avatar_pos {
        Some(ap) => proximity_term(ap, cand.place, width, cfg.reach),
        None => 0.0,
    };
    // Attachment: any cast member bonded to the avatar lifts the whole story (the emotional-story hook).
    let bond = if cand.cast.iter().any(|e| bonded.contains(e)) {
        1.0
    } else {
        0.0
    };
    let salience = cfg.w_dissonance * cand.interest
        + cfg.w_proximity * proximity
        + cfg.w_authorship * authorship
        + cfg.w_bond * bond;
    if salience < cfg.min_salience {
        return None;
    }
    let when = match cand.status {
        SiftStatus::Resolved => When::Past(cand.last_updated),
        _ => When::Now,
    };
    Some(Tell {
        subject,
        kind: kind_of(cand, by_id),
        salience,
        provenance,
        anchor: Anchor {
            place: Some(cand.place),
            when,
        },
        source: cand.support.iter().copied().collect(),
        hints: RealizeHints {
            actors: cand.cast.iter().copied().collect(),
            register: Some(cand.register),
            magnitude: cand.interest,
            tier_gate: tier_of(provenance),
        },
    })
}

/// Derive a story's provenance and its **authorship anomaly** from its supporting episodes. A story
/// Γ wrote outright (all support `Director`) reads maximally anomalous; one Γ merely *amplified* (a
/// lone `Director` episode atop emergent ones) reads far less so — the staged-vs-amplified
/// distinction the Deep scan surfaces (S6). An all-`Sim`/`Agent` story has anomaly 0.
fn provenance_of(support: &[u64], by_id: &HashMap<u64, &Episode>) -> (Provenance, f32) {
    let mut director = 0u32;
    let mut total = 0u32;
    let mut agent: Option<Entity> = None;
    for id in support {
        let Some(ep) = by_id.get(id) else { continue };
        total += 1;
        match ep.provenance {
            Provenance::Director => director += 1,
            Provenance::Agent(e) => agent = agent.or(Some(e)),
            Provenance::Sim => {}
        }
    }
    let provenance = if director > 0 {
        Provenance::Director
    } else if let Some(e) = agent {
        Provenance::Agent(e)
    } else {
        Provenance::Sim
    };
    let anomaly = if total == 0 {
        0.0
    } else {
        director as f32 / total as f32
    };
    (provenance, anomaly)
}

/// Map a forming story to the cue the player reads. A resolved arc is `Aftermath`; a forming one that
/// has already drawn blood is a `Threat`; an unbled forming one is `Tension`. (`Opportunity` /
/// `Mystery` / `Recurrence` arrive with the scan verb and Phase 5.)
fn kind_of(cand: &ThreadCandidate, by_id: &HashMap<u64, &Episode>) -> TellKind {
    let bloodshed = cand.support.iter().any(|id| {
        by_id
            .get(id)
            .is_some_and(|ep| matches!(ep.kind, EpisodeKind::Killed | EpisodeKind::Death))
    });
    match (cand.status, bloodshed) {
        (SiftStatus::Resolved, _) => TellKind::Aftermath,
        (_, true) => TellKind::Threat,
        (_, false) => TellKind::Tension,
    }
}

/// Γ's hand is content gated behind a `Deep` read — the recognition beat (S6). Everything else reads
/// openly.
fn tier_of(provenance: Provenance) -> ReadTier {
    match provenance {
        Provenance::Director => ReadTier::Deep,
        _ => ReadTier::Glance,
    }
}

/// Wrapped Chebyshev hex distance (the world wraps east–west) — mirrors the director's avatar-draw
/// proximity. Kept local so the Perception pass owns its own cheap geometry.
fn hex_dist(a: Coord, b: Coord, width: i32) -> i32 {
    let drow = (a.row - b.row).abs();
    let dcol = {
        let raw = (a.col - b.col).abs();
        raw.min(width - raw)
    };
    drow.max(dcol)
}

/// A `0..1` proximity weight: 1 on the avatar's tile, fading toward 0 at `reach`, 0 beyond.
fn proximity_term(avatar: Coord, place: Coord, width: i32, reach: i32) -> f32 {
    let d = hex_dist(avatar, place, width);
    if d > reach {
        0.0
    } else {
        1.0 - d as f32 / (reach.max(1) + 1) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sift::SiftPatternId;

    /// A bare episode at id `id` with the given kind + provenance (no cast/place needed for the
    /// provenance/kind helpers).
    fn ep(id: u64, kind: EpisodeKind, prov: Provenance) -> Episode {
        Episode {
            id,
            tick: id,
            kind,
            provenance: prov,
            parties: [None; 3],
            place: Coord::new(0, 0),
            register: None,
            detail: 0,
        }
    }

    fn id_map(eps: &[Episode]) -> HashMap<u64, &Episode> {
        eps.iter().map(|e| (e.id, e)).collect()
    }

    fn cand(
        cast: &[Entity],
        status: SiftStatus,
        interest: f32,
        support: &[u64],
    ) -> ThreadCandidate {
        ThreadCandidate {
            pattern: SiftPatternId(0),
            status,
            cast: cast.iter().copied().collect(),
            tension: "feud".into(),
            register: 0,
            place: Coord::new(2, 3),
            support: support.iter().copied().collect(),
            interest,
            first_seen: 0,
            last_updated: 10,
        }
    }

    #[test]
    fn provenance_distinguishes_staged_from_amplified() {
        // Staged: every supporting episode is Γ's — maximally anomalous.
        let staged = [
            ep(0, EpisodeKind::GrievanceFormed, Provenance::Director),
            ep(1, EpisodeKind::OpinionCrossed, Provenance::Director),
        ];
        let (p, a) = provenance_of(&[0, 1], &id_map(&staged));
        assert_eq!(p, Provenance::Director);
        assert!((a - 1.0).abs() < 1e-6, "all-Γ support reads anomaly 1.0");

        // Amplified: Γ touched one episode atop two emergent ones — far less anomalous, still Director.
        let amplified = [
            ep(0, EpisodeKind::GrievanceFormed, Provenance::Sim),
            ep(1, EpisodeKind::OpinionCrossed, Provenance::Sim),
            ep(2, EpisodeKind::Killed, Provenance::Director),
        ];
        let (p2, a2) = provenance_of(&[0, 1, 2], &id_map(&amplified));
        assert_eq!(p2, Provenance::Director);
        assert!(
            a2 > 0.0 && a2 < 0.5,
            "one-of-three Γ episodes reads ~0.33, got {a2}"
        );

        // Wholly grown: no anomaly, Sim provenance.
        let grown = [ep(0, EpisodeKind::GrievanceFormed, Provenance::Sim)];
        let (p3, a3) = provenance_of(&[0], &id_map(&grown));
        assert_eq!(p3, Provenance::Sim);
        assert_eq!(a3, 0.0);
    }

    #[test]
    fn an_agents_own_deed_keeps_agent_provenance() {
        let mut w = World::new();
        let slayer = w.spawn_empty().id();
        let eps = [
            ep(0, EpisodeKind::GrievanceFormed, Provenance::Sim),
            ep(1, EpisodeKind::Killed, Provenance::Agent(slayer)),
        ];
        let (p, a) = provenance_of(&[0, 1], &id_map(&eps));
        assert_eq!(p, Provenance::Agent(slayer));
        assert_eq!(a, 0.0, "an agent's own deed is not Γ's authorship");
    }

    #[test]
    fn kind_reflects_status_and_bloodshed() {
        let mut w = World::new();
        let (a, b) = (w.spawn_empty().id(), w.spawn_empty().id());
        let bled = [ep(0, EpisodeKind::Killed, Provenance::Sim)];
        let dry = [ep(0, EpisodeKind::GrievanceFormed, Provenance::Sim)];

        let resolved = cand(&[a, b], SiftStatus::Resolved, 1.0, &[0]);
        assert_eq!(kind_of(&resolved, &id_map(&bled)), TellKind::Aftermath);

        let forming_bled = cand(&[a, b], SiftStatus::Active, 1.0, &[0]);
        assert_eq!(kind_of(&forming_bled, &id_map(&bled)), TellKind::Threat);

        let forming_dry = cand(&[a, b], SiftStatus::Emerging, 1.0, &[0]);
        assert_eq!(kind_of(&forming_dry, &id_map(&dry)), TellKind::Tension);
    }

    #[test]
    fn proximity_fades_with_distance() {
        let here = Coord::new(5, 5);
        // On the tile: full weight. At reach: small but positive. Beyond reach: zero.
        assert!((proximity_term(here, here, 64, 6) - 1.0).abs() < 1e-6);
        let at_reach = proximity_term(here, Coord::new(5, 11), 64, 6);
        assert!(
            at_reach > 0.0 && at_reach < 0.2,
            "edge of reach is faint, got {at_reach}"
        );
        assert_eq!(proximity_term(here, Coord::new(5, 20), 64, 6), 0.0);
    }

    #[test]
    fn a_director_authored_story_is_a_deep_gated_tell() {
        let mut w = World::new();
        let (lead, foe) = (w.spawn_empty().id(), w.spawn_empty().id());
        // A feud Γ staged outright (both episodes Director) that has resolved in a kill.
        let eps = [
            ep(0, EpisodeKind::GrievanceFormed, Provenance::Director),
            ep(1, EpisodeKind::Killed, Provenance::Director),
        ];
        let c = cand(&[lead, foe], SiftStatus::Resolved, 0.8, &[0, 1]);
        let tell = tell_from_candidate(
            &c,
            &id_map(&eps),
            None,
            &HashSet::new(),
            64,
            &PerceptionCfg::default(),
        )
        .expect("a resolved Γ feud is a Tell");
        assert_eq!(tell.subject, lead, "the lead of the cast is the subject");
        assert_eq!(tell.provenance, Provenance::Director);
        assert_eq!(tell.kind, TellKind::Aftermath);
        assert_eq!(
            tell.hints.tier_gate,
            ReadTier::Deep,
            "Γ's hand needs a Deep read"
        );
        assert_eq!(tell.source.as_slice(), &[0, 1], "traceable to its episodes");
        // No avatar ⇒ proximity inert ⇒ salience is interest + full authorship anomaly only.
        let expect = 0.8 + 0.75 * 1.0;
        assert!(
            (tell.salience - expect).abs() < 1e-6,
            "salience {} != {expect}",
            tell.salience
        );
    }

    #[test]
    fn salience_is_monotonic_in_interest() {
        let mut w = World::new();
        let (a, b) = (w.spawn_empty().id(), w.spawn_empty().id());
        let eps = [ep(0, EpisodeKind::GrievanceFormed, Provenance::Sim)];
        let map = id_map(&eps);
        let cfg = PerceptionCfg::default();
        let none = HashSet::new();
        let low = tell_from_candidate(
            &cand(&[a, b], SiftStatus::Active, 0.2, &[0]),
            &map,
            None,
            &none,
            64,
            &cfg,
        )
        .unwrap();
        let high = tell_from_candidate(
            &cand(&[a, b], SiftStatus::Active, 0.9, &[0]),
            &map,
            None,
            &none,
            64,
            &cfg,
        )
        .unwrap();
        assert!(
            high.salience > low.salience,
            "more Sifter interest ⇒ more salience"
        );
    }

    #[test]
    fn a_bond_to_the_avatar_raises_salience() {
        // The emotional-story hook: two identical stories, but one names a soul bonded to the avatar —
        // and that one rises. (When-this-matters: it is what makes "your people" surface.)
        let mut w = World::new();
        let (a, b) = (w.spawn_empty().id(), w.spawn_empty().id());
        let eps = [ep(0, EpisodeKind::GrievanceFormed, Provenance::Sim)];
        let map = id_map(&eps);
        let cfg = PerceptionCfg::default();
        let c = cand(&[a, b], SiftStatus::Active, 0.3, &[0]);

        let none = HashSet::new();
        let bare = tell_from_candidate(&c, &map, None, &none, 64, &cfg).unwrap();

        let bonded: HashSet<Entity> = [a].into_iter().collect();
        let dear = tell_from_candidate(&c, &map, None, &bonded, 64, &cfg).unwrap();

        assert!(
            (dear.salience - bare.salience - cfg.w_bond).abs() < 1e-6,
            "a bonded cast adds exactly w_bond ({}) to salience",
            cfg.w_bond
        );
        assert!(dear.salience > bare.salience, "your people rise");
    }

    #[test]
    fn the_grammar_realizer_fills_a_register_template() {
        // The prose Realizer reuses the authored register template, filling {lead}/{other}.
        let reg = Registry::bundled();
        let betrayal = reg
            .register_id("betrayal")
            .expect("the betrayal register ships");
        let mut w = World::new();
        let (lead, other) = (w.spawn_empty().id(), w.spawn_empty().id());
        let tell = Tell {
            subject: lead,
            kind: TellKind::Aftermath,
            salience: 1.0,
            provenance: Provenance::Sim,
            anchor: Anchor {
                place: None,
                when: When::Past(1),
            },
            source: [0u64].into_iter().collect(),
            hints: RealizeHints {
                actors: [lead, other].into_iter().collect(),
                register: Some(betrayal),
                magnitude: 1.0,
                tier_gate: ReadTier::Glance,
            },
        };
        let names = |e: Entity| {
            if e == lead {
                "Aldric".to_string()
            } else {
                "Mara".to_string()
            }
        };
        let ctx = RealizeCtx {
            registry: &reg,
            name: &names,
        };
        let line = GrammarRealizer.realize(&tell, &ctx);
        assert!(line.contains("Aldric"), "names the lead: {line}");
        assert!(line.contains("Mara"), "names the counterpart: {line}");
        assert!(
            !line.contains('{'),
            "every template slot was filled: {line}"
        );
    }

    /// Build a one-soul `Tell` over the betrayal register with the given provenance, for scan tests.
    fn scan_tell(subject: Entity, provenance: Provenance, reg: &Registry) -> Tell {
        let betrayal = reg
            .register_id("betrayal")
            .expect("betrayal register ships");
        let gate = if provenance == Provenance::Director {
            ReadTier::Deep
        } else {
            ReadTier::Glance
        };
        Tell {
            subject,
            kind: TellKind::Aftermath,
            salience: 1.0,
            provenance,
            anchor: Anchor {
                place: None,
                when: When::Now,
            },
            source: [0u64].into_iter().collect(),
            hints: RealizeHints {
                actors: [subject].into_iter().collect(),
                register: Some(betrayal),
                magnitude: 1.0,
                tier_gate: gate,
            },
        }
    }

    #[test]
    fn the_scan_discloses_progressively_by_tier() {
        let reg = Registry::bundled();
        let mut w = World::new();
        let subj = w.spawn_empty().id();
        let tell = scan_tell(subj, Provenance::Director, &reg);
        let names = |_e: Entity| "Aldric".to_string();
        let ctx = RealizeCtx {
            registry: &reg,
            name: &names,
        };

        let glance = ScanRowRealizer {
            tier: ReadTier::Glance,
        }
        .realize(&tell, &ctx);
        assert_eq!(glance.name, "Aldric");
        assert!(!glance.demeanour.is_empty(), "Glance shows demeanour");
        assert!(glance.pursuit.is_none(), "Glance hides what they pursue");
        assert!(glance.authored.is_none(), "Glance never reveals Γ's hand");

        let read = ScanRowRealizer {
            tier: ReadTier::Read,
        }
        .realize(&tell, &ctx);
        assert!(read.pursuit.is_some(), "Read surfaces the situation");
        assert!(read.authored.is_none(), "Read still hides authorship");

        let deep = ScanRowRealizer {
            tier: ReadTier::Deep,
        }
        .realize(&tell, &ctx);
        let revealed = deep.authored.expect("Deep reveals a Γ-authored charge");
        assert!(
            revealed.contains("placed"),
            "the recognition beat: {revealed}"
        );
    }

    #[test]
    fn a_deep_scan_invents_no_authorship_for_a_grown_charge() {
        let reg = Registry::bundled();
        let mut w = World::new();
        let subj = w.spawn_empty().id();
        // A wholly grown (Sim) charge — there is no demiurge's hand to find, even at Deep.
        let tell = scan_tell(subj, Provenance::Sim, &reg);
        let names = |_e: Entity| "Mara".to_string();
        let ctx = RealizeCtx {
            registry: &reg,
            name: &names,
        };
        let deep = ScanRowRealizer {
            tier: ReadTier::Deep,
        }
        .realize(&tell, &ctx);
        assert!(
            deep.authored.is_none(),
            "a grown charge confesses nothing under a Deep read"
        );
    }

    #[test]
    fn a_stalled_story_yields_no_tell() {
        let mut w = World::new();
        let (a, b) = (w.spawn_empty().id(), w.spawn_empty().id());
        let eps = [ep(0, EpisodeKind::GrievanceFormed, Provenance::Sim)];
        let c = cand(&[a, b], SiftStatus::Abandoned, 1.0, &[0]);
        assert!(
            tell_from_candidate(
                &c,
                &id_map(&eps),
                None,
                &HashSet::new(),
                64,
                &PerceptionCfg::default()
            )
            .is_none(),
            "an abandoned (stalled) partial is not legible"
        );
    }
}
