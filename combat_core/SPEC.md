# `combat-core` — Implementation Specification

A headless, deterministic, engine-agnostic Rust library implementing a **tick-based
contested-timeline combat simulation**. This document is the implementation brief.
Build the crate exactly to this spec. Where a decision is marked **TUNABLE**, expose it
through `config.rs` rather than hard-coding it.

---

## 1. Goal & design fantasy

Combat is a **shared global tick timeline** that actors place actions onto and that the
player (and elite enemies) can *perceive and edit*. The player's core verb is bending the
flow of the next few seconds — slowing an incoming threat, interrupting a wind-up, opening
a window for an ally to exploit. The moment-to-moment cadence is **read → edit → resolve a
burst → read again**.

This crate implements **only the simulation**. It computes the resolved timeline and emits
a typed, ordered **event stream**. All visuals, camera, time-remapping, and VFX are a
separate downstream consumer of that stream and are explicitly out of scope here.

## 2. Hard constraints

1. **Language:** Rust, library crate (`combat-core`), plus a small binary crate
   (`combat-cli`) as a headless driver.
2. **No Bevy dependency. No rendering. No I/O inside the core** (the CLI does I/O).
   The core is a pure state machine driven by an external loop.
3. **Fully deterministic and replayable.** Given the same config, the same initial
   scenario, and the same sequence of controller commands, the emitted event stream must be
   **bit-identical** across runs and machines. This is the single most important property.
   See §4.
4. **No floating point anywhere in the core.** Time is integer ticks (`u64`). Continuous
   magnitudes use a 16.16 fixed-point type (`Fixed`, `i32` backing). Determinism over
   prettiness.
5. **No wall-clock, no threads, no `HashMap` iteration in ordering-sensitive paths.** Use
   ordered collections (`BTreeMap`/sorted `Vec`) wherever iteration order can affect
   results. If a `HashMap` is used for storage, never iterate it to drive resolution —
   always resolve through an explicitly sorted view.
6. Dependencies kept minimal: `serde` (+ `serde_json` in the CLI/tests) for the event/trace
   contract, and a small **seedable, deterministic** RNG (`rand_pcg` or hand-rolled
   xorshift) — never `rand::thread_rng`.

## 3. Architecture & crate layout

Functional-core / imperative-shell. The core data structs are designed to map cleanly onto
an ECS later (each becomes a component; each pure update fn becomes a system), but the core
does **not** depend on any ECS.

```
combat-core/                # lib, no bevy
  src/
    lib.rs
    tick.rs                 # Tick(u64), Fixed (16.16)
    ids.rs                  # ActorId, FactionId, InstanceId, MoveId, WindowId (newtype u32)
    config.rs               # all TUNABLE knobs (see §14)
    actor.rs                # Actor, ActorState, Vitals
    moves.rs                # MoveDef, FrameData, Effect, MoveLibrary
    timeline.rs             # ActionInstance, Timeline (the tick line + scheduler advance)
    windows.rs              # Window (timed tag) + WindowStore
    tempo.rs                # Tempo economy (accrual + spend)
    verbs.rs                # EditVerb as pure timeline transforms
    resolve.rs              # contact resolution, effect application, strict ordering
    foresight.rs            # ForesightView projection (read-only, fogged)
    events.rs               # Event enum + serialization (the contract)
    controller.rs           # Controller trait; ScriptedController; StubAi
    sim.rs                  # Sim: top-level state + step() API
  tests/
    golden.rs               # runs scenarios, diffs against committed *.golden.json
  scenarios/                # input scenarios (JSON)
  golden/                   # expected event-stream traces (JSON), committed to VCS

combat-cli/                 # bin: load scenario -> run -> print/serialize trace
PORTING.md                  # determinism rules + how to validate a reimplementation
SPEC.md                     # this file, copied in
```

## 4. Determinism rules (the crux — implement these literally)

All non-determinism risk lives in *ordering of simultaneous events*. Define and enforce a
**strict total order** everywhere two things could happen "at once":

