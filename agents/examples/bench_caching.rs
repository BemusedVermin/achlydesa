//! A throwaway benchmark for the per-tick planning caches: it times a fixed run and
//! counts heap allocations (via a counting global allocator) so the effect of the
//! movement-graph caching can be measured directly.
//!
//! Run with: `cargo run --release --example bench_caching`

use agents::{Setup, Simulation};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// Wraps the system allocator to count allocations (the headline metric for the
/// movement-graph cache, which used to allocate one `Vec` per tile per tick).
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

fn main() {
    const TICKS: u64 = 300;
    let (width, height, npcs) = (64, 48, 150);

    // Worldgen + warm-up happen here and are *not* timed.
    let mut sim = Simulation::new(Setup {
        width,
        height,
        seed: 1,
        npcs,
        ..Default::default()
    });
    // One untimed tick so first-touch allocations (lazy statics, pools) don't skew the count.
    sim.run(1);

    let allocs_before = ALLOCS.load(Ordering::Relaxed);
    let t = Instant::now();
    sim.run(TICKS);
    let dt = t.elapsed();
    let allocs = ALLOCS.load(Ordering::Relaxed) - allocs_before;

    let tiles = (width * height) as usize;
    println!("world {width}x{height} ({tiles} tiles), {npcs} npcs, {TICKS} ticks");
    println!(
        "  wall:   {dt:?}  ({:.0} ticks/s)",
        TICKS as f64 / dt.as_secs_f64()
    );
    println!("  allocs: {allocs}  ({} /tick)", allocs as u64 / TICKS);
}
