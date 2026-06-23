//! The continuous 2D spatial model (replacing the v1 abstract zones). Positions are 16.16
//! [`Fixed`] pairs — no floats, fully deterministic. A move targets a *person* and lands only if
//! that target is within the move's `reach` at the active frame (else it **whiffs**); movement
//! effects slide an actor along the 1D line toward or away from its target. All distance math is
//! integer (compare squared distances; an integer sqrt for the one normalization we need).

use crate::tick::Fixed;
use serde::{Deserialize, Serialize};

/// A point on the field. `x`/`y` are world units in 16.16 fixed-point.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Pos {
    pub x: Fixed,
    pub y: Fixed,
}

impl Pos {
    pub const ORIGIN: Pos = Pos {
        x: Fixed::ZERO,
        y: Fixed::ZERO,
    };

    pub fn new(x: Fixed, y: Fixed) -> Pos {
        Pos { x, y }
    }

    /// Build from integer coordinates.
    pub fn from_ints(x: i32, y: i32) -> Pos {
        Pos {
            x: Fixed::from_int(x),
            y: Fixed::from_int(y),
        }
    }

    /// Squared distance to `other`, in the (16.16)² scale — directly comparable to a squared
    /// [`Fixed`] reach (`reach.0 as i64 * reach.0 as i64`). Squared so no sqrt is needed for the
    /// reach test.
    pub fn dist_sq(self, other: Pos) -> i64 {
        let dx = (self.x.0 - other.x.0) as i64;
        let dy = (self.y.0 - other.y.0) as i64;
        dx * dx + dy * dy
    }

    /// Whether `other` is within `reach` of `self`.
    pub fn within(self, other: Pos, reach: Fixed) -> bool {
        let r = reach.0 as i64;
        self.dist_sq(other) <= r * r
    }

    /// Step from `self` toward `target` by up to `step` units, never overshooting it.
    pub fn step_toward(self, target: Pos, step: Fixed) -> Pos {
        let dx = (target.x.0 - self.x.0) as i64;
        let dy = (target.y.0 - self.y.0) as i64;
        let len = isqrt(dx * dx + dy * dy);
        let step = step.0 as i64;
        if len == 0 || step >= len {
            return target; // already there, or this step reaches/overshoots it
        }
        Pos {
            x: Fixed((self.x.0 as i64 + dx * step / len) as i32),
            y: Fixed((self.y.0 as i64 + dy * step / len) as i32),
        }
    }

    /// Step from `self` directly away from `from` by `step` units. Coincident points can't define a
    /// direction, so the actor holds.
    pub fn step_away(self, from: Pos, step: Fixed) -> Pos {
        let dx = (self.x.0 - from.x.0) as i64;
        let dy = (self.y.0 - from.y.0) as i64;
        let len = isqrt(dx * dx + dy * dy);
        let step = step.0 as i64;
        if len == 0 {
            return self;
        }
        Pos {
            x: Fixed((self.x.0 as i64 + dx * step / len) as i32),
            y: Fixed((self.y.0 as i64 + dy * step / len) as i32),
        }
    }
}

/// Integer square root (Newton's method) — deterministic, no floats.
fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reach_and_steps() {
        let a = Pos::from_ints(0, 0);
        let b = Pos::from_ints(3, 4); // distance 5
        assert!(a.within(b, Fixed::from_int(5)));
        assert!(!a.within(b, Fixed::from_int(4)));
        // A step of 5 toward b lands exactly on b; a step of 10 doesn't overshoot.
        assert_eq!(a.step_toward(b, Fixed::from_int(5)), b);
        assert_eq!(a.step_toward(b, Fixed::from_int(10)), b);
        // Stepping away from a by 5 doubles the distance (to 10) along the same line.
        let away = b.step_away(a, Fixed::from_int(5));
        assert_eq!(away, Pos::from_ints(6, 8));
    }
}
