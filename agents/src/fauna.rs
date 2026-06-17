//! Simple instinct-driven fauna — herds that graze the substrate and breed, and
//! the packs that hunt them. The stigmergic consumers beneath the NPCs: greedy
//! movement, no economy, no utility brain. Together they close the trophic loop —
//! vegetation → herbivore → carnivore — so the herbivore population is held in
//! check from *above* (predation) as well as *below* (forage), the two-sided
//! regulation a single level can't provide.

use crate::{Position, Substrate};
use bevy_ecs::prelude::*;
use game_sim::{SplitMix64, World as GameWorld};
use sim::Rng;
use std::collections::HashMap;

/// Survival meter: gained by feeding, spent on metabolism. Zero → death.
#[derive(Component, Clone, Copy, Debug)]
pub struct Energy(pub f32);

/// Marks a grazing herbivore.
#[derive(Component, Clone, Copy, Debug)]
pub struct Herbivore;

/// Marks a predator that hunts [`Herbivore`]s.
#[derive(Component, Clone, Copy, Debug)]
pub struct Carnivore;

/// Randomness for predation, kept on its **own** stream so the stochastic kill
/// rounding never perturbs the substrate's RNG (and so a world with no predators is
/// bit-identical to before this layer existed).
#[derive(Resource)]
pub struct FaunaRng(pub SplitMix64);

/// Tunable fauna behaviour (global knobs only).
#[derive(Resource, Clone, Debug)]
pub struct FaunaConfig {
    pub initial_energy: f32,
    pub metabolism: f32,
    pub intake: f32,
    pub eat_rate: f32,
    pub repro_threshold: f32,
    pub repro_cost: f32,
    /// Most herbivores that may breed on one tile in a tick — a crowding cap, the
    /// density-dependent regulation that stops the herd overshooting its forage and
    /// mass-starving (the boom-bust a pure logistic alone suffers).
    pub herd_cap: usize,
    /// How strongly herbivores are drawn to **company** when choosing where to move,
    /// relative to forage — a herding instinct. It makes scattered animals coalesce
    /// into moving herds dense enough to graze efficiently *and* to be worth a
    /// predator's hunt, so prey isn't so thinly spread the trophic loop starves at the
    /// top. `0` = pure ideal-free dispersal by forage alone.
    pub herd_cohesion: f32,

    // --- Carnivores ---
    pub carn_initial_energy: f32,
    pub carn_metabolism: f32,
    /// Holling type-II **attack rate** `a` and **handling time** `h`: per-tick kills
    /// are `a·N / (1 + a·h·N)` for `N` prey on the tile — so intake *saturates* as
    /// prey gets dense (a predator can only process so many), the response that
    /// stabilises real predator–prey systems where a linear (type-I) one wouldn't.
    pub carn_attack: f32,
    pub carn_handling: f32,
    /// Energy a predator gains per kill.
    pub carn_energy_per_kill: f32,
    pub carn_repro_threshold: f32,
    pub carn_repro_cost: f32,
    /// Prey on a tile holding fewer than this many are safe — a **spatial refuge**
    /// (scattered animals hide where a pack can't profitably hunt). It is what stops
    /// predators chasing the herd to total extinction, so the loop can persist
    /// instead of collapsing (a known stabiliser against the paradox of enrichment).
    pub carn_prey_refuge: usize,
}

impl Default for FaunaConfig {
    fn default() -> Self {
        Self {
            initial_energy: 50.0,
            metabolism: 0.3,
            intake: 0.8,
            eat_rate: 8.0,
            repro_threshold: 80.0,
            repro_cost: 40.0,
            herd_cap: 5,
            herd_cohesion: 3.0,

            carn_initial_energy: 60.0,
            // Patient predators: low upkeep so they ride out lean stretches between
            // kills, moderate attack and slow breeding so a pack tracks the herd
            // rather than over-culling it and starving (the predator–prey collapse).
            carn_metabolism: 0.55,
            carn_attack: 0.5,
            carn_handling: 1.2,
            carn_energy_per_kill: 18.0,
            carn_repro_threshold: 170.0,
            carn_repro_cost: 80.0,
            carn_prey_refuge: 2,
        }
    }
}

