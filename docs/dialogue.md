# Emergent Dialogue — the NPC's inner life, spoken

> **Status: BUILT (2026-06).** All three layers are implemented in
> `agents/src/dialogue.rs` (+ `assets/data/intents.ron`, `assets/data/grammar.ron`, the `ai::Input`
> extension, and the director's `Effect::Voice` hook), with the demo
> (`agents/examples/dialogue_demo.rs`) and tests. The §8 build plan was executed in full;
> what shipped vs. what is deferred is in §11 (added at build time). Read alongside
> `docs/narrative_director.md` / `_v2.md` and the IAUS/GOAP code. The design was settled
> with the owner over a design dialogue; the rationale and rejected alternatives are
> preserved below.
>
> **Update (2026-06-27) — text conversion:** this meaning/surface split is being **generalized** from
> one-line utterances to *all* descriptive prose (scenes, NPCs, world events, oblique "Wolfean"
> implication) for the new text/TUI front-end. The grammar is the model; the optional `voice` SLM
> re-voicer (§4b below) is **retired** (an LLM can distort/invent, which the new "never false" mandate
> forbids). See **`docs/prose_generation.md`** for the full procedural-prose engine and
> **`docs/text_interface.md`** for the front-end. The dialogue meaning layer (intents, scoring,
> memory, the director hook) is unchanged.
>
> **What the build proves:** a peopled world *speaks* — each co-located soul says the
> thing it most wants to say, scored by the same IAUS utility that ranks its goals, the
> words composed from a generative grammar (never a phrasebook) and coloured by mood. A
> vengeful soul threatens, a grieving one is consoled, a forgiving one reconciles — all
> emergent from state, all grounded (every line names the soul it addresses), and
> deterministic (same seed → the same conversation, word for word). With the director also
> awake, a betrayal Γ engineers is *heard* — the friend renounces the protagonist aloud
> (`Effect::Voice`). The player avatar speaks with the **same** intent verbs. The optional
> SLM realizer is a clean out-of-band seam (grammar fallback), never load-bearing. See
> `cargo run -p agents --example dialogue_demo --release`.

---

## 0. The thesis

A player feels an NPC's **humanity** through **grounded specificity**, not eloquence. A line
like *"You let my brother starve through the long winter while you feasted in the keep — I
haven't forgotten"* lands because every clause is true in the simulation: a real
[`Grievance`](../agents/src/people.rs), a real remembered famine (a `Disaster`/`Afflict` the
director or the world authored), an [`Opinion`](../agents/src/factions.rs) gone cold, a real
`vengeance` trait. No general-purpose chat model can manufacture that; **the simulation
already has it.**

So the design inverts the usual framing. Production NPC-LLM systems (Inworld's −2..+2 trust
graph, inZOI's "traits + situation → goals", Versu's social practices) spend their effort
*faking* structured social state so a model has something to talk about. This sim already
computes that, richer than any of them. **The simulation is the soul; the language model is
only the realizer.** Two consequences shape everything:

1. **Dialogue is a new action modality for the brain we already have**, not a bolt-on
   content system with its own authored logic. *Speaking is acting.* The same IAUS+GOAP that
   decides what an agent *does* decides what it *means*; the words are generated from that.
2. **Emergence lives in the meaning; the surface is generated, never retrieved.** A template
   phrasebook — however combinatorial — is a finite set a human wrote, the "canned responses"
   this project's whole architecture rejects. So the words are *composed* (by a generative
   grammar) or *generated* (by a small model), never selected from a written line list.

It is also the missing channel for the **narrative director**. Γ's manipulation is currently
invisible: opinions shift and grudges form, but the player never *hears* it. Dialogue is how
"the player should feel manipulated" finally becomes *felt*.

---

## 1. The load-bearing split: meaning is simulation, surface is rendering

The decision everything hangs on, and the one that keeps determinism intact:

