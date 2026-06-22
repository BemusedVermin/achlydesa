//! Simple instinct-driven fauna — herds that graze the substrate and breed, and
//! the packs that hunt them. The stigmergic consumers beneath the NPCs: greedy
//! movement, no economy, no utility brain. Together they close the trophic loop —
//! vegetation → herbivore → carnivore — so the herbivore population is held in
//! check from *above* (predation) as well as *below* (forage), the two-sided
//! regulation a single level can't provide.

use crate::{Position, Substrate};
use bevy_ecs::prelude::*;
use game_sim::fields::Formation;
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

// Tunable fauna behaviour ([`FaunaConfig`]) lives Bevy-free in the `config`
// crate; re-exported here and wrapped in an ECS-resource newtype.
pub use config::FaunaConfig;

/// ECS-resource handle for the [`FaunaConfig`] knobs. Derefs to the config.
#[derive(Resource, Clone, Debug)]
pub struct FaunaRes(pub FaunaConfig);

impl std::ops::Deref for FaunaRes {
    type Target = FaunaConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// What a creature eats — the trophic role it plays.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Deserialize)]
pub enum Diet {
    Herbivore,
    Carnivore,
}

/// A creature's **body plan / gait archetype** — the handle the renderer uses to
/// build its procedural mesh and to choose how it is animated. (Sim-agnostic: the
/// simulation never reads it; it exists so authored species drive their own look.)
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Deserialize)]
pub enum Form {
    /// Tall, slender quadruped — a light bounding walk.
    Strider,
    /// Bulky, low-slung quadruped — a heavy, rolling sway.
    Lumberer,
    /// Low, sleek quadruped predator — a stalking prowl.
    Prowler,
    /// Tiny scurrier — quick, skittering steps.
    Critter,
    /// Legless body — an undulating slither.
    Serpent,
    /// Floating, hovering body — no legs, a slow bob.
    Drifter,
}

/// One authored creature kind from `assets/data/bestiary.ron`: its diet, the biomes
/// and biotemperature band it thrives in, and the body/behaviour knobs that set it
/// apart — `size` scales how much it eats and burns, `fecundity` its breeding rate,
/// `gregarious` how tightly it herds (0 solitary … ~1 dense herd/pack).
#[derive(Clone, Debug)]
pub struct Species {
    pub name: String,
    pub diet: Diet,
    pub form: Form,
    pub habitat: Vec<Formation>,
    pub min_temp: f32,
    pub max_temp: f32,
    pub size: f32,
    pub fecundity: f32,
    pub gregarious: f32,
    pub color: [f32; 3],
}

impl Species {
    /// How well a tile of this `formation` at this `biotemp` suits the species
    /// (`0` unliveable … `1` ideal): habitat-formation match × biotemperature-band
    /// match (falling off over ~5 °C beyond the tolerated band).
    pub fn suitability(&self, formation: Formation, biotemp: f32) -> f32 {
        let hab = if self.habitat.contains(&formation) {
            1.0
        } else {
            0.2
        };
        let temp = if biotemp < self.min_temp {
            1.0 - (self.min_temp - biotemp) / 5.0
        } else if biotemp > self.max_temp {
            1.0 - (biotemp - self.max_temp) / 5.0
        } else {
            1.0
        };
        (hab * temp).clamp(0.0, 1.0)
    }
}

/// The RON shape of one species: habitat as formation names, resolved to
/// [`Formation`]s when the roster loads.
#[derive(serde::Deserialize)]
struct SpeciesDef {
    name: String,
    diet: Diet,
    form: Form,
    habitat: Vec<String>,
    min_temp: f32,
    max_temp: f32,
    size: f32,
    fecundity: f32,
    gregarious: f32,
    color: (f32, f32, f32),
}

fn formation_from_str(s: &str) -> Result<Formation, String> {
    Ok(match s {
        "water" => Formation::Water,
        "desert" => Formation::Desert,
        "tundra" => Formation::Tundra,
        "grassland" => Formation::Grassland,
        "shrubland" => Formation::Shrubland,
        "forest" => Formation::Forest,
        "rainforest" => Formation::Rainforest,
        other => return Err(format!("unknown habitat formation '{other}'")),
    })
}

