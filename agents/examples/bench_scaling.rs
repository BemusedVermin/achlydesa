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
//!  * **The Tier-1 collapse (Track 2).** A final `fields(T1)` row runs the same economy with
//!    the stigmergic-fields layer and a tight LOD radius around a spawned avatar, so the distant
//!    majority runs the O(1) gradient brain instead of GOAP A*. Its annotation is the µs/agent
//!    ratio vs the all-full-brain `economy` baseline — the complexity-class drop, not a constant
//!    factor, which is the whole reason Track 2 exists.
//!
//!  * **Millions, as cohorts (Track 2 / 2a+2c).** A final `Tier-2 cohorts` section runs the whole
//!    populace as statistical regional cohorts (no individual NPCs, just a bounded crystallized
//!    cast) and sweeps the stated `souls` up by orders of magnitude. The per-tick wall stays ~flat
//!    and `ns/soul` collapses toward zero, because the cohort economy is `O(regions)`, independent
//!    of headcount — the only thing that reaches millions.
//!
//! A fingerprint per config is printed too, so a determinism regression is obvious at a
//! glance (the same N+layers must print the same fingerprint every run).
//!
//! Run with: `cargo run --release --example bench_scaling [ticks] [n1,n2,...] [WxH]`
//!   e.g.     `cargo run --release --example bench_scaling 200 250,1000,4000`
//!   crowded: `cargo run --release --example bench_scaling 100 500,2000 24x18`
//!            (a small world packs the souls so `converse`'s co-location cost shows up —
//!            with the tile-bucket the `+dialogue` per-agent delta stays flat as N grows,
//!            i.e. linear, not the N^2 a full scan would give).

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

/// Radius (hexes from the avatar) kept at full GOAP detail in the Tier-1 config — small, so the
/// distant majority falls into the cheap gradient brain and the cost collapse is visible.
const FIELDS_RADIUS: i32 = 8;

/// The optional layers a config wakes — the attribution knobs.
#[derive(Clone, Copy, PartialEq)]
enum Layers {
    /// Bare economy: planning, trade, factions, metabolism. Planning dominates.
    Economy,
    /// + emergent dialogue (`converse`).
    Dialogue,
    /// + the narrative director and a handful of feuds (grievance planning + casting).
    Director,
    /// **Track 2.** Economy + the stigmergic-fields layer with an avatar and a tight
    /// [`FIELDS_RADIUS`]: the local cast stays full-brain (Tier 0) while everyone beyond runs the
    /// O(1) gradient brain (Tier 1). The point of this row is the µs/agent vs the `economy`
    /// baseline — that delta is the complexity-class drop GOAP can't get from `par_iter`.
    Fields,
}

impl Layers {
    fn label(self) -> &'static str {
        match self {
            Layers::Economy => "economy",
            Layers::Dialogue => "+dialogue",
            Layers::Director => "+director",
            Layers::Fields => "fields(T1)",
        }
    }
}

/// The A* planning budget from the `BUDGET` env var (`None` = the default 600-node search).
fn budget_env() -> Option<usize> {
    std::env::var("BUDGET").ok().and_then(|s| s.parse().ok())
}

/// Build a run with `npcs` people on a `width`x`height` world, waking `layers`.
fn build(width: i32, height: i32, npcs: usize, layers: Layers) -> Simulation {
    // Markets scale gently with the population so trade stays reachable as the crowd grows.
    let mut setup = Setup {
        width,
        height,
        seed: 7,
        warmup: 80,
        npcs,
        markets: (npcs / 200).max(3),
        feuds: 0,
        // The A* planning budget lever: `BUDGET=n` lowers it (cheaper, shorter-horizon
        // plans); unset is the default 600-node search.
        plan_budget: budget_env(),
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
        // Tier-1: wake the fields layer and a tight LOD radius. The avatar (spawned in `measure`)
        // is what `lod_dormancy` measures distance from, so it must exist for anyone to be demoted.
        Layers::Fields => {
            setup.fields = true;
            setup.sim_radius = Some(FIELDS_RADIUS);
        }
    }
    Simulation::new(setup)
}

/// Time `ticks` steps of a freshly built run, returning (wall, allocs, surviving npcs,
/// fingerprint). Worldgen + warm-up + one priming tick are excluded from the timing.
fn measure(
    width: i32,
    height: i32,
    npcs: usize,
    layers: Layers,
    ticks: u64,
) -> (Duration, usize, usize, u64) {
    let mut sim = build(width, height, npcs, layers);
    // The Tier-1 config needs an avatar for the LOD to demote anyone (it measures distance from
    // the avatar). Spawn it before timing so the steady state — local cast Tier 0, the rest Tier 1.
    if layers == Layers::Fields {
        sim.spawn_player(None);
    }
    sim.run(1); // prime: first-touch allocations (pools, lazy statics) out of the window

    let allocs_before = ALLOCS.load(Ordering::Relaxed);
    let t = Instant::now();
    sim.run(ticks);
    let wall = t.elapsed();
    let allocs = ALLOCS.load(Ordering::Relaxed) - allocs_before;
    (wall, allocs, sim.npc_count(), sim.fingerprint())
}

