//! Agents.

use crate::action::Action;
use crate::rng::Rng;
use crate::substrate::Substrate;

/// An agent over substrate `S`. Object-safe: a population is a heterogeneous
/// `Vec<Box<dyn Actor<S>>>`.
pub trait Actor<S: Substrate>: Send + Sync {
    /// View of the world (`Π`).
    fn perceive(&self, substrate: &S) -> S::Perception;

    /// Choose an action (`Δ`), or `None` to pass for the rest of the tick.
    fn decide(&self, perception: &S::Perception, rng: &mut dyn Rng) -> Option<Box<dyn Action<S>>>;

    /// Respond to an effect from another actor. Default: ignore.
    fn interact(&mut self, _effect: &S::Interaction) {}
}
