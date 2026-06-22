//! The world's biomes: how the Holdridge life-zone classifier paints a warmed-up
//! world. Prints the histogram (by structural formation and by named life zone)
//! alongside the climate ranges that drive it, so the precipitation / PET
//! calibration can be read off and tuned, and the world's health checked.
//!
//! `cargo run -p agents --example biome_demo --release`

use agents::{Setup, Simulation};
use game_sim::fields::{Biome, Formation};
use std::collections::HashMap;

const W: i32 = 96;
const H: i32 = 72;

fn main() {
    // Two years of warm-up so the annual climate averages are fully settled.
    let mut sim = Simulation::new(Setup {
        width: W,
        height: H,
        seed: 2026,
        warmup: 730,
        ..Default::default()
    });
    let gw = sim.substrate();
    let topo = gw.topology();
    let sea = gw.params().sea_level;

    let mut by_formation: HashMap<Formation, usize> = HashMap::new();
    let mut by_biome: HashMap<Biome, usize> = HashMap::new();
    let (mut land, mut biomass) = (0usize, 0.0f32);
    let (mut bt_min, mut bt_max, mut ap_min, mut ap_max) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);

    for i in topo.indices() {
        let c = topo.coord(i);
        let b = gw.biome(c);
        *by_biome.entry(b).or_default() += 1;
        *by_formation.entry(b.formation()).or_default() += 1;
        if gw.elevation(c) >= sea {
            land += 1;
            biomass += gw.plant_biomass(c);
            let (bt, ap) = (gw.biotemperature(c), gw.annual_precipitation(c));
            bt_min = bt_min.min(bt);
            bt_max = bt_max.max(bt);
            ap_min = ap_min.min(ap);
            ap_max = ap_max.max(ap);
        }
    }
    let total = topo.len();

    println!(
        "world {W}x{H}  ({total} tiles, {land} land = {:.0}%)",
        100.0 * land as f32 / total as f32
    );
    println!("biotemperature  {bt_min:.1}..{bt_max:.1} °C");
    println!("annual precip   {ap_min:.0}..{ap_max:.0} (model units)");
    println!(
        "mean land biomass {:.3} (of biomass_max {:.0})",
        biomass / land.max(1) as f32,
        gw.params().biomass_max
    );

    println!("\nby formation (share of all tiles):");
    let mut fs: Vec<_> = by_formation.into_iter().collect();
    fs.sort_by(|a, b| b.1.cmp(&a.1));
    for (f, n) in fs {
        println!(
            "  {f:>11?}  {n:>5}  {:>5.1}%",
            100.0 * n as f32 / total as f32
        );
    }

    println!("\nlife zones realised (of 39 possible):");
    let mut bs: Vec<_> = by_biome.into_iter().collect();
    bs.sort_by(|a, b| b.1.cmp(&a.1));
    for (b, n) in &bs {
        println!(
            "  {:>28}  {n:>5}  {:>5.1}%",
            b.name(),
            100.0 * *n as f32 / total as f32
        );
    }
    println!("  {} distinct biomes present", bs.len());
}