/// Regions (markets) the cohort sweep spreads its population across.
const COHORT_REGIONS: usize = 12;

/// Time `ticks` steps of a **Tier-2 cohort** world holding `cohort_pop` souls across
/// [`COHORT_REGIONS`] regions, with an avatar present to crystallize a cast. Returns (wall, souls
/// alive after the run, live entities, fingerprint). The whole populace is cohorts (no individual
/// NPCs), so the tick is the regional economy + the bounded crystallized cast — `O(regions)`,
/// independent of `cohort_pop`.
fn measure_cohorts(
    width: i32,
    height: i32,
    cohort_pop: u64,
    ticks: u64,
) -> (Duration, u64, usize, u64) {
    let mut sim = Simulation::new(Setup {
        width,
        height,
        seed: 7,
        warmup: 80,
        npcs: 0,
        markets: COHORT_REGIONS,
        cohorts: true,
        cohort_pop,
        cohort_pool_each: 5_000_000,
        // A wide promote radius so the stationary avatar is sure to land near a seat and
        // crystallize a cast — the `live` column then shows the bounded entity count alongside the
        // millions of cohort souls.
        cohort_cfg: agents::CohortConfig {
            promote_radius: 24,
            ..Default::default()
        },
        ..Default::default()
    });
    sim.spawn_player(None);
    sim.run(1); // prime
    let t = Instant::now();
    sim.run(ticks);
    let wall = t.elapsed();
    let souls = sim.cohort_population() + sim.npc_count() as u64;
    (wall, souls, sim.npc_count(), sim.fingerprint())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let ticks: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(200);
    let ns: Vec<usize> = args
        .next()
        .map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect())
        .unwrap_or_else(|| vec![250, 1000, 4000]);
    let (width, height) = args
        .next()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((96, 72));

    let budget = budget_env().map_or("600 (default)".to_string(), |b| b.to_string());
    println!(
        "scaling bench — {width}x{height} world, {ticks} ticks/config, plan budget {budget}\n"
    );
    for &n in &ns {
        println!("N = {n}");
        println!(
            "  {:<10} {:>9} {:>11} {:>11} {:>9}  {:<18}",
            "layers", "ticks/s", "us/tick", "us/agent", "alloc/tk", "fingerprint"
        );
        let mut prev: Option<Duration> = None;
        let mut econ: Option<(Duration, usize)> = None;
        // The attribution chain (each row adds a layer), then the Tier-1 row on its own — it is
        // economy + fields, *not* + director, so it is annotated against the economy baseline.
        for layers in [
            Layers::Economy,
            Layers::Dialogue,
            Layers::Director,
            Layers::Fields,
        ] {
            let (wall, allocs, alive, fp) = measure(width, height, n, layers, ticks);
            let per_tick = wall.as_secs_f64() / ticks as f64;
            let tps = 1.0 / per_tick;
            let us_tick = per_tick * 1e6;
            let us_agent = us_tick / alive.max(1) as f64;
            // Most rows compare to the previous row (the layer they add); the Tier-1 row compares
            // its *per-agent* cost to the all-full-brain economy — that ratio is the headline.
            let delta = if layers == Layers::Fields {
                econ.map(|(ew, ea)| {
                    let base = ew.as_secs_f64() * 1e6 / ticks as f64 / ea.max(1) as f64;
                    format!("  ({:.1}x us/agent vs economy)", base / us_agent.max(1e-9))
                })
                .unwrap_or_default()
            } else {
                prev.map(|p| {
                    let d = (wall.as_secs_f64() - p.as_secs_f64()) * 1e6 / ticks as f64;
                    format!("  ({d:+.0}us/tick vs prev)")
                })
                .unwrap_or_default()
            };
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
            if layers == Layers::Economy {
                econ = Some((wall, alive));
            }
            prev = Some(wall);
        }
        println!();
    }

    // --- Tier-2 cohorts (Track 2 / 2a+2c): the millions, as integer flows. ---
    // The whole populace is statistical cohorts; only a bounded cast is ever a real entity. Cost is
    // O(regions), independent of headcount, so the per-tick wall stays ~flat while `souls` grows by
    // orders of magnitude — and ns/soul collapses toward zero. This is the row that reaches millions.
    println!("Tier-2 cohorts — {COHORT_REGIONS} regions, avatar present, {ticks} ticks/config");
    println!(
        "  {:>13} {:>9} {:>11} {:>12} {:>7}  {:<18}",
        "souls", "ticks/s", "us/tick", "ns/soul", "live", "fingerprint"
    );
    for &pop in &[1_000_000u64, 10_000_000, 100_000_000] {
        let (wall, souls, live, fp) = measure_cohorts(width, height, pop, ticks);
        let per_tick = wall.as_secs_f64() / ticks as f64;
        let tps = 1.0 / per_tick;
        let us_tick = per_tick * 1e6;
        let ns_soul = per_tick * 1e9 / souls.max(1) as f64;
        println!(
            "  {:>13} {:>9.0} {:>11.0} {:>12.4} {:>7} 0x{:016X}",
            souls, tps, us_tick, ns_soul, live, fp
        );
    }
    println!();
}
