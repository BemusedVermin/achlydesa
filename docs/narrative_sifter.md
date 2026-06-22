# The Story Sifter - Gamma's perception organ (and a retelling eval harness)

> **Status: design / pre-implementation (2026-06).** A working spec, not a contract. It grounds
> the one genuinely-new idea from `docs/emergent-narrative-brief.md` (bottom-up story sifting)
> onto this codebase's real architecture, and pairs it with the eval harness the design needs to
> tune itself. Read alongside `emergent-narrative-grounding.md` (why this and not the rest of the
> brief), `narrative_director_v2.md` (the top-down drama manager this feeds), and `dialogue.md`
> (the meaning/surface split and the `MemRecord` discipline). Type and accessor names are the real
> ones in `agent_core` / `agents` at time of writing.
>
> Note on names: the narrative director is written `Γ` elsewhere; here it is spelled "Gamma"
> (this doc is ASCII-only).

---

## 0. The one-paragraph shape

The director (Gamma) is a **top-down drama manager**: it picks a register, pins a cast, and fires
beats to *manufacture* an arc. What it lacks is **eyes** - it reads casting salience from a
*snapshot* of current state, so it can see that a rival is ambitious *right now*, but not that
three souls have been *escalating a feud across the last forty ticks*. A **story sifter** is that
missing perception: a deterministic, in-tick reader of a small structured event history that
pattern-matches **forming** stories bottom-up, ranks them by interest, and hands the ranked list to
Gamma. The payoff is the **Awash hybrid** (sifter + drama manager): the sifter perceives where the
world already leans, the director amplifies it, and because the director is now nudging genuinely
primed situations, every beat gains a stronger in-world alibi - **the sifter strengthens the
deniability thesis the whole game rests on** (`narrative_director_v2.md` S2/S5). The same ranked
candidates double as the substrate for a **retelling-based eval harness** (read the stories the run
produced; are they retellable?), which is how you tune the drama weights instead of guessing.
Everything obeys the workspace invariants: deterministic, off-by-default byte-identical, its own
resources, no SLM in the path.

---

## 1. Why a sifter, and why now

**Curationism vs. drama management.** Emergent-narrative research has two poles: *curationism*
(Ryan - find the story the sim already told) and *drama management* (Mateas/Stern - make the story
happen). achlydesa has bet hard on drama management, with the Demiurge as the thematic alibi. The
brief leans curationist. The synthesis the brief itself cites is **Awash** ("sifter + drama manager
hybrid"), and it is exactly the missing half here.

**The concrete gap.** In `director.rs`, the objective is
`score = weight x drama x salience x novelty x phase_bias x spine_bias x trunk_bias x collide_bias`,
where `salience` (the inverse of resistance) is *cast fit read from the present state*. That is a
**snapshot**, not a **trajectory**. The director cannot perceive a multi-tick pattern, because no
multi-tick history exists to read (`EventQueue` is drained every tick; the `Cadence` log records
only Gamma's own beats; `Gossip` is lossy and decaying; `MemRecord` is per-soul and thinly
populated). So the director's "least resistance" is myopic: it can nudge what is primed *now*, but
it cannot recognize a story the world has been telling itself and step in to crown it.

**Why this is the highest-leverage addition.** The deniability rule says: *never select a beat the
world could not plausibly have produced itself*. You can only honor that rule as well as you can
*perceive* what the world is producing. A sifter is the perception that makes least-resistance
selection truly least-resistance - which is the same as saying it makes the manipulation more
deniable per-beat and therefore more *felt* in aggregate (the apex aesthetic; see
`emergent-narrative-grounding.md` S4). It also catches emergent stories the top-down director would
otherwise steamroll, and it is the only thing that makes the eval harness (S7) possible.

---

## 2. The substrate: a bounded, structured Chronicle ring

The brief assumes a lossless, event-sourced global Chronicle. **Do not build that.** But be honest:
the sifter genuinely needs *some* persistent, queryable, multi-tick history of **what the world did
on its own**, and none of the existing fragments suffices for general pattern-matching. The director
already keeps two append-only logs - `Director::cadence: Vec<Cadence>` (uncapped) and a capped beat
ring `Director::events: Vec<BeatEvent>` (`recent_events()`, `EVENT_CAP = 16`, which already seeds
gossip) - but **both record only Gamma's own beats**, which is exactly the wrong half: the sifter
needs to perceive the *emergent* signals (a grudge formed, an alliance soured, a throne taken) that
the director did *not* author. The other fragments don't fit either (`Gossip` is decaying and lossy;
`MemRecord` is per-soul; `EventQueue` carries only 3 transient variants, drained each tick). So the
one new piece of infrastructure is a **modest bounded episode ring** that **generalizes the existing
`BeatEvent` ring** to world events - sized for pattern-matching, not for completeness.

