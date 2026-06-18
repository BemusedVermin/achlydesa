# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`achlydesa` — a deterministic, agent-based **living-world simulation** in Rust (edition 2024)
with a playable Bevy 3D front-end. A hex world carries climate/geology/ecology; a population of
agents lives on it (working, trading, forming factions, feuding, talking) while a hidden
narrative director shapes drama. A player avatar can be dropped in to explore and converse.

## Commands

```sh
cargo build                       # whole workspace
cargo test                        # all crates; most logic lives in agents' #[cfg(test)]
cargo test -p agents <name>       # one test (e.g. `cargo test -p agents both_farming_and_baking_emerge`)
cargo run -p app --release        # the game (first build is slow — see profile note)
cargo run -p app --no-default-features --release   # compile candle/the SLM out entirely; grammar-only dialogue
```

Headless demos (no window, no Bevy engine), good for fast iteration on sim behaviour:

```sh
cargo run -p agents --example dialogue_demo --release   # also: explore_demo, director_demo,
                                                        # ecology_demo, features_demo, bench_caching
```

**Why the first `app` build is slow:** `[profile.dev]` builds *our* code at `opt-level = 1` but
all dependencies (the large Bevy stack) at `opt-level = 3` with no debuginfo (`Cargo.toml`). We
never step into the engine, so it's compiled fully optimized once and cached.

There is no separate lint config; use `cargo clippy` / `cargo fmt` defaults. The toolchain must be
recent — the code uses edition-2024 let-chains (`if let … && let …`).

## Workspace architecture

Six crates, in dependency order (each may depend only on those above it). The defining boundary is
**Bevy-free**: only `app` links the Bevy *engine*; everything else is plain Rust, and `agents` uses
`bevy_ecs` *only* (the ECS data structures, not the renderer/windowing).

| Crate | Role | Engine |
|-------|------|--------|
| `sim` | Engine-agnostic ABM core: `Substrate`/`Actor`/`Action`/`Scheduler`/`Observer` traits + seeded `Rng`. Greek symbols in docs map to these traits (see `sim/src/lib.rs`). | none |
| `config` | Configuration hub. Owns the RON format and *sources bytes* — it does **not** parse them. Holds the tunable structs (`EconConfig`, `DirectorConfig`, …) and `Params`. | none |
| `game_sim` | The substrate: a cylindrical hex world (wraps E–W, polar caps) carrying climate/ecology fields. Owns `World`, `Coord`, `Topology`, `SplitMix64`. | none |
| `agents` | **The heart.** The agent layer on `bevy_ecs`: utility AI (IAUS) + GOAP planning, integer economy, factions/politics, traits & mood, emergent dialogue, the narrative director, the player avatar. | `bevy_ecs` only |
| `voice` | Optional on-device SLM (candle + Qwen2.5) that re-voices dialogue surface text. Isolated here so the heavy `candle` stack never touches the sim crates. | none (FFI) |
| `app` | The playable front-end (Bevy 0.18 + hexx): true-3D hex columns, fog of war, follow camera, HUD. A thin **view** over the authoritative sim. | full Bevy |

`agents::Simulation` is the top-level driver: it wraps its own `bevy_ecs::World` + a fixed-order
`Schedule` and is built from a `Setup` struct (`agents/src/lib.rs`). `Setup` is *the* knob surface
for a run — world size, seed, warm-up, populations, and which optional layers wake. `app` holds the
`Simulation` as a Bevy `NonSend` resource and drives it by hand (`sim.step()`), one tick per player
action; it never lets the outer Bevy schedule run the sim.

## Non-negotiable invariants

These are enforced patterns, visible across `agents/src/lib.rs` — preserve them when extending:

- **Deterministic.** Everything runs off a seeded `SplitMix64`; the same seed yields a byte-identical
  run. The schedule runs single-threaded in a fixed order (`ExecutorKind::SingleThreaded`). Parallel
  planning (`people_plan`) is allowed only because it writes each agent's own `Plan` from read-only
  shared state, so the result is split-independent.
- **Off by default = byte-identical.** Every optional layer (dialogue, director, player avatar)
  keeps its state in its *own* resource/component and early-returns when disabled, so a world without
  it is bit-for-bit identical to one before the layer existed. New optional layers must do the same.
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
  narrative beats, dialogue intents, the generative grammar. Loaded via `config` but **parsed by
  `agents`** (the crate that owns the types). `Registry::bundled()` / `IntentBook::bundled()` etc.
  bake these in at compile time.
- `assets/config/*.ron` — **tunable knobs** (`EconConfig`, `NeedsConfig`, `DirectorConfig`,
  `VoiceConfig`, …). Each loads as `Default`, then any RON file found is layered on top via
  `figment` (`config/src/tunables.rs`). A missing file just means defaults; retune without recompiling.

The dependency arrow is always `agents → config` (and `→ game_sim → config`). `config` stays tiny
and parse-free so it can never drag the ECS or game types backward into itself.

## The dialogue / SLM seam

Dialogue splits in two (`docs/dialogue.md`): **meaning** is simulation (conversational intents scored
by the same IAUS that ranks goals — deterministic, in-tick) and **surface** is generated. The
always-available surface is a deterministic generative grammar. The optional `voice` crate implements
the `agents::TextGen` seam to *re-voice* an already-grounded line with a small LLM — it runs on a
background thread, is cached by meaning hash, and **never feeds back into simulation state**, so a
build with the model is byte-identical to one without. **No model is bundled**; `voice` downloads
the GGUF from HuggingFace per `assets/config/voice.ron`. In `app`, all candle-touching code is behind
`#[cfg(feature = "voice")]` and reached through the `voice_bridge` module only.

## Where to read more

Design docs live in `docs/` (`dialogue.md`, `narrative_director.md`, `player.md`, `deferred.md`),
plus per-crate docs: `agents/docs/odd.md` (the agent model as an ODD spec), `game_sim/docs/
simulation_details.md`, `sim/docs/design.md`. The crate-level `//!` doc comments (especially
`agents/src/lib.rs` and `config/src/lib.rs`) are detailed and current — read them first.
