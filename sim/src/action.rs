//! Actions.

use crate::actor::Actor;
use crate::substrate::Substrate;

/// A change an actor applies to the world: the substrate and other actors.
///
/// `actors[me]` is the acting actor; reach others by index and send a typed
/// `S::Interaction` via `Actor::interact`.
pub trait Action<S: Substrate>: Send + Sync {
    fn apply(&self, actors: &mut [Box<dyn Actor<S>>], me: usize, substrate: &mut S);

    /// The exclusive resource this action needs, if any. Actions with equal
    /// claims contend; the scheduler grants one winner per resource.
    fn claim(&self) -> Option<S::Claim> {
        None
    }

    /// Rank among contenders for the same claim — highest wins, ties broken at
    /// random. Use as a bid, initiative, etc.
    fn priority(&self) -> i64 {
        0
    }
}
