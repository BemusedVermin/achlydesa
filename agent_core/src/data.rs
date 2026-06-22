//! Authored game data: the **goods**, **skills**, and **recipes** that define the
//! economy, loaded from RON so they're editable without touching code.
//!
//! Authoring is the whole point. A [`Registry`] is built from three RON files
//! ([`bundled`](Registry::bundled) ships defaults; [`load`](Registry::load) reads
//! a directory). At load it resolves human-friendly names into dense ids
//! ([`GoodId`], [`SkillId`]) and validates the cross-references — a recipe naming
//! an unknown good or skill is a load error, not a silent phantom. Add an entry
//! to the data files and every inventory, market, and the action scorer pick it
//! up; no Rust changes.
//!
//! Quantities and prices are whole numbers (you can't own half a loaf). Need and
//! skill values stay continuous, so `nutrition`, `gain`, and `cap` are floats.

use bevy_ecs::prelude::Resource;
use config::{Asset, Config};
use serde::Deserialize;
use std::collections::HashMap;

/// Index of a good (and of every inventory / stock `Vec`).
pub type GoodId = usize;
/// Index of a skill (and of every NPC's skill `Vec`).
pub type SkillId = usize;

/// A natural resource on the land a recipe can draw on — each maps to a substrate
/// field. Output scales with its level, and (if the recipe depletes) it is drawn
/// down at the tile. Authored by variant name in RON (`resource: Some(Minerals)`).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    /// The land's crop-growing capacity (`carrying_capacity`). Climate-renewable.
    Fertility,
    /// Standing plant matter (`plant_biomass`). Depletes when harvested.
    Vegetation,
    /// Ore richness in the rock (`minerals`). Finite; depletes when mined.
    Minerals,
    /// Standing surface water.
    Water,
}

impl ResourceKind {
    pub const COUNT: usize = 4;
    pub fn idx(self) -> usize {
        self as usize
    }
}

/// A good as authored in `goods.ron`.
#[derive(Deserialize, Clone, Debug)]
pub struct GoodDef {
    pub name: String,
    /// Coins per unit when stock sits at `target_stock`.
    pub base_price: i64,
    /// Stock level the price is anchored to.
    pub target_stock: i64,
    /// Sustenance restored per unit eaten (a need meter, so continuous); `0` =
    /// inedible.
    pub nutrition: f32,
}

/// A skill as authored in `skills.ron`.
#[derive(Deserialize, Clone, Debug)]
pub struct SkillDef {
    pub name: String,
    /// Proficiency gained each time the skill is used.
    pub gain: f32,
    /// Maximum proficiency.
    pub cap: f32,
}

/// Index of a personality trait (and of every agent's `Personality` vector).
pub type TraitId = usize;
/// Index of a mood/emotion (and of every agent's `Mood` vector).
pub type MoodId = usize;
/// Index of a relational predicate (`enthroned`, `alive`, …). Goals are
/// conditions on predicates over entities; the planner grounds them to flat facts.
pub type PredicateId = usize;
/// Index of a dramatic **register** (and the key of the director's `reg_heat`). The director's
/// emotional-key vocabulary, authored in `registers.ron` and interned here like every other content
/// id — so a new register is pure data (see [`RegisterDef`]).
pub type RegisterId = usize;

/// How many entity *roles* a relational goal can bind — `self` (0) and one target
/// (1, e.g. the foe). A relation `pred(role)` grounds to the flat fact slot
/// [`fact_slot`]; an agent's `facts` vector is `predicate_count * ROLE_COUNT` long.
pub const ROLE_COUNT: usize = 2;

/// The flat fact slot that `predicate(role)` grounds to (role: 0 = self, 1 = target).
pub fn fact_slot(predicate: PredicateId, role: usize) -> usize {
    predicate * ROLE_COUNT + role
}

/// A personality trait / motive as authored in `traits.ron` — a continuous,
/// *innate* drive (ambition, vengeance, …) a goal's appeal can read. Unlike a
/// skill, a trait does not decay or grow by repetition: it's who you are. It's set
/// near `baseline` at birth (varied by `spread`) and changes only through
/// significant life events (see the appraisal system), which persist. Fast,
/// transient feeling is the separate mood layer, not this.
#[derive(Deserialize, Clone, Debug)]
pub struct TraitDef {
    pub name: String,
    /// The innate value of this drive across the population.
    pub baseline: f32,
    /// How much an individual's birth value varies around `baseline` (±).
    pub spread: f32,
    /// The opposing trait, if any: an event that raises this one lowers its
    /// opposite by the same amount (ambition ↔ contentment, vengeance ↔ forgiveness).
    #[serde(default)]
    pub opposes: Option<String>,
}

