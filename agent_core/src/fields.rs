//! **Stigmergic fields** and the **Tier-1 "drifter" brain** (`docs/scaling.md`, Track 2 / 2b).
//!
//! The substrate carries a handful of generic [`game_sim::StigConfig`] layers (deposited,
//! diffused, decayed there — see [`game_sim::World::install_stigmergy`]); this module assigns
//! them *meaning* ([`FOOD`]/[`DANGER`]/[`DEMAND`]), keeps them fed (the `deposit_*` systems),
//! and lets distant agents live cheaply by **following the gradient instead of planning**
//! (the [`drift`] system).
//!
//! Why this exists: per-agent GOAP A\* is ~98% of the tick (measured — `bench_scaling`), and
//! `par_iter` only buys a constant factor. To reach the masses we must drop the *complexity
//! class* for most agents: a drifter samples its six neighbours (`O(1)`) and steps uphill
//! toward food/demand and away from danger, meeting its needs by local rules. The cast near
//! the avatar stays full-brain (Tier 0); everyone beyond the LOD radius is a drifter (Tier 1).
//!
//! **Determinism / off-by-default.** No field draws RNG (diffusion is a stencil; gradient
//! ties break on a fixed entity-derived rotation, not a die). The whole layer is gated on the
//! [`FieldsConfig`] resource: absent → every `deposit_*`/`drift` system early-returns, no layer
//! is installed, and no agent is a [`Drifter`](crate::Drifter), so the world is byte-identical.

use crate::factions::Detained;
use crate::people::{
    EconRes, Inventory, Market, MoveGraph, Needs, NeedsRes, Npc, Skills, deplete_resource, price,
    read_resources,
};
use crate::{Position, Registry, Substrate};
use bevy_ecs::prelude::*;
use game_sim::{Coord, StigConfig};
use std::collections::HashMap;

/// Stigmergy layer indices — the meaning `agent_core` assigns to the substrate's generic
/// layers. The assembler installs the layers in this exact order (see [`FieldsConfig::layers`]).
pub const FOOD: usize = 0;
pub const DANGER: usize = 1;
pub const DEMAND: usize = 2;
/// How many layers the fields layer installs.
pub const LAYER_COUNT: usize = 3;

/// Tunables for the stigmergic-fields / Tier-1 layer. Held as a resource so its presence is the
/// on/off switch: absent ⇒ the whole layer is inert and the world is byte-identical.
#[derive(Resource, Clone, Copy, Debug)]
pub struct FieldsConfig {
    /// Transport (diffuse/decay) for the **food** scent — fed from tile biomass.
    pub food: StigConfig,
    /// Transport for the **danger** field — fed at each predator's tile.
    pub danger: StigConfig,
    /// Transport for the **demand** field — fed at markets short of stock.
    pub demand: StigConfig,
    /// Food scent deposited per tick per unit of (0..1 normalised) tile biomass.
    pub food_gain: f32,
    /// Danger deposited per tick at each predator's tile.
    pub danger_gain: f32,
    /// Demand deposited per tick per unit of a market's (0..1 normalised) stock deficit.
    pub demand_gain: f32,
    /// Drift pull toward the food gradient (scaled up as a drifter starves).
    pub w_food: f32,
    /// Drift pull toward the demand gradient (active when a drifter carries surplus to sell).
    pub w_demand: f32,
    /// Drift push away from the danger gradient (always active).
    pub w_danger: f32,
    /// Sustenance (0..100) below which a drifter abandons trade and seeks food.
    pub hunger_threshold: f32,
}

impl Default for FieldsConfig {
    fn default() -> Self {
        Self {
            food: StigConfig {
                diffuse: 0.20,
                decay: 0.10,
            },
            danger: StigConfig {
                diffuse: 0.25,
                decay: 0.25,
            },
            demand: StigConfig {
                diffuse: 0.20,
                decay: 0.12,
            },
            food_gain: 1.0,
            danger_gain: 5.0,
            demand_gain: 1.0,
            w_food: 1.0,
            w_demand: 0.6,
            w_danger: 1.5,
            hunger_threshold: 50.0,
        }
    }
}

impl FieldsConfig {
    /// The per-layer transport configs in install order — the array the assembler hands to
    /// [`game_sim::World::install_stigmergy`]. Order must match [`FOOD`]/[`DANGER`]/[`DEMAND`].
    pub fn layers(&self) -> [StigConfig; LAYER_COUNT] {
        [self.food, self.danger, self.demand]
    }
}

/// **Deposit food scent** from standing biomass — a diffusing "there is grazing here" signal a
/// hungry drifter several tiles away can still sense. `O(tiles)`, independent of agent count;
/// runs before `Φ` so this tick's deposit diffuses this tick. No-op when the layer is off.
pub(crate) fn deposit_food(mut substrate: ResMut<Substrate>, cfg: Option<Res<FieldsConfig>>) {
    let Some(cfg) = cfg else { return };
    let max = substrate.0.params().biomass_max.max(1e-6);
    let n = substrate.0.topology().len();
    for i in 0..n {
        let c = substrate.0.topology().coord(i);
        let frac = (substrate.0.plant_biomass(c) / max).clamp(0.0, 1.0);
        if frac > 0.0 {
            substrate.0.deposit(FOOD, c, cfg.food_gain * frac);
        }
    }
}

