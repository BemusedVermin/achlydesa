# Porting `combat_core`

`combat_core` is a pure, deterministic state machine. Any reimplementation, or any downstream
adapter that drives it (including the future Bevy layer), is **correct if and only if it reproduces
the committed golden traces bit-for-bit**. This file states the rules that make that possible — the
portability contract. `SPEC.md` (the original brief) is the design rationale; this file is the law.

## The one property that matters

> Given the same `Config`, the same initial scenario, and the same sequence of controller commands,
> the emitted `Vec<Event>` is identical across runs, machines, and build profiles.

Everything below exists to guarantee that.

## Determinism rules (implement literally)

1. **Integer time only.** Time is whole `Tick`s (`u64`). There is no wall clock, no `Instant`, no
   frame delta. The engine may *jump* to the next interesting tick (the minimum over all upcoming
   phase boundaries, window expiries, and actor ready/stagger times) instead of incrementing by one
   — but the result is identical to ticking by one. The `golden::tick_jump_equals_tick_by_tick`
   test enforces this by running both modes and diffing.

2. **No floating point anywhere in the core.** Continuous magnitudes use `Fixed` (16.16, `i32`
   backing). The exposed-damage multiply rounds half-up through an `i64` intermediate
   (`Fixed::scale_int`). A port must round identically.

3. **A strict total order at every tie.** Whenever two things could happen "at once", break the tie
   deterministically. Simultaneous activations (instances entering their active frame on the same
   tick) resolve sorted by, in order:
   1. `priority_class` — **descending** (higher wins),
   2. `faction` id — **ascending** (Player faction `0` before Enemy `1`),
   3. `actor` id — ascending,
   4. `instance` id — ascending.

4. **Decisions are serialized.** At a tick, only one controller decision resolves at a time. The
   engine selects the next pending decision by `(faction, actor)` ascending, applies the returned
   command, then re-evaluates. Each actor decides at most once per tick (a `Hold`/`Pass`/commit/edit
   marks it decided-for-this-tick), which both serializes concurrency and guarantees the inner loop
   terminates.

5. **No `HashMap` iteration in ordering-sensitive paths.** Storage that is iterated to drive
   resolution is a `BTreeMap` or a sorted `Vec` (the timeline, the move library, the window store,
   the actor map). Pointer addresses and insertion timing never influence results.

6. **One seeded RNG stream.** Any randomized effect draws from the single `Rng` (SplitMix64) seeded
   from the scenario. v1 has no randomized effects, so the stream is currently never drawn from;
   when one is added, its draw order must be fixed by the resolution order above. Never use
   `thread_rng` or system entropy.

## Event emission

The event stream is the contract *and* the golden trace, so emission order is load-bearing. Within a
tick, events are emitted in resolution order (`resolve.rs`): window expiries → startup entries → per
active instance in strict order (`ActionActive`, then any `Interrupted`, then effects in listed
order, then `ActorDowned`) → completions. A `TickAdvanced { to }` is emitted **lazily**, exactly once
just before the first real event of any tick that produces output. Boring ticks emit nothing — that
is precisely what makes tick-jump and tick-by-tick traces equal.

`Event::DecisionRequired` is part of the contract but is **not** emitted into v1 traces: the live
decision is surfaced to the caller through `StepResult::Decision`, and emitting it would flood the
stream with every `Pass`/`Hold` and break the "boring ticks emit nothing" property. A downstream
layer that wants it can synthesize it from `StepResult`.

## v1 simplifications (documented deviations)

These keep v1 small; each is a seam, not a dead end. A port may keep or replace them, but the goldens
were generated with these behaviours:

- **One decision per actor per interesting tick.** An actor is offered a single decision (readiness
  *or* dilation) per tick, not a chain of edits. Termination is therefore trivial.
- **`startup ≥ 1` and `active ≥ 1`** are enforced by `FrameData::new`. A zero-startup move would
  need its contact resolved on the tick it was committed, after that tick's contact pass had already
  run — disallowed.
- **Dilation is offered** to a Tempo-holder only when it is busy or idle-but-not-ready *and* at least
  one opposing action is live on the line (something to react to). A ready, idle actor takes a
  readiness decision (commit/hold), not an edit.
- **`SharedPlayerPool`** is approximated: Tempo is always per-actor; the model only gates whether
  enemies may dilate. True cross-actor pooling is a future refinement.
- **Interrupt Tempo** is awarded only for *contact* interrupts (reading a wind-up), not for the
  `Interrupt` edit verb.

## Spatial model (`space.rs`)

Combat is on a **continuous 2D field**: every actor has a `Pos` (a pair of 16.16 `Fixed`). A move
targets a *person* and connects only if the target is within the move's `reach` at the active frame
— else its landing effects **whiff** (`FizzleReason::OutOfReach`). `Approach`/`Withdraw` effects
slide the actor along the 1D line to its target (movement resolves *before* the reach gate, so a
lunge can close and then strike). All distance math is integer: compare squared distances for the
reach test; a Newton integer `isqrt` is the only normalization (for the one-step movement vector).
No floats, fully deterministic — a port must reproduce `space.rs` exactly.

## Validating a port

1. Build with no Bevy dependency, no floats in the core, `cargo clippy` clean.
2. Run every `scenarios/*.json` and diff the trace against `golden/*.golden.json` — they must match
   byte-for-byte (modulo JSON pretty-printing; compare parsed `Vec<Event>`).
3. Re-run each scenario twice and assert equality (reproducibility).
4. Run each scenario in forced tick-by-tick mode and assert it equals the tick-jump trace.
5. The property tests (`tests/properties.rs`) must hold: Tempo never negative, total HP monotonic
   non-increasing, no activation before scheduling, bounded termination.

If all five hold, the port is correct.
