//! The external decision interface (spec §13). The engine never decides for itself — at each
//! serialized decision point it hands a [`Decision`] plus a fogged [`ForesightView`] to a
//! [`Controller`] and applies the [`Command`] it returns.
//!
//! Three implementations ship: [`ScriptedController`] (replays a fixed command log — what the
//! golden-vector tests drive), [`StubAi`] (a minimal, deterministic placeholder that never
//! dilates), and [`EliteAi`] (a stub that *does* dilate — spends Tempo to interrupt/slow the
//! player's line, the source of enemy-driven reversals).

use crate::config::Config;
use crate::foresight::{ActorStateView, ForesightView, VisibleInstance};
use crate::ids::{ActorId, FactionId, MoveId};
use crate::moves::{Effect, MoveLibrary};
use crate::space::Pos;
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

/// A readiness action on the continuous field: commit the highest-priority affordable damage move
/// against the nearest enemy *within its reach*; if nothing is in reach, close on the nearest enemy
/// with an Approach move; else `Hold`. Deterministic (nearest, then lowest move/actor id, ties).
fn readiness_command(lib: &MoveLibrary, decision: &Decision, view: &ForesightView) -> Command {
    let my_pos = view
        .actors
        .iter()
        .find(|a| a.id == decision.actor)
        .map(|a| a.pos)
        .unwrap_or(Pos::ORIGIN);
    let enemies: Vec<(ActorId, Pos)> = view
        .actors
        .iter()
        .filter(|a| a.faction != view.own_faction && !matches!(a.state, ActorStateView::Down))
        .map(|a| (a.id, a.pos))
        .collect();
    if enemies.is_empty() {
        return Command::Hold;
    }

    // Best damage move with a target in reach.
    let mut best: Option<(u8, MoveId, ActorId)> = None;
    for &mv in &view.own_moves {
        let Some(def) = lib.get(mv) else { continue };
        if def.tempo_cost > view.own_tempo || def.requires_tag.is_some() {
            continue;
        }
        if !def
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Damage { .. }))
        {
            continue;
        }
        let target = enemies
            .iter()
            .filter(|(_, p)| my_pos.within(*p, def.reach))
            .min_by_key(|(id, p)| (my_pos.dist_sq(*p), id.0))
            .map(|(id, _)| *id);
        if let Some(t) = target {
            let pick = best.is_none_or(|(p, m, _)| {
                def.priority_class > p || (def.priority_class == p && mv.0 < m.0)
            });
            if pick {
                best = Some((def.priority_class, mv, t));
            }
        }
    }
    if let Some((_, mv, target)) = best {
        return Command::CommitAction {
            mv,
            target: Some(target),
        };
    }

    // Nothing in reach — close on the nearest enemy with an Approach move.
    let nearest = enemies
        .iter()
        .min_by_key(|(id, p)| (my_pos.dist_sq(*p), id.0))
        .map(|(id, _)| *id);
    for &mv in &view.own_moves {
        let Some(def) = lib.get(mv) else { continue };
        if def.tempo_cost > view.own_tempo {
            continue;
        }
        if def
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Approach { .. }))
        {
            return Command::CommitAction {
                mv,
                target: nearest,
            };
        }
    }
    Command::Hold
}

/// A dilation (edit) action for a Tempo-holder: interrupt the soonest unarmored wind-up if it can
/// afford it, else slow the most imminent opposing action, else `Pass`. `cfg` supplies the cost
/// model; ties break on the lowest instance id. This is what turns enemy Tempo into reversals.
fn dilation_command(lib: &MoveLibrary, cfg: &Config, view: &ForesightView) -> Command {
    let tempo = view.own_tempo;
    let now = view.current_tick;
    let mut foes: Vec<&VisibleInstance> = view.instances.iter().filter(|i| !i.own).collect();
    foes.sort_by_key(|i| (i.active_start.0, i.id.0));

    // Interrupt an unarmored action still in startup, if affordable.
    let interrupt_cost = cfg.verb_base_cost + cfg.verb_per_tick_cost;
    for inst in &foes {
        let in_startup = now >= inst.start_tick && now < inst.active_start;
        let armored = lib.get(inst.mv).map(|d| d.has_armor).unwrap_or(true);
        if in_startup && !armored && tempo >= interrupt_cost {
            return Command::EditVerb(EditVerb::Interrupt { instance: inst.id });
        }
    }
    // Else shove the most imminent threat down the line.
    if let Some(inst) = foes.first() {
        let ticks = 2u32;
        let cost = cfg.verb_base_cost + cfg.verb_per_tick_cost * ticks as i32;
        if tempo >= cost {
            return Command::EditVerb(EditVerb::Slow {
                instance: inst.id,
                ticks,
            });
        }
    }
    Command::Pass
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
        match decision.kind {
            DecisionKind::Dilation => Command::Pass,
            DecisionKind::Readiness => readiness_command(&self.lib, decision, view),
        }
    }
}

/// Like [`StubAi`], but it *dilates*: a Tempo-holding elite spends to interrupt or slow the
/// player's incoming actions (still deterministic). This is the enemy controller that makes the
/// per-actor opposed Tempo economy two-sided — the source of enemy-driven reversals.
#[derive(Clone, Debug)]
pub struct EliteAi {
    lib: MoveLibrary,
    cfg: Config,
}

impl EliteAi {
    pub fn new(lib: MoveLibrary, cfg: Config) -> Self {
        Self { lib, cfg }
    }
}

impl Controller for EliteAi {
    fn decide(&mut self, decision: &Decision, view: &ForesightView) -> Command {
        match decision.kind {
            DecisionKind::Readiness => readiness_command(&self.lib, decision, view),
            DecisionKind::Dilation => dilation_command(&self.lib, &self.cfg, view),
        }
    }
}
