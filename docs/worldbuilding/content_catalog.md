# The Waxen World — Content Authoring Catalog

> The worklist for the v2 text-first shift. Every surface a writer can author into, with its exact
> file, schema, current size, a target, a priority, and worked examples **in voice**. Pair it with
> `style_guide.md` (how to write) and `waxen_world.md` (the canon). Built to be worked **offline** —
> everything you need to author is in this file; you don't need to read the code.
>
> **Conventions:** all content lives under `assets/data/*.ron` and is parsed by the owning crate (see
> `CLAUDE.md`). Adding lines/entries needs **no recompile** for the prose surfaces (grammar, registers,
> features, bestiary, goods); it's just data. Beats/intents/norms are data too but interact with
> mechanics — author prose freely, but flag any new *mechanical* field for a dev pass.
>
> **Golden rule (repeat):** the cosmology is *never named* to the player. Canon terms in this catalog
> (Lethe, Penury, Caryatid, tallow, Corollary…) are **authoring labels only**. Write the folk idiom
> (`style_guide.md` §6).

---

## 0. How to use this on a plane

Work top-down by priority. Each surface below is self-contained: schema + examples + a target count.
Author into a scratch copy of the `.ron` file (or a `.md` draft if you prefer), keep entries small and
many, and run the `style_guide.md` §9 checklist on each line. A suggested batching for one long flight
is in §10.

**The single biggest gap for a Zork-like:** the world has **101 locations with zero descriptive prose**
(`features.ron` is placement-math only). Location prose (§E) is the new spine of a text game and is
where most of the writing lives. It needs a tiny engine hook (noted in §E) but the *prose* can and
should be drafted now.

---

## 1. The surfaces at a glance

| # | Surface | File | Now | Target | Prose? | Priority | Plane-friendly |
|---|---|---|---|---|---|---|---|
| **E** | **Location / room prose** | *new* `places.ron` | 0 | ~120 (1 per feature, + variants) | **all prose** | **P0** | ★★★ pure writing |
| **A** | Dialogue grammar | `grammar.ron` | ~450 lines / 109 buckets | 1,500+ lines | **all prose** | **P0** | ★★★ pure writing |
| **F** | Relic / item descriptions | *new* `relics.ron` | 0 | ~40 | **all prose** | P1 | ★★★ pure writing |
| **B** | Dramatic registers | `registers.ron` | 15 | 25–30 | **prose-heavy** | P1 | ★★☆ prose + 3 flags |
| **G** | Overheard asides / world flavor | `grammar.ron` helpers | ~8 helper rules | 25+ | **all prose** | P1 | ★★★ pure writing |
| **C** | Narrative beats | `beats.ron` | 97 | 150+ | mixed | P2 | ★☆☆ mechanical |
| **D** | Conversational intents | `intents.ron` | 70 | 90+ | mechanical | P2 | ★☆☆ mechanical |
| **H** | Bestiary | `bestiary.ron` | 16 | 30+ | names + flavor | P2 | ★★☆ light prose |
| **I** | Goods / relics economy | `goods.ron` | 9 | 20+ | names | P3 | ★☆☆ mechanical |
| **J** | Norms / taboos | `norms.ron` | 6 | 10–12 | mechanical | P3 | ★☆☆ mechanical |
| **—** | Codex / cosmology lore (canon doc) | `waxen_world.md` + new | — | living | reference | P1 | ★★☆ worldbuilding |

Generators (`waxen_world.md` Part II) are **not a separate file** — they are the *method* for filling
the surfaces above. §9 maps each generator (Seam-Stance, Faction, Society, History, People) onto the
concrete surfaces it feeds.

---

## E. Location / room prose  ·  P0  ·  *new surface*

**The text-game spine.** A Zork-like is its rooms. We have 101 placeable features (`features.ron`:
thorp, village, city, monastery, royal_court, temple, thieves_guild, ruined_keep, barrow,
sunken_temple, catacombs, shrine, monument, sacred_grove, faerie_ring, frontier_fort, nomad_camp,
fishing_village, mining_camp, …) with **no descriptive text**. Each needs:

- a **first-look** description (2–4 sentences, §8 voice),
- a **brief** re-look (1 sentence),
- and ideally **2–3 state variants** keyed to the Society-generator rolls (§9.C) so the same feature
  type reads differently per settlement (a salt-town vs. a fog-bound thorp).

