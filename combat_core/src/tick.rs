//! Integer time and fixed-point magnitudes — the determinism foundation.
//!
//! Time is whole [`Tick`]s (`u64`); there is no wall clock and no floating point anywhere in
//! the core (see `PORTING.md`). Continuous magnitudes (a damage multiplier, a window's
//! strength) use [`Fixed`], a 16.16 signed fixed-point number, so every arithmetic result is
//! reproducible bit-for-bit across machines.

use serde::{Deserialize, Serialize};

/// A point on the global combat timeline, in whole ticks.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Tick(pub u64);

impl Tick {
    pub const ZERO: Tick = Tick(0);

    #[inline]
    pub fn saturating_add(self, d: u64) -> Tick {
        Tick(self.0.saturating_add(d))
    }
    #[inline]
    pub fn saturating_sub(self, d: u64) -> Tick {
        Tick(self.0.saturating_sub(d))
    }
}

impl core::ops::Mul for Fixed {
    type Output = Fixed;
    /// Fixed × Fixed, via an `i64` intermediate so the product never overflows mid-flight.
    #[inline]
    fn mul(self, other: Fixed) -> Fixed {
        Fixed((((self.0 as i64) * (other.0 as i64)) >> Self::SHIFT) as i32)
    }
}

impl core::ops::Add<u64> for Tick {
    type Output = Tick;
    #[inline]
    fn add(self, rhs: u64) -> Tick {
        Tick(self.0 + rhs)
    }
}
impl core::ops::Sub<u64> for Tick {
    type Output = Tick;
    #[inline]
    fn sub(self, rhs: u64) -> Tick {
        Tick(self.0 - rhs)
    }
}

/// A 16.16 signed fixed-point number (backing `i32`): the high 16 bits are the integer part,
/// the low 16 the fraction. Used for the handful of continuous magnitudes in the core (e.g.
/// the exposed-damage multiplier). Determinism over prettiness — never a float.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct Fixed(pub i32);

impl Fixed {
    pub const SHIFT: u32 = 16;
    pub const ONE: Fixed = Fixed(1 << 16);
    pub const ZERO: Fixed = Fixed(0);

    /// Lift an integer into fixed-point.
    #[inline]
    pub fn from_int(n: i32) -> Fixed {
        Fixed(n << Self::SHIFT)
    }

    /// Truncate toward negative infinity to an integer.
    #[inline]
    pub fn to_int(self) -> i32 {
        self.0 >> Self::SHIFT
    }

    /// A ratio `num/den` as fixed-point (e.g. `from_ratio(3, 2)` is 1.5).
    #[inline]
    pub fn from_ratio(num: i32, den: i32) -> Fixed {
        Fixed((((num as i64) << Self::SHIFT) / den as i64) as i32)
    }

    /// Scale an integer by this factor, rounding to the nearest integer (ties away from zero
    /// for non-negative inputs). This is the damage-multiplier path: `1.5.scale_int(7) == 11`.
    #[inline]
    pub fn scale_int(self, n: i32) -> i32 {
        let product = (n as i64) * (self.0 as i64);
        ((product + (1 << (Self::SHIFT - 1))) >> Self::SHIFT) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_round_trips_and_scales() {
        assert_eq!(Fixed::from_int(3).to_int(), 3);
        assert_eq!(Fixed::ONE.to_int(), 1);
        let one_and_half = Fixed::from_ratio(3, 2);
        assert_eq!(one_and_half, Fixed(0x1_8000));
        // 1.5 × 10 = 15 exactly; 1.5 × 7 = 10.5 → 11 (round half up).
        assert_eq!(one_and_half.scale_int(10), 15);
        assert_eq!(one_and_half.scale_int(7), 11);
        // identity factor leaves integers untouched.
        assert_eq!(Fixed::ONE.scale_int(42), 42);
    }
}
