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

use crate::data::Registry;
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

/// A beat's **dramatic register** — the emotional key it plays in. The director runs a
/// few threads at once, each with a `spine` register, and **rotates registers freely**
/// (decision #17): betrayal dominates because it *scores* highest (the trunk), never by
/// enforcement. Distinct from the free-form [`Beat::tags`] (which only drive novelty).
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Register {
    Betrayal,
    Vengeance,
    Ambition,
    Persecution,
    War,
    Disaster,
    Loss,
    Romance,
    Triumph,
    Wonder,
    Reunion,
    Sacrifice,
    Redemption,
    Relief,
    /// The heavenly apex — mercy, healing, union, deliverance, the homecoming. Bright, and
    /// a thread **spine** in its own right (a redemption/mercy arc): the bright counterweight
    /// to the betrayal trunk, the top of the "deplorable → heavenly" spectrum.
    Grace,
}

/// How a thread pins its counterpart (the figure it grooms then reverses), by register —
/// the director's casting policy, lifted out of `pick_other` into the register table below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Casting {
    /// The warmest soul (a beloved) — the default; a bond to build then break.
    Warmest,
    /// The coldest (a foe already turned away) — vengeance, persecution.
    Coldest,
    /// The most ambitious (a would-be usurper) — ambition, war.
    Ambitious,
    /// The most pious (a believer/elder) — wonder, reunion.
    Pious,
}

/// **All per-register metadata in one place** (see [`Register::def`]): brightness, the trunk
/// flag, whether closing such a thread seeds vengeance, the casting policy, and the
/// player-facing epithet/situation text (lead vs. the pinned other). Everything that used to
/// match on `Register` in scattered helpers now reads one row here.
#[derive(Clone, Copy, Debug)]
pub struct RegisterDef {
    pub bright: bool,
    pub trunk: bool,
    pub seeds_vengeance: bool,
    pub casting: Casting,
    pub epithet_lead: &'static str,
    pub epithet_other: &'static str,
    pub situation_lead: &'static str,
    pub situation_other: &'static str,
}

impl RegisterDef {
    /// The earned epithet for the lead (`is_lead`) or the pinned other.
    pub fn epithet(&self, is_lead: bool) -> &'static str {
        if is_lead {
            self.epithet_lead
        } else {
            self.epithet_other
        }
    }
    /// The one-line situational opener for the lead or the pinned other.
    pub fn situation(&self, is_lead: bool) -> &'static str {
        if is_lead {
            self.situation_lead
        } else {
            self.situation_other
        }
    }
}

