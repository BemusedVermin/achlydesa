//! The trophic loop in motion: vegetation feeds herbivores, herbivores feed
//! carnivores. Prints the two populations over time, with predators and without, so
//! you can see the top-down control (and the predator–prey swings) the Holling
//! type-II predation produces.
//!
//! `cargo run -p agents --example ecology_demo --release`

use agents::{Setup, Simulation};

fn trajectory(fauna: usize, carnivores: usize) {
    let mut sim = Simulation::new(Setup {
        width: 48,
        height: 36,
        seed: 11,
        warmup: 300,
        fauna,
        carnivores,
        ..Default::default()
    });
    println!("\n{fauna} herbivores, {carnivores} carnivores:");
    println!("  day   herbivores  carnivores");
    for day in (0..=900).step_by(90) {
        if day > 0 {
            sim.run(90);
        }
        println!(
            "  {day:>4}   {:>9}   {:>9}",
            sim.fauna_count(),
            sim.carnivore_count()
        );
    }
}

fn main() {
    trajectory(60, 0); // herbivores alone — bottom-up regulation only
    trajectory(60, 8); // with predators — the loop closes: both tiers persist in a
    // sustained oscillation (Liebig productivity + herd aggregation +
    // a patient pack + a spatial refuge → genuine coexistence).
}
