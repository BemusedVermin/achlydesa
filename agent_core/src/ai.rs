//! A generic **utility scorer** in the Infinite Axis style (Dave Mark's IAUS).
//!
//! This is the bare scoring machinery, with no domain in it at all. A candidate
//! (here, a goal) produces normalized **inputs**; each is run through an authored
//! **response curve** to a `0..1` score; the candidate's utility is the
//! (compensated) product of its considerations. Whoever scores highest wins.
//!
//! The domain lives elsewhere: [`goals`](crate::goals) defines what a goal *is*
//! (a target condition + these considerations) and supplies the inputs. This file
//! only knows how to turn (input, curve) pairs into a number, so it never needs to
//! change when goals, actions, or facts are added.

use serde::Deserialize;

/// A response curve mapping an input to a `0..1` utility. Authored in RON.
#[derive(Deserialize, Clone, Copy, Debug)]
pub enum Curve {
    /// `m·x + b`.
    Linear { m: f32, b: f32 },
    /// `x^exp` (steepens or flattens a `0..1` input).
    Power { exp: f32 },
    /// Logistic `1 / (1 + e^(-k·(x - mid)))` — a soft threshold around `mid`.
    Logistic { mid: f32, k: f32 },
    /// `1 - x`.
    Inverse,
    /// A fixed value.
    Constant(f32),
}

impl Curve {
    pub fn eval(self, x: f32) -> f32 {
        let y = match self {
            Curve::Linear { m, b } => m * x + b,
            Curve::Power { exp } => x.max(0.0).powf(exp),
            Curve::Logistic { mid, k } => 1.0 / (1.0 + (-k * (x - mid)).exp()),
            Curve::Inverse => 1.0 - x,
            Curve::Constant(c) => c,
        };
        y.clamp(0.0, 1.0)
    }
}

/// An axis a candidate's appeal can read. Deliberately open: `Deficit` (how far a
/// goal is from satisfied) is the axis every goal supplies, and `Trait(id)` is one
/// of the agent's personality traits (a data-defined motive — ambition, vengeance,
/// …). The scorer stays generic: it doesn't know what any axis *means*; the caller
/// supplies each value. (Resolved form — authored with trait *names* and resolved
/// to ids when goals load.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// How far the goal is from being satisfied, `0` (met) … `1` (far off).
    Deficit,
    /// One of the agent's stable personality traits, by id (who you are).
    Trait(usize),
    /// One of the agent's transient moods, by id (how you feel now).
    Mood(usize),
    /// The net deontic pressure on this goal's act — how the prevailing social
    /// norms judge it *here* (see [`norms`](crate::norms)). Positive = forbidden
    /// (the agent should be reluctant), negative = obliged (socially pressed),
    /// zero = unregulated or justified. A goal weights it with a curve to decide
    /// how much it heeds the norm (`Linear { m: -1, b: 1 }` = "appeal collapses as
    /// the taboo bites").
    Sanction,

    // --- Listener-relative axes (the [`dialogue`](crate::dialogue) layer) ---
    // These read the *current addressee* of a conversational intent, supplied by the
    // caller's feature closure. They are meaningless for a goal (no listener), where
    // the closure returns `0`. Same open-axis spirit as `Trait`/`Mood`/`Sanction`.
    /// The speaker's opinion of the listener, remapped `0` (cold) … `1` (warm).
    OpinionOf,
    /// `1.0` if the speaker bears the listener a grudge, else `0`.
    GrievanceAgainst,
    /// How much salient shared history the speaker has with the listener, `0..1`.
    SharedHistory,
    /// The listener's manufactured narrative prominence (the director), `0..1`.
    Prominence,
}

/// One axis of a candidate's appeal: read an input, shape it through a curve.
#[derive(Clone, Copy, Debug)]
pub struct Consideration {
    pub input: Input,
    pub curve: Curve,
}

/// Utility of a candidate: the product of its considerations, each lifted by a
/// "makeup" factor so that adding more considerations doesn't unfairly crush the
/// score (the IAUS compensation). Each consideration reads its input value from
/// `feature` — the caller maps `Deficit`/`Trait(id)` to a number. No
/// considerations → no appeal.
pub fn score(considerations: &[Consideration], feature: impl Fn(Input) -> f32) -> f32 {
    if considerations.is_empty() {
        return 0.0;
    }
    let mod_factor = 1.0 - 1.0 / considerations.len() as f32;
    let mut total = 1.0;
    for c in considerations {
        let s = c.curve.eval(feature(c.input));
        total *= s + (1.0 - s) * mod_factor * s;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_stay_in_unit_range() {
        for x in [-1.0, 0.0, 0.3, 1.0, 2.0] {
            for c in [
                Curve::Linear { m: 2.0, b: -0.5 },
                Curve::Power { exp: 2.0 },
                Curve::Logistic { mid: 0.5, k: 8.0 },
                Curve::Inverse,
                Curve::Constant(0.7),
            ] {
                let y = c.eval(x);
                assert!((0.0..=1.0).contains(&y), "{c:?} at {x} gave {y}");
            }
        }
    }

    #[test]
    fn more_considerations_dont_collapse_the_score() {
        // Two strong considerations should still score high (compensation), rather
        // than collapsing toward their raw product.
        let cons = vec![
            Consideration {
                input: Input::Deficit,
                curve: Curve::Power { exp: 1.0 },
            },
            Consideration {
                input: Input::Deficit,
                curve: Curve::Power { exp: 1.0 },
            },
        ];
        // Naive product of two 0.85s is 0.7225; with makeup it lands above it.
        let s = score(&cons, |_| 0.85);
        assert!(
            s > 0.8,
            "compensated score should beat the raw product, got {s}"
        );
    }
}