impl Register {
    /// The single source of truth for a register's properties (see [`RegisterDef`]). The match
    /// has no wildcard, so adding a `Register` variant forces a row here — a register can never
    /// be half-wired. (Spine eligibility + order stays the explicit `director::SPINES` list, for
    /// determinism.) Registers without tuned narration fall back to the generic "Storied"/"heavy"
    /// text (preserving the prior `_ =>` behaviour); surface text only, never read by the tick.
    pub fn def(self) -> RegisterDef {
        use Register::*;
        let storied = RegisterDef {
            bright: false,
            trunk: false,
            seeds_vengeance: false,
            casting: Casting::Warmest,
            epithet_lead: "the Storied",
            epithet_other: "the Storied",
            situation_lead: "a story heavy on its shoulders.",
            situation_other: "a story heavy on its shoulders.",
        };
        match self {
            Betrayal => RegisterDef {
                trunk: true,
                seeds_vengeance: true,
                epithet_lead: "the Betrayed",
                epithet_other: "the Faithless",
                situation_lead: "still raw from a trusted friend's turning.",
                situation_other: "something unconfessed moving behind its eyes.",
                ..storied
            },
            Vengeance => RegisterDef {
                trunk: true,
                casting: Casting::Coldest,
                epithet_lead: "the Avenger",
                epithet_other: "the Hunted",
                situation_lead: "cold with a purpose it means to see through.",
                situation_other: "watchful, as one who knows it is hunted.",
                ..storied
            },
            Ambition => RegisterDef {
                casting: Casting::Ambitious,
                epithet_lead: "the Ambitious",
                epithet_other: "the Rival",
                situation_lead: "hungry for a seat it does not yet hold.",
                situation_other: "wary of a rival climbing past it.",
                ..storied
            },
            Persecution => RegisterDef {
                seeds_vengeance: true,
                casting: Casting::Coldest,
                epithet_lead: "the Hunted",
                epithet_other: "the Persecutor",
                situation_lead: "flinching, as the cornered do.",
                situation_other: "certain of its right to hound the weak.",
                ..storied
            },
            War => RegisterDef {
                casting: Casting::Ambitious,
                epithet_lead: "the Warlord",
                epithet_other: "the Enemy",
                situation_lead: "hardened by a war it cannot lay down.",
                situation_other: "an enemy's shadow never far from its mind.",
                ..storied
            },
            Disaster => RegisterDef {
                epithet_lead: "the Stricken",
                epithet_other: "the Bereaved",
                situation_lead: "hollowed by a ruin that fell on its house.",
                situation_other: "grieving a loss the famine took.",
                ..storied
            },
            Loss => RegisterDef {
                seeds_vengeance: true,
                ..storied
            },
            Romance => RegisterDef {
                bright: true,
                seeds_vengeance: true,
                epithet_lead: "the Beloved",
                epithet_other: "the Lover",
                situation_lead: "lit, for once, by something like joy.",
                situation_other: "tender toward one it should not love.",
                ..storied
            },
            Triumph => RegisterDef {
                bright: true,
                epithet_lead: "the Triumphant",
                epithet_other: "the Eclipsed",
                situation_lead: "borne up by a triumph still warm.",
                situation_other: "smarting, eclipsed by another's rise.",
                ..storied
            },
            Wonder => RegisterDef {
                bright: true,
                casting: Casting::Pious,
                epithet_lead: "the Seeker",
                epithet_other: "the Awed",
                situation_lead: "haunted by a marvel it half-understands.",
                situation_other: "awed by something it cannot name.",
                ..storied
            },
            Reunion => RegisterDef {
                bright: true,
                casting: Casting::Pious,
                ..storied
            },
            Sacrifice => RegisterDef {
                seeds_vengeance: true,
                ..storied
            },
            Redemption => RegisterDef {
                bright: true,
                ..storied
            },
            Relief => RegisterDef {
                bright: true,
                ..storied
            },
            Grace => RegisterDef {
                bright: true,
                epithet_lead: "the Redeemed",
                epithet_other: "the Merciful",
                situation_lead: "lifted by a grace it did not earn.",
                situation_other: "moved to a mercy it did not owe.",
                ..storied
            },
        }
    }
}

impl Register {
    /// The **trunk** registers — the betrayal→vengeance spine the whole game turns on.
    /// Threads on these carry a standing drama bonus, so betrayal *dominates emergently*
    /// (decision #17), never by a hard rule.
    pub fn is_trunk(self) -> bool {
        self.def().trunk
    }

    /// Whether this register stages a **brighter** experience (love, triumph, awe). The
    /// director grooms these on purpose so the fall has something to break; they still
    /// count as *staged* experience, but weigh far below suffering (decision #8).
    pub fn is_bright(self) -> bool {
        self.def().bright
    }
}

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
/// `director::pre_ok`. A new [`Register`]: the variant + one row in [`Register::def`], plus a
/// `director::SPINES` entry if it is a spine.
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

