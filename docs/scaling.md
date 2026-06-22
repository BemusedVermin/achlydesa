# Scaling toward millions of agents -- Analysis & Plan

> Working design doc. Analysis + proposal, **not yet built**. Captures where the
> per-tick budget goes today, why "more ECS" alone will not reach millions, and a
> staged path (tiers + stigmergic fields + an aggregate economy) that will -- all of it
> off-by-default and byte-identical, per the non-negotiable invariants in `CLAUDE.md`.

## The goal, and the honest reframe

We want a world that holds **millions of agents** (and therefore millions of agents'
worth of emergent content). The working theory was "we are not using ECS hard enough --
lean into Bevy parallelism." That theory is half right and worth acting on, but it does
not get us to millions on its own. The reason is arithmetic:

- The dominant per-tick cost is **per-agent GOAP A\* planning** (`people_plan` in
  `agent_core/src/people.rs`, `NODE_BUDGET = 600`). Each replanning agent expands up to
  600 search nodes, and each node generates on the order of `2*M*G + R + A` successors
  (M markets, G goods, R recipes, A affordance sites) -- the `2*M*G` term is buy/sell
  trade operators against every market.
- `par_iter` over that work buys a **constant factor** equal to core count (~10-20x). It
  does **not** change the complexity class. Going from ~10^3 to ~10^6 agents needs a
  ~1,000-10,000x reduction in per-agent cost, which is an order-of-magnitude *class*
  change, not a constant factor.

So the reframe in one line:

> **Stop trying to make a million brains cheap. Make the world cheap to be alive in, and
> spend brains only where the player and the director are looking.**

This is not a compromise away from the design -- it *is* the design. The narrative
director already manufactures prominence and stages drama around a small cast near the
avatar (`docs/narrative_director_v2.md`); the LOD layer already exists to coarsen distant
souls; the substrate is already a diffusing-field solver. The scalable architecture is
the natural extension of three things we already have.

## Where the per-tick budget actually goes

Measured by code inspection across `agent_core` and `agents`. Listed worst-first.

| System (file:symbol) | Cost | Scales as | Why |
|---|---|---|---|
| `people.rs:people_plan` | **dominant** | O(N * 600 * (2*M*G + R + A)) | GOAP A\* per replanning agent; trade-operator explosion per node (`plan.rs:successors`) |
| Per-tick `HashMap` rebuilds | heavy | O(N) alloc/tick | `people.rs:people_execute` rebuilds `people_pos` and `vassals_of` every tick; `factions.rs:faction_turn` builds a `loyalty: HashMap<(Entity,Coord),f32>` cross-product (O(N*S)) |
| Per-entity heap | memory wall | ~7-9 allocs/agent | `Personality`/`Mood`/`Skills` are `Vec<f32>`, `Opinion` is `HashMap`, `Plan.steps` is `VecDeque`, `Known` is `HashSet` (spawn bundle in `people.rs`). At 1M agents that is ~8M live allocations. |
| `dialogue.rs:converse` | O(N^2) on crowded tiles | no spatial index | co-location is found by grouping, degrades when many share a tile |
| `factions.rs:faction_turn` | O(N*S) + clones `Personality` per member | every `period` ticks | leader election clones trait vectors per member |
| `director.rs:director_step` | O(N) scan + O(B*C) casting | per tick | snapshots all souls; casts B beats against C candidates |
| `people.rs:smooth_prices`, `mood_*`, metabolism | O(N) or O(M*G) | linear | cheap, embarrassingly parallel, mostly fine |

What is already **right** and should not be "fixed":

- `people_plan` is genuinely parallel and deterministic: each agent reads a read-only
  start-of-tick snapshot (market prices, affordance availability, per-tile resource
  cache) and writes **only its own `Plan`**, so thread order cannot change the result.
- The **LOD layer** (`agent_core/src/lib.rs:lod_dormancy`, the `Dormant` component,
  `Setup::sim_radius` / `sim_far_stride`) already does the hard part correctly. Distant
  agents run on a **staggered coarse clock** (`tick % stride == entity_bits % stride`),
  and `people_metabolism` filters `Without<Dormant>` -- so **dormant agents do not starve
  while you are away**; they age slower instead of dying off-screen. It is opt-in and
  byte-identical when `sim_radius = None`.

## Determinism: the constraints any change must respect

These are the invariants from `CLAUDE.md`. They shape every option below.

