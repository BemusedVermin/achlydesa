//! The **beat library** — the director's repertoire of dramatic situations, as data.
//!
//! Grounded in **Polti's *Thirty-Six Dramatic Situations*** (each situation names its
//! *actant roles* — Avenger & Criminal, Tyrant & Conspirator, Ambitious one & Rival)
//! and the **storylet / quality-based-narrative** architecture (Failbetter Games;
//! Emily Short): a **beat is a storylet** — a *precondition* over the world's
//! qualities, a *casting* of roles drawn from the protagonist's social world, and a
//! set of *effects* that manipulate people, factions, and the land.
//!
//! The drama manager ([`director`](crate::director)) finds the beats the world can
//! currently tell, scores them by salience, the tension arc, and **novelty** (so the
//! same story isn't told twice running), and enacts one — then the world's own
//! systems (the avenge machinery, the faction turn, the planner) play it out. The
//! director **instigates**; it does not puppet. Authoring is the whole point: add a
//! beat to `beats.ron` and the director can tell it, with no Rust changes.
//!
//! A beat's [`register`](Beat::register) — its dramatic key — is itself **data**
//! (`registers.ron`, resolved to a [`RegisterId`] at load): the register *domain* (its
//! levers and surface text) is [`crate::data::RegisterDef`], separate from the beats
//! (its instances). A new register is pure RON too.

use crate::data::{RegisterId, Registry};
use config::{Asset, Bundled};
use serde::Deserialize;

/// A role a beat casts from the protagonist's social world — Polti's *actants*. The
/// director fills each from the living cast (the protagonist, and the people whose
/// traits and relationships best fit), or the beat can't be told.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// The avatar the story is told for. Always the [`Protagonist`](crate::Protagonist).
    Protagonist,
    /// One who holds the protagonist dear — warmest opinion of them. A bond to break.
    Ally,
    /// The most **ambitious** figure in the protagonist's orbit — a would-be usurper.
    Rival,
    /// One who bears the protagonist ill — coldest opinion of them (or already a grudge).
    Foe,
    /// The head of a faction the protagonist belongs to — the power over them.
    Patron,
    /// Any other soul — the crowd a story can pull a stranger from.
    Bystander,
    /// The most **sociable**, warmest figure — a love the director kindles on purpose so
    /// the reversal devastates (the *romance* register; the bond the trunk breaks).
    Lover,
    /// The most **pious** figure — a wise elder / believer, the mouth the Demiurge myth
    /// speaks through and the guide of *wonder* and *reunion* beats.
    Mentor,
}

impl Role {
    /// The casting index a role occupies, so a beat's effects can name its cast.
    pub fn slot(self) -> usize {
        self as usize
    }
}

/// The number of casting slots — every [`Role`] plus headroom. The director's slot
/// arrays are this wide.
pub const SLOTS: usize = 8;

/// Where in a thread's **groom → climax → fall** arc (decision #12) a beat belongs. A
/// beat with no declared phases fits any. Setup beats *manufacture attachment*; Climax
/// beats are the reversals; Fall beats are the aftermath that seeds the next thread.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Phase {
    Setup,
    Rising,
    Climax,
    Fall,
}

/// A precondition over the world's qualities — what must hold for a beat to be
/// *tellable*, checked once the roles are tentatively cast. These are the **starvable
/// inputs**: a world whose qualities meet none of a beat's preconditions cannot be told
/// that beat, however omnipotent the director — the freedom is in the *state*, not in
/// stripping the director (§5.A).
#[derive(Deserialize, Clone, Debug)]
pub enum Pre {
    /// `who` must have been cast at all (the role found a fit).
    Exists { who: Role },
    /// The named trait of `who` is at least `v` (`0..1`).
    TraitAtLeast {
        who: Role,
        trait_name: String,
        v: f32,
    },
    /// The named trait of `who` is at most `v`.
    TraitAtMost {
        who: Role,
        trait_name: String,
        v: f32,
    },
    /// The named mood of `who` is at least `v` — lets a beat key off a *feeling* a prior
    /// beat stirred (grief, fear, fury), so storylets chain into arcs.
    MoodAtLeast { who: Role, mood: String, v: f32 },
    /// `who` already bears a grudge (or pointedly does not).
    HasGrudge { who: Role, yes: bool },
    /// The protagonist holds the throne (or does not).
    HoldsThrone { yes: bool },
    /// The protagonist belongs to at least one faction (or none).
    InFaction { yes: bool },
    /// A faction the protagonist belongs to is at war (or none is). The hook a peace /
    /// war-weariness beat keys off.
    AtWar { yes: bool },
    /// Someone near the protagonist is *vulnerable* — sustenance below `need_below`. A
    /// disaster only bites where there are bellies to empty, so a provisioned world
    /// (deep larders, surplus) makes the survival register **uncastable**.
    VictimNearby { need_below: f32 },
    /// `who` holds (or pointedly lacks) a durable [`Bond`](crate::people::Bond) — the positive
    /// twin of `HasGrudge`. Gates beats that key off a love built earlier.
    Bonded { who: Role, yes: bool },
    /// `who` is held captive ([`Detained`](crate::factions::Detained)) — or pointedly free.
    /// Gates "free the bound" beats. (Also true for souls a faction's enforcers have jailed.)
    Bound { who: Role, yes: bool },
    /// A discovered, unspoilt marvel lies within the protagonist's reach (or pointedly does
    /// not) — gates [`Defile`](Effect::Defile): you can only ruin a wonder already found.
    DiscoveredMarvelNearby { yes: bool },
}