/// One dramatic situation, as data: a storylet the director can tell.
#[derive(Deserialize, Clone, Debug)]
pub struct Beat {
    /// A stable id, also the line the story log shows ("a_rival_rises").
    pub id: String,
    /// The dramatic **register** this beat plays in — its emotional key, read by the
    /// thread machinery (a thread's `spine`) and by register-rotation. See [`Register`].
    pub register: Register,
    /// Free-form registers / tone tags — for the **novelty** penalty (don't repeat a
    /// tone twice running) and authorial filtering: e.g. `"betrayal"`, `"violence"`,
    /// `"political"`, `"survival"`. Orthogonal to the formal [`Beat::register`].
    #[serde(default)]
    pub tags: Vec<String>,
    /// Which arc [`Phase`]s this beat suits (groom→climax→fall). Empty = any phase.
    #[serde(default)]
    pub phases: Vec<Phase>,
    /// How much dramatic **tension** telling this beat injects. Escalations are
    /// positive; *relief* beats (a reconciliation, a respite) are negative.
    pub tension: f32,
    /// The **stakes** — how much the target stands to lose or gain (a life, a throne, a
    /// bond, a love). A multiplier in the drama objective (`drama = stakes × attachment ×
    /// reversal`); high for climactic beats, low for grooming.
    #[serde(default = "one")]
    pub stakes: f32,
    /// Base authorial weight, before the drama objective.
    #[serde(default = "one")]
    pub weight: f32,
    /// The roles this beat needs cast, in the order its effects refer to them.
    pub cast: Vec<Role>,
    /// What must hold for the beat to be tellable.
    #[serde(default)]
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
    /// The defaults shipped with the crate.
    pub fn bundled() -> Self {
        let book = Self::from_ron(Bundled::get(Asset::Beats)).expect("bundled beats are valid RON");
        book.validate(&Registry::bundled())
            .expect("bundled beats resolve against the bundled registry");
        book
    }

    /// Parse a beats document (names resolved lazily at enactment).
    pub fn from_ron(ron: &str) -> Result<Self, config::ConfigError> {
        Ok(BeatBook(config::parse(ron)?))
    }

    /// Fail fast on any trait / mood name a beat refers to that the registry doesn't
    /// know — so a typo in `beats.ron` is caught at load, not silently no-op'd.
    pub fn validate(&self, reg: &Registry) -> Result<(), String> {
        for b in &self.0 {
            for e in &b.effects {
                match e {
                    Effect::Sway { trait_name, .. } => {
                        reg.trait_id(trait_name).ok_or_else(|| {
                            format!("beat '{}': unknown trait '{trait_name}'", b.id)
                        })?;
                    }
                    Effect::Stir { mood, .. } => {
                        reg.mood_id(mood)
                            .ok_or_else(|| format!("beat '{}': unknown mood '{mood}'", b.id))?;
                    }
                    _ => {}
                }
            }
            for p in &b.pre {
                match p {
                    Pre::TraitAtLeast { trait_name, .. } | Pre::TraitAtMost { trait_name, .. } => {
                        reg.trait_id(trait_name).ok_or_else(|| {
                            format!("beat '{}': unknown trait '{trait_name}'", b.id)
                        })?;
                    }
                    Pre::MoodAtLeast { mood, .. } => {
                        reg.mood_id(mood)
                            .ok_or_else(|| format!("beat '{}': unknown mood '{mood}'", b.id))?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_beats_load_and_resolve() {
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
        for reg in [
            Register::Betrayal,
            Register::Vengeance,
            Register::Romance,
            Register::Triumph,
            Register::Wonder,
            Register::Sacrifice,
            Register::Grace,
        ] {
            assert!(
                book.0.iter().any(|b| b.register == reg),
                "no beat plays the {reg:?} register"
            );
        }
        assert!(
            book.0.iter().any(|b| b.register.is_bright()),
            "the palette has no brighter register at all"
        );
    }

    #[test]
    fn an_unknown_trait_in_a_beat_is_rejected() {
        let ron = r#"[(
            id: "bad", register: Betrayal, tension: 1.0, cast: [Protagonist],
            effects: [Sway(who: Protagonist, trait_name: "greedmaxxing", delta: 0.1)],
        )]"#;
        let book = BeatBook::from_ron(ron).unwrap();
        assert!(book.validate(&Registry::bundled()).is_err());
    }
}
