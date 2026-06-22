//! Significant life events and how an agent **appraises** them — the mechanism by
//! which an *innate* personality nonetheless changes over a lifetime.
//!
//! Traits don't decay or grow by repetition (that would make them skills). They
//! shift only when something that matters happens: being crowned, being deposed,
//! being wronged. An appraisal applies a **persistent** delta to a trait — and,
//! through opposed pairs, the opposite to its opposite — so the change sticks and
//! is bounded. What each event *means* is authored in `appraisals.ron` (an agent's
//! values), not hardwired. Fast, fading feeling is the separate mood layer.

use crate::data::{MoodId, Registry, TraitId};
use crate::people::{Mood, Npc, Personality};
use bevy_ecs::prelude::*;
use config::{Asset, Config};
use serde::Deserialize;
use std::collections::HashMap;

/// Something significant that happened to an agent, awaiting appraisal.
#[derive(Clone, Copy, Debug)]
pub enum AgentEvent {
    /// Took the throne.
    Crowned,
    /// Was usurped from the throne.
    Deposed,
    /// Crossed a line — carried out an act the prevailing norms forbade (and did not
    /// excuse). Appraised on the *transgressor*: breaking a taboo leaves a mark on
    /// who they are (see `appraisals.ron`), which in turn colours how readily they
    /// break the next one.
    Transgressed,
}

impl AgentEvent {
    /// The key this event is appraised under in `appraisals.ron`.
    pub fn key(self) -> &'static str {
        match self {
            AgentEvent::Crowned => "Crowned",
            AgentEvent::Deposed => "Deposed",
            AgentEvent::Transgressed => "Transgressed",
        }
    }
}

/// Events that occurred this tick, awaiting appraisal. Filled by the act systems,
/// drained by [`appraise`].
#[derive(Resource, Default)]
pub struct EventQueue(pub Vec<(Entity, AgentEvent)>);

/// The persistent trait shifts and transient mood spikes one event produces.
#[derive(Clone, Debug, Default)]
struct EventEffects {
    traits: Vec<(TraitId, f32)>,
    moods: Vec<(MoodId, f32)>,
}

/// What each kind of event does to an agent — its authored "values": which traits
/// it lastingly shifts and which moods it stirs.
#[derive(Resource, Clone, Debug)]
pub struct Appraisals(HashMap<String, EventEffects>);

impl Appraisals {
    /// The defaults shipped with the crate — the bundled content set.
    pub fn bundled(reg: &Registry) -> Self {
        Self::load(&Config::bundled(), reg).expect("bundled appraisals are valid")
    }

    /// Load the appraisals from a [`Config`]'s content source, resolving trait
    /// names against `reg`.
    pub fn load(cfg: &Config, reg: &Registry) -> Result<Self, AppraisalError> {
        Self::from_defs(cfg.load(Asset::Appraisals)?, reg)
    }

    /// Parse and resolve an appraisals document.
    pub fn from_ron(ron: &str, reg: &Registry) -> Result<Self, AppraisalError> {
        Self::from_defs(config::parse(ron)?, reg)
    }

    /// Resolve already-parsed appraisal definitions against the registry.
    fn from_defs(defs: Vec<AppraisalDef>, reg: &Registry) -> Result<Self, AppraisalError> {
        let mut map = HashMap::new();
        for d in defs {
            let traits = d
                .traits
                .into_iter()
                .map(|(n, delta)| {
                    reg.trait_id(&n)
                        .map(|t| (t, delta))
                        .ok_or(AppraisalError::UnknownTrait(n))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let moods = d
                .moods
                .into_iter()
                .map(|(n, delta)| {
                    reg.mood_id(&n)
                        .map(|m| (m, delta))
                        .ok_or(AppraisalError::UnknownMood(n))
                })
                .collect::<Result<Vec<_>, _>>()?;
            map.insert(d.event, EventEffects { traits, moods });
        }
        Ok(Appraisals(map))
    }

    fn effects(&self, event: &str) -> Option<&EventEffects> {
        self.0.get(event)
    }
}

impl Default for Appraisals {
    fn default() -> Self {
        Self::bundled(&Registry::bundled())
    }
}

#[derive(Deserialize)]
struct AppraisalDef {
    event: String,
    #[serde(default)]
    traits: Vec<(String, f32)>,
    #[serde(default)]
    moods: Vec<(String, f32)>,
}

/// Apply this tick's events: each shifts the agent's traits *persistently* (and
/// their opposites the other way) and stirs its moods *transiently*. Runs after the
/// act systems but before metabolism, so a just-deposed agent is appraised while it
/// still lives.
pub(crate) fn appraise(
    mut events: ResMut<EventQueue>,
    appraisals: Res<Appraisals>,
    reg: Res<Registry>,
    mut people: Query<(&mut Personality, &mut Mood), With<Npc>>,
) {
    for (e, event) in events.0.drain(..) {
        let Ok((mut p, mut mood)) = people.get_mut(e) else {
            continue;
        };
        let Some(eff) = appraisals.effects(event.key()) else {
            continue;
        };
        for &(t, delta) in &eff.traits {
            p.0[t] = (p.0[t] + delta).clamp(0.0, 1.0);
            if let Some(o) = reg.opposes(t) {
                p.0[o] = (p.0[o] - delta).clamp(0.0, 1.0);
            }
        }
        for &(m, delta) in &eff.moods {
            mood.0[m] = (mood.0[m] + delta).clamp(0.0, 1.0);
            // A spike in one feeling damps its opposite — you can't be furious and
            // serene at once (mood coherence).
            if let Some(o) = reg.mood_opposes(m) {
                mood.0[o] = (mood.0[o] - delta).clamp(0.0, 1.0);
            }
        }
    }
}

/// Why loading appraisals failed.
#[derive(Debug)]
pub enum AppraisalError {
    Config(config::ConfigError),
    UnknownTrait(String),
    UnknownMood(String),
}
impl std::fmt::Display for AppraisalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppraisalError::Config(e) => write!(f, "loading appraisals: {e}"),
            AppraisalError::UnknownTrait(n) => write!(f, "appraisal refers to unknown trait '{n}'"),
            AppraisalError::UnknownMood(n) => write!(f, "appraisal refers to unknown mood '{n}'"),
        }
    }
}
impl std::error::Error for AppraisalError {}
impl From<config::ConfigError> for AppraisalError {
    fn from(e: config::ConfigError) -> Self {
        AppraisalError::Config(e)
    }
}
