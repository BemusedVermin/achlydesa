//! The **player avatar exploring** — an ordinary body walking the world, lifting the fog
//! of war and finding what it passes (`docs/dialogue.md` lineage: the player is in the
//! sim, not above it).
//!
//! It spawns an avatar where the people live, looks around, then walks across the map —
//! auto-routing over land (never the sea) — revealing tiles and discovering hidden
//! features as it goes. Movement is time passing: the avatar advances one hex per tick
//! while the rest of the world lives on around it.
//!
//! `cargo run -p agents --example explore_demo --release`

use agents::{Coord, Goals, Registry, Setup, Simulation};

fn world() -> Simulation {
    let reg = Registry::bundled();
    let goals = Goals::from_ron(
        r#"[
            (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
            (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
            (name: "stocked",   condition: Holding(good: Edible, at_least: 12), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))])
        ]"#,
        &reg,
    )
    .unwrap();
    Simulation::new(Setup {
        width: 48,
        height: 36,
        seed: 7,
        warmup: 150,
        npcs: 50,
        markets: 6,
        markets_on_settlements: true,
        goals,
        registry: reg,
        ..Default::default()
    })
}

fn describe(sim: &mut Simulation) {
    let Some(v) = sim.player_view() else { return };
    let feats = if v.here.features.is_empty() {
        String::new()
    } else {
        format!(" — {}", v.here.features.join(", "))
    };
    println!(
        "  at ({:>2},{:>2})  {:<8} elev {:>5.0}m  green {:.2}  {} souls in sight  {} tiles seen{}",
        v.pos.col,
        v.pos.row,
        v.here.terrain.name(),
        v.here.elevation,
        v.here.fertility,
        v.nearby.len(),
        v.explored,
        feats,
    );
}

/// Find a far, *reachable* land tile to head for — try the far side of the map, then walk
/// outward until something on foot answers.
fn far_target(sim: &mut Simulation, from: Coord) -> Option<Coord> {
    for d in 0..20 {
        for &(dc, dr) in &[
            (20 + d, 0),
            (20 + d, 6),
            (-(20 + d), -4),
            (0, 14 + d),
            (16 + d, -10),
        ] {
            let c = Coord::new(from.col + dc, (from.row + dr).clamp(0, 35));
            if sim.player_travel_to(c) {
                return Some(c);
            }
        }
    }
    None
}

fn main() {
    let mut sim = world();

    let _avatar = sim.spawn_player(None);
    let start = sim.player_position().unwrap();
    println!("The avatar opens its eyes:\n");
    describe(&mut sim);

    let Some(target) = far_target(&mut sim, start) else {
        println!("\n  (no reachable far shore from here — an island start)");
        return;
    };
    println!(
        "\nIt sets out for ({},{}), across the land:\n",
        target.col, target.row
    );

    let mut day = 0;
    while sim.player_traveling() && day < 400 {
        sim.step();
        day += 1;
        if day % 6 == 0 {
            describe(&mut sim);
        }
    }

    println!("\nArrived after {day} days. What it has come to know:\n");
    describe(&mut sim);
    if let Some(v) = sim.player_view() {
        let seen_feats: Vec<&str> = v
            .surroundings
            .iter()
            .flat_map(|t| t.features.iter().map(String::as_str))
            .collect();
        println!(
            "\n  It has lifted the fog from {} of the world's tiles. In view now: {} other bodies{}.",
            sim.player_explored_count(),
            v.nearby.len(),
            if seen_feats.is_empty() {
                String::new()
            } else {
                format!(", and these places: {}", seen_feats.join(", "))
            },
        );
    }
}
