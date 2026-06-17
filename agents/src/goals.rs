//! Goals as **data**: a goal is a target [`Condition`] plus an *appeal*, nothing
//! more. There is no fixed enum of goals and no goal-specific code — "be fed",
//! "keep a larder", "stay solvent", "hold the throne" are all the same object,
//! differing only in the condition they name and (downstream) the operators that
//! can satisfy it.
//!
//! The split across the AI:
//! - **What to want** — the utility layer ([`ai`](crate::ai)) scores each goal's
//!   appeal from its `deficit` (how far from satisfied) and picks the most
//!   pressing it can make progress on. See [`Goals::ranked`].
//! - **How to get it** — the planner ([`plan`](crate::plan)) finds a sequence of
//!   actions that makes the chosen condition true.
//!
//! Authoring is the whole point: add a goal to `goals.ron` (a condition + an
//! appeal curve) and the agent will pursue it, weighed against the rest, with no
//! Rust changes. Adding a *new kind of want* (a new fact to target, a new verb to
//! satisfy it) is a fact + an operator + a goal entry — the AI core is untouched.

use crate::ai::{self, Consideration, Curve, Input};
use crate::data::{PredicateId, Registry};
use crate::norms::Norms;
use crate::plan::{Condition, GoodSel, PlanState};
use bevy_ecs::prelude::Resource;
use serde::Deserialize;
use std::path::Path;

/// An authored goal: the state of the world it wants true, and how appealing
/// pursuing it is (utility considerations over the goal's deficit).
#[derive(Clone, Debug)]
pub struct Goal {
    pub name: String,
    pub condition: Condition,
    pub appeal: Vec<Consideration>,
    /// The verb effect this goal enacts, `(predicate, value)`, when it was authored
    /// in the surface [`Verb`](ConditionDef::Verb) form — the handle social
    /// [`norms`](crate::norms) regulate it by. `None` for raw relation/fact goals,
    /// which carry no deontic weight.
    pub act: Option<(PredicateId, i64)>,
}

/// The agent's goal library — every objective it might pursue, as data.
#[derive(Resource, Clone, Debug)]
pub struct Goals(pub Vec<Goal>);

/// Appeal below which a goal isn't worth pursuing at all (see [`Goals::agenda`]).
/// Tiny — it only quashes goals scoring an effective zero (a fully suppressed want),
/// never a real-but-faint one.
pub const MIN_APPEAL: f32 = 1e-4;

impl Goals {
    /// The defaults shipped with the crate.
    pub fn bundled(reg: &Registry) -> Self {
        Self::from_ron(include_str!("../data/goals.ron"), reg).expect("bundled goals are valid")
    }

    /// Load `goals.ron` from a directory, resolving any good names against `reg`.
    pub fn load(dir: impl AsRef<Path>, reg: &Registry) -> Result<Self, GoalError> {
        Self::from_ron(&std::fs::read_to_string(dir.as_ref().join("goals.ron"))?, reg)
    }

    /// Parse and resolve a goals document.
    pub fn from_ron(ron: &str, reg: &Registry) -> Result<Self, GoalError> {
        let defs: Vec<GoalDef> = ron::from_str(ron)?;
        let goals = defs
            .into_iter()
            .map(|d| {
                let appeal = d.appeal.into_iter().map(|c| c.resolve(reg)).collect::<Result<Vec<_>, _>>()?;
                let act = d.condition.verb_effect(reg)?;
                Ok(Goal { name: d.name, condition: d.condition.resolve(reg)?, appeal, act })
            })
            .collect::<Result<Vec<_>, GoalError>>()?;
        Ok(Goals(goals))
    }

    /// Appeal of pursuing goal `i` right now: its considerations scored over how
    /// far the goal is from satisfied (its `deficit`) plus this agent's
    /// `personality` (trait values, indexed by trait id). The deficit axis is the
    /// same whether the gap is hunger, an empty pantry, or thin savings; traits let
    /// an *ambitious* agent weigh seizing power while a content one ignores it.
    pub fn appeal(
        &self,
        i: usize,
        s: &PlanState,
        reg: &Registry,
        personality: &[f32],
        mood: &[f32],
        norms: &Norms,
    ) -> f32 {
        let g = &self.0[i];
        let deficit = g.condition.deficit(s, reg);
        let sanction = norms.sanction(g.act, s, reg, personality);
        ai::score(&g.appeal, |input| match input {
            Input::Deficit => deficit,
            Input::Trait(t) => personality.get(t).copied().unwrap_or(0.0),
            Input::Mood(m) => mood.get(m).copied().unwrap_or(0.0),
            Input::Sanction => sanction,
            // Listener-relative axes are the dialogue layer's; a goal has no addressee.
            Input::OpinionOf | Input::GrievanceAgainst | Input::SharedHistory | Input::Prominence => 0.0,
        })
    }

