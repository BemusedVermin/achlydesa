//! The **RPG layer**: a Worlds Without Number character model (attributes, skills, feats
//! "Foci", saving throws) grafted with Cities Without Number "Edges", as data-driven ECS
//! components plus a *deterministic* (no-dice) check engine.
//!
//! This crate is self-contained: it defines the components and the rules, parses its
//! authored content via [`config`], and rolls characters from a seeded [`sim::Rng`]. It
//! does **not** touch the agent world — the assembler (`agents`) attaches these components
//! to NPCs and the avatar, and runs checks against them. Off by default: a world the
//! assembler never stamps with these components is byte-identical to one before this layer.
//!
//! - **Attributes** ([`Abilities`]) — the six WWN scores; the flat ±2 [`wwn_mod`] table.
//! - **Skills** ([`Proficiencies`]) — the 21 WWN skills, ranks −1..4. Separate from the
//!   economy `Skills` (callings/yield) in `agent_core` — this is the adventuring layer.
//! - **Foci** ([`FociHeld`]) — two-level feats; non-combat ones act now, combat ones are
//!   authored but inert until the combat layer ships.
//! - **Edges / grants** ([`Grant`]) — broad archetype bundles; the job-system-friendly seam
//!   future combat "jobs" reuse. A reserved [`PowerTier`] (xianxia cultivation) waits for them.
//! - **Checks** ([`check`]) — `attr_mod + skill + situational ≥ difficulty`, graded by margin.

use bevy_ecs::prelude::*;
use config::{Asset, Config, ConfigError};
use serde::Deserialize;
use sim::Rng;
use std::collections::{HashMap, HashSet};

// --- Attributes (fixed WWN set; their index is their id) ---

pub const STR: usize = 0;
pub const DEX: usize = 1;
pub const CON: usize = 2;
pub const INT: usize = 3;
pub const WIS: usize = 4;
pub const CHA: usize = 5;
pub const ATTR_COUNT: usize = 6;

/// Skill-rank bounds: −1 is unskilled (a flat penalty), 4 the human cap.
pub const PROF_UNSKILLED: i8 = -1;
pub const PROF_MAX: i8 = 4;

/// The WWN difficulty ladder (roll-target numbers for [`check`]).
pub const EASY: i32 = 6;
pub const NORMAL: i32 = 8;
pub const HARD: i32 = 10;
pub const FORMIDABLE: i32 = 12;

/// Margin at/above which a successful check is a *strong* success.
pub const STRONG_MARGIN: i32 = 4;
/// WWN saving-throw base: `save = 15 − best-of-two attribute modifiers`.
pub const SAVE_BASE: i32 = 15;
/// These checks are **deterministic** — knowledge and skill decide, not luck — so instead of
/// rolling 2d6 they *take its average*. This baseline is what lets the authored WWN difficulties
/// ([`EASY`]..[`FORMIDABLE`]) read exactly as the tabletop's: without it nothing could pass
/// [`NORMAL`] (a maxed human reaches only +6), since the targets assume the ~7 the dice would add.
pub const DICE_TAKE: i32 = 7;

/// The WWN attribute-modifier table — flat and capped at ±2 (the signature of the engine):
/// `≤3 → −2`, `4–7 → −1`, `8–13 → 0`, `14–17 → +1`, `≥18 → +2`.
pub fn wwn_mod(score: i32) -> i32 {
    match score {
        ..=3 => -2,
        4..=7 => -1,
        8..=13 => 0,
        14..=17 => 1,
        _ => 2,
    }
}

// --- Components (attached to NPCs and the avatar by the assembler) ---

/// The six WWN attribute scores (3..18, modifiable past it by Edges/Foci). Their index is
/// their id ([`STR`]..[`CHA`]); modifiers come from [`wwn_mod`].
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Abilities {
    pub scores: [i32; ATTR_COUNT],
}

/// The three WWN saving throws, each defended by the better of two attributes.
#[derive(Clone, Copy, Debug)]
pub enum Save {
    Physical,
    Evasion,
    Mental,
}

impl Abilities {
    /// The modifier for attribute `attr` ([`STR`]..[`CHA`]).
    pub fn modifier(&self, attr: usize) -> i32 {
        wwn_mod(self.scores[attr])
    }

