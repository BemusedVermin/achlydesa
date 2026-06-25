//! Deontic **social norms** — permissions, prohibitions, and obligations layered
//! over the verbs an agent might pursue (Versu/Praxis, Richard Evans & Emily
//! Short). Where a [`goal`](crate::goals) says what an agent *wants*, a norm says
//! what its society holds *permitted, forbidden, or obliged* — and the want is
//! weighed against that judgement before the agent commits.
//!
//! A norm regulates an **act** (a verb's effect, e.g. `avenge` = make a foe not
//! alive), under a **modality**:
//! - **Forbidden** — a taboo. It adds a *sanction* that suppresses the goal's
//!   appeal, so the act is pursued only reluctantly (or not at all).
//! - **Permitted** — a justification/exception. If one is in force it *lifts* the
//!   prohibitions on that act: vengeance becomes righteous when the foe is a tyrant.
//! - **Obliged** — a duty. It presses the act *toward* being done (a negative
//!   sanction), so a bound agent pursues it even absent personal need.
//!
//! Two things keep this from being a moral straitjacket, both authored as data:
//! - a norm's **`when`** — a relational [`Condition`] over the same grounded facts
//!   the planner uses — so it only bites in the situations it names; and
//! - a norm's **`defiance`** — a personality trait that *resists* it, so the same
//!   taboo restrains the meek yet barely touches the defiant. Norms shape a
//!   population's behaviour without dictating any individual's.
//!
//! The whole layer is opt-in: with no norms authored, [`Norms::sanction`] is always
//! zero and goal appeal is exactly as before. The deontic force enters appeal
//! through the [`Sanction`](crate::ai::Input::Sanction) input — the scorer and the
//! planner are untouched.

use crate::data::{PredicateId, Registry, TraitId};
use crate::goals::{ConditionDef, GoalError};
use crate::plan::{Condition, PlanState};
use crate::scalar::Fx;
use bevy_ecs::prelude::Resource;
use config::{Asset, Config};
use serde::Deserialize;

/// The deontic status a norm confers on its act.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modality {
    /// A taboo: pursuing the act carries a social sanction.
    Forbidden,
    /// A justification: while in force it lifts the prohibitions on the act.
    Permitted,
    /// A duty: the act is socially pressed even without personal motive.
    Obliged,
}

/// A resolved social norm: a deontic judgement on an act, optionally conditional on
/// a situation and resistible by a disposition.
#[derive(Clone, Debug)]
pub struct Norm {
    /// The act this judges — a verb's grounded effect `(predicate, value)`, matched
    /// against a goal's [`act`](crate::goals::Goal::act).
    pub act: (PredicateId, i64),
    pub modality: Modality,
    /// Strength of the sanction (Forbidden) or pull (Obliged); ignored for Permitted.
    pub weight: f32,
    /// The situation the norm applies in (over the agent's grounded facts); `None` =
    /// always.
    pub when: Option<Condition>,
    /// A trait that resists this norm: the agent feels the sanction scaled by
    /// `1 - trait`, so a fully-`defiance` soul ignores it. `None` = inescapable.
    pub defiance: Option<TraitId>,
}

/// The society's norms, shared as a resource. Empty by default — norms are opt-in
/// scenario data, like the throne or feuds.
#[derive(Resource, Clone, Debug, Default)]
pub struct Norms(pub Vec<Norm>);

impl Norms {
    /// The defaults shipped with the crate (an empty norms set — no taboos unless
    /// a scenario authors them).
    pub fn bundled(reg: &Registry) -> Self {
        Self::load(&Config::bundled(), reg).expect("bundled norms are valid")
    }

    /// Load the norms from a [`Config`]'s content source, resolving
    /// verb/predicate/trait names.
    pub fn load(cfg: &Config, reg: &Registry) -> Result<Self, NormError> {
        Self::from_defs(cfg.load(Asset::Norms)?, reg)
    }

