//! Diagnostic: does the director actually stage *encounterable* drama in the **app's** world config?
//! Replicates `app::build_world` (US-scale 192x144, 300 NPCs, director on, LOD r26) headlessly,
//! spawns the avatar where the app does, and reports — over a play-length run — how many beats fire,
//! how many souls are in talking range of the avatar, and what the avatar would sense/overhear. This
//! is the ground truth for the "it feels the same" report: if beats are scarce or no one is near, the
//! surfacing has nothing to show.
//!
//!   cargo run -p agents --example app_drama_probe --release

use agents::{Goals, Registry, Setup, Simulation};

fn main() {
    let reg = Registry::bundled();
    let goals = Goals::from_ron(
        r#"[
            (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
            (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
            (name: "stocked",   condition: Holding(good: Edible, at_least: 12), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
            (name: "solvent",   condition: Money(at_least: 200),      appeal: [(input: Deficit, curve: Linear(m: 0.5, b: 0.0))])
        ]"#,
        &reg,
    )
    .unwrap();
    let mut params = config::tunables::params();
    params.plates = 5;
    params.uplift_falloff = 16.0;
    println!("generating the US-scale world (this is the slow part)…");
    let world = game_sim::World::generate(192, 144, params, 7);

    let mut sim = Simulation::from_world(
        world,
        Setup {
            seed: 7,
            warmup: 400,
            // Fauna trimmed (they don't touch the director) to keep the probe quick.
            fauna: 100,
            carnivores: 20,
            npcs: 300,
            markets: 12,
            markets_on_settlements: true,
            dialogue: true,
            director: true,
            rpg: true,
            party: true,
            exploration: true,
            survival: true,
            survival_everyone: false,
            sim_radius: Some(26),
            sim_far_stride: 12,
            goals,
            registry: reg,
            ..Default::default()
        },
    );
    sim.spawn_player(None);
    let apos = sim.player_position().unwrap();
    println!("avatar spawned at ({},{}); {} NPCs in the world.", apos.col, apos.row, sim.npc_count());
    println!("NPCs within the avatar's sight at spawn: {}", sim.player_nearby_npcs().len());
    println!("\nNow simulating a player who *follows the unrest* (travels toward the strongest mark):");
    println!("  tick | beats | nearby | tidings (what the HUD banner would say)");
    println!("  -----+-------+--------+----------------------------------------");
    for i in 1..=800u32 {
        sim.step();
        // Chase the drama like a player reading the crimson pips: head for the strongest mark.
        if !sim.player_traveling()
            && let Some((place, _)) = sim.drama_marks().into_iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        {
            sim.player_travel_to(place);
        }
        if i % 50 == 0 {
            let nearby = sim.player_nearby_npcs().len();
            let tid = sim.tidings().unwrap_or_else(|| "(the land is quiet)".into());
            println!("  {:>4} | {:>5} | {:>6} | {}", sim.substrate().tick(), sim.director_beats_fired(), nearby, tid);
        }
    }
    // A peek at who is around the avatar at the end — the souls it could actually talk to.
    println!("\nSouls on the avatar's tile at the end:");
    if let Some(p) = sim.player_position() {
        for line in sim.souls_at(p) {
            println!("  · {line}");
        }
    }
}
