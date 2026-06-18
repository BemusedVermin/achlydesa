//! The ground itself: one merged, vertex-coloured mesh of hex columns (land, by compressed
//! relief) and flat water faces (ocean, by depth), built from just the tiles the player has
//! uncovered. Rebuilt only when the explored set grows.
//!
//! Walls are **neighbour-aware**: each of a tile's six sides drops only to its neighbour's
//! height — so equal tiles abut seamlessly, steps show a single clean cliff (no double-drawn,
//! z-fighting walls), and the map's outer edge drops to a solid bedrock base so nothing reads
//! as hollow.

use crate::layout::{hex_corners, land_height, tile_world};
use crate::mesh::MeshBuf;
use crate::palette::{self, SIDE_SHADE};
use crate::props::tile_seed;
use agents::{Simulation, Terrain};
use bevy::prelude::*;
use std::collections::HashMap;

/// Metres of water depth at which the deep-water colour is reached.
const DEPTH_SCALE: f32 = 1500.0;
/// How far the outer edge of the map drops — a slab of bedrock under the world.
const BEDROCK: f32 = -2.5;

/// Quantise a world centre to an integer key, so a tile's geometric neighbour can be looked up
/// by position (centres are exact multiples, so this never collides).
fn qkey(p: Vec2) -> (i32, i32) {
    ((p.x * 64.0).round() as i32, (p.y * 64.0).round() as i32)
}

pub fn build_ground_mesh(sim: &Simulation) -> Mesh {
    let gw = sim.substrate();
    let sea = gw.params().sea_level;
    let explored = sim.player_explored();

    // Pass 1 — every explored tile's top height, keyed by its world centre.
    let mut height: HashMap<(i32, i32), f32> = HashMap::with_capacity(explored.len());
    for &c in &explored {
        let elev = gw.elevation(c);
        let top = if Terrain::of(elev, sea) == Terrain::Ocean { 0.0 } else { land_height(elev - sea) };
        height.insert(qkey(tile_world(c.col, c.row)), top);
    }

    // Pass 2 — geometry.
    let mut b = MeshBuf::default();
    for &c in &explored {
        let elev = gw.elevation(c);
        let terrain = Terrain::of(elev, sea);
        let centre = tile_world(c.col, c.row);
        let (top, rgb) = if terrain == Terrain::Ocean {
            let depth01 = ((sea - elev) / DEPTH_SCALE).clamp(0.0, 1.0);
            (0.0, palette::vary(palette::water_rgb(depth01), tile_seed(c.col, c.row, 0x5EE2)))
        } else {
            let fert = gw.carrying_capacity(c).clamp(0.0, 1.0);
            let lit = palette::snow_blend(
                palette::ground_rgb(terrain, gw.biome(c).formation(), fert),
                elev - sea,
            );
            (land_height(elev - sea), palette::vary(lit, tile_seed(c.col, c.row, 0x5EED)))
        };
        add_top(&mut b, centre, top, rgb);
        add_walls(&mut b, centre, top, rgb, &height);
    }
    b.into_mesh()
}

/// The flat top face of a hex (a six-spoke fan), at height `top`.
fn add_top(b: &mut MeshBuf, centre: Vec2, top: f32, rgb: [f32; 3]) {
    let cs = hex_corners(centre);
    let mid = Vec3::new(centre.x, top, centre.y);
    for k in 0..6 {
        let a = cs[k];
        let n = cs[(k + 1) % 6];
        b.tri(mid, Vec3::new(n.x, top, n.y), Vec3::new(a.x, top, a.y), rgb);
    }
}

/// Side walls, one per edge, dropping only as far as the exposed face needs: to the neighbour's
/// height where there is one, to bedrock at the map's edge. Darkened for cheap relief shading.
fn add_walls(b: &mut MeshBuf, centre: Vec2, top: f32, rgb: [f32; 3], height: &HashMap<(i32, i32), f32>) {
    let side = palette::lerp([0.0, 0.0, 0.0], rgb, SIDE_SHADE);
    let cs = hex_corners(centre);
    for k in 0..6 {
        let a = cs[k];
        let nx = cs[(k + 1) % 6];
        // The neighbour across this edge sits two apothems out along the edge normal.
        let neighbour = centre + ((a + nx) * 0.5 - centre) * 2.0;
        let floor = height.get(&qkey(neighbour)).copied().unwrap_or(BEDROCK).min(top);
        if top - floor <= 1e-4 {
            continue; // this edge is buried against an equal-or-taller neighbour
        }
        // Wound so the face points outward (away from centre), or back-face culling hides it.
        b.quad(Vec3::new(a.x, top, a.y), Vec3::new(nx.x, top, nx.y), Vec3::new(nx.x, floor, nx.y), Vec3::new(a.x, floor, a.y), side);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lone column's top faces up and all six side walls face **outward** — the guard against
    /// the winding bug that made columns invisible (back-face culled).
    #[test]
    fn column_faces_point_out() {
        let mut b = MeshBuf::default();
        add_top(&mut b, Vec2::ZERO, 2.0, [0.5, 0.5, 0.5]);
        add_walls(&mut b, Vec2::ZERO, 2.0, [0.5, 0.5, 0.5], &HashMap::new()); // no neighbours → 6 walls
        let mut walls = 0;
        for (verts, n) in b.tris() {
            let c = (verts[0] + verts[1] + verts[2]) / 3.0;
            if n.y.abs() < 0.5 {
                // A side wall: its horizontal normal must point away from the centre (origin).
                assert!(Vec2::new(n.x, n.z).dot(Vec2::new(c.x, c.z)) > 0.0, "wall faces inward: n={n:?}");
                walls += 1;
            } else {
                assert!(n.y > 0.0, "top face must point up, got {n:?}");
            }
        }
        assert_eq!(walls, 12, "expected six side walls (12 triangles)");
    }
}
