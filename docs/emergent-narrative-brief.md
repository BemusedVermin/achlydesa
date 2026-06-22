# Emergent Narrative — Engagement Layer Implementation Brief

> **Purpose of this document.** This is an implementation brief for an AI coding agent (Claude Code).
> It specifies *additive* modules that turn an existing simulation's event stream into legible,
> varied, player-facing narrative. It is grounded in the emergent-narrative research literature and
> in shipped procedural-narrative games (sources in the Appendix). Read the "Assumptions to confirm"
> section first and resolve those before writing code.

---

## 0. TL;DR for the agent

The game already has: a working simulation, NPCs with motivations/goals, an **event-sourced Chronicle**
(append-only event log), a **Lie/Want/Need dissonance engine** for characters, and a three-layer
dialogue stack: **storylets → HTN fragment assembly → tagged-grammar surface realization**.

The problem is **not generation** — it is **curation + framing** (legibility) plus **surface variety**.
Do **not** rewrite the three-layer stack. It is the correct substrate. Add the following layers:

1. **Sifter** — pattern-matches the Chronicle into ranked *candidate narrative threads* (the legibility engine).
2. **Director ("God")** — a drama manager that reads sifted threads and *intervenes* in the storylet/HTN
   layers to shape pacing. This is diegetic: the in-fiction "divine director" *is* the drama manager.
3. **Character Memory / Callbacks** — per-character memory of player-relevant events so specific people
   stay legible across time (Nemesis-style).
4. **Surface Realization upgrade** — keep the grammar for barks; route conversation/journal text through a
   pluggable realizer interface so a small local fine-tuned model can be slotted in later *behind the same
   interface* without rework.
5. **Surfacing & onboarding (no-mission sandbox)** — deliver threads via world-pull + Director-push +
   contracts *generated from threads*; teach the player with a guaranteed first thread and a dissolving
   prologue, never a quest with markers.
6. **Spatial / POI layer** — make hexes the spatial projection of the simulation (a place exists because
   someone/something is there), not exploration-for-loot content.

Guiding principle throughout: **the simulation/sifter decides WHAT happens and WHY it matters; the
realization layer only renders an already-decided beat.** ("Events first, rationalize after.")

---

## 1. Assumptions to confirm before coding

The agent must confirm these with the developer and adapt; do not guess silently.

- **Language / engine.** Interfaces below are written in TypeScript flavor (matching prior project work).
  If the codebase is C#, Rust, etc., translate the contracts faithfully; the shapes matter, the syntax does not.
- **Chronicle read API.** This brief assumes the Chronicle exposes (a) an append hook / subscription for
  new events and (b) a queryable history. Confirm the actual API surface and the `Event` schema before
  building the Sifter against it. The Sifter must treat the Chronicle as **read-only**.
- **Event schema.** Confirm the real fields. This brief assumes at least: stable `id`, `timestamp`/tick,
  `type`, participating `entityIds`, `locationId`, and a payload. If events do not currently carry the
  character-state context the Sifter needs (e.g. which Lie/Want/Need was implicated), decide whether to
  enrich events at emit-time or to join against character state at sift-time.
- **Storylet selection hook.** The Director needs a sanctioned way to bias/inject storylets. Confirm where
  storylet eligibility and weighting are computed so the Director can write into it cleanly.
- **Spatial model (drives §10).** Confirm whether sim entities/events carry hex coordinates (**spatial**) or
  the sim is **aspatial** (abstract relationships/goals) with the hex grid as a separate display. If spatial,
  POIs are a query/render problem; if aspatial, §10 requires a real binding-layer module.
- **Contract economy (drives §9).** Confirm whether a job/contract economy already exists. If yes, the
  contract board becomes the primary surfacing vehicle; if not, word-of-mouth + Director push is primary.
- **Scope of first milestone.** Recommend shipping Phases 0–2 first (Sifter + Journal channel) because they
  prove the legibility thesis with the least new surface area. Confirm before expanding.

---

## 2. Core architecture