1. **Single-threaded, fixed-order schedule** (`ExecutorKind::SingleThreaded`). Parallelism
   is allowed *inside* a system only when each unit of work writes its own state from
   read-only shared state (the `people_plan` pattern).
2. **Separate, derived RNG streams.** Each subsystem seeds a dedicated `SplitMix64` by
   xoring the run seed with a distinct constant (economy, feature placement, personality,
   professions, rpg, director, dialogue, predation -- see `agents/src/lib.rs`). A new
   subsystem must take a **new** constant, never draw from an existing stream.
3. **Off-by-default = byte-identical.** Every optional layer early-returns when disabled
   and keeps state in its own resource/component.
4. **Integer economy.** `total_money()` is an exact invariant; trade only moves coins.

The systems that are **order-coupled today** -- and therefore cannot simply be
parallelized -- are `people_execute` (serialized trades against shared markets; despawn
order feeds vassal inheritance), `faction_turn` (tax transfers, casualty selection),
`director_step` (in-tick precondition checks + RNG draw order), and `converse` (shared
mood/opinion writes, speaker cooldown in iteration order). The plan below does not fight
this; it **shrinks N for these systems** so they stay cheap while serial.

## Track 1 -- optimizations within the current model

Do these regardless of the bigger decision. They are pure wins and several are
prerequisites for Track 2. Expected envelope: **~10^3 -> ~10^4-10^5** full agents at
interactive tick rates.

1. **One spatial bucket grid.** A `Resource` of `Vec<SmallVec<[Entity; k]>>` indexed by
   tile, rebuilt once per tick (or maintained incrementally on the Move step). Every
   "who is near whom" query -- execute-phase strikes, `converse`, director casting --
   reads it in O(local) instead of O(N) scans or O(N^2) clustering. **Highest-leverage
   structural fix**, and it kills the `people_pos` rebuild.
2. **Struct-of-arrays for hot per-agent state.** `Personality`, `Mood`, `Skills`,
   `Inventory.stock` are fixed-width per world. Store them as dense arrays in a SoA
   resource indexed by a stable dense agent id (or at minimum as fixed `[f32; K]`
   components instead of `Vec`). Removes ~8M allocations at 1M agents and turns
   `mood_decay` / `mood_shapes_traits` / metabolism into cache-friendly, SIMD-able
   streaks.
3. **Shrink the GOAP inner loop.** Restrict trade operators to *known/reachable* markets
   (we already track `Known`) and to goods with a live deficit; lower `NODE_BUDGET` for
   non-prominent agents. Cost tracks `M*G` -- cut that and the dominant term drops.
4. **Parallelize the safe remainder.** `smooth_prices`, `mood_*`, metabolism, and
   discovery are trivially `par_iter` (own-state writes). Leave the order-coupled systems
   serial; Track 2 removes the need to parallelize them.

## Track 2 -- the model that reaches millions

Three interlocking ideas. Each generalizes something we already have.

### 2a. Three tiers of fidelity (generalize `Dormant` into a spectrum)

- **Tier 0 -- full brain (hundreds).** IAUS + GOAP + dialogue + appraisal. The cast near
  the avatar and **whoever the director has cast or made prominent**. We already compute
  this set (`Protagonist` + prominence + avatar radius).
- **Tier 1 -- utility-only (thousands to tens of thousands).** IAUS picks a goal (cheap,
  no A\*); movement and logistics are **field-following** (2b); needs are met by local
  rules. No per-agent search.
- **Tier 2 -- statistical cohorts (the millions).** Not individual entities between
  events. Each settlement/region holds **population cohorts** (counts by calling, faction,
  mood band) and the economy runs as **integer flows** (production, consumption,
  migration, births/deaths). Individuals **crystallize** into real ECS entities only on
  **promotion** (the player approaches, or the director needs a face) and **dissolve**
  back when they leave the lens.

Promotion/demotion is a deterministic function of distance + director interest. This is
the "managed mass" approach (Dwarf Fortress / CK3): a million souls exist as content, but
only the relevant few hundred are ever fully simulated *at once*. Content and drama do not
come from a million brains -- they come from the right cast, which the director already
manufactures. The masses are the texture and the **pool to promote from**.

### 2b. Stigmergic fields (the Emergence lesson) -- and we already own the engine