impl Pre {
    /// The role this precondition constrains (so casting knows which slots matter), if any.
    pub fn who(&self) -> Option<Role> {
        match self {
            Pre::Exists { who }
            | Pre::TraitAtLeast { who, .. }
            | Pre::TraitAtMost { who, .. }
            | Pre::MoodAtLeast { who, .. }
            | Pre::HasGrudge { who, .. }
            | Pre::Bonded { who, .. }
            | Pre::Bound { who, .. } => Some(*who),
            Pre::HoldsThrone { .. }
            | Pre::InFaction { .. }
            | Pre::AtWar { .. }
            | Pre::VictimNearby { .. }
            | Pre::DiscoveredMarvelNearby { .. } => None,
        }
    }
}

/// What a beat *does* when it fires — the director's lever vocabulary, now reaching
/// people and factions, not just the land. Each is a manipulation the world's own
/// systems then carry forward.
///
/// Adding a lever is a small, fixed recipe (levers are a curated vocabulary; beats are the
/// open, data-driven content - a new beat is pure RON, no code). A new `Effect`: add the
/// variant here, an arm in [`Beat::roles`] (the roles it casts), and an enactment arm in
/// `director::director_step`. A new [`Pre`]: the variant, an arm in [`Pre::who`] and in
/// `director::pre_ok`. A new **register**: add a row to `registers.ron` — pure data, no code.
#[derive(Deserialize, Clone, Debug)]
pub enum Effect {
    /// Set `who` to bear a grudge against `against` — Polti's *Crime Pursued by
    /// Vengeance*. Drives the avenge machinery wherever an avenge goal is authored.
    Grudge { who: Role, against: Role },
    /// Shift a personality trait of `who` by `delta` (embolden ambition, stoke
    /// vengeance, wear away forgiveness). Clamped to `0..1`.
    Sway {
        who: Role,
        trait_name: String,
        delta: f32,
    },
    /// Shift a mood of `who` by `delta` (kindle anger, strike fear, lift joy).
    Stir { who: Role, mood: String, delta: f32 },
    /// Move `who`'s opinion of `toward` by `delta` — poison a friendship toward
    /// betrayal (negative) or thaw an enmity (positive). Clamped to `-1..1`.
    Turn { who: Role, toward: Role, delta: f32 },
    /// Make the protagonist's faction lay a no-kill **taboo** on its members — Polti's
    /// *Supplication* / persecution: its enforcers then come for the vengeful.
    Decree,
    /// Set the protagonist's faction at **war** with its nearest rival — *Revolt* /
    /// *Rivalry*. The faction turn takes it from there (exclusion, casualties).
    War,
    /// A disaster on the protagonist's locale: drain the sustenance of everyone within
    /// `radius` by `severity` and scour the vegetation, so the famine bites *people*
    /// and the loss persists — *Falling Prey to Misfortune*.
    Disaster { radius: i32, severity: f32 },
    /// Single out `who` for misfortune — drain their sustenance by `severity`, toward
    /// starvation. *Loss of a Loved One* when aimed at an ally; a personal calamity.
    Afflict { who: Role, severity: f32 },
    /// A **marvel** is revealed in the protagonist's locale — the *wonder* register.
    /// Discover the nearest still-hidden [`Feature`](crate::Features) in reach (a real
    /// fact entered into the world, wired to the discovery layer) and fill the cast with
    /// **awe**. Joy the director authors on purpose, so a later beat can defile it.
    Reveal,
    /// Put words in `who`'s mouth — the director makes them *speak* a conversational
    /// [`intent`](crate::dialogue) at the protagonist (the manufactured betrayal *heard*,
    /// not merely tallied). A no-op unless the [`dialogue`](crate::dialogue) layer is awake.
    Voice { who: Role, intent: String },
    /// Restore a need of `who` — the bright twin of [`Afflict`]: heal a struck body, feed
    /// the starving. Mirrors the affordance [`Relieve`](crate::features::EffectDef::Relieve)
    /// write to [`Needs`](crate::people::Needs). Material grace.
    Relieve {
        who: Role,
        need: crate::features::NeedKind,
        amount: i32,
    },
    /// Slay `who` (by `by`). **Interim**: a mortal wound the metabolism finishes next tick (a
    /// plausible in-world death preserving the deniability rule); true Slay routes through
    /// combat later. The apex of a vengeance or atrocity beat; `by` is the slayer (recorded
    /// as a `Killed` episode once the Chronicle is wired).
    Slay { who: Role, by: Role },
    /// Exalt `who` — raise their standing in awe and pride (the heavenly apex). **Interim**:
    /// narrative prominence + a soaring high; the true power-tier raise awaits the rpg
    /// ascendant tier.
    Exalt { who: Role },
    /// Defile the nearest discovered, unspoilt marvel in the protagonist's reach — the dark
    /// twin of [`Reveal`]: a wonder ruined (`features.defile_at_index`). Stirs despair/sorrow
    /// in the cast. Gate it with [`Pre::DiscoveredMarvelNearby`].
    Defile,
    /// Forge a durable [`Bond`](crate::people::Bond) from `who` to `to` (a vow, oath-kin, a
    /// love) — the bright setup the director can later reverse so the break *means* something.
    Bond { who: Role, to: Role },
    /// Bind `who` in captivity (the Archons' `bind`, `norms.ron`): reuses the faction-enforcer
    /// [`Detained`](crate::factions::Detained) machinery — `who` cannot act until freed.
    Bind { who: Role },
    /// Free `who` from captivity (the defiant `free`, `norms.ron`) — strike off another's
    /// chains. The signature heavenly/defiant deed.
    Free { who: Role },
}