    /// Parse and resolve a norms document.
    pub fn from_ron(ron: &str, reg: &Registry) -> Result<Self, NormError> {
        Self::from_defs(config::parse(ron)?, reg)
    }

    /// Resolve already-parsed norm definitions against the registry.
    fn from_defs(defs: Vec<NormDef>, reg: &Registry) -> Result<Self, NormError> {
        let norms = defs
            .into_iter()
            .map(|d| {
                let act = reg.verb(&d.act).ok_or(NormError::UnknownVerb(d.act))?;
                let when = d
                    .when
                    .map(|c| c.resolve(reg))
                    .transpose()
                    .map_err(NormError::Condition)?;
                let defiance = d
                    .defiance
                    .map(|t| reg.trait_id(&t).ok_or(NormError::UnknownTrait(t)))
                    .transpose()?;
                Ok(Norm {
                    act,
                    modality: d.modality,
                    weight: d.weight,
                    when,
                    defiance,
                })
            })
            .collect::<Result<Vec<_>, NormError>>()?;
        Ok(Norms(norms))
    }

    /// The norms that bear on `act` in state `s` — those regulating it whose `when`
    /// holds (an unconditional norm always holds).
    fn applicable(&self, act: (PredicateId, i64), s: &PlanState, reg: &Registry) -> Vec<&Norm> {
        self.0
            .iter()
            .filter(|n| n.act == act && n.when.as_ref().is_none_or(|c| c.satisfied(s, reg)))
            .collect()
    }

    /// Whether the prohibitions on an act are *justified* away in this context: a
    /// permission or duty in force that is **at least as specific** as the strongest
    /// prohibition. Specificity is how conditional a norm is — a `when` clause makes
    /// it more specific than a blanket rule — so "killing a tyrant is permitted"
    /// (conditional) overrides "killing is forbidden" (blanket), but a blanket
    /// "self-defence permitted" does *not* override "killing the envoy is forbidden".
    /// Ties go to the allowance: an explicit permission/duty beats an equally-broad
    /// taboo. (Specificity is the `when`-count, 0 or 1 today; the ordering already
    /// generalises to conjunctions.)
    fn justified(in_force: &[&Norm]) -> bool {
        let spec = |n: &&Norm| u32::from(n.when.is_some());
        let strongest = |keep: fn(Modality) -> bool| {
            in_force.iter().filter(|n| keep(n.modality)).map(spec).max()
        };
        let forbid = strongest(|m| m == Modality::Forbidden);
        let allow = strongest(|m| matches!(m, Modality::Permitted | Modality::Obliged));
        allow.is_some_and(|a| forbid.is_none_or(|f| a >= f))
    }

    /// The net deontic pressure on pursuing `act` in state `s`, as felt by an agent
    /// with this `personality`. Positive = the act is sanctioned (forbidden) and
    /// should be suppressed; negative = obliged (socially pressed); zero = no norm
    /// bites, the act is unregulated, or it is justified.
    ///
    /// A justified act (see [`justified`](Self::justified)) carries no prohibition
    /// pressure — but obligations still *pull* (a duty tugs whether or not a taboo
    /// also applies). Each prohibition that stands contributes its weight, scaled
    /// down by how much the agent's `defiance` trait resists it.
    pub fn sanction(
        &self,
        act: Option<(PredicateId, i64)>,
        s: &PlanState,
        reg: &Registry,
        personality: &[Fx],
    ) -> Fx {
        let Some(act) = act else { return Fx::ZERO };
        let in_force = self.applicable(act, s, reg);
        let justified = Self::justified(&in_force);
        let mut pressure = Fx::ZERO;
        for n in &in_force {
            // Authored weight is a finite `f32` literal — converting once here is exact.
            let weight = Fx::from_num(n.weight);
            match n.modality {
                Modality::Forbidden if !justified => {
                    let defiance = n
                        .defiance
                        .and_then(|t| personality.get(t))
                        .copied()
                        .unwrap_or(Fx::ZERO);
                    pressure += weight * (Fx::ONE - defiance.clamp(Fx::ZERO, Fx::ONE));
                }
                Modality::Obliged => pressure -= weight,
                _ => {}
            }
        }
        pressure
    }

