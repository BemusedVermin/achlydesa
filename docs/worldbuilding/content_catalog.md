# The Waxen World — Content Authoring Catalog

> The corpus worklist for the v2 text conversion. It enumerates the **fragment/tell library** that
> `docs/prose_generation.md` calls "the product" but does not itemise, written in the voice of
> `docs/worldbuilding/style_guide.md` and the canon of `waxen_world.md`. Built to be worked
> **offline** — every surface you can author into is here with its schema, a target, and worked
> examples in voice; you don't need to read the code.
>
> **This catalog is a companion to the two v2 design docs — read them first; they are the canon for
> *how* prose is made, and this catalog must not contradict them:**
> - **`docs/prose_generation.md`** — the no-LLM, never-false generative-grammar + Wolfean-tells pipeline.
> - **`docs/text_interface.md`** — the terminal (ratatui) parser front-end the prose renders into.
>
> **Do not edit those two files.** This catalog and the other two `worldbuilding/` docs sit beside
> them and fill the worldbuilding/corpus they keep saying is "forthcoming."

---

## 0. The one correction that reframes everything

**You do not hand-write a description per location, NPC, or item.** That is ruled out by design
(`prose_generation.md` §"The problem": the fact-space is emergent and open; a hand-authored passage
per situation does not scale). Instead, **prose is generated from sim facts** by a guarded grammar
and a library of *tells*, assembled per scene by salience. So the writer's job is **not** "describe
the 101 features"; it is to **author a finite library of fact-keyed fragments** the engine assembles
against the infinite set of fact *instances*.

Concretely, there are exactly **two prose surfaces** to author into, plus the existing agent_core
content lists:

| Surface | File | What it is | Status |
|---|---|---|---|
| **Tells** | `assets/data/tells.ron` *(new, design-status)* | Oblique symptom fragments, one library per *fact kind*; the Wolfean layer. **The heart of the corpus.** | design (`prose_generation.md` §Data-driven content) |
| **Grammar** | `assets/data/grammar.ron` *(exists; being hardened)* | Guarded/tagged Tracery productions — the sentence skeletons and fills tells/lines realise through. | live; evolving to guarded form |

Everything else (registers, intents, beats, bestiary, goods, norms) is **agent_core meaning-data**,
unaffected by the front-end shift — still worth authoring, lower priority for the text conversion.

> **Retracted from the previous draft of this catalog:** the proposed `places.ron` / `relics.ron`
> "one `look`/`brief` string per feature" surface, and any reference to the Bevy `app`,
> `player_view`, `PlaceRealizer`, or the SLM/`voice` re-voicer. All of that is **superseded** —
> `app` and `voice` are retired (`text_interface.md` §Workspace changes; `prose_generation.md`
> §Determinism "Retire the SLM"), and per-entity static prose is the anti-pattern the architecture
> exists to avoid. Location prose is *assembled from tells over the facts true at a tile*, not stored.

---

## 1. The surfaces at a glance

| # | Surface | File | Now | Target | Plane-friendly | Priority |
|---|---|---|---|---|---|---|
| **A** | **Wolfean tells** | `tells.ron` *(new)* | 0 | a library per fact-kind (~30 kinds × 4–8 tells) | ★★★ pure writing + light tagging | **P0** |
| **B** | **Grammar productions / skeletons** | `grammar.ron` | ~450 lines | 1,500+, guarded & tagged | ★★★ pure writing + light tagging | **P0** |
| **C** | Dramatic registers | `registers.ron` | 15 | 25–30 | ★★☆ prose + 3 flags | P1 |
| **D** | Conversational intents | `intents.ron` | 70 | 90+ | ★☆☆ mechanical | P2 |
| **E** | Narrative beats | `beats.ron` | 97 | 150+ | ★☆☆ mechanical | P2 |
| **F** | Bestiary | `bestiary.ron` | 16 | 30+ | ★★☆ light prose | P2 |
| **G** | Goods / norms | `goods.ron`, `norms.ron` | 9 / 6 | 20 / 12 | ★☆☆ mechanical | P3 |
| **—** | Salience / fact-kind importances | data (per `prose_generation.md` §Salience) | — | tuned | ☆ dev tuning | P2 |

The `waxen_world.md` Part II generators are **not a file** — they are the *method* for deciding which
fact-kinds and tells to author and how they should read. §8 maps each generator onto these surfaces.

---

## A. Wolfean tells — `assets/data/tells.ron` · P0 · *the heart of the corpus*