```
                         ┌─────────────────────────────┐
   simulation events ───▶│  Chronicle (event-sourced)  │  (EXISTS, read-only to new modules)
                         └──────────────┬──────────────┘
                                        │ subscribe / query
                                        ▼
                         ┌─────────────────────────────┐
                         │  SIFTER                      │  ◀── pattern DSL + interest scoring
                         │  events → ThreadCandidate[]  │
                         └──────────────┬──────────────┘
                                        │ ranked threads
                    ┌───────────────────┼────────────────────┐
                    ▼                                          ▼
        ┌───────────────────────┐                 ┌──────────────────────────┐
        │  DIRECTOR ("God")     │ ── interventions │  JOURNAL channel         │
        │  drama manager        │ ───────────────▶ │  recounts sifted threads │
        │  pacing + prospective │                  └────────────┬─────────────┘
        │  intervention         │                               │
        └───────────┬───────────┘                               │
                    │ bias / inject                             │  decided beats
                    ▼                                            ▼
        ┌───────────────────────────────────────────────────────────────────┐
        │  EXISTING STACK:  storylets → HTN fragment assembly                 │
        └───────────────────────────────┬───────────────────────────────────┘
                                         │ decided Beat (structured)
                                         ▼
                         ┌─────────────────────────────┐
                         │  REALIZER (interface)        │
                         │  ├─ GrammarRealizer (exists) │  ← barks, cheap, expand corpus
                         │  └─ SlmRealizer (optional)   │  ← conversation/journal, local SLM
                         └─────────────────────────────┘
                                         ▲
                         ┌───────────────┴─────────────┐
                         │  CHARACTER MEMORY            │  ← per-character player-relevant events
                         │  feeds storylet preconditions│     (callbacks)
                         └─────────────────────────────┘
```

*Player-facing delivery (how threads reach the player, §9) and the hex/POI layer (§10) consume the same
`ThreadCandidate` / `Beat` / `Intervention` types defined below; they add delivery surfaces, not new engines.*

---

## 3. Shared data model

Define these once and share across modules. (Do not duplicate the existing `Event` type — import it.)

```ts
// A narratively-meaningful pattern match over the Chronicle.
interface ThreadCandidate {
  id: string;
  patternId: string;             // which SiftPattern produced it
  status: 'emerging' | 'active' | 'resolved' | 'abandoned';
  cast: EntityId[];              // characters/places bound by the pattern
  tension: string;              // short machine-readable label, e.g. "betrayal_pending"
  supportingEventIds: EventId[]; // the events that constitute the thread so far
  projectedShape?: BeatLabel[];  // optional: what beats would complete the arc
  interest: number;              // ranking score (see §4.2)
  firstSeenTick: number;
  lastUpdatedTick: number;
}

// A single decided narrative beat handed to realization. The CONTENT is already decided;
// realization only chooses words. This is the contract that lets grammar/SLM be swapped.
interface Beat {
  channel: 'bark' | 'conversation' | 'journal';
  speakerId?: EntityId;          // omitted for journal/director voice
  addresseeId?: EntityId;
  threadId?: string;             // link back to the thread this advances, if any
  intent: BeatLabel;             // e.g. "accuse", "reconcile", "boast", "recount_betrayal"
  facts: Record<string, unknown>;// the concrete particulars the text MUST convey
  tone: string[];                // tags for selection/conditioning
  callbacks?: CallbackRef[];     // prior events to reference (see §7)
}

// A Director action. Diegetically, this is "God" nudging the world.
interface Intervention {
  kind: 'bias_storylet' | 'inject_storylet' | 'set_flag' | 'spawn_pressure';
  threadId: string;              // the thread being shaped
  payload: Record<string, unknown>;
  reason: string;                // for debugging + retelling logs
}

type BeatLabel = string;
type EntityId = string;
type EventId = string;
```

---

## 4. Module: Sifter (Phase 1 — build first)

**Responsibility:** Read the Chronicle and surface a ranked, live list of candidate narrative threads.
This is the legibility engine. It is the single most load-bearing new module — design it to be modular
from the outset.

### 4.1 Pattern DSL