- **Time** advances in integer ticks. The engine may *jump* to the next interesting tick
  (the minimum over all upcoming phase boundaries, window expiries, and actor ready-times)
  rather than incrementing by 1, but the result must be identical to ticking by 1.
- **Simultaneous activations** (multiple actions entering their active frame on the same
  tick) resolve sorted by, in order:
  1. `priority_class` (higher first),
  2. `faction_order` (Player faction before Enemy faction; configurable enum order),
  3. `actor_id` ascending,
  4. `instance_id` ascending.
- **Decisions are serialized.** At a given tick, only one controller decision is resolved
  at a time. The engine selects the next pending decision by `(faction_order, actor_id)`
  ascending, requests it, applies the returned command, then re-evaluates. This turns all
  concurrent choices into a deterministic sequence.
- **RNG** (used only for any randomized effect, e.g. variance — keep minimal in v1) is a
  single seeded stream stored in `Sim`. Draw order is fixed by the resolution order above.
  The seed is part of the scenario.
- Never let `HashMap` iteration, pointer addresses, or insertion timing influence results.

## 5. Data model

```rust
// tick.rs
pub struct Tick(pub u64);
pub struct Fixed(pub i32); // 16.16; impl add/sub/mul/from_int/to_int. v1 use is light.

// ids.rs — all newtype(u32), Copy, Ord, serde
pub struct ActorId(u32);
pub struct FactionId(u32);   // 0 = Player, 1 = Enemy (extensible)
pub struct InstanceId(u32);  // monotonic, assigned by Sim
pub struct MoveId(u32);
pub struct WindowId(u32);

// actor.rs
pub struct Vitals { pub hp: i32, pub max_hp: i32 }

pub enum ActorState {
    Idle,                          // ready to act; a decision point
    Committed(InstanceId),         // currently executing a scheduled action
    Staggered { until: Tick },     // cannot act/edit until `until`
    Down,                          // defeated
}

pub struct Actor {
    pub id: ActorId,
    pub faction: FactionId,
    pub vitals: Vitals,
    pub tempo: i32,                // Tempo pool; 0 for mooks. See §9.
    pub next_ready_tick: Tick,     // when this actor may next take an Idle decision
    pub state: ActorState,
    pub foresight_horizon: u32,    // ticks this actor can see ahead (a stat)
    pub zone: ZoneId,              // minimal spatial model, see §11/§18
}

// moves.rs
pub struct FrameData { pub startup: u32, pub active: u32, pub recovery: u32 } // ticks

pub enum Effect {
    Damage { amount: i32 },
    LineKnockback { ticks: u32 },          // shoves target along the timeline (§7)
    OpenWindow { tag: WindowTag, duration: u32, magnitude: Fixed },
    Stagger { ticks: u32 },
}

pub struct MoveDef {
    pub id: MoveId,
    pub name: String,
    pub frames: FrameData,
    pub priority_class: u8,                // contact ordering (higher wins)
    pub effects: Vec<Effect>,              // applied on the active frame, in order
    pub requires_tag: Option<WindowTag>,   // e.g. payoff move requires target Exposed
    pub has_armor: bool,                   // if true, cannot be interrupted in startup
    pub range: ZoneReq,                    // minimal: SameZone | AnyZone (v1)
    pub tempo_cost: i32,                   // 0 for a normal reactive action
}

pub struct MoveLibrary { /* MoveId -> MoveDef, BTreeMap */ }

// timeline.rs
pub enum InstanceStatus { Scheduled, Resolving, Resolved, Cancelled }

pub struct ActionInstance {
    pub id: InstanceId,
    pub actor: ActorId,
    pub mv: MoveId,
    pub target: Option<ActorId>,
    pub start_tick: Tick,          // tick the startup phase begins
    pub status: InstanceStatus,
    // derived (recompute whenever start_tick changes):
    //   startup_end  = start_tick + startup
    //   active_start = startup_end
    //   active_end   = active_start + active
    //   recovery_end = active_end + recovery
}

pub struct Timeline { /* current_tick, instances: BTreeMap<InstanceId, ActionInstance> */ }

// windows.rs
pub enum WindowTag { Exposed, /* extensible */ }
pub struct Window {
    pub id: WindowId, pub actor: ActorId, pub tag: WindowTag,
    pub start: Tick, pub end: Tick, pub magnitude: Fixed,
}
```