```rust
/// A structured, prose-free handle on something that happened. `Copy`, bounded party slots,
/// so the ring stays cheap. The surface layer renders it; the sifter only reads handles. This is
/// the existing `BeatEvent` shape (id/tick/register/place/lead+other) generalized past beats.
#[derive(Clone, Copy, Debug)]
pub struct Episode {
    pub id: u64,
    pub tick: u64,
    pub kind: EpisodeKind,
    pub parties: [Option<Entity>; 3],   // actor, target, third - the cast a pattern binds
    pub place: Coord,
    pub register: Option<Register>,     // set for BeatFired
    pub detail: i32,                    // kind-specific: OpinionCrossed direction (-1 cold/+1 warm), etc.
}

pub enum EpisodeKind {
    // --- the violent core (combat & death are a planned subsystem - docs/rpg_survival_exploration.md) ---
    // The highest-interest episodes the sifter ranks: a killing is the apex narratable event. The
    // combat/death system is their emit site; `Killed` puts the slayer in parties[0], victim in [1].
    Killed, Death, Wounded,
    // --- social transitions ---
    // from the Grievance insert (the avenge machinery in people.rs + Effect::Grudge in director_step)
    GrievanceFormed,
    // from the Opinion mutation sites (Effect::Turn + the faction opinion updates); recorded only
    // when an edge crosses the ally_threshold/foe_threshold the director already uses (detail = dir)
    OpinionCrossed,
    // --- hard events / politics ---
    // from EventQueue / AgentEvent (events.rs)
    Crowned, Deposed, Transgressed,
    // from factions.rs transitions
    WarDeclared, ThroneTaken, ThroneLost, FactionJoined, FactionLeft,
    // the director's own hand - ALREADY emitted as BeatEvent; mirror it here for context (sets register)
    BeatFired,
}

/// A bounded ring of recent episodes. A resource (no component), so a chronicle-free world is
/// byte-identical. Cap is a few thousand (vs. BeatEvent's 16); salience-weighted retention is an
/// open question for long slow burns (S10).
#[derive(Resource)]
pub struct Chronicle { ring: VecDeque<Episode>, cap: usize, next_id: u64 }

impl Chronicle {
    /// Called from the emit sites below. A no-op (early return) when the layer is disabled, so
    /// nothing is ever recorded in a world without the sift/director layers.
    pub fn record(&mut self, kind: EpisodeKind, parties: [Option<Entity>; 3], place: Coord, detail: i32);
    pub fn recent(&self) -> impl Iterator<Item = &Episode>;
}
```

**Where records are written:** the sites that already emit `AgentEvent` into `EventQueue`
(`events.rs`); the `Grievance` insert and the `Opinion` mutation sites (`Effect::Turn` in
`director_step`, plus the avenge/faction updates in `people.rs`/`factions.rs`); the throne/war/
faction transitions (`factions.rs`); and `director_step`'s beat firing (where `BeatEvent` is already
pushed - emit the mirror `Episode` in the same place). Each gains one guarded `chronicle.record(...)`
line. **Combat and death are first-class here, not an afterthought:** a killing is the apex
narratable event, so the forthcoming combat/death subsystem
(`docs/rpg_survival_exploration.md`) is the natural emit site for `Killed` / `Wounded` / `Death`.
A death recorded at the despawn site is the **interim** until that lands - and it retires the
inference the director does today (its wake-watching notices `!alive.contains(&e)` rather than being
told), folding death-attribution into one explicit signal.

**This is the honest minimum** the brief's Chronicle reduces to: bounded, structured, off-by-
default, written from existing signals, read only by the sifter (S3) and the eval harness (S7).

---

## 3. The pattern layer - RON-authored, incremental, binds a cast

Patterns are **data**, in the `goals.ron` / `beats.ron` / `intents.ron` idiom, so designers add
stories without touching engine code. A pattern is an ordered window of episode-kind predicates
that **bind a cast** (later predicates reuse earlier bindings), plus the interest axes it scores on.

