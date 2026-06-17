//! Randomness `ω`.

/// Random input to the transition. Seed it for reproducible runs.
pub trait Rng {
    fn next_u64(&mut self) -> u64;

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    fn gen_bool(&mut self, p: f64) -> bool {
        self.next_f64() < p
    }

    /// `[0, n)`; `0` when `n == 0`.
    fn gen_range(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}
