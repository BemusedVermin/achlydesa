# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`achlydesa` — a deterministic, agent-based **living-world simulation** in Rust (edition 2024)
with a **text-first, Zork-like front-end** (a terminal TUI). A hex world carries
climate/geology/ecology; a population of agents lives on it (working, trading, forming factions,
feuding, talking) while a hidden narrative director shapes drama. The player explores and converses
as a body in the world, typing commands; the world advances MUD-like, one (or many) ticks per
action. *Front-end conversion in progress — see **`docs/text_interface.md`** (TUI + pure parser +
diegetic journal/map) and **`docs/prose_generation.md`** (the no-LLM, never-false procedural prose
engine). The old Bevy 3D `app` and the `voice` SLM crate are being retired.*

## Commands

```sh
cargo build                       # whole workspace
cargo test                        # all crates; most logic lives in agents' #[cfg(test)]
cargo test -p agents <name>       # one test (e.g. `cargo test -p agents both_farming_and_baking_emerge`)
cargo run -p tui --release        # the game (terminal text client; replaces the retired `app`)
```

Headless demos (no window, no engine), good for fast iteration on sim behaviour:

```sh
cargo run -p agents --example dialogue_demo --release   # also: explore_demo, director_demo,
                                                        # ecology_demo, features_demo, bench_caching
```

**Build profile note:** `[profile.dev]` builds *our* code at `opt-level = 1` and all dependencies at
`opt-level = 3` with no debuginfo (`Cargo.toml`) — a holdover from the heavy Bevy `app` (now being
retired), so the large engine compiled fully optimized once and cached. The `tui` client is light, so
this matters far less; the knob is harmless to keep.

There is no separate lint config; use `cargo clippy` / `cargo fmt` defaults. The toolchain must be
recent — the code uses edition-2024 let-chains (`if let … && let …`).

## Workspace architecture

Crates in dependency order (each may depend only on those above it). The defining boundary is
**engine-free**: the whole workspace is plain Rust (`agents`/`agent_core` use `bevy_ecs` *only* — the
ECS data structures, not a renderer/windowing). The new `tui` front-end is a thin terminal client; the
old `app` (full Bevy) and `voice` (SLM) crates are being retired (see `docs/text_interface.md`).

| Crate | Role | Engine |
|-------|------|--------|
| `sim` | Engine-agnostic ABM core: `Substrate`/`Actor`/`Action`/`Scheduler`/`Observer` traits + seeded `Rng`. Greek symbols in docs map to these traits (see `sim/src/lib.rs`). | none |
| `config` | Configuration hub. Owns the RON format and *sources bytes* — it does **not** parse them. Holds the tunable structs (`EconConfig`, `DirectorConfig`, …) and `Params`. | none |
| `game_sim` | The substrate: a cylindrical hex world (wraps E–W, polar caps) carrying climate/ecology fields. Owns `World`, `Coord`, `Topology`, `SplitMix64`. | none |
| `agent_core` | **The heart.** The agent layer on `bevy_ecs`: utility AI (IAUS) + GOAP planning, integer economy, factions/politics, traits & mood, emergent dialogue, the narrative director, the player avatar, fauna. *Every shared component/resource/type lives here* (extracted from the old `agents`). | `bevy_ecs` only |
| `rpg` | Worlds-Without-Number character model — six attributes, the 21 skills, two-level Foci, Cities-Without-Number Edges (data-driven *grant bundles*), deterministic (no-dice) checks, saves; a reserved xianxia power tier. RON-authored. | `bevy_ecs` |
| `travel` | **Pure.** Travel cost model (a forest hex ≈ a day; roads cheap, slope adds), weighted Dijkstra routing, a procedural road-tree builder, climb/boat edge gates. | none |
| `party` | Recruited companions that travel as a stack with the avatar — the roster, config, and the disposition→difficulty helper. | `bevy_ecs` |
| `survival` | Per-tile, per-day vital drain (thirst/warmth/stamina) on every body, mitigated by Constitution / the Survive skill / gear. | `bevy_ecs` |
| `explore` | The exploration data layer over `travel`: the road network, carried gear, the cost/gate config. | `bevy_ecs` |
| `agents` | **The thin assembler.** Owns `Simulation`/`Setup`/the fixed-order schedule, wires `agent_core` + the feature crates (`rpg`/`party`/`survival`/`explore`) into a run, exposes the public API, and re-exports the whole surface so `app` and the demos import everything from `agents`. | `bevy_ecs` only |
| `tui` | **The front-end.** A terminal text client (ratatui + crossterm): a scrollback prose pane + ASCII hex map + diegetic journal + status panels, a pure verb-noun parser, action-driven turns. A thin **view** over the authoritative sim. | terminal |
| `app` | *(retiring)* The old Bevy 0.18 3D front-end — true-3D hex columns, fog of war, follow camera, HUD. Being replaced by `tui`. | full Bevy |
| `voice` | *(retiring)* Optional on-device SLM that re-voiced dialogue surface text. Incompatible with the new "never hallucinate, no LLM" prose mandate — removed. | none (FFI) |

