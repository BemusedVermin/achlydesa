# The Waxen World — Style Guide

> How to write *anything* the player reads in achlydesa: scene prose, dialogue lines, rumor,
> the symptom that implies a relic, a single overheard aside. Read `waxen_world.md` (the canon)
> first, then this, then `content_catalog.md` (the worklist). When a line you're authoring and this
> guide disagree, the **Voice Gallery in `waxen_world.md` §I wins** — those seven passages are
> the tuning fork. Read one aloud before a writing session.
>
> **This guide is the *voice* companion to the v2 architecture in `docs/prose_generation.md` (how
> prose is generated — guarded grammar + Wolfean tells, no LLM, never false) and
> `docs/text_interface.md` (the parser front-end). It does not override them; it tells you how the
> fragments and tells they call for should *sound*.** Note the consequence: you are not writing
> finished passages, you are writing **fact-keyed fragments** (a *tell* = one oblique symptom of one
> true fact) that the engine assembles per scene. The craft below is exactly the craft of writing a
> good tell.

---

## 1. The one rule

> **Why this is *safe*, not just stylish (the v2 justification):** the engine never states a falsehood
> because it only ever emits a fragment whose underlying fact is true (`prose_generation.md`). The
> Wolfean voice rides on top of that as a Gricean move — **flout Quantity, never Quality**: say *less*
> than the whole truth (omit the bald fact), but everything you *do* say is literally true. Hemingway's
> iceberg works because the simulation genuinely holds the seven-eighths underwater. So the voice and
> the truth-guarantee are the same discipline: imply by omission, never by invention.

**The narrator is native to the Cerement and finds nothing strange.** Everything else here is a
consequence of that one fact. The world is a forgery — a waxen imitation of a real world, kept
asleep — and the people in it are dreaming. The teller does not know this and never will. So:

- The wrongness is **never named**. It arrives on an *idiom*, a *habit*, a *custom obeyed without
  reason*. The reader assembles the horror the narrator never sees. (A man's hand "wouldn't take
  the lamplight right, gave it back too smooth" — we are never told he has tallowed.)
- The teller **explains nothing supernatural**, because to them it isn't. They explain the *mundane*
  — the fare, the custom, the toll — with the over-precision of someone managing a world that
  doesn't quite hold. That misplaced precision *is* the dread.
- The reader is always **one inference ahead of the narrator, never behind.** If you find yourself
  spelling out the secret, cut back until the reader has to lean in.

This is the difference between weird fiction and horror: we don't describe the monster. We describe
the carter turning the mule without a word, and let the reader feel the shape of the thing in the
even, dreamless hills he won't drive into after dark.

---

## 2. Voice — the defaults

| Dimension | Default | Notes |
|---|---|---|
| **Person** | second person ("you paid him full fare") | The player *is* the teller's "you." Drop to third for gossip *about* others ("Vhessa's boy came back"). |
| **Tense** | past | "The road got generous." Even present-moment description leans on the habitual past ("the lamps along the Procession *sinking* low as his chair went by"). |
| **Register** | plain, worn, lived-in | The vocabulary of a carter, a clerk, a digger — not a poet. Beauty comes from *rhythm and juxtaposition*, never from ornate words. |
| **Stance** | unsurprised, practical, a little tired | The teller has seen this before and has rules for it. Dread is delivered deadpan. |
| **Knowledge** | partial, hedged, forgetful | "The toll-book went back eleven years and then stopped." Memory frays at the edges — this is Lethe, and it should *feel* like ordinary forgetting. |

**The Wolfean move** (after Gene Wolfe): a sentence states a plain fact, then qualifies it with a
clause that quietly reveals the world. *"She gave him bread through the bars and did not open the
gate."* The first half is kindness; the second half is everything. Lead with the ordinary act, let
the revealing clause land last.

---

## 3. Techniques (the toolbox)

1. **The custom-without-reason.** People do precise, strange things "because that was the custom and
   because he was right to." Give the rule, withhold the why. The withheld why is the tell.
2. **The habitual betrays the supernatural.** "The way they did when you hadn't been thorough" — a
   *way they did*, a recurring pattern, tells us the returned dead are common here without one word
   of exposition. Reach for "the way X did," "as X always," "you learned not to."
3. **The over-true detail.** A flagged person is "slightly too accurate about your business." Truth
   that is *too good* reads as wrong. Use exactness as a horror instrument.
4. **The material tell.** The supernatural leaves a residue on *objects and bodies*: a hinge with "a
   weight that argued with your hand," a hand that "gives the light back too smooth." Anchor the weird
   in something you could touch.
5. **The thing that makes everything near it look thin.** Authentic Aevum-relics, real things in a
   forged world, "make the table look like the lie." Realness is the anomaly here, not unrealness.
6. **The acquired wisdom, stated as resignation.** "You learned not to ask whose name a person used
   when they thanked you." A lesson the teller has internalized = a rule of the world, delivered as
   weary common sense.
7. **The unfinished forgetting.** Let memory and provenance *trail off*: a toll-book stops at "a clean
   first page... in a hand no one living wrote." Don't resolve it. Lethe doesn't.
8. **Economy of dread.** One precise wrong detail per passage. Two competes; three is a haunted house.
   The carter passage has exactly one: the dreamless shape of the hills. Trust it.

---

## 4. Hard "don'ts"

- **Don't name the cosmology to the player.** Never write *Lethe*, *Penury*, *Palingenesis*, *Aevum*,
  *the Caryatids*, *Scintilla*, *tallowing*, *Corollary*, *the forgery*, *the Demiurge* in player-
  facing text. These are **authoring labels**, for the catalog and code only. (See §6 for what the
  *characters* call these things instead.)
- **Don't explain the rules.** No "because the dead return when not properly burned." The teller would
  never say it; they'd say "the way they did when you hadn't been thorough."
- **Don't editorialize the horror.** No "eerily," "unnervingly," "with a chill," "something deeply
  wrong." If you reach for an adverb of dread, you've under-written the noun. Replace the adverb with a
  concrete habit.
- **Don't let the teller be surprised.** Astonishment breaks the native stance. The teller adjusts,
  pays the fare, sweeps the step, and carries on.
- **Don't over-poeticize.** No purple. "The mist moves the way sleep does" works because it's plain and
  exact. "Gossamer tendrils of somnolent vapor" does not. Cut every word a tired carter wouldn't use.
- **Don't resolve provenance.** The age of the bridge, the author of the toll-book, where the
  good company came from — leave the seam open. The game *is* the player reading seams; never close
  one for them.
- **Don't make the player the chosen one.** They are "one small actor, not the protagonist of a
  scripted epic" (`docs/gameplay_targets.md`). The Scintilla stirs; it does not anoint.

---

## 5. The forensic contract (why the voice and the mechanics are the same thing)

The game's core verb is **reading the seams of the forgery** — the Perception Layer surfaces *tells*,
and the player renders *verdicts* (is this ruin authentic, is this person returned, is this faction a
threat). The prose voice exists to **serve that verb**: every passage should carry exactly the readable
material the player needs, dressed as the teller's ordinary observation.

- A **tell** is the formal unit of this game's prose (`prose_generation.md` §"The Wolfean layer"): a
  `Tell` is one *oblique symptom* of one true fact — a concrete detail the player can notice or pass
  over (the too-smooth hand, the clean first page, the lamps sinking low). You author tells into
  `tells.ron`; each carries a `distinctive` dial.
- **Oblique by default — keep `distinctive` low.** The reader must do the inference; a high
  `distinctive` tell is nearly a statement, so reserve it. A high-perception character notices *more*
  tells (a denser web of implication), never an *easier* one — so do not author a "plain" version of a
  tell for skilled readers. Reading is never trivialised.
- Never *flag* a flagged thing — **leak it through ordinary self-presentation** (`waxen_world.md` §E).
  The innkeeper isn't "described as Returned"; the tell is "slightly too accurate about your business,
  and cannot tell you how long he has kept the place."
- **Focalise.** A tell is only surfaced if the observer could plausibly perceive it (co-located, has
  the prerequisite knowledge, the symptom is visible enough). Write the symptom as something *this*
  body, in *this* place, would actually notice — and tint it through their mood (free indirect
  discourse: the same fact reads differently to an anxious vs. a gloating observer).
- **Fidelity is in the prose — and it traces to a real record.** Rumor, gossip, and corrupt text are a
  legitimate second tier (`prose_generation.md` §"The distortion tier"), but **every falsehood derives
  from a real fact** — a garbled rumor, a worn slate, a half-memory; never invented lore. So when a
  claim is second-hand or Lethe-thinned, the prose hedges ("they say," "the toll-book went back eleven
  years and then stopped," "you'd have sworn you'd never met her") and the *distortion lives in the
  record, not in your prose*. High-fidelity (you witnessed it) prose is flat and certain. Write so the
  player can **triangulate** — compare frayed accounts and recover what truly happened.

If a line reads beautifully but gives the player nothing to *read*, it's set-dressing. If it gives a
tell but breaks the native stance, it's a tutorial. The target is both at once.

---

## 6. Diction bank — what the characters say instead

The cosmology has authoring names (§4) and **folk names** — the worn idiom the dreaming use for things
they half-feel. Player-facing text uses only the folk register. This is also the existing in-repo
vocabulary (`assets/data/grammar.ron` already speaks of *the fog*, *the wheel*, *the seven*); keep it
and extend toward the Waxen canon rather than replacing it.

| Authoring concept (canon) | Folk idiom the teller uses (player-facing) |
|---|---|
| The forgery / the Cerement (the false world) | "this fog," "the way things are," "the world as it's kept" |
| Lethe (engineered amnesia) | "the mist took it," "the fog closes in around it," "it went back eleven years and then stopped" |
| Penury / the Dimming (heat & abundance dwindling) | "the cold," "the lamps sinking low," "the year going a little shorter" |
| Palingenesis (the returned dead) | "came back," "the come-back-wrong," "you hadn't been thorough" |
| A Castellan (a concealment-lord) | "the one they called the Penurious," "his lordship," a local by-name |
| The Seven Caryatids (the seven laws) | "the seven," "what holds it up," "the archons made a fine imitation of a sky" |
| Aevum-relic (a real thing) | "a thing that real," "it made the table look like the lie," "it didn't go soft when it should have" |
| Tallowing / extenuation (sorcery that smooths the self away) | "the soft hand," "a candle's hand where a man's should be," "they don't last, that trade" |
| The Asymptote / thinning (thin places) | "the thin places," "that even, dreamless shape," "the low ground" |
| Scintilla (a waking soul) | "you'd begun to remember," "you woke a little," never named directly — show it as the teller noticing what others don't |
| The Matins / waking (the endgame) | "waking," "the grey hour," "if it ever ends" — kept distant, mythic, mostly unspoken |

> Note on the existing grammar: the repo's `#world_aside#`, `#fog_phrase#`, `#mist#`, `#oath#` helper
> rules already establish *fog / wheel / the seven / the archons*. That is the live folk register —
> author into it. The new Waxen terms (Cerement, Caryatid, Castellan, tallow) are **authoring-side
> precision**; they sharpen what the fog *means*, they do not become words a character says.

---

## 7. Worked transforms (before → after)

Train your ear on these. Each "before" is a plausible first draft; each "after" obeys the guide.

**Exposition → tell.**
- ✗ *Long ago the empire fell, and now only haunted ruins remain, their magic still potent.*
- ✓ *The old families paid stupid money for pieces dug out of the keep and kept them behind glass and wouldn't say why.*

**Named cosmology → folk idiom.**
- ✗ *The Castellan of Penury passed, draining warmth as he went.*
- ✓ *The lamps along the Procession sank low as his chair went by and came back grudging after, the warming-beggars gone to other walls.*

**Editorialized dread → concrete habit.**
- ✗ *The dead man stood at the gate, unnervingly calm, and she felt a deep wrongness.*
- ✓ *He stood at the gate saying the small true things he'd always said. She gave him bread through the bars and did not open the gate.*

**Surprise → native stance.**
- ✗ *You couldn't believe the bridge had no record of when it was built!*
- ✓ *You asked how long the bridge had stood and she said it had always stood, which is what they all said about anything that mattered.*

**Resolved provenance → open seam.**
- ✗ *The hinge was clearly a relic of the true world, the only real thing for miles.*
- ✓ *It had a weight that argued with your hand and an edge that hadn't gone soft in all the years it should have. Set on the table, it made the table look like the lie.*

---

## 8. Length & rhythm targets by surface

| Surface | Length | Voice notes |
|---|---|---|
| **Room / location prose** (first look) | 2–4 sentences | One establishing image + one tell + one custom or habit. Second person, present-habitual. End on the seam. |
| **Room re-look / brief** | 1 sentence | The bare scene; no fresh tell unless state changed. |
| **Relic / item description** | 1–3 sentences | Lead with the material tell (weight, edge, light). The realer it is, the wronger it reads. |
| **Dialogue line** (grammar) | 1 short utterance | Spoken, not narrated. Idiom over grammar. Mood colors word choice, not punctuation. (See `content_catalog.md` §A for slots.) |
| **Overheard rumor / gossip** | 1–2 sentences | Hedged ("they say"), fidelity-graded. The lower the fidelity, the more it frays. |
| **Beat / scene framing** (register prose) | 1 line each (epithet, situation, told, plea) | Compressed. The epithet is a by-name, not a title. |
| **Codex / lore fragment** (if surfaced diegetically) | ≤ 1 short paragraph | A found document, a thing someone said once. Never a narrator's history lesson. |

Default to **shorter**. The Voice Gallery passages are long because they're showpieces; in-game, a
location is three sentences and a held breath.

---

## 9. The checklist (pin this above the keyboard)

Before you commit a line, run it:

1. Would a tired carter say it this plainly? (kill the purple)
2. Is the cosmology *named*, or *leaked*? (leak it)
3. Is there exactly **one** wrong thing here? (not zero, not three)
4. Is the teller surprised? (they shouldn't be)
5. Did I explain *why*? (don't — give the custom, withhold the reason)
6. Can the player *read* something from this? (a tell, a verdict, a seam)
7. Did I close a seam I should have left open? (re-open it)
8. Lead with the ordinary, land the revealing clause last? (Wolfean shape)
