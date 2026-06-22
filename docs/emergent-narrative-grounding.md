# Emergent Narrative - Grounding the Engagement-Layer Brief (with an MDA reading)

> **Status: analysis (2026-06).** A companion to `docs/emergent-narrative-brief.md` (an
> external implementation brief written without sight of this codebase). This doc resolves the
> brief's "assumptions to confirm" against the real architecture, says what is already built,
> isolates the one genuinely-new idea worth taking, names the things the brief would *harm* if
> followed literally, and reads the whole proposal through MDA (Mechanics -> Dynamics ->
> Aesthetics). Read alongside `narrative_director_v2.md` (what Gamma stages), `narrative_surfacing.md`
> (how it reaches the player), and `dialogue.md` (the meaning/surface split). The concrete
> follow-on spec is `narrative_sifter.md`.
>
> Note on names: the narrative director is written `Γ` in the other docs; here it is spelled
> "Gamma" (this doc is ASCII-only).

---

## 0. Bottom line up front

The brief is a good, literature-grounded document written **as if the codebase were a blank
slate**. It explicitly says (its S1) to confirm its assumptions before coding. Having confirmed
them against source:

- **All three of its core architectural assumptions are wrong for this repo.** There is no
  event-sourced Chronicle, no Lie/Want/Need engine, and no storylet/HTN stack.
- **Four of its six proposed modules are already built or already specced here** - and in two
  cases (the Director and the Realizer) the existing design is *ahead* of the brief. Followed
  literally, the brief would regress the director and rebuild the dialogue stack.
- **It contains exactly one genuinely new, high-leverage idea this codebase lacks: the Sifter**
  (bottom-up story detection), plus two smaller real gaps (retelling-based evaluation, and
  onboarding).

The valuable move is not to *implement the brief*. It is to graft the Sifter onto the existing
top-down director as its missing perception organ. That graft - and the eval harness that lets
you tune it - is what would actually make the narratives better. The argument for why runs
through the MDA reading in S4. The build is specified in `narrative_sifter.md`.

---

## 1. The brief's mental model vs. reality

The brief (its S0) assumes the game has an "event-sourced Chronicle," a "Lie/Want/Need dissonance
engine," and a "storylets -> HTN fragment assembly -> tagged-grammar" stack. Confirmed against
source, none of those is accurate:

| Brief assumes | achlydesa actually has | Verdict |
|---|---|---|
| **Event-sourced Chronicle** - an append-only, queryable global log the Sifter reads | `EventQueue` is **transient, drained every tick**, carrying only 3 variants (`Crowned`, `Deposed`, `Transgressed`; `agent_core/src/events.rs`). Persistent history is **fragmented** across non-unified, special-purpose stores: per-soul `MemRecord` (dialogue); the director's `Cadence` log and its capped `BeatEvent` ring (`recent_events()`, cap 16) - both **director-authored only**; and the `Gossip` rumor store. | **No substrate for the Sifter exists.** The brief's central module reads a log that is not there - and what append-only logs exist record only Gamma's own beats, not the emergent world events the Sifter needs. The most important correction. |
| **Lie/Want/Need dissonance engine** (stated lie vs. want vs. true need; drama = the gap) | Reiss/CK3-style innate **traits** (ambition<->contentment, vengeance<->forgiveness, greed, sociability, piety, caution) in `Personality(Vec<f32>)`; transient named **moods** (joy/calm/anger/sorrow/fear/hope/love/awe plus the achlydesan rapture/despair/dread/elation/foreboding; data in `moods.ron`); dyadic `Opinion(HashMap<Entity,f32>)`; a target-only `Grievance(Entity)` (**no magnitude field**); `Needs`. | **No Lie/Want/Need anywhere.** The brief's "dissonance magnitude" interest signal (its S4.2) must be re-grounded on the real social state - and "grievance magnitude" must be *derived* (recency + convergence), since `Grievance` carries no weight. |
| **storylets -> HTN -> grammar** | **IAUS** utility scoring picks the goal (`ai.rs` + `goals.rs`); **GOAP** A* plans the action sequence (`plan.rs`, the `Step` enum); a generative **grammar** + optional **SLM** render the dialogue surface. No storylets, no HTN. | **Half right at the surface only.** The closest things to "storylets" are the director's RON `Beat`s and the dialogue `Intent`s - both precondition-gated, effect-bearing, data-authored. |

