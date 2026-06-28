# Procedural prose generation (no LLM, never false)

Status: **design** (2026-06-27). Companion to [`text_interface.md`](text_interface.md), which
covers the terminal front-end the prose is rendered into. This doc covers *how the world is put
into words*. It is the heart of the text conversion.

## The problem

We are turning achlydesa into a text-heavy, Zork-like game over a living, MUD-like simulation.
The world model is solved already: agents have goals (IAUS + GOAP), personalities (curated motive
traits + named-emotion moods), relationships, factions, memory, a hidden narrative director, and a
Story Sifter that records what actually happened. **The only missing layer is prose** -- turning
that sea of typed facts into descriptive English a player reads.

Two constraints make this hard, and together they rule out the obvious tools:

1. **No LLM. The prose can NEVER hallucinate or state a false thing.** Not "rarely"; never. A
   sentence the player reads must correspond to a fact the simulation actually holds. This
   disqualifies neural text generation (it asserts plausible-but-unsourced claims by construction)
   and also disqualifies Markov / n-gram generation, which recombines fragments into propositions
   that appeared in *neither* source (the Mark V. Shaney failure mode). Statistical text models
   optimise word-sequence plausibility, not truth; they have no representation of a proposition to
   check. They are out.
2. **The fact-space is open.** Facts emerge from the simulation; you cannot pre-enumerate them, so
   you cannot hand-write a sentence for every possible fact. A pure hand-authored corpus (one
   passage per situation) does not scale to an emergent world.