    /// A saving-throw target number: `15 − best-of-two modifiers` (lower is better).
    pub fn save(&self, save: Save) -> i32 {
        let (a, b) = match save {
            Save::Physical => (STR, CON),
            Save::Evasion => (DEX, INT),
            Save::Mental => (WIS, CHA),
        };
        SAVE_BASE - self.modifier(a).max(self.modifier(b))
    }
}

/// Per-WWN-skill proficiency rank (−1..4), indexed by [`RpgData`] skill id. The adventuring
/// skill layer — distinct from the economy `Skills` (which gate trades / scale yield).
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Proficiencies {
    pub ranks: Vec<i8>,
}

impl Proficiencies {
    /// Rank in skill `id`; `−1` (unskilled) if out of range.
    pub fn rank(&self, id: usize) -> i8 {
        self.ranks.get(id).copied().unwrap_or(PROF_UNSKILLED)
    }
}

/// Per-focus level held (0 = none, 1, 2), indexed by [`RpgData`] focus id.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct FociHeld {
    pub levels: Vec<u8>,
}

impl FociHeld {
    /// Level held in focus `id` (0 if none/out of range).
    pub fn level(&self, id: usize) -> u8 {
        self.levels.get(id).copied().unwrap_or(0)
    }
}

/// Free-form capability flags a character carries (e.g. `"climbing_proficient"`, `"literate"`),
/// granted by Edges/Foci. The cheap, extensible seam other layers read (travel gates, etc.).
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct Flags(pub HashSet<String>);

impl Flags {
    pub fn has(&self, flag: &str) -> bool {
        self.0.contains(flag)
    }
}

/// Reserved cultivation / power tier (xianxia). `0` = mortal. Unused now; the deferred combat
/// layer reads and escalates it.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerTier(pub u8);

/// The archetype Edge a character was rolled with (index into [`RpgData::edges`]), if any —
/// the broad "what they are" label, handy for display.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Archetype(pub Option<usize>);

// --- The deterministic check engine ---

/// The outcome of a [`check`], carrying its margin (success amount).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckOutcome {
    Fail(i32),
    Pass(i32),
    Strong(i32),
}

impl CheckOutcome {
    pub fn succeeded(self) -> bool {
        !matches!(self, CheckOutcome::Fail(_))
    }

    /// Signed margin: `(attr_mod + skill + situational + DICE_TAKE) − difficulty`.
    pub fn margin(self) -> i32 {
        match self {
            CheckOutcome::Fail(m) | CheckOutcome::Pass(m) | CheckOutcome::Strong(m) => m,
        }
    }

    /// A discrete strength multiplier for scaling an effect by the result (used by the
    /// dialogue seam): fail 0.0, pass 1.0, strong 1.5.
    pub fn strength(self) -> f32 {
        match self {
            CheckOutcome::Fail(_) => 0.0,
            CheckOutcome::Pass(_) => 1.0,
            CheckOutcome::Strong(_) => 1.5,
        }
    }
}

/// Resolve a skill check **deterministically** (no dice — knowledge/skill, not luck, decides,
/// matching the discovery system): succeed iff `attr_mod + skill + situational + DICE_TAKE ≥
/// difficulty` (the [`DICE_TAKE`] baseline standing in for the average 2d6), graded into
/// [`CheckOutcome::Strong`] at a margin of [`STRONG_MARGIN`].
pub fn check(attr_mod: i32, skill: i8, situational: i32, difficulty: i32) -> CheckOutcome {
    let margin = attr_mod + skill as i32 + situational + DICE_TAKE - difficulty;
    if margin < 0 {
        CheckOutcome::Fail(margin)
    } else if margin >= STRONG_MARGIN {
        CheckOutcome::Strong(margin)
    } else {
        CheckOutcome::Pass(margin)
    }
}

// --- Grant bundles (Edges, Foci levels, and future combat "jobs") ---

/// One effect an Edge / Focus level stamps onto a character. The generic, data-driven seam
/// that makes the advancement model job-system-friendly.
#[derive(Deserialize, Clone, Debug)]
pub enum Grant {
    /// Raise a skill's rank by `by` (clamped to −1..4).
    SkillRank { skill: String, by: i8 },
    /// Add `by` to an attribute score.
    AttrBonus { attr: String, by: i32 },
    /// Grant a focus at (at least) `level`.
    Focus { focus: String, level: u8 },
    /// Set a capability flag.
    Flag(String),
    /// Raise the (deferred) power tier to at least this.
    PowerTier(u8),
}