**Engine note (one small dev hook needed):** `features.ron` entries carry no text field today.
Proposed authoring schema — a parallel `assets/data/places.ron` keyed by feature `name`, so prose is
decoupled from placement math and a writer never edits the math file:

```ron
// assets/data/places.ron — proposed; prose can be drafted now against this shape
{
    "thorp": (
        look: "A double handful of turf-roofed houses leaning on each other against the cold. \
               The lamps are kept low here even at supper, and no one says it's to save the oil. \
               A child watches you from a doorway and does not wave.",
        brief: "The low houses, the kept-low lamps, the unwaving child.",
        // optional state variants, chosen by the settlement's rolled pressures (§9.C):
        variants: [
            (when: "scarcity:salt",   look: "..."),
            (when: "return:venerate", look: "..."),
        ],
    ),
    "barrow": ( look: "...", brief: "..." ),
    // ...one per feature name
}
```

> **Action for a dev (not the plane):** add a `places.ron` loader + a `look`/`brief` field surfaced by
> the player's "look" verb (`player_view` in `player.rs`) and the Perception Layer's `PlaceRealizer`.
> Until then, prose drafted against the shape above is ready to drop in.

**What to write, per feature category:**

- **Community** (thorp, village, city, monastery, fishing_village, mining_camp, nomad_camp,
  frontier_fort): lead with how the local **Penury** (what's rationed) and **return-custom** show in
  daily habit. The tell is a custom obeyed without reason.
- **Court** (royal_court, temple, guild, thieves_guild, druid_circle, barons_keep): lead with how
  **legitimacy** is claimed (the forged lineage no one tests — Lethe). The tell is the too-clean
  provenance.
- **Ruin** (ruined_keep, barrow, sunken_temple, catacombs, collapsed_mine): the **Aevum/forgery**
  ambiguity — is this real-old or painted-deep? The tell is realness that makes the present look thin,
  *or* depth that's suspiciously legible.
- **Wilderness** (shrine, monument, sacred_grove, natural_wonder, faerie_ring, thin places): the
  **Asymptote** — the even, dreamless shape of a thinning. The tell is the land going wrong-smooth.

**Worked examples (drop-in voice):**

> **thorp** — *A double handful of turf-roofed houses leaning on each other against the cold. The
> lamps are kept low here even at supper, and no one says it's to save the oil. A child watches you
> from a doorway and does not wave.*

> **ruined_keep** — *The keep had fallen the way old things fall, except the fall was too tidy — the
> stones lay where a careful hand would have laid them. Diggers worked the lower courses for pieces
> that argued with the hand, and the old families paid for those and asked nothing about the rest.*

> **faerie_ring (a thin place)** — *Past the last fence the grass came in even and untrodden, poured
> into that smooth dreamless shape, and the path you'd been on simply stopped caring which way it
> went. Folk left the ring a wide berth and a little bread. You did the same and were glad to.*

> **fishing_village** — *Boats drawn up past the tide-line, nets mended and re-mended. They don't
> speak of the lake here, only of "the low water," and they go out at the grey hour and not before.*

**Target:** one `look` + `brief` for all ~101 features (P0), then 2–3 variants for the dozen most
common feature types (P1). This alone is a flight's work.

---

## A. Dialogue grammar  ·  P0  ·  `assets/data/grammar.ron`

The deterministic generative surface for *all* spoken lines (NPC↔NPC, NPC→player, the Perception
Layer's prose log, director-forced lines). **No phrasebook** — lines are composed from rules at
runtime. This is the second-largest pure-writing surface.

**Schema:** a RON map of `"rule_name": [ "line", "line", … ]`. Add a line by adding a string to the
right bucket. It's live immediately.

```ron
"greet/warm": [
    "#listener#. It does my soul good to see your face.",
    "Ah — #listener#. Come close. The fog is thick tonight.",
    // add your line here ↓
    "#listener# — sit. You look like the road's been at you.",
],
```

**Bucket naming:** `act` or `act/affect`. The renderer tries `act/affect` first, falls back to bare
`act`. So `accuse/angry` is angry-flavored accusation; `accuse` is the neutral floor.

- **Acts** (illocutionary): `greet, accuse, praise, mourn, console, reconcile, confide, plead,
  threaten, boast, gossip, deflect, recruit` (and the matching set in `intents.ron`).
- **Affects** (mood tags): `warm, angry, afraid, calm, grieving, …` (matches the speaker's dominant
  mood). When in doubt author the bare act first, then the high-frequency moods (warm/angry/afraid).

**Substitution tokens:** `#listener#`, `#speaker#`, `#referent#` (the subject of conflict/affection),
plus helper rules you can call inside any line: `#address#`, `#vow#`, `#oath#`, `#world_aside#`,
`#fog_phrase#`, `#epithet#`, `#lament#`, `#mist#`. (See §G — those helpers are themselves authorable.)

**What to write:**
- **Deepen every existing bucket** to 12–15 lines (most have ~10). Variety kills repetition; this is
  the single highest-impact grind.
- **Fill missing `act/affect` pairs.** Each act should have at least `warm`, `angry`, `afraid`,
  `calm`, `grieving` where it makes sense (a grieving boast is rare; a grieving mourn is the point).
- **Push the Waxen idiom in.** Lines should leak the world: forgetting mid-sentence (Lethe-marked
  speaker), uncanny accuracy (Returned), the cold of a passing lord (Penury). Don't name it.

**Worked examples (in voice):**

> `confide/afraid`: *"Don't write this down. The fog closes in around it if you write it down — I've
> watched it happen. Just… keep it where the mist can't reach."*

> `accuse/angry`: *"You came back wrong, #listener#, and you've the gall to thank me by my own name.
> I'll not open the gate."*

> `mourn/grieving`: *"He said the small true things to the last. That the well-rope wanted replacing.
> I keep the good knife sharp now. He'd have hated that."*

**Target:** ~450 → 1,500+ lines. Prioritize: deepen `greet/console/mourn/confide/gossip` (highest
in-game frequency), then accusation/threat/plead for drama.

---

## F. Relic / item descriptions  ·  P1  ·  *new surface*

Aevum-relics and ordinary goods the player can examine. In a text game, "examine X" is a core verb;
relics are where the **Aevum/forgery** seam is most legible at hand-scale. No file exists yet.

**Proposed schema** (mirror `places.ron`): `assets/data/relics.ron`, keyed by item id, with `look`
prose. Lead with the **material tell** (`style_guide.md` §3.4–3.5).

```ron
{
    "true_hinge": ( look: "Just a hinge, but it has a weight that argues with your hand and an edge \
                           that hasn't gone soft in all the years it should have. Set it on a table \
                           and the table starts to look like the lie." ),
    "toll_book":  ( look: "..." ),
}
```

**What to write:** ~20 authentic relics (the realer-than-real category — they "make everything near
them look thin"), ~20 ordinary goods (`goods.ron`: grain, bread, shroud, veil, basket, ore, ingot,
tool — flat, mundane, no tell), and a handful of **tallow-residue** objects (a candle's-hand smoothness
on a thing a tallowed sorcerer handled).

**Target:** ~40 entries. Engine hook: same shape as §E (a small loader + the "examine" verb).

---

## B. Dramatic registers  ·  P1  ·  `assets/data/registers.ron`

The emotional keys the narrative director plays in. Each register carries the **surface prose** for an
arc: epithets, one-line plights, gossip, quest text. 15 exist (betrayal, vengeance, romance, wonder,
ambition, war, disaster, triumph, grace, …). Prose-heavy with three small mechanical flags.

**Schema:**
```ron
(
    name: "betrayal",
    spine: true, trunk: true, bright: false, seeds: Some("vengeance"), casting: Warmest,  // mechanical
    epithet_lead: "the Betrayed", epithet_other: "the Faithless",      // by-names (player-facing)
    situation_lead: "still raw from a trusted friend's turning.",       // one-line plight
    situation_other: "something unconfessed moving behind its eyes.",
    noun: "betrayal",                                                   // for gossip
    told: "They say {lead} was betrayed by {other}.",                  // gossip sentence
    quest_plea: "\"{other} wronged me, and I cannot let it lie. Find them.\"",  // NPC's ask
    quest_objective: "Seek out {other}, who wronged {giver}.",          // HUD line
),
```
Placeholders: `{lead}`, `{other}`, `{giver}`, `{noun}`.

**What to write:** new Waxen-native registers the cosmology begs for — **loss/grief**, **return** (the
come-back-wrong), **dimming** (Penury creeping cold), **forgetting** (Lethe eating a bond),
**provenance/recognition** (the Aevum-real surfacing), **tallow** (a soul smoothing away),
**waking** (Scintilla stirring). Each is ~7 prose lines. Keep the epithet a *by-name* ("the Penurious",
"the Faithless"), never a title.

**Worked example (new register, drop-in):**
```ron
(
    name: "forgetting",
    spine: false, trunk: false, bright: false, seeds: None, casting: Warmest,
    epithet_lead: "the Half-Remembered", epithet_other: "the One They Lose",
    situation_lead: "losing the thread of its own days, and pretending not to.",
    situation_other: "a name on the tip of every tongue and no face to hang it on.",
    noun: "a forgetting",
    told: "They say {lead} can't rightly tell you how long {lead} has kept the place.",
    quest_plea: "\"I had it. I had all of it. Help me find where it went before the mist has the rest.\"",
    quest_objective: "Recover what {giver} is losing to the fog.",
),
```

**Target:** 15 → 25–30. The 3 mechanical flags (`spine/trunk/bright/seeds/casting`) on *new* registers
should be sanity-checked by a dev pass; the prose is yours.

---

## G. Overheard asides & world-flavor helpers  ·  P1  ·  `assets/data/grammar.ron`

The helper rules that any dialogue line can call — they carry the ambient dread. Small, pure prose,
extremely high leverage because every line that calls them inherits the flavor. These already exist:
`#world_aside#`, `#fog_phrase#`, `#mist#`, `#oath#`, `#vow#`, `#lament#`, `#epithet#`, `#address#`.

**Schema:** same as §A — a named bucket of strings.
```ron
"world_aside": [
    "the wheel turns and we forget",
    "even the stones here dream",
    "the archons made a fine imitation of a sky",
    // add ↓
    "the toll-book always starts on a clean page, you ever notice",
],
```

**What to write:** deepen each helper to 12–15 lines and add new helper categories the Waxen canon
wants: `#dimming#` (the cold/lamps), `#returned_tell#` (the over-true small thing), `#thin_place#`
(the dreamless smoothness), `#provenance_doubt#` (how-old-is-it hedges), `#tallow_tell#` (the soft
hand). These become the reusable tells §A lines lean on.

**Worked examples:**
> `#dimming#`: *"the lamps go grudging when his chair passes"* · *"the year's a little shorter than
> it was"* · *"we keep them low and don't say why"*
> `#returned_tell#`: *"he knew my business better than I'd told it"* · *"she thanked me by a name I
> never gave her"*

**Target:** ~8 → 25+ buckets, each 10–15 lines deep.

---

## C. Narrative beats  ·  P2  ·  `assets/data/beats.ron`  ·  *mechanical, less plane-friendly*

The director's repertoire of dramatic micro-scenes (Polti's 36 situations as a base). 97 exist. These
are **data, not prose** — preconditions, casting, world-effects — so they're harder to author offline
without testing, but new ones can be drafted against the schema.

**Schema:**
```ron
(
    id: "a_kindred_spirit",
    register: "romance",                         // → registers.ron supplies the prose
    tags: ["romance", "relief", "personal"],
    phases: [Setup],                             // Setup | Rising | Climax | Fall
    tension: -0.5, stakes: 0.6,
    cast: [Protagonist, Lover],                  // roles to fill
    pre: [Exists(who: Lover)],                   // preconditions
    effects: [                                   // world mutations
        Turn(who: Lover, toward: Protagonist, delta: 0.6),
        Stir(who: Lover, mood: "love", delta: 0.4),
    ],
),
```
Roles: `Protagonist, Ally, Rival, Foe, Patron, Bystander, Lover, Mentor`. Effects:
`Turn, Stir, Sway, Grudge, Decree, War, Disaster, Afflict, Reveal, Bond, Free, Voice`.

**What to write (offline-draftable, dev-verified later):** beats for the new Waxen registers (§B) —
**a return** (someone the player knew comes back wrong: `Reveal` + `Afflict`), **a dimming** (a
Castellan passes, `Disaster`-lite cold), **a forgetting** (a bonded soul loses the player — leans on
`Effect::Free`/the impact-floor, per `gameplay_targets.md`), **a recognition** (an Aevum-relic
surfaces). Tie each to its register so the prose comes free.

**Target:** 97 → 150+. Draft on the plane; flag for a dev to validate `pre`/`effects` against the
real enums before committing to a run.

---

## D. Conversational intents  ·  P2  ·  `assets/data/intents.ron`  ·  *mechanical*

The *why* behind a line — pairs an act with IAUS scoring (who wants to say it) and social effects. 70
exist. Mostly you don't touch these; add one only to unlock a new *voice* (a new act/affect that
grammar §A then renders).

