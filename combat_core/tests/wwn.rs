//! The optional WWN to-hit gate (`Config::wwn_checks`): a damaging move connects only if
//! `accuracy + to_hit_base - target.evasion >= 0`, and crits when the margin is wide. With the gate
//! off (the default) every in-reach hit lands, as the golden scenarios rely on.

use combat_core::*;

fn one_strike(attacker_accuracy: i32, target_evasion: i32, wwn: bool) -> Vec<Event> {
    let cfg = Config {
        wwn_checks: wwn,
        ..Config::default()
    };
    let lib = MoveLibrary::from_defs([MoveDef::builder(MoveId(1), "Jab")
        .frames(2, 1, 2)
        .reach(Fixed::from_int(4))
        .accuracy(attacker_accuracy)
        .damage(6)
        .build()]);
    let mut sim = Sim::new(cfg, lib, 1);
    let mk = |id, faction, evasion, ready| Actor {
        id: ActorId(id),
        faction: FactionId(faction),
        vitals: Vitals::new(30),
        tempo: 0,
        next_ready_tick: Tick(ready),
        state: ActorState::Idle,
        foresight_horizon: 0,
        pos: Pos::ORIGIN,
        evasion,
    };
    sim.add_actor(mk(1, 0, 0, 0), vec![MoveId(1)]);
    sim.add_actor(mk(2, 1, target_evasion, 100), vec![MoveId(1)]);

    let mut player = ScriptedController::new();
    player.push(
        0,
        ActorId(1),
        Command::CommitAction {
            mv: MoveId(1),
            target: Some(ActorId(2)),
        },
    );

    let mut guard = 0;
    while let StepResult::Decision { decision, view } = sim.run_until_decision_or_end() {
        let cmd = player.decide(&decision, &view);
        sim.submit(cmd);
        guard += 1;
        if guard > 20 {
            break;
        }
    }
    sim.drain_events()
}

fn landed(trace: &[Event]) -> Option<i32> {
    trace.iter().find_map(|e| match e {
        Event::Hit { amount, .. } => Some(*amount),
        _ => None,
    })
}

fn whiffed(trace: &[Event]) -> bool {
    trace.iter().any(|e| {
        matches!(
            e,
            Event::ActionFizzled {
                reason: FizzleReason::Missed,
                ..
            }
        )
    })
}

#[test]
fn gate_off_always_lands() {
    // Even with terrible accuracy, the gate being off means the hit lands for full.
    let trace = one_strike(-5, 12, false);
    assert_eq!(landed(&trace), Some(6));
    assert!(!whiffed(&trace));
}

#[test]
fn high_evasion_dodges() {
    // accuracy -2 + base 7 - evasion 12 = -7 < 0 → the target evades.
    let trace = one_strike(-2, 12, true);
    assert!(
        whiffed(&trace),
        "a nimble target should evade a weak attacker"
    );
    assert_eq!(landed(&trace), None);
}

#[test]
fn a_strong_hit_crits() {
    // accuracy 6 + base 7 - evasion 3 = 10 ≥ strong_margin(4) → a strong hit: 6 × 1.5 = 9.
    let trace = one_strike(6, 3, true);
    assert_eq!(
        landed(&trace),
        Some(9),
        "a wide margin should crit for 1.5×"
    );
}
