//! The driver.

use crate::actor::Actor;
use crate::substrate::Substrate;

/// Owns the configuration and iterates the transition `T`.
pub trait Simulation {
    type Substrate: Substrate;

    fn substrate(&self) -> &Self::Substrate;
    fn actors(&self) -> &[Box<dyn Actor<Self::Substrate>>];
    fn time(&self) -> u64;

    /// One transition `x_{t+1} = T(x_t, ω_t)`.
    fn step(&mut self);

    fn run(&mut self, steps: u64) {
        for _ in 0..steps {
            self.step();
        }
    }
}