**Schema:**
```ron
(
    id: "a_greeting", act: "greet", tags: ["warmth"], weight: 0.4,
    appeal: [
        (input: OpinionOf,            curve: Linear(m: 0.8, b: 0.1)),
        (input: Trait("sociability"), curve: Linear(m: 0.6, b: 0.1)),
    ],
    moves: [ Turn(who: Listener, toward: Speaker, delta: 0.03) ],
),
```
Inputs: `OpinionOf, Trait(name), Mood(name), GrievanceAgainst, SharedHistory, Prominence`.

**What to add (rare):** intents for Waxen tells — e.g. `a_slip_of_memory` (a Lethe-marked speaker,
appeals on a "forgetting" mood), `a_too-true-thing` (a Returned speaker). Pair each with a grammar
bucket (§A) of the same act. **Target:** 70 → ~90, mostly to support the new registers.

---

## H. Bestiary  ·  P2  ·  `assets/data/bestiary.ron`  ·  *light prose*

16 creatures (ash elk, rime vole, glass deer, wraith cat, dust stalker, …). Names are evocative;
there's no description field yet. Plane-friendly for **naming**, and for drafting one-line descriptions
into a future `look` field (same hook as §E).

**Schema:**
```ron
(
    name: "ash elk", diet: Herbivore, form: Strider, habitat: ["tundra"],
    min_temp: 0.0, max_temp: 7.0, size: 1.5, fecundity: 0.7, gregarious: 1.0,
    color: (0.60, 0.62, 0.66),
),
```
Diet: `Herbivore | Carnivore`. Form: `Strider | …` (check enum before adding new forms).

