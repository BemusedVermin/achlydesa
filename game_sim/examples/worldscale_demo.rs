//! Headless sanity check for the large, US-scale app world. Generates the world the way
//! `app::build_world` does (large dimensions, few plates, wider uplift falloff), then warms
//! the climate the way the app does and prints the shape of the result — land fraction,
//! elevation range, continents, and how the biome mix matures over the warmup — so the
//! generation can be tuned without launching the heavy 3-D front-end.
//!
//! Run: `cargo run -p game_sim --example worldscale_demo --release`

use sim::Substrate;
use std::collections::{HashMap, VecDeque};

fn biome_mix(world: &game_sim::World) -> Vec<(String, usize)> {
    let topo = world.topology();
    let mut m: HashMap<String, usize> = HashMap::new();
    for i in 0..topo.len() {
        *m.entry(world.biome(topo.coord(i)).name().to_string())
            .or_default() += 1;
    }
    let mut v: Vec<_> = m.into_iter().collect();
    v.sort_unstable_by_key(|x| std::cmp::Reverse(x.1));
    v
}

fn is_green(name: &str) -> bool {
    ["forest", "rain", "moist", "wet", "grass", "steppe"]
        .iter()
        .any(|k| name.contains(k))
}

fn print_mix(label: &str, mix: &[(String, usize)], n: usize) {
    let green: usize = mix
        .iter()
        .filter(|(k, _)| is_green(k))
        .map(|(_, c)| *c)
        .sum();
    let land: usize = mix
        .iter()
        .filter(|(k, _)| k.as_str() != "open water")
        .map(|(_, c)| *c)
        .sum();
    let green_of_land = if land == 0 {
        0.0
    } else {
        100.0 * green as f32 / land as f32
    };
    println!(
        "{label}: {} biome types, {green_of_land:.1}% of land is 'green' (forest/grass/wet)",
        mix.len()
    );
    for (name, c) in mix.iter().take(10) {
        println!(
            "    {name:<24} {c:>6}  {:>4.1}%",
            100.0 * *c as f32 / n as f32
        );
    }
}

fn main() {
    // Mirror app::build_world's world knobs.
    let (width, height, seed) = (192, 144, 7);
    let mut params = config::tunables::params();
    params.plates = 5;
    params.uplift_falloff = 16.0;
    let plates = params.plates;
    let sea = params.sea_level;

    let mut world = game_sim::World::generate(width, height, params, seed);
    let n = world.topology().len();

    // Land fraction, elevation range, continents — all fixed at world-gen (elevation never
    // changes), so read them once before warming the climate. Scoped so the immutable
    // borrow of `world` ends before the evolve loop.
    {
        let topo = world.topology();
        let is_land = |i: usize| world.elevation(topo.coord(i)) >= sea;
        let mut land = 0usize;
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for i in 0..n {
            let e = world.elevation(topo.coord(i));
            lo = lo.min(e);
            hi = hi.max(e);
            if e >= sea {
                land += 1;
            }
        }
        let mut seen = vec![false; n];
        let mut sizes: Vec<usize> = Vec::new();
        for s in 0..n {
            if seen[s] || !is_land(s) {
                continue;
            }
            let mut size = 0usize;
            let mut q = VecDeque::from([s]);
            seen[s] = true;
            while let Some(u) = q.pop_front() {
                size += 1;
                for l in topo.neighbors(u) {
                    if !seen[l.to] && is_land(l.to) {
                        seen[l.to] = true;
                        q.push_back(l.to);
                    }
                }
            }
            sizes.push(size);
        }
        sizes.sort_unstable_by(|a, b| b.cmp(a));
        let big = sizes.iter().filter(|&&s| s >= 50).count();
        println!("world {width}×{height} = {n} cells, seed {seed}, {plates} plates");
        println!(
            "land: {land} cells ({:.1}%)   elevation: {lo:.0}..{hi:.0} m",
            100.0 * land as f32 / n as f32
        );
        println!(
            "continents: {} land masses, {big} of them ≥50 tiles; largest few: {:?}\n",
            sizes.len(),
            sizes.iter().take(6).collect::<Vec<_>>()
        );
    }

    // Warm the climate exactly as `Simulation::new` does (same dedicated RNG stream), and
    // watch the biome mix mature. One year is 365 days; biomes are classified from a running
    // annual average, so the mix is only meaningful once the climate has spun up.
    let mut rng = game_sim::SplitMix64::new(seed ^ 0x9E37_79B9_7F4A_7C15);
    let mut t = 0u32;
    for cp in [120u32, 365, 730, 1095] {
        while t < cp {
            world.evolve(&mut rng);
            t += 1;
        }
        print_mix(&format!("after {cp} warmup days"), &biome_mix(&world), n);
        println!();
    }
}
