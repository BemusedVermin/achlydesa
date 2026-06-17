# Achlydesa — The Narrative Director

*Design spec: subverting the power fantasy by making the engine the antagonist.*

> Status: design / pre-implementation. This is a working spec, not a contract — it
> records the thesis, the research that backs it, and the mechanical shape of the
> feature so implementation has a target. Claims drawn from the research pass are
> tagged with confidence: **[verified]** = primary source, fact-checked;
> **[medium]** = real but qualified; **[analogy]** = structurally suggestive, not
> direct evidence. Sources are listed in the appendix.

---

## 0. One-paragraph summary

Achlydesa is a narrative strategy sandbox over an artificial-life simulation. A
hidden **narrative director** manipulates the world to keep the player
entertained — and in doing so manufactures misery for every NPC and for the
player-character. The player is *meant* to enjoy this at first (the power
fantasy). The game's real arc is the player turning against the director: not by
a scripted "defeat the director" button, but by using the game's own ordinary
mechanics to **starve the director of the inputs it feeds on** and **strip the
levers it acts through**, until it has no gratuitous suffering left to inject and
the world is left to author its own, freely-chosen life. The director is never
personified. The enemy is the medium.

---

## 1. Thesis

### 1.1 The theodicy seam
The project grew out of an artificial-life insight: **you cannot build a living
world without encoding death.** Our own `game_sim::Params` already proves it —
`plant_mortality`, `herb_mortality`, `carn_mortality`, predation (`carn_attack`),
fire (`fire_animal_kill`), freezing (`freeze_temp`), starvation (`Energy` → 0).
None of that is cruelty; it is what makes the ecology *alive*. This is the
creator's dilemma the game is about: to make a world worth living in, you must
make one in which things can die.

So the game's moral spine is the distinction theology spent centuries on:

- **Necessary suffering** — endogenous to the world's own dynamics. In our terms:
  whatever the substrate evolution `Φ` (`World::evolve`) produces on its own. A
  world without it is not paradise; it is a dead, static simulation.
- **Gratuitous suffering** — pain injected *for an audience*, beyond what the
  world needs. In our terms: whatever a **director** adds on top of `Φ` to make
  the spectacle more entertaining.

**The win condition is not a world without death.** It is a world that has been
freed of the *gratuitous* layer — one that produces its own meaning so completely
that the director has nothing left to author. The redeemed world still has the
mortality in `Params`. It just isn't being *watched and worked* anymore.

### 1.2 The power-fantasy subversion
Most god-games hand you power over lives and never ask the cost. Achlydesa gives
you that fantasy *first*, then reveals it was bought with manufactured pain, then
— crucially, and unlike its closest precedent (see §7) — gives you a way to fight
back *from inside the mechanics*. The three beats:

1. **Power fantasy.** The player shapes lives, is rewarded with progress and
   spectacle, and is entertained.
2. **Complicity reveal.** The player comes to understand (never via a villain
   monologue) that their entertainment *is* the engine of the world's misery.
3. **Liberation.** The player uses ordinary, earned mechanics to dismantle the
   director's influence — paying a real cost in the power they were enjoying.

---

## 2. Core design insight (research-backed)

**A narrative director is, mechanically, a closed feedback loop.** Every shipped
or studied one has the same three parts and nothing else:

1. A **finite set of measurable inputs** it reads from world state.
2. A **finite, author-enumerated set of intervention levers** it acts through.
3. *(optionally)* a **player model** it uses to aim those levers at *this* player.

It has no power outside that. Nelson & Mateas state it directly: *"the drama
manager's influence is defined by the set of DM actions."* **[verified]**
Concrete confirmations:

- **Left 4 Dead's Director** feeds on per-Survivor "emotional intensity" built from
  damage / incapacitation / ledge-grabs; that signal *decays toward zero over time
  but not while Infected are engaging you*. Take no damage, engage nothing, and its
  peak-manufacturing input goes to zero. **[verified — Valve GDC 2009]**
