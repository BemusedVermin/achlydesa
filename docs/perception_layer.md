# The Perception Layer — making the simulation legible (and learning to read Gamma's hand)

> **Status: design / pre-implementation (2026-06).** A working spec, not a contract. It defines a
> single legibility pipeline — Chronicle → Sifter → **Tell** → **Realizer** → surface — that turns
> the drama the sim already produces into something the player can *read*, and that makes the
> player's growing ability to perceive the director's authorship the core recognition mechanic.
> Read alongside `narrative_sifter.md` (the built `Chronicle` ring + `Sift` matcher this sits on
> top of), `narrative_surfacing.md` (the inviolable "fiction, not apparatus" rule and the diegetic
> channels), `combat-core-spec.md` (the tick timeline one surface renders), and `dialogue.md` (the
> meaning/surface split this generalises). Type and accessor names are the real ones in
> `agent_core` / `agents` / `combat_core` at time of writing.
>
> Note on names: the narrative director is written `Γ` in the other docs; here it is spelled
> "Gamma" (this doc is ASCII-only).

---

## 0. One-paragraph shape

The simulation already stages drama — feuds escalate, opinions sour, the beloved falls, wars are
declared, Gamma manufactures arcs onto specific named souls — and the `Sift` layer already
*perceives* the forming stories bottom-up (`narrative_sifter.md`). What is missing is the last
hop: **none of it reaches the player as something legible.** The Perception Layer is that hop. It
introduces one atom — a **`Tell`**, a structured, prose-free, salience-ranked unit of legible
information derived from the Chronicle and the Sifter — and one rendering contract — a **`Realizer`**
that turns a Tell into a medium (a line of prose, a charged place, a scan readout, a timeline
marker). Every player-facing legibility surface is then the *same* ranked set of Tells under a
different filter and a different Realizer. Crucially, every Tell carries **provenance** — was the
state it reports *grown* by the sim or *written* by Gamma — and the deepest reads of the world
surface that distinction. That is not a side feature: the meta-plot is the player learning to see
the demiurge's seams, so **legibility is the theme**, and this layer is what instantiates it. The
whole thing obeys the workspace invariants — deterministic, off-by-default byte-identical, its own
resources, read-only over the sim, the SLM never load-bearing.

---

## 1. The problem, and why this is the highest-leverage layer