/// A mood/emotion as authored in `moods.ron` — a *transient* feeling (anger, joy,
/// …) layered on top of the stable traits. Unlike a trait it rests at zero, spikes
/// when an event is appraised, and fades back each step at `decay`. A goal's appeal
/// can read it (`Mood("anger")`) to weight behaviour by how the agent feels *now*.
#[derive(Deserialize, Clone, Debug)]
pub struct MoodDef {
    pub name: String,
    /// Fraction of the feeling that fades each step (0 = never, 1 = instantly).
    pub decay: f32,
    /// The opposing feeling, if any: a spike in this one damps its opposite, so an
    /// agent can't be furious and serene at once (mood coherence, à la PAD).
    #[serde(default)]
    pub opposes: Option<String>,
    /// A trait this feeling slowly *reshapes* when sustained — how mood, over a
    /// life, settles into character (a life of anger breeds vengeance). Persistent,
    /// not decaying; this is nurture, distinct from the fast mood itself.
    #[serde(default)]
    pub shapes: Option<String>,
    /// How fast `shapes` shifts that trait, per step, scaled by the mood's level.
    #[serde(default)]
    pub shape_rate: f32,
}

/// A verb as authored in `verbs.ron` — a named action whose achievement means
/// making a relation hold of its *target*: `avenge` = make `alive(target)` false,
/// `rule` = make `enthroned(target)` true. The surface grammar's vocabulary: a goal
/// can be written `(verb: "avenge", target: Foe)` instead of a raw relation.
#[derive(Deserialize, Clone, Debug)]
pub struct VerbDef {
    pub name: String,
    /// The predicate the verb sets on its target.
    pub predicate: String,
    /// The value it sets it to (`0` = false, `1` = true, …).
    pub value: i64,
}

/// How a thread pins its counterpart (the figure it grooms then reverses), by register — the
/// director's casting policy. A small **closed generic vocabulary** (unlike the open register set):
/// each variant names a distinct selection algorithm in `director::pick_other`, so adding one is a
/// real behavioural change, not content. Registers *name* their policy in data (`casting: Coldest`).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Casting {
    /// The warmest soul (a beloved) — the default; a bond to build then break.
    #[default]
    Warmest,
    /// The coldest (a foe already turned away) — vengeance, persecution.
    Coldest,
    /// The most ambitious (a would-be usurper) — ambition, war.
    Ambitious,
    /// The most pious (a believer/elder) — wonder, reunion.
    Pious,
}

/// A register as authored in `registers.ron` (its `seeds` cross-reference not yet resolved).
#[derive(Deserialize, Clone, Debug)]
struct RegisterDto {
    name: String,
    #[serde(default)]
    spine: bool,
    #[serde(default)]
    trunk: bool,
    #[serde(default)]
    bright: bool,
    #[serde(default)]
    seeds: Option<String>,
    #[serde(default)]
    casting: Casting,
    #[serde(default = "storied_epithet")]
    epithet_lead: String,
    #[serde(default = "storied_epithet")]
    epithet_other: String,
    #[serde(default = "storied_situation")]
    situation_lead: String,
    #[serde(default = "storied_situation")]
    situation_other: String,
    #[serde(default = "default_noun")]
    noun: String,
    #[serde(default = "default_told")]
    told: String,
    #[serde(default = "default_quest_plea")]
    quest_plea: String,
    #[serde(default = "default_quest_objective")]
    quest_objective: String,
}

// The generic fall-throughs a register left untuned inherits — the old `storied`/`_ =>` arms, now
// serde defaults, so a *new* register added with only its structural fields still reads sensibly.
fn storied_epithet() -> String {
    "the Storied".into()
}
fn storied_situation() -> String {
    "a story heavy on its shoulders.".into()
}
fn default_noun() -> String {
    "strange turn".into()
}
fn default_told() -> String {
    "They say {lead} is caught up in a {noun}.".into()
}
fn default_quest_plea() -> String {
    "\"Find {other} for me — there is a matter between us.\"".into()
}
fn default_quest_objective() -> String {
    "Find {other} for {giver}.".into()
}