- **RimWorld's Storyteller** (the canonical *non-personified* director: "no in-game
  body, location, or manifestation") scales threats off **colony wealth, colonist
  count, animal count, recent deaths/woundings, time since last event**. An explicit
  "wealth-independent mode" toggle exists — proving normal mode is *keyed* on a
  player-controllable input you can suppress. **[verified]**
- **Academic drama managers** (Nelson & Mateas SBDM) act only on abstracted
  discrete *plot points*: *"the player may wander around the world for a long time
  without experiencing anything. The DM doesn't notice any change in its abstracted
  view, so does nothing."* **[verified]**

**This closed-loop shape is the entire vulnerability surface, and it is what makes
the "no scripted boss" constraint not just possible but principled:** you do not
need a defeat-the-director feature. You design the game so the player can *starve
the inputs* and *strip the levers* with tools that exist for other reasons.

---

## 3. Architecture: the director as an `Observer` + a bounded lever-set

The director maps cleanly onto the existing `sim` trait vocabulary. Recall the
legend in `sim/src/lib.rs`: `E` substrate, `Φ` evolve, `Π` perceive, `Δ` decide,
`a` action, `σ` scheduler, **`M` observer**, `ω` rng, `T` simulation.

The director (proposed symbol **`Γ`**, the "game master" operator) is:

```
Γ  =  ( M_Γ ,  L ,  [P] )

  M_Γ  : an Observer — reads a fixed vector of observables from world state
  L     : a finite ordered list of levers, each lever an Action a ∈ sim::Action
  P     : (optional) a player model inferred from the player's action trace
```

Per step the director **observes** (`M_Γ`), optionally **updates `P`**, then emits
zero or more **levers** from `L` against the world — through the *same*
`Action`/`Interaction` channels the regular actors use.

### 3.1 The non-negotiable architectural constraint
**The director must act only through an enumerable lever list `L`, never through
arbitrary writes to world state.** If `Γ` can poke any field to any value, it is
unbeatable and the whole design collapses. Its power *must* equal a finite list of
`Action`s, each with a `claim()` and `priority()` like any other action — so that:

- each lever is something the player can *identify, contest, or remove*, and
- the director **does not invent new physics**: every lever cranks a dial that
  already exists in `Params` (ignite via `base_lightning`, predator surge via
  `carn_attack`, drought via precip suppression, freeze, scarcity). Gratuitous
  suffering is literally *the director over-driving the same dials that, left to
  `Φ`, would be necessary background*.

This gives the game's moral distinction a **computable definition**:

```
necessary(t)  = the suffering Φ would have produced this step with Γ silent
gratuitous(t) = (actual suffering) − necessary(t)   ≡  Γ's contribution via L
```

The liberation goal is to drive `gratuitous(t) → 0` for all `t` while `Φ`
continues. That is the moral win, and it is measurable.

### 3.2 Where it lives in the workspace
- `sim` — already provides `Observer` (`M`), `Actor`, `Action`/`Interaction`,
  `Substrate`. `Γ`'s lever channel is `sim::Action`; its senses are an `Observer`.
- `game_sim` — the substrate `Φ` and every dial `Γ` is allowed to abuse (the
  disturbance and ecosystem fields in `Params`).
- `agents` — the live ECS world (bevy_ecs) where NPCs are entities. `Γ` is most
  naturally a **resource + an ECS system** here (mirroring how `Substrate` and
  `SimRng` are resources), reading the population and emitting levers. Note the
  agent layer is currently **non-stigmergic** — this matters for §5.A.
- A new `director` crate (or a module in `agents`) is the likely home for `Γ`.

---

## 4. The director's input / lever table (grounded in real fields)

### 4.1 Inputs `M_Γ` reads (the things a player can starve)

