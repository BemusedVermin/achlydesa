# The Agent Model — an ODD description

This describes the `agents` crate's agent-based model following the **ODD protocol**
(Overview, Design concepts, Details; Grimm et al., *A standard protocol for
describing individual-based and agent-based models*, 2006/2010/2020). ODD is the
field standard for making an ABM reproducible and reviewable; this document is the
agent layer's reference, kept in step with the code. The physical substrate it runs
on is documented separately in `game_sim/docs/simulation_details.md`.

---

## 1. Overview

### 1.1 Purpose
Model a society of autonomous people on a climate/ecology substrate so that
**economic and social structure emerges** — division of labour, trade, prices,
factions, feuds, the use of places — from individual decision-making rather than
authored scripts. There are no `baker` or `lord` classes; those are *outputs*. The
model is the world the *narrative director* (`docs/narrative_director.md`) will later
act on, and its near-term target is a self-sustaining equilibrium.

### 1.2 Entities, state variables, and scales
- **NPC** (a person; `people::Npc`). State: `Position` (a hex), `Needs`
  (`sustenance`, `rest`, `0..100`), `Skills` (proficiency per trade — also the
  **calling**: a `0` is a trade never learned and so never practised), `Inventory`
  (`money` plus whole-unit goods), `Personality` (innate trait vector),
  `Mood` (transient emotion vector), `Plan` (current goal + cached steps), `Known`
  (the tiles whose hidden features it has discovered — a private map), and optional
  relational components — `Patron`, `Grievance` (a foe), `Liege` (a lord).
- **Fauna** — `Herbivore`s graze the substrate; `Carnivore`s hunt the herbivores
  (Holling type-II predation), closing the trophic loop with `Energy` as the shared
  survival meter.
- **Market** (`people::Market`). State: per-good `stock`, a coin pool `money`, and a
  smoothed `price_basis` (the lagged stock prices are read from).
- **Tile features** (`features::Features`, `FeatureCatalog`). Settlements, courts,
  ruins, wonders placed on tiles, each with a discovery tier and authored
  **affordances**; resolved into live `WorldAffordances` (smart-object actions with
  depletion state).
- **Faction** (`factions::Faction`, in the `Factions` resource; persistent by seat). A
  bloc around a court with a `Government`, ruling `leaders`, members, `laws`, the rivals
  it is `at_war` with, and Force/Cunning/Wealth. Each NPC carries an `Allegiance` (a list
  of bonds — court + loyalty — so it may belong to several), an `Opinion` (directed,
  sparse, of the leaders it has served and rivals it has fought), and may bear a
  `Detained` marker while held by enforcers.
- **Shared world facts**: a `Throne` (one ruler), abstract `facts` the planner
  grounds relations to.
- **Resources** (global): `Registry` (authored goods/recipes/skills/traits/…),
  `Goals`, `Norms`, `Appraisals`, the three config structs, `SimRng`, `Substrate`.
- **Scales**: one tick = one day; space is a cylindrical hex grid (E–W wrap, polar
  caps). Runs are tens of thousands of ticks over grids of a few thousand tiles.

### 1.3 Process overview and scheduling
A single tick runs these systems in a fixed chain (single-threaded executor;
per-agent work inside a system is order-free and parallel):

1. `advance_substrate` — the substrate evolves one day (`Φ`).
2. `fauna::forage`, `fauna::lifecycle`, `fauna::hunt`, `fauna::carnivore_lifecycle`
   — herds graze (dispersing by forage) and breed under a crowding cap; predators
   hunt them (Holling type-II) and breed or starve.
3. `people_plan` — each person picks the most appealing goal it can act on (utility
   AI) and **searches** a plan to satisfy it (GOAP). Order-free, parallel.
4. `people_execute` — each performs the next step of its plan against the live world
   (trade, produce, gather/relieve at a feature, move, a deed).
5. `smooth_prices` — each market's `price_basis` chases its real stock (EMA).
6. `discover_features` — a hex an NPC stands on yields up its Hidden features.
7. `appraise` → `mood_shapes_traits` → `mood_decay` — events become feelings,
   feelings slowly reshape character, feelings fade.
8. `people_metabolism` — needs drain; the starved die.
9. `regen_affordances` — worked feature sites refill toward capacity.
10. `faction_turn` — every `period` ticks: factions are governed, taxed, legislated,
    fought, and policed.
11. `detention_countdown` — release law-breakers whose detention has run out.

The schedule is fixed and all randomness comes from a seeded RNG, so a run is
reproducible.

---

## 2. Design concepts

