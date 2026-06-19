//! The **survival layer**: per-tile, per-day vital drain on every body — the avatar, its
//! companions, and the NPCs alike. It feeds the RPG: Constitution and the Survive skill (and
//! shelter/gear) blunt the drain. Off by default — no `Vitals` are attached and the system is
//! never added to the schedule — so a world without it is byte-identical.

use agent_core::{Npc, Player, Position, Substrate};
use bevy_ecs::prelude::*;
use rpg::{Abilities, Flags, Proficiencies};

/// A body's survival meters, `0` (critical) … `100` (fine), drained per day by the climate and
/// terrain it stands on. Thirst and warmth are lethal; stamina is a non-lethal travel buffer.
#[derive(Component, Clone, Copy, Debug)]
pub struct Vitals {
    pub thirst: f32,
    pub warmth: f32,
    pub stamina: f32,
}

impl Default for Vitals {
    fn default() -> Self {
        Self { thirst: 100.0, warmth: 100.0, stamina: 100.0 }
    }
}

impl Vitals {
    /// The most depleted of the lethal meters (thirst/warmth) — how close to death this body is.
    pub fn lowest_lethal(&self) -> f32 {
        self.thirst.min(self.warmth)
    }
}

/// Tunable survival behaviour. Self-contained here (like the party/rpg layers); the assembler
/// carries it in `Setup` and inserts it as a resource when the layer is on.
#[derive(Resource, Clone, Copy, Debug)]
pub struct SurvivalConfig {
    /// Thirst drain per day in an *arid* tile, and the extra per °C above `heat_ref`.
    pub thirst_rate: f32,
    pub heat_ref: f32,
    pub heat_thirst: f32,
    /// A tile has water at hand (so it slakes thirst) if its surface water exceeds
    /// `water_threshold` **or** its vegetation fraction exceeds `lush_threshold` — water is where
    /// things grow, so only true wastes parch you. `water_relief` is the thirst regained there.
    pub water_threshold: f32,
    pub lush_threshold: f32,
    pub water_relief: f32,
    /// Warmth drains only below `cold_ref` (temperate climes never chill you), `cold_warmth` per
    /// °C of cold; a warm tile restores warmth by `warmth_regen`.
    pub cold_ref: f32,
    pub cold_warmth: f32,
    pub warmth_regen: f32,
    /// Stamina the daily rest restores toward full (the travel layer spends it harder).
    pub stamina_recovery: f32,
    /// Mitigation: drain fraction removed per Constitution modifier and per Survive rank, capped
    /// at `max_mitigation`. A hardy, woods-wise body weathers the wastes far longer.
    pub con_offset: f32,
    pub survive_offset: f32,
    pub max_mitigation: f32,
    /// A lethal meter at/below this kills an NPC (the avatar is left at the floor for the game).
    pub death_floor: f32,
}

impl Default for SurvivalConfig {
    fn default() -> Self {
        Self {
            thirst_rate: 3.0,
            heat_ref: 25.0,
            heat_thirst: 0.2,
            water_threshold: 0.15,
            lush_threshold: 0.2,
            water_relief: 12.0,
            cold_ref: 2.0,
            cold_warmth: 0.4,
            warmth_regen: 10.0,
            stamina_recovery: 12.0,
            con_offset: 0.10,
            survive_offset: 0.08,
            max_mitigation: 0.6,
            death_floor: 0.0,
        }
    }
}

/// Drain every body's [`Vitals`] for the day from the tile it stands on — thirst rising with heat
/// and aridity, warmth falling with the cold — blunted by Constitution, the Survive skill, and
/// shelter/gear flags. An NPC whose thirst or warmth bottoms out dies (the same sink as
/// starvation); the avatar is left at the floor for the game to handle. Deterministic — drains are
/// a pure function of tile data and integer stats, no RNG.
pub fn survival_metabolism(
    mut commands: Commands,
    mut bodies: Query<
        (Entity, &mut Vitals, &Position, Option<&Npc>, Option<&Abilities>, Option<&Proficiencies>, Option<&Flags>),
        Or<(With<Npc>, With<Player>)>,
    >,
    substrate: Res<Substrate>,
    cfg: Res<SurvivalConfig>,
    data: Option<Res<rpg::RpgData>>,
) {
    let world = &substrate.0;
    let survive_id = data.as_ref().and_then(|d| d.skill_id("Survive"));
    for (e, mut v, pos, npc, ab, prof, flags) in &mut bodies {
        let temp = world.temperature(pos.0);
        let water = world.surface_water(pos.0);

        // Hardiness: Constitution + Survive blunt the day's drain (capped).
        let con = ab.map_or(0, |a| a.modifier(rpg::CON));
        let survive = prof.and_then(|p| survive_id.map(|i| p.rank(i) as i32)).unwrap_or(0);
        let mitigation = (con as f32 * cfg.con_offset + survive as f32 * cfg.survive_offset).clamp(0.0, cfg.max_mitigation);
        let keep = 1.0 - mitigation;

        // Thirst: green or wet land has water at hand; only arid wastes parch you, worse in heat.
        let biomass_frac = (world.plant_biomass(pos.0) / world.params().biomass_max).clamp(0.0, 1.0);
        if water > cfg.water_threshold || biomass_frac > cfg.lush_threshold {
            v.thirst = (v.thirst + cfg.water_relief).min(100.0);
        } else {
            let drain = cfg.thirst_rate + (temp - cfg.heat_ref).max(0.0) * cfg.heat_thirst;
            v.thirst = (v.thirst - drain * keep).max(0.0);
        }

        // Warmth: only the cold drains it (temperate climes are fine and restore it); a sheltered
        // or warm-geared body loses it half as fast.
        let cold = (cfg.cold_ref - temp).max(0.0);
        if cold > 0.0 {
            let sheltered = flags.is_some_and(|f| f.has("sheltered") || f.has("warm_gear"));
            let drain = cold * cfg.cold_warmth * if sheltered { 0.5 } else { 1.0 };
            v.warmth = (v.warmth - drain * keep).max(0.0);
        } else {
            v.warmth = (v.warmth + cfg.warmth_regen).min(100.0);
        }

        // Stamina: the daily rest refills it; the travel layer spends it harder.
        v.stamina = (v.stamina + cfg.stamina_recovery).min(100.0);

        // Death on a lethal meter — NPCs only; the avatar is left at the floor for the game.
        if npc.is_some() && (v.thirst <= cfg.death_floor || v.warmth <= cfg.death_floor) {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vitals_start_full() {
        assert_eq!(Vitals::default().lowest_lethal(), 100.0);
    }
}