A **tell** is an oblique surface fragment that a fact *entails* but never names — a symptom, not the
diagnosis (`prose_generation.md` §"The Wolfean layer"). The fact `Poor(npc)` is never rendered "he is
poor"; it surfaces as *"he counts the coppers twice before he lets them go, and his cloak has been
turned at the collar."* The engine only emits a tell when its `implies` fact is **true right now**, so
a tell can never lie. This is where the house style lives, and it is the single largest writing job.

**Schema** (from `prose_generation.md`; author against this shape now — the loader is design-status):

```ron
// assets/data/tells.ron
Tell {
    implies:            FactKind,            // the true fact this obliquely conveys
    template:           "...",               // grammar fragment: a SYMPTOM, never the diagnosis
    distinctive:        u8,                  // 0..100 — how uniquely this detail is caused by the fact
    sense:              Sense,               // Sight | Sound | Smell | Behaviour — how it is noticed
    visibility:         u8,                  // 0..100 — how plainly observable the symptom is
    requires_knowledge: ["token", ...],      // observer must already know this to read the tell
    mood_tint:          { "grief": "...", "gloating": "..." },  // free-indirect colour by observer mood
}
```

**Authoring rules (load-bearing — `prose_generation.md` is explicit):**
- **Oblique by default.** Keep `distinctive` low-to-mid; the reader must do the inference. A high
  `distinctive` is nearly a statement — reserve it. The perception/lore skill *widens* what's noticed
  (more tells), it never makes a fact plainly stated; do **not** author "clear" tells for high-skill
  readers.
- **The template names the symptom, never the fact.** No cosmology words ever (`style_guide.md` §4).
  `Returned(npc)` surfaces as *"he thanked you by a name you never gave"* — never *"he is one of the
  returned."*
- **Every word is an authored constant or a true slot.** Slots (`{name}`, `{count}`, `{place}`) bind
  to real sim values; if a slot can't bind, the tell is skipped. Don't write a slot that isn't a real
  attribute.
- **One charged detail.** Each tell is *one* symptom. Scene assembly picks the single top tell per
  keystone fact and suppresses the rest — so make each tell carry its fact alone.
- **`mood_tint` is colour, not new fact.** The same true symptom reads differently through an anxious
  vs. a gloating observer; the *fact* doesn't change.

**The fact-kinds to write libraries for** (4–8 tells each, mixed `sense`/`distinctive`). These are the
catalogue's spine — each maps a Waxen-canon concept to a renderable fact-kind, surfaced obliquely:

| Fact-kind (authoring) | The thing | Example tell (template, oblique) | `sense` |
|---|---|---|---|
| `Poor(x)` | scarcity at a body | "counts the coppers twice before letting them go; the cloak turned at the collar" | Behaviour |
| `Feud(a,b)` / grievance | a feud | "breaks off talking as you pass; will not be in the same room as {other}" | Behaviour |
| `Returned(x)` (Palingenesis) | come-back-wrong | "thanks you by a name you never gave; too sure of the small true things" | Behaviour |
| `Tallowed(x)` (Wrought) | a smoothing soul | "the hand gives the lamplight back too smooth; the glove goes on before the coin" | Sight |
| `Dimming(here)` (Penury) | the cold/lord | "the lamps go grudging and come back low; the year a little shorter than it was" | Sight |
| `Forgetting(x)` (Lethe) | thinned memory | "loses the thread mid-sentence and pretends he meant to; the ledger starts on a clean page" | Behaviour |
| `Aevum(thing)` | a real relic | "a weight that argues with the hand; an edge that hasn't gone soft; makes the table look like the lie" | Sight |
| `ThinPlace(tile)` (Asymptote) | a thinning | "the grass comes in even and untrodden; the path stops caring which way it goes" | Sight |
| `ForgedLineage(faction)` | untested claim | "the charter is plainly thirty years old and no one says so" | Sight |
| `Grieving(x)` / mood tells | named moods | "keeps the good knife sharp now, the way he'd have hated" | Behaviour |
| `Hostile(x→you)` | turned opinion | "the greeting comes a half-beat late; the hand stays near the belt" | Behaviour |
| `Bonded(x→you)` | attachment (endgame) | "saved you the warm seat without being asked; falls into step like the road kept the place" | Behaviour |

Plus tells for the everyday sim facts the scene needs (`Present`, `Trading`, `Wounded`, `Hungry`,
faction `Stance`, season/weather), authored flat and low-`distinctive` so the charged ones stand out.

**Worked example (full entry, in voice):**
```ron
Tell(
    implies: Returned, template: "{name} thanks you by a name you never gave, and is too sure \
             of the small true things — that the well-rope wants replacing, which knife you keep dull.",
    distinctive: 35, sense: Behaviour, visibility: 60, requires_knowledge: [],
    mood_tint: { "dread": " You do not ask whose name it was.", "fond": " You let it pass, as you'd learned to." },
),
```