/// A dramatic **register** resolved into the registry — the director's emotional-key *domain
/// object*, separated from its instances (the `registers.ron` rows). Every field that the director
/// once read off a hardcoded `match` on a `Register` enum now lives here as data: the structural
/// levers (`spine`/`trunk`/`bright`/`seeds`/`casting`) the deterministic tick reads, and the surface
/// vocabulary (epithet/situation/`noun`/`told`/quest prose) the view renders.
#[derive(Clone, Debug)]
pub struct RegisterDef {
    pub name: String,
    /// Spine-eligible — a thread can take this register as its key. The *order* of the spine
    /// registers in the file is the director's rotation order (see [`Registry::spines`]).
    pub spine: bool,
    /// The betrayal→vengeance trunk the world turns on — a standing drama bonus.
    pub trunk: bool,
    /// A brighter staged experience (love/triumph/awe), groomed so the fall has something to break.
    pub bright: bool,
    /// The register a *closing* thread of this one seeds next (grief → vengeance), if any.
    pub seeds: Option<RegisterId>,
    /// How a thread of this register pins its counterpart.
    pub casting: Casting,
    pub epithet_lead: String,
    pub epithet_other: String,
    pub situation_lead: String,
    pub situation_other: String,
    /// The plain noun the world gives this drama when it lacks the particulars.
    pub noun: String,
    /// The high-fidelity gossip sentence — `{lead}`/`{other}`/`{noun}` placeholders.
    pub told: String,
    /// The giver's spoken charge / the HUD objective — `{giver}`/`{other}` placeholders.
    pub quest_plea: String,
    pub quest_objective: String,
}

impl RegisterDef {
    /// The earned epithet for the lead (`is_lead`) or the pinned other.
    pub fn epithet(&self, is_lead: bool) -> &str {
        if is_lead {
            &self.epithet_lead
        } else {
            &self.epithet_other
        }
    }
    /// The one-line situational opener for the lead or the pinned other.
    pub fn situation(&self, is_lead: bool) -> &str {
        if is_lead {
            &self.situation_lead
        } else {
            &self.situation_other
        }
    }
}

/// The raw RON documents the game data is built from. Construct with the fields you
/// have and `..Default::default()` for the rest (each defaults to an empty list).
pub struct DataFiles<'a> {
    pub goods: &'a str,
    pub skills: &'a str,
    pub recipes: &'a str,
    pub traits: &'a str,
    pub moods: &'a str,
    pub predicates: &'a str,
    pub verbs: &'a str,
    pub registers: &'a str,
}

impl Default for DataFiles<'_> {
    fn default() -> Self {
        Self {
            goods: "[]",
            skills: "[]",
            recipes: "[]",
            traits: "[]",
            moods: "[]",
            predicates: "[]",
            verbs: "[]",
            registers: "[]",
        }
    }
}

/// A recipe as authored in `recipes.ron` (good/skill names not yet resolved).
#[derive(Deserialize, Clone, Debug)]
struct RecipeDef {
    name: String,
    skill: String,
    inputs: Vec<(String, u32)>,
    outputs: Vec<(String, u32)>,
    /// Natural resource the recipe draws on, if any (crafts draw on none).
    resource: Option<ResourceKind>,
    /// Minimum resource level to attempt.
    min_resource: f32,
    /// Resource units consumed per attempt (stigmergic). `0` = read-only scaling.
    deplete: f32,
    effort: f32,
}

/// A recipe with good/skill names resolved to ids.
#[derive(Clone, Debug)]
pub struct Recipe {
    pub name: String,
    pub skill: SkillId,
    pub inputs: Vec<(GoodId, u32)>,
    pub outputs: Vec<(GoodId, u32)>,
    pub resource: Option<ResourceKind>,
    pub min_resource: f32,
    pub deplete: f32,
    pub effort: f32,
}