// --- Authored content (RON DTOs, stored as-is; grants resolved at apply time) ---

#[derive(Deserialize, Clone, Debug)]
struct AttrDef {
    name: String,
}

/// One WWN skill, tagged by interaction class so the assembler can find the social / world
/// skills the early game prioritises (combat skills are neither, inert until combat).
#[derive(Deserialize, Clone, Debug)]
pub struct SkillInfo {
    pub name: String,
    #[serde(default)]
    pub social: bool,
    #[serde(default)]
    pub world: bool,
}

/// One level of a [`Focus`] — its grants and a one-line description.
#[derive(Deserialize, Clone, Debug)]
pub struct FocusLevel {
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub grants: Vec<Grant>,
}

/// A WWN Focus (feat): up to two levels, tagged combat or not.
#[derive(Deserialize, Clone, Debug)]
pub struct Focus {
    pub name: String,
    #[serde(default)]
    pub combat: bool,
    pub levels: Vec<FocusLevel>,
}

/// A Cities-Without-Number "Edge": a broad archetype bundle of grants.
#[derive(Deserialize, Clone, Debug)]
pub struct Edge {
    pub name: String,
    #[serde(default)]
    pub desc: String,
    pub grants: Vec<Grant>,
}

// --- The resolved registry (shared as a resource) ---

/// The resolved RPG content set: attributes, skills, foci, edges, with names interned to
/// dense ids and every grant cross-reference validated. Mirrors `agent_core`'s `Registry`.
#[derive(Resource, Clone, Debug)]
pub struct RpgData {
    attr_names: Vec<String>,
    attr_ids: HashMap<String, usize>,
    skills: Vec<SkillInfo>,
    skill_ids: HashMap<String, usize>,
    foci: Vec<Focus>,
    focus_ids: HashMap<String, usize>,
    edges: Vec<Edge>,
    edge_ids: HashMap<String, usize>,
}

fn index<'a>(names: impl IntoIterator<Item = &'a String>) -> HashMap<String, usize> {
    names
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect()
}

impl RpgData {
    /// The content baked into the binary — the shipping default set.
    pub fn bundled() -> Self {
        Self::load(&Config::bundled()).expect("bundled rpg data is valid")
    }

    /// Load and resolve the RPG content from a [`Config`]'s source.
    pub fn load(cfg: &Config) -> Result<Self, LoadError> {
        let attrs: Vec<AttrDef> = cfg.load(Asset::Attributes)?;
        let skills: Vec<SkillInfo> = cfg.load(Asset::RpgSkills)?;
        let foci: Vec<Focus> = cfg.load(Asset::Foci)?;
        let edges: Vec<Edge> = cfg.load(Asset::Edges)?;
        Self::resolve(attrs, skills, foci, edges)
    }

    fn resolve(
        attrs: Vec<AttrDef>,
        skills: Vec<SkillInfo>,
        foci: Vec<Focus>,
        edges: Vec<Edge>,
    ) -> Result<Self, LoadError> {
        if attrs.len() != ATTR_COUNT {
            return Err(LoadError::Attributes(format!(
                "expected {ATTR_COUNT} attributes, got {}",
                attrs.len()
            )));
        }
        let attr_names: Vec<String> = attrs.into_iter().map(|a| a.name).collect();
        let attr_ids = index(&attr_names);
        let skill_ids = index(skills.iter().map(|s| &s.name));
        let focus_ids = index(foci.iter().map(|f| &f.name));
        let edge_ids = index(edges.iter().map(|e| &e.name));
        let data = Self {
            attr_names,
            attr_ids,
            skills,
            skill_ids,
            foci,
            focus_ids,
            edges,
            edge_ids,
        };
        data.validate()?;
        Ok(data)
    }

    /// Every grant in every Focus level and Edge must name a known skill / attribute / focus.
    fn validate(&self) -> Result<(), LoadError> {
        let one = |g: &Grant| -> Result<(), LoadError> {
            let ok = match g {
                Grant::SkillRank { skill, .. } => self.skill_ids.contains_key(skill),
                Grant::AttrBonus { attr, .. } => self.attr_ids.contains_key(attr),
                Grant::Focus { focus, .. } => self.focus_ids.contains_key(focus),
                Grant::Flag(_) | Grant::PowerTier(_) => true,
            };
            ok.then_some(())
                .ok_or_else(|| LoadError::UnknownRef(format!("{g:?}")))
        };
        for f in &self.foci {
            for lvl in &f.levels {
                for g in &lvl.grants {
                    one(g)?;
                }
            }
        }
        for e in &self.edges {
            for g in &e.grants {
                one(g)?;
            }
        }
        Ok(())
    }