```ron
(
  id: "feud_escalating",
  // machine-readable label; surfaced ONLY to the dev overlay / eval, never to the player (S2 of
  // narrative_surfacing.md - the no-apparatus rule).
  tension: "feud_escalating",
  // the spine this candidate would let the director amplify at low resistance.
  register: Vengeance,
  // an ordered window over recent episodes; `bind` introduces a variable, `where` reuses one.
  window: [
    (kind: GrievanceFormed, bind: [(A, actor), (B, target)]),
    (kind: OpinionCrossed,  where: [(A, actor), (B, target)], dir: Cold),
    (kind: Harm,            where: [(A, actor), (B, target)]),
  ],
  // how a match's interest is computed - surprise + dissonance, on real state (S4).
  interest: [
    (axis: Rarity,          curve: Power(exp: 1.0)),    // base-rate surprise (statistical)
    (axis: GrievanceWeight, curve: Linear(m: 1.0, b: 0.0)),
    (axis: OpinionReversal, curve: Linear(m: 0.8, b: 0.0)),
  ],
  emerging_at: 1,   // one predicate matched -> status Emerging
  active_at:   2,   // two or more -> Active (eligible for the director graft)
  window_ticks: 60, // unmatched past this -> Abandoned
)
```

The matcher is **incremental**: each new `Episode` is offered to every pattern's open partial
matches and to fresh starts, advancing bindings or spawning candidates. A match emits/updates a
`ThreadCandidate`:

```rust
pub struct ThreadCandidate {
    pub pattern: SiftPatternId,
    pub status: SiftStatus,                 // Emerging | Active | Resolved | Abandoned
    pub cast: SmallVec<[Entity; 4]>,        // the bound variables
    pub tension: TensionId,                 // interned label (dev/eval only)
    pub register: Register,                 // the spine it would feed Gamma
    pub place: Coord,                       // where it is centered (casting + markers)
    pub support: SmallVec<[u64; 8]>,        // the Episode ids that constitute it
    pub interest: f32,
    pub first_seen: u64,
    pub last_updated: u64,
}

#[derive(Resource, Default)]
pub struct Sift {
    candidates: Vec<ThreadCandidate>,
    seen: HashMap<SiftPatternId, u64>,      // base-rate counters for the Rarity axis
}
impl Sift {
    /// Ranked, highest interest first; the director and eval read this.
    pub fn ranked(&self, min_interest: f32) -> impl Iterator<Item = &ThreadCandidate>;
}
```

