//! Measurement `M`.

use crate::actor::Actor;
use crate::substrate::Substrate;

/// Extracts observables from the current configuration.
pub trait Observer<S: Substrate> {
    type Observation;

    fn observe(
        &mut self,
        actors: &[Box<dyn Actor<S>>],
        substrate: &S,
        time: u64,
    ) -> Self::Observation;
}
