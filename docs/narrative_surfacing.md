# Surfacing the Narrative — making the director *felt* through play

> **Status: design / pre-implementation (2026-06).** A working spec, not a contract. It
> records the problem, the one inviolable rule, the keystone idea (embrace the SLM's
> unreliability as fiction), the four player-facing channels, and a phased build plan.
> Read alongside `docs/narrative_director.md` / `_v2.md` (what Γ stages) and
> `docs/dialogue.md` (the meaning/surface split this builds on). Type and accessor names
> are the real ones in `agent_core` / `agents` at time of writing.

---

## 0. One-paragraph summary

Γ already stages a season of drama — betrayals, vengeance, wars, the beloved falling — onto
**specific named souls**, and `dialogue.rs` already lets it put charged lines in their mouths
(`Effect::Voice`). But none of it reaches the player: in `app` the director is left **asleep**,
and even awake, its output lands in structures the front-end never reads (`Director::cadence`,
`::log`, `::prominence`, and `forced` utterances in the dialogue log). The fix is **not** a plot
HUD — the design forbids personifying Γ or showing a meter. The fix is to deliver the plot through
the verbs the avatar already has (**walk, look, talk, search, travel, and act on its places**)
across diegetic channels — recurring characters who remember, gossip that spreads, scenes you travel
to and *witness* on **living POIs**, and a fallible ledger of what you've been told — bound together
by one keystone: the
spoken/heard world is an **unreliable surface over a true hidden sim**, and we *embrace* the SLM's
hallucination as the dream-purgatory's veil rather than fighting it.

---

## 1. The problem & the diagnosis

The complaint: *"there is next to no narration of plot… I imagine that is because I cannot interact
with it,"* with an explicit failure mode — *"the player must read a large block of text."*

The substance of the plot **already happens**. What's missing is the interface:

- In `app`, `main.rs` leaves **the director asleep** (`Setup::director` unset) so exploration stays
  whole. So for most sessions Γ stages *nothing*.
- When Γ *is* awake, its output is human-readable but unread by the front-end. The `story_log`
  example proves the material is fully reconstructable from `director_cadence()`,
  `director_beats_fired()`, and `dialogue_log()` filtered to `forced == true` — it just lives only
  in a dev tool.
- Plot has **no diegetic surface**. Grudges form, opinions flip, wars are declared — and the player,
  walking the world, never hears or sees any of it.

The real missing thing is not "edit the director" (that is the §5.B endgame tool — a separate,
later design). It is that **the plot must arrive through ordinary play**.

---

## 2. The one inviolable rule: surface the *fiction*, never the *apparatus*

`narrative_director.md` §8 is binding here: **no morality meter, no scorekeeping, hide Γ's hand,
and "make suffering specific, not aggregate — felt weight comes from one named NPC the player
knows."** This disqualifies the obvious build — a panel showing *"Betrayal thread · phase Climax ·
heat 0.8,"* a prominence readout, a tension gauge, the cadence timeline. Those surface the
**machinery**; they personify the thing the whole game depends on staying hidden, and they convert
a moral situation into a number to optimize.

So, the rule for every player-facing element below:

> **The player only ever meets people with stories and places where things happened. They never see
> a thread, a register, a phase, a prominence number, or a `gratuitous/staged` total.**

The same data has **two audiences**:

| Audience | Sees | Where |
|---|---|---|
| **You (the author/debugger)** | the machinery — threads, registers, phases, prominence, cadence, `staged(t)` | a **debug overlay** (folds into the profiling/debug tooling already added), gated behind a dev flag |
| **The player** | the fiction — named souls, their voiced situations, rumors, witnessable scenes | the four channels in §4 |

The debug overlay is not just allowed but *useful* — it's how you'll tune Γ. It simply must never
be the player's window onto the plot.

---

## 3. The keystone: the veil — embrace SLM hallucination as fiction

`dialogue.md` §1 already splits **meaning** (a deterministic sim object — what is *true*) from
**surface** (the generated words — what is *said*). Hallucination can only ever corrupt the
surface. That is not a liability to clamp; in a **Gnostic dream-purgatory** it is the exact texture
we want. The spoken world is the Demiurge's veil; **gnosis** is piercing it to the true sim
beneath. Rumor is *supposed* to distort; a dream is *supposed* to misremember.

