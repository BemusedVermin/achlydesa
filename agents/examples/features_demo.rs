//! A tour of a generated world: the tile features it is stocked with (settlements,
//! courts, ruins, wonders, and the hexes that stack several), then a live run that
//! reports the emergent society — who practises which trade, and how hard the
//! features' affordances get worked.
//!
//! `cargo run -p agents --example features_demo --release`

use agents::{Category, Setup, Simulation};
use std::collections::BTreeMap;

fn main() {
    let mut sim = Simulation::new(Setup {
        width: 64,
        height: 48,
        seed: 2026,
        warmup: 300,
        npcs: 80,
        ..Default::default()
    });
    let topo = sim.substrate().topology().clone();

    {
        let cat = sim.feature_catalog();
        let feats = sim.features();
        println!(
            "World 64×48, seed 2026 — {} features placed\n",
            feats.total()
        );

        // Per-category and per-kind tallies.
        for category in Category::ALL {
            let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
            for (_, f) in feats.iter() {
                if cat.def(f.kind).category == category {
                    *kinds.entry(cat.name(f.kind)).or_default() += 1;
                }
            }
            let total: usize = kinds.values().sum();
            println!("{category:?} ({total}):");
            for (name, n) in &kinds {
                println!("    {name:<16} {n}");
            }
        }

        // Hexes that layer two or more categories — "multiple landmarks per hex".
        let mut stacked: Vec<(usize, Vec<&str>)> = Vec::new();
        for i in topo.indices() {
            let here = feats.at_index(i);
            let cats: std::collections::HashSet<Category> =
                here.iter().map(|f| cat.def(f.kind).category).collect();
            if cats.len() >= 2 {
                stacked.push((i, here.iter().map(|f| cat.name(f.kind)).collect()));
            }
        }
        println!(
            "\n{} hexes stack two or more categories. A few:",
            stacked.len()
        );
        for (i, names) in stacked.iter().take(8) {
            let c = topo.coord(*i);
            println!("    ({:>2},{:>2}): {}", c.col, c.row, names.join(" + "));
        }
    }

    // Let the society run, then read it off the observer.
    sim.run(400);
    let skill_names: Vec<String> = (0..sim.registry().skill_count())
        .map(|s| sim.registry().skill(s).name.clone())
        .collect();
    let c = sim.census();
    println!("\n--- after 400 days ({} NPCs born) ---", 80);
    println!(
        "population {}, coins {}, goods {}",
        c.population, c.money, c.goods
    );
    println!("emergent professions (born to a calling, deepened by doing):");
    for (id, &n) in c.professions.iter().enumerate() {
        println!(
            "    {:<10} {n} practising",
            skill_names.get(id).map(String::as_str).unwrap_or("?")
        );
    }
    println!(
        "feature affordances: {} sites, {} uses so far, {} worked out right now",
        c.affordance_sites, c.affordance_uses, c.worked_out_sites
    );
}
