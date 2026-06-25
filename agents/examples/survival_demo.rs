//! Headless look at the survival layer: thirst, warmth and stamina drain per day from the tile
//! each body stands on, blunted by Constitution and the Survive skill. NPCs don't yet seek water
//! or shelter on their own (a deferred GOAP integration), so this shows the *raw* pressure — how
//! a population fares over a season under the harsh meters — rather than a managed equilibrium.
//!
//! Run: `cargo run -p agents --example survival_demo --release`

use agents::{Setup, Simulation};

fn main() {
    let mut sim = Simulation::new(Setup {
        width: 64,
        height: 48,
        seed: 7,
        npcs: 200,
        rpg: true,
        survival: true,
        ..Default::default()
    });
    let start = sim.npc_count();
    println!("start: {start} NPCs (survival + RPG on)\n");

    for season in 1..=3 {
        sim.run(30);
        let npcs = sim.npcs();
        let n = npcs.len();
        let (mut thirst, mut warmth, mut stamina, mut nearest) = (0.0f32, 0.0, 0.0, f32::MAX);
        for &e in &npcs {
            if let Some(v) = sim.vitals_of(e) {
                // Vitals are fixed-point; this demo just aggregates them as f32 for display.
                thirst += v.thirst.to_num::<f32>();
                warmth += v.warmth.to_num::<f32>();
                stamina += v.stamina.to_num::<f32>();
                nearest = nearest.min(v.lowest_lethal().to_num::<f32>());
            }
        }
        let d = n.max(1) as f32;
        println!(
            "day {:>3}: {n:>3} alive ({:>3}% of start) — mean thirst {:>3.0}, warmth {:>3.0}, stamina {:>3.0}; nearest death {:.0}",
            season * 30,
            100 * n / start.max(1),
            thirst / d,
            warmth / d,
            stamina / d,
            if nearest == f32::MAX { 100.0 } else { nearest },
        );
    }
}
