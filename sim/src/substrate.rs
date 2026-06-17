//! Shared environment `E`.

use crate::rng::Rng;

/// The shared environment actors perceive and act on.
pub trait Substrate: Send + Sync {
    type Position;
    /// Shared view type returned by `Actor::perceive`.
    type Perception;
    /// Shared effects actors send one another via `Actor::interact`.
    type Interaction;
    /// Identifies an exclusive resource actions contend for; equal claims
    /// compete. Use `()` if the model has no contested resources.
    type Claim: PartialEq;

    /// Autonomous dynamics `Φ` (diffusion, decay, regrowth). Default: no-op.
    fn evolve(&mut self, _rng: &mut dyn Rng) {}
}