    /// Whether `act` is socially **forbidden** here: an unjustified prohibition is in
    /// force — *regardless of the actor's disposition*. A defiant soul may do it
    /// anyway, but doing it is still a transgression (one the appraisal system can
    /// hold them to). This is the objective social verdict, distinct from
    /// [`sanction`](Self::sanction)'s felt, defiance-scaled deterrence.
    pub fn forbids(&self, act: Option<(PredicateId, i64)>, s: &PlanState, reg: &Registry) -> bool {
        let Some(act) = act else { return false };
        let in_force = self.applicable(act, s, reg);
        !Self::justified(&in_force) && in_force.iter().any(|n| n.modality == Modality::Forbidden)
    }
}

// --- Authored form (verb / predicate / trait names not yet resolved) ---

#[derive(Deserialize)]
struct NormDef {
    /// The verb whose effect this norm regulates.
    act: String,
    modality: Modality,
    #[serde(default = "unit_weight")]
    weight: f32,
    /// The situation it applies in (omit for "always").
    #[serde(default)]
    when: Option<ConditionDef>,
    /// A personality trait that resists it (omit for "inescapable").
    #[serde(default)]
    defiance: Option<String>,
}

fn unit_weight() -> f32 {
    1.0
}

/// Why loading norms failed.
#[derive(Debug)]
pub enum NormError {
    Config(config::ConfigError),
    UnknownVerb(String),
    UnknownTrait(String),
    /// A `when` condition failed to resolve (unknown predicate/good/…).
    Condition(GoalError),
}

