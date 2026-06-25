//! The project's fixed-point gameplay scalar, [`Fx`] — the determinism-hardening replacement for the
//! `f32` gameplay scalars (`docs/scaling.md`, "fixed-point arithmetic everywhere"). Exact
//! integer-backed arithmetic instead of `f32`, so there is no cross-platform/compiler float drift and
//! addition is associative (summation order can't perturb a fingerprint).
//!
//! Backed by the [`fixed`](https://docs.rs/fixed) crate's `I64F64` (Q63.64): ~±9.2e18 range with
//! ~5.4e-20 resolution — wide enough that intermediate products like `wellbeing · headcount` or
//! `population · fertility` can't overflow at the populations the cohort layer carries. The crate
//! provides the arithmetic, ordering, and `from_num`/`to_num` conversions.
//!
//! The IAUS appraisal ([`crate::ai`]) also needs *transcendentals* — the response curves use
//! `x^p` and the logistic `1/(1+e^…)`. The `cordic` crate (the usual fixed-point math lib) only
//! implements its `CordicNumber` for the ≤64-bit `fixed` types (`FixedI64`/`32`/`16`/`8`), **not**
//! the 128-bit `I64F64` we use, and it ships no `ln` at all. So this module carries a tiny,
//! self-contained [`fx_math`] of `exp`/`ln`/`powf`/`logistic` evaluated directly on `Fx` by
//! range-reduction + a fixed series — pure integer arithmetic, so it inherits `Fx`'s exactness and
//! determinism (no cross-platform float drift). At 64 fractional bits these match `f64` to ~1e-15.

pub type Fx = fixed::types::I64F64;

/// Deterministic fixed-point transcendentals on [`Fx`], for the IAUS response curves.
///
/// Everything here is plain `Fx` arithmetic (add/mul/div/shift) over a fixed iteration count, so a
/// given input maps to the same bits on every platform and toolchain — the whole point of moving the
/// appraisal off `f32`.
pub mod fx_math {
    use super::Fx;

    /// `ln 2`, parsed from a decimal literal to the full ~1e-19 resolution of `Fx` (more precise
    /// than `f64::consts::LN_2`, and no float intermediate).
    const LN2: Fx = Fx::lit("0.6931471805599453094172321214581766");

    /// Natural log of `x > 0`. Range-reduces `x = m·2^e` with `m ∈ [1,2)`, then sums the fast
    /// `ln m = 2·(t + t³/3 + t⁵/5 + …)`, `t = (m-1)/(m+1) ∈ [0,1/3)`. Panics on `x ≤ 0` (callers
    /// gate that — a curve never feeds a non-positive value here).
    pub fn ln(x: Fx) -> Fx {
        debug_assert!(x > Fx::ZERO, "ln of non-positive {x}");
        let one = Fx::ONE;
        let two = Fx::from_num(2);
        let mut m = x;
        let mut e: i64 = 0;
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
        let mut term = t;
        let mut sum = Fx::ZERO;
        let mut k: i64 = 1;
        // t ≤ 1/3, so t^(2n+1) decays fast; 24 odd terms is well past Fx's resolution.
        for _ in 0..24 {
            sum += term / Fx::from_num(k);
            term *= t2;
            k += 2;
        }
        Fx::from_num(e) * LN2 + two * sum
    }

    /// `e^x`. Range-reduces `x = k·ln2 + r` with `|r| ≤ ln2/2`, evaluates `e^r` by Taylor, then
    /// scales by `2^k` with an exact shift.
    pub fn exp(x: Fx) -> Fx {
        let l2 = LN2;
        let kf = (x / l2).round();
        let k: i64 = kf.to_num();
        let r = x - kf * l2;
        let mut term = Fx::ONE;
        let mut sum = Fx::ONE;
        // |r| ≤ 0.347, so r^n/n! is past Fx's resolution well before 20 terms.
        for n in 1..=20i64 {
            term *= r / Fx::from_num(n);
            sum += term;
        }
        if k >= 0 {
            sum << (k as u32)
        } else {
            sum >> ((-k) as u32)
        }
    }

    /// `x^p` for `x ≥ 0` via `e^{p·ln x}`. `x ≤ 0` returns `0` (the curves only raise a clamped
    /// `0..1` input, where `0^p = 0` is the intended floor).
    pub fn powf(x: Fx, p: Fx) -> Fx {
        if x <= Fx::ZERO {
            Fx::ZERO
        } else {
            exp(p * ln(x))
        }
    }

    /// Logistic `1 / (1 + e^{-k·(x - mid)})` — the soft threshold the `Logistic` curve uses.
    pub fn logistic(x: Fx, mid: Fx, k: Fx) -> Fx {
        Fx::ONE / (Fx::ONE + exp(-k * (x - mid)))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn transcendentals_match_f64() {
            for &v in &[0.05_f64, 0.25, 0.5, 0.7, 1.0, 1.5, 2.0, 7.3, 100.0] {
                let got: f64 = ln(Fx::from_num(v)).to_num();
                assert!((got - v.ln()).abs() < 1e-12, "ln({v}) = {got}");
            }
            for &v in &[-6.0_f64, -1.0, -0.3, 0.0, 0.3, 1.0, 6.0] {
                let got: f64 = exp(Fx::from_num(v)).to_num();
                assert!(
                    (got - v.exp()).abs() < 1e-9 * v.exp().max(1.0),
                    "exp({v}) = {got}"
                );
            }
            for &(x, p) in &[(0.3_f64, 1.5_f64), (0.7, 2.0), (0.05, 1.3), (1.0, 1.0)] {
                let got: f64 = powf(Fx::from_num(x), Fx::from_num(p)).to_num();
                assert!((got - x.powf(p)).abs() < 1e-9, "{x}^{p} = {got}");
            }
            let got: f64 =
                logistic(Fx::from_num(0.8), Fx::from_num(0.5), Fx::from_num(8.0)).to_num();
            assert!((got - 1.0 / (1.0 + (-8.0 * 0.3_f64).exp())).abs() < 1e-9);
        }

        #[test]
        fn deterministic_bits() {
            let a = powf(Fx::from_num(0.3), Fx::from_num(1.5));
            let b = powf(Fx::from_num(0.3), Fx::from_num(1.5));
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }
}