The brief was evidently written without sight of `narrative_director_v2.md` and
`narrative_surfacing.md`. Those two docs and the brief are near-isomorphic in their channel design
(Nemesis / gossip / witness / ledger / contracts): the project has independently converged on the
same literature. Where the brief adds value is precisely the small set of places it diverges.

---

## 2. Module by module: what exists, what is a real gap

**Brief S4 (Sifter) - the one real architectural gap.** There is no bottom-up story detection.
The director's `Thread`s are constructed **top-down**: the director picks a `spine` (a `Register`),
pins a `lead`/`other`, and fires beats to *manufacture* the arc. Nothing reads "what stories are
already forming in the world" and ranks them. This is the brief's most valuable contribution. See
`narrative_sifter.md` for the grounded build.

**Brief S6 (Director / "God") - already built, and ahead of the brief.** The brief proposes a
RimWorld-style pacing manager with three pluggable curves (EscalatingCurve / FalseSecurity /
Random). achlydesa's director (Gamma; `director.rs` + `beats.rs`; status BUILT) is a
**drama-maximizer**: `score = drama x novelty / resistance`, `drama = stakes x attachment x
reversal`, with manufactured persistent prominence, 2-3 staggered groom->climax->fall threads,
cross-thread collisions, emergent register dominance, an impact-floor that yields the "freed world
tells 0 beats" liberation, and the Demiurge/deniability thesis. **Implementing the brief's Director
would be a strict regression.** The brief's one usable idea here - pacing *personality* - already
exists as register-rotation plus phase scheduling, and could be exposed as a config dial
("Phoebe vs. Cassandra") if wanted.

**Brief S7 (Character memory / Nemesis) - mostly built.** `MemRecord` + Ebbinghaus forgetting +
the `SharedHistory` IAUS axis (`dialogue.rs`); arc-aware epithets ("the Betrayed") and recurrence
are shipped per `narrative_surfacing.md` S10. The brief's Appendix-B patent caution is sound and
already honored (built from general primitives).

**Brief S8 (Realizer + SLM) - already built; the brief's prescription is your existing design.**
`dialogue.rs` already has the `TextGen` trait, `build_prompt`, `state_hash`, and `SlmRealizer<G>`
with grammar fallback. The brief's SLM constraints (tiny local model, structured I/O not
instructions, renderer-only so plot-hallucination is impossible, retry-until-valid masked by
animation) are verbatim your `dialogue.md` S4b. You also go one better: `narrative_surfacing.md`
S3 turns the SLM's hallucination into the **fidelity-graded veil**, a feature the brief never
imagines. Nothing to do except keep the seam.

