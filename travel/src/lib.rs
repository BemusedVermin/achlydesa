//! Pure, engine-agnostic **travel** over the hex world: how many days a tile costs to cross, which
//! edges a body can pass (steep climbs need gear, rivers need a boat), the weighted least-cost
//! route between two tiles, and the procedural road network. No ECS, no agents — just `game_sim`
//! terrain and the `pathfinding` crate. Deterministic: integer costs, index-ordered neighbours.
//!
//! The unit of cost is a **day**: one forest hex ≈ a day's walk (the 1.0 baseline). Roads are far
//! cheaper, wastes and rainforest slower, and a climb adds with the metres of ascent.

use game_sim::fields::Formation;
use game_sim::{Coord, Topology, World};
use pathfinding::prelude::dijkstra;
use std::collections::HashSet;

/// What a traveller can do, gating otherwise-impassable edges.
#[derive(Clone, Copy, Debug, Default)]
pub struct Caps {
    /// Can scale a steep edge (climbing gear + a proficient party share).
    pub climbing: bool,
    /// Can cross deep water (a boat).
    pub boat: bool,
}

impl Caps {
    /// Unrestricted — used when laying roads (engineers switchback anything).
    pub fn all() -> Self {
        Self {
            climbing: true,
            boat: true,
        }
    }
}

/// Cost-model tunables. Pure data; the caller owns/overrides it.
#[derive(Clone, Copy, Debug)]
pub struct CostModel {
    /// Multiplier on a road tile — roads are the fast lane.
    pub road_factor: f32,
    /// Extra days added per metre of *ascent* between two tiles.
    pub slope_per_m: f32,
    /// Ascent (m) above which an edge is a climb — impassable without `Caps::climbing`.
    pub climb_threshold: f32,
    /// Standing water above which a tile is deep — impassable without `Caps::boat`.
    pub water_threshold: f32,
    /// Extra days to wade a wet (river/marsh) tile, scaled by its water.
    pub water_cost: f32,
}

impl Default for CostModel {
    fn default() -> Self {
        Self {
            road_factor: 0.35,
            slope_per_m: 0.0012,
            climb_threshold: 700.0,
            water_threshold: 0.5,
            water_cost: 1.5,
        }
    }
}

/// Base days to cross a tile by its biome formation (forest = the day-walk baseline).
fn formation_cost(f: Formation) -> f32 {
    match f {
        Formation::Forest => 1.0,
        Formation::Grassland => 0.8,
        Formation::Shrubland => 1.1,
        Formation::Tundra => 1.3,
        Formation::Desert => 1.4,
        Formation::Rainforest => 1.7,
        Formation::Water => 4.0, // not normally entered (land routing), but priced steep
    }
}

/// Days to **enter** tile `c` (terrain + standing water + road bonus), before the slope term.
pub fn tile_cost(world: &World, model: &CostModel, c: Coord, on_road: bool) -> f32 {
    let mut cost = formation_cost(world.formation(c));
    let water = world.surface_water(c);
    if water > 0.05 {
        cost += model.water_cost * water.min(1.0);
    }
    if on_road {
        cost *= model.road_factor;
    }
    cost.max(0.05)
}

/// Whether the edge `a → b` can be crossed with `caps`: a steep ascent needs climbing gear; a
/// deep-water tile needs a boat.
pub fn edge_passable(world: &World, model: &CostModel, a: Coord, b: Coord, caps: Caps) -> bool {
    let ascent = (world.elevation(b) - world.elevation(a)).max(0.0);
    if ascent > model.climb_threshold && !caps.climbing {
        return false;
    }
    if world.surface_water(b) > model.water_threshold && !caps.boat {
        return false;
    }
    true
}

/// The land neighbours of tile index `t` (same land-filter as the planner's movement graph).
fn land_neighbors(world: &World, topo: &Topology, t: usize) -> Vec<usize> {
    let sea = world.params().sea_level;
    topo.neighbors(t)
        .iter()
        .map(|l| l.to)
        .filter(|&n| world.elevation(topo.coord(n)) >= sea)
        .collect()
}

/// The integer edge cost (×100 days) of stepping from tile `tc` into neighbour index `n`.
fn step_cost(
    world: &World,
    topo: &Topology,
    model: &CostModel,
    tc: Coord,
    n: usize,
    on_road: bool,
) -> u32 {
    let nc = topo.coord(n);
    let slope = (world.elevation(nc) - world.elevation(tc)).max(0.0) * model.slope_per_m;
    ((tile_cost(world, model, nc, on_road) + slope) * 100.0) as u32
}