/// The resolved game data, shared as a resource.
#[derive(Resource, Clone, Debug)]
pub struct Registry {
    goods: Vec<GoodDef>,
    good_ids: HashMap<String, GoodId>,
    skills: Vec<SkillDef>,
    skill_ids: HashMap<String, SkillId>,
    recipes: Vec<Recipe>,
    traits: Vec<TraitDef>,
    trait_ids: HashMap<String, TraitId>,
    /// Resolved opposite of each trait (parallel to `traits`).
    trait_opposes: Vec<Option<TraitId>>,
    moods: Vec<MoodDef>,
    mood_ids: HashMap<String, MoodId>,
    /// Resolved opposite of each mood (parallel to `moods`).
    mood_opposes: Vec<Option<MoodId>>,
    /// Resolved (trait, rate) each mood slowly shapes (parallel to `moods`).
    mood_shapes: Vec<Option<(TraitId, f32)>>,
    /// Relational predicate names; their index is their fact slot when grounded.
    predicates: Vec<String>,
    predicate_ids: HashMap<String, PredicateId>,
    /// Surface-grammar verbs: name → the (predicate, value) it makes of its target.
    verbs: HashMap<String, (PredicateId, i64)>,
    /// The director's dramatic registers, resolved (the domain split from its instances).
    registers: Vec<RegisterDef>,
    register_ids: HashMap<String, RegisterId>,
    /// The spine-eligible registers, in file (= director rotation) order — the old `SPINES`.
    spine_order: Vec<RegisterId>,
}

impl Registry {
    /// The defaults shipped with the crate — the bundled content set.
    pub fn bundled() -> Self {
        Self::load(&Config::bundled()).expect("bundled data is valid")
    }

    /// Load every data RON document from a [`Config`]'s content source (the
    /// bundled defaults, a live directory, or a test set).
    pub fn load(cfg: &Config) -> Result<Self, LoadError> {
        // config does the byte-sourcing and RON parse into the DTO shapes; the
        // resolution below (names → ids, cross-validation) is ours.
        Self::resolve(
            cfg.load(Asset::Goods)?,
            cfg.load(Asset::Skills)?,
            cfg.load(Asset::Recipes)?,
            cfg.load(Asset::Traits)?,
            cfg.load(Asset::Moods)?,
            cfg.load(Asset::Predicates)?,
            cfg.load(Asset::Verbs)?,
            cfg.load(Asset::Registers)?,
        )
    }

    /// Parse and resolve the RON documents (each field is one document's text).
    pub fn from_ron(files: DataFiles) -> Result<Self, LoadError> {
        Self::resolve(
            config::parse(files.goods)?,
            config::parse(files.skills)?,
            config::parse(files.recipes)?,
            config::parse(files.traits)?,
            config::parse(files.moods)?,
            config::parse(files.predicates)?,
            config::parse(files.verbs)?,
            config::parse(files.registers)?,
        )
    }