**Brief S9 (surfacing, no-mission sandbox) - mostly built; onboarding is the gap.** The four
channels (world/pull, journal/index, director/push, reputation) are exactly
`narrative_surfacing.md` S4. Contracts-from-threads exists as `quest_for()` (derives a charge from
a live thread's lead+other; `agents/src/lib.rs`); push exists as the tidings banner; you even added
`player_counsel(npc, calm)` to feed or cool a vendetta. **What is genuinely missing: onboarding** -
the "guaranteed first thread" + "dissolving prologue." A real, on-theme gap.

**Brief S10 (spatial / POI) - the brief's hard question is already answered.** The brief frets over
whether the sim is spatial or aspatial. **It is spatial**: entities carry `Position`/`Coord`,
gossip groups souls by tile, the world is a hex `Topology`. So POIs are the *easy* branch
(query/render), not a binding-layer module. POI-A (legibility) and POI-B (the Use verb) are
shipped; POI-C/D/E are deferred in `narrative_surfacing.md` S8.

---

## 3. Grounding: what to build, what not to, and the invariants the brief ignores

### 3.1 The one thing worth building: the Sifter as the director's perception organ

The deep framing - which the brief's own Appendix names but does not connect - is **curationism
vs. drama management** (Ryan vs. Mateas/Stern). The brief leans curationist (find the story the sim
already told). achlydesa has bet hard on drama management (make the story happen, with the Demiurge
as the thematic alibi). The synthesis is **Awash** (which the brief cites as a "sifter + drama
manager hybrid"), and it is exactly what is missing.

The precise gap: the director's `resistance` term reads casting salience from a **snapshot** of
current state ("is this rival already ambitious right now?"). It cannot perceive a **trajectory** -
"these three souls have been escalating a feud across the last 40 ticks" - because there is no
multi-tick event history to pattern-match. A Sifter gives Gamma **temporal perception of forming
stories**, and that perception directly strengthens the apex thesis: you can only "nudge where the
world already leans" (the deniability rule) if you can *perceive* where it leans. The Sifter is the
eyes that make least-resistance selection truly least-resistance. See `narrative_sifter.md`.

### 3.2 Re-grounding the brief's pieces on real types

- **The Chronicle it reads.** Do not build the brief's lossless global log. The honest minimum is
  a small, **bounded, structured, off-by-default episode ring**, fed from the sites that already
  emit `AgentEvent` plus the director's beat firings and the opinion/grievance/throne/faction
  transitions. The Sifter reads structured handles, never prose (the `MemRecord.summary_key`
  discipline). The existing fragments do *not* suffice as-is (cadence = director-authored only;
  gossip = lossy/decaying; `MemRecord` = per-soul and thinly populated), so a modest new store is
  required - see `narrative_sifter.md` S2 for why and how small.
- **Interest = surprise + dissonance, re-grounded.** "Statistical surprise" maps to base-rate
  rarity of a register/pattern (the director already half-tracks this via its novelty/recency
  penalty). "Dissonance," with no Lie/Want/Need, re-grounds onto the real state: **grievance
  magnitude**, **opinion reversals** (an ally going cold in `Opinion`), **trait-vs-norm tension**
  (a vengeful soul under a no-kill `Sanction` - literally the `Input::Sanction` axis), and **mood
  reversals** (a triumph turned to sorrow). Richer than the brief's statistics, exactly as the
  brief argues for the Lie/Want/Need case.

### 3.3 The invariants the brief is silent on (and must obey)

The brief never mentions determinism. In this repo that is non-negotiable (CLAUDE.md):

- **A Sifter that feeds Gamma's selection runs inside the seeded tick and must be byte-identical**,
  drawing any randomness from a *dedicated derived RNG stream* (run-seed XOR a new constant, as
  `rumor`/`DirectorRng` do). A Sifter that only produces player-facing retellings may run off-tick
  and player-side (like the SLM). The brief's async `Promise`-returning, `rebuildFrom`-on-load
  interfaces fit the off-tick use, *not* the in-tick director-feeding use. These have opposite
  constraints; decide which first.
- **Off by default = byte-identical.** With the sift layer off, the director is bit-for-bit what it
  is today. The layer keeps its state in its own resources and early-returns when disabled.
- **Never put the SLM in the deterministic path.** Already your rule; the brief's S8 agrees by
  accident.
- **Integer economy / V&V invariants** (`observe.rs`) stay green; the surfacing/sift layers add no
  money, births, or affordance uses, so `Census`/`check` are inert to them.

### 3.4 What NOT to build (where the brief would harm)

- **Do not build the brief's Director.** It is behind yours. At most, expose Gamma's register
  rotation as a named pacing-personality dial.
- **Do not build a lossless global Chronicle.** Build the bounded episode ring (S3.2).
- **Do not import Lie/Want/Need.** Use grievance / opinion / trait / norm / mood.
- **Do not let `quest_for` charges become a markered task list.** Keep them derived, diegetic ("a
  commotion to the east," not a waypoint), and *missable* (the MDA collision in S4.3).

---

## 4. MDA reading

Mechanics (rules, data, algorithms) -> Dynamics (runtime behavior over time + player input) ->
Aesthetics (the emotional experience). Designers build M->D->A; players read A->D->M. The
discipline: judge every proposed mechanic by the dynamic it produces and whether that dynamic
serves or collides with the target aesthetic.

### 4.1 The target aesthetic stack (what achlydesa is actually for)

Mapping onto LeBlanc's eight: the design targets **Narrative** (drama), **Discovery** (the
Outer-Wilds gnosis loop), **Fantasy** (the Gnostic dream-purgatory), and **Fellowship** (named
souls who remember you). But its apex aesthetic is not in the standard eight: **complicity / felt
manipulation** - the dramatic irony of discovering that your own affection was authored, in the
DDLC / Pony Island lineage the docs cite. "The player should feel manipulated"
(`narrative_director_v2.md` S0) is the design's whole payload. Every mechanic is judged against
*that*.

### 4.2 The core M -> D -> A chain the whole game rests on

- **Mechanic:** least-resistance selection, `drama x novelty / resistance`, with the hard rule
  "never select a beat the world could not plausibly have produced itself."
- **Dynamic:** *deniability* - each beat is individually plausible (an in-world alibi), but the
  aggregate (too many reversals, too well-timed) becomes viscerally felt though no single instance
  proves a hand.
- **Aesthetic:** the felt manipulation. The apex.

This chain is the lens for the brief. A proposed mechanic is good iff it strengthens it.

### 4.3 The brief's proposals through MDA

**Sifter -> STRENGTHENS the apex.** Better perception of where the world leans -> lower true
resistance -> more plausible nudges -> stronger deniability dynamic -> stronger felt-manipulation
aesthetic. The rare proposal that feeds the apex chain directly. **Build it.**

**SLM + fidelity veil -> serves Discovery + Fantasy, via a textbook MDA move.** The mechanic's
weakness (hallucination) is re-framed as the intended aesthetic (a dream-purgatory world that
lies). The dynamic is the `hear (distorted) -> seek -> witness -> know` loop; the aesthetic is
gnosis. The cleanest MDA design in the project - keep it.

**Onboarding -> the onboarding mechanic IS the thesis.** A Demiurge staging your first hook is the
apex aesthetic experienced in miniature. Teaching *narrative literacy* (threads exist, the world
remembers, things happen without you) rather than *marker-following* is the difference between a
player who reads the sandbox as alive and one who reads it as empty. Real gap, on-theme - but build
it last, since it can only teach vehicles that already exist (the brief is right about ordering).

**Aesthetic collisions to forbid (where the brief, taken literally, damages the apex):**

- **A visible thread/interest/tension panel** (the brief's S11 retelling dump, if shown to the
  *player*) converts the **Narrative** aesthetic into **Submission** - a meter to optimize.
  Optimizing a manipulation-meter destroys moral reflection and *kills the apex*. Both your docs
  already forbid this (`narrative_surfacing.md` S2, `narrative_director.md` S8). Keep the retelling
  dump strictly **dev-only**. The single most important MDA guardrail.
- **Contract board / quest markers as the primary vehicle** produce a **Challenge/Submission**
  dynamic (checklist completion) that collides with the no-marker literacy goal. Keep `quest_for`
  charges derived, diegetic, and *missable* - missability is what makes the caught stories feel
  real.
- **Journal as primary driver** -> if the player must open a menu to *discover* a story, immersion
  is already lost. The journal is the index, never the headline. Both docs agree.

### 4.4 The MDA verdict on "will it make the narratives better?"

The honest diagnosis from `narrative_surfacing.md` S10 is that a playtest "felt the same" not
because modules were missing but because the drama was all **pull** and low-density - an
*encounterability* problem, since addressed with tidings/markers/counsel/charges/density. That
tells you the lever is **dynamics, not more mechanics**. Of the brief's proposals, the Sifter is
the only one that moves the apex dynamic (deniability/encounterability), and the eval harness is
the only one that lets you *measure* whether the dynamics are landing. Everything else the brief
proposes is built, designed, or a regression.

---

## 5. Recommendation (priority order)

1. **Sifter as Gamma's perception organ** (the Awash hybrid). The one genuinely-new,
   apex-strengthening idea. Build the bounded episode ring; ground "interest" on
   grievance/opinion/norm/mood; honor seeded-stream + off-by-default. Spec: `narrative_sifter.md`.
2. **Retelling + expressive-range eval harness**, extending `observe.rs`/`Census`. Dev-only. This
   is what tells you whether any of it is working, and how to tune the `drama` weights.
3. **Onboarding** (guaranteed first thread + dissolving prologue). Real gap, deeply on-theme. Last,
   because it can only teach vehicles that already exist.
4. **Ignore** the brief's Director, Realizer, Memory, Gossip, and spatial-binding modules as new
   work - they are built or designed, and in two cases the brief is behind you. Keep the brief as
   external citation/validation.

Net: treat the brief as a literature-grounded second opinion that confirms the project's direction,
harvest its one missing idea (the Sifter), respect the two aesthetic guardrails it would otherwise
trip (no visible meter; keep charges missable), and bolt the new work onto the
determinism/off-by-default invariants the brief never mentions.