    pub fn skill_count(&self) -> usize {
        self.skills.len()
    }
    pub fn focus_count(&self) -> usize {
        self.foci.len()
    }
    pub fn skill_id(&self, name: &str) -> Option<usize> {
        self.skill_ids.get(name).copied()
    }
    pub fn attr_id(&self, name: &str) -> Option<usize> {
        self.attr_ids.get(name).copied()
    }
    pub fn focus_id(&self, name: &str) -> Option<usize> {
        self.focus_ids.get(name).copied()
    }
    pub fn edge_id(&self, name: &str) -> Option<usize> {
        self.edge_ids.get(name).copied()
    }
    pub fn skill_name(&self, id: usize) -> &str {
        &self.skills[id].name
    }
    pub fn attr_name(&self, id: usize) -> &str {
        &self.attr_names[id]
    }
    pub fn is_social(&self, id: usize) -> bool {
        self.skills.get(id).is_some_and(|s| s.social)
    }
    pub fn is_world(&self, id: usize) -> bool {
        self.skills.get(id).is_some_and(|s| s.world)
    }
    pub fn skills(&self) -> &[SkillInfo] {
        &self.skills
    }
    pub fn foci(&self) -> &[Focus] {
        &self.foci
    }
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }
    pub fn edge_name(&self, id: usize) -> &str {
        &self.edges[id].name
    }

    /// Stamp a bundle of [`Grant`]s onto a rolled character (used for Edges and Foci).
    pub fn apply_grants(&self, grants: &[Grant], out: &mut Rolled) {
        for g in grants {
            match g {
                Grant::SkillRank { skill, by } => {
                    if let Some(&id) = self.skill_ids.get(skill) {
                        out.proficiencies.ranks[id] =
                            (out.proficiencies.ranks[id] + by).clamp(PROF_UNSKILLED, PROF_MAX);
                    }
                }
                Grant::AttrBonus { attr, by } => {
                    if let Some(&id) = self.attr_ids.get(attr) {
                        out.abilities.scores[id] += by;
                    }
                }
                Grant::Focus { focus, level } => {
                    if let Some(&id) = self.focus_ids.get(focus) {
                        out.foci.levels[id] = out.foci.levels[id].max(*level);
                    }
                }
                Grant::Flag(f) => {
                    out.flags.0.insert(f.clone());
                }
                Grant::PowerTier(t) => {
                    out.power.0 = out.power.0.max(*t);
                }
            }
        }
    }
}

// --- Character generation (the one place this crate draws randomness) ---

/// A freshly rolled character's components, ready for the assembler to insert onto an entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rolled {
    pub abilities: Abilities,
    pub proficiencies: Proficiencies,
    pub foci: FociHeld,
    pub flags: Flags,
    pub power: PowerTier,
    /// The archetype Edge stamped on this character, if any (index into [`RpgData::edges`]).
    pub edge: Option<usize>,
}

fn roll_3d6(rng: &mut dyn Rng) -> i32 {
    (0..3).map(|_| 1 + rng.gen_range(6) as i32).sum()
}

/// Roll a character from `rng`: 3d6 per attribute, a random archetype Edge applied, and a
/// couple of background skill bumps for within-archetype variety. Deterministic for a given
/// `rng` sequence — the caller seeds a dedicated stream so this never perturbs other layers.
pub fn roll(rng: &mut dyn Rng, data: &RpgData) -> Rolled {
    let mut scores = [0i32; ATTR_COUNT];
    for s in &mut scores {
        *s = roll_3d6(rng);
    }
    let mut out = Rolled {
        abilities: Abilities { scores },
        proficiencies: Proficiencies {
            ranks: vec![PROF_UNSKILLED; data.skills.len()],
        },
        foci: FociHeld {
            levels: vec![0; data.foci.len()],
        },
        flags: Flags::default(),
        power: PowerTier(0),
        edge: None,
    };
    if !data.edges.is_empty() {
        let e = rng.gen_range(data.edges.len());
        data.apply_grants(&data.edges[e].grants, &mut out);
        out.edge = Some(e);
    }
    for _ in 0..2 {
        if !data.skills.is_empty() {
            let s = rng.gen_range(data.skills.len());
            out.proficiencies.ranks[s] =
                (out.proficiencies.ranks[s] + 1).clamp(PROF_UNSKILLED, PROF_MAX);
        }
    }
    out
}