**Target:** a library for ~30 fact-kinds, 4–8 tells each (~150–240 tells) — a flight's grind, and the
highest-value writing in the project. Vary `sense` and skeleton (see §B) so a fact never reads the same
way twice.

---

## B. Grammar productions / skeletons — `assets/data/grammar.ron` · P0

The deterministic surface every line (dialogue, tells, scene prose, the "while you were busy" feed)
realises through. Today it is a blind Tracery expander; v2 hardens each production into a **guarded,
tagged fragment** (`prose_generation.md` §"The grammar, hardened"). A bare string keeps working — a
`Production` with no guards, no tags, weight 1 — so **author plain lines now; add guards/tags as the
hardened form lands**.

**Schema (target, guarded/tagged):**
```ron
Production {
    template: "...",          // literal text + #symbol# recursion + {slot} fills
    when:     [Guard, ...],   // ALL must hold against live sim facts, else ineligible (the truth gate)
    tags:     [Tag, ...],     // implies:feud, register:grim, sense:sight, topic:harvest — accumulate up
    weight:   1,
}
```
Guards read facts only: `Mood("grief") >= 0.5`, `Trait("vengeance") >= 0.6`, `Relation(Feud, a, b)`,
`Season(Waning)`, `Present(x)`, `TimeOfDay(Dusk)`, `Knows(observer, token)`. Tags are author markup;
because they accumulate, a finished line ships with machine-readable meaning the journal files by.

**Authoring rules:**
- **Vary structure, not just fillers** (the 10,000-bowls rule). Author many *skeletons* per fact shape;
  ten skeletons × five fills reads far more varied than one × fifty.
- **The Venom cap.** ≤ 2 independently-varied slots per sentence, ≤ 3–4 per passage, or it reads as
  nonsense. Vary the *meaning-bearing* word (the crime, the stake), not verb synonyms.
- **No elegant variation for pronouns.** Repeat the name when a pronoun would be ambiguous; the
  discourse model (`prose_generation.md` §Referring expressions) licenses pronouns — don't rotate
  synonyms for variety.