**What to write:** ~14 more biome-appropriate fauna with Waxen-tinged names (things that read as the
fog's own wildlife — pale, wrong-smooth, too-quiet). Keep names plain-eerie, not fantasy-baroque (per
`style_guide.md`: a tired carter's nouns). **Target:** 16 → 30+.

---

## I. Goods & economy  ·  P3  ·  `assets/data/goods.ron`  ·  *mechanical*

9 goods (grain, bread, reed-fibre, shroud, veil, basket, ore, ingot, tool). Integer economy — new
goods need a `recipe` (`recipes.ron`) to enter the world. Schema:
```ron
(name: "bread", base_price: 30, target_stock: 25, nutrition: 45.0),
```
**What to add:** lamp-oil/heat (the Penury currency — load-bearing for the cosmology), salt (a common
scarcity), and any relic-category trade good. Pair each with a recipe. **Target:** 9 → 20+. Mechanical;
do with a dev.

---

## J. Norms / taboos  ·  P3  ·  `assets/data/norms.ron`  ·  *mechanical*

The deontic rules — the Caryatid Law as the world's taboo system. 6 exist (forbid `awaken`/`ascend`,
oblige `bind`, …). Schema:
```ron
(act: "awaken", modality: Forbidden, weight: 1.3, defiance: Some("gnosis")),
```
Modality: `Forbidden | Permitted | Obliged`. These map directly to the **Lacuna** (the unsayable) and
the player's transgressive verbs. **What to add:** a `name`/`speak-the-name` taboo (the local Unsayable
from the Society generator, §9.C), a `remember-too-deep` taboo (Lethe). **Target:** 6 → 10–12.
Mechanical; dev pass.

