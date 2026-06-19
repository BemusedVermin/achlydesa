//! Dressing the land: for each tile the player uncovers, decide what grows or juts there from
//! its vegetation class and relief, and spawn instanced prop entities. Runs once per tile (the
//! explored set only ever grows), seeded by the tile's coord so a forest is the same forest
//! every time. Pure view: it reads the substrate and never writes it.

use crate::layout::{HEX_R, tile_top, tile_world};
use crate::props::{Prop, PropLibrary, Rng, tile_seed};
use agents::{Coord, Simulation, Terrain};
use bevy::prelude::*;
use game_sim::World as GameWorld;
use game_sim::fields::Formation;
use std::f32::consts::TAU;

/// Tag for every scattered prop entity (trees, rocks, and — later — buildings).
#[derive(Component)]
pub struct Decor;

/// Dress each tile revealed this frame (the `fresh` delta from [`crate::ground::Explored`]). The
/// delta is already de-duplicated, so every tile is decorated exactly once with no per-frame rescan.
pub fn decorate_fresh(commands: &mut Commands, lib: &PropLibrary, sim: &Simulation, fresh: &[Coord]) {
    let gw = sim.substrate();
    for &c in fresh {
        decorate_tile(commands, lib, gw, c);
    }
}

/// Spawn one prop instance: a library variant of `prop`, dropped at a seeded spot inside the
/// hex with a random facing and size.
fn place(commands: &mut Commands, lib: &PropLibrary, prop: Prop, centre: Vec2, top: f32, rng: &mut Rng, scale: (f32, f32), spread: f32) {
    let Some(mesh) = lib.pick(prop, rng) else { return };
    let a = rng.range(0.0, TAU);
    let r = HEX_R * spread * rng.unit().sqrt();
    let (ox, oz) = (r * a.cos(), r * a.sin());
    commands.spawn((
        Decor,
        Mesh3d(mesh),
        MeshMaterial3d(lib.material.clone()),
        Transform {
            translation: Vec3::new(centre.x + ox, top, centre.y + oz),
            rotation: Quat::from_rotation_y(rng.range(0.0, TAU)),
            scale: Vec3::splat(rng.range(scale.0, scale.1)),
        },
    ));
}

fn decorate_tile(commands: &mut Commands, lib: &PropLibrary, gw: &GameWorld, c: Coord) {
    let sea = gw.params().sea_level;
    let elev = gw.elevation(c);
    let terrain = Terrain::of(elev, sea);
    if terrain == Terrain::Ocean {
        return;
    }
    let centre = tile_world(c.col, c.row);
    let top = tile_top(gw, c);
    let formation = gw.biome(c).formation();
    let fert = gw.carrying_capacity(c).clamp(0.0, 1.0);
    let mut rng = Rng::new(tile_seed(c.col, c.row, 0xDEC0_FFEE));

    // Vegetation by formation. Counts are modest so dense regions stay readable, not a thicket.
    match formation {
        Formation::Rainforest => {
            // The lushest canopy: more crowns, broadleaf-dominant, no dead wood.
            let n = 3 + rng.int(4); // 3–6
            for _ in 0..n {
                let prop = if rng.chance(0.8) { Prop::Broadleaf } else { Prop::Conifer };
                place(commands, lib, prop, centre, top, &mut rng, (0.95, 1.4), 0.74);
            }
        }
        Formation::Forest => {
            let n = 2 + rng.int(4); // 2–5
            for _ in 0..n {
                let prop = if fert < 0.09 {
                    Prop::DeadTree // an ashen / sickly wood where nothing thrives
                } else if rng.chance(0.5) {
                    Prop::Conifer
                } else {
                    Prop::Broadleaf
                };
                place(commands, lib, prop, centre, top, &mut rng, (0.8, 1.25), 0.72);
            }
        }
        Formation::Shrubland => {
            let n = 1 + rng.int(3); // 1–3
            for _ in 0..n {
                let prop = if rng.chance(0.2) { Prop::Broadleaf } else { Prop::Shrub };
                place(commands, lib, prop, centre, top, &mut rng, (0.8, 1.2), 0.74);
            }
        }
        Formation::Grassland => {
            let n = 1 + rng.int(3);
            for _ in 0..n {
                let prop = if rng.chance(0.18) { Prop::Shrub } else { Prop::GrassTuft };
                place(commands, lib, prop, centre, top, &mut rng, (0.8, 1.4), 0.78);
            }
        }
        Formation::Tundra => {
            if rng.chance(0.6) {
                let n = 1 + rng.int(2);
                for _ in 0..n {
                    let prop = if rng.chance(0.3) { Prop::DeadTree } else { Prop::Shrub };
                    place(commands, lib, prop, centre, top, &mut rng, (0.6, 0.95), 0.76);
                }
            }
        }
        Formation::Desert | Formation::Water => {}
    }

    // Rock, thick on the peaks and scattered on the heights.
    let rocks = match terrain {
        Terrain::Mountain => 2 + rng.int(3),
        Terrain::Highland if rng.chance(0.45) => 1 + rng.int(2),
        _ if rng.chance(0.07) => 1,
        _ => 0,
    };
    for _ in 0..rocks {
        place(commands, lib, Prop::Boulder, centre, top, &mut rng, (0.7, 1.4), 0.7);
    }
}