- A **conversational intent** is a *simulation* object — *what an NPC means to convey to whom,
  and what it does to the social state*. It is scored deterministically from the agent's own
  drives/relations/memory, it mutates opinion/mood/grievance like any other system, and it is
  **part of the seeded tick**. Byte-identical, off-by-default, exactly like the director.
- The **surface text** — the actual words — is *rendering*. It is generated **out of band**,
  never feeds back into sim state, and is therefore allowed to be model-generated, cached, or
  absent entirely.

A build with **no model loaded** still produces complete, emergent dialogue (via the grammar
floor, §4) and stays byte-for-byte identical to one with a model. The model is an *enhancement*,
never load-bearing — the only determinism story that survives the literature (Thinking Machines
Lab, *Defeating Nondeterminism in LLM Inference*, He et al. 2025: even temperature-0 hosted
inference gives ~80 unique outputs per 1,000 calls; provider `seed` is best-effort). We cannot
make a model deterministic, so we **never put it in the deterministic path**.

---

## 2. The meaning layer — emergent, deterministic, whole-population

Exactly parallel to the existing stack ([ai-architecture-roadmap]: *IAUS picks the goal, GOAP
plans the sequence*). Here **IAUS scores conversational intents** the same way it scores
physical goals — reusing [`ai::Consideration` / `ai::score`](../agents/src/ai.rs) verbatim.

**An intent is authored data**, in the `RON + Registry` idiom of `goals.ron`/`norms.ron`/
`beats.ron`:

```ron
(
  id: "wound_an_old_grievance",
  // what kind of speech act (Searle illocutionary class) — for the surface generator & effects
  act: Accuse,
  // scored like a goal: the IAUS compensated product of these considerations, vs THIS listener
  appeal: [
     (input: GrievanceAgainst(Listener), curve: Linear(m: 1.0, b: 0.0)),
     (input: Trait("vengeance"),         curve: Power(exp: 1.5)),
     (input: Mood("anger"),              curve: Linear(m: 0.8, b: 0.1)),
     (input: Sanction,                   curve: Linear(m: -1.0, b: 1.0)), // a no-kill/peace taboo cools it
  ],
  // the deterministic social consequence — the SAME Effect vocabulary as beats.rs
  effects: [ Turn(who: Listener, toward: Speaker, delta: -0.15), Stir(who: Speaker, mood: "anger", delta: 0.1) ],
)
```

**Why this is genuinely emergent (not a dialogue tree).** An NPC means to wound *because of who
it is and what happened* — its `vengeance` trait, its open `Grievance` against this listener, its
present `anger`, weighed against the deontic `Sanction` of the prevailing norms — never because a
designer wrote that branch. The intent that fires is a product of the agent's whole simulated
state, exactly as a GOAP plan is composed, not scripted.

**Scoring reuses `ai::score`** (the IAUS "compensation" product already in the codebase). The
`feature` closure maps each `Input` to a number for *this speaker → this listener*. The existing
`Input` enum (`Deficit`, `Trait(id)`, `Mood(id)`, `Sanction`) gains a few **listener-relative**
axes — a clean, in-grain extension, the same way `Trait`/`Mood`/`Sanction` were added:

- `OpinionOf(Listener)` — `Opinion::of(listener)` (warmth → confide/plead; cold → accuse/deflect).
- `GrievanceAgainst(Listener)` — `1.0` if the speaker holds a `Grievance` against the listener.
- `SharedHistory(Listener)` — salience mass of memories the two share (see §3).
- `Prominence` — the director's manufactured prominence of the speaker/listener (§5).
- `DirectorPush(intent)` — pressure injected by Γ when it wants this said (§5).

**Content determination (which facts, which referent) is also emergent.** Once an intent is
chosen, the *propositional content* — which grievance, which remembered event, which third party
— is selected by **salience over the agent's real relational + memory state** (§3), never from a
list. This is the "plan → realize" split (CICERO; Reiter & Dale): decide *what to express*
symbolically and grounded, so the surface generator can only phrase it, never invent it.

