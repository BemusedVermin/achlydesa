//! Biome-specific fauna in motion. Spawns a broad mixed population, lets it settle
//! and breed, then prints where each species ended up — the Holdridge formation its
//! herds occupy — so you can see creatures sorting themselves into the biomes they
//! thrive in (and the wastes left to the few that suit them).
//!
//! `cargo run -p agents --example fauna_demo --release`

use agents::{Setup, Simulation};
use game_sim::fields::Formation;
use std::collections::HashMap;

fn main() {
    let mut sim = Simulation::new(Setup {
        width: 96,
        height: 72,
        seed: 2026,
        warmup: 730,
        fauna: 500,
        carnivores: 140,
        ..Default::default()
    });
    println!(
        "seeded {} herbivores, {} carnivores; running 300 days…",
        sim.fauna_count(),
        sim.carnivore_count()
    );
    sim.run(300);

    let names: Vec<String> = sim
        .bestiary()
        .species
        .iter()
        .map(|s| s.name.clone())
        .collect();
    let census = sim.fauna_census();
    let gw = sim.substrate();

    // species index -> (total, formation histogram)
    let mut by_species: HashMap<usize, (usize, HashMap<Formation, usize>)> = HashMap::new();
    for (_id, si, c) in &census {
        let f = gw.formation(*c);
        let e = by_species.entry(*si).or_insert((0, HashMap::new()));
        e.0 += 1;
        *e.1.entry(f).or_default() += 1;
    }

    println!(
        "\n{} creatures alive across {} species:",
        census.len(),
        by_species.len()
    );
    println!(
        "  {:<16} {:>5}   where they live (by formation)",
        "species", "n"
    );
    let mut rows: Vec<_> = by_species.into_iter().collect();
    rows.sort_by_key(|x| std::cmp::Reverse(x.1.0));
    for (si, (total, forms)) in rows {
        let mut fs: Vec<_> = forms.into_iter().collect();
        fs.sort_by_key(|x| std::cmp::Reverse(x.1));
        let where_str: Vec<String> = fs
            .iter()
            .take(3)
            .map(|(f, n)| format!("{f:?} {}%", 100 * n / total.max(1)))
            .collect();
        println!("  {:<16} {total:>5}   {}", names[si], where_str.join(", "));
    }
}
