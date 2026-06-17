# achlydesa

A deterministic, agent-based **living-world simulation** written in Rust, with a playable
3D front-end. A hex world carries climate, geology, and ecology; a population of agents
lives on it -- working, trading, forming factions, remembering, feuding, and *talking* --
while a narrative director quietly shapes the drama. You can drop a player avatar into the
world and explore it, wait, and hold a conversation with the souls you meet.

> Status: a work in progress. The simulation, the agent layers, and a first playable
> front-end are built and tested. Combat and an on-device dialogue model are next.

## Workspace layout

A Cargo workspace of four crates, smallest to largest:

- `sim` -- engine-agnostic agent-based-modelling core: a deterministic RNG and the
  `Substrate` / `Actor` / `Action` / `Scheduler` / `Observer` traits the rest builds on.
- `game_sim` -- the simulation substrate: a cylindrical hex world carrying the geological,
  climate, and ecosystem fields (elevation, temperature, water, vegetation, carrying
  capacity, and so on). Bevy-free.
- `agents` -- the agent layer (built on `bevy_ecs`): utility AI (IAUS) plus GOAP planning,
  an integer economy with smoothed prices, factions and politics, personality (traits) and
  mood, emergent dialogue, a narrative "director", and the player avatar. Bevy-free.
- `app` -- the playable front-end (Bevy 0.18 + hexx): a true-3D hex-column view of the
  world (column height is real elevation) with fog of war, a follow camera, and a HUD. The
  simulation stays authoritative; the renderer is a thin view over it.

Design docs live in `docs/`.

## Building and running

Requires a recent Rust toolchain (edition 2024).

    cargo build
    cargo test

Run the game. The first build is slow because the Bevy engine is compiled fully optimized;
later builds are quick:

    cargo run -p app --release

Controls: click a tile to travel | Space to wait | T to speak to a nearby soul | A/D orbit
| W/S tilt | scroll to zoom. Time is turn-based: the world advances exactly one tick per
action you take, and stands still otherwise.

Headless demos (no window):

    cargo run -p agents --example dialogue_demo --release
    cargo run -p agents --example explore_demo --release

## Design principles

- **Deterministic.** Everything runs off seeded RNG; the same seed yields the same run,
  byte for byte.
- **Off by default.** Optional layers (dialogue, the director, the player) keep all their
  state in their own resources, so a world without them is byte-identical to before they
  existed.
- **Data-driven.** Goals, norms, conversational intents, narrative beats, and the
  generative grammar are authored as RON data, not hard-coded.
- **Integer economy.** Money and goods are integers; trade conserves money.

## Dialogue, and the on-device LLM seam

Dialogue is a new action modality for the same brain the agents already use. It splits in
two. *Meaning* is simulation: conversational intents are scored by the same utility AI that
ranks goals, from the speaker's traits, mood, opinion of the listener, and the grudges
between them -- deterministic, in-tick, whole-population. *Surface* is generated, never
drawn from a phrasebook; the always-available surface is a generative grammar.

Because the player is the avatar's mind, the player is not scored: the avatar carries no
traits or mood, and you choose your line from the full repertoire. The soul you address
answers from its own state.

There is also an optional, out-of-band seam for a small on-device language model: a
`TextGen` trait and an `SlmRealizer` (cache -> generate -> sanity-guard -> grammar
fallback). The model only re-voices an already-grounded line for the one conversation in
focus; it never feeds back into simulation state, so a build with no model loaded is
byte-identical to one with. **No model is bundled** -- a multi-gigabyte model plus FFI is a
host concern. To experiment with LLM-voiced dialogue, implement `TextGen` over a backend
such as `candle` or `llama.cpp` in the `app` crate and feed it `dialogue::build_prompt`.
