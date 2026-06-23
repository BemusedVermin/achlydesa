//! The external decision interface (spec §13). The engine never decides for itself — at each
//! serialized decision point it hands a [`Decision`] plus a fogged [`ForesightView`] to a
//! [`Controller`] and applies the [`Command`] it returns.
//!
//! Two implementations ship: [`ScriptedController`] (replays a fixed command log — what the
//! golden-vector tests drive) and [`StubAi`] (a minimal, deterministic placeholder policy).

use crate::foresight::ForesightView;
use crate::ids::{ActorId, FactionId, MoveId};
use crate::moves::{Effect, MoveLibrary, ZoneReq};
use crate::tick::Tick;
use crate::verbs::EditVerb;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// What kind of decision the engine is asking for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecisionKind {
    /// An `Idle`, ready actor *must* act: `CommitAction` or `Hold`.
    Readiness,
    /// A Tempo-holding actor *may* edit the line: `EditVerb`, or `Pass`.
    Dilation,
}

/// A request for one actor's decision at one tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Decision {
    pub actor: ActorId,
    pub faction: FactionId,
    pub tick: Tick,
    pub kind: DecisionKind,
}

/// What a controller hands back.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Command {
    /// Schedule an action starting now (a readiness action).
    CommitAction { mv: MoveId, target: Option<ActorId> },
    /// Spend Tempo to edit the line (a dilation action).
    EditVerb(EditVerb),
    /// Decline to act this tick; park until ready again (readiness only).
    Hold,
    /// Decline to dilate this tick (dilation only).
    Pass,
    /// Reserved (spec §13): elect to act this tick. v1 offers dilation automatically, so this is
    /// treated as `Pass`.
    Dilate,
}

/// Both the player and the AI implement this single trait.
pub trait Controller {
    fn decide(&mut self, decision: &Decision, view: &ForesightView) -> Command;
}

/// Replays a predetermined command log keyed by `(tick, actor)`. Deterministic by construction —
/// this is what the golden-vector tests use. Multiple commands at the same `(tick, actor)` are
/// consumed in order (so an actor can chain edits within a tick).
#[derive(Clone, Debug, Default)]
pub struct ScriptedController {
    script: BTreeMap<(u64, u32), VecDeque<Command>>,
}

impl ScriptedController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue `cmd` for `actor` at `tick`.
    pub fn at(mut self, tick: u64, actor: ActorId, cmd: Command) -> Self {
        self.script
            .entry((tick, actor.0))
            .or_default()
            .push_back(cmd);
        self
    }

    pub fn push(&mut self, tick: u64, actor: ActorId, cmd: Command) {
        self.script
            .entry((tick, actor.0))
            .or_default()
            .push_back(cmd);
    }
}

impl Controller for ScriptedController {
    fn decide(&mut self, decision: &Decision, _view: &ForesightView) -> Command {
        if let Some(queue) = self.script.get_mut(&(decision.tick.0, decision.actor.0))
            && let Some(cmd) = queue.pop_front()
        {
            return cmd;
        }
        // No scripted command: the safe default that keeps the sim moving.
        match decision.kind {
            DecisionKind::Readiness => Command::Hold,
            DecisionKind::Dilation => Command::Pass,
        }
    }
}

/// A minimal, deterministic policy (spec §13): on a readiness decision, commit the
/// highest-priority affordable damage move against the lowest-id reachable enemy; otherwise Hold.
/// Never dilates. No cleverness — a placeholder behind the trait.
#[derive(Clone, Debug)]
pub struct StubAi {
    lib: MoveLibrary,
}

impl StubAi {
    pub fn new(lib: MoveLibrary) -> Self {
        Self { lib }
    }
}

impl Controller for StubAi {
    fn decide(&mut self, decision: &Decision, view: &ForesightView) -> Command {
        if decision.kind == DecisionKind::Dilation {
            return Command::Pass;
        }
        let my_zone = view
            .actors
            .iter()
            .find(|a| a.id == decision.actor)
            .map(|a| a.zone)
            .unwrap_or(0);

        // Lowest-id alive enemy, by zone.
        let enemy = |req: ZoneReq| -> Option<ActorId> {
            view.actors
                .iter()
                .filter(|a| {
                    a.faction != view.own_faction
                        && !matches!(a.state, crate::foresight::ActorStateView::Down)
                })
                .filter(|a| req == ZoneReq::AnyZone || a.zone == my_zone)
                .map(|a| a.id)
                .min()
        };

        let mut best: Option<(u8, MoveId, ActorId)> = None;
        for &mv in &view.own_moves {
            let Some(def) = self.lib.get(mv) else {
                continue;
            };
            if def.tempo_cost > view.own_tempo {
                continue;
            }
            let deals_damage = def
                .effects
                .iter()
                .any(|e| matches!(e, Effect::Damage { .. }));
            if !deals_damage {
                continue;
            }
            // A move gated by a window the AI can't guarantee is skipped (keep it simple).
            if def.requires_tag.is_some() {
                continue;
            }
            let Some(target) = enemy(def.range) else {
                continue;
            };
            // Prefer higher priority; tie-break on the lowest move id.
            let pick = match best {
                None => true,
                Some((p, m, _)) => {
                    def.priority_class > p || (def.priority_class == p && mv.0 < m.0)
                }
            };
            if pick {
                best = Some((def.priority_class, mv, target));
            }
        }

        match best {
            Some((_, mv, target)) => Command::CommitAction {
                mv,
                target: Some(target),
            },
            None => Command::Hold,
        }
    }
}
