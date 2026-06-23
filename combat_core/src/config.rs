//! All TUNABLE knobs in one place (spec §14). Everything that a designer might want to retune
//! lives on [`Config`] with a documented default rather than being hard-coded in the rules.

use crate::tick::Fixed;
use serde::{Deserialize, Serialize};

/// How an action's cost is accounted. v1 ships `TickOccupancy` (an action simply occupies its
/// frames on the line); `TickPlusAp` (a separate action-point axis) is a reserved seam.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum CostModel {
    #[default]
    TickOccupancy,
    TickPlusAp,
}

/// Who may edit a committed action.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum EditLockPolicy {
    /// Once an actor commits, *its owner* can no longer Slow/Haste/Interrupt/cancel it — only the
    /// opposing faction can touch it (via verbs or contact). More honest, more tense.
    #[default]
    LockedOnCommit,
    /// The owner may re-edit its own instance while it is still in startup.
    EditableUntilActive,
}

/// How the Tempo economy is pooled.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum TempoModel {
    /// Each actor owns its own pool; both sides can dilate, enabling enemy-driven reversals.
    #[default]
    PerActorOpposed,
    /// A single player-faction pool; enemies never dilate.
    SharedPlayerPool,
}

/// The full knob surface. Construct via [`Config::default`] and override fields, or deserialize
/// a scenario's `config` block over the defaults.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub cost_model: CostModel,
    pub edit_lock_policy: EditLockPolicy,
    pub tempo_model: TempoModel,
    /// Foresight fog: hide enemy Tempo pools from the projected view.
    pub hide_enemy_tempo: bool,
    /// Damage multiplier applied when a hit lands inside a matching `Exposed` window.
    pub exposed_damage_mult: Fixed,
    /// Anti-stall: a `Hold` parks the actor for this many ticks before it is offered again.
    pub hold_quantum: u32,
    /// Tempo spend for an edit verb is `verb_base_cost + verb_per_tick_cost * magnitude`.
    pub verb_base_cost: i32,
    pub verb_per_tick_cost: i32,
    /// Tempo awarded for landing a (contact) interrupt.
    pub tempo_on_interrupt: i32,
    /// Tempo awarded for landing a hit inside an `Exposed` window (the squad payoff).
    pub tempo_on_window_hit: i32,
    /// Per-actor fallback foresight horizon when an actor doesn't specify its own.
    pub default_foresight_horizon: u32,
    /// Tempo the player party and elite enemies spawn with (mooks always spawn with 0).
    pub starting_tempo: i32,
    /// Safety bound — the sim refuses to run past this many ticks (guards against a stall).
    pub max_ticks: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cost_model: CostModel::TickOccupancy,
            edit_lock_policy: EditLockPolicy::LockedOnCommit,
            tempo_model: TempoModel::PerActorOpposed,
            hide_enemy_tempo: true,
            exposed_damage_mult: Fixed::from_ratio(3, 2), // 1.5
            hold_quantum: 4,
            verb_base_cost: 2,
            verb_per_tick_cost: 1,
            tempo_on_interrupt: 3,
            tempo_on_window_hit: 2,
            default_foresight_horizon: 24,
            starting_tempo: 6,
            max_ticks: 100_000,
        }
    }
}