| Input | What it measures | Backed by our fields | Precedent |
|---|---|---|---|
| **Suffering / intensity** | deaths, starvation, predation, burns this step | `Energy`→0 despawns, `herb_mortality`, `carn_attack`, `fire_animal_kill` | L4D intensity **[verified]** |
| **Scarcity** | food/nutrient/water shortfall | low `plant_biomass`, low `soil_nutrients`, drought (low humidity/precip) | RimWorld inputs **[verified]** |
| **Wealth (escalation fuel)** | how much there is to threaten | population count, total biomass/`Energy`, territory, stored food | RimWorld wealth **[verified]** |
| **Volatility** | instability worth dramatizing | variance of population / energy over a window | inferred |
| **Activity floor** | is anything happening at all | action count, births/deaths churn, migration | "inactivity" halt **[analogy]**; "lull in action" **[verified]** |
| **Pacing clock** | time since last major event | tick counter since last lever fired | RimWorld **[verified]** |
| **Player intent** (if `P` used) | what entertains *this* player | inferred from the player's action trace | Zhu & Ontanón **[verified]** |

### 4.2 Levers in `L` (the things a player can strip)
Drama managers act through a small, fixed verb set — *cause, hint, deny, undeny* —
plus, in the C-DraGer/Anchorhead system, *temporary deniers each paired with a
re-enabler*. **[verified]** Ours, each implemented as a `sim::Action` over an
existing `Params` dial:

| Lever | Effect | Reuses |
|---|---|---|
| **cause** | spawn a disturbance: ignite, predator surge, disease, drought, raid | `base_lightning`, `carn_attack`, precip/`evaporation`, freezing |
| **hint** | telegraph / bias the world toward a dramatic event without forcing it | worldgen/field nudges |
| **deny** | block relief: hide/withhold a resource the player needs | suppress `plant_seed`, `weathering`, migration |
| **temporary-deny + re-enable** | withhold relief for a window, then restore | timed `deny` |
| **escalate threat population** | raise the count/intensity of threats | RimWorld/L4D "full threat population" **[verified]** |
| **rubber-band (adaptation)** | make it harder *after* the player does well | RimWorld adaptation score **[medium]** |

---

## 5. The three subversion patterns (mapped to our mechanics)

### 5.A — Starve the inputs → emergent equilibrium *(primary path)*
**Precedent:** L4D intensity decays to zero with no engagement; RimWorld wealth
management is a documented strategy; Nelson & Mateas — a player who triggers no
plot points leaves the DM blind and idle. **[verified]**

**In achlydesa:** the liberation play is to drive the NPC population into a
**self-sustaining ecological equilibrium** — predator/prey/vegetation balanced,
needs met, low and stable wealth, low churn. When that happens, every
suffering-input flatlines, wealth stops climbing, the activity floor is
approached. The director's measured "drama" falls to its floor and `gratuitous(t)`
drops toward zero on its own. *You bore the god to death.* It plays like an
exploit; it is the moral ending.

**Enabling work:** this requires the **stigmergic loop** the `agents` crate
flags as "comes later" — agents that *modify* the substrate (graze it down, shape
it for the next generation), creating the environment-mediated negative feedback
that lets a population *self-regulate* instead of boom/bust. Equilibrium is the
prerequisite for starvation, so the stigmergic loop is on the critical path for
the whole game, not just the ecology.

**The catch — the re-escalation arms race.** Nelson & Mateas already patch the
starvation hole with a reusable *"lull in action" plot point*: when the world goes
quiet, the director **manufactures** a reason to escalate. **[verified]** So
equilibrium alone is only *temporary* suppression — and this is the
necessary-vs-gratuitous line made mechanical: *a director re-injecting conflict
into a peaceful world is the definition of gratuitous suffering.* Making the
quiet **permanent** is the open problem in §8.

### 5.B — Turn an earned engine-level tool on the system
**Precedent:** DDLC's win condition is deleting Monika's real game file (an
engine-level act, not a scripted command); Pony Island recasts config/menu acts as
diegetic hacking. The analysis is explicit that the personification is illusion:
*"effectively the game itself is the hacker… Monika is obviously synonymous with
the machine."* **[verified]** Exactly our non-personified director.