**The complaint** (`narrative_surfacing.md` S1): *"there is next to no narration of plot... the
player must read a large block of text."* The substance of the plot already happens; the interface
does not exist. Three felt problems — an empty-feeling overworld, button-mash combat, "content feels
thin" — are one problem in three costumes: **the systems are illegible.** The player cannot read the
world back as story (Dwarf Fortress's retrospective legibility) and cannot read it forward to form a
plan (Dishonored/Prey's prospective legibility).

**Why fix legibility before authoring more content.** Two reasons compound:

1. It makes *all existing content* legible at once — the richest leverage available, because the
   drama is already there and unread.
2. In *this* game the act of perceiving is the theme. The Gnostic frame makes the endgame "the
   player learns to recognise that some souls' fates were authored, not grown." A legibility layer
   that carries **provenance** is therefore the only system that simultaneously (a) makes everything
   legible and (b) instantiates the core recognition mechanic. No other system does both.

---

## 2. The pipeline

```
Chronicle           Sift                Perception            Surface
(bounded ring   →   (matches arcs   →   (ranks Tells by   →   (filter + Realizer +
 of Episodes,       over episodes,      salience, tags        budget; one per
 chronicle.rs)      sift.rs)            provenance)           player channel)
```

The contract that collapses the surfaces into one:

> **Every surface is the same salience-ranked set of `Tell`s, differing only in (a) a
> spatial/temporal/kind filter, (b) a render budget, and (c) the target `Realizer`.**

This is true at the **render** layer. It is *not* true at the **production** layer: narrative Tells
come from a Sifter pattern-match over a multi-tick window; combat Tells come from the encounter
scheduler per tick (S5.4). One render contract, two production cadences — see S8.

---

## 3. Core types

All Perception state lives in **its own resources**, computed from a read-only snapshot of the
`Chronicle` + `Sift` each pass — exactly as `ThreadCandidate`s already are (`sift.rs`). **Tells are
not ECS components and are never spawned as entities**: spawning into the sim `World` would churn
archetypes and risk perturbing determinism and iteration order, the precise thing `Chronicle` /
`Sift` are structured to avoid. A `Perception` resource holds a `Vec<Tell>`; per-surface selection
is a `filter().take(budget)` over it.

### 3.1 The atom: `Tell`

A **tell** is a legible cue you learn to read; it is also the root of *telling a story*; and reading
Gamma's tells is the endgame. One `Tell` is one renderable unit.

```rust
/// One renderable unit of legible information. A plain struct held in the `Perception` resource's
/// Vec — NOT a Component. Prose-free: the Realizer renders it; baking text here would forfeit the
/// reuse that is the whole point (one Tell -> a line, a glyph, a row, or a marker).
pub struct Tell {
    pub subject: Entity,            // who/what this is about (a soul, a POI, a faction)
    pub kind: TellKind,             // tension | opportunity | threat | mystery | aftermath | ...
    pub salience: f32,              // the ranked-filter input (S4)
    pub provenance: Provenance,     // grown by the sim, or written by Gamma — the theme hook (S6)
    pub anchor: Anchor,             // spatial + temporal placement; surfaces filter on it
    pub source: SmallVec<[u64; 4]>, // the Chronicle Episode ids this derives from (traceable)
    pub hints: RealizeHints,        // structured payload the Realizer consumes
}

pub enum TellKind { Tension, Opportunity, Threat, Mystery, Aftermath, Recurrence }

/// Spatial + temporal placement, so a surface filters with a predicate (no marker entity needed).
pub struct Anchor { pub place: Option<Coord>, pub when: When }

pub enum When {
    Past(u64),       // already happened — the prose log, map history
    Now,             // current standing state — the scan
    Scheduled(u32),  // a committed future intent at a combat tick — the timeline telegraph
}

/// Structured, never prebaked. One Tell renders as prose, a charged place, a scan row, or a marker.
pub struct RealizeHints {
    pub actors: SmallVec<[Entity; 4]>,
    pub register: Register,         // maps into the tagged grammar AND glyph/icon tables
    pub magnitude: f32,             // emphasis (glow, weight) — DEV surfaces only; see S7
    pub tier_gate: ReadTier,        // min read-tier to reveal full content (S5.3)
}
```

`Tell::source` keeps every Tell **traceable** back to the Chronicle episodes it came from — the same
discipline that lets the retelling eval read a candidate's `support` (`sift.rs`).

### 3.2 The renderer: `Realizer`

```rust
pub trait Realizer {
    type Out;
    fn realize(&self, tell: &Tell, ctx: &RealizeCtx) -> Self::Out;
}
```

Each Realizer is small, swappable, independently testable:

| Realizer           | `Out`        | Surface          | Reuse note                                                    |
|--------------------|--------------|------------------|--------------------------------------------------------------|
| `GrammarRealizer`  | `String`     | prose log        | **the existing tagged grammar, wired to a new input**        |
| `PlaceRealizer`    | `PoiMood`    | drama-map        | a POI's *fiction* (who is there, what happened) — not a gauge |
| `ScanRowRealizer`  | `ScanLine`   | read-the-room    | a soul's goal / charged state / provenance as a diegetic line |
| `TimelineRealizer` | `MarkerSpec` | combat timeline  | a `combat_core` scheduled instance -> marker at its tick      |
| `SlmRealizer`      | `String`     | prose log (opt.) | the `voice` `TextGen` seam, off by default, never load-bearing |

The `GrammarRealizer` is the cheap win: prose surfacing is mostly *wiring an existing system to a new
input*. The `SlmRealizer` stays exactly where `dialogue.md` scoped it — an optional re-voicer behind
the same trait, cached by meaning hash, never feeding sim state.

### 3.3 The consumer: a surface

A surface is a filter + a Realizer + a budget. This is the only thing each player channel
specialises.

```rust
pub trait Surface {
    type R: Realizer;
    fn select<'a>(&self, p: &'a Perception) -> impl Iterator<Item = &'a Tell>; // spatial/temporal/kind predicate
    fn realizer(&self) -> &Self::R;
    fn budget(&self) -> usize;   // max Tells shown — forces salience to do real work
}
```

`budget()` is not incidental. A surface that shows everything is as illegible as one that shows
nothing ("the player must read a large block of text"). The budget is what makes salience matter and
what creates restraint.

---

## 4. Salience — why a Tell rises

Salience is the ranked-filter input and the lever that makes the world feel *curated* without
authoring. It **reuses the signals the Sifter already computes** (`sift.rs` `Axis`), plus three new
terms:

```
salience(tell) =
      w_surprise    * rarity                 // sift.rs Axis::Rarity — base-rate surprise
    + w_dissonance  * dissonance_axes         // the SUM of GrievancePressure + OpinionReversal
                                              //   + NormTension + MoodReversal + Bloodshed (sift.rs)
    + w_proximity   * player_relevance        // spatial + social distance to the avatar (new)
    + w_recurrence  * recurrence_bonus         // subject/motif recurs across POIs/threads (S7, new)
    + w_authorship  * authorship_anomaly       // provenance doesn't follow from history (S6, new)
```

Notes:

- **"Dissonance" is not a per-agent scalar.** This codebase has no Lie/Want/Need engine; the drama
  signal is the Sifter's **axis sum**, already grounded on real social state — a converging grudge,
  a soured opinion edge, a coiled taboo, a mood low, bloodshed. A high-interest `ThreadCandidate` is
  *loud*; its constituent Tells inherit that. Reuse it; do not invent a new model.
- **`player_relevance`** uses `ThreadCandidate::place` (a `Coord`) and social distance. The
  avatar-gated `draw` bias in `director.rs` is the precedent for an avatar-weighted term that is
  inert in any headless/player-less run (keeping V&V byte-identical).
- **`authorship_anomaly`** is the thematic term (S6). Most of the time ~0; it spikes when Gamma
  writes state that does not follow from a soul's history.
- Weights are **per-surface tunable** (a `SalienceProfile` per `Surface`), so the combat timeline can
  weight threat + proximity while the map weights surprise + recurrence — one function, many
  profiles. Tune them with the **expressive-range / retelling eval** (`narrative_sifter.md` S7), not
  by guesswork.

---

## 5. The four surfaces

Each surface = a filter + a Realizer + a budget + a couple of surface rules. **All obey the one
inviolable rule** (`narrative_surfacing.md` S2): *surface the fiction, never the apparatus.* The
player never sees a thread, a register name, a phase, a prominence number, a kind label, or a
magnitude. The same Tell may feed a **dev overlay** (which *does* show kind/magnitude/score, gated
behind a dev flag) and a **player surface** (which shows only the fiction) — the two-audience split
`narrative_surfacing.md` already draws.

### 5.1 Prose log — build first

- **Filter:** `When::Past`, involving the avatar or its known souls/places, top-`budget` by salience.
- **Realizer:** `GrammarRealizer` (existing). `SlmRealizer` optional.
- **Channel:** route through the gossip / ledger plumbing (`narrative_surfacing.md` S4), so it arrives
  as rumours and recollections, not a wall-of-text panel.
- **Surface rule — restraint / lacunae:** render the salient *beat*, not the full causal chain. Give
  "just enough" and let the player's pattern-matching close the gap (S7). Under-narration is a
  feature and is cheaper.
- **Why first:** it generalises the one thing already proven to land (text/lore) to the *whole game*,
  reuses the dialogue stack, and validates the Chronicle → Tell → Realizer path end to end with zero
  new tech.

### 5.2 Drama-map

- **Filter:** Tells with a `place`, one top-salience Tell per POI (`budget = 1` per POI).
- **Realizer:** `PlaceRealizer` → the POI's **fiction**: who is there, what happened, rendered as
  demeanour / a one-line on focus. **Not** a kind-tinted, magnitude-sized glyph — that is the
  apparatus, and it belongs on the dev overlay only.
- **Surface rule:** a settlement reads *charged or quiet through its story*, at a glance. The player
  routes toward where things are happening instead of toward empty tiles — navigation becomes
  reading. The charge is conveyed diegetically (a gathering, smoke, a silence), never as a gauge.

### 5.3 Read-the-room scan — the new verb

The missing immersive-sim verb is **assess, then exploit** — today only exploit ships.

- **Trigger:** an active player verb that costs time / tempo / skill (see tiers). Because it is an
  *active verb that feeds back into sim*, it obeys the determinism discipline (S8): any randomness
  from a dedicated derived stream, off-by-default byte-identical.
- **Filter:** `When::Now`, subjects = souls in the current cell, ordered by salience.
- **Realizer:** `ScanRowRealizer` → a diegetic line grounded on **state that exists**.
- **Surface rule — progressive disclosure (`ReadTier`):**

  | Tier     | Cost            | Reveals (grounded on real state)                                          |
  |----------|-----------------|---------------------------------------------------------------------------|
  | `Glance` | free / passive  | *that* this soul is charged — a dominant low `Mood`, an active `Grievance`, a soured `Opinion` edge — as demeanour, no number |
  | `Read`   | a verb / time   | what it is **pursuing** (its IAUS goal / GOAP intent) + its salient social fact (whom it grudges, who soured) |
  | `Deep`   | skill / tempo   | **provenance** — whether this charge was *grown* or *authored* (S6); the recognition beat |

  Tiers preserve mystery (no info-dump), reward investment (skill expression), and gate the theme:
  catching Gamma requires a *Deep* read. The scan is how you learn to see the demiurge.

### 5.4 Combat timeline + tempo verb

`combat_core` already is a **shared global tick timeline**: actors place `ActionInstance`s that move
to `InstanceStatus::Scheduled`, an `observer.foresight_horizon` bounds visible lookahead, a **Tempo**
economy lets an actor dilate to act outside readiness and apply edit verbs (interrupt / displace /
counter), and `hide_enemy_tempo` is foresight fog (`combat-core-spec.md`). The Perception Layer's job
here is **narrow and real: render that engine.** It does not re-derive combat from the Sifter.

- **Filter:** `When::Scheduled(tick)` within the current encounter, all actors, restricted to
  `[current_tick, current_tick + foresight_horizon]`.
- **Realizer:** `TimelineRealizer` → a marker (who / verb icon / resolves-at-tick), the lethal
  incoming hit emphasised.
- **Surface rule — telegraph everything (Into the Breach model):** every committed action is a
  visible marker *before* it resolves. Combat becomes "read the track, find the smartest response,"
  and mashing stops being optimal input — the spam problem dissolves structurally.
- **The tempo verb is already the engine's `Tempo` + `foresight_horizon`:** spending Tempo deepens
  lookahead and buys the planning-pause window to insert / re-order an intent into a gap. Depth comes
  from verbs **interacting on the timeline** (interrupt = delete a Scheduled marker, displace = shift
  its tick, counter = resolve before it), i.e. tick-position is the tactical space — already
  `combat_core`'s edit verbs. The Perception Layer renders it; it does not reinvent it.

---

## 6. Provenance — the surface where theme == architecture

The Chronicle is append-only at the emit sites; tagging authorship there costs ~nothing.

```rust
pub enum Provenance {
    Sim,              // emergent from ordinary system rules
    Agent(Entity),    // a normal soul's own choice
    Director,         // Gamma's privileged write — already flagged by EpisodeKind::BeatFired
}
```

- **Add one field to `Episode`** (`chronicle.rs`) and one argument at each `record(...)` tap. The
  data is half-present already: `EpisodeKind::BeatFired` is recorded at the same site Gamma pushes a
  `BeatEvent`, so director-authored episodes are already distinguishable.
- Gamma writes `Effect`s (Grudge / Sway / Turn / Decree / War / …), not a "Want" or a "Lie." So the
  anomaly is defined over **effects landing on souls they don't follow from**: a grudge *staged* onto
  two souls with no prior friction reads as more anomalous than one that merely amplified an existing
  convergence — exactly the `is_forming` / convergence distinction the Sifter already draws
  (`sift.rs`). That difference is the `authorship_anomaly` salience term (S4).
- A **`Deep` scan — and only a Deep scan — surfaces it**, as a diegetic line: *her grief did not grow
  here; it was placed.* That readout **is** the Gnostic recognition mechanic. The horror is the
  player learning to see the seams — and because it reports a real, traceable fact (an emergent
  episode vs. a `BeatFired` one), it needs no invented cognitive model to carry it.

---

## 7. Apophenia — cheap drama via the player's own pattern-matching

You do not author meaning; you surface legible breadcrumbs and let the player make it.

- A `Recurrence` Sifter pattern over the Chronicle boosts Tells whose subject / motif / name recurs
  across POIs or threads (the `recurrence_bonus` salience term, S4). It is a pattern in the existing
  `sift.ron` idiom, not a new subsystem.
- The prose log (S5.1) and the drama-map (S5.2) then *naturally* foreground "the same name keeps
  appearing," "this motif repeats." The player's brain does the narrative authoring for free.
- Pair restraint (S5.1) with recurrence: partial connections + lacunae are what trigger apophenia.
  Surface the thread, not the conclusion.

---

## 8. Determinism, off-by-default, and the two audiences

Every piece obeys the workspace invariants (as `narrative_sifter.md` S6 spells out for the Sifter):

- **Read-only over the sim.** The Perception pass is pure arithmetic over the deterministic
  `Chronicle` + a read-only `Sift`/world snapshot; it writes only its own `Perception` resource. A
  perception-off world is **byte-identical**.
- **Off by default = byte-identical.** `Perception` is its own resource, absent unless the layer is
  woken; selection early-returns when absent. The established absent-resource pattern makes this
  trivial — another reason Tells are a resource Vec, not spawned entities.
- **The active verbs are the exception that proves the rule.** The scan (S5.3) and the combat tempo
  verb (S5.4) *do* change sim/encounter state. They live inside the determinism discipline: any
  randomness from a **dedicated derived stream** (run-seed XOR a distinct constant), never pulled
  from an existing stream; deterministic given the seed.
- **The SLM never feeds back** (`dialogue.md`): `SlmRealizer` re-voices a Tell on a background
  thread, cached by meaning hash; a build with it is byte-identical to one without.
- **Two audiences, one Tell.** The author/debugger sees the apparatus — kind, magnitude, salience,
  provenance, the source episodes — on a dev overlay gated behind a flag. The player sees only the
  fiction. `RealizeHints::magnitude` and `TellKind` are **dev-surface inputs**; no player Realizer
  renders them as a number or a label.

---

## 9. Build plan (phased; each phase ships value alone; all share the Phase-0 spine)

| Phase | Deliverable | Reuses | Acceptance |
|------|-------------|--------|-----------|
| **0 — spine** | `Tell` + `Provenance` + `Anchor` + the `Realizer` / `Surface` traits + the salience fn; add `Provenance` to `Episode` and tag it at each `record()` tap | `Chronicle`, `Sift` | Unit: a hand-authored episode → Sifter → a `Tell` with correct subject/kind/**provenance**; salience monotonic in the Sifter axis sum; perception-off world byte-identical. Nothing visible yet. |
| **1 — prose log** | the log over the Chronicle, via the gossip/ledger channel | Phase 0 + **tagged grammar** | The player meets a readable, salience-ordered, budgeted account of their own recent history as rumour/recollection; no model required; no wall of text. |
| **2 — drama-map** | POIs read charged-through-fiction | Phase 0 + Spatial/POI | An empty POI reads quiet, a tense one reads tense, at a glance — diegetically; the kind/magnitude glyph exists **only** on the dev overlay. |
| **3 — read-the-room scan + `ReadTier`** | the scan verb + provenance reveal | Phase 0 + the real per-soul state (goal/mood/grievance/opinion) | Active scan lists in-cell souls by salience; tiers gate detail; a **Deep** scan on a Gamma-authored soul surfaces the authorship anomaly as a diegetic line. Determinism held. |
| **4 — combat timeline** | a `TimelineRealizer` over `combat_core` | Phase 0 + **`combat_core`** | All committed intents render as markers on the tick track before resolution; tempo deepens lookahead + buys the pause; a fresh player survives an encounter by *reading*, not mashing. |
| **5 — SLM + recurrence** | `SlmRealizer` (optional) + apophenia | Phase 0 + Phase 1 | SLM swappable behind `Realizer` with grammar fallback (byte-identical when off); a recurrence-boosted motif visibly threads a recurring name across ≥2 POIs. |

Everything hangs off Phase 0: four player-facing surfaces, one shared contract, no surface that
can't be ripped out or reskinned without touching the others. Test the way the rest of the sim is
tested — seeded, deterministic, off-by-default, a saved run as the golden transcript — and keep the
director / integer-economy V&V green throughout.

---

## 10. Open questions

1. **Tell lifetime.** Recompute each Perception pass (stateless, simplest, matches the retrospective
   Sifter) vs. persist + invalidate. Recommend **stateless**; optimise only if a frame budget shows
   the pass.
2. **Cadence.** Narrative Tells can lag (a fixed interval, like the Sifter); combat Tells need
   near-real-time within an encounter. Two cadences feeding one `Perception` resource — likely a
   per-tick combat path and a slower narrative path sharing the same `Tell` type.
3. **Read-tier resource.** Does the scan's tier draw from the **same pool** as the combat tempo verb?
   Unifying them ("perception" as one resource spent on reading the world *or* reading a fight) is
   thematically tidy but couples two subsystems — decide deliberately.
4. **Provenance leak rate.** How obvious should Gamma's authorship be at `Deep`? Too legible kills the
   horror; too hidden and players never catch it. A tuning knob, not a binary — expose it as one, and
   calibrate it against the retelling eval, not by guess.
5. **Map charge without a gauge.** What diegetic vocabulary conveys "this POI is tense" without a
   kind/magnitude glyph? (Gatherings, silences, smoke, who is present.) The hardest no-apparatus
   design problem in the layer.
