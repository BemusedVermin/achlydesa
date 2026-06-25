//! Micro-benchmark for the IAUS response-curve transcendentals (`scalar::fx_math`).
//!
//! Answers the open question on PR #16: how much does evaluating the curves in 128-bit `I64F64` cost
//! versus (a) the `f32` they replaced and (b) a *smaller* 64-bit fixed type (`I32F32`)? For each we
//! time the two curves the appraisal actually uses — `Power` (`x^p`, needs `ln`) and `Logistic`
//! (`1/(1+e^…)`). `cordic` is included only on the `Logistic`'s `exp`: it ships no `ln`/`powf`, so the
//! `Power` curve can't use it at any width — the reason `fx_math` exists. Run with:
//!
//! ```sh
//! cargo run -p agent_core --example bench_fx_math --release
//! ```
//!
//! Wall-clock timings — never linked into the sim; an examples-only tool.

use std::hint::black_box;
use std::time::Instant;

use agent_core::scalar::{Fx, fx_math};
use fixed::types::I32F32;

/// The same range-reduced series as `scalar::fx_math`, ported to the 64-bit `I32F32` so we can price
/// the *integer width* in isolation (identical algorithm, half the bits).
mod small {
    use fixed::types::I32F32 as F;
    const LN2: F = F::lit("0.6931471805599453094");

    pub fn ln(x: F) -> F {
        let (one, two) = (F::ONE, F::from_num(2));
        let (mut m, mut e) = (x, 0i32);
        while m >= two {
            m >>= 1;
            e += 1;
        }
        while m < one {
            m <<= 1;
            e -= 1;
        }
        let t = (m - one) / (m + one);
        let t2 = t * t;
        let (mut term, mut sum, mut k) = (t, F::ZERO, 1i32);
        for _ in 0..16 {
            sum += term / F::from_num(k);
            term *= t2;
            k += 2;
        }
        F::from_num(e) * LN2 + two * sum
    }

    pub fn exp(x: F) -> F {
        let kf = (x / LN2).round();
        let k: i32 = kf.to_num();
        let r = x - kf * LN2;
        let (mut term, mut sum) = (F::ONE, F::ONE);
        for n in 1..=14i32 {
            term *= r / F::from_num(n);
            sum += term;
        }
        if k >= 0 {
            if k >= 31 { F::MAX } else { sum << (k as u32) }
        } else if -k >= 32 {
            F::ZERO
        } else {
            sum >> ((-k) as u32)
        }
    }

    pub fn powf(x: F, p: F) -> F {
        if x <= F::ZERO {
            F::ZERO
        } else {
            exp(p * ln(x))
        }
    }
    pub fn logistic(x: F, mid: F, k: F) -> F {
        F::ONE / F::ONE.saturating_add(exp(-k * (x - mid)))
    }
}

const ITERS: u32 = 100_000;

fn main() {
    // Curve inputs in their authored domain: x ∈ (0,1], exponent 1.7, logistic k=12 around mid=0.5.
    let xs: Vec<f64> = (1..=64).map(|i| i as f64 / 64.0).collect();
    let evals = ITERS as f64 * xs.len() as f64 * 2.0; // 2 curve evals (Power + Logistic) per step

    // ---- f32 (what the migration replaced) ----
    let t = Instant::now();
    let mut acc = 0.0f32;
    for _ in 0..ITERS {
        for &x in &xs {
            let x = x as f32;
            acc += black_box(x.powf(1.7));
            acc += black_box(1.0 / (1.0 + (-12.0 * (x - 0.5)).exp()));
        }
    }
    black_box(acc);
    let f32_ns = t.elapsed().as_nanos();

    // ---- I64F64 series (the shipped path) ----
    let t = Instant::now();
    let mut acc = Fx::ZERO;
    let (p, k, mid) = (Fx::from_num(1.7), Fx::from_num(12), Fx::from_num(0.5));
    for _ in 0..ITERS {
        for &x in &xs {
            let x = Fx::from_num(x);
            acc += black_box(fx_math::powf(x, p));
            acc += black_box(fx_math::logistic(x, mid, k));
        }
    }
    black_box(acc);
    let i64_ns = t.elapsed().as_nanos();

    // ---- I32F32 series (same algorithm, half the width) ----
    let t = Instant::now();
    let mut acc = I32F32::ZERO;
    let (p, k, mid) = (
        I32F32::from_num(1.7),
        I32F32::from_num(12),
        I32F32::from_num(0.5),
    );
    for _ in 0..ITERS {
        for &x in &xs {
            let x = I32F32::from_num(x);
            acc += black_box(small::powf(x, p));
            acc += black_box(small::logistic(x, mid, k));
        }
    }
    black_box(acc);
    let i32_ns = t.elapsed().as_nanos();

    // ---- I32F32: Power via our series (cordic has no ln) + Logistic via cordic::exp ----
    let t = Instant::now();
    let mut acc = I32F32::ZERO;
    let (p, k, mid) = (
        I32F32::from_num(1.7),
        I32F32::from_num(12),
        I32F32::from_num(0.5),
    );
    for _ in 0..ITERS {
        for &x in &xs {
            let x = I32F32::from_num(x);
            acc += black_box(small::powf(x, p)); // cordic can't do x^p — no ln
            acc += black_box(I32F32::ONE / I32F32::ONE.saturating_add(cordic::exp(-k * (x - mid))));
        }
    }
    black_box(acc);
    let cordic_ns = t.elapsed().as_nanos();

    let per = |ns: u128| ns as f64 / evals;
    println!(
        "curve evals: {:.0}M  (Power x^1.7 + Logistic k=12, half each)",
        evals / 1e6
    );
    println!(
        "  f32 std                  {:6.1} ns/eval   baseline",
        per(f32_ns)
    );
    println!(
        "  I64F64 series (shipped)  {:6.1} ns/eval   {:.1}x f32",
        per(i64_ns),
        i64_ns as f64 / f32_ns as f64
    );
    println!(
        "  I32F32 series            {:6.1} ns/eval   {:.1}x f32   {:.2}x faster than I64F64",
        per(i32_ns),
        i32_ns as f64 / f32_ns as f64,
        i64_ns as f64 / i32_ns as f64
    );
    println!(
        "  I32F32 series+cordic exp {:6.1} ns/eval   {:.1}x f32   (Power still needs our ln)",
        per(cordic_ns),
        cordic_ns as f64 / f32_ns as f64
    );
}