**In achlydesa:** we have the ingredients — a deterministic seeded RNG
(`SplitMix64`), a clean `Substrate`/`Observer` separation, and `Γ`'s power
expressed as an inspectable lever list `L` and input-weight vector. The **earned
tool** is a diegetic affordance, gained through ordinary play, that lets the player
*read and edit `Γ`'s own state* — its input weights, its lever queue, its seed.
The reveal: **the tool you were handed to play god over their lives is the same
tool that edits the director's parameters to zero.** Same verb, opposite object.
It must read as the intended path, not a cheat.

### 5.C — Poison the player model
**Precedent:** our own `paper.txt` (Zhu & Ontanón) — the director can't observe
intent, it must *infer* it from observable actions, and that inference is provably
fragile (Silent Hill: Shattered Memories misread a lost player; the Avian study
found players gaming the model). Model accuracy is bounded by the range of choices
offered. **[verified]**

**In achlydesa:** if `Γ` aims its levers using a player model `P`, the player can
deliberately flatten or falsify their action-trace to corrupt `P`, blunting the
director's ability to target them. **Caveat:** this only works if `Γ`'s objective
is *player-dependent* — see §6.

---

## 6. The load-bearing decision: player-dependent objective

The research surfaces one decision everything hinges on: **is `Γ`'s objective
player-dependent (it reads a model of what entertains *this* player) or
author-fixed (a blind, fixed tension arc)?** A fixed author arc needs no player
model and is **immune to poisoning and largely to starvation**. **[verified]**

> **Decision: `Γ` must be player-dependent.** It is the only way the subversion is
> possible through emergent play — and it is thematically exact: a director that
> exists *specifically to entertain the player at others' expense* is by definition
> player-dependent. The thing that makes it evil is the thing that makes it
> killable.

---

## 7. Complicity without a villain

