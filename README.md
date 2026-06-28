# achlydesa

A deterministic, agent-based **living-world simulation** written in Rust, played through a
text-first, Zork-like **terminal** front-end. A hex world carries climate, geology, and
ecology; a population of agents lives on it -- working, trading, forming factions,
remembering, feuding, and *talking* -- while a hidden narrative director quietly shapes the
drama. You drop into the world as a body within it: explore, take notes, and hold
conversations with the souls you meet, while the world advances around you one turn at a time.

> Status: a work in progress, mid **front-end conversion**. The simulation and agent layers
> are built and tested. The old 3D front-end is being replaced by a terminal client -- a
> living, MUD-like text adventure. See `docs/text_interface.md` and `docs/prose_generation.md`.

## Workspace layout

A Cargo workspace. The simulation is engine-free (plain Rust; the agent layers use `bevy_ecs`
-- the ECS data structures only -- never a renderer):

- `sim` -- engine-agnostic agent-based-modelling core: a deterministic RNG and the
  `Substrate` / `Actor` / `Action` / `Scheduler` / `Observer` traits the rest builds on.
- `config` -- the configuration hub: the RON format, the tunable knobs, and content sources.
- `game_sim` -- the simulation substrate: a cylindrical hex world carrying the geological,
  climate, and ecosystem fields (elevation, temperature, water, vegetation, carrying
  capacity, and so on).
- `agent_core` -- the heart: utility AI (IAUS) plus GOAP planning, an integer economy with
  smoothed prices, factions and politics, personality (traits) and mood, emergent dialogue, a
  narrative director, and the player avatar.
- `rpg` / `travel` / `party` / `survival` / `explore` -- the Worlds-Without-Number character
  model, the travel/routing model, recruited companions, per-tile vital drain, and the
  exploration layer.
- `agents` -- the thin assembler: wires the layers into a run and exposes the public API.
- `combat_core` / `combat_cli` -- the (in-progress) combat model and a headless harness.
- `tui` -- *(in progress)* the terminal text front-end that replaces the retired 3D `app`.

Design docs live in `docs/` -- start with `text_interface.md` and `prose_generation.md`.

## Building and running

Requires a recent Rust toolchain (edition 2024).

    cargo build
    cargo test

Headless demos of the simulation (no front-end):

    cargo run -p agents --example dialogue_demo --release
    cargo run -p agents --example explore_demo --release

The terminal client (`cargo run -p tui --release`) is under construction.

## Design principles

- **Deterministic.** Everything runs off seeded RNG; the same seed yields the same run, byte
  for byte.
- **Off by default, on in the game.** Optional layers keep all their state in their own
  resources, so a world without one is byte-identical to a world with it -- the tool that lets
  a layer be switched off cleanly for a focused test. The game itself turns every layer on.
- **Data-driven.** Goals, norms, conversational intents, narrative beats, and the generative
  grammar are authored as RON data, not hard-coded.
- **Integer economy.** Money and goods are integers; trade conserves money.

## Dialogue and procedural prose

Dialogue is an action modality for the same brain the agents already use, and it splits in
two. *Meaning* is simulation: conversational intents are scored by the same utility AI that
ranks goals, from the speaker's traits, mood, opinion of the listener, and the grudges
between them -- deterministic, in-tick, whole-population. *Surface* is generated, never drawn
from a phrasebook: a generative grammar assembles authored fragments from grounded facts.

The text conversion generalizes that split to all descriptive prose, under one hard rule: the
prose must **never hallucinate or state a false thing**. Every word is an authored constant or
a slot filled from a real simulation fact, so truth is structural -- which is why there is
**no LLM** anywhere in the surface (a neural or Markov generator asserts unsourced claims by
construction). See `docs/prose_generation.md` for the full engine: the guarded grammar,
salience selection, referring expressions, the oblique "Wolfean" implication layer, and the
truth-derived rumor/gossip distortion tier.