So we make unreliability a **first-class, deterministic sim quantity** and let the SLM confabulate
*in proportion to it*.

### 3.1 Fidelity — a deterministic dial on truth

Every piece of narrative knowledge the player can hold (a rumor, a remembered claim, a thing an NPC
told them) carries a scalar **`fidelity ∈ [0,1]`**:

- `1.0` — **witnessed**: the avatar saw it happen. Ground truth.
- high — heard first-hand from a participant.
- low — third-hand gossip, many hops from the event, gone stale.

Fidelity **decays deterministically** — each gossip hop, each unit of map distance from where the
beat fired, and each day of age multiplies it down — computed in a **dedicated derived RNG stream**
(`rumor` = run-seed XOR a distinct constant, per the workspace's separate-streams invariant). The
*structured* rumor (which beat, which souls, which place) is sim state and exact; **fidelity is the
only thing that erodes**, and it erodes by arithmetic, not by the model.

### 3.2 Confabulation gated by fidelity

When a rumor or remembered claim is *rendered* to words, fidelity sets the surface generator's
**latitude**:

- **High fidelity →** the prompt is tightly grounded (`dialogue.rs` knowledge-gating, §4b there):
  the words say only what's true. Grammar floor picks precise templates.
- **Low fidelity →** the prompt is deliberately *loosened* — under-specified, even instructed to
  speculate, hedge, or embellish ("they say… or maybe it was his brother, I forget"). The SLM
  **confabulates**, and the confabulation is **diegetically correct**: a third-hand rumor *should*
  have invented detail. The grammar floor mirrors this with vaguer, hedged templates ("someone
  did something to the steward, they say").

This is the inversion the brief asks for: **we don't fight hallucination; we commission it, scaled
by how garbled the rumor already is.** The model's weakness becomes the dream's voice.

### 3.3 Why determinism is untouched (byte-identical guarantee)

The invariant from `dialogue.md` §1 holds verbatim:

- **`fidelity` and the structured rumor are sim state** — seeded, in the `rumor` stream, part of
  the deterministic tick. Same seed → same fidelity everywhere.
- **The SLM is still off-tick, cosmetic, cached** by the meaning hash (now including fidelity), and
  **never feeds back**. With the model off, the **grammar floor renders the same fidelity
  deterministically** (precise vs. hedged templates). A build with the model is byte-identical to
  one without — only the *vividness* of the hedging differs.
- **The player's beliefs are player-side.** When a rumor reaches the avatar, the avatar's recorded
  fidelity/claim is *display + player-knowledge* state. It must **never feed back** into any NPC or
  director decision (exactly as the SLM surface never does), so it cannot perturb the sim.

### 3.4 Why it is thematically exact

The discovery layer already runs an **Outer-Wilds knowledge loop** for places (lore-gated, no
dice). The veil extends that loop to the **social/narrative** layer: the player hears a distorted
claim (low fidelity), is pulled to **witness** it (§4.3), and witnessing snaps it to truth
(`fidelity → 1`). The Archons' world *lies*; truth is earned by going and seeing. The unreliable
narrator is not a concession to a weak model — it is the medium of a purgatory where nothing spoken
can be fully trusted.

---

## 4. The channels (and the stage they play on)

The four channels (§§4.1–4.4) deliver the same emergent material (`cadence`, `prominence`, `forced`
Voice lines); §4.5 is the **living POI** that is the stage they play on. All obey §2 (fiction only)
and §3 (fidelity-graded), and they are complementary, not alternatives — §5 shows the loop they
form.

### 4.1 Nemesis — recurring characters who remember
*Anchors: Shadow of Mordor's Nemesis system; Wildermyth's named heroes.*

Highest leverage, because it reuses `Director::prominence` + `Effect::Voice` almost directly.

- **Prominent souls become legible in the world.** A soul with high `prominence` gets a *sticky
  name* and an **earned epithet** reflecting its arc's `register` — "the Betrayed," "the Pretender,"
  "the Bereaved." (Dialogue already keeps "stable name epithets per entity" for the grammar; this
  promotes that to an arc-aware, player-visible honorific.)
- **The story arrives in their own voice.** When the avatar talks to a thread's `lead` or `other`
  mid-arc, the conversation **opens with their situation** as a `forced`/high-`appeal` Voice line —
  the man tells you, bitterly, that he was betrayed. You don't read it; you hear it. (`DirectorPush`
  + the `Prominence` IAUS axis already exist to raise this intent's appeal without puppeting.)
- **History accrues on a face.** Meet him again after the thread advances and he references what
  changed — driven by `SharedHistory` + the memory log. Recurrence *is* the narration.

This is "specific, not aggregate" made mechanical, and it hides the hand completely — he is just a
person with a story.

### 4.2 Gossip — the world is talking
*Anchors: RimWorld's short named letters; Dwarf Fortress / Crusader Kings character-centric news.*

The connective tissue, and where the veil (§3) lives most naturally.

- When a beat fires, it seeds a **structured rumor** — `{ beat, cast: souls, place, fidelity:
  high }` — into a propagation system. Rumors **spread NPC-to-NPC** during `converse` (a new
  conversational intent, *gossip*, scored by the IAUS like any other), **losing fidelity each hop**
  (§3.1).
- The avatar **overhears** rumors from NPCs it passes or talks to: a bite-sized, in-character line
  whose vagueness/embellishment is set by the rumor's current fidelity (§3.2). Never a wall of
  text — one hedged sentence.
- **Extend the existing rumor type.** `PlayerKnowledge { lore, rumors: Vec<Rumor> }` and `Rumor {
  subject: String, target: Option<Coord> }` already carry *place* rumors for the discovery loop.
  Add a *narrative* rumor variant carrying the cast + beat + fidelity, so a heard rumor becomes a
  **thread the player can pull toward** spatially (its `target` is where the beat fired).

This is the channel that makes **distant** drama legible — important given Distance-LOD: drama among
LOD-dormant far souls can't be encountered face-to-face, but its rumor still reaches you.

### 4.3 Witnessable scenes — beats as places you travel to and watch
*Anchors: Return of the Obra Dinn's show-don't-tell; Crusader Kings map events.*

The **gnosis verb** — how the player converts a low-fidelity rumor into truth.

- A fired beat has a `place` and a `cast`. Drop a **transient, subtle** world/minimap marker there —
  a gathering, a smoke column, a funeral procession — *not* a quest marker (§8: never signpost). "A
  commotion to the east," at most.
- The player *chooses* to travel and **witness**. The scene plays as **NPC action + a few `forced`
  Voice lines staged in 3D** — not prose. (The conversation UI and minimap/Map you just rebuilt are
  the delivery surfaces; the LOD radius already governs what's live near the avatar.)
- **Witnessing sets `fidelity → 1`** for that beat in the player's knowledge — you saw it. This is
  the loop's payoff: the only way past the veil is to be there.

### 4.4 The ledger — what you've been *told*
*Anchors: Hades' Codex; Wildermyth's "story so far"; The Sims memories.*

The pull-not-push catch-up surface — and explicitly **not** a director dashboard.

- An **acquaintance page** listing only souls the **avatar has actually met** (diegetic epistemics —
  gated by `Known`/encounter, never a god-view), each with **one line of plain in-world state** in
  the characters' terms ("Aelric — once your host; now grieving his brother") and the few moments
  you personally witnessed or were told.
- **Each recorded claim shows its fidelity as fiction, not a number** — "you saw this" vs. "you
  heard this, third-hand." Two low-fidelity rumors may **contradict**; the ledger preserves the
  contradiction rather than resolving it.
- Those contradictions are the **curiosity-gap hook** (§8.2, "anomaly as the hook"): the player
  notices the accounts don't line up and goes to *witness* the truth — driving §4.3. The ledger is
  the avatar's **fallible memory**, which is exactly why it can never read as an authorial overview.

It also absorbs the temptation that §2 forbids: the curious player who *would* have wanted a thread
panel gets a legitimate, in-fiction place to look instead.

### 4.5 POIs as living stages — the ground the loop stands on
*Anchors: Octopath Traveler / HD-2D (the deferred ideal); Stardew / RimWorld lived-in places; Kenshi & Dwarf Fortress sites that remember.*

Today a POI offers the player exactly one verb — **look** — yet the world's ~80 authored feature
kinds (Community / Court / Ruin / Wilderness) are *already socially alive in the sim*: NPCs discover
them, route to them, **use** their affordances (forage, shelter, pray, apprentice, mine, drink), and
deplete/regenerate them (`WorldAffordances`, `regen_affordances`, the `uses` counter). The aliveness
exists; it is just **invisible and unusable** to the player. So "make POIs feel alive" is mostly the
same *surfacing* problem as the plot — plus restoring the verbs the player is missing. Four
dimensions, in leverage order:

**A. Legible — show the life already there.** At (or inspecting) a POI, surface *who is present and
what they're doing*, in plain fiction: "Two villagers haggling at the market; a pilgrim kneeling at
the shrine." Pure surfacing of `PlayerView.nearby` + each soul's current `Step`/plan — no new sim,
and the biggest single "alive" win. (Per §2: fiction, never "capacity 3/40" — say "the oasis is
nearly drunk dry.")

**B. Usable — give the player the verbs NPCs already have.** Add one **Use/Interact** verb (a new
`ActionKind`, gated by `enabled()` on an available, discovered affordance at the avatar's tile,
dispatched through a new `player_use_affordance` sim verb mirroring `search()`). The player forages
the grove, shelters in the cave, **apprentices** at a hall to learn a skill — a real participant in
the affordance economy, not a tourist. (Player depletion of shared sites perturbs NPC planning —
consistent with the existing player-as-actor model, where travel / search / recruit already move the
world.)

**C. Historied — the place remembers, and the plot is staged here.** A POI is the natural **anchor
for the narrative loop**: a beat fires *at* a place (§4.3 witnessing happens at a POI; §4.2 rumors
carry its `target`). A POI accrues a visible past — the throne that was seized, the shrine where the
beloved fell — so a place the player knows *remembers* what Γ did to it (persistence-as-the-tell,
§8). This is where POI-interaction and narrative-surfacing become **one feature**: the POI is the
physical node where *hear → seek → witness → know* completes.

**D. Reactive — the place visibly changes with the sim.** A drained oasis, a bustling vs. emptied
market, a sacked settlement — drive feature art + the inspect surface from affordance depletion and
faction state, so the world reads as lived-in and consequential, not static set-dressing.

**The Gnostic payoff — POIs are where gnosis is earned.** The Court / Ruin vocabulary is already
loaded with theme (`archon_throne`, `gnostic_conventicle` → *the counter-cosmogony*, `well_of_lethe`
→ drink to *forget*, `gate_of_the_seven` → *the way beyond*), and these features already `reveal`
lore. So POI interaction is the **knowledge engine** that pierces the veil (§3): you learn the
counter-cosmogony *at* the conventicle, you risk *forgetting* at Lethe. This wants the affordance
**effect vocabulary to grow** past survival/economy (`Relieve` / `Yield` / `Teach`) to carry
**narrative/knowledge effects** — reveal-lore, hear-a-rumor (a seer's hint), forget-lore,
shift-mood/forgiveness, petition-a-faction — so a Court or Ruin POI is narratively alive, not a
vending machine.

**The deferred ideal (Octopath HD-2D interiors).** The dream is a procgen, enterable, HD-2D **scale
interior** explored room-by-room. That is a large build (a second spatial mode, an art pipeline,
interior procgen) and is **deferred**. The nearer-term substitute is the **threshold**: a richer
inspect/enter panel that shows a POI's *life* (present souls + activity), *history* (what happened
here), *affordances* (what you can do), and *lore* (what you might learn) — everything that makes a
place feel alive *from the doorway*, before you can step inside. A → B → C → D delivers most of
"alive" without the interior; the interior is the eventual upgrade to the threshold, not a
prerequisite for it.

---

## 5. How the channels interlock (the loop)

```
   Γ stages a beat onto named souls          (sim, deterministic — already built)
        │
        ├─►  forced Voice line                ──►  4.1 Nemesis: heard from the soul, face-to-face
        │                                              (high fidelity — first-hand)
        │
        └─►  structured rumor (fidelity hi)   ──►  4.2 Gossip: spreads NPC→NPC, fidelity decays,
                 │                                     avatar overhears a hedged/embellished claim
                 │                                     (low fidelity — the veil, §3)
                 ▼
            heard claim + place  ──►  the ledger records it as "told, uncertain"  (4.4)
                 │                         │
                 │                         └─►  contradictions accrue → curiosity gap
                 ▼
            "a commotion to the east"  ──►  4.3 player travels & WITNESSES  ──►  fidelity → 1
                                                  │
                                                  └─►  ledger reconciles to truth; the dream resolves
```

**Hear (distorted) → seek → witness (true) → know.** That is the entire narrative loop, and it is
the same Outer-Wilds knowledge loop the discovery layer already runs, now applied to *people and
events* and graded by fidelity. The plot is never *read*; it is *overheard, doubted, and
confirmed.*

The seek/witness leg lands at a **POI** (§4.5): the place a rumor's `target` points to is where you
go, see who's there, *act on* it, and learn its truth — the POI is the loop's physical ground.

---

## 6. Data & seams

**Reused as-is (no sim changes):** `Director::{prominence, threads, log, cadence}`;
`Thread{ spine, lead, other, phase, heat }`; `Cadence{ tick, beat, register, phase, lead_prominence,
collision }`; `Effect::Voice{ who, intent }` + the forced queue + `converse`; the IAUS axes
`Prominence` / `DirectorPush` / `SharedHistory` / `OpinionOf` / `GrievanceAgainst`; the memory log
(`MemRecord`, salience); `Simulation::{director_cadence, director_beats_fired, dialogue_log,
protagonist, display_name, player_talk}`; `PlayerKnowledge{ lore, rumors }`.

**New (mostly in `agent_core`, behind the existing off-by-default discipline):**

1. **`fidelity`** on narrative knowledge + the deterministic decay model (§3.1), seeded by a new
   `rumor` derived RNG stream.
2. **A narrative `Rumor` variant** carrying `{ beat, cast, place, fidelity }`, and a **gossip
   propagation system** (a new conversational *gossip* intent scored by `ai::score`; spreads in
   `converse`).
3. **Arc-aware epithets** — promote the per-entity epithet to reflect a soul's leading `register`
   when `prominence` is high.
4. **Fidelity-graded surface latitude** — thread `fidelity` into `build_prompt` (loosen at low
   fidelity) and into grammar template selection (hedged variants). Cosmetic, off-tick.
5. **A transient beat-marker** layer in `app` (place-tagged, decaying), and a **witness** action
   that snaps player-belief fidelity to truth.
6. **The acquaintance ledger** (player-side knowledge, fidelity-tagged, encounter-gated) + its
   `app` page.
7. **A dev-only debug overlay** surfacing the machinery (§2) for tuning.

**New (POI interaction, §4.5):** a player **Use / Interact** verb — `ActionKind::Use` + the
`enabled()` gate + an `action_button_click` arm + a `do_interact` helper (`app`), dispatched through
a new `player_use_affordance` sim verb mirroring `search()` (`player.rs`); `PlayerView` gains
`available_affordances` and a *present-souls + activity* readout; the affordance `EffectDef`
vocabulary grows past `Relieve` / `Yield` / `Teach` to carry **narrative/knowledge effects**
(reveal-lore, a seer's hint → rumor, forget-lore, shift-mood, petition-a-faction); `feature_art`
gains depletion/faction-driven variants. Reuses `WorldAffordances` / `AffordanceSite` /
`regen_affordances`, `Features` / `FeatureCatalog`, `FindState`, `PlayerView.nearby`,
`discover_features`.

**In `app`:** wake the director (`Setup::director`), and prefer casting threads around souls in/near
the avatar's region so the plot is *encounterable* face-to-face (gossip covers the rest).

---

## 7. Determinism & the off-switch

Every new piece obeys the workspace invariants:

- **Sim state stays deterministic & seeded.** `fidelity`, rumor propagation, and gossip intents live
  in the `rumor`/`DialogueRng` streams; same seed → byte-identical.
- **The SLM stays out of the deterministic path** (`dialogue.md` §1). Fidelity only widens its
  *latitude*; with the model off, the grammar floor renders the same fidelity deterministically.
  Model-on is byte-identical to model-off.
- **Player belief never feeds back** into NPC/director decisions (same rule as the SLM surface), so
  witnessing/ledger updates can't perturb the sim.
- **Off by default = byte-identical.** The whole layer keeps its state in its own
  resources/components and early-returns when disabled; a world without narrative-surfacing is
  bit-for-bit identical to one before it. New randomness comes only from the dedicated `rumor`
  stream — never pulled from an existing one.

---

## 8. Build plan (phased; each phase shippable & testable)

1. **Phase 0 — wake & wire (smallest visible win).** Turn on `Setup::director` in `app`; route
   `forced` Voice lines into the conversation UI so talking to a thread's `lead`/`other` opens with
   their situation. **No new data.** This alone turns "no narration" into "the people around me are
   living a story." *(Channel 4.1, minimal.)*
2. **Phase 1 — Nemesis.** Arc-aware epithets + the `Prominence`/`DirectorPush` opening line + recurrence
   via `SharedHistory`. *(Channel 4.1, full.)*
3. **Phase 2 — gossip + fidelity (the keystone).** The narrative `Rumor` variant, the gossip intent,
   deterministic fidelity decay, and fidelity-graded surface latitude (grammar first; SLM latitude
   when the `voice` feature is on). *(Channels 4.2 + 3.)*
4. **Phase 3 — witnessable scenes.** Transient beat markers, the `witness` action, `fidelity → 1` on
   sight. *(Channel 4.3, closes the loop.)*
5. **Phase 4 — the ledger.** The encounter-gated, fidelity-tagged acquaintance page; contradictions
   preserved. *(Channel 4.4.)*
6. **Phase 5 — debug overlay.** The machinery view for tuning Γ (dev flag). *(Author-only.)*

**POI track (interleaves with the above — POIs are the stage the loop stands on, §4.5):**

- **POI-A — legibility** *(pairs with Phase 0–1)*: surface present souls + their activity at a POI.
  Cheap, pure surfacing; the biggest "alive" win.
- **POI-B — the Use verb** *(independent, small)*: `ActionKind::Use` + `player_use_affordance` +
  `PlayerView.available_affordances`. The player forages / shelters / apprentices like an NPC.
- **POI-C — narrative effects & the gnosis anchor** *(pairs with Phase 2–3)*: grow the affordance
  `EffectDef` vocabulary (reveal / forget / hint / petition); make the POI the node where rumor →
  witness → know completes (the conventicle teaches the counter-cosmogony, Lethe forgets).
- **POI-D — reactivity** *(ongoing)*: feature art + inspect reflect depletion/faction state — a
  drained oasis, a sacked town.
- **POI-E — enterable interiors (deferred)**: procgen HD-2D scale interiors (Octopath). A second
  spatial mode; the eventual upgrade to the POI-A *threshold* panel, not a prerequisite for it.

Test the way the rest of the sim is tested: seeded, deterministic, off-by-default; reuse the
`story_log` reconstruction as a golden transcript. Keep the director V&V tests green (the surfacing
layer adds no suffering/brightness — like `Voice`, it is inert to `staged(t)`).

---

## 9. Open questions

- **Fidelity decay shape & gossip reach.** How fast fidelity should erode per hop/distance/day, and
  how far rumors travel, before the world feels either omniscient or amnesiac. Calibration.
- **Contradiction generation.** Do low-fidelity rumors merely *hedge*, or do they actively *mutate*
  structured content (subject drift "the captain" → "a captain")? Mutation is richer but must stay
  deterministic (compute it in the `rumor` stream, never in the SLM).
- **Encounterability vs. LOD.** Should Γ bias thread-casting toward the avatar's region, or lean
  entirely on gossip to carry distant drama? Couples directly to the Distance-LOD radius.
- **Witness staging.** How much of a beat can be *re-played* as 3D action after it has already fired
  in the sim, vs. shown as aftermath (the funeral, not the murder). Aftermath is cheaper and often
  more Obra-Dinn.
- **Epithet authoring.** Generated from register, or a small authored lexicon per register? (Mirrors
  the grammar-depth question in `dialogue.md` §9.)
- **Affordance effect vocabulary (POIs).** How far to grow `EffectDef` toward knowledge/social
  effects vs. keep affordances utilitarian and carry narrative purely through beats/dialogue. Lethe
  forgetting and the conventicle teaching are the test cases.
- **Player vs. NPC draw on shared sites.** Player affordance use depletes sites the populace relies
  on (consistent with player-as-actor) — welcome participation, or cap the player's draw so it can't
  starve them?
- **Forgetting as a one-way door.** Losing gnosis at the well of Lethe is thematically exact —
  irreversible by design — but needs a clear diegetic warning so it reads as a *choice*, not a trap.
- **The Octopath interior — scope & timing.** A full second spatial mode (interior procgen + art
  pipeline) is the largest deferred item; decide if/when the *threshold* panel (POI-A) suffices.

---

## 10. Implementation status (2026-06)

Built so far on branch `narrative-surfacing`, in milestone order:

- **Phase 0 + 1 — director woken & surfaced.** Director on by default in `app`
  (`ACHLYDESA_NODIRECTOR` disables); **avatar-proximity casting bias** (avatar-gated → headless
  byte-identical, all 7 director V&V tests pass). Arc-aware **epithets** ("the Betrayed") +
  **situational openers**, and a soul's recent forced `Voice` line, open a conversation on its story.
- **POI-A — legibility.** The inspect read-out lists who is present on a tile and what each is doing.
- **POI-B + the crafts bridge.** A **Use** verb (E / tray button) engages a POI's affordance; the
  avatar now carries an `Inventory` satchel and economy `Skills`, so `Yield`/`Teach` sites genuinely
  **gather goods and teach callings** — the same effects an NPC's `Step::Use` gets — shown on the
  Inventory tab. Relief sites refresh the avatar's vitals.
- **Phase 2 + 2b — gossip + fidelity (the veil, with teeth).** Each beat seeds a rumour into its
  witnesses (fidelity 1.0); the new `gossip` module spreads it **soul-to-soul each tick, decaying a
  hop at a time** and shedding the counterpart once worn thin — deterministic, no randomness,
  order-independent, off until the director seeds. `Simulation::overheard` renders the sharpest
  rumour a *nearby soul actually holds*, by that copy's fidelity; a conversation opens on it. So the
  garbling is how far the telephone game travelled, not merely the player's distance.
- **Phase 3 (first cut) — drama markers.** `Simulation::drama_marks` exposes the recent drama the
  avatar can sense (by the same fidelity); the minimap and Map tab draw a crimson pip at each (even
  over fog) — "a commotion to the east" — and arriving makes the gossip sharp by proximity (the
  chosen *markers + sharp-on-arrival* shape, not staged scenes).
- **Phase 4 (first cut) — the ledger.** The Journal tab lists the souls the avatar has met (recorded
  when a conversation opens) with where each one's story stands now; a soul since dead is remembered
  as gone. Player-side memory; never feeds the sim.

**Deferred / next:** Phase 2c (structured subject-drift *mutation* of rumours, and SLM-graded
confabulation at low fidelity, out-of-band when the voice model is on); drama-marker refresh while
stationary; the ledger's per-soul **rumour history + contradictions** (the curiosity-gap hook); the
debug overlay; POI-C narrative affordance effects; POI-D reactivity; POI-E interiors; trading the
avatar's gathered goods.

All determinism / off-by-default invariants hold; the only standing test failures are 3 pre-existing
ones unrelated to this work.