- **Basic principles.** Utility AI in the Infinite Axis style (Dave Mark's IAUS) for
  *what to want*; GOAP (Orkin) for *how to get it*; agent-based computational
  economics (Tesfatsion; Sugarscape) for the price-from-stock market; *The Sims*
  smart-object/affordance model for places; Crusader-Kings/WWN lineages for
  relationships and factions.
- **Emergence.** Professions, trade routes, prices, settlement use, and feuds are not
  coded; they arise. A baker exists because someone *born to baking* (calling) buys
  grain and bakes because, at the local price, that pays and feeds it.
- **Adaptation.** Each tick an agent re-chooses its goal by appeal and re-plans if its
  plan is spent or its goal changed. Opportunity-gating ("can I even progress this?")
  falls out of "is there a plan?".
- **Objectives.** A goal is a `Condition` (a desired world-state) with an authored
  appeal (IAUS considerations over deficit, personality traits, mood, and deontic
  sanction). The chosen goal is the most appealing unsatisfied, un-vetoed one that a
  plan exists for.
- **Learning.** Practising a trade raises its skill (yield and the specialisation
  spiral); sustained mood reshapes innate traits over a life. A born calling is fixed
  *unless* the agent apprentices at a **guild's teach affordance**, which lifts a new
  skill above zero — occupational mobility, found by the planner as a "learn the
  trade, then practise it" plan.
- **Prediction.** Implicit and short-horizon: the planner plans the next *leg* of a
  standing goal and replans, and prices it against a lagged `price_basis` so plans'
  expected payoff matches what they get.
- **Sensing.** An agent reads its own needs/skills/inventory, the resource levels and
  affordances of reachable tiles, the start-of-tick market snapshots, the positions of
  others it has a relation to, and shared facts (the throne). Feature **discovery is
  per-agent**: each carries a private map (`Known`) and may only act on a Hidden/Secret
  feature it has personally visited — knowledge spreads by exploration, not omniscience.
- **Interaction.** Trade (buy/sell move coins and goods, exactly conserved), deeds
  (seize the throne, strike a foe), and the inheritance of grudges (a slain lord's
  vassal takes up the quarrel). Norms (permitted/forbidden/obliged acts) shape which
  interactions an agent will pursue.
- **Stochasticity.** One seeded `SplitMix64`. Independent concerns draw from separate
  substreams (economy placement, personalities, callings, feature placement) so adding
  one never perturbs another.
- **Collectives.** Factions are higher-order actors: a bloc of people loyal to a court,
  carrying aggregate WWN stats (Force/Cunning/Wealth) summed from its members and running
  the loop one level up — each faction turn it competes with rivals for those members
  (loyalty follows power). The *skeleton* (formation, consolidation, rise/fall) is built;
  faction **actions** that reach back into the world (war, taxation, commands) come next.
- **Observation.** `observe::Census` snapshots population, wealth, goods, the emergent
  professions, feature use, and the factions each tick; `observe::check` asserts the invariants
  (money conserved, population non-increasing, prices in band, affordance uses
  monotone) that separate genuine emergence from artefact (Galán & Edmonds, 2009).

---

## 3. Details

### 3.1 Initialization
From a `Setup`: generate and warm the substrate, place tile features (own RNG
substream), resolve their affordances, spawn fauna, then markets (scattered on
fertility, or seated in settlements), then NPCs in market catchments. Each NPC is born
with a personality near the trait baselines (separate substream), a **calling** of
`professions_per_agent` trades (round-robin primary so every trade is covered;
separate substream), a starting larder, and an endowment of coins. Optional scenario
hooks add ambition, feuds, vassals, and a throne.

### 3.2 Input data
All content is authored RON in `assets/data/` and loaded into the `Registry` /
catalogs: `goods`, `recipes`, `skills`, `traits`, `moods`, `predicates`, `verbs`,
`norms`, `appraisals`, `goals`, and `features` (with their suitability and
affordances). Adding a good, recipe, goal, norm, or feature is a data edit — no Rust.

### 3.3 Submodels
- **Utility (appeal).** `score = ∏ considerations` with the IAUS makeup compensation;
  a goal below the appeal floor is vetoed (the channel a taboo restrains an act
  through). See `ai.rs`, `goals.rs`.
- **Planning (GOAP).** Forward A\* over a symbolic `PlanState` (needs, money, stock,
  position, abstract facts). Operators: eat/graze/rest, one per authored recipe
  (**gated by calling**), buy/sell at reachable markets, **use** a feature affordance,
  place-based **deeds**, and move. Bounded by a node budget; a goal past the budget is
  approached leg by leg. See `plan.rs`.
- **Specialisation.** A recipe is runnable only where `skill > 0`; born callings are
  seeded, the rest stay `0` and untrainable, so production splits across people and
  trade for what you cannot make becomes necessary. See `Skills`, `birth_skills`.
- **Feature affordances (smart objects).** Each feature advertises actions —
  `Relieve` a need (an oasis feeds, a monastery rests), `Yield` a good (a mine, gated
  by calling), or `Teach` a calling (a guild). The planner routes to and uses
  available sites — but only ones it has *discovered* (a Hidden site needs the agent's
  `Known`), and `Teach` is found via a `learned` overlay on the plan state so "learn
  then practise" emerges. Working a depletable site draws it down (stigmergy);
  `regen_affordances` refills it. See `features.rs`, `people::{build_affordances,
  regen_affordances, discover_features}`.
- **Predation (trophic loop).** Carnivores move to the neighbouring tile with the most
  prey and kill at the Holling type-II rate `a·N / (1 + a·h·N)` (saturating in prey
  density `N`), the fractional part resolved by a draw from a dedicated fauna RNG, and
  only above a **spatial refuge** density so scattered prey survive (preventing
  predator-driven extinction). A fed predator breeds, a hungry one starves — top-down
  control over the herd, which is itself regulated from below by forage and a per-tile
  crowding cap. Herbivores also seek **company** (`herd_cohesion`), coalescing into moving
  herds dense enough to graze well *and* to be worth hunting. With the substrate's soil
  feeding back (Q10 decomposition + soil-carbon→carrying-capacity) and growth combined by
  **Liebig's minimum** (a much greener world), the loop now reaches genuine **predator–prey
  coexistence** — both tiers persist in a sustained oscillation. See `fauna.rs`, `game_sim`.
- **Factions.** Persistent by court seat; every `period` ticks a faction is governed,
  taxed, legislated, fought, and policed. Its **government** comes from the court kind
  (guild → oligarchy, temple → democracy, royal seat → monarchy) and sets who leads (the
  most ambitious; a wealth-and-ambition council; or the elected *median voter* — least
  total personality-distance to the members) and how hard it taxes. **Tribute** flows
  member→leader (conserved), costing **loyalty**; loyalty also rises with pride in a
  strong bloc and drifts to baseline. People may belong to **several** factions: each is
  pulled to the strongest courts within reach — pull `= (1 + Force)/(1 + distance)`, bent
  toward a current faction if loyal and off it if soured — taking up to `max_factions`,
  honouring **exclusion laws**. A faction lays **laws** on members: a `Taboo` (e.g. a
  no-kill law) and `Exclude` (no dual membership, imposed by war). **Enforcement**: a
  no-kill faction **detains** members who hold a grudge (`Detained` → cannot act), and a
  strong one **executes** a repeat offender; a member is also *reluctant* to break a
  faction law, its taboo folded into its effective `Norms` so its goal appeal is
  suppressed. **War**: out-sized neighbours fall out (mutual exclusion) and the stronger
  inflicts casualties; a warring faction also **commands** its keenest member as a
  champion against the enemy's head. People accumulate **opinions** of the leaders they
  serve and rivals they fight, and are drawn toward leaders they like — so allegiance
  follows relationships, not only power. Deterministic. See `factions.rs`.
- **Market & prices.** `price = base · target / max(basis, ε)` clamped to a band;
  `basis` is an EMA of stock (`smooth_prices`), so prices move gradually and the
  cobweb is damped. Trade is exactly money-conserving. See `people::price`,
  `EconConfig`.
- **Norms (deontic).** Acts are permitted/forbidden/obliged with specificity ordering;
  the net sanction feeds a goal's appeal, and a forbidden act carried through is
  appraised as a transgression. See `norms.rs`.
- **Personality, mood, appraisal.** Innate traits bias appeal; events spike moods;
  moods slowly reshape traits; moods decay. See `events.rs`, `people::{mood_*}`.
- **Discovery.** Landmarks are known on placement; an NPC searching a hex reveals its
  Hidden features; Secrets stay latent. See `people::discover_features`.

---

## 4. Known gaps (relative to the design vision)
Factions are now a full political system — government, law (internalised in members'
*appeal* and enforced), tribute, command (war-champions), an opinion graph, and **war**.
What remains is deeper: **command** beyond drafting champions (a leader steering members'
ordinary plans/goals); a *full pairwise* opinion graph (today people hold opinions of the
leaders they serve and rivals they fight, not of every co-member); and first-contact
reaction rolls. On the
ecology side the trophic loop now reaches **genuine predator–prey coexistence** (Liebig
productivity + herd aggregation + a patient pack + a refuge), so the standing-predator-tier
gap is closed; what's left is a richer **moisture model** and C:N soil stoichiometry.
Discovery is per-agent but doesn't spread by **word of mouth**; the teach affordance widens
callings but agents seek it only when their born trade can't serve a goal. These are the
next steps, not silent omissions.