    /// Goal indices, most appealing first (deterministic; ties broken by order).
    pub fn ranked(
        &self,
        s: &PlanState,
        reg: &Registry,
        personality: &[f32],
        mood: &[f32],
        norms: &Norms,
    ) -> Vec<usize> {
        let appeals: Vec<f32> =
            (0..self.0.len()).map(|i| self.appeal(i, s, reg, personality, mood, norms)).collect();
        let mut idx: Vec<usize> = (0..self.0.len()).collect();
        idx.sort_by(|&a, &b| appeals[b].total_cmp(&appeals[a]).then(a.cmp(&b)));
        idx
    }

    /// The goals actually worth pursuing right now, best first: those *unsatisfied*
    /// and with appeal above [`MIN_APPEAL`]. The floor matters — without it the
    /// planner falls through to any unmet goal once the pressing ones are handled,
    /// so a goal an agent has *no* live reason to want (a forbidden act its
    /// disposition doesn't excuse, scoring ~0) would still be done in idle moments.
    /// The floor lets appeal genuinely *veto*, which is what makes a prohibition (or
    /// any quenched motive) restrain rather than merely deprioritise.
    pub fn agenda(
        &self,
        s: &PlanState,
        reg: &Registry,
        personality: &[f32],
        mood: &[f32],
        norms: &Norms,
    ) -> Vec<usize> {
        let mut live: Vec<(usize, f32)> = (0..self.0.len())
            .filter(|&i| !self.0[i].condition.satisfied(s, reg))
            .map(|i| (i, self.appeal(i, s, reg, personality, mood, norms)))
            .filter(|&(_, a)| a >= MIN_APPEAL)
            .collect();
        live.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        live.into_iter().map(|(i, _)| i).collect()
    }

}

// --- Authored form (good names not yet resolved) ---

#[derive(Deserialize)]
struct GoalDef {
    name: String,
    condition: ConditionDef,
    appeal: Vec<ConsiderationDef>,
}

/// The RON form of a [`Consideration`]; trait names resolved to ids on load.
#[derive(Deserialize)]
struct ConsiderationDef {
    input: InputDef,
    curve: Curve,
}

/// The RON form of an [`Input`]: `Deficit`, `Trait("name")`, `Mood("name")`, or
/// `Sanction` (the deontic pressure on the goal's act).
#[derive(Deserialize)]
enum InputDef {
    Deficit,
    Trait(String),
    Mood(String),
    Sanction,
}

impl ConsiderationDef {
    fn resolve(self, reg: &Registry) -> Result<Consideration, GoalError> {
        let input = match self.input {
            InputDef::Deficit => Input::Deficit,
            InputDef::Trait(n) => Input::Trait(reg.trait_id(&n).ok_or(GoalError::UnknownTrait(n))?),
            InputDef::Mood(n) => Input::Mood(reg.mood_id(&n).ok_or(GoalError::UnknownMood(n))?),
            InputDef::Sanction => Input::Sanction,
        };
        Ok(Consideration { input, curve: self.curve })
    }
}

/// The RON form of a [`Condition`]; names (goods, predicates) resolve to ids on
/// load. Shared with [`norms`](crate::norms), whose `when` clauses are conditions.
#[derive(Deserialize)]
pub(crate) enum ConditionDef {
    Sustenance { at_least: i32 },
    Rest { at_least: i32 },
    Money { at_least: i64 },
    Holding { good: GoodSelDef, at_least: u32 },
    /// A relational predicate of a bound entity: `Relation("alive", Foe, 0)` =
    /// "make alive(foe) false". `subject` says whose — the agent itself (`Me`, the
    /// default) or its bound target (`Foe`). Grounds to a flat fact slot.
    Relation {
        predicate: String,
        #[serde(default)]
        subject: SubjectDef,
        equals: i64,
    },
    /// The surface form: a verb applied to a target — `Verb("avenge", Foe)` reads
    /// "avenge (myself) on the foe". The verb names both the predicate and the value
    /// it seeks (`avenge` → make `alive` `0`); `target` says of whom. Sugar over
    /// `Relation`, grounding to the same flat fact slot.
    Verb {
        verb: String,
        #[serde(default)]
        target: SubjectDef,
    },
    /// A raw flat fact slot (escape hatch / tests).
    Fact { fact: usize, equals: i64 },
}

