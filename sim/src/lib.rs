//! Engine-agnostic agent-based modelling core.
//!
//! Symbol → trait: `E` [`Substrate`], `Φ` [`Substrate::evolve`], `Π`
//! [`Actor::perceive`], `Δ` [`Actor::decide`], `a` [`Action`], `ρ`
//! [`Action::claim`], `σ` [`Scheduler`], `M` [`Observer`], `ω` [`Rng`], `T`
//! [`Simulation`].

pub mod action;
pub mod actor;
pub mod observer;
pub mod rng;
pub mod scheduler;
pub mod simulation;
pub mod substrate;

pub use action::Action;
pub use actor::Actor;
pub use observer::Observer;
pub use rng::Rng;
pub use scheduler::{Scheduler, Synchronous};
pub use simulation::Simulation;
pub use substrate::Substrate;
