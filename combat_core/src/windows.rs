//! Windows — timed tags an actor carries (spec §5). v1 has one tag, `Exposed`, the setup→payoff
//! hook: a hit landed inside an `Exposed` window is multiplied and pays Tempo.

use crate::ids::{ActorId, WindowId};
use crate::tick::{Fixed, Tick};
use serde::{Deserialize, Serialize};

/// The kind of a window. Extensible; v1 ships just `Exposed`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum WindowTag {
    Exposed,
}

/// A timed tag on an actor, spanning `[start, end]` (inclusive of `start`; it expires *at* `end`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Window {
    pub id: WindowId,
    pub actor: ActorId,
    pub tag: WindowTag,
    pub start: Tick,
    pub end: Tick,
    pub magnitude: Fixed,
}

/// Storage for live windows. A sorted `Vec` keyed by id — we only ever iterate it in id order, so
/// resolution is never influenced by insertion timing or hash iteration (spec §4).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WindowStore {
    windows: Vec<Window>,
    next_id: u32,
}

impl WindowStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a window on `actor`, returning its fresh id. Windows are appended in id order.
    pub fn open(
        &mut self,
        actor: ActorId,
        tag: WindowTag,
        start: Tick,
        end: Tick,
        magnitude: Fixed,
    ) -> WindowId {
        let id = WindowId(self.next_id);
        self.next_id += 1;
        self.windows.push(Window {
            id,
            actor,
            tag,
            start,
            end,
            magnitude,
        });
        id
    }

    /// Remove and return every window whose `end == tick`, in id order.
    pub fn expire_at(&mut self, tick: Tick) -> Vec<Window> {
        let mut expired = Vec::new();
        self.windows.retain(|w| {
            if w.end == tick {
                expired.push(*w);
                false
            } else {
                true
            }
        });
        expired
    }

    /// The first active window on `actor` with `tag` at `tick` (in id order), if any.
    pub fn active(&self, actor: ActorId, tag: WindowTag, tick: Tick) -> Option<&Window> {
        self.windows
            .iter()
            .find(|w| w.actor == actor && w.tag == tag && w.start <= tick && tick < w.end)
    }

    /// All live windows, in id order (read-only; for foresight projection).
    pub fn iter(&self) -> impl Iterator<Item = &Window> {
        self.windows.iter()
    }

    /// Drop every window belonging to `actor` (used when an actor goes down).
    pub fn drop_actor(&mut self, actor: ActorId) {
        self.windows.retain(|w| w.actor != actor);
    }
}