---

## 9. Bridging the generators (waxen_world.md Part II) to these surfaces

The Part II generators aren't a file you fill — they're the **method** for filling the surfaces above
consistently. When you author, roll the generator and let it drive the prose.

- **§A Seam-Stance** → governs the *voice* of every authored person and faction. Oblivious = props the
  forgery up, explains nothing (the default for most §A grammar lines and §E rooms). Uneasy =
  superstition/avoidance (the custom-without-reason; most tells live here). Collaborator/Wrought/Waked
  = the rare, memorable ones (drive the dramatic registers §B and beats §C). **Use it as a dial on tone
  for any line you write.**

- **§B Faction generator** → feeds **registers (§B)**, **beats (§C)**, and faction-flavored **grammar
  (§A)**. When the director instantiates a faction (Keepers / House of the Seven / Tallow-Order /
  Lectors / Undertakings / Render-folk), it needs: a plain self-name, a by-name epithet (register
  field), and 2–3 grammar lines in its idiom. **Author a "faction voice pack"**: ~5 lines per
  archetype that leak its Entangled Law. (Engine note: faction naming/voice-pack selection is a dev
  hook; the *lines* are yours to write now.)

- **§C Society generator** → feeds **location prose (§E)** directly. Each settlement rolls Scarcity /
  Amnesia / Return-custom / local-Taboo; those four become the four tells in the room's `look`, and the
  `variants` field (§E) is exactly where the rolled pressures swap the prose. **This is why §E wants
  variants.** Also feeds **norms (§J)** (the local Unsayable).