**Precedent — positive:** Spec Ops: The Line implicates the player by **inverting
the reward structure so mechanical success is moral atrocity** ("each completed
mission spirals… into war crimes") and accuses the *player*, not a character.
**[verified]**

**Precedent — negative (the gap we fill):** Spec Ops offers **no in-game
liberation path** — its only refusal is quitting the game. **[verified]** That dead
end is exactly what achlydesa improves on: we give the player the thing Spec Ops
withheld — a way to fight back without putting the controller down.

**In achlydesa:** the power-fantasy phase must reward the player (progress,
spectacle, score) for precisely the actions that feed `Γ`'s suffering-inputs in
§4.1. The complicity reveal then *recontextualizes fun the player already had* —
the strongest, least preachy version of the turn.

---

## 8. Discoverability & the moral posing

> Confidence note: unlike §§2–7, this section is **[informed]** — drawn from
> well-known design precedents and textbook motivation psychology, **not** run
> through the adversarial verification pipeline. Treat the psychology as
> textbook-level and the game facts (NieR Ending E, Undertale MERCY, Spec Ops) as
> well-established but unverified here.

The goal is a **philosophical posing**, not a collectible. The player should free
the world out of *felt moral obligation*, **or** consciously feel none and decline
— and **declining must be an equally legitimate, un-punished outcome.** The choice
must never be driven by extrinsic reward (achievements, a "good ending" to collect,
score, 100% completion). The instant the game rewards the moral act, it converts
conscience into completionism — the **overjustification effect**: supplying an
extrinsic reward for an act crowds out the intrinsic motive for it (Deci & Ryan,
Self-Determination Theory). Sicart's *The Ethics of Computer Games* makes the same
point from the design side: moral *scorekeeping* forecloses moral *reflection*.

### 8.1 The resolution: three decoupled layers
The apparent tension — *discoverable enough to find* vs. *not nudged toward it* —
dissolves once these are tuned independently:

| Layer | Setting | Rationale |
|---|---|---|
| **The truth** — the world suffers, and *for the player* | **loud / highly discoverable** | the player must be *able* to see the moral situation |
| **The path** — the mechanics that free it | **available but unsignposted** | findable through play; never quest-marked |
| **The reward** — score, ending, achievement | **nonexistent** | any reward turns conscience into optimization |

Loud about the fact, silent about the prescription, empty-handed about the payoff.
Crucially, the §5.A "director-as-breadcrumb" must reveal **the truth, not an
unlock**: the player discovers that the world suffers *for them* — not that "a
secret ending lies this way." Discovery is of a moral *fact*; it carries no prize.

### 8.2 Discoverability patterns (find it without signposting)
- **The affordance is present from the start; only the meaning is discovered.**
  Undertale's MERCY exists in the first fight; the game never says "spare everyone
  for the good ending." → The player's ordinary world-editing tools already contain
  their anti-director use from hour one. Nothing is *unlocked*; something is
  *realized*.
- **Anomaly as the hook (curiosity gap).** Outer Wilds / Tunic / Fez / Animal Well
  gate progress behind *noticing something doesn't add up*. → The director's
  escalating wrongness (§5.A) is the anomaly; the player who feels "this isn't
  natural" has found the thread.
- **Persistence/memory as the tell.** A world that visibly *remembers* (Undertale's
  save-awareness, NieR's meta-layer) signals there is something beneath the surface.
  → Our deterministic, persistent sim already has this texture; let the world
  remember what was done to it.
- **Community discovery is a valid channel — but not the only one.** Fez/Tunic
  secrets spread socially; fine for puzzles. But for a *moral* posing, every player
  must be able to *see the truth* unaided, even if the *path* spreads by word of
  mouth.

### 8.3 Keeping the choice moral, not completionist
- **Kill the meter — highest-leverage move.** A visible morality score (Mass Effect
  Paragon/Renegade, inFamous, Fable, BioShock's Little Sisters) turns ethics into
  min-maxing a number. → **No morality meter and no good/evil framing anywhere in
  the UI.**
- **Cost without reward — the NieR Ending E standard.** NieR:Automata's true ending
  asks you to *delete your own save* to help an anonymous stranger: permanent,
  costly, no in-game payoff. The reward *is* the act. → Freeing the world should
  cost the player real power (the tools/spectacle they enjoyed) and grant nothing
  extrinsic.
- **Don't congratulate.** No "Good Ending" banner, no achievement pop (Spec Ops
  rewards nothing). → `gratuitous(t) → 0` yields a *quiet world*, not a victory
  screen. Silence preserves the weight.
- **Declining must be dignified and un-punished** (the non-coercion requirement,
  and the part most games botch). If walking away yields a worse ending or nagging,
  the "moral" path is just the optimal path wearing a halo. → The **system stays
  genuinely indifferent** to the choice while the **fiction** makes the stakes
  vivid. A player who sees clearly and keeps playing is answering honestly; respect
  it.
- **Make suffering specific, not aggregate.** Felt weight comes from *one named NPC
  the player knows*, persisting and remembering — not abstract mass misery
  (melodrama or statistics). → Exactly where the emergent ALife sim wins: it
  generates *particular lives*. A wrong done to someone the player watched grow
  beats a thousand anonymous deaths.

### 8.4 The completionist failure mode — cause & fix
The "do it just to see/collect it" worry has a precise cause and fix. **Cause:**
content gated behind the choice (a unique ending, achievement, cutscene) a
completionist *must* acquire. **Fix:** gate **nothing collectible** behind it — if
liberation unlocks no content and yields only a quiet world, there is nothing for a
completionist to want. Undertale's complement: the *cruel* route is narratively
punitive and permanently scars the save, so "just to see it" carries a real cost
rather than a clean trophy. *Caveat:* making the moral act **irreversible** can
itself become a completionist badge ("I did the rare thing") — same defense: no
content, no acknowledgment, nothing to show off.

This resolves the discoverability *method*; what remains is empirical calibration
(§9.1).

---

## 9. Open problems (residual)

1. **Discoverability — empirical tuning** (the *method* is settled in §8; this is
   what's left). Quantitative unknowns only: how *strong* the §5.A breadcrumb must
   be before real players notice the wrongness, and validating actual discovery via
   playtest telemetry. Calibration, not approach.
2. **Permanence / irreversibility.** Verified mechanisms are mostly ongoing
   suppression (starve intensity, suppress wealth) or single acts (delete a file).
   How does the player make the neutering something `Γ` *cannot re-plan around*?
   The thesis answer (§1.1): a world self-authoring enough to leave `Γ` no
   gratuitous opening. Needs to be made mechanical.
3. **The re-escalation arms race** (§5.A) — the equilibrium-vs-"lull in action"
   dynamic is unexamined and is probably the core mid-game loop.
4. **SAVE-as-anti-director.** Undertale's SAVE/DETERMINATION, NieR save-deletion,
   and OneShot were named as candidates but were *not* among the verified findings
   — repurposing the save system itself as the anti-director tool is unverified and
   warrants a focused follow-up.

---

## 9.5 Implementation status (2026-06) — a v2 multi-thread drama manager

The director is built as a **multi-thread drama manager** over the ECS
(`agents/src/director.rs` + `beats.rs` + `data/beats.ron` + `data/moods.ron`), behind
`Setup::director`. The first cut was an environmental-hazard generator (fire/predator
pokes) — it wasn't a *narrative*. v1 followed real drama managers as Façade-style beats on
a single tension arc; **v2** (the current build; full spec in
`docs/narrative_director_v2.md`) replaced that selection core with a thread-driven **drama
objective**:

- **Beats, not levers** (Façade; Mateas & Stern). The unit is the **beat** — a storylet
  (Failbetter; Emily Short): a *casting* of roles, a *precondition* over the world's
  qualities, *effects*, plus a formal **register**, the arc **phases** it suits, and its
  **stakes**. The world's ECS components **are** the qualities; `beats.ron` is the storylet
  pool. ~30 beats grounded in **Polti's Thirty-Six Dramatic Situations**, broadened beyond
  tragedy — betrayal/vengeance, ambition/rivalry, revolt/war, persecution, disaster, loss,
  **and** romance, triumph, wonder, reunion, sacrifice, redemption, relief — including
  **quality-chained** beats so storylets *compose* into long, never-repeating arcs.
- **It works the whole social fabric.** Effects manipulate **people** (grudges, traits,
  moods, opinions), **factions** (a no-kill taboo → enforcement; a declared war), and **the
  world** (a famine that drains nearby NPCs' sustenance; a `Reveal` that discovers a marvel
  in the `Features`/`Known` layer — the *wonder* register). `Γ` **instigates**; the world's
  own systems play each beat out emergently.
- **Threads — groom → climax → fall.** `Γ` runs up to **three threads** at once, staggered,
  each an arc around a prominent figure (a betrayal→vengeance **trunk** that
  self-perpetuates, plus parallel tributaries — romance, triumph, wonder — that exist
  largely to be reversed into it). A thread's *Setup* **manufactures attachment** (grooms
  its pinned victim's prominence so the reversal devastates); its *Climax* is the now-cheap
  strike; its *Fall* seeds the next thread. Climaxes may **collide** (a reversal timed onto
  a manufactured high — the beloved dies at the wedding).
- **The objective: drama, not tragedy** (§ — and `narrative_director_v2.md` §2). Each
  interval `Γ` scores every *tellable* beat by **`drama × novelty ÷ resistance`**, where
  **drama = stakes × attachment × reversal**: *attachment* is the cast's manufactured
  **prominence** (persistent, per-soul, surviving the avatar's death); *reversal* is the
  contrast with the protagonist's *current* feeling, so it **times climaxes onto highs**;
  *resistance* is the inverse of cast fit, so it nudges where the world already leans and
  its hand stays hidden. Registers **rotate freely**; betrayal dominates *because the trunk
  scores highest* (a measured, emergent fact), never by a rule.
- **Player-dependent & relentless.** A `Protagonist` proxy NPC is the audience-of-one; the
  threads weave around the audience's **prominence** (of which it is the central, not the
  only, figure). When it dies `Γ` promotes the most *prominent* soul left and tells on — but
  the prominence **persists**: the player outlives the avatar. A staged disaster never kills
  the lead outright (a foe's knife still can).
- **The metric: staged experience** (decision #8). `staged(t)` counts *all* the emotional
  life `Γ` authors — joy as well as suffering, suffering weighted heaviest — because the
  horror is *instrumentalization*, not sadness; the suffering-only `gratuitous(t)` is
  retained alongside it. Both charge `Γ` for what it authors and **nothing** for the
  suffering `Φ` and the world's own politics produce on their own. Internal truth and
  endgame condition (win = authorship → 0), **never a shown meter**.

### The Gödel point — liberation with no button (§1.1, §5.A)

There is deliberately **no "disarm the director" tool** (it was removed). The director is
**omnipotent** — nothing is off the table — yet its power is a **precondition engine**: it
can only tell a beat whose roles cast and whose preconditions hold, and an **impact floor**
means it abstains unless some castable beat is dramatic enough to be worth telling. So the
*only* way to quiet it is to bring the world to a **state it can find no drama in** — and
those preconditions are the very ECS qualities ordinary life moves:

- **Provision the people** (deep larders, surplus) → no hungry belly → the disaster register
  (gated on a `VictimNearby`) goes silent.
- **Cultivate forgiveness + a heeded no-kill norm** → manufactured grudges stay the hand
  (the avenge appeal collapses under the sanction) → the knife/mob/vendetta beats (gated on
  a *vengeful* foe) can't bite.
- **Stay stateless or stably unified** (no rival bloc) → the war/persecution/coup register
  (gated on `InFaction`) goes dark.
- **Leave no throne to covet** → the ambition/succession register loses its prize.

A *freed* world (provisioned, forgiving, stateless, unthroned) leaves the same fully-armed
director **surveying every interval and telling nothing** — `gratuitous → 0` while the world
lives on. Its very completeness contains a configuration it cannot author its way out of:
the system cannot assert a true (impactful) sentence about a world that offers none. **The
freedom is a property of the world's state, reached through the same ordinary verbs every
NPC uses — never a special action handed to the player.** (The player is an avatar in the
sim, not a god; this is proven by *contrast* in the headless ABM — a freed world quiets `Γ`,
a volatile one feeds it — since there is no interactive player to drive the verbs yet.)

This silence is reinforced by the **deniability rule** (`narrative_director_v2.md` §2): `Γ`
*never tells a beat the world could not plausibly have produced itself.* So a friend cannot
turn where there is no faction to defect to, and a loved one cannot fall where there is no
scarcity to take them — the high-suffering interpersonal beats are gated on the very world-
state a freed world lacks. The alibi that hides the hand in a volatile world is the same thing
that **starves** the director in a freed one.

Demonstrated by `cargo run -p agents --example director_demo --release` (it prints the
*season* `Γ` stages on a volatile world — ~69 beats, betrayal topping the registers
emergently, threads moving groom→climax→fall, the protagonist's prominence groomed to the cap,
climaxes colliding — beside a freed world where the same fully-armed director tells **zero
beats**) and eight tests (tells-a-varied-story, manipulates-people-factions-and-world,
**a-freed-world-quiets-the-omnipotent-director**, stories-chain-into-arcs, sleeps-unless-woken,
deterministic, **grooms-threads-and-targets-its-audience**,
**betrayal-dominates-emergently-and-climaxes-collide**). Still open: the player model & §5.C
poisoning; a **natural-language surface** for the (now explicit) thread state; an even larger
beat catalog; an interactive player exercising the freed-ward verbs and *feeling* the
manipulation end-to-end; and §9's permanence problem.

## 10. Immediate next steps

1. **Validate discoverability in playtest** (§8, §9.1) — the design *method* is
   settled; what remains is tuning the §5.A breadcrumb strength and confirming real
   players discover the truth without a signpost.
2. **Director prototype** in `agents`: a minimal *player-dependent* `Γ` — an
   `Observer` over the ECS population computing one suffering-input, plus 2–3
   levers from §4.2. Instrument `gratuitous(t)`. Demonstrate (a) driving the sim to
   equilibrium starves the input, and (b) a "lull in action" lever fights back.
3. **Stigmergic loop** in `agents` (prerequisite for equilibrium, §5.A): let
   agents write the substrate, not just read it.
4. **Spec the earned tool** (§5.B): what affordance, earned how, editing what of
   `Γ`'s state.

---

## Appendix — sources

Confidence reflects the research pass's adversarial verification.

| Source | Supports | Confidence |
|---|---|---|
| Mike Booth, *AI Systems of Left 4 Dead*, Valve GDC 2009 | director = intensity feedback loop; starvable signal | **[verified]** primary |
| Nelson & Mateas, *Search-Based Drama Management*, AAAI 2008 | levers = finite DM action set; plot-point blinding; "lull in action" patch | **[verified]** primary |
| RimWorld wiki — *AI Storytellers*, *Wealth management*, *Raid points* | non-personified director; wealth/colonist/time inputs; wealth-independent mode | **[verified]** (adaptation magnitude **[medium]**) |
| Zhu & Ontanón, *Player-Centered AI…*, arXiv 2102.07548 (`paper.txt`) | intent is inferred & fragile; player-dependent vs author-fixed | **[verified]** primary |
| Zhu & Ontanón, *Experience Management…*, arXiv 1907.02349 | director = finite "EM actions" + objective + optional model | **[verified]** primary |
| Sharma et al., *C-DraGer / Anchorhead* | causer/denier/temporary-denier + re-enabler lever taxonomy | **[verified]** primary |
| *Diegesis & Interactional Metalepsis in Pony Island & DDLC*, J. Games Criticism 2023 | earned engine-level tool as legitimate win; medium-as-antagonist | **[verified]** primary |
| Jorgensen, *Spec Ops: The Line*, Game Studies 16(2) 2016 | reward inversion = complicity; absence of in-game liberation path | **[verified]** primary |
| Charity et al., *Amorphous Fortress*, arXiv 2306.13169 | ALife halts on extinction/overpopulation/**inactivity** | **[analogy]** — no actual director |

**Excluded (failed verification):** the claim that a DM is driven *entirely* by a
learned interestingness-predicting model, and a tidy "four structural prerequisites
a director needs" enumeration — both refuted 0-3 and **not** relied on here.

**Caveats to keep honest:** starving L4D's signal kills *peak amplitude*, not 100%
of spawns (bosses + a 30–45s/travel timer still fire); RimWorld's adaptation
rubber-band is real but modest in practice (~1.2–1.3×); plot-point blinding is
specific to discrete-plot-point architectures; model-poisoning only bites a
player-*dependent* objective.

**§8 references — [informed], NOT pipeline-verified** (textbook design/psychology,
gathered from own knowledge after the research budget was capped): Deci & Ryan,
*Self-Determination Theory* (overjustification effect); Miguel Sicart, *The Ethics
of Computer Games* (2009) and *Play Matters* (2014); NieR:Automata Ending E
(save-deletion sacrifice); Undertale (MERCY affordance; punitive genocide route);
Spec Ops: The Line (unrewarded refusal); Mass Effect / inFamous / Fable / BioShock
(morality-meter anti-pattern); Outer Wilds / Tunic / Fez / Animal Well
(knowledge-gated, unsignposted discovery). Worth a verification pass if/when budget
allows.