The output of this layer per exchange is an **utterance plan**: `{speaker, listener, act,
referent(s), affect}` — fully grounded, deterministic, and the thing whose social effects already
applied in the tick.

---

## 3. Memory — a cheap symbolic log over authoritative state (no ML)

The biggest leverage in this project: shipped systems (Mantella, Herika, AI Town) re-derive
memory by summarizing chat transcripts because they have no authoritative history. **This sim
does.** Don't build a vector store over invented text — *rank the real thing*.

- **Episodic log:** a small per-NPC ring of salient events, written from the existing
  [`events::appraise`](../agents/src/events.rs) path (a death witnessed, a famine survived, an oath
  broken, a beat Γ staged). Each record: `{summary_key, tick, last_recalled, importance,
  parties: SmallVec<Entity>, register}`. `summary_key` is a structured handle (event kind + parties
  + tick), *not* prose — the surface layer renders it.
- **Importance is already computed** — reuse the appraisal **poignancy**, the **`Grievance`
  weight**, and the director's **`prominence`**. No LLM importance pass (cf. Generative Agents,
  Park et al. UIST 2023, which needs one).
- **Relevance is symbolic, not embedding-based** — cheap arithmetic, no model: shared parties
  (set membership with the listener), register/tag overlap with the current intent, recency
  (a tick subtraction). This is all RimWorld's beloved social memory is.
- **Mood-congruent channel** (Emotional RAG, Han et al. 2024): bias retrieval by the speaker's
  mood, so an angry speaker surfaces grievances and a fond one surfaces shared joys. Mood-congruent
  recall is most of what makes remembered dialogue feel *human* rather than indexed.
- **Forgetting:** Ebbinghaus `R = exp(−t / S)`, `S += 1` and `t = 0` on each recall (MemoryBank,
  2023) — trivia fades, the wound you keep reopening stays sharp.