/// Each herbivore steps to a nearby tile (here and the six neighbours) chosen in
/// proportion to its **forage plus company** — `biomass + herd_cohesion · herd here`
/// — then grazes it (depleting the substrate, the stigmergic write), paying
/// metabolism. The forage term spreads animals onto good ground (an ideal-free
/// distribution that damps overgrazing); the cohesion term draws them together into
/// *moving herds* — dense enough to graze efficiently and to be worth hunting, yet
/// migratory, so they don't overgraze a single tile. On barren, empty ground a lone
/// animal wanders to find food. Draws from the fauna RNG, so it stays seeded.
pub(crate) fn forage(
    mut fauna: Query<(&mut Position, &mut Energy), With<Herbivore>>,
    mut substrate: ResMut<Substrate>,
    config: Res<FaunaConfig>,
    mut rng: ResMut<FaunaRng>,
) {
    // Start-of-tick herd density per tile, so cohesion pulls toward where the herd is.
    let density: HashMap<usize, usize> = {
        let topo = substrate.0.topology();
        let mut d = HashMap::new();
        for (pos, _) in fauna.iter() {
            *d.entry(topo.index_of(pos.0)).or_default() += 1;
        }
        d
    };

    for (mut position, mut energy) in &mut fauna {
        let dest = {
            let world = &substrate.0;
            let topo = world.topology();
            let here = topo.index_of(position.0);
            let appeal = |i: usize, c: game_sim::Coord| {
                world.plant_biomass(c) + config.herd_cohesion * density.get(&i).copied().unwrap_or(0) as f32
            };
            let mut cands: Vec<(game_sim::Coord, f32)> = vec![(position.0, appeal(here, position.0))];
            for link in topo.neighbors(here) {
                cands.push((topo.coord(link.to), appeal(link.to, topo.coord(link.to))));
            }
            let total: f32 = cands.iter().map(|&(_, w)| w).sum();
            if total > 0.0 {
                let mut t = rng.0.next_f64() as f32 * total;
                let mut chosen = cands[0].0;
                for &(c, w) in &cands {
                    if t < w {
                        chosen = c;
                        break;
                    }
                    t -= w;
                }
                chosen
            } else {
                // All barren and empty — wander to look for greener ground.
                cands[rng.0.gen_range(cands.len())].0
            }
        };
        position.0 = dest;
        let grazed = substrate.0.graze(dest, config.intake);
        energy.0 += config.eat_rate * grazed - config.metabolism;
    }
}

/// Births and deaths: starve at zero energy, breed when well fed — but only where
/// the tile isn't already crowded past [`FaunaConfig::herd_cap`], so the herd can't
/// pile up density without limit on a single rich tile.
pub(crate) fn lifecycle(
    mut commands: Commands,
    mut fauna: Query<(Entity, &mut Energy, &Position), With<Herbivore>>,
    substrate: Res<Substrate>,
    config: Res<FaunaConfig>,
) {
    let topo = substrate.0.topology();
    let mut density: HashMap<usize, usize> = HashMap::new();
    for (_, _, position) in &fauna {
        *density.entry(topo.index_of(position.0)).or_default() += 1;
    }
    for (entity, mut energy, position) in &mut fauna {
        if energy.0 <= 0.0 {
            commands.entity(entity).despawn();
        } else if energy.0 >= config.repro_threshold {
            let crowded = density.get(&topo.index_of(position.0)).copied().unwrap_or(0) >= config.herd_cap;
            if !crowded {
                energy.0 -= config.repro_cost;
                commands.spawn((Herbivore, Position(position.0), Energy(config.repro_cost)));
            }
        }
    }
}