## 6. The tick loop (`sim.rs`, pseudocode — implement faithfully)

```
fn run_until_decision_or_end(sim) -> StepResult:
    loop:
        resolve_tick(sim, sim.current_tick)         // §7; emits events

        // serialize decisions at this tick
        loop:
            pending = collect_actionable(sim)        // see below, deterministically ordered
            if pending.is_empty(): break
            d = pending.first()                      // (faction_order, actor_id) ascending
            view = foresight::project(sim, d.observer)   // §10
            return StepResult::Decision { decision: d, view }
            // (Sim::submit(cmd) applies the command then the caller re-enters this fn)

        if combat_ended(sim): return StepResult::Ended(outcome(sim))

        next = next_interesting_tick(sim)            // min phase boundary / window end / ready time
        match next:
            Some(t) => sim.current_tick = t
            None    => return StepResult::Ended(Stalemate)   // guard; should not happen
```

`collect_actionable` returns, at `current_tick`:
- every actor in `Idle` state whose `next_ready_tick <= current_tick` and who has **not yet
  decided this tick** (must act — may `CommitAction` or `Hold`), plus
- every actor with `tempo > 0` who has elected to **dilate** this tick and has not yet
  decided (may apply `EditVerb`s / insert an action / `Pass`).

A `Pass` or `Hold` marks that actor *decided-for-this-tick* so it is not re-offered, which
guarantees the inner loop terminates. `Hold` sets `next_ready_tick = current_tick +
HOLD_QUANTUM` (**TUNABLE**) so a waiting actor cannot stall the sim forever.

`Sim::submit(command)` applies a command as a pure transform and emits resulting events:
- `CommitAction { actor, mv, target }` → schedule an `ActionInstance` starting at
  `current_tick`; set actor `Committed`; deduct `tempo_cost`; emit `ActionScheduled`.
- `EditVerb(v)` → apply per §8; deduct Tempo; emit the verb's events.
- `Pass` / `Hold` → as above.

## 7. `resolve_tick` — activation, contact, effects (`resolve.rs`)

For a given `tick`, in this exact order:

1. **Expire windows** whose `end == tick`. Emit `WindowClosed`. Remove from store.
2. **Phase entry events** for instances crossing a boundary at `tick`
   (`ActionStarted` at `start_tick`; `ActionActive` at `active_start`). Keep instance
   `status` in sync (`Scheduled → Resolving` at active).
3. **Contact resolution.** Gather all instances with `active_start == tick`. Sort by the
   strict order in §4. For each, in order:
   - Skip if `status == Cancelled`.
   - Resolve target validity: target exists, not `Down`, and `range`/zone requirement met;
     if `requires_tag` is set, target must currently carry that window tag (else the move
     **fizzles** — emit `ActionFizzled`, no effects).
   - **Interrupt check (this is the "read the wind-up" payoff):** if the target currently
     has an `ActionInstance` whose phase at `tick` is **startup** and that move's
     `has_armor == false`, cancel the target's instance (`status = Cancelled`, set target
     `Idle`, `next_ready_tick = tick`), emit `Interrupted`, and award interrupt Tempo to the
     attacker's faction (§9).
   - Apply this move's `effects` in listed order:
     - `Damage` → subtract from target HP; if a matching `Exposed` window is active on the
       target, multiply by `config.exposed_damage_mult` (**TUNABLE**) and award window-hit
       Tempo. Emit `Hit { attacker, target, amount, knockback }`.
     - `LineKnockback { ticks: k }` → **the reversal mechanic.** If the target has a
       `Committed` instance, shift that instance's `start_tick` later by `k` (recompute
       derived boundaries) and emit `LineShoved`. Otherwise increase the target's
       `next_ready_tick` by `k`. Either way the target's next contribution to the fight is
       pushed down the line.
     - `OpenWindow` → insert a `Window` on the target spanning `[tick, tick + duration]`;
       emit `WindowOpened`.
     - `Stagger { ticks }` → set target `Staggered { until: tick + ticks }`; emit
       `ActorStaggered`.
   - If target HP `<= 0`: set `Down`, cancel its committed instance if any, emit
     `ActorDowned`.