- **Per-participant scoping** (Mantella's hard-won lesson — don't reference a conversation you
  weren't in): an NPC recalls only what it was present for. The [`Known`](../agents/src/people.rs)
  component already enforces exactly this discipline for *places*; extend it to *events*.

Retrieval score per memory = `recency × importance × relevance(symbolic) × mood_congruence`,
top-k, deterministic. The selected memories are the referents the meaning layer (§2) hands to
the surface layer.

---

## 4. The surface layer — generated, never a phrasebook

Two generators, both driven by the *same* emergent utterance plan (§2). The human authors
**rules and lexicon, or trusts a model's weights — never lines.**

### 4a. Generative grammar (deterministic, whole-population, the floor)

A compositional micro-grammar in the Dwarf Fortress / Caves of Qud lineage (and the
[goal-grammar-direction] the project already flagged — Inform-style verb/target surface, Ceptre
operators). Authoring is a **lexicon tagged by register/affect/faction** plus **composition
rules**, not a line library. The utterance plan is *realized* by composing through the grammar,
seeded by the deterministic `DialogueRng`:

```
realize({act: Accuse, referent: grievance(brother, starved, long_winter), affect: cold_anger})
   → compose: [stance|cold] [address|listener] [predicate|grievance] [time|event] [vow|vengeance]
   → "You let my brother starve through the long winter — and you call yourself my kin."
```

The specific sentence was **never written by a human**; it was *composed* from the agent's state
through general rules — emergent in the same sense a GOAP plan is. Deterministic, byte-identical,
costs a string build, runs for the whole population in the tick. This is the **complete, shippable
baseline** and the always-available fallback.

### 4b. SLM realizer (optional, foreground, one conversation at a time)

When the player is actually in a focused conversation with **one** NPC, a small on-device model
*generates* the line from the NPC's **serialized inner state** — identity, motive profile, current
mood, standing toward the player, the specific shared history — not from a template. Genuinely
emergent surface, no authored lines. SLMs at this scale ship today (inZOI "Smart Zoi": a **0.5B**
Mistral-NeMo-Minitron, fully on-device, ~1 GB; *Mecha BREAK*: Nemotron-Mini-4B local). Constraints
that keep it reasonable on a desktop:

- **One conversation at a time** — only the NPC in focus; the other N stay on the grammar floor.
- **Off the deterministic tick** — the intent already fired and already mutated sim state; the
  model only colors the *words*, so the simulation stays seeded and byte-identical regardless.
- **It generates from a grounded plan, it doesn't decide.** The prompt is the utterance plan +
  knowledge-gated context; the model phrases *this NPC saying this*, bounded by what it's given.
- **Knowledge-gated = structural anti-hallucination.** Feed only what the NPC actually knows
  (its `Known`, its memory, its faction lore). It can't break lore it was never given — beating
  the #1 lore-break cause, *query sparsity* (RoleBreak, 2024).
- **Optional, like a graphics setting**, behind a toggle. No model / no VRAM headroom → the
  grammar floor, losing nothing structural.
- **Seed-cached by canonical state hash** so a replay says the same words (reproducible *display*,
  not just sim).
- **Model:** 0.5–1.5B int4 (SmolLM2-1.7B, Qwen2.5-1.5B-Instruct, Llama-3.2-1B, 0.5B Nemotron-Mini),
  ~0.5–1.5 GB, single short streamed generation; CPU-side is viable (~1–2 s for one focused line,
  zero VRAM contention with the renderer). **Runtime:** in-ecosystem — `candle` (pure-Rust) or
  `llama.cpp` bindings / `mistral.rs`. No Python, no service, loads once.

### The honest grain-of-emergence note

Words don't emerge *ex nihilo*. The emergence has a **grain**: a human authors either a
generative grammar (lexicon + rules — §4a) or trusts a model's weights (§4b). What is *not* done
is selecting from a phrasebook. Both are emergent in the sense that matters — the specific thing
this NPC says was *produced* from its state, never written by a person — the same way a GOAP plan
is composed, not scripted. "Emergent surface" means *generated by a grammar or a model*, not
*appears from nothing*.

---

## 5. The narrative-director hook

This is where it pays off thematically. Extend the director's `Effect` set (`beats.rs`) with
**`Voice(who: Role, intent: String)`** so Γ's manipulations are *heard*: `a_friend_turns` doesn't
only flip an `Opinion` — the friend *speaks the renunciation*, in their own idiolect, grounded in
the manufactured grievance. The director's **`prominence`** decides which encounters earn the
expensive (SLM, cached, knowledge-checked) treatment vs. a one-line grammar bark, and a new
`DirectorPush` intent axis (§2) lets Γ raise the appeal of an intent it wants surfaced *without*
puppeting (the agent still must have the state to plausibly say it — the deniability rule). The
**felt manipulation** the v2 director is built toward finally has a delivery channel: the player
hears the betrayal in a voice they were groomed to trust.

---

## 6. Determinism, cost, and the reasonable boundary

- **Simulation determinism is untouched.** Which intent fires and its social consequence
  (opinion/mood/grievance deltas) are decided by the seeded meaning layer (§2). Only the *rendered
  words* are model-generated, and those are cosmetic and seed-cacheable. The sim never blocks on or
  depends on a model.
- **Emergence of meaning is universal; emergence of rendered words is foreground-only.** Background
  NPCs' dialogue *meaning* still emerges (the sim computes it) and surfaces as social events in a
  log / as consequences (RimWorld's social log is emergent and beloved with zero prose generation),
  or gets the grammar/SLM treatment only when the player looks. Firing a model for 200 NPCs every
  tick is the unreasonable thing we explicitly *don't* do.
- **Off by default**, like the director — a dialogue-free world is byte-identical to one before
  this layer.

---

## 7. Consistency & evaluation (mostly offline / cheap)

- **Knowledge gating** (§4b) is the structural defense. On top, an optional offline **NLI check**
  (premise = serialized state, hypothesis = candidate line) rejects contradictions at bake time —
  free at runtime. Anti-repetition (distinct-n) since looping is the most obvious non-human tell.
- **Trait expression check** (InCharacter, ACL 2024): periodically interview an NPC and back-map
  answers to its designed motive profile — self-report questionnaires *fail* for role-play agents;
  behavioral interview scores ~80%. A repeatable "is this NPC actually the vengeful/pious one I
  simulated?" gate.

---

## 8. Build plan

1. **v0 — meaning layer + generative grammar (no ML, deterministic, complete & shippable).**
   The conversational-intent RON layer + the listener-relative `Input` axes; intent scoring via
   `ai::score`; intent `effects` (reusing the beat `Effect` vocabulary); the episodic-memory log +
   symbolic ranked retrieval; the generative grammar surface; the `Voice` director effect. Tested
   the way the rest of the sim is (seeded, deterministic, off-by-default). **Most of the humanity
   comes from here.**
2. **v1 — optional SLM realizer.** A `candle`-based 0.5–1.5B generator behind a toggle, foreground
   conversation only, off-tick, knowledge-gated, seed-cached, grammar fallback. Optional offline
   NLI bake check.
3. **v2 — player input understanding.** When there's an interactive player: classify player
   utterances into a player *intent* (same intent vocabulary as NPCs — the player is an NPC avatar
   with the same verbs), closing the loop with the director.

---

## 9. Open decisions / deferred

- **Grammar depth vs. SLM reliance.** How rich the §4a grammar should be (it's the floor and the
  fallback) before leaning on §4b. Recommendation: a genuinely good grammar, so the SLM is polish,
  not a crutch.
- **CPU vs. GPU for the SLM**, and whether an external/cloud model is ever acceptable at runtime
  (it would be the project's first non-deterministic, outward-facing dependency — recommend not).
- **VAD mood coordinates** (PELD, Wen et al. TOIS 2024: personality scales how hard events move
  mood, via Big-Five→VAD maps). Optional: makes mood→tone principled, but adds to the mood model;
  named-mood descriptors may suffice.
- **Player dialogue as symbolic acts** (same intent verbs, deterministic) vs. free-text parsed —
  the former is more on-theme and is what v2 assumes.
- The honest open frontier: emergent *surface* for the *whole population* in real time needs a
  model per NPC (unreasonable) — so ambient emergence stays at the *meaning* level, read through
  consequences/log, not per-NPC prose. Accepted scope, not a bug.

---

## 10. Research grounding (selected)

Meaning-as-action / plan→realize: CICERO (Meta, *Science* 2022); Reiter & Dale NLG (2000);
Versu/Prom Week social practices (Evans & Short 2013–14). Memory: Generative Agents (Park et al.,
UIST 2023, recency×importance×relevance); MemoryBank/Ebbinghaus (2023); Emotional RAG (Han et al.
2024); Mantella per-participant scoping. Surface generation: Dwarf Fortress / Caves of Qud
generative grammars; Ashby et al. (CHI 2023, "LLM fills a grammar bank offline"). On-device SLMs:
inZOI Smart Zoi (0.5B Mistral-NeMo-Minitron on-device); *Mecha BREAK* (Nemotron-Mini-4B local);
NVIDIA ACE tiers. Conditioning/consistency: numbers→words binning (Humanoid Agents, EMNLP-demo
2023; Inworld); knowledge gating vs. query-sparsity (RoleBreak 2024); InCharacter behavioral trait
eval (ACL 2024); PELD personality-scaled mood (TOIS 2024). Determinism: Thinking Machines Lab,
*Defeating Nondeterminism in LLM Inference* (He et al. 2025).

---

## 11. What shipped (build notes, 2026-06)

The §8 plan was executed in full. Specifics:

- **The meaning layer reuses the IAUS scorer.** `ai::Input` gained the listener-relative
  axes `OpinionOf` / `GrievanceAgainst` / `SharedHistory` / `Prominence` (goals return `0`
  for them). A conversational `Intent` (`assets/data/intents.ron`, ~14 of them) is authored in the
  `ConsiderationDef` idiom and scored by **`ai::score` verbatim** — *speaking is acting*.
  `Move`s (Turn/Stir/Sway/Grudge over Speaker|Listener) are the canon social consequence,
  applied in the tick. Which intent fires is emergent (vengeful→threaten, warm→confide,
  grieving→mourn, forgiving→reconcile), gated by an `appeal_floor` (the conversational
  impact floor — speak only when you have something to say).
- **Memory is a cheap symbolic log** in the `Dialogue` resource (no embeddings): per-soul
  `MemRecord`s with importance + Ebbinghaus `strength` (rises on recall, fades otherwise);
  `SharedHistory` reads it; the referent is the most salient memory of the listener (a
  standing `Grievance` is the loudest). Per-soul, written for both parties.
- **The surface is a generative grammar** (`assets/data/grammar.ron`): a Tracery-style
  symbol→productions map with `#recursion#` and `#speaker#`/`#listener#`/`#referent#`
  slots, keyed `act/affect` (mood-coloured) with a bare-`act` fallback, seeded by the
  dialogue RNG. Stable name epithets per entity. Composed, never a phrasebook.
- **The SLM seam (v1)** is out of band and host-pluggable: a `TextGen` trait, `build_prompt`
  (the character card assembled from the grounded utterance — numbers already words),
  `state_hash` (FNV over the *meaning*), and `SlmRealizer<G>` (cache → generate → guard →
  grammar fallback). Tested with a deterministic fake generator. **`candle` is NOT bundled**
  — a multi-GB model + FFI is a host-app concern, kept out of the byte-identical sim crate;
  the adapter is a localized drop-in (implement `TextGen` over candle/llama.cpp).
- **The player-avatar path (v2, corrected):** this is a role-playing game, so the *player*
  is the avatar's mind -- the avatar carries no traits, mood, or opinion, and the sim does
  NOT score what it "wants" to say. `Simulation::player_intents()` returns the **unscored
  full repertoire** (`dialogue::repertoire`); the player chooses the meaning. `player_say` /
  `player_talk` enact it through `dialogue::perform` (made tolerant of an attribute-less
  speaker: absent components read as neutral), landing the consequence on the listener. The
  addressed NPC answers from its own state via `dialogue::reply` (the scored path -- an NPC
  IS its own mind). `player_talk(listener, id)` is one turn-based action: it says, hears the
  reply, and steps one tick. The scored `available()` is now the NPC-reply path only.
  Free-text NLU (utterance -> intent) is the deferred, model-dependent extension.
- **Director hook:** `Effect::Voice { who, intent }` on a beat forces `who` to speak the
  intent at the protagonist; `director_step` pushes it to the `Dialogue` forced queue (a
  no-op when dialogue is asleep), and `converse` drains it. Wired into `a_friend_turns`,
  `the_knife_in_the_dark`, `betrayed_at_the_summit` — Γ's betrayals are now *heard*.
- **Determinism & off-by-default preserved:** all state in the `Dialogue` resource (no NPC
  component → dialogue-free worlds byte-identical), seeded `DialogueRng`, `converse`
  early-returns when disabled. Director tests stay green (`Voice` is inert without dialogue;
  it adds no suffering/brightness). New tests: sleeps-unless-woken, reflects-the-social-
  state, player-avatar-speaks, director-voices-its-betrayals, deterministic; plus the
  module's grammar/seam/validation tests.
- **Still deferred** (unchanged from §9): a richer grammar; the live `candle` adapter +
  on-device model; VAD mood coordinates; deeper event-memory hooks from `events::appraise`
  (currently memory is written from conversations + grudges); free-text player NLU; the
  *felt* manipulation at scale (needs a real interactive player).

[ai-architecture-roadmap]: ../agents/src/ai.rs
[goal-grammar-direction]: ./narrative_director.md