The reference project, Leafwing's **Emergence** (an organic factory-builder; repo archived
Nov 2023, design book at leafwing-studios.github.io/Emergence), scales its colony with
**signals**: every object emits a scalar that **diffuses and decays** over the tilemap, and
units **follow gradients** ("nudge, not command") instead of path-planning. The payoff:
diffusion is O(tiles) and **independent of unit count**; gradient-following is O(1) per
agent. That is exactly the decoupling we need.

We do not need a new subsystem for this. **`game_sim`'s substrate is already a
diffusing-field solver** (climate/ecology fields, advanced each tick by `advance_substrate`).
Add a handful of **resource/demand fields** ("food here", "market wants grain", "danger")
as additional substrate layers. Tier-1/2 agents then move by sampling the local gradient
(6 hex neighbors) rather than running A\*. This collapses the dominant cost for everyone
except Tier 0. The `world-model-review` punch-list already flagged "add stigmergy" -- this
is where it pays off.

### 2c. A regional economy removes the determinism bottleneck

The reason `people_execute` must be serial is the **globally contended market** (integer
money conservation across one shared pool). Make the **bulk** economy **regional**: markets
clear locally per settlement as aggregate flows. Regions are then independent, so
execution **shards by region and parallelizes** while still conserving money exactly
(integer in/out per region balances). Tier-0 individuals still trade coin-for-coin at their
local market; the masses trade as cohort flows. `total_money()` stays an exact invariant.

## Continuous / background simulation

Today `app` steps the sim **once per player action** (`drive_sim`), on the main thread,
holding `Simulation` as a `NonSend` resource. Making it continuous means moving the whole
`Simulation` onto a **dedicated thread** driven by a fixed-timestep accumulator, with the
renderer reading **double-buffered snapshots**. `Simulation` owns its own `bevy_ecs::World`,
so this is clean -- the only reason it is `NonSend` is convenience, not a real constraint.
Determinism is preserved (tick order stays fixed; the thread just owns the loop), and render
fps decouples from sim tps.

Important: this is **orthogonal to scale**. It does not by itself enable millions -- each
tick must still fit the wall-clock budget. But a fixed real-time tick budget **forces** us
to bound per-tick work, which is precisely the discipline that makes Track 2 honest.
Continuous + tiering is the right pairing.

## Determinism: why none of this breaks the invariants

- **Field diffusion** is a deterministic stencil; the fields get a new derived RNG stream
  (new xor constant) if they need any randomness at all.
- **Tier transitions** are deterministic functions of state; Tier-1 agents write only their
  own components (the `people_plan` pattern), so they are `par_iter`-safe.
- **Aggregate economy** is integer flows -> `total_money()` remains exact.
- **Off-by-default**: with the feature disabled, every agent is Tier 0 and the world is
  bit-for-bit the current one. Tiers collapse to "all full brain"; fields are absent;
  the economy stays global.

## Staged roadmap

1. **Track 1 now** -- spatial bucket grid + SoA packing + GOAP operator pruning. Standalone
   wins; the grid and SoA are prerequisites for tiers.
2. **Prototype 2b (fields)** as new substrate layers; let Tier-1 agents follow gradients.
   Biggest conceptual lever, reuses machinery we already trust.
3. **Then 2a/2c** -- cohorts + regional economy for the actual millions, with continuous
   background stepping layered in.

## Open questions / risks

- **Promotion fidelity.** When a Tier-2 cohort crystallizes into individuals, their state
  (skills, money, relationships) must be reconstructed deterministically and plausibly from
  cohort aggregates. Getting this seamless (no "pop-in" of personality) is the hard part.
- **Field expressiveness.** Gradient-following covers "go toward X." Goals that are not
  spatial-gradient-shaped (revenge against a specific moving foe, multi-step crafting) still
  need real planning -- those agents must be Tier 0/1, which bounds how cheap the mass can be.
- **Director reach across tiers.** The director must be able to reach into Tier 2 to stage
  drama (promote a cohort member into a face). The seam between statistical masses and the
  drama manager needs design (`docs/narrative_director_v2.md`).
- **Benchmark first.** Before committing to numbers, a headless bench that scales N and
  reports per-system tick cost would turn the estimates above into measurements.

## Relationship to existing docs

- `CLAUDE.md` -- the non-negotiable invariants (determinism, off-by-default, RNG streams,
  integer economy) that constrain all of the above.
- `docs/narrative_director_v2.md` -- the drama manager whose prominence selection *is* the
  Tier-0 cast.
- `game_sim/docs/simulation_details.md` -- the substrate field solver that stigmergic
  fields extend.
