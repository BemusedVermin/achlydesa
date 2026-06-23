//! Foresight projection (spec §10): a pure, read-only window onto the sim containing only what an
//! observer is allowed to see. The same function safely serves both the player UI and an AI
//! policy — fog is applied here, once, so no caller can peek at data it shouldn't.

use crate::actor::{ActorState, ZoneId};
use crate::ids::{ActorId, FactionId, InstanceId, MoveId};
use crate::sim::Sim;
use crate::tick::Tick;
use crate::windows::{Window, WindowTag};
use serde::{Deserialize, Serialize};

/// A fogged summary of an actor's live state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ActorStateView {
    Idle,
    Acting,
    Staggered { until: Tick },
    Down,
}

/// What an observer can see of one actor.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ActorView {
    pub id: ActorId,
    pub faction: FactionId,
    pub hp: i32,
    pub max_hp: i32,
    pub zone: ZoneId,
    /// `None` when fogged (enemy Tempo with `hide_enemy_tempo`).
    pub tempo: Option<i32>,
    pub state: ActorStateView,
}

/// One committed action on the line, as the observer sees it. Own-faction instances are fully
/// visible; enemy instances expose only their (already-committed) phase boundaries.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct VisibleInstance {
    pub id: InstanceId,
    pub actor: ActorId,
    pub faction: FactionId,
    pub mv: MoveId,
    pub target: Option<ActorId>,
    pub start_tick: Tick,
    pub active_start: Tick,
    pub active_end: Tick,
    pub recovery_end: Tick,
    /// True if this instance belongs to the observer's own faction.
    pub own: bool,
}

/// The read-only view handed to a controller at a decision point.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ForesightView {
    pub observer: ActorId,
    pub own_faction: FactionId,
    pub current_tick: Tick,
    /// How far ahead this observer sees.
    pub horizon: u32,
    /// The observer's Tempo (always visible to itself).
    pub own_tempo: i32,
    /// The moves the observer may commit.
    pub own_moves: Vec<MoveId>,
    /// Committed actions overlapping `[current_tick, current_tick + horizon]`, in id order.
    pub instances: Vec<VisibleInstance>,
    /// Active windows at `current_tick`, in id order.
    pub windows: Vec<WindowView>,
    /// Every actor, fogged per the rules above.
    pub actors: Vec<ActorView>,
}

/// A visible window (a flattened [`Window`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct WindowView {
    pub actor: ActorId,
    pub tag: WindowTag,
    pub start: Tick,
    pub end: Tick,
}

impl From<&Window> for WindowView {
    fn from(w: &Window) -> Self {
        WindowView {
            actor: w.actor,
            tag: w.tag,
            start: w.start,
            end: w.end,
        }
    }
}

fn state_view(state: ActorState) -> ActorStateView {
    match state {
        ActorState::Idle => ActorStateView::Idle,
        ActorState::Committed(_) => ActorStateView::Acting,
        ActorState::Staggered { until } => ActorStateView::Staggered { until },
        ActorState::Down => ActorStateView::Down,
    }
}

/// Project the sim into the view `observer` is allowed to see.
pub fn project(sim: &Sim, observer: ActorId) -> ForesightView {
    let me = sim.actor(observer).expect("observer exists");
    let own_faction = me.faction;
    let horizon = sim.effective_horizon(me);
    let now = sim.current_tick();
    let sight_end = now.saturating_add(horizon as u64);

    // Committed actions overlapping the sight window, in id order.
    let mut instances = Vec::new();
    for inst in sim.timeline().iter() {
        if !inst.live() {
            continue;
        }
        // Overlaps [now, sight_end]?  (its active/recovery may already straddle now.)
        if inst.start_tick > sight_end {
            continue;
        }
        if inst.recovery_end() < now {
            continue;
        }
        let faction = sim
            .actor(inst.actor)
            .map(|a| a.faction)
            .unwrap_or(own_faction);
        instances.push(VisibleInstance {
            id: inst.id,
            actor: inst.actor,
            faction,
            mv: inst.mv,
            target: inst.target,
            start_tick: inst.start_tick,
            active_start: inst.active_start(),
            active_end: inst.active_end(),
            recovery_end: inst.recovery_end(),
            own: faction == own_faction,
        });
    }

    // Active windows, in id order.
    let windows: Vec<WindowView> = sim
        .windows()
        .iter()
        .filter(|w| w.start <= now && now < w.end)
        .map(WindowView::from)
        .collect();

    // Actors, fogged per side.
    let hide_enemy_tempo = sim.config().hide_enemy_tempo;
    let mut actors: Vec<ActorView> = sim
        .actors()
        .map(|a| ActorView {
            id: a.id,
            faction: a.faction,
            hp: a.vitals.hp,
            max_hp: a.vitals.max_hp,
            zone: a.zone,
            tempo: if a.faction == own_faction || !hide_enemy_tempo {
                Some(a.tempo)
            } else {
                None
            },
            state: state_view(a.state),
        })
        .collect();
    actors.sort_by_key(|a| a.id);

    ForesightView {
        observer,
        own_faction,
        current_tick: now,
        horizon,
        own_tempo: me.tempo,
        own_moves: sim.kit(observer).to_vec(),
        instances,
        windows,
        actors,
    }
}
