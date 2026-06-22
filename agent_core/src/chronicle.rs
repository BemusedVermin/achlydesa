//! The **Chronicle** — a bounded, structured, off-by-default ring of recent world episodes: the
//! substrate the [`sift`](crate::sift) layer reads to perceive forming stories (and the eval
//! harness reads for retellings). It generalises the director's `BeatEvent` ring past beats to the
//! *emergent* world events the director did not author — a grudge formed, an opinion crossed, a
//! death, a war declared — which is exactly the half a bottom-up sifter needs.
//!
//! **Deterministic & off-by-default.** State lives in the [`Chronicle`] resource (no component); it
//! is inserted only when the sift layer is enabled, and every emit site is a guarded
//! `if let Some(c) = chronicle.as_deref_mut() { c.record(...) }` that records *intent at the
//! mutation site* (entities captured from locals, before any despawn) — never via bevy change
//! detection. With the layer off the resource is absent, every tap is a no-op, and the world is
//! byte-identical to one before the Chronicle existed. The ring's contents are a pure function of
//! the seeded, fixed-order tick.

use crate::beats::Register;
use bevy_ecs::prelude::*;
use game_sim::Coord;
use std::collections::VecDeque;

/// What kind of thing happened — the structured handle the sifter pattern-matches over. Prose-free;
/// the surface layer renders it. The violent core (`Killed`/`Death`) is first-class: a killing is
/// the apex narratable event. (Combat is a planned subsystem; until then a `Slay` beat records
/// `Killed` at enact while the body is finished by metabolism, and a starvation records `Death`.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Deserialize)]
pub enum EpisodeKind {
    /// A soul slain by another — `parties[0]` the slayer, `parties[1]` the victim.
    Killed,
    /// A soul died (starvation / unattributed) — `parties[0]` the dead.
    Death,
    /// A grudge formed — `parties[0]` the holder, `parties[1]` the target.
    GrievanceFormed,
    /// An opinion edge crossed a threshold — `parties[0]` -> `parties[1]`; `detail` = `-1` cold / `+1` warm.
    OpinionCrossed,
    /// `parties[0]` took the throne.
    Crowned,
    /// `parties[0]` was deposed.
    Deposed,
    /// `parties[0]` crossed a prevailing taboo.
    Transgressed,
    /// War declared — `parties[0]`/`parties[1]` the opposed sides (factions' leaders, if known).
    WarDeclared,
    /// The director staged a beat — `register` set; `parties[0]` lead, `parties[1]` the counterpart.
    BeatFired,
}

/// One structured episode. `Copy`, bounded party slots, so the ring stays cheap.
#[derive(Clone, Copy, Debug)]
pub struct Episode {
    pub id: u64,
    pub tick: u64,
    pub kind: EpisodeKind,
    /// actor, target, third — the cast a pattern binds (any may be absent).
    pub parties: [Option<Entity>; 3],
    pub place: Coord,
    /// Set for [`EpisodeKind::BeatFired`] (the beat's register), else `None`.
    pub register: Option<Register>,
    /// Kind-specific scalar: e.g. `OpinionCrossed` direction (`-1` cold / `+1` warm).
    pub detail: i32,
}

/// A bounded ring of recent episodes — the sifter's substrate. A resource (no component), so a
/// chronicle-free world is byte-identical. Inserted only when the sift layer is enabled.
#[derive(Resource, Debug)]
pub struct Chronicle {
    ring: VecDeque<Episode>,
    cap: usize,
    next_id: u64,
}

impl Chronicle {
    /// A fresh chronicle holding at most `cap` recent episodes (oldest dropped past the cap).
    pub fn new(cap: usize) -> Self {
        Self {
            ring: VecDeque::new(),
            cap: cap.max(1),
            next_id: 0,
        }
    }

    /// Append an episode at `tick`. Called from the guarded emit taps; records intent (the
    /// entities are passed in, captured before any despawn), so it never races component state.
    pub fn record(
        &mut self,
        tick: u64,
        kind: EpisodeKind,
        parties: [Option<Entity>; 3],
        place: Coord,
        register: Option<Register>,
        detail: i32,
    ) {
        let id = self.next_id;
        self.next_id += 1;
        self.ring.push_back(Episode {
            id,
            tick,
            kind,
            parties,
            place,
            register,
            detail,
        });
        while self.ring.len() > self.cap {
            self.ring.pop_front();
        }
    }

    /// The episodes held, oldest first — what the sifter and the eval harness read.
    pub fn recent(&self) -> impl Iterator<Item = &Episode> {
        self.ring.iter()
    }

    /// How many episodes are currently held.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_is_bounded_and_ordered() {
        let mut c = Chronicle::new(3);
        let at = Coord::new(0, 0);
        for _ in 0..5 {
            c.record(1, EpisodeKind::Death, [None; 3], at, None, 0);
        }
        // Capped to the loudest few (here, the most recent), oldest dropped.
        assert_eq!(c.len(), 3);
        let ids: Vec<u64> = c.recent().map(|e| e.id).collect();
        assert_eq!(ids, vec![2, 3, 4], "keeps the most recent, in order");
    }
}