- **Content requested by tags.** A line is asked for by meaning ("`implies:feud`, `register:grim`, not
  `gloating`"). Tag your productions so requests can find them.

**What to write:**
- **Deepen the existing dialogue buckets** (`greet`, `console`, `mourn`, `confide`, `gossip`,
  `accuse`, `threaten`, `plead`, `recruit`, …; act + `/affect`) to 12–15 lines each in Waxen idiom.
- **Author skeleton sets** for the scene-prose fact shapes the tells (§A) plug into — multiple sentence
  shapes for "a body at a tile," "a feature seen," "a thing examined," "exits described in-voice."
- **Deepen the world-flavor helper rules** (`#world_aside#`, `#fog_phrase#`, `#mist#`, `#oath#`,
  `#vow#`, `#lament#`, `#epithet#`, `#address#`) and add `#dimming#`, `#thin_place#`,
  `#provenance_doubt#` — the reusable ambient idiom every line can call.

**Worked example (in voice, with tags as a comment until the hardened form lands):**
```ron
"mourn/grieving": [
    "He said the small true things to the last — the well-rope, the dull knife. I keep it sharp now. He'd have hated that.",
    // tags: register:grim, implies:grief ; guard: Mood("grief") >= 0.5
],
```

**Target:** ~450 → 1,500+ lines. Prioritise `greet/console/mourn/confide/gossip` (highest in-game
frequency) and the scene-prose skeleton sets the tells need.

---

## C. Dramatic registers — `assets/data/registers.ron` · P1

The emotional keys the narrative director plays in; each carries the surface prose for an arc
(epithets, one-line plights, gossip, quest text). 15 exist. **agent_core meaning-data — unaffected by
the front-end shift, but its epithet/situation/told/quest prose surfaces in the text client**, so
it's worth authoring.

**Schema:**
```ron
(
    name: "betrayal",
    spine: true, trunk: true, bright: false, seeds: Some("vengeance"), casting: Warmest,  // mechanical
    epithet_lead: "the Betrayed", epithet_other: "the Faithless",       // by-names (surface)
    situation_lead: "still raw from a trusted friend's turning.",        // one-line plight
    situation_other: "something unconfessed moving behind its eyes.",
    noun: "betrayal",
    told: "They say {lead} was betrayed by {other}.",                   // gossip sentence
    quest_plea: "\"{other} wronged me, and I cannot let it lie. Find them.\"",
    quest_objective: "Seek out {other}, who wronged {giver}.",
),
```

**What to write:** Waxen-native registers the cosmology begs for — **loss/grief**, **return** (the
come-back-wrong), **dimming** (Penury cold), **forgetting** (Lethe eating a bond), **provenance**
(the Aevum-real surfacing — pairs with the `Aevum` tell), **tallow** (a soul smoothing away),
**waking** (Scintilla stirring). ~7 prose fields each. Keep the epithet a *by-name*, never a title.
Mark the 3 mechanical flags `TODO(dev)` on new registers.

**Target:** 15 → 25–30. (Full new-register example: see git history / the `forgetting` sketch — epithet
"the Half-Remembered", told "can't rightly tell you how long {lead} has kept the place.")

---

## D. Conversational intents — `assets/data/intents.ron` · P2 · *mechanical*

The *why* behind a line — an act paired with IAUS scoring (who wants to say it) and social effects. 70
exist. Under the parser conversation model (`text_interface.md` §Conversation), the player picks an
intent by typing it (`accuse him`, `ask about the feud`) — the meaning system is unchanged; only the
*picker* changed from menu to parser. Add an intent only to unlock a new *voice* (then author its
grammar bucket in §B).

**Schema:**
```ron
(
    id: "a_greeting", act: "greet", tags: ["warmth"], weight: 0.4,
    appeal: [ (input: OpinionOf, curve: Linear(m: 0.8, b: 0.1)),
              (input: Trait("sociability"), curve: Linear(m: 0.6, b: 0.1)) ],
    moves: [ Turn(who: Listener, toward: Speaker, delta: 0.03) ],
),
```
**What to add (rare):** intents for Waxen tells — `a_slip_of_memory` (Lethe), `a_too_true_thing`
(Returned). Pair each with a grammar bucket. **Target:** 70 → ~90.

---

## E. Narrative beats — `assets/data/beats.ron` · P2 · *mechanical*

The director's repertoire of dramatic micro-scenes (Polti). 97 exist. **Data, not prose** — casting +
preconditions + world-effects — harder to author offline without testing, but new ones draft against
the schema. The text client surfaces them via the prose layer and the "while you were busy" feed; the
director's `Effect::Voice` lines are heard in conversation (`text_interface.md` §Conversation).

**Schema:**
```ron
(
    id: "a_kindred_spirit", register: "romance", tags: ["romance","relief","personal"],
    phases: [Setup], tension: -0.5, stakes: 0.6,
    cast: [Protagonist, Lover], pre: [Exists(who: Lover)],
    effects: [ Turn(who: Lover, toward: Protagonist, delta: 0.6),
               Stir(who: Lover, mood: "love", delta: 0.4) ],
),
```
Roles: `Protagonist, Ally, Rival, Foe, Patron, Bystander, Lover, Mentor`. Effects include `Bond`,
`Free`, `Voice` (the endgame attachment primitives, per `gameplay_targets.md`).

**What to draft:** beats for the new Waxen registers (§C) — **a return**, **a dimming**, **a
forgetting** (leans on `Effect::Free`/the impact-floor), **a recognition** (an `Aevum` relic
surfaces). Tie each to its register so the surface prose comes free. **Target:** 97 → 150+. Flag
`pre`/`effects` for a dev to validate against the real enums.

---

## F. Bestiary — `assets/data/bestiary.ron` · P2 · *light prose*

16 creatures. Names are evocative; description is *generated* from tells (§A) over the creature's
facts, not stored — so author **names + a fact-tell or two** (e.g. a `Wrong(fauna)` tell: "too quiet
for its size; the eyes don't track the way an animal's should"). Schema:
```ron
(name: "ash elk", diet: Herbivore, form: Strider, habitat: ["tundra"],
 min_temp: 0.0, max_temp: 7.0, size: 1.5, fecundity: 0.7, gregarious: 1.0, color: (0.60,0.62,0.66)),
```
**What to write:** ~14 more biome fauna with plain-eerie Waxen names (the fog's own wildlife — pale,
wrong-smooth, too-quiet); confirm `form`/`diet` enums before adding new variants. **Target:** 16 → 30+.

---

## G. Goods & norms — `assets/data/goods.ron`, `norms.ron` · P3 · *mechanical*

- **Goods** (9): add **lamp-oil/heat** (the Penury currency — load-bearing) and **salt**; each needs a
  `recipe` to enter the integer economy. `(name: "lamp-oil", base_price: ..., target_stock: ..., nutrition: 0.0)`.
- **Norms** (6, the Caryatid Law as taboo): add the local **Unsayable** (Lacuna) and a
  **remember-too-deep** (Lethe) taboo. `(act: "name-the-lake", modality: Forbidden, weight: ..., defiance: Some("gnosis"))`.

Both mechanical; do with the compiler. Targets: goods 9→20, norms 6→12.

---

## 8. Bridging the generators (waxen_world.md Part II) → these surfaces

The Part II generators are the *method* for deciding what to author, not a file:

- **§A Seam-Stance** → a dial on the *voice* of every tell and line. Oblivious = props the forgery up,
  explains nothing (flat, low-`distinctive` tells). Uneasy = the custom-without-reason (most charged
  tells). Collaborator/Wrought/Waked = the rare, memorable fact-kinds (drive registers §C, beats §E).
- **§B Faction generator** → **register epithets (§C)** + **faction-stance tells (§A)** + a
  **grammar voice-pack (§B)**: ~5 lines per archetype (Keepers / House of the Seven / Tallow-Order /
  Lectors / Undertakings / Render-folk) that leak its Entangled Law.
- **§C Society generator** → the four rolled pressures (Scarcity / Amnesia / Return-custom / local
  Taboo) become the **fact-kinds whose tells (§A)** a settlement's scene assembles from — *this* is how
  the same feature type reads differently per town, with **no per-location string**. Also feeds the
  local Unsayable norm (§G).
- **§D History generator** → the **distortion tier** (`prose_generation.md` §"The distortion tier"):
  the four strata (Lived / Hearsay / Ruin / Unwritten) are reliability levels driving rumor garble and
  corrupt-text erosion. Author **degraded variants** (half-legible inscriptions, frayed rumors) — every
  falsehood still traces to a real fact. "Is this ruin real Aevum or forged depth?" is the top
  provenance verdict: author tells for *both* readings (the `Aevum` tell and a `ForgedDepth` tell) and
  resolve neither.
- **§E People generator** → the cosmological flags (Returned / Tallowing / Lethe-marked /
  Effigies-marked / Concord-marked / Corollary) **are the fact-kinds you author tell libraries for**
  (§A). A flag is *leaked through a tell*, never labelled. The Corollary flag is the one the player may
  suspect and never confirm — author its tells deliberately ambiguous (low `distinctive`).

---

## 9. Suggested flight plan (one long flight, prose-only, no compiler)

Reordered for v2: the work is fragments and tells, not static descriptions. All draftable offline as
prose against the schemas above; add guards/tags as comments where the hardened form isn't in yet.

1. **§A Tells** — draft tell libraries for the 12 spine fact-kinds in the §A table (4–8 each), then the
   everyday facts. *The biggest, highest-value job.* ~2–3 hrs.
2. **§B Grammar — scene skeletons** — author multiple sentence shapes per scene fact-shape (body at a
   tile, feature seen, thing examined, exits in-voice) so tells realise with variety. ~1 hr.
3. **§B Grammar — dialogue depth** — bring `greet/console/mourn/confide/gossip` to ~15 lines each. ~45 min.
4. **§B Grammar — world-flavor helpers** — deepen the helpers and add `#dimming# #thin_place#
   #provenance_doubt#`. ~30 min.
5. **§C Registers** — the 7 new Waxen registers, prose fields only; flags `TODO(dev)`. ~45 min.
6. **§8 voice-packs** — 5 lines + a stance-tell per faction archetype; a tell-set per people-flag. ~45 min.

Leave intents (§D), beats (§E), goods/norms (§G), and salience tuning for a session with the compiler.
Mark anything uncertain `TODO(dev)` and keep writing — don't let a mechanical question stall the prose.

---

## 10. Status / provenance

- These four `worldbuilding/` docs are the v2 authoring set, on branch
  `claude/text-adventure-worldbuilding-hh6bun` (rebased onto `v2`).
- **Canon for *how* prose is made lives in `docs/prose_generation.md` + `docs/text_interface.md`
  (committed on v2 — do not edit). This catalog defers to them** and itemises the corpus they call
  "the product." Where this catalog and those docs ever disagree, **those docs win** — open an issue.
- **New file to populate:** `assets/data/tells.ron` (design-status in `prose_generation.md`); the
  loader is a dev task, the tell prose is authorable now against the documented `Tell` schema.
- **Superseded** (removed from an earlier draft of this catalog): a `places.ron`/`relics.ron`
  per-entity description surface, and references to `app`/`player_view`/`PlaceRealizer`/the SLM
  re-voicer — all retired by the v2 design.
- Counts in §1 read from `assets/data/*.ron` at the v2 tip (2026-06-27). Re-check before a big push.