`agents::Simulation` is the top-level driver: it wraps its own `bevy_ecs::World` + a fixed-order
`Schedule`. **The caller owns world generation** — the front-end builds its `game_sim::World`
(US-scale, few plates) and injects it via `Simulation::from_world(world, setup)`; `Simulation::new(setup)` is a
headless/test convenience that generates a small default world from `Setup::params`. `Setup` is *the*
knob surface — seed, warm-up, populations, and which optional layers wake (`dialogue`, `director`,
`rpg`, `party`, `survival`, `exploration`, `perception`, `combat`, `sift`, `fields`, `cohorts`, …).
**The game turns them all on**; each can be switched off only for a lean headless/test run, where it
keeps to its own state and doesn't perturb the rest (`Setup::default()` stays minimal so a test opts
into exactly what it exercises). The front-end owns the `Simulation` and drives it by hand
(`sim.step()`), one or many ticks per player action (action-driven turns) — never a background clock.

The RPG / survival / exploration layers (the `rpg`/`party`/`survival`/`travel`/`explore` crates) are
a large, ongoing build — design, rationale, and per-phase status live in
**`docs/rpg_survival_exploration.md`**; read it before extending them.

## Non-negotiable invariants

These are enforced patterns, visible across `agents/src/lib.rs` — preserve them when extending:

- **Deterministic.** Everything runs off a seeded `SplitMix64`; the same seed yields a byte-identical
  run. The schedule runs single-threaded in a fixed order (`ExecutorKind::SingleThreaded`). Parallel
  planning (`people_plan`) is allowed only because it writes each agent's own `Plan` from read-only
  shared state, so the result is split-independent.
- **Layers default ON, and disable *cleanly*.** The full experience is the default — the game
  (the front-end's `Setup`) turns on every layer it has. Do **not** add a feature off-by-default and make the
  user hunt for a flag; if something is asked for, it ships *on*. Each layer still keeps its state in
  its *own* resource/component and early-returns when disabled, so it *can* be switched off — for a
  focused test or a lean headless run — without perturbing the others. That isolation (and the
  byte-identity it happens to give a disabled layer) is a tool for turning things off *cleanly*, never
  a reason to keep them off. The `Setup::default()` used by tests stays minimal so a test opts into
  exactly the layers it exercises; that is the "able to be turned off" case, not the player's.
- **Separate, derived RNG streams.** Each subsystem that needs randomness seeds a *dedicated* stream
  by xor-ing the run seed with a distinct constant (e.g. predation, feature placement, the director,
  dialogue). This is what lets a layer be added without perturbing any other layer's stream. Never
  pull a new subsystem's randomness from an existing stream.
- **Integer economy.** Money and goods are integers; trade conserves money exactly (deaths are the
  only sink). `total_money()` is an exact invariant, used in V&V tests.

## Data-driven content

Two distinct kinds of authored content, both under the top-level `assets/`, both reached only
through the `config` crate (no crate touches `assets/` directly):

- `assets/data/*.ron` — **content lists**: goods, recipes, skills, traits, moods, goals, norms,
  narrative beats, dialogue intents, the generative grammar, the bestiary, and the WWN RPG content
  (`attributes`, `rpg_skills`, `foci`, `edges`). Loaded via `config` but **parsed by the owning
  crate** (`agent_core` for the world model, `rpg` for the RPG content). `Registry::bundled()` /
  `RpgData::bundled()` / `IntentBook::bundled()` etc. bake these in at compile time.
- `assets/config/*.ron` — **tunable knobs** (`EconConfig`, `NeedsConfig`, `DirectorConfig`,
  …). Each loads as `Default`, then any RON file found is layered on top via
  `figment` (`config/src/tunables.rs`). A missing file just means defaults; retune without recompiling.

The dependency arrow is always `agents → config` (and `→ game_sim → config`). `config` stays tiny
and parse-free so it can never drag the ECS or game types backward into itself.

## The dialogue / prose seam

Dialogue splits in two (`docs/dialogue.md`): **meaning** is simulation (conversational intents scored
by the same IAUS that ranks goals — deterministic, in-tick) and **surface** is generated. The surface
is a deterministic generative grammar that assembles authored fragments from grounded facts — never a
phrasebook, never an LLM. The text conversion generalizes this same split from one-line utterances to
*all* descriptive prose (scenes, NPCs, world events, oblique "Wolfean" implication), under a hard
rule: the prose must **never hallucinate or state a false thing** — every word is an authored constant
or a slot filled from a real sim fact (see **`docs/prose_generation.md`** for the full NLG pipeline,
the guarded/tagged grammar, salience selection, referring expressions, and the Wolfean tell layer).
The old optional `voice` SLM seam (`TextGen`/`SlmRealizer`, re-voicing lines with a small model) is
**retired**: an LLM rephrasing can distort or invent, which the new mandate forbids. The grammar was
always the floor; it is now the whole surface, and all selection runs on a dedicated derived RNG
stream so the layer stays deterministic and byte-identical when off.

## Where to read more

Design docs live in `docs/`. For the text conversion, start with **`text_interface.md`** (the TUI
front-end, pure parser, diegetic journal/map, world-time) and **`prose_generation.md`** (the no-LLM,
never-false procedural prose engine). Then `dialogue.md`, `narrative_director.md`, `player.md`,
`deferred.md`, plus per-crate docs: `agents/docs/odd.md` (the agent model as an ODD spec),
`game_sim/docs/simulation_details.md`, `sim/docs/design.md`. The crate-level `//!` doc comments
(especially `agents/src/lib.rs` and `config/src/lib.rs`) are detailed and current — read them first.