/// One dramatic situation, as authored in `beats.ron` (its `register` not yet resolved).
#[derive(Deserialize, Clone, Debug)]
struct BeatDef {
    id: String,
    /// The dramatic register's authored **name** (`"betrayal"`), resolved to a [`RegisterId`].
    register: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    phases: Vec<Phase>,
    tension: f32,
    #[serde(default = "one")]
    stakes: f32,
    #[serde(default = "one")]
    weight: f32,
    cast: Vec<Role>,
    #[serde(default)]
    pre: Vec<Pre>,
    effects: Vec<Effect>,
}

/// One dramatic situation, resolved: a storylet the director can tell.
#[derive(Clone, Debug)]
pub struct Beat {
    /// A stable id, also the line the story log shows ("a_rival_rises").
    pub id: String,
    /// The dramatic **register** this beat plays in — its emotional key, resolved to a dense
    /// [`RegisterId`] (the domain lives in [`crate::data::RegisterDef`]). Read by the thread
    /// machinery (a thread's `spine`) and by register-rotation.
    pub register: RegisterId,
    /// Free-form registers / tone tags — for the **novelty** penalty (don't repeat a
    /// tone twice running) and authorial filtering: e.g. `"betrayal"`, `"violence"`,
    /// `"political"`, `"survival"`. Orthogonal to the formal [`Beat::register`].
    pub tags: Vec<String>,
    /// Which arc [`Phase`]s this beat suits (groom→climax→fall). Empty = any phase.
    pub phases: Vec<Phase>,
    /// How much dramatic **tension** telling this beat injects. Escalations are
    /// positive; *relief* beats (a reconciliation, a respite) are negative.
    pub tension: f32,
    /// The **stakes** — how much the target stands to lose or gain (a life, a throne, a
    /// bond, a love). A multiplier in the drama objective (`drama = stakes × attachment ×
    /// reversal`); high for climactic beats, low for grooming.
    pub stakes: f32,
    /// Base authorial weight, before the drama objective.
    pub weight: f32,
    /// The roles this beat needs cast, in the order its effects refer to them.
    pub cast: Vec<Role>,
    /// What must hold for the beat to be tellable.
    pub pre: Vec<Pre>,
    /// What telling it does to the world.
    pub effects: Vec<Effect>,
}

fn one() -> f32 {
    1.0
}

