//! Headless look at the RPG layer (Worlds Without Number). Builds a peopled world with the
//! RPG layer awake and prints the rolled population — attribute spread, archetype (Edge) mix,
//! and how many took up each social / world-interaction skill — plus the avatar's sheet.
//!
//! Run: `cargo run -p agents --example rpg_demo --release`

use agents::{Setup, Simulation};
use std::collections::HashMap;

fn main() {
    let mut sim = Simulation::new(Setup {
        width: 48,
        height: 36,
        seed: 7,
        npcs: 80,
        rpg: true,
        ..Default::default()
    });

    let npcs = sim.npcs();
    println!("{} NPCs rolled with WWN stats\n", npcs.len());

    // Attribute spread.
    let labels = ["Str", "Dex", "Con", "Int", "Wis", "Cha"];
    let mut sums = [0i64; 6];
    for &e in &npcs {
        if let Some(a) = sim.abilities_of(e) {
            for (i, s) in sums.iter_mut().enumerate() {
                *s += a.scores[i] as i64;
            }
        }
    }
    let n = npcs.len().max(1) as f64;
    print!("mean attributes:  ");
    for i in 0..6 {
        print!("{} {:.1}   ", labels[i], sums[i] as f64 / n);
    }
    println!("\n");

    // Archetype (Edge) mix.
    let mut edges: HashMap<String, usize> = HashMap::new();
    for &e in &npcs {
        *edges
            .entry(sim.archetype_of(e).unwrap_or("(none)").to_string())
            .or_default() += 1;
    }
    let mut mix: Vec<_> = edges.into_iter().collect();
    mix.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("archetypes:");
    for (name, c) in &mix {
        println!("  {name:<14} {c}");
    }
    println!();

    // Skill uptake (trained = rank above unskilled), split into the prioritized classes.
    let (social, world): (Vec<String>, Vec<String>) = {
        let data = sim.rpg_data().unwrap();
        (
            data.skills()
                .iter()
                .filter(|s| s.social)
                .map(|s| s.name.clone())
                .collect(),
            data.skills()
                .iter()
                .filter(|s| s.world)
                .map(|s| s.name.clone())
                .collect(),
        )
    };
    let trained = |skill: &str, sim: &Simulation| {
        npcs.iter()
            .filter(|&&e| sim.proficiency_of(e, skill).is_some_and(|r| r > -1))
            .count()
    };
    println!("social skills — trained NPCs:");
    for s in &social {
        println!("  {s:<12} {}", trained(s, &sim));
    }
    println!("\nworld-interaction skills — trained NPCs:");
    for s in &world {
        println!("  {s:<12} {}", trained(s, &sim));
    }

    // The avatar gets capabilities too (rolled from its own sub-stream).
    let avatar = sim.spawn_player(None);
    println!(
        "\navatar archetype: {}",
        sim.archetype_of(avatar).unwrap_or("(none)")
    );
    if let Some(a) = sim.abilities_of(avatar) {
        print!("avatar attributes: ");
        for (i, label) in labels.iter().enumerate() {
            print!("{} {}   ", label, a.scores[i]);
        }
        println!();
    }
}
