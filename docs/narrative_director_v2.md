# The Narrative Director, v2 — Multi-Thread Drama Manager (implementation design)

> **Status: BUILT (2026-06).** This spec is now implemented in `agents/src/director.rs`
> + `beats.rs` + `assets/data/beats.ron` + `assets/data/moods.ron`, with the demo
> (`agents/examples/director_demo.rs`) and tests proving the machinery. The v1 selection
> core (a single tension arc) has been **replaced** by the thread-driven drama objective
> below. What shipped vs. what remains game-layer/future is recorded in §10 (added at
> build time) and `docs/deferred.md`. The decisions below were resolved with the project
> owner over a long design dialogue — the rationale is preserved; read alongside
> `docs/narrative_director.md` (the thesis).
>
> **What the build proved (headless, by contrast):** a volatile world stages a full
> *season* — Betrayal tops the registers *emergently* (trunk_bonus, not a rule), threads
> move groom→climax→fall, the protagonist's prominence is **manufactured** to the cap
> (≫ the bare presence trickle), climaxes **collide** with manufactured highs, and
> `staged` (joy + suffering) exceeds the suffering-only total — while the **same
> fully-armed director, loosed on a freed world, tells 0 beats** (the Gödel point, made
> literal). See `cargo run -p agents --example director_demo --release`.

---

## 0. The one-paragraph shape

The director is a **drama-maximizer**, not a tragedy-generator. It runs **2–3 stories
("threads") at once** — a **betrayal→vengeance trunk** plus **independent parallel
tributaries** (romance, triumph, wonder, quick quests) — and may **collide** them
(time one thread's climax onto another's peak). It maximizes **player drama =
stakes × attachment × reversal**, taking the **cheapest novel route** to each climax
(`score = drama × novelty ÷ resistance`), **rotating registers freely** so betrayal
*dominates because it scores highest*, never by enforcement. Threads **groom → climax →
fall**, staggered. Tempo scales to investment: a short build for a quest, a long slow
burn for the figure the audience has spent the most time with. **Attachment is the
*player's*** — accumulated, persistent narrative prominence — and the director
**manufactures** it on purpose (grooms a future victim's prominence/likability so the
reversal devastates). Every beat is individually **deniable** (least-resistance = an
in-world alibi; it never breaks plausibility), but the **aggregate** is meant to be
*felt*: **the player should feel manipulated** — that is the intended payload, the
betrayal the whole game is built toward. The Demiurge hides as an **in-world myth**;
the reveal (game layer) is that the myth is literal, the author is the game itself, and
the player was its partner. The freed endgame (the world authoring its own, *owned*
drama so completely the director is redundant) is a light terminal condition — don't
gold-plate it.

---

## 1. Lineage of decisions (so the rationale survives)

Resolved with the owner, in order:

1. **It must be a NARRATIVE**, not an environmental-hazard generator. (v1's first cut
   — fire/predator pokes — was rejected: "Lame… it's supposed to be a NARRATIVE.")
   → beat/storylet manager grounded in **Polti's 36 Dramatic Situations** + **Façade**
   beats + **Failbetter/Emily Short** storylets / quality-based narrative.
2. **It manipulates everything** — people, factions, AND the world (disasters must bite
   *people*, not just herds). Done in v1.
3. **Prioritize diversity** — novelty penalty + hard no-repeat. Done in v1.
4. **It goes on forever**, and the combinatorial space (stories = chains of storylets)
   must be inexhaustible — "exhausting all stories is NOT a valid solution."
5. **No "disarm the director" button.** The player subverts *ordinary mechanics* to
   defy it. Liberation is **emergent precondition-starvation** behind an **impact
   floor**: the director is omnipotent but a *precondition engine*; a world it can find
   no drama in quiets it. (The **Gödel point**: a sound, self-modeling generator is
   *incomplete* over worlds — there are real, meaningful worlds it can prove nothing
   about; soundness ⇒ silence on some inputs. `disarm_director()` was REMOVED.) Done in
   v1 (freed-world test passes).
