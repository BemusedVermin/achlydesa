//! A tiny seedable, deterministic RNG (SplitMix64) — the project's standard stream, hand-rolled
//! here so the core pulls in no `rand` crate and never touches `thread_rng`. A single instance
//! lives in `Sim`; draw order is fixed by the resolution order, so draws are reproducible.

use serde::{Deserialize, Serialize};

/// SplitMix64. Stateful, cheap, and good enough for the light randomized variance v1 needs.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// A uniform-ish draw in `[0, n)` (modulo reduction — adequate for v1's light use). `n == 0`
    /// yields `0`.
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as u32
        }
    }
}