- Patterns are **composable** and **incremental** (detect threads *while they are still forming*, not only
  in retrospect). Model on Kreminski's *Winnow* (incremental sifting DSL) rather than retrospective-only sifters.
- A pattern is a predicate over an event subsequence that **binds variables** (characters, places) and emits
  or updates a `ThreadCandidate`.
- Patterns must be data/config, not hardcoded, so designers can add them without touching engine code.

```ts
interface SiftPattern {
  id: string;
  // Called on each new event with current bindings; returns updated match state or null.
  match(event: Event, bindings: Bindings, history: ChronicleView): MatchResult | null;
  // Optional: declare when a partial match should be considered "emerging" vs "active".
  stageOf?(match: MatchResult): ThreadCandidate['status'];
}

interface Sifter {
  registerPattern(p: SiftPattern): void;
  ingest(event: Event): void;            // incremental, called on append
  query(opts?: { minInterest?: number; status?: ThreadCandidate['status'] }): ThreadCandidate[];
  rebuildFrom(history: ChronicleView): void; // retrospective bootstrap on load
}
```

- **Build order within Phase 1:** implement retrospective `rebuildFrom` first (simpler to test against a
  saved Chronicle), then add incremental `ingest`.

### 4.2 Interest scoring

The hard part of sifting is that hand-written patterns match *too much*. Rank matches; do not surface all.
Combine two signals:

1. **Statistical surprise.** Prefer matches that are unlikely from a statistical perspective
   (Kreminski et al., "Select the Unexpected"). Track base rates of pattern instances and rank rarer ones higher.
2. **Dissonance magnitude (project-specific, higher value).** The existing Lie/Want/Need engine is a richer
   interest signal than raw statistics. The most narratable beats are where revealed behavior **violates a
   character's stated Lie** or **exposes the Want/Need gap**. Score threads by the dissonance they implicate.

```ts
function interestScore(t: ThreadCandidate, ctx: ScoringContext): number {
  return W_SURPRISE * surprise(t, ctx) + W_DISSONANCE * dissonance(t, ctx);
  // tune weights via the eval harness (§9), not by guesswork.
}
```

### 4.3 Acceptance criteria (Phase 1)

- Given a saved Chronicle, `rebuildFrom` produces a ranked thread list.
- The **top-ranked thread is retellable**: a human reading only the thread's supporting events + tension
  label would describe it as "a story." If not, the interest heuristic is wrong — tune §4.2 before moving on.
- Incremental `ingest` produces the same threads as `rebuildFrom` for the same event sequence.
- Adding a new `SiftPattern` requires no engine changes.

---

## 5. Module: Journal channel (Phase 2 — cheapest legibility win)

**Responsibility:** Recount sifted threads to the player in a framing voice (the Director/"God" voice).
This proves the curationist thesis with almost no new surface area: it is pure curation + framing, no new
generation. Ship this before the Director's interventions.

- Input: ranked `ThreadCandidate[]` from the Sifter.
- For each surfaced thread, emit `Beat{ channel: 'journal', intent: 'recount_*', facts: <thread particulars> }`
  to the realizer.
- Framing matters more than prose richness here. A modular "comic-panel"-style framing (à la Wildermyth) or
  an omniscient-director chronicle entry both work; pick one and make the frame a template.

**Acceptance criteria:** The journal shows, at any time, the 1–3 most interesting live threads, named and
framed, updating as they develop. A playtester can answer "what stories am I in right now?" from the journal alone.

---

## 6. Module: Director / drama manager ("God") (Phase 3)

**Responsibility:** Decide which emerging thread to amplify and *when*, then write into the storylet/HTN
layers to make it more likely to continue. This is **prospective intervention** (sifter identifies an
emerging thread; drama manager nudges it forward), and it is diegetic — the in-fiction divine director's
manipulations *are* these interventions.

### 6.1 Pacing strategies (pluggable)

Model the storyteller-personality pattern from RimWorld. Each strategy is a function from
`(threads, worldState, history) → Intervention[]`. Provide at least:

- **EscalatingCurve** — rising tension then relief then rising again (RimWorld "Cassandra").
- **FalseSecurity** — long calm, then hard turns (RimWorld "Phoebe").
- **Random** — minimally shaped (RimWorld "Randy"); useful as a baseline/control.

```ts
interface PacingStrategy {
  id: string;
  decide(threads: ThreadCandidate[], world: WorldStateView, history: ChronicleView): Intervention[];
}

interface Director {
  setStrategy(s: PacingStrategy): void;
  tick(now: number): Intervention[];   // called periodically; reads Sifter, returns interventions
}
```

### 6.2 Intervention surface

The Director must only write through sanctioned hooks (§1): bias storylet weights, inject a storylet,
set a flag the storylet preconditions read, or spawn "pressure" (an event that pushes a thread along).
It must **never** rewrite the Chronicle (the Chronicle records what happened; the Director shapes what
happens next).

### 6.3 Caution on aesthetics

Over-intervention erodes the "these events actually happened, unauthored" feeling that makes emergent
narrative compelling. Bias toward the **lightest** intervention that keeps a thread alive. Log every
intervention with `reason` for the eval harness.

**Acceptance criteria:** With `EscalatingCurve`, playthroughs show a measurable tension rhythm in the
Chronicle; with `Random` as control, they do not. Interventions are auditable and reversible in config.

---

## 7. Module: Character memory / callbacks (Phase 4)

**Responsibility:** Keep *specific characters* legible across time so the player tracks people, not just
events. This is the cheap, general core of the Nemesis pattern (avoid replicating the specific patented
pipeline; build from primitives — see Appendix note).

