//! A headless **scaling bench**: it grows the agent population `N` and reports the
//! per-tick cost, so the estimates in `docs/scaling.md` become measurements.
//!
//! Two things it makes visible:
//!  * **How cost scales with N.** A near-constant µs/agent column means linear scaling; a
//!    rising one means a super-linear term is biting (the classic culprit is `converse`'s
//!    co-location scan — run this before and after the tile-bucket fix to see it flatten).
//!  * **Where the tick goes.** There is no cheap way to time a single private system
//!    headlessly, so instead we lean on the off-by-default architecture: time a bare
//!    economy, then the same run with `dialogue`, then with the `director` too. The deltas
//!    *attribute* the cost to each layer. Planning/execution/factions are always-on and
//!    show up as the economy baseline (planning dominates it — that is the whole reason
//!    Track 2 exists).
//!
//! A fingerprint per config is printed too, so a determinism regression is obvious at a
//! glance (the same N+layers must print the same fingerprint every run).
//!
//! Run with: `cargo run --release --example bench_scaling [ticks] [n1,n2,...]`
//!   e.g.     `cargo run --release --example bench_scaling 200 250,1000,4000`

use agents::{DirectorConfig, Setup, Simulation};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Allocation-counting wrapper, so we can report allocs/tick (the memory-wall proxy the
/// scaling doc cares about — ~8 live allocs per agent today).
struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

/// The optional layers a config wakes — the attribution knobs.
#[derive(Clone, Copy)]
enum Layers {
    /// Bare economy: planning, trade, factions, metabolism. Planning dominates.
    Economy,
    /// + emergent dialogue (`converse`).
    Dialogue,
    /// + the narrative director and a handful of feuds (grievance planning + casting).
    Director,
}

impl Layers {
    fn label(self) -> &'static str {
        match self {
            Layers::Economy => "economy",
            Layers::Dialogue => "+dialogue",
            Layers::Director => "+director",
        }
    }
}

/// Build a run with `npcs` people on a fixed, roomy world (so the population — not the
/// map — is what grows), waking `layers`.
fn build(npcs: usize, layers: Layers) -> Simulation {
    // A roomy world kept fixed across N: 96x72 = 6912 tiles. Markets scale gently with the
    // population so trade stays reachable as the crowd grows.
    let mut setup = Setup {
        width: 96,
        height: 72,
        seed: 7,
        warmup: 80,
        npcs,
        markets: (npcs / 200).max(3),
        feuds: 0,
        ..Default::default()
    };
    match layers {
        Layers::Economy => {}
        Layers::Dialogue => setup.dialogue = true,
        Layers::Director => {
            setup.dialogue = true;
            setup.director = true;
            setup.feuds = (npcs / 20).max(2);
            setup.director_cfg = DirectorConfig {
                beat_interval: 7,
                ..Default::default()
            };
        }
    }
    Simulation::new(setup)
}

/// Time `ticks` steps of a freshly built run, returning (wall, allocs, surviving npcs,
/// fingerprint). Worldgen + warm-up + one priming tick are excluded from the timing.
fn measure(npcs: usize, layers: Layers, ticks: u64) -> (Duration, usize, usize, u64) {
    let mut sim = build(npcs, layers);
    sim.run(1); // prime: first-touch allocations (pools, lazy statics) out of the window

    let allocs_before = ALLOCS.load(Ordering::Relaxed);
    let t = Instant::now();
    sim.run(ticks);
    let wall = t.elapsed();
    let allocs = ALLOCS.load(Ordering::Relaxed) - allocs_before;
    (wall, allocs, sim.npc_count(), sim.fingerprint())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let ticks: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(200);
    let ns: Vec<usize> = args
        .next()
        .map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect())
        .unwrap_or_else(|| vec![250, 1000, 4000]);

    println!("scaling bench — 96x72 world, {ticks} ticks/config\n");
    for &n in &ns {
        println!("N = {n}");
        println!(
            "  {:<10} {:>9} {:>11} {:>11} {:>9}  {:<18}",
            "layers", "ticks/s", "us/tick", "us/agent", "alloc/tk", "fingerprint"
        );
        let mut prev: Option<Duration> = None;
        for layers in [Layers::Economy, Layers::Dialogue, Layers::Director] {
            let (wall, allocs, alive, fp) = measure(n, layers, ticks);
            let per_tick = wall.as_secs_f64() / ticks as f64;
            let tps = 1.0 / per_tick;
            let us_tick = per_tick * 1e6;
            let us_agent = us_tick / alive.max(1) as f64;
            let delta = prev
                .map(|p| {
                    let d = (wall.as_secs_f64() - p.as_secs_f64()) * 1e6 / ticks as f64;
                    format!("  ({d:+.0}us/tick vs prev)")
                })
                .unwrap_or_default();
            println!(
                "  {:<10} {:>9.0} {:>11.0} {:>11.2} {:>9} 0x{:016X}{}",
                layers.label(),
                tps,
                us_tick,
                us_agent,
                allocs as u64 / ticks,
                fp,
                delta,
            );
            prev = Some(wall);
        }
        println!();
    }
}
