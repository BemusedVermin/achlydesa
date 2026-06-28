# The Player Avatar — exploration (built)

> **Status: exploration BUILT (2026-06).** First cut of *actual player interaction*. The
> player is an ordinary **body in the world**, not a god above it — faithful to the
> project's standing principle (the narrative-director thesis: *the player is an NPC avatar
> with the same verbs*). This cut is **exploration only**; talking to NPCs and fighting are
> deferred to later passes (the avatar is a real ECS entity, so those layers can see it like
> any other body when they come). Code: `agents/src/player.rs` + the `Simulation` API in
> `lib.rs`; demo `agents/examples/explore_demo.rs`; **playable 3D front-end in the `app`
> crate** (`cargo run -p app --release`).

> **Front-end conversion (2026-06-27):** the Bevy 3D `app` described below is being **retired** in
> favour of a text-first, Zork-like **terminal client** (new `tui` crate) over the same authoritative
> sim — pure verb-noun parser, ASCII hex map, diegetic journal, action-driven turns. The avatar model
> and the `Simulation` API in this doc are unchanged; only the *view* changes. See
> **`docs/text_interface.md`** (front-end) and **`docs/prose_generation.md`** (how the world is
> narrated, no LLM, never false). Treat the `app`-specific sections below as historical.

## Playing it — the `app` crate (Bevy front-end)

A first **graphical, playable** window onto the world, in the `app` crate (Bevy 0.18 +
hexx). The simulation stays bevy-free and authoritative; the Bevy shell is a thin view over
it (held as a `NonSend` resource, driven by hand each frame) — the same separation the
reference strategy-tactics game uses. Presentation, mirroring that reference:

- **True-3D hex columns** whose height is the land's *real elevation* (0–5000 m), each
  vertex-coloured by terrain band (ocean / coast / lowland / highland / mountain), greened
  by fertility, with darker side walls — one merged mesh, rebuilt only when the map grows.
- **Fog of war**: only the **explored** tiles are drawn; the rest is void. The map literally
  grows as you walk.
- The **avatar** is a bright capsule; the **populace** are pale markers, shown only where the
  fog has lifted; a follow-camera orbits the avatar.
- A **`bevy_ui` HUD**: the "look" panel (terrain underfoot, elevation, fertility, features,
  souls near, day, tiles revealed), a **"nearby voices"** panel streaming the emergent NPC
  dialogue happening around you (dialogue is on, so the world talks *as you move*), and a
  controls/status strip.

**Time is action-driven (turn-based):** the world advances **exactly one tick per player
action**, and stands frozen otherwise -- the player's action is the clock. There are three
actions so far:

- **Move** -- clicking a tile sets a route; the avatar walks it one hex per tick, so a
  journey costs one tick per hex (total ticks = total hexes walked). `drive_sim` ticks only
  while travelling, watchably paced.
- **Wait** -- tapping **Space** lets exactly one tick pass where the avatar stands: it holds
  its ground while the world lives a single moment around it. `wait_input` ->
  `Simulation::player_wait()`. Ignored mid-journey and when no avatar exists.
- **Talk** -- pressing **T** near a soul opens a conversation; the player chooses a line and
  the soul answers, each spoken exchange costing one tick. `talk_input` ->
  `Simulation::player_talk()`. See "Talking" below.

Stand still and take no action, and the populace stands still with you. The remaining action
(fight) will slot in the same way -- each one tick.

Run: `cargo run -p app --release`. **Controls:** click a tile to travel | **Space** wait |
**T** speak | **A/D** orbit | **W/S** tilt | **scroll** zoom.

## What it is

A controllable avatar that **walks the land, lifts the fog of war, and discovers what it
passes** — the same feature-discovery the NPCs get by visiting. It is a real entity
(`Position`, `Known`, a `Player` marker) but **not an `Npc`**, so every AI system (planning,
dialogue, the director) skips it for free via their `With<Npc>` filters — *the human is its
planner*. All player state lives in one `PlayerState` resource (avatar handle, route,
revealed map), so a world with no player is **byte-identical** and the layer is off by
default until `spawn_player` is called.

## The pieces

- **`Player`** — a marker component on the avatar entity.
- **`PlayerState`** (resource) — `{ avatar, path, destination, explored: HashSet<tile>,
  sight, speed }`. The `explored` set is the revealed map (fog of war).
- **Movement** — `player_travel_to(coord)` auto-routes over the **land** `MoveGraph` (BFS,
  deterministic in the graph's fixed neighbour order); `None` if unreachable, so a body
  **cannot route onto the sea**. The `player_travel` system then walks the route one
  (`speed`) hex per tick — *movement is time passing*; the world lives on around the
  avatar. `player_halt` stops it.
- **Discovery** — entering a tile inserts it into the avatar's `Known` and reveals its
  hidden features into the shared `Features` map (`discover_at_index`, Hidden tier) — the
  same mechanism `people::discover_features` gives NPCs.
- **Sight / reveal** — each step reveals the `sight`-ring of tiles around the avatar into
  `explored` (BFS rings over the topology).
- **Observation (`player_view`)** — the "look" verb: the tile underfoot, the tiles in sight
  (terrain banded by elevation — ocean/coast/lowland/highland/mountain — plus fertility,
  vegetation, water, and the features it can make out), and the bodies nearby. A feature is
  visible if it is a Landmark (seen by all) or the avatar has visited its tile.

## Simulation API

`spawn_player(at)` · `player_travel_to(coord) -> bool` · `player_halt()` ·
`player_wait() -> bool` (pass one tick in place; `false` if no avatar) ·
`player_traveling() -> bool` · `player_position() -> Option<Coord>` ·
`player_explored_count() -> usize` · `player_view() -> Option<PlayerView>`.

Front-end loop: `spawn_player(None)` → `player_travel_to(dest)` → `step()`/`run()` while
`player_traveling()`, reading `player_view()` to render. The demo walks an avatar from a
highland mining camp across the coast to a fishing village, the revealed map growing
28 → 170 tiles.

## Talking (built)

The player can speak to a soul within reach (press **T**). This is a role-playing game, so
the player -- not the avatar -- is the mind: the avatar carries **no** personality, mood, or
opinion, and the sim does **not** score what it "wants" to say. The player is offered the
whole repertoire of conversational verbs, unranked (`Simulation::player_intents`), and
*chooses* the meaning; the sim only renders the words and visits the consequence on the soul
addressed. That soul answers from *its own* state (`dialogue::reply`, the scored NPC path).
One spoken exchange is one action -- one tick (`Simulation::player_talk` says, hears the
reply, and advances the world a tick). Speech runs through the same machinery as emergent NPC
dialogue (`docs/dialogue.md`); the avatar simply needs no attributes to use it.

## Deferred (the later passes)

- **Fighting** -- a combat verb set, likewise shared with NPCs.
- **Metabolism / survival** — whether the avatar hungers, rests, and can die like an NPC
  (currently it is a pure observer; it does not metabolise).
- **Richer perception** — line-of-sight blocking by relief, a persistent remembered map
  (currently `explored` is "seen ever"), rumours of unseen places.
- **An interactive front-end** — this is a headless API proven by the demo + tests; a real
  renderer/input loop is future.

Tests: world-runs-with-no-player (off-by-default), explores-and-lifts-the-fog,
cannot-walk-on-water, deterministic.