The resolution -- confirmed by the prior art (see [Prior art](#prior-art)) and chosen by the
owner -- is a **generative grammar that assembles authored fragments, driven by structured sim
facts**. Authors write a *finite* library of fragments keyed to fact *shapes*; the engine selects
and assembles them against the *infinite* set of fact *instances*. Truth is then a **structural
property, not a runtime check**: every word is either an authored constant or a slot filled from a
real sim value, and a fragment can only be chosen when its precondition over the live sim state
holds. There is no stage whose job is to originate a proposition, so a falsehood has no way in.

This is the same split the dialogue layer already uses (`docs/dialogue.md`): **meaning is
simulation, surface is generated.** We are generalising that split from one-line utterances to all
descriptive prose -- scenes, NPCs, world events, the Wolfean implications below.

## The pipeline (classical NLG, because it cannot lie)

We adopt the consensus data-to-text architecture (Reiter & Dale). Each stage is a pure function of
the previous stage's output; content enters only at the front, from the sim:

```
sim facts
   |
   v
(1) CONTENT SELECTION   pick which true facts to mention (salience). Output: a small set of
                        Messages, each a typed proposition drawn ONLY from sim state.
   |
   v
(2) MICROPLANNING       lexicalise (fact -> which fragment), aggregate (merge facts that share a
                        subject), refer (how to name each entity -- see Referring expressions).
   |
   v
(3) REALISATION         a thin morphology pass: articles (.a), plurals (.s), tense (.ed),
                        capitalisation, spacing. Renders a fully-specified line; invents nothing.
   |
   v
prose
```

The firewall is stage (1): the grammar in stages (2)-(3) can only ever see *selected* facts, so it
literally cannot mention a fact that was not selected, and selection only ever *picks from* what is
true. This is why the architecture is safe by construction rather than by vigilance.

We already have stage (3) in embryo: `Grammar::expand` in `agent_core/src/dialogue.rs:889`
recurses `#symbol#` productions and fills `{speaker}/{listener}/{referent}` slots. The work is to
(a) add guards + tags to productions so selection is fact-driven, (b) add the salience stage in
front, (c) add the morphology modifiers, and (d) add a discourse model for referring expressions.

## The grammar, hardened

Today: `pub struct Grammar(HashMap<String, Vec<String>>)` -- a blind Tracery expander. It picks a
production uniformly at random with no knowledge of world state, so on its own it *can* assert
false things ("it is raining" on a clear day). That is fine for the dialogue floor, where every
slot is pre-grounded by the utterance plan, but a description grammar reads the world directly and
must be guarded. We evolve the production from a bare string into a **guarded, tagged fragment**
(Improv's model on a Tracery substrate; Expressionist's tag semantics):

```
// A production is one authored fragment, optionally gated and labelled.
Production {
    template: String,        // Tracery-style: literal text + #symbol# recursion + {slot} fills
    when:     Vec<Guard>,    // ALL must hold against the FactContext, else this rule is INELIGIBLE
    tags:     Vec<Tag>,      // accumulate up the derivation; satisfy content requests; feed dryness
    weight:   u16,           // relative frequency among eligible rules (default 1)
}

// A Guard is a predicate over real sim facts -- the truth gate. Examples:
//   Mood("grief") >= 0.5 | Trait("vengeance") >= 0.6 | Relation(Feud, subject, other)
//   Season(Waning) | Knows(observer, token) | Present(subject) | TimeOfDay(Dusk)
// Guards never mutate state; they only read it.

// A Tag is author-defined markup: implies:feud, register:grim, sense:sight, topic:harvest ...
```

A bare string production (the entire current `grammar.ron`) is just a `Production` with no guards,
no tags, weight 1 -- so the existing grammar keeps working unchanged. Guards/tags are additive.

### Selection per symbol (the four-step gate)

When the expander needs to rewrite a symbol it does **not** pick uniformly. It runs:

1. **Filter (truth gate).** Drop every production whose `when` guards do not all hold against the
   current `FactContext`. What survives is, by definition, true to say right now. This is the
   no-hallucination guarantee, localised to one choice point. (Improv `mismatchFilter` returning
   "exclude".)
2. **Dryness (anti-repetition).** Drop (or down-weight) productions used too recently, tracked by a
   small ring of recent tag-sets. Defeats the "samey" feel at the seam. (Improv `dryness`.)
3. **Specificity rank.** Prefer the production matching the *most* guards / requested tags -- the
   most specific true thing -- with broad guard-free productions as guaranteed fallbacks, so there
   is always something true to say. (Emily Short's salience-as-specificity; Improv `fullBonus`.)
4. **Seeded pick.** Choose among the top tier by `weight`, using the dedicated prose RNG stream
   (below), so the result is deterministic.

### Content requests (top-level meaning)

A description is requested by *meaning*, Expressionist-style: "give me a line that
`implies:feud` and is `register:grim`, and must not be `register:gloating`." The request carries
`required` and `prohibited` tag sets; a generated line satisfies it iff its accumulated tags are a
superset of `required` and disjoint from `prohibited`. Because tags accumulate up the derivation,
the finished line ships with machine-readable meaning -- which the sim can react to with zero NLP
(e.g. the journal can auto-file a line under the feud it was about).

### Slots are state-bound terminals

Every `{slot}` resolves to a typed sim value (a name, a place, a count, an epithet) or the
production fails and we backtrack to another. A slot is **never** a free generator. This is the
other half of the truth guarantee: the open, un-enumerable part of the fact-space (the actual
names, numbers, entities) enters only through slots bound to real values, while the finite authored
part (the phrasings) is what the grammar varies. You author a closed grammar over an open world.

## Salience: what to say, and what to leave out

A live scene holds far more true facts than you would ever narrate. Saying all of them is the
fastest way to sound like a database dump. Content selection ranks and caps:

- **Importance score per fact** (BabyTalk): authored base importance by fact kind, scaled by
  magnitude/severity. Two-sided thresholds: above HIGH -> must mention; below LOW -> never mention;
  the middle band competes for a bounded number of slots.
- **Three axes of "worth saying"** (Emily Short): *mechanical* (does it change what the player can
  do?), *narrative* (does it set stakes/motive/consequence -- e.g. director or Sifter relevance?),
  *legibility* (does it help the player understand why something happened?).
- **Surprise / novelty.** Skip the mundane, surface the noteworthy: rarity (`-log p`), deviation
  (z-score), and -- best -- "how much does this move the observer's beliefs" (Bayesian surprise),
  so a trusted ally turning hostile outranks a stranger's ordinary grumbling even though "hostile"
  is common. The Story Sifter's interest scoring already does a version of this ("Select the
  Unexpected": rank chronicle matches by ascending property likelihood); reuse it.
- **What changed since the player last looked** is itself high-salience -- the spine of the
  "while you were travelling..." event feed (see `text_interface.md`).

### Scene assembly: mention-once

Tile/scene descriptions follow Inform 7's room-description model: compute a salience priority per
present thing (NPC, feature, readable, fauna), describe in priority order, and **flag each entity
"mentioned" once its name is printed** so it is not repeated by a later catch-all. First visit gets
the full description; revisits get a terse heading plus only what changed (a per-tile "described
before" flag, which we already half-have via the fog/`Known` model).

## The Wolfean layer: imply, do not state

The owner wants scenes described the way Gene Wolfe writes -- the narration never announces the
obvious fact, it lets a casual, concrete detail *imply* it, and an attentive reader assembles the
truth. This is not decoration; it is the house style, and it must be procedural and never false.

The literary principle that makes this safe: **flout Quantity, never Quality** (Grice). We say
*less* than the whole truth -- we omit the bald fact -- but everything we *do* say is literally
true. Hemingway's iceberg works here precisely because the simulation genuinely holds the
seven-eighths we leave underwater, so the omission is sound, not hollow. And Wolfe's own rule is
that it must stay *solvable*: the attentive player can always reconstruct the fact.

### Mechanism

For each fact *kind* we author a library of **tells** -- oblique surface fragments that the fact
*entails* but that never name it:

```
Tell {
    implies:            FactKind,            // the true fact this obliquely conveys
    template:           String,              // grammar fragment; a SYMPTOM, never the diagnosis
    distinctive:        u8,                  // 0..100: how uniquely this detail is caused by the fact
    sense:              Sense,               // Sight | Sound | Smell | Behaviour -- how it is noticed
    visibility:         u8,                  // 0..100: how plainly observable the symptom is
    requires_knowledge: Vec<Token>,          // observer must already know X to read this tell
    mood_tint:          Map<Mood, String>,   // free-indirect colouring by the focal observer's mood
}
```

Example. The fact is `Poor(npc)`. We never emit "He is poor." A tell might be
`"#he# counts the coppers twice before #he# lets them go, and #his# cloak has been turned at the
collar"` -- two concrete, true symptoms (the sim knows the purse is near-empty and the garment is
old) from which the player infers poverty.

The per-scene algorithm, given a focal observer `O` and the set of true facts `F`:

1. **Focalise / gate.** Keep only facts `O` could plausibly perceive: `O` is co-located or has the
   knowledge downstream of it; the tell's `visibility` clears `O`'s perception; `O` satisfies
   `requires_knowledge`. Everything else is simply not noticed. (Genette's internal focalisation;
   Hemingway's omission; this is also *per-observer subjective knowledge*, the Talk-of-the-Town
   guarantee -- `O` surfaces beliefs, not omniscient truth, so even a mistaken `O` reports
   truthfully *about its own mind*.)
2. **Score salience** over the survivors (above).
3. **Keystone + suppress.** Take the top-K salient facts (K small, often 1). For each, pick **one**
   tell, ranked by `distinctive` and fit to a `sense` `O` has available. Emit that single detail;
   withhold the fact and the other tells. (The "telling detail": one charged particular, the rest
   suppressed.) Mark the surfaced entity "mentioned".
4. **Realise** the tell through the guarded grammar with the tag `implies:F` and the
   `mood_tint` branch for `O`'s current mood (free indirect discourse -- the same fact reads
   differently through an anxious vs. a gloating observer).

`distinctive` is the dial between mystery and clarity: a low-`distinctive` tell is more ambiguous
(more Wolfean, more inference hops); a high-`distinctive` tell is nearly a statement. **The default
sits oblique -- the reader must be challenged, and reading is never trivialized** (owner, 2026-06-27).
The perception/lore RPG skill does **not** turn the dial toward clarity: it *widens* what an attentive
avatar notices -- surfacing *additional* tells, opening *more* lore threads, implying *more* -- without
making any single fact plainly stated. A high-perception character reads a richer, denser web of
implication, not an easier one; the puzzle of any given fact stays as oblique as ever, and the RPG can
never short-circuit the act of reading. (The URR riddle technique -- distance between what you know and
what you must infer -- is preserved in depth, not shortened.) Background facts of low salience can drop
to a flat one-line *tell-by-summary* so the *shown* beats stand out (the show/tell ratio discipline).

This is the same engine as the rest of the prose layer -- a guarded grammar over true facts -- with
two extra moves bolted on: *focalisation* (filter to what the observer notices) and *indirection*
(pick the symptom fragment, not the fact fragment). Nothing here can state a falsehood, because a
tell is only eligible when its `implies` fact is true.

## The distortion tier: rumor, gossip, and corrupt text

Decided 2026-06-27: there **is** a second tier for diegetic falsehoods -- but under one iron rule,
**every falsehood must derive from a real fact. No entirely fabricated lore.** A rumor that gets the
details wrong, a half-legible inscription, a piece of malicious gossip -- these are allowed and wanted,
because they are *true facts about a mind or a degraded record*, not inventions of the renderer. This
is the same guarantee Talk of the Town gives: a lie or a misremembering is a **tracked data structure**
(a belief with provenance), faithfully surfaced, never a hallucination.

The mechanism is one model on two timescales:

- **Gossip / rumor (short timescale).** A claim that propagates between agents, garbling as it goes.
  We already have the seed of this: `overheard()` returns gossip "fidelity-garbled by distance/age",
  and the dialogue layer keeps a per-soul memory ring. We make distortion first-class: a rumor is a
  **source fact + a bounded transformation + provenance** (who said it, when, how many mouths it has
  passed through). Transformations are *truth-preserving in origin, lossy in detail*: value mutation
  along an authored belief-mutation graph (Talk of the Town -- "brown" is likelier to drift to "black"
  than to "white"), partial knowledge (drop a qualifier), misattribution (right deed, wrong person),
  exaggeration (scale a magnitude). Garble rises with social, spatial, and temporal distance.
- **Corrupt / ancient text (long timescale).** The same transformation pipeline run with heavy
  degradation and age: a ruined book, a worn slate, a dream-fog half-memory. It still points at a real
  historical fact in the chronicle/Sifter; it is just eroded.

Crucially, the *grammar still assembles the surface exactly as for truthful prose* -- the "falsehood"
lives entirely in the rumor/belief **data**, never in the text engine. So there is still no renderer
hallucination: we render a distorted *record*, and the record traces to truth. A fabricated claim with
no source fact is forbidden by construction (a distortion needs a `source` to exist).

This is a feature, not a hazard: because rumors disagree and each traces to truth, the player can
**triangulate** -- compare accounts, weigh provenance, and recover what actually happened. That is the
detective pleasure the Wolfean surface is already cultivating, extended to the social layer. It also
keeps faith with the curationist principle: we *recount* records (even degraded ones); we never invent.
Determinism and the dedicated RNG stream apply to distortion exactly as to truthful prose.

## Referring expressions (naming entities truthfully over time)

To vary how an entity is named -- "Aldric" / "the smith" / "the wounded smith" / "he" -- without
ever being ambiguous or wrong, keep a small **discourse model** (a resource): per entity ever
mentioned, its gender/number, a salience that boosts on mention and decays per sentence, its last
mention, and its last grammatical role; a scene boundary resets pronoun licensing.

- **Form by salience** (Givenness Hierarchy): in focus -> pronoun; uniquely identifiable -> "the
  smith"; first mention -> full name / "a smith".
- **Epithets are distinguishing descriptions, never decoration** (Dale & Reiter incremental
  algorithm): build "the smith" / "the young smith" by walking the entity's *true* attributes in a
  fixed preference order, adding only attributes that rule out another entity currently in scope.
  Because every attribute is read from the entity's own components, the epithet is true by
  construction -- "the smith" is licensed only if the sim says smith.
- **Pronoun safety** (the inverted-resolver test): before emitting "he"/"she"/"they", run a
  deterministic resolver (most-salient matching antecedent wins) and emit the pronoun *only if it
  would resolve back to the intended entity*; otherwise fall back to name/epithet. Plus the cheap
  conservative rule: no pronoun if another in-scope entity shares its gender/number. This makes a
  wrong pronoun structurally impossible. Repeat the name when a pronoun is unsafe -- never rotate
  synonyms for variety's sake ("elegant variation" causes ambiguity).

## Avoiding "10,000 bowls of oatmeal"

Combinatorial uniqueness is worthless; *perceived* uniqueness is the goal (Compton). Concrete
disciplines, all cheap and deterministic:

- **Vary structure, not just fillers.** Author many sentence *skeletons* per fact shape and choose
  among skeletons; ten skeletons x five fillers reads as far more varied than one skeleton x fifty.
- **Surface process / cause.** The sim knows *why* a fact holds (a feud, a drought, a betrayal).
  Surfacing the cause beside the fact reads as a living world, not noise -- "the pushed-up soil at
  the base of the tree."
- **Signature detail ("barnacling").** Give each entity one or two standout, sim-derived epithets
  (the scarred one-eyed smith) and let them recur, rather than spreading uniform adjectives.
- **The Venom rule.** Vary the *meaning-bearing* word (the crime, the betrayal, the stake), not
  synonyms of the verb; and cap independently-varied slots at <=2 per sentence and <=3-4 per
  passage, or the text reads as nonsense.
- **The corpus is the product.** Most of the authoring effort and quality lives in the fragment
  library, not the engine. Budget for it; lint it (see below).

## Determinism and isolation (the non-negotiables)

The prose layer obeys every existing invariant:

- **Dedicated derived RNG stream.** All fragment-selection randomness comes from
  `seed ^ <distinct prose constant>`, never from an existing stream. Identical facts -> byte-
  identical prose, and adding the prose layer perturbs no other layer's stream.
- **Off-by-default, byte-identical, isolated.** Like every other layer, prose keeps its state in
  its own resource and early-returns when disabled; a run with prose off is byte-identical in sim
  state to one with it on. (Prose is a *view*; it never feeds back into simulation state -- exactly
  the rule the SLM seam already obeyed.)
- **Retire the SLM / `voice` path.** The optional on-device model that re-voiced grounded lines
  (`agent_core/src/dialogue.rs:936`, the `voice` crate) is **incompatible with the new mandate**:
  an LLM rephrasing can distort or invent, which the "never false" rule forbids, and the owner has
  ruled out LLMs. The guarded grammar + the Wolfean layer become the sole surface. `TextGen`,
  `SlmRealizer`, `build_prompt`, and the `voice` crate are removed from the build (see
  `text_interface.md` for the workspace change). The grammar was always the floor; it is now the
  whole house.

## Data-driven content

Following the `assets/data` convention (content lists, parsed by the owning crate, baked via
`bundled()`):

- `assets/data/grammar.ron` -- extended to the guarded/tagged production form (back-compatible).
- `assets/data/tells.ron` -- the per-fact-kind Wolfean tell libraries (new).
- Fact-kind base importances and tell tuning live as data, retunable without recompiling.

A `#[cfg(test)]` provenance lint is worth building early (the Caves of Qud "colour by origin"
trick): tag each fragment as authored-constant vs. state-derived vs. random-branch, and assert that
no production asserts a proposition not present in its `FactContext`. That turns "never false" from
a hope into a test. A second lint enforces the Venom cap (<=2 varied slots/sentence).

## Build order (suggested)

1. Morphology modifiers (`.a/.s/.ed/.capitalize`) on the existing `expand` -- safe, immediately
   useful, can never hallucinate.
2. Guards + tags + the four-step selection gate over `Grammar`; keep `grammar.ron` working.
3. The discourse model + referring expressions (epithets, pronoun safety).
4. Content selection / salience for scene assembly (mention-once, first-visit/revisit).
5. The Wolfean tell libraries + focalisation, as `tells.ron` + a `prose` resource.
6. Provenance + Venom lints.

Each step is independently testable against the determinism harness (same seed -> identical prose).

## Resolved (owner, 2026-06-27)

- **Two tiers, both truth-derived.** A truthful tier plus a distortion tier (rumor/gossip + corrupt
  text) -- but no entirely fabricated lore; every falsehood traces to a real fact (see
  [The distortion tier](#the-distortion-tier-rumor-gossip-and-corrupt-text)).
- **Oblique by default; the reader is challenged.** The perception/lore skill *widens* what is
  noticed (more tells, more threads), it never makes a fact plainly stated and never trivializes
  reading (see the `distinctive` discussion above).

## Open questions (for the owner, before implementation)

1. **Crate placement.** The prose engine extends `agent_core`'s grammar and reads sim facts. Does
   it stay in `agent_core` (alongside dialogue), or become its own `prose` crate that `agents`
   wires in like the other feature crates? (Leaning: stays in `agent_core` -- it is the same
   grammar -- with the front-end-specific assembly in the new `tui` crate.)
2. **Distortion provenance depth.** How much rumor provenance to track for triangulation -- just
   source + garble level (cheap), or a full who-told-whom chain (richer detective play, more state)?
   The dialogue memory ring already gives us a starting point.

## Prior art

The research these decisions rest on, with the load-bearing sources.

**NLG pipeline / why grammars cannot hallucinate**
- Reiter & Dale, *Building NLG Systems* (2000); Gatt & Krahmer, *Survey of the State of the Art in
  NLG*, https://arxiv.org/abs/1703.09902 -- the content-determination -> microplanning ->
  realisation split; truth is structural because content enters only at the front.
- Reiter, "Generated Texts Must Be Accurate!" https://ehudreiter.com/2019/09/26/generated-texts-must-be-accurate/
  and "Problems with Rule-Based NLG" https://ehudreiter.com/2022/01/26/problems-with-rule-based-nlg/
  -- rule-based systems "generate only what their rules explicitly specify."
- Dale & Reiter, *Gricean Maxims in REG* (1995), https://arxiv.org/abs/cmp-lg/9504020 -- the
  incremental algorithm for truthful referring expressions.
- Why Markov is disqualified: Mark V. Shaney, https://en.wikipedia.org/wiki/Mark_V._Shaney ;
  "stochastic parrots" (form without meaning), https://en.wikipedia.org/wiki/Stochastic_parrot .

**Expansion grammars + state + variety**
- Compton, *Tracery* (ICIDS 2015) and "So you want to build a generator..."
  https://galaxykate0.tumblr.com/post/139774965871/so-you-want-to-build-a-generator -- the
  substrate, modifiers, and the 10,000-bowls-of-oatmeal problem.
- Dias, *Improv*, https://github.com/sequitur/improv -- model-backed grammar; tag groups + a filter
  chain that hard-excludes mismatches and ranks the rest (the architecture we adopt).
- Ryan et al., *Expressionist* (ICIDS 2016), https://github.com/james-owen-ryan/expressionist and
  *Curating Simulated Storyworlds* (PhD, 2018), https://escholarship.org/uc/item/1340j5h2 --
  author-tagged CFG; content requested by required/prohibited tags; precondition tags re-checked
  during expansion; the curationist principle (recount what truly happened, never invent it).
- Emily Short, "World Models Rendered in Text" https://emshort.blog/2018/06/19/world-models-rendered-in-text/ ,
  "Describing a Procedurally Generated World" https://emshort.blog/2019/03/05/describing-a-procedurally-generated-world/ ,
  "Beyond Branching" https://emshort.blog/2016/04/12/beyond-branching-quality-based-and-salience-based-narrative-structures/
  -- salience-as-specificity, the three axes of importance, the *Annals of the Parrigues* engine
  (fact-set-tagged fragments, "greatest number of facts" selection, the Venom/Mushroom/Egg
  principles).

**Game prior art (truthful sim -> text)**
- Caves of Qud: Grinblat & Bucklew, *Subverting Historical Cause & Effect*,
  https://www.pcgworkshop.com/archive/grinblat2017subverting.pdf -- state-driven replacement
  grammar (path-addressed by typed entity fields); the two-tier truth/texture split; mint names
  from state then persist them.
- Dwarf Fortress -- the legends/announcement text is a pure function over typed, id-only event
  records; the renderer owns vocabulary and grammar but zero facts.
- Ultima Ratio Regum (Mark R. Johnson): "chains of meaning" and the riddle/poem generators,
  https://www.markrjohnsongames.com/2025/02/16/ultima-ratio-regum-0-11-update-29-riddle-generation-3-poems/
  -- one true fact rendered across many oblique surfaces; show-don't-tell as a tunable difficulty.
- Talk of the Town / Bad News (James Ryan): per-character belief facets with explicit Accuracy;
  dialogue renders the speaker's *belief* via runtime variables, so lies/misremembering are true
  facts about a mind, never renderer hallucinations,
  http://www.gameaipro.com/GameAIPro3/GameAIPro3_Chapter37_Simulating_Character_Knowledge_Phenomena_in_Talk_of_the_Town.pdf .

**Salience / interest**
- Portet, Reiter, Gatt et al., BabyTalk (AIJ 2009), https://staff.um.edu.mt/albert.gatt/pubs/bt45-aij.pdf
  -- importance score + two-sided thresholds + caps + pre-selection abstraction.
- Kreminski et al., "Select the Unexpected" (ICIDS 2022),
  https://mkremins.github.io/publications/StU_ICIDS2022.pdf -- rank chronicle matches by ascending
  property likelihood (drop-in for the Sifter's interest scoring).

**The Wolfean technique**
- ultan.org.uk, "everything has to be true somehow," https://ultan.org.uk/everything-has-to-be-true-somehow/
  -- Wolfe's method (truth by omission, not deception; characterise by action; must stay solvable).
- Hemingway's iceberg / theory of omission, https://en.wikipedia.org/wiki/Iceberg_theory ;
  Genette's focalisation + free indirect discourse, https://en.wikipedia.org/wiki/Focalisation .