impl std::fmt::Display for NormError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NormError::Config(e) => write!(f, "loading norms: {e}"),
            NormError::UnknownVerb(n) => write!(f, "norm regulates unknown verb '{n}'"),
            NormError::UnknownTrait(n) => write!(f, "norm defied by unknown trait '{n}'"),
            NormError::Condition(e) => write!(f, "norm's `when`: {e}"),
        }
    }
}
impl std::error::Error for NormError {}
impl From<config::ConfigError> for NormError {
    fn from(e: config::ConfigError) -> Self {
        NormError::Config(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Facts, Stock};
    use game_sim::Coord;

    /// A bare state with a `facts` vector long enough for the bundled predicates.
    fn state(reg: &Registry, facts: Facts) -> PlanState {
        PlanState {
            sustenance: 100,
            rest: 100,
            money: 0,
            stock: Stock::from_elem(0u32, reg.good_count()),
            pos: Coord::new(0, 0),
            facts,
            learned: Stock::new(),
        }
    }

    /// The grounded fact slot a verb's effect lands on for the given role.
    fn slot(reg: &Registry, verb: &str, role: usize) -> usize {
        let (p, _) = reg.verb(verb).unwrap();
        crate::data::fact_slot(p, role)
    }

    fn empty_facts(reg: &Registry) -> Facts {
        Facts::from_elem(0i64, reg.predicate_count() * crate::data::ROLE_COUNT)
    }

    #[test]
    fn no_norms_means_no_sanction() {
        let reg = Registry::bundled();
        let norms = Norms::default();
        let s = state(&reg, empty_facts(&reg));
        assert_eq!(norms.sanction(reg.verb("avenge"), &s, &reg, &[]), Fx::ZERO);
    }

    #[test]
    fn a_taboo_sanctions_the_act() {
        // Killing is forbidden: pursuing `avenge` carries a unit sanction.
        let reg = Registry::bundled();
        let norms = Norms::from_ron(r#"[(act: "avenge", modality: Forbidden)]"#, &reg).unwrap();
        let s = state(&reg, empty_facts(&reg));
        assert_eq!(norms.sanction(reg.verb("avenge"), &s, &reg, &[]), Fx::ONE);
        // ...but it says nothing about an unrelated act.
        assert_eq!(norms.sanction(reg.verb("rule"), &s, &reg, &[]), Fx::ZERO);
    }

    #[test]
    fn a_permission_justifies_the_act() {
        // Killing is forbidden — unless the foe sits the throne (a tyrant): then
        // vengeance is permitted and the sanction lifts.
        let reg = Registry::bundled();
        let norms = Norms::from_ron(
            r#"[
                (act: "avenge", modality: Forbidden),
                (act: "avenge", modality: Permitted, when: Some(Relation(predicate: "enthroned", subject: Foe, equals: 1))),
            ]"#,
            &reg,
        )
        .unwrap();
        // Foe is a commoner: the taboo stands.
        let s = state(&reg, empty_facts(&reg));
        assert_eq!(norms.sanction(reg.verb("avenge"), &s, &reg, &[]), Fx::ONE);
        // Foe holds the throne: justified, sanction gone.
        let mut facts = empty_facts(&reg);
        facts[slot(&reg, "rule", 1)] = 1; // enthroned(foe) = 1
        let s = state(&reg, facts);
        assert_eq!(norms.sanction(reg.verb("avenge"), &s, &reg, &[]), Fx::ZERO);
    }

    #[test]
    fn defiance_lets_the_vengeful_break_the_taboo() {
        // The taboo is resisted by vengeance: the meek feel its full weight, the
        // vengeful barely feel it.
        let reg = Registry::bundled();
        let norms = Norms::from_ron(
            r#"[(act: "avenge", modality: Forbidden, defiance: Some("vengeance"))]"#,
            &reg,
        )
        .unwrap();
        let s = state(&reg, empty_facts(&reg));
        let mut meek = vec![Fx::ZERO; reg.trait_count()];
        meek[reg.trait_id("vengeance").unwrap()] = Fx::ZERO;
        let mut vengeful = vec![Fx::ZERO; reg.trait_count()];
        vengeful[reg.trait_id("vengeance").unwrap()] = Fx::ONE;
        assert_eq!(norms.sanction(reg.verb("avenge"), &s, &reg, &meek), Fx::ONE);
        assert_eq!(
            norms.sanction(reg.verb("avenge"), &s, &reg, &vengeful),
            Fx::ZERO
        );
    }

    #[test]
    fn an_obligation_presses_the_act() {
        // A duty pulls the act toward being done — a negative sanction.
        let reg = Registry::bundled();
        let norms =
            Norms::from_ron(r#"[(act: "rule", modality: Obliged, weight: 0.5)]"#, &reg).unwrap();
        let s = state(&reg, empty_facts(&reg));
        assert_eq!(
            norms.sanction(reg.verb("rule"), &s, &reg, &[]),
            Fx::from_num(-0.5)
        );
    }

    #[test]
    fn a_specific_prohibition_overrides_a_general_permission() {
        // Killing is broadly permitted, but killing the king is specifically
        // forbidden — the specific rule wins where it applies.
        let reg = Registry::bundled();
        let norms = Norms::from_ron(
            r#"[
                (act: "avenge", modality: Permitted),
                (act: "avenge", modality: Forbidden, when: Some(Relation(predicate: "enthroned", subject: Foe, equals: 1))),
            ]"#,
            &reg,
        )
        .unwrap();
        // Foe is a commoner: only the general permission applies — free to act.
        let s = state(&reg, empty_facts(&reg));
        assert_eq!(norms.sanction(reg.verb("avenge"), &s, &reg, &[]), Fx::ZERO);
        assert!(!norms.forbids(reg.verb("avenge"), &s, &reg));
        // Foe is the king: the specific prohibition overrides the broad permission.
        let mut facts = empty_facts(&reg);
        facts[slot(&reg, "rule", 1)] = 1; // enthroned(foe) = 1
        let s = state(&reg, facts);
        assert_eq!(norms.sanction(reg.verb("avenge"), &s, &reg, &[]), Fx::ONE);
        assert!(norms.forbids(reg.verb("avenge"), &s, &reg));
    }

    #[test]
    fn a_duty_overrides_the_taboo_and_pulls() {
        // Killing is forbidden, but one is obliged to avenge a wrong while the foe
        // lives — the specific duty lifts the taboo and tugs the act forward.
        let reg = Registry::bundled();
        let norms = Norms::from_ron(
            r#"[
                (act: "avenge", modality: Forbidden, weight: 1.0),
                (act: "avenge", modality: Obliged, weight: 0.4, when: Some(Relation(predicate: "alive", subject: Foe, equals: 1))),
            ]"#,
            &reg,
        )
        .unwrap();
        // Foe dead: duty discharged, only the blanket taboo stands.
        let s = state(&reg, empty_facts(&reg));
        assert_eq!(norms.sanction(reg.verb("avenge"), &s, &reg, &[]), Fx::ONE);
        assert!(norms.forbids(reg.verb("avenge"), &s, &reg));
        // Foe alive: the duty applies, justifies the act (no longer forbidden), and
        // leaves a net negative pressure (a pull).
        let mut facts = empty_facts(&reg);
        facts[slot(&reg, "avenge", 1)] = 1; // alive(foe) = 1
        let s = state(&reg, facts);
        // The authored weight is parsed as `f32`, so the expected sanction must convert from the
        // same `f32` value (0.4 has no exact binary form, and f32 ≠ f64 there).
        assert_eq!(
            norms.sanction(reg.verb("avenge"), &s, &reg, &[]),
            Fx::from_num(-0.4f32)
        );
        assert!(!norms.forbids(reg.verb("avenge"), &s, &reg));
    }