/// Predation. Each carnivore steps to the neighbouring tile holding the most prey,
/// then kills from it at the Holling type-II rate `a·N / (1 + a·h·N)` — a count that
/// rises with local prey density `N` but **saturates** (the predator can only
/// handle so many), which is what keeps the loop from running away. The fractional
/// part of the rate is resolved by a single draw from the fauna RNG, so a kill is
/// stochastic but reproducible. A fed predator gains energy; a hungry one only pays
/// metabolism and dwindles, so predators concentrate where prey is and thin out
/// where it isn't — the top-down half of the herd's regulation.
#[allow(clippy::type_complexity)]
pub(crate) fn hunt(
    mut commands: Commands,
    mut carnivores: Query<(&mut Position, &mut Energy), With<Carnivore>>,
    herbivores: Query<(Entity, &Position), (With<Herbivore>, Without<Carnivore>)>,
    substrate: Res<Substrate>,
    config: Res<FaunaConfig>,
    mut rng: ResMut<FaunaRng>,
) {
    let topo = substrate.0.topology();
    // Live prey by tile, in stable query order (so `drain` is deterministic).
    let mut prey: HashMap<usize, Vec<Entity>> = HashMap::new();
    for (e, p) in &herbivores {
        prey.entry(topo.index_of(p.0)).or_default().push(e);
    }
    let count_at = |prey: &HashMap<usize, Vec<Entity>>, i: usize| prey.get(&i).map_or(0, Vec::len);

    for (mut pos, mut energy) in &mut carnivores {
        let here = topo.index_of(pos.0);
        let mut best = here;
        let mut best_n = count_at(&prey, here);
        for l in topo.neighbors(here) {
            let n = count_at(&prey, l.to);
            if n > best_n {
                best_n = n;
                best = l.to;
            }
        }
        pos.0 = topo.coord(best);

        let n = best_n as f32;
        // Below the refuge density there is nothing worth hunting — scattered prey hide.
        let rate = if best_n >= config.carn_prey_refuge {
            config.carn_attack * n / (1.0 + config.carn_attack * config.carn_handling * n)
        } else {
            0.0
        };
        let mut kills = rate.floor() as usize;
        if rng.0.gen_bool((rate - rate.floor()).clamp(0.0, 1.0) as f64) {
            kills += 1;
        }
        let mut killed = 0;
        if let Some(list) = prey.get_mut(&best) {
            let k = kills.min(list.len());
            for e in list.drain(0..k) {
                commands.entity(e).despawn();
                killed += 1;
            }
        }
        energy.0 += config.carn_energy_per_kill * killed as f32 - config.carn_metabolism;
    }
}

/// Predator births and deaths, mirroring [`lifecycle`] with the carnivore knobs.
pub(crate) fn carnivore_lifecycle(
    mut commands: Commands,
    mut carnivores: Query<(Entity, &mut Energy, &Position), With<Carnivore>>,
    config: Res<FaunaConfig>,
) {
    for (entity, mut energy, position) in &mut carnivores {
        if energy.0 <= 0.0 {
            commands.entity(entity).despawn();
        } else if energy.0 >= config.carn_repro_threshold {
            energy.0 -= config.carn_repro_cost;
            commands.spawn((Carnivore, Position(position.0), Energy(config.carn_repro_cost)));
        }
    }
}

/// The tiles fauna spawn on: vegetated land (where forage and so prey gather), or
/// any land if nothing has greened yet.
fn fauna_pool(substrate: &GameWorld) -> Vec<usize> {
    let topo = substrate.topology();
    let sea = substrate.params().sea_level;
    let is_land = |i: usize| substrate.elevation(topo.coord(i)) >= sea;
    let vegetated: Vec<usize> =
        topo.indices().filter(|&i| is_land(i) && substrate.plant_biomass(topo.coord(i)) > 0.2).collect();
    if vegetated.is_empty() {
        topo.indices().filter(|&i| is_land(i)).collect()
    } else {
        vegetated
    }
}

/// Place `count` herbivores on random vegetated land (else any land).
pub(crate) fn spawn_fauna(world: &mut World, substrate: &GameWorld, rng: &mut SplitMix64, count: usize, energy: f32) {
    if count == 0 {
        return;
    }
    let topo = substrate.topology();
    let pool = fauna_pool(substrate);
    if pool.is_empty() {
        return;
    }
    for _ in 0..count {
        let coord = topo.coord(pool[rng.gen_range(pool.len())]);
        world.spawn((Herbivore, Position(coord), Energy(energy)));
    }
}

/// Place `count` carnivores on the same vegetated land prey favour, so packs start
/// near herds.
pub(crate) fn spawn_carnivores(world: &mut World, substrate: &GameWorld, rng: &mut SplitMix64, count: usize, energy: f32) {
    if count == 0 {
        return;
    }
    let topo = substrate.topology();
    let pool = fauna_pool(substrate);
    if pool.is_empty() {
        return;
    }
    for _ in 0..count {
        let coord = topo.coord(pool[rng.gen_range(pool.len())]);
        world.spawn((Carnivore, Position(coord), Energy(energy)));
    }
}