- **§D History generator** → feeds **ruin/relic prose (§E, §F)** and a future **codex**. The four
  strata (Lived / Hearsay / Ruin / Unwritten) are *reliability tiers* — author ruin and relic prose so
  the deeper the claim, the more it frays (`style_guide.md` §5, fidelity-in-prose). "Is this ruin real
  Aevum or forged depth?" is the top provenance verdict — write both readings into the prose and
  resolve neither.

- **§E People generator** → feeds **grammar affect-buckets (§A)**, **intents (§D)**, and the
  **Corollary** flag. The cosmological flags (Returned / Tallowing / Lethe-marked / Effigies-marked /
  Concord-marked / Corollary) are *tells leaked through ordinary self-presentation*, never labels.
  **Author a "flag tell-set"**: for each flag, 3–5 grammar lines and one `#helper#` (§G) that leaks it.
  (e.g. Lethe-marked → loses threads mid-sentence; Returned → over-true small things; tallowed → the
  soft hand, "they don't last, that trade.")

---

## 10. Suggested flight plan (one long flight, prose-only, no compiler)

Ordered to keep you in pure-writing mode (no engine round-trips), highest impact first:

1. **§E Location prose** — draft `look` + `brief` for all ~101 features into a `places.ron` draft.
   (Biggest, most valuable, pure writing.) ~2–3 hrs.
2. **§A Grammar deepening** — bring `greet / console / mourn / confide / gossip` to 15 lines each, in
   Waxen idiom. ~1 hr.
3. **§G World-flavor helpers** — add `#dimming# #returned_tell# #thin_place# #provenance_doubt#
   #tallow_tell#`, 10+ lines each. ~45 min.
4. **§F Relic descriptions** — 20 Aevum relics + 10 tallow-residue objects. ~45 min.
5. **§B New registers** — write the 7 Waxen-native registers (loss, return, dimming, forgetting,
   provenance, tallow, waking), prose fields only; mark the mechanical flags `TODO(dev)`. ~45 min.
6. **§9 voice packs** — 5 lines per faction archetype + per people-flag tell-set, into a scratch file.
   ~45 min.

Everything in 1–6 is drop-in prose against schemas in this file. Beats (§C), intents (§D), goods (§I),
norms (§J) are mechanical — leave them for a session with the compiler. Mark anything you're unsure of
with `TODO(dev)` and keep moving; don't let a mechanical question stall the writing.

---

## 11. Status / provenance

- These three docs (`waxen_world.md`, `style_guide.md`, `content_catalog.md`) are the v2 worldbuilding
  authoring set. Committed to branch `claude/text-adventure-worldbuilding-hh6bun`.
- **Counts** in §1 are read from the current `assets/data/*.ron` (2026-06). Re-check before a big push.
- **New files proposed** (need a small dev loader hook before prose goes live): `places.ron` (§E),
  `relics.ron` (§F), and `look`/`brief` fields surfaced via `player_view` + the Perception Layer's
  `PlaceRealizer`. The prose is authorable now; the hook is a separate, small code task.
- The cosmology terms used as labels here trace to `waxen_world.md`; the in-repo folk register
  (`fog / wheel / the seven / archons`) is already live in `grammar.ron` — extend it, don't replace it.