- Maintain a per-character memory of **player-relevant** events (wins, losses, slights, debts, escapes).
- Expose memory to storylet preconditions so characters can **reference past encounters** ("callbacks").
- Callbacks attach to `Beat.callbacks` so realization can surface them ("You're the one who left me for dead
  at the river crossing.").

```ts
interface CharacterMemory {
  record(entityId: EntityId, event: Event, salience: number): void;
  recall(entityId: EntityId, opts?: { aboutEntity?: EntityId; topK?: number }): CallbackRef[];
}
```

**Acceptance criteria:** An NPC the player wronged earlier references that specific event in a later
encounter, gated by memory rather than by generic role.

---

## 8. Module: Surface realization upgrade (Phase 5 — last)

**Responsibility:** Render decided `Beat`s into text. Keep the existing grammar; add an interface so a
small local model can be slotted in later for the channels where wit matters, **without** changing callers.

### 8.1 The interface (build this first, it's the seam)

```ts
interface Realizer {
  realize(beat: Beat): Promise<string>;  // beat is already decided; realizer only chooses words
}
```

### 8.2 Backends

- **GrammarRealizer (exists).** Use for **barks**. The fix for "templated" barks is corpus size + richer
  selection tags, not a new generator. Reference point: Caves of Qud's history reads well because a large
  authored corpus (tens of thousands of words) is repackaged by replacement grammar — variety is
  combinatorial over hand-written, voiced fragments. Action: expand the authored fragment corpus and the
  state-tags that gate selection.

- **SlmRealizer (optional, for conversation/journal).** A small, **fine-tuned, quantized, LOCAL** model used
  strictly as a surface realizer. This is the *reasonable* form of "use an LLM," and it directly addresses
  prior bad experiences (hallucination, instruction-following, compute). Design constraints, from the 2026
  SLM-for-games proof of concept (Appendix):
  - **Tiny base model** (e.g. ~1B params) + LoRA fine-tune, quantized to 4–8 bit (≈0.8–1.3 GB footprint),
    run locally (e.g. llama.cpp); fits alongside a game on an 8 GB consumer GPU.
  - **No runtime instruction prompt.** Train the model to map **structured input → structured output**
    (feed it `Beat.facts`/state as structured data, not a natural-language instruction it can disobey).
    Structured I/O is the main lever for the creativity-vs-consistency trade-off.
  - **Renderer only.** The model never decides plot, only phrasing → plot hallucination is impossible by
    construction.
  - **Retry-until-valid**, masked by animation. Generate at temperature ~0.75; validate output against a
    cheap check; retry on failure. ~2 attempts typically suffice; budget a few seconds hidden behind a
    camera move or "scribe is writing…" beat.
  - **Ground training data in the game world** via a DAG of choice nodes (lore lists) + generation nodes,
    using a larger teacher model offline to write the fine-tuning corpus once.
  - One narrowly-scoped model per task; compose specialists rather than one generalist.

**Acceptance criteria:** Callers depend only on `Realizer`. Swapping `GrammarRealizer` ↔ `SlmRealizer`
for a channel requires no change above the interface.

---

## 9. Surfacing threads to the player (no-mission sandbox)

**Context.** This is an immersive-sim sandbox with **no mission structure**. Current player surface = a hex
grid over the world, verb interactions on a tile, and a conversation pane. "How threads reach the player" is
its own design problem, separate from generating them, and must be solved **before** onboarding — a tutorial
only ever teaches the steady-state vehicle, so the vehicle must exist first.

**Four channels.** Route every sifted thread through a deliberate mix:
- **World / pull (primary).** NPC barks and rumors referencing live threads (`Beat{channel:'bark', threadId}`).
  Ambient headline layer; respects autonomy but only reaches a present, attentive player.
- **Journal / index (backstop).** §5. Always-available "what threads am I in / aware of." The **index, never
  the primary driver** — if the player must open a menu to *discover* a story, the immersion is already lost.
- **Director / push (calibrated).** §6. When a thread the player is tied to ripens, the Director arranges an
  intersection: someone seeks them out, an ambush, a letter, a remembered face returns (callback, §7). This
  is the **load-bearing** vehicle for a sandbox and is diegetic — it *is* "God."
- **Reputation / passive.** The player feels threads through how the world treats them.

**Policy: pull by default, push when justified.** Trigger a push when (a) the player has been passive beyond a
threshold, or (b) a high-interest thread is about to resolve unwitnessed. **Let threads resolve without the
player sometimes** — missability is what makes the caught ones feel real; do not guarantee witnessing. All
thresholds are config.

**Contracts generated from threads (recommended primary vehicle for a mercenary sandbox).** Do **not** author
contracts. Derive them from sifted threads + world state, so a contract is the visible tip of a thread and the
board gives quest-like *legibility and goals* without quest-like *authorship*.

```ts
interface ContractOffer {
  id: string;
  threadId: string;                 // the sifted thread this surfaces
  issuerId: EntityId;               // who's offering, drawn from the thread's cast
  objective: BeatLabel;             // e.g. "raid_caravan", "extract_person"
  locationId?: string;              // where it resolves (see §10)
  stakes: Record<string, unknown>;  // what shifts in the sim on success/failure
}
// Generated by querying the Sifter. Accepting / declining / ignoring an offer all Chronicle
// as events, so the player's response to a contract is itself re-siftable.
```

**Onboarding — NOT a quest.** A marker-following tutorial teaches marker-following, the opposite of the
literacy a sandbox needs (a player trained on a marker then hunts for the next marker and reads the sandbox as
empty). Two moves, combinable:
1. **Guaranteed first thread.** The Director forces one clean, low-stakes, highly legible thread to form early
   and surface through the *real* vehicles (rumor → an NPC who remembers you → a consequence). The player
   learns the system by living a genuine instance of it, not a scripted fake.
2. **Dissolving prologue.** A short bounded opening that teaches the verbs and that *actions are remembered*,
   then a visible, diegetic hand-off ("no one is going to tell you what to do now") so the player understands
   the rail is gone **by design**, not by omission.

What you teach is **narrative literacy** — threads exist, you're in them, the world remembers, things happen
with or without you, the journal is the index and the world is the headline — not mechanics. On-theme: a divine
director staging the player's first hook and engineering coincidences is exactly what the meta-narrative means,
so the onboarding mechanism is also a thesis statement.

**Acceptance criteria.** A new player, given no markers, forms and notices their first thread within the opening
session via pull + one Director push. Declining/ignoring a contract is recorded and can itself spawn or alter
threads. In playtest logs, some high-interest threads are observed to resolve unwitnessed without breaking the
experience.

---

## 10. Spatial / POI layer (hex world)

**The trap to avoid.** "Procedurally generate POIs for exploration" is a *spatial-content* framing, but the
problem is narrative surfacing. Exploration-for-loot content (ruins/dungeons to find and clear) becomes a
*second game* competing with the narrative engine for attention — breadth without meaning, the classic
proc-gen sandbox failure. **A POI should exist because something is happening there or someone lives there**,
so crossing the grid is moving through the story-world and exploration's payoff is people, threads, and
consequences (exactly what the Sifter already tracks) — not loot.

**Two layers:**
- **Persistent anchor layer (worldgen).** Settlements, faction holds, a few landmarks. Required for **spatial
  memory** — the player must be able to form a mental map ("the river crossing, the hold in the north").
  Stable, even if generated.
- **Dynamic, thread-bearing layer (sim/Director-placed).** The camp where a sifted conflict is playing out,
  the ambush, the crossroads where a remembered NPC reappears. Ephemeral, fiction-motivated. This is the
  calibrated push (§9) made spatial: the Director revealing/placing a POI *is* God arranging an intersection.
  Contracts (§9) resolve at these places.

**Discovery reveals state, not terrain.** Uncovering a tile surfaces *who is here and what they want* (the
Want/Need tension, who holds it) and Chronicles the discovery as a **siftable event**. Exploration becomes an
**information economy** — explore to learn about threads (intel, secrets, grudges), which the journal then makes
legible — rather than a loot loop.

**Density over breadth.** A small region the sim can keep alive beats a vast map with sparse meaning, every
time, for this genre. Generate a region, not a continent. If a place persists, let the sim keep changing *what
is true there* (who holds it, what's happening) even when the terrain is fixed. **Never ship static placed
dungeons disconnected from the sim** — that is the bolted-on "second game."

**The seam (confirm first — see §1).** Is the simulation already **spatial** (entities/events carry hex
coordinates) or **aspatial** (abstract relationships/goals) with the hex grid as a separate display?
- **If spatial:** POIs mostly fall out of querying the sim by location — a reveal/render problem.
- **If aspatial:** you need a **binding layer** projecting sim state onto hexes. This is a real module; design
  it deliberately, because it is the seam where the narrative engine meets the map.

```ts
interface PointOfInterest {
  hex: HexCoord;
  kind: 'anchor' | 'dynamic';
  occupants: EntityId[];
  threadId?: string;                         // the thread this place currently bears, if any
  revealedState?: Record<string, unknown>;   // what discovery surfaces (tension, holder, …)
}
interface SpatialBinding {
  poisAt(hex: HexCoord): PointOfInterest[];  // spatial sim: query; aspatial sim: project from sim state
  reveal(hex: HexCoord): Event;              // emits a siftable discovery event
}
```

**Acceptance criteria.** Every dynamic POI traces to a live thread or occupant; none are placed merely to
"fill the map." Revealing a tile emits a Chronicle event and surfaces occupant tension. Disabling the dynamic
layer leaves a still-navigable anchor map (proving the split is clean).

---

## 11. Eval harness (build alongside Phase 1; do not skip)

The field evaluates emergent-narrative systems by their **retellings**, not by moment-to-moment text.
Instrument accordingly:

- **Retelling dump.** At end of a playthrough, dump the threads the Sifter surfaced (ranked, with supporting
  events and any Director interventions). Read them. Would you retell any to a friend? If the top thread is
  not retellable, fix the **interest heuristic (§4.2)** — not the prose.
- **Expressive range analysis.** Run many headless simulations; visualize the distribution of thread types/
  tensions the Sifter finds. Flags both monotony (one story every time) and incoherence (no patterns ever fire).
- **Intervention audit.** Diff `Random` vs `EscalatingCurve` pacing on tension rhythm to confirm the Director
  is doing something, and that it isn't over-authoring.

---

## 12. Recommended build order (each phase independently shippable)

| Phase | Module | Proves |
|-------|--------|--------|
| 0 | Shared data model + confirm Chronicle/storylet hooks + spatial & contract assumptions (§1, §3) | Foundations |
| 1 | Sifter: retrospective → incremental + interest scoring (§4) + eval (§11) | Legibility engine works |
| 2 | Journal channel recounting sifted threads (§5) | "What stories am I in?" answerable |
| 3 | Director / drama manager with pacing + prospective intervention (§6) | Pacing & diegetic "God" |
| 4 | Character memory + callbacks (§7) | People stay legible across time |
| 5 | Realizer interface; expand grammar corpus; optional SLM renderer (§8) | Surface variety |
| 6 | Spatial / POI layer: anchor + dynamic, discovery-reveals-state (§10) | Places mean something |
| 7 | Surfacing policy (pull/push) + contracts-from-threads (§9) | Threads reach the player without a rail |
| 8 | Onboarding: guaranteed first thread + dissolving prologue (§9) | Player learns literacy, not markers |

Ship 0–2 first. Do not start Phase 5's SLM work until the Sifter's top threads are reliably retellable —
surface polish on illegible threads is wasted effort.

Phases 6–8 come after the core proves out and depend on §1's spatial/contract answers (Phase 6 is a
query/render problem if the sim is spatial, a new binding module if it is aspatial). **Build onboarding (Phase
8) last** — it can only teach delivery vehicles that already exist. If world/diegetic surfacing is wanted
earlier, Phases 6–7 may move up, but never ahead of a working Sifter (§4) and Journal (§5).

---

## Appendix A — Source material (consult as needed)

**Research — curationism & story sifting (legibility):**
- James Ryan, *Curating Simulated Storyworlds* (dissertation) — curationist emergent narrative; "story sifting."
- Kreminski, Wardrip-Fruin, Mateas — *Winnow*: incremental/prospective story-sifting DSL.
- Kreminski et al., "Select the Unexpected: A Statistical Heuristic for Story Sifting" (ICIDS 2022).
- "Stories from the Bottom Up: Emergent Narratives with Composable Story Sifting Patterns" (FDG, ACM 3723809).
- "Awash: Prospective Story Sifting Intervention for Emergent Narrative" (sifter + drama manager hybrid).
- Johnson-Bey et al., *Neighborly* / *Talk of the Town* — simulationist story generators; Centrifuge sifting tool.

**Research — storylets & structure:**
- Kreminski & Wardrip-Fruin, "Sketching a Map of the Storylets Design Space" (ICIDS 2018).
- Emily Short — quality-based / salience-based narrative writing (blog: emshort.blog).
- Kreminski et al., "Evaluating AI-Based Games through Retellings" (FDG 2019) — the retelling eval method.

**Research — local generation (the reasonable LLM path):**
- Munk, Valdivia, Burelli, "High-quality generation of dynamic game content via small language models:
  A proof of concept" (arXiv 2601.23206, Jan 2026) — fine-tuned quantized SLMs as scoped renderers; DAG
  training-data grounding; structured I/O; retry-until-success; on-device timing.

**Shipped games:**
- *Caves of Qud* — Grinblat & Bucklew, "Subverting historical cause & effect: generation of mythic
  biographies in Caves of Qud" (PCG workshop) — events-first + replacement grammar over a large authored corpus.
- *RimWorld* — AI Storyteller (drama-manager) personalities (Cassandra/Phoebe/Randy) as pacing functions.
- *Wildermyth* — alternating authored/system-driven layers; personality-gated lines; character substitution;
  comic-panel framing. Note its limitation: any character castable in any role → weak iconicity (your
  Lie/Want/Need engine is the countermeasure).
- *Shadow of Mordor / War* — Nemesis system: per-character memory + "yes-and" callbacks + persistence.

## Appendix B — Legal note (not legal advice)

Warner Bros. holds a patent covering the specific Nemesis *system* (in force to ~2036, and historically used
to deter close clones). Build §7 from **general primitives** — per-character event memory, salience scoring,
callback selection in dialogue, character promotion/persistence — rather than replicating their specific
claimed pipeline. If this module grows central to the game, get qualified legal review.