/// **Deposit danger** at every predator's tile — the field drifters flee. `O(predators)`.
/// No-op when the layer is off.
pub(crate) fn deposit_danger(
    carnivores: Query<&Position, With<crate::fauna::Carnivore>>,
    mut substrate: ResMut<Substrate>,
    cfg: Option<Res<FieldsConfig>>,
) {
    let Some(cfg) = cfg else { return };
    for &Position(c) in &carnivores {
        substrate.0.deposit(DANGER, c, cfg.danger_gain);
    }
}

/// **Deposit demand** at markets running short of stock — "goods wanted here", the gradient a
/// drifter carrying surplus climbs to find a buyer. `O(markets · goods)`. No-op when off.
pub(crate) fn deposit_demand(
    markets: Query<(&Position, &Market), Without<Npc>>,
    mut substrate: ResMut<Substrate>,
    reg: Res<Registry>,
    cfg: Option<Res<FieldsConfig>>,
) {
    let Some(cfg) = cfg else { return };
    // Collect (tile, deficit) first, so the market-query borrow is released before the deposit.
    let deposits: Vec<(Coord, f32)> = markets
        .iter()
        .map(|(p, m)| {
            let mut deficit = 0.0;
            for g in 0..m.stock.len() {
                let target = reg.good(g).target_stock.max(1) as f32;
                let have = m.stock[g] as f32;
                deficit += ((target - have) / target).clamp(0.0, 1.0);
            }
            (p.0, deficit)
        })
        .collect();
    for (c, deficit) in deposits {
        if deficit > 0.0 {
            substrate.0.deposit(DEMAND, c, cfg.demand_gain * deficit);
        }
    }
}