impl Beat {
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// Every role this beat references, across its cast, preconditions, and effects —
    /// the slots the director must fill before it can tell the beat.
    pub fn roles(&self) -> Vec<Role> {
        let mut rs = self.cast.clone();
        let mut push = |r: Role| {
            if !rs.contains(&r) {
                rs.push(r);
            }
        };
        for p in &self.pre {
            if let Some(r) = p.who() {
                push(r);
            }
        }
        for e in &self.effects {
            match e {
                Effect::Grudge { who, against } => {
                    push(*who);
                    push(*against);
                }
                Effect::Turn { who, toward, .. } => {
                    push(*who);
                    push(*toward);
                }
                Effect::Slay { who, by } => {
                    push(*who);
                    push(*by);
                }
                Effect::Bond { who, to } => {
                    push(*who);
                    push(*to);
                }
                Effect::Sway { who, .. }
                | Effect::Stir { who, .. }
                | Effect::Afflict { who, .. }
                | Effect::Voice { who, .. }
                | Effect::Relieve { who, .. }
                | Effect::Exalt { who, .. }
                | Effect::Bind { who, .. }
                | Effect::Free { who, .. } => push(*who),
                Effect::Decree
                | Effect::War
                | Effect::Disaster { .. }
                | Effect::Reveal
                | Effect::Defile => {}
            }
        }
        rs
    }
}

/// The director's whole repertoire — the strippable action set `L`, now a library of
/// dramatic situations. Emptying it frees the world (the §5.B / §1.1 liberation).
#[derive(bevy_ecs::prelude::Resource, Clone, Debug, Default)]
pub struct BeatBook(pub Vec<Beat>);

impl BeatBook {
    /// The defaults shipped with the crate — resolved against the bundled registry.
    pub fn bundled() -> Self {
        Self::from_ron(Bundled::get(Asset::Beats), &Registry::bundled())
            .expect("bundled beats are valid RON and resolve against the bundled registry")
    }

    /// Parse a beats document and resolve each beat's register name against `reg` (trait /
    /// mood names stay resolved lazily at enactment, but are validated here too). A typo in a
    /// register, trait, or mood name is a load error, not a silent no-op.
    pub fn from_ron(ron: &str, reg: &Registry) -> Result<Self, BeatError> {
        let defs: Vec<BeatDef> = config::parse(ron)?;
        let mut beats = Vec::with_capacity(defs.len());
        for d in defs {
            let register =
                reg.register_id(&d.register)
                    .ok_or_else(|| BeatError::UnknownRegister {
                        beat: d.id.clone(),
                        register: d.register.clone(),
                    })?;
            beats.push(Beat {
                id: d.id,
                register,
                tags: d.tags,
                phases: d.phases,
                tension: d.tension,
                stakes: d.stakes,
                weight: d.weight,
                cast: d.cast,
                pre: d.pre,
                effects: d.effects,
            });
        }
        let book = BeatBook(beats);
        book.validate(reg)?;
        Ok(book)
    }

