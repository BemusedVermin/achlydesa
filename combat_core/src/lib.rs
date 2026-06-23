//! `combat_core` — a headless, deterministic, engine-agnostic **tick-based contested-timeline**
//! combat simulation.
//!
//! Combat is a single global tick line that actors place actions onto and that the player (and
//! elite enemies) can perceive and edit — slowing an incoming threat, interrupting a wind-up,
//! opening a window for an ally. The cadence is **read → edit → resolve a burst → read again**.
//!
//! This crate is *only the simulation*: it computes the resolved timeline and emits a typed,
//! ordered [`Event`] stream. Visuals, camera, time-remapping, and VFX are a downstream consumer
//! of that stream and live elsewhere (the Bevy app). The single most important property is
//! **determinism**: the same config + scenario + command sequence yields a **bit-identical**
//! event stream across runs and machines. The rules that guarantee it are in `PORTING.md`.
//!
//! No Bevy, no rendering, no I/O, no floating point, no wall clock, no threads. Time is integer
//! [`Tick`]s; the handful of continuous magnitudes use 16.16 [`Fixed`]. Iteration that can affect
//! results is always over ordered collections.
//!
//! # Driving the sim
//! ```
//! use combat_core::*;
//! # fn demo(mut sim: Sim, controller: &mut dyn Controller) {
//! loop {
//!     match sim.run_until_decision_or_end() {
//!         StepResult::Decision { decision, view } => {
//!             let cmd = controller.decide(&decision, &view);
//!             sim.submit(cmd);
//!         }
//!         StepResult::Ended(_outcome) => break,
//!     }
//! }
//! let _trace = sim.drain_events();
//! # }
//! ```

pub mod actor;
pub mod config;
pub mod controller;
pub mod events;
pub mod foresight;
pub mod ids;
pub mod moves;
pub mod rng;
pub mod scenario;
pub mod sim;
pub mod space;
pub mod tick;
pub mod timeline;
pub mod verbs;
pub mod windows;

mod resolve;

// ── Flat public surface — `use combat_core::*` brings in the whole vocabulary. ──────────────
pub use actor::{Actor, ActorState, Vitals};
pub use config::{Config, CostModel, EditLockPolicy, TempoModel};
pub use controller::{
    Command, Controller, Decision, DecisionKind, EliteAi, ScriptedController, StubAi,
};
pub use events::{Event, FizzleReason, Outcome};
pub use foresight::{ActorStateView, ActorView, ForesightView, VisibleInstance, WindowView};
pub use ids::{ActorId, FactionId, InstanceId, MoveId, WindowId};
pub use moves::{Effect, FrameData, MoveBuilder, MoveDef, MoveLibrary};
pub use rng::Rng;
pub use scenario::{ActorSpec, Scenario, ScriptedCmd, run, run_with_mode};
pub use sim::{Sim, StepResult};
pub use space::Pos;
pub use tick::{Fixed, Tick};
pub use timeline::{ActionInstance, InstanceStatus, Phase, Timeline};
pub use verbs::{EditVerb, VerbError};
pub use windows::{Window, WindowStore, WindowTag};
