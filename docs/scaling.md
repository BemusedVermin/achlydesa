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

## Track 1 -- what shipped (branch `scaling-track1`)

Built and measured; SoA packing and the continuous background thread were deferred by
decision (a follow-up pass). Everything here is **byte-identical** unless explicitly opted
into (the plan-budget knob), guarded by a pinned state-fingerprint of three reference runs
(`agents`'s `track1_runs_are_byte_identical_to_master`).

- **Benchmark first.** `cargo run -p agents --release --example bench_scaling [ticks]
  [n1,n2,...] [WxH]` grows N and reports ticks/s, µs/tick, **µs/agent**, **allocs/tick**, and
  a determinism fingerprint per config. It attributes cost across layers by toggling the
  optional ones (economy → `+dialogue` → `+director`), and reads `BUDGET=n` for the planning
  knob below. `Simulation::fingerprint()` is the read-only, order-/toolchain-independent fold
  the guard pins.
- **GOAP successor prune (byte-identical).** `apply()` clones the `PlanState` *before*
  checking preconditions, so every rejected successor pays a clone — the dominant per-tick
  allocation. `successors()` now skips the operators `apply()` is guaranteed to reject on the
  same checks (`Make` outside the agent's callings — most of a registry; `Eat` of un-held
  goods; `Graze` on barren tiles), leaving the search tree identical. ~8% fewer allocs / ~8%
  faster planning at N=200–800.
- **`converse` tile-bucket (byte-identical).** Grouping candidates by tile once turns the
  per-speaker co-location scan from O(N) into O(tile-occupancy) — the whole loop from O(N²)
  to O(N) at bounded density. Measured: per-agent dialogue cost stays ~flat (7→9 µs) across a
  4× population at constant density, where a full scan would have grown it ~4×.
- **Configurable plan budget (off by default).** `Setup::plan_budget: Option<usize>` (a
  `PlanConfig` resource) replaces the hard-coded `NODE_BUDGET = 600`; `None` is the unchanged
  600-node search. This is the "lower NODE_BUDGET for the masses" lever the tiers will use.
  Measured: at N=800, 600 → 150 cuts planning from 628 → 177 µs/agent (3.6×) and 38.4M → 10.8M
  allocs/tick (3.5×).
- **Parallelize the safe remainder — measured, then *not* done.** The bench is decisive:
  planning is **~98% of the tick** (at N=800, 502 ms full vs a 9 ms floor with the budget
  forced to 1). The whole non-planning tail (substrate `evolve` — O(tiles), fixed — plus
  execute, factions, metabolism, discovery) is **<2%**, and those tail systems have shared
  writes (`Throne`, `Features`, despawn order) that make parallelizing them a determinism
  risk for sub-2% gain — and likely net-slower from `par_iter` dispatch overhead on trivial
  bodies. Planning (`people_plan`) and the mood systems are *already* `par_iter`. So the
  honest call is to leave the tail serial; the real lever is shrinking N for planning, which
  is Track 2.

The headline confirmed by measurement: **per-agent A\* planning is the whole cost**, and it
is already as parallel as the model allows. No amount of tail-parallelization or constant
factors changes the class — which is exactly why Track 2 (tiers + fields + cohorts) exists.

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
manufactures. The masses are the texture and the **pool to promote from**. **(Now built — see
"Track 2 -- what shipped": `agent_core::cohorts`, with crystallization to/from a bounded live cast.)**

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
is where it pays off. **(Now prototyped -- see "Track 2 -- what shipped" below.)**

### 2c. A regional economy removes the determinism bottleneck

The reason `people_execute` must be serial is the **globally contended market** (integer
money conservation across one shared pool). Make the **bulk** economy **regional**: markets
clear locally per settlement as aggregate flows. Regions are then independent, so
execution **shards by region and parallelizes** while still conserving money exactly
(integer in/out per region balances). Tier-0 individuals still trade coin-for-coin at their
local market; the masses trade as cohort flows. `total_money()` stays an exact invariant. **(Now
built — see "Track 2 -- what shipped": the per-region cohort economy is shard-ready, kept serial for
now.)**

## Track 2 -- what shipped (2a + 2b + 2c: tiers, fields, and the regional economy)

Built and measured on branch `scaling-track2-fields`. Everything here is **off by default and
byte-identical** when off (the Track-1 fingerprint guard still passes). All three Track-2 ideas
landed as a three-tier spectrum: **Tier 0** (full GOAP brain, near the avatar), **Tier 1**
(gradient-following "drifters", the fields prototype, 2b), and **Tier 2** (statistical cohorts whose
economy runs as integer regional flows, 2a+2c). Promotion/demotion moves souls between tiers as the
avatar — the lens — moves.

- **Generic stigmergy layers in the substrate (`game_sim`).** `World::install_stigmergy(&[StigConfig
  { diffuse, decay }])` allocates N zeroed, double-buffered scalar layers; `deposit(layer, coord,
  amount)` / `stig(layer, coord)` are the write/read hooks, and a diffusion-stencil + exponential-
  decay step (`update_stigmergy`) joins `Φ`. A layer carries **no meaning** -- `game_sim` stays
  agent-agnostic -- and the step is `O(tiles · layers)`, **independent of agent count**. Zero layers
  by default, so a stigmergy-free world is byte-identical. This is exactly the engine 2b called for,
  on the same machinery the climate fields already use.
- **Three fields, fed by agent-side deposit systems (`agent_core::fields`).** FOOD (from tile
  biomass), DANGER (from predators), DEMAND (from markets short of stock) -- deposited *before* `Φ`
  so the signal diffuses the same tick. The *meaning* (which index is which) lives in `agent_core`;
  the substrate just spreads the numbers.
- **The Tier-1 "drifter" brain.** `Drifter` generalises `Dormant` from "asleep" to "awake on a
  cheaper brain": when the layer is on, `lod_dormancy` makes every NPC beyond `sim_radius` a drifter
  instead of a coarse-clocked full brain. A drifter runs `drift` -- `O(1)`/tick, **no A\***: step one
  tile up the weighted gradient (toward food when hungry, toward demand when carrying surplus, away
  from danger), then meet needs by local rules (produce a calling-good, trade at an adjacent market,
  eat or graze -- honouring the same `HungerModel` as `people_execute`, so a drifter feeds itself as
  well as a planner). Deterministic and RNG-free (gradient ties break on a fixed entity-id rotation);
  serial like `people_execute` because it trades against shared markets -- which is fine, since each
  turn is `O(1)`, so even serial the masses are orders of magnitude under GOAP.
  `people_plan`/`people_execute` skip drifters (`Without<Drifter>`); the near cast stays full Tier-0.
- **Measured: per-tick cost decouples from N.** `bench_scaling`'s `fields(T1)` row (economy + fields
  + a tight radius + a spawned avatar) against the all-full-brain `economy` baseline, 96×72 world,
  120 ticks:

  | N | economy µs/agent | fields(T1) µs/agent | ratio | economy µs/tick | fields(T1) µs/tick |
  |---|---|---|---|---|---|
  | 500 | 710 | 423 | 1.7× | 354,000 | 212,000 |
  | 2000 | 764 | 69 | **11×** | 1,415,000 | **138,000** |

  The fields tick stays ~flat as N grows 4× while the economy scales linearly, so the per-agent ratio
  widens (1.7× → 11×); allocs/tick decouple from N too (fields ~9-14M vs economy 24.5M → 104M). The
  Tier-0 cohort is bounded by the radius and the masses are cheap, so total per-tick work stops
  tracking N -- the complexity-class change, not the constant factor `par_iter` buys.