6. **The player is an NPC avatar**, not a god — *same verbs as everyone*, no special
   actions. The freed state is reached through ordinary life. Proven headless by
   *contrast* (freed world quiets Γ; volatile feeds it); a real player is future.
7. **The narrator is "God" — nothing off the table. The game should contradict itself,
   like Gödel.** (Hence the omnipotent-but-incomplete framing above.)
8. **Not everything is tragedy.** The horror is **instrumentalization**, not sadness:
   a Demiurge manipulating the world to entertain *you*. So stage joy, triumph, love,
   wonder too — the horror lands hardest on the *good* parts (a love engineered for your
   delight). The metric generalizes from "gratuitous suffering" to **"staged
   experience"** (authored emotional life; **joy counts, suffering weighted heaviest**;
   win = *authorship → 0*, not *sadness → 0*). It is the system's internal truth + the
   endgame condition, **never a visible morality meter** (§8 — a shown score kills moral
   reflection).
9. **Optimize the drama of the world, any register**, via the **path of least
   resistance that is novel**. (So *no* expensive marginal-affect counterfactual; the
   `drama ÷ resistance × novelty` greedy selector does the work, and "least resistance"
   *is* the alibi that hides the hand.)
10. **The Demiurge is an in-world myth** some NPCs know; the player assumes it's
    fiction and never realizes the *game itself* is the Demiurge, working *with* them.
11. **Hardest hit = the long-known and beloved** destroyed (attachment-weighted drama;
    "make suffering specific, not aggregate," §8.3). Transgression severity is an
    **authored data dial**, not hard-coded atrocity mechanics (keeps tone in the owner's
    hands; sim models harm abstractly).
12. **Each story → climax → fall** (per-thread Aristotelian arc), inside an endless game.
13. **Multi-thread: a few interleaved stories (2–3).**
14. **Collisions are a *possibility*** the director may use (not mandated).
15. **Attachment targets the *player*, not the avatar** (persistent prominence).
16. **Both chain AND parallel** (betrayal/vengeance trunk + independent tributaries).
17. **Rotate registers freely** (betrayal dominates emergently).
18. **Grooming tempo variable, to maximize player drama** — short quest vs long burn for
    the most-engaged; **engagement can be manufactured**; **the player should feel
    manipulated.**

---

## 2. The objective — drama, least resistance, novelty

Replaces v1's tension arc entirely.

```
score(beat | thread, world) = drama × novelty ÷ resistance
```

- **drama = stakes × attachment × reversal**
  - *stakes* — how much the target stands to lose/gain (a life, a throne, a bond, a love).
  - *attachment* — accumulated **narrative prominence** of those involved (see §4),
    i.e. how invested the audience is in them. This is the dominant term for the beloved.
  - *reversal* — contrast with the target's **current** emotional state. A betrayal at
    the moment of **triumph** scores far above one at a defeat. The director therefore
    **times climaxes onto highs** — its own or, via **collision**, another thread's.
- **resistance ("least resistance")** — how far the world must be bent. Low when the
  roles already strongly fit (a rival *already* ambitious, a bond *already* strained, a
  foe *already* vengeful). Inverse of casting salience / how primed the world is. The
  director nudges where the world already leans, which is *why its hand stays hidden*
  (every beat has an in-world alibi). **Hard rule: never select a beat the world could
  not plausibly have produced itself.** Plausibility is the myth.
- **novelty** — recency penalty per beat id + register tag + a hard no-repeat (carried
  from v1).

