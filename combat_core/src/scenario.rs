//! Scenario format and the run harness (spec §15/§16). A [`Scenario`] is a fully declarative
//! description of a fight — config overrides, the move library, the initial actors, the RNG seed,
//! and a scripted command log — that (de)serializes to JSON. [`run`] plays it to completion with
//! a [`ScriptedController`] for scripted factions and a [`StubAi`] for any AI factions, returning
//! the event trace. Same scenario in, same trace out (the golden-vector contract).

use crate::actor::{Actor, ActorState, Vitals};
use crate::config::Config;
use crate::controller::{Command, Controller, ScriptedController, StubAi};
use crate::events::Event;
use crate::ids::{ActorId, FactionId, MoveId};
use crate::moves::{MoveDef, MoveLibrary};
use crate::sim::{Sim, StepResult};
use crate::space::Pos;
use crate::tick::Tick;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One combatant's starting state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActorSpec {
    pub id: u32,
    pub faction: u32,
    pub hp: i32,
    #[serde(default)]
    pub tempo: i32,
    /// Starting position on the field (integer world units).
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub foresight_horizon: u32,
    /// The tick at which this actor first becomes ready to act.
    #[serde(default)]
    pub ready_tick: u64,
    /// Move ids the actor may commit.
    pub kit: Vec<u32>,
}

/// One entry in the scripted command log, keyed by `(tick, actor)`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScriptedCmd {
    pub tick: u64,
    pub actor: u32,
    pub command: Command,
}

/// A fully declarative fight.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub config: Config,
    pub seed: u64,
    pub moves: Vec<MoveDef>,
    pub actors: Vec<ActorSpec>,
    #[serde(default)]
    pub script: Vec<ScriptedCmd>,
    /// Factions driven by [`StubAi`] rather than the script.
    #[serde(default)]
    pub ai_factions: Vec<u32>,
}

impl Scenario {
    /// Build the initial [`Sim`] from this scenario (no controllers wired yet).
    pub fn build(&self) -> Sim {
        let lib = MoveLibrary::from_defs(self.moves.iter().cloned());
        let mut sim = Sim::new(self.config, lib, self.seed);
        for a in &self.actors {
            let actor = Actor {
                id: ActorId(a.id),
                faction: FactionId(a.faction),
                vitals: Vitals::new(a.hp),
                tempo: a.tempo,
                next_ready_tick: Tick(a.ready_tick),
                state: ActorState::Idle,
                foresight_horizon: a.foresight_horizon,
                pos: Pos::from_ints(a.x, a.y),
            };
            let kit = a.kit.iter().map(|&m| MoveId(m)).collect();
            sim.add_actor(actor, kit);
        }
        sim
    }
}

/// Play a scenario to completion and return its full event trace.
pub fn run(scenario: &Scenario) -> Vec<Event> {
    run_with_mode(scenario, false)
}

/// As [`run`], but optionally forcing the tick-by-tick advance (the determinism harness compares
/// the two modes — they must agree).
pub fn run_with_mode(scenario: &Scenario, force_tick_by_tick: bool) -> Vec<Event> {
    let mut sim = scenario.build();
    sim.set_force_tick_by_tick(force_tick_by_tick);

    let mut scripted = ScriptedController::new();
    for c in &scenario.script {
        scripted.push(c.tick, ActorId(c.actor), c.command);
    }
    let mut ai = StubAi::new(sim.library().clone());
    let ai_factions: BTreeSet<u32> = scenario.ai_factions.iter().copied().collect();

    while let StepResult::Decision { decision, view } = sim.run_until_decision_or_end() {
        let cmd = if ai_factions.contains(&decision.faction.0) {
            ai.decide(&decision, &view)
        } else {
            scripted.decide(&decision, &view)
        };
        sim.submit(cmd);
    }
    sim.drain_events()
}