#[derive(Deserialize)]
pub(crate) enum GoodSelDef {
    Edible,
    Named(String),
}

/// Which bound entity a relational condition is about. The role's index is the
/// entity slot the planner grounds against (self = 0, foe = 1).
#[derive(Deserialize, Default, Clone, Copy)]
pub(crate) enum SubjectDef {
    #[default]
    Me,
    Foe,
}

impl SubjectDef {
    fn role(self) -> usize {
        match self {
            SubjectDef::Me => 0,
            SubjectDef::Foe => 1,
        }
    }
}

impl ConditionDef {
    /// The verb effect `(predicate, value)` this condition enacts, if it was
    /// authored in the surface [`Verb`](ConditionDef::Verb) form — the handle social
    /// norms regulate the goal by. `None` for the raw relation/fact forms.
    fn verb_effect(&self, reg: &Registry) -> Result<Option<(PredicateId, i64)>, GoalError> {
        match self {
            ConditionDef::Verb { verb, .. } => {
                Ok(Some(reg.verb(verb).ok_or_else(|| GoalError::UnknownVerb(verb.clone()))?))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn resolve(self, reg: &Registry) -> Result<Condition, GoalError> {
        Ok(match self {
            ConditionDef::Sustenance { at_least } => Condition::Sustenance { at_least },
            ConditionDef::Rest { at_least } => Condition::Rest { at_least },
            ConditionDef::Money { at_least } => Condition::Money { at_least },
            ConditionDef::Holding { good, at_least } => Condition::Holding { good: good.resolve(reg)?, at_least },
            ConditionDef::Relation { predicate, subject, equals } => {
                let p = reg.predicate_id(&predicate).ok_or(GoalError::UnknownPredicate(predicate))?;
                Condition::Fact { fact: crate::data::fact_slot(p, subject.role()), equals }
            }
            ConditionDef::Verb { verb, target } => {
                let (p, value) = reg.verb(&verb).ok_or(GoalError::UnknownVerb(verb))?;
                Condition::Fact { fact: crate::data::fact_slot(p, target.role()), equals: value }
            }
            ConditionDef::Fact { fact, equals } => Condition::Fact { fact, equals },
        })
    }
}

impl GoodSelDef {
    fn resolve(self, reg: &Registry) -> Result<GoodSel, GoalError> {
        Ok(match self {
            GoodSelDef::Edible => GoodSel::Edible,
            GoodSelDef::Named(n) => GoodSel::Named(reg.good_id(&n).ok_or(GoalError::UnknownGood(n))?),
        })
    }
}

/// Why loading goals failed.
#[derive(Debug)]
pub enum GoalError {
    Io(std::io::Error),
    Ron(ron::error::SpannedError),
    UnknownGood(String),
    UnknownTrait(String),
    UnknownMood(String),
    UnknownPredicate(String),
    UnknownVerb(String),
}

impl std::fmt::Display for GoalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoalError::Io(e) => write!(f, "reading goals: {e}"),
            GoalError::Ron(e) => write!(f, "parsing goals: {e}"),
            GoalError::UnknownGood(n) => write!(f, "goal refers to unknown good '{n}'"),
            GoalError::UnknownTrait(n) => write!(f, "goal refers to unknown trait '{n}'"),
            GoalError::UnknownMood(n) => write!(f, "goal refers to unknown mood '{n}'"),
            GoalError::UnknownPredicate(n) => write!(f, "goal refers to unknown predicate '{n}'"),
            GoalError::UnknownVerb(n) => write!(f, "goal refers to unknown verb '{n}'"),
        }
    }
}
impl std::error::Error for GoalError {}
impl From<std::io::Error> for GoalError {
    fn from(e: std::io::Error) -> Self {
        GoalError::Io(e)
    }
}
impl From<ron::error::SpannedError> for GoalError {
    fn from(e: ron::error::SpannedError) -> Self {
        GoalError::Ron(e)
    }
}

impl Default for Goals {
    fn default() -> Self {
        Self::bundled(&Registry::bundled())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Facts, Stock};
    use game_sim::Coord;

    fn state(reg: &Registry, sustenance: i32, rest: i32, money: i64, edible: u32) -> PlanState {
        // Put all the edible food in the first edible good.
        let mut stock = Stock::from_elem(0u32, reg.good_count());
        if let Some(g) = (0..reg.good_count()).find(|&g| reg.good(g).nutrition > 0.0) {
            stock[g] = edible;
        }
        PlanState { sustenance, rest, money, stock, pos: Coord::new(0, 0), facts: Facts::new(), learned: Stock::new() }
    }