// --- Errors ---

#[derive(Debug)]
pub enum LoadError {
    Config(ConfigError),
    Attributes(String),
    UnknownRef(String),
}

impl From<ConfigError> for LoadError {
    fn from(e: ConfigError) -> Self {
        LoadError::Config(e)
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Config(e) => write!(f, "loading rpg asset: {e}"),
            LoadError::Attributes(m) => write!(f, "attributes.ron: {m}"),
            LoadError::UnknownRef(m) => write!(f, "grant references unknown content: {m}"),
        }
    }
}

impl std::error::Error for LoadError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A standalone SplitMix64 so the rpg crate's tests need no game_sim dependency.
    struct TestRng(u64);
    impl Rng for TestRng {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
    }

    #[test]
    fn modifier_table_is_flat_and_capped() {
        assert_eq!(wwn_mod(3), -2);
        assert_eq!(wwn_mod(7), -1);
        assert_eq!(wwn_mod(8), 0);
        assert_eq!(wwn_mod(13), 0);
        assert_eq!(wwn_mod(14), 1);
        assert_eq!(wwn_mod(18), 2);
        assert_eq!(wwn_mod(25), 2, "modifier stays capped past 18");
    }

    #[test]
    fn check_grades_by_margin() {
        // With the DICE_TAKE baseline, the authored WWN ladder reads as the tabletop's:
        // an unskilled everyman clears EASY but not NORMAL, the competent clear NORMAL, and
        // FORMIDABLE wants a specialist.
        assert!(
            check(0, 0, 0, EASY).succeeded(),
            "an everyman manages an easy task"
        );
        assert!(
            matches!(check(0, 0, 0, NORMAL), CheckOutcome::Fail(-1)),
            "but not a normal one"
        );
        assert!(
            check(1, 0, 0, NORMAL).succeeded(),
            "a competent hand clears NORMAL"
        );
        assert!(
            matches!(check(2, 3, 0, NORMAL), CheckOutcome::Strong(4)),
            "a specialist aces it"
        );
        assert!(
            matches!(check(0, 0, 1, NORMAL), CheckOutcome::Pass(0)),
            "margin 0 meets the difficulty"
        );
        assert!(
            !check(2, 2, 0, FORMIDABLE).succeeded(),
            "the merely-good still fail the formidable"
        );
        assert!(
            check(2, 4, 0, FORMIDABLE).succeeded(),
            "the specialist clears the formidable"
        );
    }

    #[test]
    fn saves_take_the_better_of_two() {
        let a = Abilities {
            scores: [18, 8, 8, 8, 8, 8],
        };
        assert_eq!(
            a.save(Save::Physical),
            SAVE_BASE - 2,
            "STR 18 → +2 defends Physical"
        );
        assert_eq!(a.save(Save::Mental), SAVE_BASE, "WIS/CHA 8 → +0");
    }

    #[test]
    fn bundled_data_loads_and_validates() {
        let d = RpgData::bundled();
        assert_eq!(d.attr_names.len(), ATTR_COUNT);
        assert!(d.skill_count() >= 20, "the full WWN skill list");
        assert!(!d.foci().is_empty() && !d.edges().is_empty());
        let convince = d.skill_id("Convince").expect("Convince exists");
        let survive = d.skill_id("Survive").expect("Survive exists");
        assert!(d.is_social(convince) && d.is_world(survive));
    }

    #[test]
    fn roll_is_deterministic_and_applies_an_edge() {
        let d = RpgData::bundled();
        let a = roll(&mut TestRng(42), &d);
        let b = roll(&mut TestRng(42), &d);
        assert_eq!(a, b, "same seed → identical character");
        assert_eq!(a.proficiencies.ranks.len(), d.skill_count());
        assert!(a.edge.is_some(), "an archetype edge was stamped");
        // The edge raised at least one skill above unskilled.
        assert!(a.proficiencies.ranks.iter().any(|&r| r > PROF_UNSKILLED));
    }
}
