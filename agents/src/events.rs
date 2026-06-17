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
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

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
    /// The defaults shipped with the crate.
    pub fn bundled(reg: &Registry) -> Self {
        Self::from_ron(include_str!("../data/appraisals.ron"), reg).expect("bundled appraisals are valid")
    }

    /// Load `appraisals.ron` from a directory, resolving trait names against `reg`.
    pub fn load(dir: impl AsRef<Path>, reg: &Registry) -> Result<Self, AppraisalError> {
        Self::from_ron(&std::fs::read_to_string(dir.as_ref().join("appraisals.ron"))?, reg)
    }

    /// Parse and resolve an appraisals document.
    pub fn from_ron(ron: &str, reg: &Registry) -> Result<Self, AppraisalError> {
        let defs: Vec<AppraisalDef> = ron::from_str(ron)?;
        let mut map = HashMap::new();
        for d in defs {
            let traits = d
                .traits
                .into_iter()
                .map(|(n, delta)| reg.trait_id(&n).map(|t| (t, delta)).ok_or(AppraisalError::UnknownTrait(n)))
                .collect::<Result<Vec<_>, _>>()?;
            let moods = d
                .moods
                .into_iter()
                .map(|(n, delta)| reg.mood_id(&n).map(|m| (m, delta)).ok_or(AppraisalError::UnknownMood(n)))
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
        let Ok((mut p, mut mood)) = people.get_mut(e) else { continue };
        let Some(eff) = appraisals.effects(event.key()) else { continue };
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
    Io(std::io::Error),
    Ron(ron::error::SpannedError),
    UnknownTrait(String),
    UnknownMood(String),
}
impl std::fmt::Display for AppraisalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppraisalError::Io(e) => write!(f, "reading appraisals: {e}"),
            AppraisalError::Ron(e) => write!(f, "parsing appraisals: {e}"),
            AppraisalError::UnknownTrait(n) => write!(f, "appraisal refers to unknown trait '{n}'"),
            AppraisalError::UnknownMood(n) => write!(f, "appraisal refers to unknown mood '{n}'"),
        }
    }
}
impl std::error::Error for AppraisalError {}
impl From<std::io::Error> for AppraisalError {
    fn from(e: std::io::Error) -> Self {
        AppraisalError::Io(e)
    }
}
impl From<ron::error::SpannedError> for AppraisalError {
    fn from(e: ron::error::SpannedError) -> Self {
        AppraisalError::Ron(e)
    }
}