    fn top(goals: &Goals, reg: &Registry, s: &PlanState) -> String {
        top_for(goals, reg, s, &[])
    }

    fn top_for(goals: &Goals, reg: &Registry, s: &PlanState, personality: &[f32]) -> String {
        let n = Norms::default();
        goals.0[goals.ranked(s, reg, personality, &[], &n)[0]].name.clone()
    }

    /// A personality vector with one named trait set to `value`, the rest zero.
    fn personality_with(reg: &Registry, name: &str, value: f32) -> Vec<f32> {
        let mut p = vec![0.0; reg.trait_count()];
        p[reg.trait_id(name).unwrap()] = value;
        p
    }

    #[test]
    fn bundled_goals_load() {
        let reg = Registry::bundled();
        let goals = Goals::bundled(&reg);
        let names: Vec<_> = goals.0.iter().map(|g| g.name.as_str()).collect();
        assert!(names.contains(&"sustained") && names.contains(&"stocked") && names.contains(&"solvent"));
    }

    #[test]
    fn the_starving_pick_survival_over_everything() {
        let reg = Registry::bundled();
        let goals = Goals::bundled(&reg);
        // Desperately hungry but rich and well-stocked: survival still tops.
        let s = state(&reg, 5, 100, 100_000, 99);
        assert_eq!(top(&goals, &reg, &s), "sustained");
    }

    #[test]
    fn the_fed_but_bare_pantry_go_shopping() {
        let reg = Registry::bundled();
        let goals = Goals::bundled(&reg);
        // Not hungry, rested, rich — but the larder is empty: stock up.
        let s = state(&reg, 100, 100, 100_000, 0);
        assert_eq!(top(&goals, &reg, &s), "stocked");
    }

    #[test]
    fn the_secure_but_poor_seek_money() {
        let reg = Registry::bundled();
        let goals = Goals::bundled(&reg);
        // Fed, rested, larder full, but broke: pursue wealth.
        let s = state(&reg, 100, 100, 0, 99);
        assert_eq!(top(&goals, &reg, &s), "solvent");
    }

    #[test]
    fn only_the_ambitious_eye_the_throne() {
        // Economy goals plus "rule": hold the throne (an abstract fact). Its appeal
        // is gated on ambition, so a content soul never pursues it while a secure,
        // ambitious one does — personality, not need, drives this goal.
        let reg = Registry::bundled();
        let ron = r#"[
            (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
            (name: "stocked",   condition: Holding(good: Edible, at_least: 15), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
            (name: "rule",      condition: Verb(verb: "rule", target: Me),
                appeal: [(input: Trait("ambition"), curve: Linear(m: 1.0, b: 0.0)), (input: Deficit, curve: Linear(m: 1.0, b: 0.0))]),
        ]"#;
        let goals = Goals::from_ron(ron, &reg).unwrap();
        // Secure (fed and well-stocked) so survival/provisioning are quiet, and the
        // throne is unheld (fact 0 == 0, so "rule" has full deficit).
        let mut s = state(&reg, 100, 100, 0, 99);
        s.facts = Facts::from_elem(0, 1);
        assert_eq!(top_for(&goals, &reg, &s, &personality_with(&reg, "ambition", 1.0)), "rule", "the ambitious go for it");
        assert_ne!(top_for(&goals, &reg, &s, &personality_with(&reg, "ambition", 0.0)), "rule", "the content do not");
    }

    #[test]
    fn an_unknown_trait_in_a_goal_is_rejected() {
        let reg = Registry::bundled();
        let ron = r#"[(name: "x", condition: Fact(fact: 0, equals: 1),
            appeal: [(input: Trait("greedmaxxing"), curve: Linear(m: 1.0, b: 0.0))])]"#;
        assert!(matches!(Goals::from_ron(ron, &reg), Err(GoalError::UnknownTrait(_))));
    }

    #[test]
    fn an_unknown_good_in_a_goal_is_rejected() {
        let reg = Registry::bundled();
        let ron = r#"[(name: "hoard", condition: Holding(good: Named("unobtanium"), at_least: 5),
            appeal: [(input: Deficit, curve: Linear(m: 1.0, b: 0.0))])]"#;
        assert!(matches!(Goals::from_ron(ron, &reg), Err(GoalError::UnknownGood(_))));
    }
}