/// **The Tier-1 brain.** For each [`Drifter`](crate::Drifter) — every NPC beyond the LOD radius
/// — run one cheap turn with *no* A\* search: step one tile up the relevant gradient (toward food
/// when hungry, toward demand when carrying surplus, always away from danger), then meet needs by
/// local rules — produce a calling-good if the tile allows, square up at an adjacent market, and
/// eat or graze. `O(1)` per agent (six-neighbour sample + a fixed handful of recipe/market checks),
/// so even serial this is orders of magnitude under GOAP. Serial like `people_execute` because it
/// trades against shared markets and writes the substrate; deterministic in ECS iteration order.
///
/// No-op when the fields layer is off (no `FieldsConfig`) — so a world without it is byte-identical.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn drift(
    mut npcs: Query<
        (
            Entity,
            &mut Needs,
            &mut Inventory,
            &mut Skills,
            &mut Position,
            Option<&Detained>,
        ),
        (With<Npc>, With<crate::Drifter>, Without<crate::Suspended>),
    >,
    mut markets: Query<(Entity, &Position, &mut Market), Without<Npc>>,
    mut substrate: ResMut<Substrate>,
    move_graph: Res<MoveGraph>,
    reg: Res<Registry>,
    econ: Res<EconRes>,
    needs_cfg: Res<NeedsRes>,
    // The same hunger model `people_execute` honours: grazing gives flat relief in a plain economy
    // (the default) and tile-biomass-scaled relief under the survival layer. A drifter must use the
    // *same* rule, or it grazes far worse than the full-brain cast and starves where they thrive.
    hunger: Option<Res<crate::HungerModel>>,
    cfg: Option<Res<FieldsConfig>>,
) {
    let Some(cfg) = cfg else { return };
    let hunger_model = hunger.as_deref().copied().unwrap_or_default();

    // Where each market sits, by tile index — so a drifter checks its own tile + neighbours
    // against this in O(1) instead of scanning every market (O(drifters·markets) → O(drifters)).
    let market_at: HashMap<usize, Entity> = {
        let topo = substrate.0.topology();
        markets
            .iter()
            .map(|(e, p, _)| (topo.index_of(p.0), e))
            .collect()
    };

    for (entity, mut needs, mut inv, mut skills, mut pos, detained) in &mut npcs {
        if detained.is_some() {
            continue; // held by enforcers — cannot act
        }
        let here = pos.0;
        let hungry = needs.sustenance < cfg.hunger_threshold;
        // Surplus = a non-food good held to sell (food is kept to eat). Only chase demand when fed.
        let has_surplus = !hungry
            && (0..inv.stock.len()).any(|g| inv.stock[g] > 0 && reg.good(g).nutrition == 0.0);

        // --- 1. Move one tile up the weighted gradient (staying put is allowed). ---
        // A starving drifter weights food more heavily, so the scent overrides everything else.
        let starving = (cfg.hunger_threshold - needs.sustenance).max(0.0) / cfg.hunger_threshold;
        let w_food = if hungry {
            cfg.w_food * (1.0 + starving)
        } else {
            0.0
        };
        let w_demand = if has_surplus { cfg.w_demand } else { 0.0 };
        let w_danger = cfg.w_danger;
        let dest = {
            let sub = &substrate.0;
            let hi = sub.topology().index_of(here);
            let neighbors = move_graph.neighbors(hi);
            let score = |c: Coord| -> f32 {
                w_food * sub.stig(FOOD, c) + w_demand * sub.stig(DEMAND, c)
                    - w_danger * sub.stig(DANGER, c)
            };
            let mut best = here;
            let mut best_score = score(here);
            if !neighbors.is_empty() {
                // Rotate the scan start by entity id so equal-scoring neighbours don't all draw
                // every drifter to the same tile — a deterministic spread, no RNG.
                let nlen = neighbors.len();
                let start = (entity.to_bits() as usize) % nlen;
                for k in 0..nlen {
                    let c = neighbors[(start + k) % nlen];
                    let s = score(c);
                    if s > best_score {
                        best_score = s;
                        best = c;
                    }
                }
            }
            best
        };
        pos.0 = dest;

        // --- 2. Produce: run the first calling-recipe this tile + larder supports (one/tick). ---
        for r in reg.recipes() {
            if !skills.0.get(r.skill).is_some_and(|&s| s > 0.0) {
                continue;
            }
            let level = r
                .resource
                .map(|k| read_resources(&substrate.0, dest)[k.idx()]);
            let resource_ok = level.is_none_or(|l| l >= r.min_resource);
            let inputs_ok = !r.inputs.iter().any(|&(g, qty)| inv.stock[g] < qty);
            if resource_ok && inputs_ok {
                let scale = level.unwrap_or(1.0);
                for &(g, qty) in &r.inputs {
                    inv.stock[g] -= qty;
                }
                let skill = skills.0[r.skill];
                for &(g, qty) in &r.outputs {
                    inv.stock[g] += (qty as f32 * (1.0 + skill) * scale).round() as u32;
                }
                let sd = reg.skill(r.skill);
                skills.0[r.skill] = (skill + sd.gain).min(sd.cap);
                if let Some(kind) = r.resource
                    && r.deplete > 0.0
                {
                    deplete_resource(&mut substrate.0, dest, kind, r.deplete);
                }
                break;
            }
        }

        // --- 3. Trade at an adjacent market: buy food when hungry, else sell the top surplus. ---
        let market_ent = {
            let topo = substrate.0.topology();
            let di = topo.index_of(dest);
            market_at.get(&di).copied().or_else(|| {
                topo.neighbors(di)
                    .iter()
                    .find_map(|l| market_at.get(&l.to).copied())
            })
        };
        if let Some(me) = market_ent
            && let Ok((_, _, mut m)) = markets.get_mut(me)
        {
            if hungry {
                // Buy one unit of the most nutritious good the market stocks and we can afford.
                let pick = (0..m.stock.len())
                    .filter(|&g| m.stock[g] > 0 && reg.good(g).nutrition > 0.0)
                    .max_by(|&a, &b| reg.good(a).nutrition.total_cmp(&reg.good(b).nutrition));
                if let Some(g) = pick {
                    let p = price(&reg, &econ, g, m.price_basis[g].round().max(0.0) as u32);
                    if inv.money >= p {
                        inv.money -= p;
                        m.money += p;
                        m.stock[g] -= 1;
                        inv.stock[g] += 1;
                    }
                }
            } else {
                // Sell one unit of the non-food good we hold the most of, if the market can pay.
                let pick = (0..inv.stock.len())
                    .filter(|&g| inv.stock[g] > 0 && reg.good(g).nutrition == 0.0)
                    .max_by_key(|&g| inv.stock[g]);
                if let Some(g) = pick {
                    let p = price(&reg, &econ, g, m.price_basis[g].round().max(0.0) as u32);
                    if m.money >= p {
                        inv.stock[g] -= 1;
                        inv.money += p;
                        m.stock[g] += 1;
                        m.money -= p;
                    }
                }
            }
        }

        // --- 4. Subsist: eat a held food if hungry, else graze whatever the tile bears. ---
        if hungry {
            let food = (0..inv.stock.len())
                .filter(|&g| inv.stock[g] > 0 && reg.good(g).nutrition > 0.0)
                .max_by(|&a, &b| reg.good(a).nutrition.total_cmp(&reg.good(b).nutrition));
            if let Some(g) = food {
                inv.stock[g] -= 1;
                needs.sustenance = (needs.sustenance + reg.good(g).nutrition).min(100.0);
            } else {
                // Graze the tile — flat relief in a plain economy, biomass-scaled under survival —
                // exactly as `people_execute` does, so a drifter feeds itself as well as a planner.
                let relief = match hunger_model {
                    crate::HungerModel::Flat => needs_cfg.eat_grass_relief,
                    crate::HungerModel::TileBiomass => {
                        let frac = (substrate.0.plant_biomass(dest)
                            / substrate.0.params().biomass_max)
                            .clamp(0.0, 1.0);
                        needs_cfg.eat_grass_relief * frac
                    }
                };
                substrate.0.graze(dest, 1.0);
                needs.sustenance = (needs.sustenance + relief).min(100.0);
            }
        }
    }
}
