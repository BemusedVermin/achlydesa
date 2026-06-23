//! `EliteAi` must actually *dilate* — spend Tempo to bend the player's line. This drives a fight
//! where the player winds up a slow, unarmored attack and a Tempo-holding elite is given the
//! chance to react; the elite should interrupt the wind-up. (The stub never would.)

use combat_core::*;

fn rig() -> (Sim, MoveLibrary, Config) {
    let lib = MoveLibrary::from_defs([
        MoveDef::builder(MoveId(1), "WindUp")
            .frames(6, 1, 3)
            .priority(1)
            .damage(10)
            .build(),
        MoveDef::builder(MoveId(2), "Jab")
            .frames(2, 1, 2)
            .priority(2)
            .damage(3)
            .build(),
    ]);
    let cfg = Config::default();
    let mut sim = Sim::new(cfg, lib.clone(), 99);
    // The player is ready and will commit the slow, unarmored wind-up.
    sim.add_actor(
        Actor {
            id: ActorId(1),
            faction: FactionId::PLAYER,
            vitals: Vitals::new(30),
            tempo: 0,
            next_ready_tick: Tick(0),
            state: ActorState::Idle,
            foresight_horizon: 0,
            pos: Pos::ORIGIN,
            evasion: 0,
        },
        vec![MoveId(1)],
    );
    // The elite is *not* ready (so it only ever dilates) and holds Tempo to spend.
    sim.add_actor(
        Actor {
            id: ActorId(2),
            faction: FactionId::ENEMY,
            vitals: Vitals::new(30),
            tempo: 12,
            next_ready_tick: Tick(100),
            state: ActorState::Idle,
            foresight_horizon: 0,
            pos: Pos::ORIGIN,
            evasion: 0,
        },
        vec![MoveId(2)],
    );
    (sim, lib, cfg)
}

#[test]
fn an_elite_interrupts_the_players_windup() {
    let (mut sim, lib, cfg) = rig();

    let mut player = ScriptedController::new();
    player.push(
        0,
        ActorId(1),
        Command::CommitAction {
            mv: MoveId(1),
            target: Some(ActorId(2)),
        },
    );
    let mut elite = EliteAi::new(lib, cfg);

    let mut interrupted = false;
    let mut guard = 0;
    loop {
        match sim.run_until_decision_or_end() {
            StepResult::Decision { decision, view } => {
                let cmd = if decision.faction == FactionId::PLAYER {
                    player.decide(&decision, &view)
                } else {
                    elite.decide(&decision, &view)
                };
                sim.submit(cmd);
            }
            StepResult::Ended(_) => break,
        }
        for e in sim.drain_events() {
            if matches!(e, Event::Interrupted { by, .. } if by == ActorId(2)) {
                interrupted = true;
            }
        }
        guard += 1;
        if interrupted || guard > 50 {
            break;
        }
    }
    assert!(
        interrupted,
        "the elite should spend Tempo to interrupt the player's unarmored wind-up"
    );
}