    #[test]
    fn forbids_ignores_disposition() {
        // A vengeful soul barely *feels* the taboo (sanction near zero), yet the act
        // is still socially forbidden — a transgression, however willing the hand.
        let reg = Registry::bundled();
        let norms = Norms::from_ron(
            r#"[(act: "avenge", modality: Forbidden, defiance: Some("vengeance"))]"#,
            &reg,
        )
        .unwrap();
        let s = state(&reg, empty_facts(&reg));
        let mut vengeful = vec![Fx::ZERO; reg.trait_count()];
        vengeful[reg.trait_id("vengeance").unwrap()] = Fx::ONE;
        assert_eq!(
            norms.sanction(reg.verb("avenge"), &s, &reg, &vengeful),
            Fx::ZERO,
            "feels no deterrence"
        );
        assert!(
            norms.forbids(reg.verb("avenge"), &s, &reg),
            "but it is still forbidden"
        );
    }

    #[test]
    fn an_unknown_verb_in_a_norm_is_rejected() {
        let reg = Registry::bundled();
        assert!(matches!(
            Norms::from_ron(r#"[(act: "teleport", modality: Forbidden)]"#, &reg),
            Err(NormError::UnknownVerb(_))
        ));
    }

    #[test]
    fn the_bundled_archon_law_loads_and_resolves() {
        // The shipped `norms.ron` carries the (dormant) Law of the Archons — valid,
        // bundled scenario data, switched on via `Setup { norms: Norms::bundled(&reg), .. }`
        // rather than active by default. It must parse and resolve against the bundled
        // registry (its verbs/predicates/traits), and it must encode the cardinal taboo:
        // waking another soul is forbidden.
        let reg = Registry::bundled();
        let law = Norms::bundled(&reg);
        assert!(
            !law.0.is_empty(),
            "the Archon Law should be authored in norms.ron"
        );
        let s = state(&reg, empty_facts(&reg));
        assert!(
            law.sanction(reg.verb("awaken"), &s, &reg, &[]) > Fx::ZERO,
            "under the Law, awakening another should carry a sanction"
        );
    }
}