- **Tier-2 statistical cohorts + the regional economy (`agent_core::cohorts`, 2a+2c).** The millions
  are **not entities**. Each region (a market) holds a `Cohort`: a population *count by calling*, a
  coin *pool*, an aggregate *sustenance*. `cohort_step` advances it as **integer flows** — production
  sells into the regional market, consumption buys food back out (setting sustenance), births/deaths
  grow or shrink the count, and people migrate toward better-fed regions. Cost is `O(regions ·
  callings)` + `O(regions²)` migration — **independent of headcount**, so a region of thirty souls
  and one of thirty million cost the same. The per-region step writes only its own region and
  migration is a snapshot-then-deltas pass, so it is **shard-ready (2c)** even though it is kept
  serial for now (it is already dwarfed by the crystallized cast's GOAP).
- **Crystallization (the 2a promotion seam).** `cohort_crystallize` promotes a **bounded** cast
  (`crystallize_cap`) of real ECS entities from a region when the avatar comes near, and **dissolves**
  them back into the count when it leaves — Tier 2 → Tier 0/1 and back. The live entity count stays
  small however large the stated population. A dedicated `CohortRng` reconstructs personalities so it
  perturbs no other stream; the lost-history "pop-in" the design flagged is reconstructed plausibly
  (a prototype's fidelity, the open risk below).
- **The integer economy holds across all three tiers.** Every coin flow — production, consumption,
  migration, *and* promotion/demotion — is an explicit integer transfer between a cohort pool, a
  market purse, and entity purses, so `total_money()` (now counting pools) is conserved exactly;
  **deaths are the only sink**, as for individuals. A test pins this across the economy and the
  promote→demote round-trip.
- **Measured: the millions are nearly free.** `bench_scaling`'s `Tier-2 cohorts` section, 96×72, 12
  regions, an avatar present, sweeping the stated population up by orders of magnitude:

  | stated | sustained souls | µs/tick | ns/soul | live cast |
  |---|---|---|---|---|
  | 1 M | 0.97 M | 30,000 | 31 | 24 |
  | 10 M | 9.7 M | 53,000 | 5.5 | 24 |
  | 100 M | 97 M | 58,000 | **0.6** | 24 |

  µs/tick stays ~flat while souls grow 100×, so ns/soul collapses toward zero — **~97 million souls at
  ~58 ms/tick**. (The tick is dominated by the bounded 24-agent crystallized cast's GOAP; the cohort
  economy itself is ~0.6 ms with no cast — `O(regions)`, not `O(souls)`.) That is the only thing that
  reaches millions: not faster brains, but *fewer* of them, with the mass carried as flows.

  The population now **holds** at the stated level (≈97% sustained) rather than collapsing — the
  earlier model's population-scaled food gave a constant ratio and ran away; a fixed land **carrying
  capacity** per region (fertility-weighted) anchors it. Demand is tracked **per good**, so a drifter
  routes the specific good it carries to where that good is short. Both landed in the follow-up below.

What is **still** deferred (honest scope): the per-region step is shard-ready but kept serial (it is
cheap next to the crystallized cast, so threading it buys little today); and **promotion fidelity** —
a crystallized member's skills/personality are reconstructed from aggregates (plausible, but not its
true lost history; relationships are not reconstructed at all). This is the remaining open risk.

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
   wins; the grid and SoA are prerequisites for tiers. *(Partly done — see "Track 1 — what
   shipped": the bench, the GOAP successor prune, the `converse` tile-bucket, and the
   plan-budget knob landed; SoA packing and a shared cross-system grid resource remain.)*
2. **Prototype 2b (fields)** as new substrate layers; let Tier-1 agents follow gradients.
   Biggest conceptual lever, reuses machinery we already trust. *(Done — see "Track 2 — what
   shipped": generic stigmergy layers in `game_sim`, the food/danger/demand deposits, and the
   Tier-1 drifter brain on a two-tier LOD; measured to decouple per-tick cost from N.)*
3. **Then 2a/2c** -- cohorts + regional economy for the actual millions *(Done — see "Track 2 — what
   shipped": statistical cohorts per region, an integer regional economy, and crystallization to/from
   a bounded live cast; measured at ~97M souls / ~58ms tick. Follow-up added a land carrying capacity
   (population now holds, doesn't collapse) and per-good demand. Remaining: shard the region step
   across threads, and higher-fidelity promotion.)*

## Open questions / risks

- **Promotion fidelity.** When a Tier-2 cohort crystallizes into individuals, their state
  (skills, money, relationships) must be reconstructed deterministically and plausibly from
  cohort aggregates. Getting this seamless (no "pop-in" of personality) is the hard part.
  *(Improved — `cohort_crystallize` reconstructs deterministically: calling from the cohort's
  calling mix, money as the pool share, personality rolled from a dedicated stream, skill jittered
  per member (novices and veterans, not clones), and a larder of the staple food drawn from the
  regional market. Still approximate — it is plausible, not the member's true lost history; and
  **relationships are not reconstructed at all** (the cohort holds no social graph, and ties need
  authored bond/feud content to matter). That remains the open fidelity question.)*
- **Field expressiveness.** Gradient-following covers "go toward X." Goals that are not
  spatial-gradient-shaped (revenge against a specific moving foe, multi-step crafting) still
  need real planning -- those agents must be Tier 0/1, which bounds how cheap the mass can be.
- **Director reach across tiers.** The director must be able to reach into Tier 2 to stage
  drama (promote a cohort member into a face). The seam between statistical masses and the
  drama manager needs design (`docs/narrative_director_v2.md`).
- **Benchmark first.** *(Done — `examples/bench_scaling.rs`.)* It confirmed the central
  claim: per-agent A\* planning is ~98% of the tick and the rest is <2%, so the work is a
  complexity-class problem (Track 2), not a constant-factor one.
- **Deferred: fixed-point arithmetic everywhere (determinism hardening).** **All gameplay-affecting
  floating-point arithmetic must, in the future, be replaced by fixed-point.** Two review findings on
  PR #10 motivate this. (1) *Precision:* `total as f32` loses precision above 2^24 (~16.7M), biasing
  per-capita flows at scale — now fixed for the cohort economy with an integer `scale()`, but the same
  hazard lurks wherever a count or coin value meets a float. (2) *Associativity:* f32 addition is not
  associative, so accumulating `sustenance` in ECS/archetype order made the fingerprint fragile to
  layout changes (worked around by sorting on entity id, but the fragility is intrinsic to float
  sums). Floats are also not guaranteed bit-identical across platforms or compiler/LLVM versions,
  which directly threatens the determinism invariant in `CLAUDE.md`. The migration is large and
  cross-cutting — `Needs`/`Mood`/`Personality`/`Skills`, market `price_basis`, cohort `sustenance`,
  the IAUS/appraisal scoring, and ultimately the substrate's climate/ecology fields are all `f32`
  today — so it is staged future work, but the direction is settled: **gameplay state and everything
  the fingerprint folds should be exact integer / fixed-point**, with floats confined (if anywhere)
  to cosmetic-only quantities that never feed back into simulation state.

## Relationship to existing docs

- `CLAUDE.md` -- the non-negotiable invariants (determinism, off-by-default, RNG streams,
  integer economy) that constrain all of the above.
- `docs/narrative_director_v2.md` -- the drama manager whose prominence selection *is* the
  Tier-0 cast.
- `game_sim/docs/simulation_details.md` -- the substrate field solver that stigmergic
  fields extend.