/// Weighted least-cost **route** from `from` to `to` over land, honoring tile cost, the road
/// network (`is_road`), slope, and the edge gates for `caps`. Returns the hex steps (excluding
/// `from`); `None` if `to` is unreachable with these capabilities.
pub fn route(
    world: &World,
    model: &CostModel,
    is_road: &dyn Fn(usize) -> bool,
    from: Coord,
    to: Coord,
    caps: Caps,
) -> Option<Vec<Coord>> {
    let topo = world.topology();
    let goal = topo.index_of(to);
    let res = dijkstra(
        &topo.index_of(from),
        |&t| {
            let tc = topo.coord(t);
            land_neighbors(world, topo, t)
                .into_iter()
                .filter(|&n| edge_passable(world, model, tc, topo.coord(n), caps))
                .map(|n| (n, step_cost(world, topo, model, tc, n, is_road(n))))
                .collect::<Vec<_>>()
        },
        |&t| t == goal,
    );
    res.map(|(path, _)| path.into_iter().skip(1).map(|i| topo.coord(i)).collect())
}

/// The per-tile **entry cost** (days) for the whole map — the field the day-budget travel pacing
/// reads to know a road hex is cheap and a mountain hex dear. `1.0` ≈ a day's forest walk.
pub fn cost_field(world: &World, model: &CostModel, is_road: &dyn Fn(usize) -> bool) -> Vec<f32> {
    let topo = world.topology();
    (0..topo.len())
        .map(|t| tile_cost(world, model, topo.coord(t), is_road(t)))
        .collect()
}

/// Build a **road network**: a greedy least-cost tree connecting the `hubs` (settlement tiles).
/// Each hub routes to the nearest already-connected tile over raw terrain (preferring existing
/// road, so spurs merge), and that path becomes road. Returns the road tile indices.
pub fn build_roads(world: &World, model: &CostModel, hubs: &[Coord]) -> HashSet<usize> {
    let topo = world.topology();
    let mut roads: HashSet<usize> = HashSet::new();
    let Some(&first) = hubs.first() else {
        return roads;
    };
    roads.insert(topo.index_of(first));
    let caps = Caps::all();
    for &h in &hubs[1..] {
        let start = topo.index_of(h);
        if roads.contains(&start) {
            continue;
        }
        let res = dijkstra(
            &start,
            |&t| {
                let tc = topo.coord(t);
                land_neighbors(world, topo, t)
                    .into_iter()
                    .filter(|&n| edge_passable(world, model, tc, topo.coord(n), caps))
                    .map(|n| (n, step_cost(world, topo, model, tc, n, roads.contains(&n))))
                    .collect::<Vec<_>>()
            },
            |&t| roads.contains(&t),
        );
        if let Some((path, _)) = res {
            roads.extend(path);
        }
    }
    roads
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_world() -> World {
        World::generate(40, 30, config::tunables::params(), 7)
    }

    fn first_land(world: &World) -> Coord {
        let topo = world.topology();
        let sea = world.params().sea_level;
        (0..topo.len())
            .map(|i| topo.coord(i))
            .find(|&c| world.elevation(c) >= sea)
            .unwrap()
    }

    #[test]
    fn a_road_tile_is_cheaper_than_raw_terrain() {
        let world = small_world();
        let c = first_land(&world);
        let model = CostModel::default();
        assert!(
            tile_cost(&world, &model, c, true) < tile_cost(&world, &model, c, false),
            "roads are the fast lane"
        );
    }

    #[test]
    fn roads_connect_reachable_hubs() {
        let world = small_world();
        let topo = world.topology();
        let sea = world.params().sea_level;
        // Two *adjacent* land tiles are certainly on the same landmass, so they must connect.
        let a = first_land(&world);
        let b = topo
            .neighbors(topo.index_of(a))
            .iter()
            .map(|l| l.to)
            .find(|&n| world.elevation(topo.coord(n)) >= sea)
            .map(|n| topo.coord(n))
            .expect("a coastal/inland land tile has a land neighbour");
        let roads = build_roads(&world, &CostModel::default(), &[a, b]);
        assert!(
            roads.contains(&topo.index_of(a)) && roads.contains(&topo.index_of(b)),
            "both hubs are on the road network"
        );
    }

    #[test]
    fn routing_is_deterministic() {
        let world = small_world();
        let topo = world.topology();
        let sea = world.params().sea_level;
        let land: Vec<Coord> = (0..topo.len())
            .map(|i| topo.coord(i))
            .filter(|&c| world.elevation(c) >= sea)
            .collect();
        let (a, b) = (land[0], land[land.len() / 2]);
        let no_road = |_: usize| false;
        let r1 = route(&world, &CostModel::default(), &no_road, a, b, Caps::all());
        let r2 = route(&world, &CostModel::default(), &no_road, a, b, Caps::all());
        assert_eq!(r1, r2, "same inputs → same route");
    }
}