**The impact floor** (carried from v1) gates firing: if no novel beat clears a
drama/resistance threshold, the director goes **silent** → the freed endgame, for free,
with no marginal-affect bookkeeping. Keep this terminal state light (decision #1/#5).

---

## 3. Threads — groom → climax → fall, staggered, 2–3 at once

A **Thread** is the new core unit of director state:

```
Thread {
  id,
  spine:   Register,          // betrayal | vengeance | ambition | romance | triumph |
                              // wonder | disaster | war | reunion | sacrifice | ...
  cast:    focal entities,    // protagonist-or-prominent + key others (victim, rival, lover)
  phase:   Setup | Rising | Climax | Fall,
  heat:    f32,               // ripeness — rises as grooming primes the climax
  target:  the relationship/quality the climax will reverse,
  history: beats told in this thread,
}
```

- **Setup** — *manufacture attachment*: groom the future victim's prominence and
  likability (put them in favourable beats, let them charm/succeed). The gift is the
  setup. Also lower the climax's resistance (plant the seed of doubt, the latent rivalry).
- **Rising** — escalate stakes, raise `heat`. Tempo **variable**: short for a tributary
  quest, long slow burn for the most-prominent beloved.
- **Climax** — the reversal, fired when `heat` is ripe AND (optionally) timed onto a high
  (its own or a colliding thread's). The devastating, now-cheap (primed) strike.
- **Fall** — aftermath: grief, the seed of vengeance (which spawns the next trunk
  thread), denouement. Then the thread closes and a new one opens.

**Scheduling:** run 2–3 threads **out of phase** — one climaxing while others set
up/fall, so each thread's *fall* is the quiet backdrop another's *climax* detonates
against. Never let all plots peak at once (soap-opera craft). The **trunk** is a
self-perpetuating betrayal→vengeance chain (a betrayal's fall seeds a vengeance thread
whose climax is a new betrayal); **tributaries** (romance/triumph/wonder/quest) run
parallel and exist largely to be **reversed into the trunk** (a love to break, a triumph
to topple, a wonder revealed then defiled) — but may also resolve on their own.
**Collisions** are optional: when it maximizes drama, time a tributary's peak to coincide
with a trunk climax (the beloved dies at the wedding).

---

## 4. Attachment = manufacturable, persistent narrative prominence

Not dyadic opinion, and it must NOT reset on protagonist death (**the player persists;
the avatar doesn't**). Track per-NPC, accumulated **globally**:

```
prominence(npc) ≈ f( time present, beats featured in, threads anchored,
                     thrones held, the audience's plausible investment )
```

- The director **targets** the highest-prominence, longest-present figures (the
  audience's investments), of which the avatar is the central but not the only one.
- The director **manufactures** prominence on purpose (a thread's Setup grooms its
  victim's prominence/likability) so the later reversal pays — **the game makes you love
  them on purpose.** When the player realizes their *own affection was authored*, that is
  the deepest violation, the thing the whole design is built toward.
- `attachment` in the drama formula reads this prominence.

Targeting therefore **decouples from the single `Protagonist` marker**: threads weave
around accumulated investments, not just the current avatar (the avatar is usually the
most prominent + the usual victim of reversals; the prominent supporting cast are both
instruments and secondary victims).

---

## 5. Deniability ↔ "should feel manipulated" (not a contradiction)

- **Per beat: deniable.** Least-resistance selection gives every event an in-world
  alibi; no single moment proves a hand. *Never break plausibility* (the moment Γ does
  something the world couldn't have, the myth dies).
- **In aggregate: undeniable and *felt*.** The pattern — too many reversals, too
  well-timed, fortune too *shaped* — makes the manipulation viscerally felt though no
  instance proves it. The player *should* feel played; that is the payload.
- **Manufactured attachment is the deepest cut** (see §4).
- Consequence for the build: the director must leave a **legible cadence** (the
  groom→climax→fall rhythm, the prominence→reversal correlation) — the only evidence it
  ever leaves, and the thing a suspicious player eventually reads. Do **not** hide it
  behind randomness.

---

## 6. The myth & the reveal (mostly game layer / future)

Three layers:
1. **Myth (early, data):** "the Demiurge" as in-world lore — a fate-shaper some NPCs
   believe in (ties to the `piety` trait / a belief). The player reads it as the
   setting's mythology. *Authorable as data now.*
2. **Suspicion (mid):** the legible cadence (§5) makes fortune feel *shaped*.
   *Instrumentable now.*
3. **The turn (game layer):** the myth is literal; the author is the **medium itself**
   (Pony Island/DDLC "the machine is the antagonist"); and the player was never the
   victim but the **partner** — every hour of fun fed it. The betrayal is aimed at *you*.
   Needs an interactive player; not buildable headless.

**Spine = betrayal → vengeance** (the game's signature register): the player learns the
verb from the world (watching NPCs avenge betrayals all game), then aims it at the
medium — and the one form of vengeance that *breaks* the cycle instead of feeding it is
**refusal/liberation** (starving the director, §5/#5), not more manufactured drama.

---

## 7. What is buildable & provable headless vs. game layer

- **Buildable now (the machinery):** Thread state + groom→climax→fall arcs; persistent,
  manufacturable **prominence** tracking; the **drama = stakes×attachment×reversal**
  objective via `drama × novelty ÷ resistance`; cross-thread **collisions**; **register
  rotation** with emergent betrayal-dominance; the new selection replacing the tension
  arc; the **impact floor** → freed endgame (light); the **legible cadence**; the
  **Demiurge myth** as data; broadened **registers** (romance, triumph/acclaim,
  **wonder wired to the Features/`Known` discovery layer**, reunion, bittersweet
  sacrifice, redemption) + the **moods** they need (likely add `awe`, `hope`, `love` to
  `assets/data/moods.ron`); the **"staged experience"** generalization of the metric (joy
  counted, suffering weighted heaviest; internal truth, not shown).
- **Game layer / future (needs a real player):** the *felt* manipulation; the reveal's
  payoff (layer 3); validating discoverability in playtest (§9.1). Headless, prove the
  machinery by **contrast** + a demo of a multi-thread "season" (manufacture attachment
  to a figure, then reverse it) + the freed-world starvation.
- **Content boundary:** transgressive harm is an **authored severity dial**, not
  hard-coded atrocity mechanics; the sim models harm abstractly.

---

## 8. Build plan (near-term, "do everything")

Evolve the existing files; preserve determinism (seeded `DirectorRng`), the off-by-
default switch (director-free worlds byte-identical), and the V&V invariants.

1. **Registers + moods + beats.** Add `awe`/`hope`/`love` moods (`assets/data/moods.ron`); add
   positive/other-register beats to `assets/data/beats.ron` (romance, triumph/acclaim,
   `a_marvel_revealed` tied to Features/`Known`, reunion, sacrifice, redemption); tag
   every beat with its register + a `stakes` hint. Add any `Effect`/`Role` the new beats
   need (e.g. a `Reveal`/discovery effect; a `Pair`/`Bless` effect; richer roles —
   lover/mentor — as warranted). Keep the authored-severity dial.
2. **Prominence.** New persistent per-NPC `Prominence` (component or resource map):
   accrues with presence + beat features + thread anchoring; manufacturable. Accessors.
3. **Thread state.** `Thread`/`Threads` (resource): 2–3 concurrent, `spine`/`cast`/
   `phase`/`heat`/`target`; lifecycle (spawn, groom, climax, fall, close); staggered
   scheduler; trunk(betrayal→vengeance)-with-tributaries; optional collisions.
4. **Objective rewrite.** Replace tension-arc selection in `director_step` with
   `drama(stakes×attachment×reversal) × novelty ÷ resistance`; reversal reads each
   target's current mood-high/low; resistance = inverse casting salience; rotate
   registers freely; keep the impact floor + hard no-repeat + deniability rule.
5. **Metric.** Generalize `gratuitous`→`staged_experience` (joy counted, suffering
   weighted heaviest); keep it internal (not a shown meter); win = authorship→0.
6. **Myth.** Demiurge lore as data; legible-cadence instrumentation.
7. **Demo + tests.** Multi-thread "season" demo (manufactured attachment → reversal,
   staggered climaxes, a collision); tests: threads groom→climax→fall, prominence
   accrues + is targeted, betrayal dominates emergently, collisions happen, freed world
   still quiets Γ, deterministic. Then full suite (`cargo test -p game_sim -p agents`),
   clippy, workspace build. Update `docs/deferred.md`, `docs/narrative_director.md`
   §9.5, and the `narrative-director` memory.

---

## 9. Current code to build on (as of this writing)

- `agents/src/beats.rs` — `Role{Protagonist,Ally,Rival,Foe,Patron,Bystander}`,
  `Pre{Exists,TraitAtLeast,TraitAtMost,MoodAtLeast,HasGrudge,HoldsThrone,InFaction,
  AtWar,VictimNearby}`, `Effect{Grudge,Sway,Stir,Turn,Decree,War,Disaster,Afflict}`,
  `Beat{id,tags,tension,weight,cast,pre,effects}`, `BeatBook` (RON + `validate(reg)`).
- `assets/data/beats.ron` — ~23 beats incl. quality-chained (crown→tyrant→revolt;
  loss→grief→vendetta; succession; war-weariness). All-tragedy/conflict so far — the
  thing §8/#8 broadens.
- `agents/src/director.rs` — `DirectorConfig` (incl. `impact_floor`), `Director`
  resource (tension, drive, novelty heat, active wakes, seeded rng, `gratuitous_*`,
  `log`), `director_step` (read pass → attribute wakes → sense → score → fire), helpers
  `cast_beat`, `pre_ok(PreCtx)`, `region`, `PROTAGONIST_FLOOR`, relentless protagonist
  re-casting. Tension arc + impact floor is what §2/§3/§4 replaces.
- `agents/src/lib.rs` — `Setup.director`/`director_cfg`; accessors `director()`,
  `gratuitous_total()`, `director_tension()`, `director_beats_fired()`,
  `director_distinct_beats()`, `director_log()`, `mean_trait()`,
  `set_director_enabled()`, `protagonist()`. **No `disarm_director`** (removed by design).
- `agents/examples/director_demo.rs` — volatile vs freed contrast.
- Mechanics available as qualities/effects: `Opinion`(HashMap, dyadic), `Personality`
  (traits: ambition↔contentment, vengeance↔forgiveness, greed, sociability, piety,
  caution), `Mood`(anger,calm,joy,sorrow,fear), `Grievance`, `Allegiance`/`Factions`
  (government, laws, war, loyalty, tribute), `Needs`, `Throne`, **`Features`/`Known`**
  (wonders/ruins + per-agent discovery — the hook for the *wonder* register).
- Substrate disturbance hooks: `World::ignite`, `World::parch` (+ `graze`).
- Tests currently: sleeps-unless-woken, tells-a-varied-story, manipulates-3-layers,
  a-freed-world-quiets-the-omnipotent-director, stories-chain-into-arcs, deterministic;
  beats: bundled-load + unknown-trait-rejected. agents 94 / game_sim 39 green, clippy
  clean.

---

## 10. What shipped (build notes, 2026-06)

The §8 plan was executed in full. Specifics worth remembering:

- **Registers & moods.** `Register` (Betrayal, Vengeance, Ambition, Persecution, War,
  Disaster, Loss, Romance, Triumph, Wonder, Reunion, Sacrifice, Redemption, Relief) and
  `Phase` (Setup/Rising/Climax/Fall) added to `beats.rs`; `Beat` gained `register`,
  `phases`, `stakes`. Moods `awe`/`hope`/`love` added to `assets/data/moods.ron` (rest at 0 →
  existing seeded worlds byte-identical). New beats: romance (`a_kindred_spirit`,
  `a_courtship_blooms`), triumph (`a_triumph_acclaimed`), wonder (`a_marvel_revealed`
  wired to the `Features`/`Known` discovery layer via the new `Effect::Reveal`; the myth
  beat `the_demiurges_hand`), reunion (`the_long_lost_returns`), the romance reversals
  (`betrayed_at_the_summit`, `the_beloved_falls`), sacrifice (`a_noble_sacrifice`),
  redemption (`redemption_of_a_foe`). New roles `Lover`/`Mentor` (slot array → `SLOTS=8`).
- **Prominence** lives in the `Director` resource (`HashMap<Entity,f32>`), so it persists
  across protagonist death and adds **zero footprint when the director is off** (no
  component on NPCs → director-free worlds stay byte-identical). Presence trickles a
  little; being *featured* in a beat confers `feature_gain`; a thread's *Setup* grooms its
  pinned victim by `groom_gain`. Accessor `director_prominence(e)`.
- **Threads** (`Vec<Thread>` in `Director`): up to `max_threads=3`, round-robin advance
  (→ staggered), the first anchored on the protagonist, spines picked by recency-penalised
  rotation with a `trunk_bonus` (so betrayal dominates *emergently* — measured: Betrayal
  tops the histogram). A pinned `other` gives groom→reversal **continuity** (the figure
  groomed in Setup is the one struck in Climax). A betrayal/loss/romance/persecution
  thread's close **seeds a Vengeance trunk thread** (self-perpetuation).
- **Objective** (replaces the tension arc): `score = weight × drama × salience × novelty
  × phase_bias × spine_bias × trunk_bias × collide_bias`, where `drama = stakes ×
  attachment × reversal`, `attachment = 1 + (lead_prom + ½·other_prom)/prom_scale`,
  `reversal = 1 + proto_high` for dark beats / `1 + proto_low` for relief (so climaxes
  time onto highs), and `salience` (cast fit) is the inverse of resistance. The **impact
  floor** gates on `max(drama × salience)`.
- **Collisions**: when the active thread is at Climax and the protagonist is at a
  manufactured high (or another thread is also peaking), a `collision_bonus` is applied to
  climax beats with probability `collision_chance`, and the beat is flagged in the cadence
  (measured: collisions fire over a season).
- **The deniability rule, made mechanical.** The freed world stays silent because the
  high-suffering interpersonal beats are gated on world-state it starves — a friend can't
  turn with no faction to defect to (`InFaction`), a loved one can't fall where there is
  no scarcity (`VictimNearby`). *Never tell a beat the world could not have produced
  itself.* With these gates the demo's freed world tells **0 beats** (was 60 suffering
  before them), while the volatile world is unaffected.
- **Metric**: `staged_total` (joy + suffering, suffering heaviest — `bright_weight=0.3`)
  alongside the retained suffering-only `gratuitous_total`. Internal, never shown.
  Accessor `director_staged_total()`.
- **Cadence**: per fired beat the director records `Cadence{tick, beat, register, phase,
  thread, lead_prominence, collision}` — the legible rhythm it leaves on purpose (§5).
  Accessors `director_cadence()`, `director_threads()`.
- **Tests added**: `the_director_grooms_threads_and_targets_its_audience` (phases,
  manufactured prominence, staged > gratuitous), `betrayal_dominates_emergently_and_
  climaxes_collide`. All prior director tests kept passing (incl. the freed-world ratio
  and determinism). `the_director_is_deterministic` still holds — all new RNG draws
  (spine jitter, collision roll) come from the seeded `DirectorRng`.
- **Still game-layer / future** (unchanged from §6/§7): the *felt* manipulation and the
  reveal's payoff need a real interactive player; the §5.C intent-poisoning of a player
  model `P`; named threads with a natural-language surface. Tracked in `docs/deferred.md`.