Build the **retrospective** path first (run patterns over the whole ring - trivial to test against
a saved run), then the **incremental** path, and assert they agree for the same episode sequence
(the brief's S4.3 acceptance criterion, kept).

---

## 4. Interest scoring - grounded on real state

The brief's signal is `W_SURPRISE * surprise + W_DISSONANCE * dissonance`. We keep the shape and
re-ground both terms, since there is no Lie/Want/Need here:

- **Surprise (statistical).** `Rarity` = base-rate rarity of this pattern instance, from
  `Sift::seen` counters (rarer patterns rank higher). This is the same intuition as the director's
  existing novelty/recency penalty, now applied to *observed* patterns rather than *fired* beats.
- **Dissonance (project-specific, the richer signal).** With no Lie/Want/Need, dissonance
  re-grounds onto the real social state - and these are *better* signals than raw statistics,
  exactly as the brief argues. Note that `Grievance(Entity)` is a **target-only marker with no
  magnitude field**, so "grievance weight" must be *derived*, not read off the component:
  - `GrievancePressure` - derived: recency since the `GrievanceFormed` episode (its `Episode.tick`)
    and **convergence** - how many souls hold a grudge against one target (the director already
    counts exactly this as `grudges_at_proto` in `director_step`).
  - `OpinionReversal` - how far an `Opinion` edge has swung, via `Opinion::of(other)` deltas across
    the supporting episodes (an ally crossing from above `ally_threshold` to below `foe_threshold`
    is high drama).
  - `NormTension` - a cast member whose trait pushes toward an act the prevailing norms forbid
    (literally the `Input::Sanction` axis: a vengeful soul under a no-kill taboo is a coiled
    spring).
  - `MoodReversal` - a swing from a high to a low, measured with the *same* up/down the director
    reads for its `reversal` term: `MoodIds::high` (joy/hope/love/awe/rapture/elation + 0.5 calm)
    vs. `MoodIds::low` (anger/sorrow/fear/despair/dread/foreboding) in `director.rs`.
  - `Bloodshed` - a `Killed` / `Wounded` episode among the cast. With combat and death coming, a
    violent end is the apex stakes the sifter ranks: a feud that reaches a killing, an ally cut down,
    a duel. A forming thread trending toward bloodshed should outrank a quieter one, and a `Killed`
    that consummates a standing `GrievanceFormed` (vengeance paid) is the most retellable beat the
    world can produce.

Scoring reuses `ai::Curve` so axes are authored exactly like goal/intent considerations. **Tune the
weights with the eval harness (S7), not by guesswork** (the brief's explicit instruction, kept).

---

## 5. The graft into the director (two phases, so it ships read-only first)

**Phase A - read-only (no director coupling).** The sift system runs in the fixed schedule *before*
`director_step`, producing ranked candidates in `Sift`. Nothing in the director changes. The eval
harness (S7) and, optionally, the player-facing channels (S9 of `narrative_surfacing.md`) read the
candidates. This proves the legibility thesis with **zero risk to the director's behavior** and is
fully off to the side - the cheapest way to learn whether the patterns are any good.

**Phase B - the perception organ (the actual payoff).** `director_step` consults `Sift` in two
places, both gated by the sift-enabled flag (so a director-only world, sift off, is byte-identical
to today). The exact precedent to copy is the existing **`AVATAR_DRAW`** bias (`director.rs`): the
director already biases selection with an additive term (`draw(c)`) that is **avatar-gated** - it
is `0` in any headless/player-less run, which is precisely how the avatar-draw feature keeps the
director's V&V baseline byte-identical. The sift graft follows the identical discipline: its bias
is neutral whenever the sift layer is off, so director-only worlds are unchanged.

1. **Thread seeding.** The thread-spawn loop in `director_step` picks `spine` via `pick_spine`
   (recency-penalised register rotation + `trunk_bonus` + jitter), `lead` by prominence + `draw`,
   and `other` via `pick_other` (by spine). The graft: when a high-interest **Active** candidate
   exists, adopt *its* `register` as the spine and *its* cast as `lead`/`other` (and `place` for the
   marker), in preference to the arbitrary rotation. The world demonstrably already leans here, so
   resistance is genuinely low and the alibi is genuinely strong.
2. **Beat resistance.** In the score product
   `weight * drama * salience * novelty * phase_bias * spine_bias * trunk_bias * collide_bias`,
   add one more multiplier, `sift_bias`, for beats whose cast overlaps a live candidate's cast -
   riding *alongside* the biases already there. Note `salience` (the cast-fit term from `cast_beat`,
   the inverse of resistance) stays as is: it is the *snapshot* signal; `sift_bias` is the
   *trajectory* signal layered on top. This is the snapshot-to-trajectory upgrade - the director now
   rewards nudging stories that are *forming over time*, not just statically primed.

**Crucial restraint.** The sifter *informs*, it does not *dictate*. The Demiurge must still
**author** (manufacture attachment, time reversals, collide threads) - it is a drama manager, not a
pure curator. If the graft lowers resistance so hard that Gamma only ever amplifies pre-existing
stories, you have turned it back into a curationist sifter and lost the apex thesis. Keep a floor of
director-initiated, manufactured threads; the sifter biases *selection*, it does not replace
*invention*. This balance is an explicit tuning target (S10).

---

## 6. Determinism and the off-switch

Every piece obeys the workspace invariants:

- **In-tick and deterministic.** The sift system is pure arithmetic + thresholds over the
  deterministic `Chronicle` ring. Like `gossip_spread`, it computes candidates from a **pre-write
  snapshot** of the ring so the result is **order-independent**, and it runs at a fixed point in the
  single-threaded schedule (before `director_step` in Phase B).
- **No randomness needed in v0.** If a future version ever samples or tiebreaks, it draws from a
  **dedicated `sift` derived RNG stream** (run-seed XOR a distinct constant), never from an existing
  stream - the separate-streams invariant.
- **Off by default = byte-identical.** `Chronicle`, `Sift`, and the pattern book are their own
  resources; `record()` and the sift system early-return when the layer is disabled. A world without
  the sift layer is bit-for-bit a world before it. With the layer on but the director off, the
  sifter merely observes (Phase A) and changes nothing in the sim.
- **The eval harness never feeds back** into sim state (S7), exactly as the SLM surface and the
  player's ledger never do.
- **V&V stays green.** The layer adds no money, births, or affordance uses, so `observe.rs`'s
  `Census`/`check` invariants are inert to it.

---

## 7. The eval harness - retelling dump + expressive range (extends observe.rs)

The field evaluates emergent-narrative systems by their **retellings**, not by moment-to-moment
text (Kreminski et al., FDG 2019). The existing `observe.rs` is the V&V skeleton (`Census` snapshot
+ `check` invariants + the emergent-professions histogram); extend it with two narrative read-outs.
Both are **dev-only** - never shown to the player (the no-meter rule; a visible interest score
converts Narrative into Submission and kills the apex aesthetic, `emergent-narrative-grounding.md`
S4.3).

- **Retelling dump.** At end of a headless run, dump the ranked `Sift` candidates (each with its
  `tension`, `cast`, and `support` episodes) interleaved with the director's `Cadence` and any
  interventions. A `Retelling { threads: Vec<RetoldThread> }` you can print and *read*. Acceptance
  (the brief's, kept): the top candidate should read as "a story" to a human seeing only its
  support episodes + tension label. If not, the interest heuristic (S4) is wrong - fix the weights,
  not the prose.
- **Expressive-range analysis.** Run many seeds headless; histogram the `tension`/`register`
  distribution of surfaced candidates (mirrors `Census::professions` / `trades_in_use`). Flags both
  **monotony** (the same story every time) and **incoherence** (no patterns ever fire). This is the
  knob-tuning instrument for the `drama` weights and the pattern book.

These two read-outs are what turn "the playtest felt the same" (`narrative_surfacing.md` S10) from a
vibe into a measurement.

---

## 8. Build plan (phased; each phase shippable and testable)

1. **S0 - the episode ring.** `Chronicle` resource + `Episode`/`EpisodeKind` + `record()` wired into
   the emit sites (events, opinion/grievance, factions, director, and the despawn site for `Death`).
   **`Killed` / `Wounded` / `Death` are first-class kinds from the start** - record `Death` at the
   despawn site now, and route `Killed` / `Wounded` through the combat/death subsystem when it lands
   (`docs/rpg_survival_exploration.md`); the ring is shaped so that adds only new `record(...)` calls,
   no reshaping. Bounded, off-by-default. Tests: ring fills deterministically over a seeded run;
   empty and inert when the layer is off (byte-identical).
2. **S1 - the sifter, read-only + the eval harness.** `SiftPattern` RON book + `Sift` resource +
   the in-tick sift system (retrospective then incremental, asserted to agree) + interest scoring +
   the retelling dump and expressive-range analysis. **Proves legibility with zero director
   coupling.** This is the milestone to ship first (the brief's "prove legibility before polish").
3. **S2 - the graft (the perception organ).** `director_step` consults `Sift` for thread seeding +
   beat resistance, gated by the sift flag. Tests: sift-off world byte-identical to today's
   director; sift-on world shows lower mean resistance / higher casting-salience on fired beats;
   determinism held; the manufactured-thread floor (S5) preserved.
4. **S3 (optional) - surface candidates through existing channels.** Let the tidings banner /
   journal reference high-interest *Active* candidates near the avatar at fiction-only fidelity
   (reuse the gossip/`Rumor` plumbing). Never the apparatus - no tension labels, no scores.

Test the way the rest of the sim is tested: seeded, deterministic, off-by-default; reuse a saved run
as a golden transcript for the retelling dump. Keep all director V&V tests green.

---

## 9. Data and seams

**Reused as-is:** `Register` / `Phase` / `Thread` / `Cadence` / `Director` and `director_step`
(`director.rs`); `ai::Curve` / `ai::Consideration` / `Input::Sanction` (`ai.rs`); `Opinion`
(`factions.rs`), `Grievance` / `Personality` / `Mood` (`people.rs`); the `EventQueue` / `AgentEvent`
emit path and `appraise` (`events.rs`); `Census` / `check` / `Violation` (`observe.rs`); the
`Gossip` / `Rumor` plumbing for the optional S3 surface; the RON + Registry content idiom (`data.rs`).

**Verified signatures (against source, 2026-06)** - the shapes the graft must match:

- `Director` (resource): `prominence: HashMap<Entity,f32>` (private; accessor `prominence_of(e)`),
  `threads: Vec<Thread>` (private; accessor `threads()`), `cadence: Vec<Cadence>` (pub),
  `log: Vec<(u64,String)>` (pub), `events: Vec<BeatEvent>` (pub; accessor `recent_events()`, capped
  `EVENT_CAP = 16`), `staged_total: f64` / `gratuitous_total: f64` (pub), `rng: SplitMix64` (private,
  seeded). Surface accessors `epithet_of(e)` / `situation_of(e)`.
- `Thread { id: u64, spine: Register, lead: Entity, other: Option<Entity>, phase: Phase, heat: f32,
  ripeness: f32, beats: u32, climaxed: bool, is_trunk: bool }`.
- `Cadence { tick: u64, beat: String, register: Register, phase: Phase, thread: u64,
  lead_prominence: f32, collision: bool }`.
- `BeatEvent { id: u64, tick: u64, register: Register, place: Coord, lead: Entity, other: Option<Entity> }`
  (the shape `Episode` generalizes).
- `Register` - 14 variants, with `is_trunk()` (Betrayal|Vengeance) and `is_bright()`;
  `Phase { Setup, Rising, Climax, Fall }`.
- `Beat { id: String, register: Register, tags: Vec<String>, phases: Vec<Phase>, tension: f32,
  stakes: f32, weight: f32, cast: Vec<Role>, pre: Vec<Pre>, effects: Vec<Effect> }`;
  `BeatBook(Vec<Beat>)`.
- `Effect` = Grudge / Sway / Stir / Turn / Decree / War / Disaster / Afflict / Reveal / Voice - no
  `Harm`/`Death` variant *today* (deaths are inferred via the director's wake-watching). Combat and
  death are a planned subsystem (`docs/rpg_survival_exploration.md`) and will be the emit site for the
  `Killed` / `Wounded` / `Death` episodes; the sifter is designed around their arrival, not absence.
- `Pre` = Exists / TraitAtLeast / TraitAtMost / MoodAtLeast / HasGrudge / HoldsThrone / InFaction /
  AtWar / VictimNearby. `Role` (8 variants) + `SLOTS = 8`.
- Casting: `cast_beat(...) -> Option<([Option<Entity>; SLOTS], f32)>`, where the `f32` is
  **salience** in `0..1` (cast fit = the inverse of resistance).
- The score product (in `director_step`):
  `weight * drama * salience * novelty * phase_bias * spine_bias * trunk_bias * collide_bias`, with
  `drama = stakes.max(0) * attachment * reversal`,
  `attachment = 1 + (lead_p + 0.5*other_p) / prom_scale`,
  `reversal = 1 + proto_high` (dark, `tension >= 0`) or `1 + proto_low` (relief), and
  `impact = drama * salience` gating `impact_floor`. The `sift_bias` graft (S5) is one more
  multiplier in this product.

**New (all in `agent_core`, behind the off-by-default discipline):**

1. `Chronicle` resource + `Episode` / `EpisodeKind` + `record()`, plus the one-line taps at the
   existing emit sites (S2).
2. The `SiftPattern` RON book (a new `assets/data/sift.ron`, parsed by `agent_core` like
   `beats.ron`) + `SiftPatternId` / `TensionId` interning.
3. `Sift` resource + `ThreadCandidate` + `SiftStatus` + the incremental matcher + interest scoring.
4. The sift system, scheduled before `director_step`; the Phase-B consult points inside
   `director_step` (gated).
5. The eval read-outs on `observe.rs`: `Retelling` / `retelling_dump()` + the expressive-range
   histogram, plus accessors on `Simulation` (dev-only).
6. A reserved `sift` derived RNG stream (unused in v0; for any future sampling).

---

## 10. Open questions

- **Window semantics.** Strict ordering vs. partial/out-of-order matching of the predicate window;
  how `window_ticks` interacts with long slow-burn arcs.
- **Ring sizing vs. slow burns.** A beloved groomed over a very long arc can age out of a bounded
  ring before the climax. Salience-weighted retention (the `MemRecord` Ebbinghaus idea) may be
  needed so the wound the world keeps reopening stays in the ring.
- **Base-rate surprise.** Per-pattern global counter vs. per-region; the cold-start problem
  (everything is rare early). Calibrate against the expressive-range histogram.
- **Interest weights.** Tuned by the eval harness, never guessed - the whole point of building S7
  alongside S1.
- **Coupling strength (the central tuning).** How hard the graft may lower resistance before Gamma
  stops authoring and degenerates into a pure curator. Keep a manufactured-thread floor (S5); decide
  its size.
- **Does the sifter also feed dialogue?** A soul caught in a sifted feud is plausibly more likely to
  *speak* to it - a candidate could raise the appeal of the matching conversational `Intent`
  (another `ai::Input` axis). Tempting and in-grain, but expands scope; deferred until the
  director graft proves out.
- **Player-side vs. sim-side surfacing (S3).** Whether surfaced candidates are display-only
  (player-side, never feeding back, like the ledger) or genuinely raise the director's hand toward
  the avatar's region (couples to the Distance-LOD question in `narrative_surfacing.md` S9).