/// The loaded creature roster — every [`Species`] the world can host. Static
/// content (like the registry); held as a resource so the fauna systems can read it.
#[derive(Resource, Clone, Debug)]
pub struct Bestiary {
    pub species: Vec<Species>,
}

impl Bestiary {
    /// The roster baked in at compile time from `assets/data/bestiary.ron`.
    pub fn bundled() -> Self {
        Self::from_ron(config::Bundled::get(config::Asset::Bestiary))
            .expect("bundled bestiary is valid RON")
    }

    /// Parse and resolve a roster from RON text (habitat names → formations).
    pub fn from_ron(ron: &str) -> Result<Self, String> {
        let defs: Vec<SpeciesDef> = config::parse(ron).map_err(|e| e.to_string())?;
        let species = defs
            .into_iter()
            .map(|d| {
                let habitat = d
                    .habitat
                    .iter()
                    .map(|s| formation_from_str(s))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok::<_, String>(Species {
                    name: d.name,
                    diet: d.diet,
                    form: d.form,
                    habitat,
                    min_temp: d.min_temp,
                    max_temp: d.max_temp,
                    size: d.size,
                    fecundity: d.fecundity,
                    gregarious: d.gregarious,
                    color: [d.color.0, d.color.1, d.color.2],
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { species })
    }

    /// Indices of every species of a given diet.
    pub fn of_diet(&self, diet: Diet) -> Vec<usize> {
        self.species
            .iter()
            .enumerate()
            .filter(|(_, s)| s.diet == diet)
            .map(|(i, _)| i)
            .collect()
    }
}

/// The species a fauna entity belongs to — an index into the [`Bestiary`].
#[derive(Component, Clone, Copy, Debug)]
pub struct SpeciesId(pub usize);

/// Each herbivore steps to a nearby tile (here and the six neighbours) chosen in
/// proportion to its **forage plus company** — `biomass + herd_cohesion · herd here`
/// — then grazes it (depleting the substrate, the stigmergic write), paying
/// metabolism. The forage term spreads animals onto good ground (an ideal-free
/// distribution that damps overgrazing); the cohesion term draws them together into
/// *moving herds* — dense enough to graze efficiently and to be worth hunting, yet
/// migratory, so they don't overgraze a single tile. On barren, empty ground a lone
/// animal wanders to find food. Draws from the fauna RNG, so it stays seeded.
pub(crate) fn forage(
    mut fauna: Query<(&mut Position, &mut Energy, &SpeciesId), With<Herbivore>>,
    mut substrate: ResMut<Substrate>,
    bestiary: Res<Bestiary>,
    config: Res<FaunaRes>,
    mut rng: ResMut<FaunaRng>,
) {
    // Start-of-tick herd density per tile, so cohesion pulls toward where the herd is.
    let density: HashMap<usize, usize> = {
        let topo = substrate.0.topology();
        let mut d = HashMap::new();
        for (pos, _, _) in fauna.iter() {
            *d.entry(topo.index_of(pos.0)).or_default() += 1;
        }
        d
    };

    for (mut position, mut energy, species) in &mut fauna {
        let sp = &bestiary.species[species.0];
        let dest = {
            let world = &substrate.0;
            let topo = world.topology();
            let here = topo.index_of(position.0);
            // Forage appeal is biomass *as this species can use it* — weighted by how
            // well the tile's biome and climate suit it — plus the pull of company,
            // scaled by how gregarious the species is.
            let appeal = |i: usize, c: game_sim::Coord| {
                let suit = sp.suitability(world.biome(c).formation(), world.biotemperature(c));
                world.plant_biomass(c) * suit
                    + config.herd_cohesion
                        * sp.gregarious
                        * density.get(&i).copied().unwrap_or(0) as f32
            };
            let mut cands: Vec<(game_sim::Coord, f32)> =
                vec![(position.0, appeal(here, position.0))];
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
                // No forage it can use nearby — wander to look for better ground.
                cands[rng.0.gen_range(cands.len())].0
            }
        };
        position.0 = dest;
        // Bigger animals eat more and burn more.
        let grazed = substrate.0.graze(dest, config.intake * sp.size);
        energy.0 += config.eat_rate * grazed - config.metabolism * sp.size;
    }
}

/// Births and deaths: starve at zero energy, breed when well fed — but only where
/// the tile isn't already crowded past [`FaunaConfig::herd_cap`], so the herd can't
/// pile up density without limit on a single rich tile.
pub(crate) fn lifecycle(
    mut commands: Commands,
    mut fauna: Query<(Entity, &mut Energy, &Position, &SpeciesId), With<Herbivore>>,
    substrate: Res<Substrate>,
    bestiary: Res<Bestiary>,
    config: Res<FaunaRes>,
) {
    let world = &substrate.0;
    let topo = world.topology();
    let params = world.params();
    let mut density: HashMap<usize, usize> = HashMap::new();
    for (_, _, position, _) in &fauna {
        *density.entry(topo.index_of(position.0)).or_default() += 1;
    }
    for (entity, mut energy, position, species) in &mut fauna {
        let sp = &bestiary.species[species.0];
        if energy.0 <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        // Fecund species breed at a lower energy bar.
        if energy.0 >= config.repro_threshold / sp.fecundity {
            let c = position.0;
            let biome = world.biome(c);
            let suit = sp.suitability(biome.formation(), world.biotemperature(c));
            // Per-biome carrying capacity: the richer the biome, the denser a herd it
            // can carry before crowding stops further breeding.
            let cap = ((config.herd_cap as f32) * (0.3 + 1.6 * biome.profile(params).productivity))
                .round()
                .max(1.0) as usize;
            let crowded = density.get(&topo.index_of(c)).copied().unwrap_or(0) >= cap;
            // Breed only where the animal actually thrives.
            if suit >= 0.4 && !crowded {
                energy.0 -= config.repro_cost;
                commands.spawn((Herbivore, Position(c), Energy(config.repro_cost), *species));
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
    mut carnivores: Query<(&mut Position, &mut Energy, &SpeciesId), With<Carnivore>>,
    herbivores: Query<(Entity, &Position), (With<Herbivore>, Without<Carnivore>)>,
    substrate: Res<Substrate>,
    bestiary: Res<Bestiary>,
    config: Res<FaunaRes>,
    mut rng: ResMut<FaunaRng>,
) {
    let world = &substrate.0;
    let topo = world.topology();
    // Live prey by tile, in stable query order (so `drain` is deterministic).
    let mut prey: HashMap<usize, Vec<Entity>> = HashMap::new();
    for (e, p) in &herbivores {
        prey.entry(topo.index_of(p.0)).or_default().push(e);
    }
    let count_at = |prey: &HashMap<usize, Vec<Entity>>, i: usize| prey.get(&i).map_or(0, Vec::len);

    for (mut pos, mut energy, species) in &mut carnivores {
        let sp = &bestiary.species[species.0];
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
        // Bigger predators burn more, and hunting outside one's range costs extra.
        let c = topo.coord(best);
        let suit = sp.suitability(world.biome(c).formation(), world.biotemperature(c));
        let upkeep = config.carn_metabolism * sp.size * (2.0 - suit);
        energy.0 += config.carn_energy_per_kill * killed as f32 - upkeep;
    }
}

/// Predator births and deaths, mirroring [`lifecycle`] with the carnivore knobs.
pub(crate) fn carnivore_lifecycle(
    mut commands: Commands,
    mut carnivores: Query<(Entity, &mut Energy, &Position, &SpeciesId), With<Carnivore>>,
    bestiary: Res<Bestiary>,
    config: Res<FaunaRes>,
) {
    for (entity, mut energy, position, species) in &mut carnivores {
        let sp = &bestiary.species[species.0];
        if energy.0 <= 0.0 {
            commands.entity(entity).despawn();
        } else if energy.0 >= config.carn_repro_threshold / sp.fecundity {
            energy.0 -= config.carn_repro_cost;
            commands.spawn((
                Carnivore,
                Position(position.0),
                Energy(config.carn_repro_cost),
                *species,
            ));
        }
    }
}

/// Place `count` creatures of `diet`, each a species dropped into a biome it
/// thrives in: every species of that diet is given the pool of land tiles whose
/// formation and climate suit it (suitability ≥ 0.6), and animals are drawn from
/// those pools. If the world has warmed into no such habitat yet, a random species
/// is scattered onto any land so the trophic loop still seeds.
fn spawn_diet(
    world: &mut World,
    substrate: &GameWorld,
    bestiary: &Bestiary,
    rng: &mut SplitMix64,
    count: usize,
    energy: f32,
    diet: Diet,
) {
    if count == 0 {
        return;
    }
    let idxs = bestiary.of_diet(diet);
    if idxs.is_empty() {
        return;
    }
    let topo = substrate.topology();
    let sea = substrate.params().sea_level;
    let land: Vec<usize> = topo
        .indices()
        .filter(|&i| substrate.elevation(topo.coord(i)) >= sea)
        .collect();
    if land.is_empty() {
        return;
    }
    // Each species' suitable home tiles.
    let pools: Vec<(usize, Vec<usize>)> = idxs
        .iter()
        .map(|&si| {
            let sp = &bestiary.species[si];
            let pool: Vec<usize> = land
                .iter()
                .copied()
                .filter(|&i| {
                    let c = topo.coord(i);
                    sp.suitability(substrate.biome(c).formation(), substrate.biotemperature(c))
                        >= 0.6
                })
                .collect();
            (si, pool)
        })
        .filter(|(_, p)| !p.is_empty())
        .collect();

    for _ in 0..count {
        let (si, coord) = if pools.is_empty() {
            (
                idxs[rng.gen_range(idxs.len())],
                topo.coord(land[rng.gen_range(land.len())]),
            )
        } else {
            let (si, pool) = &pools[rng.gen_range(pools.len())];
            (*si, topo.coord(pool[rng.gen_range(pool.len())]))
        };
        match diet {
            Diet::Herbivore => {
                world.spawn((Herbivore, Position(coord), Energy(energy), SpeciesId(si)));
            }
            Diet::Carnivore => {
                world.spawn((Carnivore, Position(coord), Energy(energy), SpeciesId(si)));
            }
        }
    }
}

/// Place `count` herbivores, each in a biome its species favours.
pub fn spawn_fauna(
    world: &mut World,
    substrate: &GameWorld,
    bestiary: &Bestiary,
    rng: &mut SplitMix64,
    count: usize,
    energy: f32,
) {
    spawn_diet(
        world,
        substrate,
        bestiary,
        rng,
        count,
        energy,
        Diet::Herbivore,
    );
}

/// Place `count` carnivores, each in a biome its species favours (near its prey).
pub fn spawn_carnivores(
    world: &mut World,
    substrate: &GameWorld,
    bestiary: &Bestiary,
    rng: &mut SplitMix64,
    count: usize,
    energy: f32,
) {
    spawn_diet(
        world,
        substrate,
        bestiary,
        rng,
        count,
        energy,
        Diet::Carnivore,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_bestiary_loads_with_both_diets() {
        let b = Bestiary::bundled();
        assert!(
            b.species.len() >= 8,
            "expected a broad roster, got {}",
            b.species.len()
        );
        assert!(
            !b.of_diet(Diet::Herbivore).is_empty(),
            "no herbivores in the roster"
        );
        assert!(
            !b.of_diet(Diet::Carnivore).is_empty(),
            "no carnivores in the roster"
        );
        for s in &b.species {
            assert!(!s.habitat.is_empty(), "{} has no habitat", s.name);
            assert!(
                s.min_temp <= s.max_temp,
                "{} has an inverted temperature band",
                s.name
            );
            assert!(
                s.size > 0.0 && s.fecundity > 0.0,
                "{} has a non-positive body knob",
                s.name
            );
        }
    }

    #[test]
    fn suitability_peaks_in_habitat_and_falls_off_outside_it() {
        let b = Bestiary::bundled();
        let ash = b
            .species
            .iter()
            .find(|s| s.name == "ash elk")
            .expect("ash elk present");
        // Ideal in its cold tundra; hostile in hot desert (wrong formation and far too warm).
        assert!(
            ash.suitability(Formation::Tundra, 3.0) > 0.9,
            "tundra at 3°C should be ideal"
        );
        assert!(
            ash.suitability(Formation::Desert, 28.0) < 0.3,
            "hot desert should be hostile"
        );
    }
}
