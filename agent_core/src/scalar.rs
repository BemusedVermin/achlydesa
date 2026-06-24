//! The project's fixed-point gameplay scalar, [`Fx`] — the determinism-hardening replacement for the
//! `f32` gameplay scalars (`docs/scaling.md`, "fixed-point arithmetic everywhere"). Exact
//! integer-backed arithmetic instead of `f32`, so there is no cross-platform/compiler float drift and
//! addition is associative (summation order can't perturb a fingerprint).
//!
//! Backed by the [`fixed`](https://docs.rs/fixed) crate's `I64F64` (Q63.64): ~±9.2e18 range with
//! ~5.4e-20 resolution — wide enough that intermediate products like `wellbeing · headcount` or
//! `population · fertility` can't overflow at the populations the cohort layer carries. The crate
//! provides the arithmetic, ordering, and `from_num`/`to_num` conversions; no transcendental
//! functions are needed by the layers that use it.

pub type Fx = fixed::types::I64F64;
