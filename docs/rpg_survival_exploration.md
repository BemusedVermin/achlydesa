# RPG (Worlds Without Number) + Survival + Exploration Overhaul — Design & Plan

> Working design doc. Mirrors the approved plan in `.claude/plans/`. The crate
> decomposition (Phase 0) is the foundational refactor; feature work (Phases 1–7) layers on top.

## Context

`achlydesa` today is a living-world ABM with autonomous NPCs (utility AI + GOAP, integer
economy, factions, emergent dialogue, a hidden director) and a playable avatar that walks, talks,
searches and discovers. It has **no RPG character layer**, a thin Sustenance/Rest survival model,
and **free, costless movement** — no roads, rivers, party, or interactable POIs.

This adds three interlocking layers (speech + world-interaction skills first; combat deferred),
**modularised into crates**, and **extensible via asset files**:

1. **RPG** — Worlds Without Number attributes, skills, feats (Foci), saving throws + *Edges*
   grafted from Cities Without Number — on **both NPCs and the avatar**.
2. **Survival** — per-tile, per-tick consumption (thirst, warmth, stamina, tile-dependent hunger)
   that *feeds* the RPG (Survive + CON + gear mitigate it).
3. **Exploration** — interactable POIs, terrain/elevation travel cost, roads, rivers,
   equipment-gated edges (climbing gear + a proficient share of the party), carts & paid passage,
   and a **US-scale world** (months to cross; ~1 hex = a day's walk).

The advancement model stays **job-system-friendly** (Edges and future combat "jobs" are generic
data-driven *grant bundles*), with a reserved, unfilled **power-tier** (xianxia cultivation) field.

### Locked decisions
- WWN **Attributes + Skills** always in. Also now: **Foci** (non-combat active, combat-tagged
  inert), **Edges** (CWN graft as grant-bundles), **Saving throws**. **No fixed WWN classes.**
- RPG skills are a **parallel additive** layer; economy `Skills` (farming/baking) untouched (unify
  to WWN Craft/Work later — deferred).
- **Deterministic threshold** checks (no dice): `attr_mod + skill + situational ≥ difficulty`,
  graded by margin.
- **Party** by **live recruitment** (Recruit speech act + a deterministic Convince/Lead check vs
  disposition); members keep their stats and move as a stack with the avatar.
- Survival adds **Thirst, Warmth, Stamina/Encumbrance** + **tile-dependent Sustenance**; applies to
  **everyone** (party + all NPCs).
- World becomes **US-scale**, **few tectonic plates** (huge continents).
- **Modular: new crates where they earn their boundary. Prefer existing external crates.**

## Crate architecture (full ECS modularization)

The dependency reality (`people` is the hub; `factions`/`dialogue`/`director`/`events`/`observe`/
`player` all depend on it; no hard cycles) forces a foundational `agent_core` crate, with the new
features layered on top and a thin `agents` assembler.

```
sim → config → game_sim ─┬─→ agent_core ─┬─→ rpg ─┬─→ survival ┐
              └→ rpg     │               │        ├─→ party    ┼─→ agents → app
            (rpg dep: config, sim, bevy_ecs)      └→ travel ───┴─→ explore ┘
                                                  (agents assembles the schedule)
```

| Crate | Engine | Responsibility |
|-------|--------|----------------|
| **agent_core** *(new)* | bevy_ecs | Today's `agents` library minus `Simulation`/`Setup` — every shared **component/resource/type** (`Npc`, `Position`, `Needs`, `Skills`, `Inventory`, `Personality`, `Mood`, `Opinion`, `Allegiance`, `Detained`, `Known`, `Market`, `Throne`, `Substrate`, `MoveGraph`, `Registry`, features/affordances, `PlayerState`…) and the existing systems. Defines the **integration seams** (below). |
| **rpg** *(new)* | bevy_ecs | WWN/CWN data model: `Abilities`/`Proficiencies`/`FociHeld`/`Flags`/`PowerTier`/`Archetype` components, `RpgData` registry + RON parsing (`attributes`/`rpg_skills`/`foci`/`edges`), `wwn_mod`, the deterministic **`check`** engine, grant bundles + `roll`, saves. **Self-contained — Dep: `config`, `sim`, `bevy_ecs` only** (no `agent_core`; the assembler attaches its components). Tunables (`RpgConfig`) deferred — WWN constants for now. |
| **travel** *(new)* | none (pure) | Engine-agnostic cost model: `tile_cost(biome, slope, road)`, weighted **Dijkstra/A\*** over the hex graph (uses the existing **`pathfinding`** crate), least-cost road builder, edge-gate predicates. Dep: `game_sim`, `pathfinding`. |
| **survival** *(new)* | bevy_ecs | `Vitals` + `survival_metabolism`, `SurvivalConfig`. Dep: `agent_core`, `rpg`, `game_sim`. |
| **party** *(new)* | bevy_ecs | `PartyMember`, `Party`, pure `recruit_check`, `party_share_with_flag`, the follow system. Dep: `agent_core`, `rpg`. |
| **explore** *(new)* | bevy_ecs | `Roads`, `Gear` + `gear.ron`, POI-interact effects, weighted-route wiring, edge gates, carts/paid-passage, river derivation. Dep: `agent_core`, `rpg`, `party`, `travel`, `game_sim`. |
| **agents** *(thinned)* | bevy_ecs | The **assembler**: `Simulation`, `Setup` (+ feature flags, `params`), the schedule (core + feature systems, fixed order), the public API, post-spawn attachment of feature components, the avatar-speech check, and re-exports of `agent_core` + feature crates so `app` barely changes. |

**External crates:** `pathfinding` (already a dep — weighted routing + roads), `serde`/`ron` via
`config` (assets), `bevy_ecs` (ECS). No new heavyweight deps needed; `SplitMix64` (from `sim`) stays
the RNG for determinism.

**World-construction seam (revised after review):** `agents` does **not** author a world for the
app. `app` calls `game_sim::World::generate(...)` itself and injects it via
`Simulation::from_world(world, setup)`; `Simulation::new(setup)` is only a headless/test convenience
that generates a default world then delegates. Worldgen lives in `game_sim` and is *invoked* by
whoever owns the world (the app), not bundled into the agent driver.

**Progress:** Phase 0 (agent_core extraction) ✅ byte-identical · Phase 1 (US-scale world + the seam
above) ✅ · Phase 2 (rpg crate + NPC/avatar wiring, `Setup.rpg`) ✅. Phases 3–7 pending.

## Cross-cutting invariant rulings

- **R0 — Off-by-default = byte-identical.** Each feature layer has a `Setup` bool (default false,
  OR'd with its config `enabled`), its own component/resource, and an early-return when off. After
  every step the whole-workspace `cargo test` must stay green.
- **R1 — One new RNG stream.** RPG **stat-gen** draws from `rpg_seed = seed ^ <new u64>` in a pass
  **after** `spawn_npcs` (never interleaved). Checks, survival drain, road/river derivation, travel
  cost are deterministic from data — no RNG.
- **R2 — Integer economy untouched.** Carts/passage are coin **transfers**; `total_money()` exact.
- **R3 — A tick is one day.** The day-cost model is an **avatar-only** fractional accumulator in
  `player_travel`; NPC `Step::Move`/`MoveGraph` costs are never re-weighted.
- **R4 — Avatar is not an `Npc`.** Survival queries `Or<(With<Npc>, With<Player>)>` behind its flag.

### Integration seams (in `agent_core`, byte-identical no-ops by default; populated from above)
- **S1 — Planning suspension.** Generalize the existing `Detained` skip: `agent_core` adds a
  `Suspended` marker that `people_plan`/`people_execute` skip (exactly like `Detained`,
  `people.rs:470/616`). `party` inserts `Suspended` on followers; core never learns about
  `PartyMember`. Absent component ⇒ byte-identical.
- **S2 — Hunger model.** `people_metabolism` reads a core flag (`HungerModel::TileBiomass` vs flat);
  the assembler flips it on when survival is enabled. New axes (`Vitals`) are a separate
  `survival`-crate system; core's metabolism only gains a gated branch.
- **S3 — Speech scaling.** The **avatar's** speech check is computed in the `agents` assembler
  (which has both `dialogue` and `rpg`) before applying moves — no core hook. NPC↔NPC emergent
  persuasion scaling is **deferred** (optional core hook later) to avoid a boxed-closure seam now.

## Phase 0 — Crate decomposition (foundational; must land byte-identical)

- Create `agent_core`: **move whole module files** (`ai, data, plan, goals, norms, features, people,
  factions, events, dialogue, player, fauna, beats, director, observe`) + shared types (`Position`,
  `Substrate`, `SimRng`, `advance_substrate`) out of `agents` into `agent_core`. Internal `crate::`
  paths are preserved by moving whole files. Make the ~19 schedule system fns `pub`. Add the `S1`
  `Suspended` marker and the `S2` hunger flag (both inert by default).
- Thin `agents`: keep `Simulation`/`Setup`/schedule/public API; `pub use agent_core::*` (+ feature
  crates) so `app` imports barely change.
- Scaffold empty feature crates (`rpg`, `travel`, `survival`, `party`, `explore`) + workspace
  members + manifests.
- **Verify:** whole-workspace `cargo test` byte-identical green; `app` builds (both feature paths).
  This phase changes **no behaviour** — pure relocation + visibility + inert seams.

## Phase 1 — US-scale world (app config; crate defaults unchanged)
- Add `params: Params` to `Setup` (default `tunables::params()`); `app::build_world` builds the
  large `Params` (few `plates`, raised `uplift_falloff`/erosion) + large `width/height`. **Do not**
  edit checked-in `assets/config/params.ron` (the `tunables.rs:508` round-trip test). Crate defaults
  stay small (fast, byte-identical tests).
- Reuse `worldgen::generate`; rivers already derived from flow-accumulation. Add a `worldscale_demo`.

## Phase 2 — RPG core (`rpg` crate)
- Assets via new `config::Asset` variants: `attributes.ron`, `rpg_skills.ron` (21 WWN skills, separate
  from economy `skills.ron`), `foci.ron`, `edges.ron`, optional `saves.ron`.
- Grant bundle: `SkillRank | AttrBonus | GrantFocus | Flag | PowerTier` (`PowerTier` reserved).
- Components on NPCs **and** avatar: `Abilities{[i32;6]}`, `Proficiencies{Vec<i8>}` (−1..4; **not**
  named `Skills`), `FociHeld{Vec<u8>}`, `Flags(HashSet<String>)`, `PowerTier(u8)`. `RpgData`
  resource + `wwn_mod` + pure `check(...) -> CheckOutcome{Fail|Pass|Strong + margin}` + `roll_stats`
  (the only RNG, `rpg_seed`). `RpgConfig` + `rpg.ron`.
- `agents` wiring: `Setup{rpg, rpg_cfg}`; a post-`spawn_npcs` gated pass attaches rolled components;
  avatar stats in `spawn_player`. No new per-tick system → schedule unchanged when off.

## Phase 3 — Party (`party` crate)
- `PartyMember{since}`, `Party{Vec<Entity>}`, `PartyConfig{enabled, max_size, recruit_difficulty,
  disposition_weight}`. Pure `recruit_check(opinion, mood, abilities, prof) -> bool`,
  `party_share_with_flag`.
- `agents`: `player_recruit(listener)` orchestrates Opinion+mood → check → insert `PartyMember` +
  `Suspended` (S1) + push to `Party`. Follow system sets members' `Position` to the avatar's in the
  `player_travel` slot.

## Phase 4 — Speech + world-interaction skills (`agents` wiring + `agent_core` seam-light)
- Avatar speech: in `player_talk`/`apply_conversational_intent`, compute `rpg::check(Convince/Lead vs
  listener Mental save/disposition)` and scale the move deltas (discrete grades 0/0.5/1.0/1.5) before
  `dialogue::apply_moves`. Add optional `check: Option<CheckSpec>` to `Intent` (`#[serde(default)]`
  → existing `intents.ron` unaffected). NPC↔NPC scaling deferred (S3).
- Notice → reveal `Secret` features on a passed check (param default false); Survive → cut survival
  drain + surface water/forage; Heal → restore a vital; Exert/Climb, Ride/Sail → travel gates (P6).

## Phase 5 — Survival (`survival` crate; everyone, gated)
- `Vitals{thirst, warmth, stamina}` + `survival_metabolism` (slot beside `people_metabolism`):
  drains from `world.{temperature, surface_water, plant_biomass, biome}`; Survive rank + CON mod +
  gear/shelter offset; despawn at floor (reuse the core despawn/throne-vacate). Query
  `Or<(With<Npc>,With<Player>)>`. `SurvivalConfig` + `survival.ron`.
- Tile-dependent Sustenance via the **S2** hunger flag in core (flat when off → economy baselines
  byte-identical).

## Phase 6 — Exploration (`travel` pure crate + `explore` crate)
- **travel (pure):** `tile_cost`, weighted Dijkstra/A* (`pathfinding`), least-cost road builder, edge
  predicates — index-tie-broken for determinism.
- **explore:** `Roads(HashSet<usize>)` built at world-gen between settlements; `path_to_weighted` for
  the avatar route (**NPC graph untouched**, R3); the `player_travel` day-budget accumulator +
  `PlayerState.travel_residual`; `Gear(HashSet<String>)` + `gear.ron`; POI `player_interact` reusing
  the `Step::Use` machinery + new `EffectDef` variants; steep/river edge gates (climbing share +
  gear / boat); carts & `player_hire_transport` (integer transfer, R2). Rivers from existing
  `surface_water`.

## Phase 7 — App render + V&V
- Render roads/rivers (tint in `world_mesh.rs` via new accessors); **mesh chunking** for US scale
  (fog already draws only explored tiles). Capstone V&V: all layers on + large world → same-seed
  byte-identical run + `total_money()` conserved. Docs: `player.md`, `CLAUDE.md` (crate layout, the
  one new RNG const, the day accumulator), `deferred.md`.

## Verification
- After **every phase**: `cargo build` (default + `--no-default-features`) and whole-workspace
  `cargo test` green (off-by-default proof). Per-phase unit tests live in their crate.
- Per-crate demos: `worldscale_demo`, `rpg_demo`, `party_demo`, `survival_demo`, extended
  `explore_demo`/`dialogue_demo`. Manual `cargo run -p app --release` on the large world.

## Deferred
Combat (job system + xianxia power tiers — `PowerTier`/grant-bundle hooks reserved), combat-Foci
activation, NPC↔NPC speech scaling (S3 hook), full ocean traversal (ship/airship/flight),
economy↔WWN-Craft unification, JRPG dialog + procedural portraits, a real (non-debug) UI, deeper
economic sim. Pre-existing: rebaseline the brittle narrative V&V tests + sync `params.rs` defaults
to the calibrated `params.ron`.