    /// Fail fast on any trait / mood name a beat refers to that the registry doesn't
    /// know — so a typo in `beats.ron` is caught at load, not silently no-op'd.
    pub fn validate(&self, reg: &Registry) -> Result<(), BeatError> {
        for b in &self.0 {
            for e in &b.effects {
                match e {
                    Effect::Sway { trait_name, .. } => {
                        reg.trait_id(trait_name)
                            .ok_or_else(|| BeatError::UnknownTrait {
                                beat: b.id.clone(),
                                trait_name: trait_name.clone(),
                            })?;
                    }
                    Effect::Stir { mood, .. } => {
                        reg.mood_id(mood).ok_or_else(|| BeatError::UnknownMood {
                            beat: b.id.clone(),
                            mood: mood.clone(),
                        })?;
                    }
                    _ => {}
                }
            }
            for p in &b.pre {
                match p {
                    Pre::TraitAtLeast { trait_name, .. } | Pre::TraitAtMost { trait_name, .. } => {
                        reg.trait_id(trait_name)
                            .ok_or_else(|| BeatError::UnknownTrait {
                                beat: b.id.clone(),
                                trait_name: trait_name.clone(),
                            })?;
                    }
                    Pre::MoodAtLeast { mood, .. } => {
                        reg.mood_id(mood).ok_or_else(|| BeatError::UnknownMood {
                            beat: b.id.clone(),
                            mood: mood.clone(),
                        })?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

/// Why loading beats failed — parse error, or a beat naming content the registry doesn't know.
#[derive(Debug)]
pub enum BeatError {
    Config(config::ConfigError),
    /// A beat names a register the registry doesn't define.
    UnknownRegister {
        beat: String,
        register: String,
    },
    /// A beat's effect or precondition names a trait the registry doesn't define.
    UnknownTrait {
        beat: String,
        trait_name: String,
    },
    /// A beat's effect or precondition names a mood the registry doesn't define.
    UnknownMood {
        beat: String,
        mood: String,
    },
}

impl std::fmt::Display for BeatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BeatError::Config(e) => write!(f, "loading beats: {e}"),
            BeatError::UnknownRegister { beat, register } => {
                write!(f, "beat '{beat}': unknown register '{register}'")
            }
            BeatError::UnknownTrait { beat, trait_name } => {
                write!(f, "beat '{beat}': unknown trait '{trait_name}'")
            }
            BeatError::UnknownMood { beat, mood } => {
                write!(f, "beat '{beat}': unknown mood '{mood}'")
            }
        }
    }
}
impl std::error::Error for BeatError {}
impl From<config::ConfigError> for BeatError {
    fn from(e: config::ConfigError) -> Self {
        BeatError::Config(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Registry;

    #[test]
    fn bundled_beats_load_and_resolve() {
        let reg = Registry::bundled();
        let book = BeatBook::bundled();
        assert!(
            book.0.len() >= 20,
            "the beat library should be richly stocked, got {}",
            book.0.len()
        );
        // The free-form tone tags are represented (novelty registers).
        for tag in [
            "betrayal",
            "political",
            "ambition",
            "disaster",
            "relief",
            "vengeance",
            "revolt",
        ] {
            assert!(
                book.0.iter().any(|b| b.has_tag(tag)),
                "no beat carries the '{tag}' tone tag"
            );
        }
        // The palette is broad — not all tragedy. The brighter registers the director
        // grooms (decision #8) are stocked alongside the trunk and its tributaries.
        // Registers are now data, so assert by resolved name → id.
        for name in [
            "betrayal",
            "vengeance",
            "romance",
            "triumph",
            "wonder",
            "sacrifice",
            "grace",
        ] {
            let id = reg
                .register_id(name)
                .unwrap_or_else(|| panic!("register '{name}' should exist"));
            assert!(
                book.0.iter().any(|b| b.register == id),
                "no beat plays the {name} register"
            );
        }
        assert!(
            book.0.iter().any(|b| reg.register_def(b.register).bright),
            "the palette has no brighter register at all"
        );
    }

    #[test]
    fn an_unknown_trait_in_a_beat_is_rejected() {
        let reg = Registry::bundled();
        let ron = r#"[(
            id: "bad", register: "betrayal", tension: 1.0, cast: [Protagonist],
            effects: [Sway(who: Protagonist, trait_name: "greedmaxxing", delta: 0.1)],
        )]"#;
        assert!(BeatBook::from_ron(ron, &reg).is_err());
    }

    #[test]
    fn an_unknown_register_in_a_beat_is_rejected() {
        // Adding a register is data, but *referring* to one that doesn't exist is still a
        // load error — the resolution catches the typo, not a silent phantom register.
        let reg = Registry::bundled();
        let ron = r#"[(
            id: "bad", register: "schadenfreude", tension: 1.0, cast: [Protagonist],
            effects: [Stir(who: Protagonist, mood: "joy", delta: 0.1)],
        )]"#;
        assert!(BeatBook::from_ron(ron, &reg).is_err());
    }

    #[test]
    fn a_beat_in_a_data_authored_register_is_castable() {
        use crate::data::DataFiles;
        use config::{Asset, Bundled};
        // A register that exists in NO Rust code, authored purely in RON (traits/moods bundled so
        // the beat's mood effect still validates)...
        let registers = r#"[(name: "schadenfreude", spine: true, casting: Coldest)]"#;
        let reg = Registry::from_ron(DataFiles {
            traits: Bundled::get(Asset::Traits),
            moods: Bundled::get(Asset::Moods),
            registers,
            ..Default::default()
        })
        .unwrap();
        // ...and a beat that plays in it loads and resolves with no code change at all.
        let ron = r#"[(
            id: "a_petty_delight", register: "schadenfreude", tension: 0.4, cast: [Protagonist],
            effects: [Stir(who: Protagonist, mood: "joy", delta: 0.2)],
        )]"#;
        let book =
            BeatBook::from_ron(ron, &reg).expect("a beat in a data-authored register resolves");
        assert_eq!(book.0.len(), 1);
        assert_eq!(
            book.0[0].register,
            reg.register_id("schadenfreude").unwrap()
        );
    }
}