4. **Completion.** Instances with `recovery_end == tick`: set `status = Resolved`, set the
   owning actor `Idle` with `next_ready_tick = tick`. (They become decision points on the
   next `collect_actionable`.)
5. **Stagger expiry.** Actors whose `Staggered.until == tick` return to `Idle`.

## 8. Edit verbs (`verbs.rs`) — pure timeline transforms

Each verb is `fn(&mut Sim, params) -> Result<(), VerbError>`, costs Tempo scaled by
magnitude, respects `config.edit_lock_policy`, and emits events.

- `Slow { instance, ticks }` → push `start_tick` later by `ticks`; recompute boundaries.
- `Haste { instance, ticks }` → pull `start_tick` earlier by `ticks` (clamp so it never
  precedes `current_tick`).
- `Interrupt { instance }` → if `instance` is currently in **startup** and `!has_armor`,
  cancel it (as in §7's interrupt). Otherwise `Err(NotInterruptible)`.
- `Insert { actor, mv, target }` → schedule a new action for a *dilating* actor outside its
  normal readiness (the bullet-time insert). Requires the actor be eligible to act.

`edit_lock_policy` (**TUNABLE**):
- `LockedOnCommit` (default): once an actor commits an action, **its owner** cannot
  `Slow`/`Haste`/`Interrupt`/cancel it; only the *opposing* faction can affect it via verbs
  or contact. (More honest, more tense.)
- `EditableUntilActive`: the owner may re-edit its own instance while still in startup.

## 9. Tempo economy (`tempo.rs`)

`tempo_model` (**TUNABLE**):
- `PerActorOpposed` (default): each actor has its own `tempo` pool; mooks spawn with 0,
  the player party and elite enemies spawn with a configured amount. Both sides may dilate,
  enabling enemy-driven reversals.
- `SharedPlayerPool`: a single player-faction pool; enemies never dilate.

Spending: dilating to act outside readiness and applying edit verbs cost Tempo. Verb cost =
`base_cost + per_tick_cost * magnitude` (**TUNABLE** constants). A command that would
overspend is rejected (`Err(InsufficientTempo)`); the engine must never go negative.

Accrual (all **TUNABLE** magnitudes), awarded during `resolve_tick`:
- landing an `Interrupt`,
- landing a hit inside an `Exposed` window (the squad payoff),
- (optional) per clean exchange survived.

Emit `TempoChanged { actor, delta, new_total }` on every change.

## 10. Foresight projection (`foresight.rs`)

`project(sim, observer) -> ForesightView` is a **pure, read-only** function. The view
contains only what `observer` is allowed to see:

- The timeline restricted to `[current_tick, current_tick + observer.foresight_horizon]`.
- For each visible enemy instance, expose **only the currently-committed action's phases**
  (startup/active/recovery boundaries) — never the actor's *future intentions* beyond what
  is already committed. Fog everything past the committed action.
- Own-faction instances are fully visible.
- Active windows, current Tempo pools (own faction full; enemy Tempo optionally hidden via
  `config.hide_enemy_tempo`, **TUNABLE**), HP, and zones.

This view is what the UI will later render and what the AI policy consumes (with its own,
possibly smaller, horizon). It must contain no data the observer shouldn't have, so the
same function safely serves both player and AI.

## 11. Minimal spatial model (v1)

Keep spatial *abstract* in v1: a small set of `ZoneId`s (e.g. an enum or `u8`). `ZoneReq`
is `SameZone | AnyZone`. Moves may require the target be in the same zone. A `Reposition`
move (changes `actor.zone`, occupies tick-time like any action) is enough to make zones
matter without a continuous positional sim. Full lane + lateral-offset positioning (cf. the
earlier TICK spatial model) is a **future module** — design `ZoneId`/`ZoneReq` behind a
small trait-ish boundary so it can be swapped without touching resolution logic.

## 12. Events & the trace contract (`events.rs`)

A single `enum Event`, `#[derive(Serialize, Deserialize, PartialEq)]`, each carrying the
`tick` it occurred at and relevant ids/magnitudes. This ordered `Vec<Event>` IS:
(a) the contract the presentation layer will consume, and (b) the golden-vector trace.

```rust
pub enum Event {
    TickAdvanced     { to: Tick },
    DecisionRequired { actor: ActorId, faction: FactionId, tick: Tick },
    ActionScheduled  { instance: InstanceId, actor: ActorId, mv: MoveId, target: Option<ActorId>, start: Tick },
    ActionStarted    { instance: InstanceId, tick: Tick },   // enters startup
    ActionActive     { instance: InstanceId, tick: Tick },   // enters active
    Hit              { instance: InstanceId, attacker: ActorId, target: ActorId, amount: i32 },
    LineShoved       { target: ActorId, instance: Option<InstanceId>, ticks: u32 },
    Interrupted      { interrupted: InstanceId, by: ActorId },
    ActionFizzled    { instance: InstanceId, reason: FizzleReason },
    WindowOpened     { window: WindowId, actor: ActorId, tag: WindowTag, end: Tick },
    WindowClosed     { window: WindowId },
    ActorStaggered   { actor: ActorId, until: Tick },
    TempoChanged     { actor: ActorId, delta: i32, new_total: i32 },
    ActionCompleted  { instance: InstanceId, tick: Tick },
    ActorDowned      { actor: ActorId, tick: Tick },
    CombatEnded      { outcome: Outcome },
}
```

Event emission order within a tick must follow the resolution order in §4/§7 so traces are
stable.

## 13. Controller interface (`controller.rs`)

The engine is driven externally; both player and AI implement one trait.

```rust
pub enum Command { CommitAction { mv: MoveId, target: Option<ActorId> },
                   EditVerb(EditVerb), Hold, Pass, Dilate /* elects to act this tick */ }

pub trait Controller {
    fn decide(&mut self, decision: &Decision, view: &ForesightView) -> Command;
}
```

Provide two implementations:
- `ScriptedController` — replays a predetermined `Vec<Command>` keyed by `(tick, actor)`.
  This is what golden-vector tests use. Deterministic by construction.
- `StubAi` — a minimal, deterministic policy (e.g. "if a target is in range, commit the
  highest-priority affordable damage move; never dilate"). No cleverness in v1; it is a
  placeholder behind the `Controller` trait so it can be replaced wholesale later.

## 14. Config / tunables (`config.rs`)

All of these are fields on a `Config` struct, with documented defaults:

| Knob | Default | Notes |
|---|---|---|
| `cost_model` | `TickOccupancy` | vs `TickPlusAp` (adds a separate AP axis) |
| `edit_lock_policy` | `LockedOnCommit` | vs `EditableUntilActive` |
| `tempo_model` | `PerActorOpposed` | vs `SharedPlayerPool` |
| `hide_enemy_tempo` | `true` | foresight fog for enemy Tempo |
| `exposed_damage_mult` | `1.5` (as `Fixed`) | window payoff |
| `HOLD_QUANTUM` | `4` ticks | anti-stall for `Hold` |
| `verb_base_cost` / `verb_per_tick_cost` | tuned | Tempo spend |
| `tempo_on_interrupt` / `tempo_on_window_hit` | tuned | Tempo accrual |
| `default_foresight_horizon` | tuned | per-actor stat fallback |

## 15. Golden vectors & `PORTING.md`

- `scenarios/*.json`: a scenario defines config overrides, the move library, initial actors
  (with factions, HP, Tempo, zones), the RNG seed, and a `ScriptedController` command log.
- `golden/*.golden.json`: the expected `Vec<Event>` for each scenario.
- `tests/golden.rs`: for each scenario, run the sim to completion with the scripted
  controller, serialize the event stream, and assert equality with the committed golden
  file. Provide a `BLESS=1` env switch to regenerate goldens intentionally.
- Author at least these scenarios:
  1. **two-mook trade** — no Tempo, pure reactive readiness, one downs the other.
  2. **interrupt** — player reads an enemy wind-up, inserts a fast strike, cancels it.
  3. **line-knockback reversal** — enemy is winning on the line; a knockback hit shoves its
     committed action late and the player's squad gets a free exchange.
  4. **setup → payoff** — one ally opens `Exposed`, a second lands the payoff inside the
     window for the damage multiplier and Tempo.
  5. **opposed dilation** — an elite spends its own Tempo to `Slow` the player's payoff,
     turning the exchange back.
- `PORTING.md`: state the determinism rules (§4) as the portability contract; explain that
  any reimplementation or downstream adapter (including the future Bevy layer) is correct
  iff it reproduces the golden traces bit-for-bit.

## 16. Public API & example driver

```rust
// core
let mut sim = Sim::new(config, scenario_setup);   // assigns ids, seeds RNG
loop {
    match sim.run_until_decision_or_end() {
        StepResult::Decision { decision, view } => {
            let cmd = controllers[decision.faction].decide(&decision, &view);
            sim.submit(cmd);
        }
        StepResult::Ended(outcome) => break,
    }
    // emitted events are drained via sim.drain_events() into a sink
}
```

`combat-cli`: takes a scenario path, runs it with `ScriptedController` (or `StubAi` for the
enemy faction), and writes the event trace to stdout / a file as pretty JSON. This is the
human-inspectable proof the engine works without any renderer.

## 17. Test requirements & acceptance criteria

Definition of done:
1. `cargo build`, `cargo clippy` clean; no floats in `combat-core`; no Bevy dep.
2. All five golden scenarios pass and are committed.
3. **Determinism test:** running any scenario twice yields identical traces; running with
   the event-driven tick-jump vs forced tick-by-tick advance yields identical traces.
4. Property tests (via `proptest` or hand-rolled): Tempo never goes negative; no instance
   ever activates before `current_tick`; a `LockedOnCommit` owner can never alter its own
   committed instance; total HP is monotonic non-increasing; the sim always terminates
   (no infinite decision loop) within a bounded tick count for bounded scenarios.
5. `combat-cli` runs scenario 3 and prints a legible reversal trace.

## 18. Non-goals (v1) & future modules

Explicitly **not** in this build, but design seams so each slots in without rewrites:
- **Rendering / Bevy adapter.** Later: a `combat-bevy` crate mapping core structs →
  components and the `Sim::step` orchestration → a Bevy schedule, consuming the event
  stream to drive animation/camera/VFX. The core must stay bevy-free.
- **RPG stat layer.** v1 moves are hand-authored `MoveDef`s. Later: a WWN-style RPG layer
  that *compiles into* `FrameData` + `Effect`s through a single bridge module (mirrors the
  TICK RPG→frame-data bridge). Keep `MoveDef` construction behind a builder so the bridge
  can target it.
- **Full spatial model.** Replace the abstract `ZoneId`/`ZoneReq` with lane + lateral
  offset behind the same boundary.
- **Sophisticated AI.** Replace `StubAi` behind the `Controller` trait; it may consume the
  same `ForesightView` the player sees.
- **Theme layer.** None of the Gnostic framing belongs in the sim; it is presentation +
  later AI flavor (an "Archon" is just a `Controller` that holds Tempo and edits the line).

---

### Build order suggested for Claude Code
1. `tick`, `ids`, `config`, `events` (the vocabulary).
2. `actor`, `moves`, `timeline` (state).
3. `resolve` + `sim` tick loop with reactive readiness only (no Tempo, no verbs) — get
   scenario 1 green.
4. `verbs` + `tempo` + interrupt/knockback/windows — scenarios 2–4.
5. opposed dilation — scenario 5.
6. property + determinism tests, `combat-cli`, `PORTING.md`.