    /// Resolve already-parsed definitions into the registry: intern each name to
    /// a dense id and validate every cross-reference (a recipe naming an unknown
    /// good/skill, a mood opposing an unknown mood) up front.
    #[allow(clippy::too_many_arguments)]
    fn resolve(
        goods: Vec<GoodDef>,
        skills: Vec<SkillDef>,
        raw: Vec<RecipeDef>,
        traits: Vec<TraitDef>,
        moods: Vec<MoodDef>,
        predicates: Vec<String>,
        verb_defs: Vec<VerbDef>,
        register_defs: Vec<RegisterDto>,
    ) -> Result<Self, LoadError> {
        let good_ids = index(goods.iter().map(|g| &g.name));
        let skill_ids = index(skills.iter().map(|s| &s.name));
        let trait_ids = index(traits.iter().map(|t| &t.name));
        let mood_ids = index(moods.iter().map(|m| &m.name));
        let predicate_ids = index(predicates.iter());
        let verbs = verb_defs
            .into_iter()
            .map(|v| {
                let p = predicate_ids
                    .get(&v.predicate)
                    .copied()
                    .ok_or(LoadError::UnknownPredicate(v.predicate))?;
                Ok((v.name, (p, v.value)))
            })
            .collect::<Result<HashMap<_, _>, LoadError>>()?;
        let mood_opposes = moods
            .iter()
            .map(|m| match &m.opposes {
                None => Ok(None),
                Some(n) => mood_ids
                    .get(n)
                    .copied()
                    .map(Some)
                    .ok_or_else(|| LoadError::UnknownMood(n.clone())),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mood_shapes = moods
            .iter()
            .map(|m| match &m.shapes {
                None => Ok(None),
                Some(n) => trait_ids
                    .get(n)
                    .copied()
                    .map(|t| Some((t, m.shape_rate)))
                    .ok_or_else(|| LoadError::UnknownTrait(n.clone())),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let trait_opposes = traits
            .iter()
            .map(|t| match &t.opposes {
                None => Ok(None),
                Some(n) => trait_ids
                    .get(n)
                    .copied()
                    .map(Some)
                    .ok_or_else(|| LoadError::UnknownTrait(n.clone())),
            })
            .collect::<Result<Vec<_>, _>>()?;
        // The registers: intern each name, then a second pass resolves the self-referential `seeds`
        // cross-reference (grief → which register) to an id — exactly like `mood_opposes` above.
        let register_ids = index(register_defs.iter().map(|r| &r.name));
        let registers = register_defs
            .iter()
            .map(|r| {
                let seeds = match &r.seeds {
                    None => None,
                    Some(n) => Some(
                        register_ids
                            .get(n)
                            .copied()
                            .ok_or_else(|| LoadError::UnknownRegister(n.clone()))?,
                    ),
                };
                Ok(RegisterDef {
                    name: r.name.clone(),
                    spine: r.spine,
                    trunk: r.trunk,
                    bright: r.bright,
                    seeds,
                    casting: r.casting,
                    epithet_lead: r.epithet_lead.clone(),
                    epithet_other: r.epithet_other.clone(),
                    situation_lead: r.situation_lead.clone(),
                    situation_other: r.situation_other.clone(),
                    noun: r.noun.clone(),
                    told: r.told.clone(),
                    quest_plea: r.quest_plea.clone(),
                    quest_objective: r.quest_objective.clone(),
                })
            })
            .collect::<Result<Vec<_>, LoadError>>()?;
        // Spine registers in file order — the director's register-rotation order (the old `SPINES`).
        let spine_order: Vec<RegisterId> = registers
            .iter()
            .enumerate()
            .filter(|(_, r)| r.spine)
            .map(|(i, _)| i)
            .collect();
        let good = |name: &str| {
            good_ids
                .get(name)
                .copied()
                .ok_or_else(|| LoadError::UnknownGood(name.to_owned()))
        };
        let skill = |name: &str| {
            skill_ids
                .get(name)
                .copied()
                .ok_or_else(|| LoadError::UnknownSkill(name.to_owned()))
        };

        let mut recipes = Vec::with_capacity(raw.len());
        for r in raw {
            let resolve = |pairs: Vec<(String, u32)>| {
                pairs
                    .into_iter()
                    .map(|(n, q)| good(&n).map(|id| (id, q)))
                    .collect::<Result<Vec<_>, _>>()
            };
            recipes.push(Recipe {
                skill: skill(&r.skill)?,
                inputs: resolve(r.inputs)?,
                outputs: resolve(r.outputs)?,
                name: r.name,
                resource: r.resource,
                min_resource: r.min_resource,
                deplete: r.deplete,
                effort: r.effort,
            });
        }

        Ok(Self {
            goods,
            good_ids,
            skills,
            skill_ids,
            recipes,
            traits,
            trait_ids,
            trait_opposes,
            moods,
            mood_ids,
            mood_opposes,
            mood_shapes,
            predicates,
            predicate_ids,
            verbs,
            registers,
            register_ids,
            spine_order,
        })
    }

    pub fn good_count(&self) -> usize {
        self.goods.len()
    }
    pub fn skill_count(&self) -> usize {
        self.skills.len()
    }
    pub fn trait_count(&self) -> usize {
        self.traits.len()
    }
    pub fn trait_def(&self, id: TraitId) -> &TraitDef {
        &self.traits[id]
    }
    pub fn trait_id(&self, name: &str) -> Option<TraitId> {
        self.trait_ids.get(name).copied()
    }
    /// The opposing trait of `id`, if it has one.
    pub fn opposes(&self, id: TraitId) -> Option<TraitId> {
        self.trait_opposes[id]
    }
    pub fn mood_count(&self) -> usize {
        self.moods.len()
    }
    pub fn mood_def(&self, id: MoodId) -> &MoodDef {
        &self.moods[id]
    }
    pub fn mood_id(&self, name: &str) -> Option<MoodId> {
        self.mood_ids.get(name).copied()
    }
    /// The opposing mood of `id`, if it has one.
    pub fn mood_opposes(&self, id: MoodId) -> Option<MoodId> {
        self.mood_opposes[id]
    }
    /// The trait `id` slowly shapes when sustained, and the rate, if any.
    pub fn mood_shapes(&self, id: MoodId) -> Option<(TraitId, f32)> {
        self.mood_shapes[id]
    }
    pub fn predicate_count(&self) -> usize {
        self.predicates.len()
    }
    pub fn predicate_id(&self, name: &str) -> Option<PredicateId> {
        self.predicate_ids.get(name).copied()
    }
    /// The relational effect of a surface verb: the `(predicate, value)` it makes
    /// of its target.
    pub fn verb(&self, name: &str) -> Option<(PredicateId, i64)> {
        self.verbs.get(name).copied()
    }
    pub fn good(&self, id: GoodId) -> &GoodDef {
        &self.goods[id]
    }
    pub fn good_id(&self, name: &str) -> Option<GoodId> {
        self.good_ids.get(name).copied()
    }
    pub fn skill(&self, id: SkillId) -> &SkillDef {
        &self.skills[id]
    }
    pub fn skill_id(&self, name: &str) -> Option<SkillId> {
        self.skill_ids.get(name).copied()
    }
    pub fn recipes(&self) -> &[Recipe] {
        &self.recipes
    }
    pub fn register_count(&self) -> usize {
        self.registers.len()
    }
    /// The resolved domain object for register `id` — its whole shape (levers + surface text).
    pub fn register_def(&self, id: RegisterId) -> &RegisterDef {
        &self.registers[id]
    }
    pub fn register_id(&self, name: &str) -> Option<RegisterId> {
        self.register_ids.get(name).copied()
    }
    /// The register's authored name (`"betrayal"`) — for inspection / human-readable logs.
    pub fn register_name(&self, id: RegisterId) -> &str {
        &self.registers[id].name
    }
    /// The spine-eligible registers, in the director's rotation order (the old `SPINES`).
    pub fn spines(&self) -> &[RegisterId] {
        &self.spine_order
    }
    /// The register a *closing* thread of `id` seeds next (grief → vengeance), if any.
    pub fn register_seeds(&self, id: RegisterId) -> Option<RegisterId> {
        self.registers[id].seeds
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::bundled()
    }
}

/// Map each name to its position.
fn index<'a>(names: impl Iterator<Item = &'a String>) -> HashMap<String, usize> {
    names.enumerate().map(|(i, n)| (n.clone(), i)).collect()
}

/// Why loading game data failed.
#[derive(Debug)]
pub enum LoadError {
    Config(config::ConfigError),
    UnknownGood(String),
    UnknownSkill(String),
    UnknownTrait(String),
    UnknownMood(String),
    UnknownPredicate(String),
    UnknownRegister(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Config(e) => write!(f, "loading game data: {e}"),
            LoadError::UnknownGood(n) => write!(f, "recipe refers to unknown good '{n}'"),
            LoadError::UnknownSkill(n) => write!(f, "recipe refers to unknown skill '{n}'"),
            LoadError::UnknownTrait(n) => write!(f, "trait/mood refers to unknown trait '{n}'"),
            LoadError::UnknownMood(n) => write!(f, "mood opposes unknown mood '{n}'"),
            LoadError::UnknownPredicate(n) => write!(f, "verb refers to unknown predicate '{n}'"),
            LoadError::UnknownRegister(n) => write!(f, "register seeds unknown register '{n}'"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<config::ConfigError> for LoadError {
    fn from(e: config::ConfigError) -> Self {
        LoadError::Config(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_data_loads_and_resolves() {
        let reg = Registry::bundled();
        // The core food economy (grain → bread, farming/baking) is always present; the
        // bundled set layers more goods/skills on top (the weaver's reed→shroud trade),
        // so assert the floor, not an exact count.
        assert!(reg.good_count() >= 2);
        assert!(reg.skill_count() >= 2);
        let grain = reg.good_id("grain").unwrap();
        let bread = reg.good_id("bread").unwrap();
        let bake = reg.recipes().iter().find(|r| r.name == "bake").unwrap();
        assert_eq!(bake.inputs, vec![(grain, 2)]);
        assert_eq!(bake.outputs, vec![(bread, 1)]);
        assert_eq!(reg.skill(bake.skill).name, "baking");
    }

    #[test]
    fn adding_a_resource_trade_needs_no_code() {
        // A whole new trade — mining ore out of the rock — authored as pure data,
        // drawing on a natural resource it depletes.
        let goods = r#"[(name: "ore", base_price: 20, target_stock: 20, nutrition: 0.0)]"#;
        let skills = r#"[(name: "mining", gain: 0.03, cap: 6.0)]"#;
        let recipes = r#"[(name: "mine", skill: "mining", inputs: [], outputs: [("ore", 1)],
            resource: Some(Minerals), min_resource: 0.1, deplete: 0.02, effort: 1.5)]"#;
        let reg = Registry::from_ron(DataFiles {
            goods,
            skills,
            recipes,
            ..Default::default()
        })
        .unwrap();
        let mine = &reg.recipes()[0];
        assert_eq!(mine.resource, Some(ResourceKind::Minerals));
        assert!(mine.deplete > 0.0);
    }

    #[test]
    fn a_recipe_naming_an_unknown_skill_is_rejected() {
        let goods = r#"[(name: "grain", base_price: 10, target_stock: 40, nutrition: 28.0)]"#;
        let skills = r#"[(name: "farming", gain: 0.02, cap: 5.0)]"#;
        let recipes = r#"[(name: "bake", skill: "baking", inputs: [], outputs: [("grain", 1)],
            resource: None, min_resource: 0.0, deplete: 0.0, effort: 1.0)]"#;
        assert!(matches!(
            Registry::from_ron(DataFiles {
                goods,
                skills,
                recipes,
                ..Default::default()
            }),
            Err(LoadError::UnknownSkill(_))
        ));
    }

    #[test]
    fn a_recipe_naming_an_unknown_good_is_rejected() {
        let goods = r#"[(name: "grain", base_price: 10, target_stock: 40, nutrition: 28.0)]"#;
        let skills = r#"[(name: "baking", gain: 0.02, cap: 5.0)]"#;
        let recipes = r#"[(name: "bake", skill: "baking", inputs: [("flour", 1)],
            outputs: [("grain", 1)], resource: None, min_resource: 0.0, deplete: 0.0, effort: 1.0)]"#;
        assert!(matches!(
            Registry::from_ron(DataFiles {
                goods,
                skills,
                recipes,
                ..Default::default()
            }),
            Err(LoadError::UnknownGood(_))
        ));
    }

    #[test]
    fn bundled_traits_load() {
        let reg = Registry::bundled();
        assert!(reg.trait_count() >= 3);
        let a = reg.trait_id("ambition").expect("ambition trait exists");
        assert!(reg.trait_def(a).baseline >= 0.0);
    }

    #[test]
    fn bundled_registers_resolve() {
        let reg = Registry::bundled();
        assert!(reg.register_count() >= 15);
        let betrayal = reg.register_id("betrayal").expect("betrayal register exists");
        let def = reg.register_def(betrayal);
        assert!(def.trunk && def.spine);
        // the spine rotation reproduces the old `director::SPINES` order, betrayal first.
        assert_eq!(reg.spines().first().copied(), Some(betrayal));
        // betrayal's grief seeds vengeance (the self-perpetuating trunk), resolved to an id.
        assert_eq!(reg.register_seeds(betrayal), reg.register_id("vengeance"));
    }

    #[test]
    fn a_new_register_needs_no_code() {
        // A register that exists in NO Rust source — authored purely in RON, with its `seeds`
        // cross-reference and (untuned) surface text — resolves into the registry exactly like a
        // new good or mood. This is the whole point: the domain is data, the code is generic.
        let registers = r#"[
            (name: "vengeance", casting: Coldest),
            (name: "schadenfreude", spine: true, trunk: true, seeds: Some("vengeance"),
             casting: Ambitious, noun: "delicious ruin"),
        ]"#;
        let reg = Registry::from_ron(DataFiles {
            registers,
            ..Default::default()
        })
        .expect("a registers doc with a novel register resolves");
        let id = reg.register_id("schadenfreude").expect("the new register exists");
        let def = reg.register_def(id);
        assert!(def.spine && def.trunk);
        assert_eq!(def.casting, Casting::Ambitious);
        assert_eq!(def.noun, "delicious ruin");
        assert!(reg.spines().contains(&id));
        // seeds resolved to the vengeance id; untuned epithet fell back to the generic default.
        assert_eq!(reg.register_seeds(id), reg.register_id("vengeance"));
        assert_eq!(def.epithet_lead, "the Storied");
    }

    #[test]
    fn a_register_seeding_an_unknown_register_is_rejected() {
        let registers = r#"[(name: "loss", seeds: Some("nirvana"))]"#;
        assert!(matches!(
            Registry::from_ron(DataFiles {
                registers,
                ..Default::default()
            }),
            Err(LoadError::UnknownRegister(_))
        ));
    }
}
