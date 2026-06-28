# Worked Examples — one for everything you author

> A copy-paste starting point for each authoring surface, in the Waxen voice, with the real RON
> syntax. Read alongside `content_catalog.md` (the worklist), `style_guide.md` (the voice), and the
> v2 canon `../prose_generation.md` + `../text_interface.md`. A pre-seeded starter file lives at
> `assets/data/tells.ron` — fill it in rather than starting blank.

## The mental shift (read this first)

**You are not writing scenes or room descriptions. You are writing *fragments* the engine snaps
together over facts the sim already tracks.** Two kinds of fragment do almost all the work:

- a **tell** — one *symptom* of one true fact (§1), and
- a **grammar line** — a sentence *shape* (§2).

Everything else (registers, intents, beats) tells the director *what dramas can happen* — you are not
writing their words, the grammar + tells do that. The cosmology is **never named** to the player
(`style_guide.md` §4/§6): you write the symptom, the folk idiom, the habit — never "Lethe", "returned",
"tallowed", "the forgery".

---

## 1. Tells — `assets/data/tells.ron` (the heart; start here)

**What it is:** one oblique symptom of one true fact. The sim knows `Poor(npc)` is true; you never
write "he is poor" — you write what a person would *notice* and let the player infer. The engine only
shows a tell when its fact is true, so it can't lie.

**Recipe:** pick a true fact → write the symptom, never the fact → set the dials.

```ron
Tell( implies: Poor,
    template: "{name} counts the coppers twice before letting them go, and the cloak has been turned at the collar.",
    distinctive: 30,        // 0..100 — how loudly it points at the fact. KEEP LOW (15..45) by default.
    sense: Behaviour,       // Sight | Sound | Smell | Behaviour — how it is noticed
    visibility: 60,         // 0..100 — how easy it is to spot at all
    requires_knowledge: [], // tokens the observer must hold first ([] = none)
    mood_tint: {            // optional: colour by the OBSERVER's mood. {} = none.
        "pity":     " You looked away to spare {name} the seeing of it.",
        "contempt": " You had seen the trick before and were not moved.",
    },
),
```

Vary the *sense* and *distinctive* across a fact's tells so it never reads the same way twice. The
Waxen fact-kinds (none names the cosmology):

```ron
Tell( implies: Returned,    // Palingenesis — NEVER "he is one of the returned"
    template: "{name} thanks you by a name you never gave, and is too sure of the small true things.",
    distinctive: 35, sense: Behaviour, visibility: 55, requires_knowledge: [],
    mood_tint: { "dread": " You did not ask whose name it had been." } ),

Tell( implies: Aevum,       // a real thing — NEVER "this is an authentic relic"
    template: "It has a weight that argues with your hand and an edge that hasn't gone soft in all the years it should have. Set on the table, it makes the table look like the lie.",
    distinctive: 50, sense: Sight, visibility: 40, requires_knowledge: [], mood_tint: {} ),
```

`{name}`/`{other}`/`{place}` bind to real sim values — you never type a name. **Target: ~30 fact-kinds,
4–8 tells each.** The starter file seeds 12 kinds; deepen those and add more.

> **Dials, plainly:** `distinctive` low = oblique/Wolfean (the default); high (60+) = nearly stated
> (reserve it). A high-perception character notices *more* tells, never an *easier* one — so don't
> author "plain" versions for skilled readers (`prose_generation.md`).

---

## 2. Grammar — `assets/data/grammar.ron`

Two jobs. **(a) Dialogue lines** — add strings to a bucket named `act` or `act/affect`:

```ron
"mourn/grieving": [
    "He said the small true things to the last — the well-rope, the dull knife. I keep it sharp now. He'd have hated that.",
    "I keep setting two cups. The mist takes the habit slower than it took him.",
],
```
`#listener#`, `#speaker#`, `#referent#` are filled in by the engine.

**(b) Scene skeletons** — the sentence *shapes* a tell drops into (the tell fills `#detail#`):

```ron
"present_body": [
    "A {kind} is here. #detail#",
    "{name} keeps to the far side of the square. #detail#",
    "You are not alone: {name}, a {kind}, at work and not minding you. #detail#",
],
"exit_line": [   // six hex directions, described in-voice
    "A road runs {dir}, smoothed flatter than anyone cut it.",
    "{dir}, the hills pour into that even shape you don't walk into after dark.",
],
```

**(c) Helper rules** — reusable idiom any line can call as `#name#`:

```ron
"dimming": [
    "the lamps go grudging when his chair passes",
    "the year's a little shorter than it was",
],
"provenance_doubt": [
    "the toll-book always starts on a clean page, you ever notice",
    "it's always stood, they say — which is what they say of anything that matters",
],
```
(v2 will attach `when:`/`tags:` guards to productions; for now write the strings and leave a
`// guard: Mood("grief")>=0.5` comment if you have one in mind.)

---

## 3. Registers — `assets/data/registers.ron`

**What it is:** a *kind of drama* the director can run, plus the surface words for it.

```ron
(
    name: "forgetting",
    // mechanical flags — leave as-is and mark TODO(dev):
    spine: false, trunk: false, bright: false, seeds: None, casting: Warmest,
    // surface prose (yours):
    epithet_lead: "the Half-Remembered", epithet_other: "the One They Lose",
    situation_lead: "losing the thread of its own days, and pretending not to.",
    situation_other: "a name on every tongue and no face to hang it on.",
    noun: "a forgetting",
    told: "They say {lead} can't rightly tell you how long {lead} has kept the place.",
    quest_plea: "\"I had it. I had all of it. Help me find where it went before the mist has the rest.\"",
    quest_objective: "Recover what {giver} is losing to the fog.",
),
```
`{lead}`/`{other}`/`{giver}` fill with real people. **Write ~7:** loss, return, dimming, forgetting,
provenance, tallow, waking. The epithet is a *by-name*, never a title.

---

## 4. Intents — `assets/data/intents.ron` (mechanical; rarely added)

**What it is:** a thing an NPC can *mean* to say + the scoring for who'd want to. Add one only to
unlock a new voice, then give it a `grammar.ron` bucket to speak through.

```ron
(
    id: "a_slip_of_memory",
    act: "confide",
    tags: ["unease"],
    weight: 0.4,
    appeal: [
        (input: Mood("foreboding"), curve: Linear(m: 0.8, b: 0.1)),  // urge rises with unease
        (input: Trait("oblivion"),  curve: Linear(m: 0.6, b: 0.1)),
    ],
    moves: [ Stir(who: Speaker, mood: "foreboding", delta: 0.05) ],  // effect of saying it
),
```

---

## 5. Beats — `assets/data/beats.ron` (mechanical; draft, flag for dev)

**What it is:** a dramatic situation the director can stage — *who*, *when allowed*, *what it changes*.
No prose; it borrows words from its register.

```ron
(
    id: "one_came_back",            // Return — a known soul comes back wrong
    register: "return",
    tags: ["return", "personal", "dread"],
    phases: [Climax],               // Setup | Rising | Climax | Fall
    tension: 0.6, stakes: 0.7,
    cast: [Protagonist, Bystander], // the returned one + someone who knew them
    pre: [Exists(who: Bystander)],
    effects: [
        Stir(who: Bystander, mood: "dread", delta: 0.4),
        Reveal,                     // surfaces the fact the tells then leak
    ],
),
```
Roles / `pre` / `effects` must match the real enums (listed in the file header). Draft it, mark
`TODO(dev): validate effects`, confirm in a compiler session.

---

## 6. Bestiary — `assets/data/bestiary.ron` (names; eeriness comes from a tell)

```ron
(name: "pale strider", diet: Herbivore, form: Strider, habitat: ["moor"],
 min_temp: 1.0, max_temp: 9.0, size: 1.2, fecundity: 0.6, gregarious: 0.8, color: (0.78, 0.79, 0.74)),
```
The description is generated from tells over its facts — so for eeriness, add a tell in `tells.ron`:
`implies: WrongFauna, template: "It is too quiet for its size, and the eyes don't track the way an animal's should."`

---

## 7. Goods / Norms — `assets/data/goods.ron`, `norms.ron` (mechanical, lowest priority)

```ron
// goods.ron — lamp-oil is the Penury currency; needs a recipe to enter the economy
(name: "lamp-oil", base_price: 40, target_stock: 18, nutrition: 0.0),

// norms.ron — the local Unsayable (taboo to name here)
(act: "name-the-lake", modality: Forbidden, weight: 1.0, defiance: Some("gnosis")),
```

---

## The 80/20 for a flight

If the flight is for one thing: **tells (§1)** and **grammar scene-skeletons + dialogue (§2)**.
Registers (§3) are a quick win. §4 down are mechanical — draft loosely, mark `TODO(dev)`, and don't let
a mechanical question stall the writing. Run every line through `style_guide.md` §9 before you keep it.
