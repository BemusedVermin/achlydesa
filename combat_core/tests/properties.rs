//! Property tests (spec §17.4). Random `StubAi`-vs-`StubAi` fights must always uphold the core
//! invariants: Tempo never goes negative, total HP is monotonic non-increasing, no action
//! activates before it was scheduled, and the sim always terminates within a bounded tick count.

use combat_core::*;
use proptest::prelude::*;

/// A fixed pool of three moves. Every actor's kit always includes move 0 (a fast, always-usable
/// jab) so a fight can never deadlock for lack of a legal action.
fn move_pool() -> MoveLibrary {
    MoveLibrary::from_defs([
        MoveDef::builder(MoveId(0), "Jab")
            .frames(2, 1, 1)
            .priority(2)
            .damage(2)
            .build(),
        MoveDef::builder(MoveId(1), "Strike")
            .frames(3, 1, 2)
            .priority(1)
            .damage(4)
            .build(),
        MoveDef::builder(MoveId(2), "Heavy")
            .frames(5, 1, 3)
            .priority(1)
            .armored()
            .damage(7)
            .build(),
    ])
}

fn actor(id: u32, faction: u32, hp: i32) -> Actor {
    Actor {
        id: ActorId(id),
        faction: FactionId(faction),
        vitals: Vitals::new(hp),
        tempo: 0,
        next_ready_tick: Tick(0),
        state: ActorState::Idle,
        foresight_horizon: 0,
        pos: Pos::ORIGIN,
    }
}

fn kit_for(bits: u8) -> Vec<MoveId> {
    let mut kit = vec![MoveId(0)];
    if bits & 1 != 0 {
        kit.push(MoveId(1));
    }
    if bits & 2 != 0 {
        kit.push(MoveId(2));
    }
    kit
}

fn build_fight(seed: u64, p_hps: &[i32], e_hps: &[i32], kit_bits: &[u8]) -> Sim {
    let mut sim = Sim::new(Config::default(), move_pool(), seed);
    let mut id = 1u32;
    let mut bit = 0usize;
    for &hp in p_hps {
        sim.add_actor(actor(id, 0, hp), kit_for(kit_bits[bit % kit_bits.len()]));
        id += 1;
        bit += 1;
    }
    for &hp in e_hps {
        sim.add_actor(actor(id, 1, hp), kit_for(kit_bits[bit % kit_bits.len()]));
        id += 1;
        bit += 1;
    }
    sim
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn random_fights_uphold_invariants(
        seed in any::<u64>(),
        p_hps in prop::collection::vec(4i32..24, 1..=3),
        e_hps in prop::collection::vec(4i32..24, 1..=3),
        kit_bits in prop::collection::vec(any::<u8>(), 1..=6),
    ) {
        let mut sim = build_fight(seed, &p_hps, &e_hps, &kit_bits);
        let mut ai = StubAi::new(sim.library().clone());

        let mut total_prev = i64::MAX;
        let mut iters = 0u64;
        loop {
            iters += 1;
            prop_assert!(iters < 60_000, "fight failed to terminate");
            match sim.run_until_decision_or_end() {
                StepResult::Decision { decision, view } => {
                    // Tempo never negative.
                    for a in sim.actors() {
                        prop_assert!(a.tempo >= 0, "negative Tempo on actor {:?}", a.id);
                    }
                    // Total HP monotonic non-increasing (no healing in v1).
                    let total: i64 = sim.actors().map(|a| a.vitals.hp as i64).sum();
                    prop_assert!(total <= total_prev, "total HP increased");
                    total_prev = total;
                    let cmd = ai.decide(&decision, &view);
                    sim.submit(cmd);
                }
                StepResult::Ended(_) => break,
            }
        }

        // No action ever activates before the tick it was scheduled at.
        let events = sim.drain_events();
        let mut scheduled: std::collections::BTreeMap<u32, u64> = Default::default();
        for ev in &events {
            match ev {
                Event::ActionScheduled { instance, start, .. } => {
                    scheduled.insert(instance.0, start.0);
                }
                Event::ActionActive { instance, tick } => {
                    if let Some(&start) = scheduled.get(&instance.0) {
                        prop_assert!(tick.0 >= start, "instance {} activated before it was scheduled", instance.0);
                    }
                }
                _ => {}
            }
        }
    }
}
