//! The shared hex-map geometry: where a tile sits in world space, how tall its column
//! stands, and the corners of its top face. One source of truth for the mesh builder, the
//! prop scatter, the markers, and the camera — so they can never drift apart.

use agents::{Coord, Terrain};
use bevy::prelude::*;
use game_sim::World as GameWorld;

pub const SQRT3: f32 = 1.732_050_8;
/// Hex top-face radius (centre to corner). Exactly 1.0 so adjacent hex tops share their edges
/// and the surface reads as one cohesive land, not a scatter of separate chips. (It also makes
/// the geometric neighbour lookup in the ground builder exact.)
pub const HEX_R: f32 = 1.0;

/// Floor height of any land tile, so even sea-level shore stands a little proud of the water.
pub const MIN_LAND_H: f32 = 0.18;
/// Hard cap on a land column's height (world units). Mountains rise to about here and no
/// further, so every tile top stays flat and a player piece can always land on it — the
/// "flatten the peaks" the design calls for.
pub const MAX_LAND_H: f32 = 4.2;
/// Metres of real relief at which the height curve reaches ~63% of its cap. Low hills gain
/// height quickly; high mountains saturate toward [`MAX_LAND_H`].
const HEIGHT_KNEE_M: f32 = 1150.0;

/// World-space centre of a tile's top face (x, z), matching the sim's pointy-top odd-offset
/// layout exactly (via hexx), so the view lines up with the simulation's own coordinates.
pub fn tile_world(col: i32, row: i32) -> Vec2 {
    let h = hexx::Hex::from_offset_coordinates([col, row], hexx::OffsetHexMode::Odd, hexx::HexOrientation::Pointy);
    let (q, r) = (h.x as f32, h.y as f32);
    Vec2::new(SQRT3 * (q + r / 2.0), 1.5 * r)
}

/// A pointy-top hex's six top-face corners around `centre`, at radius [`HEX_R`].
pub fn hex_corners(centre: Vec2) -> [Vec2; 6] {
    std::array::from_fn(|k| {
        let a = std::f32::consts::FRAC_PI_6 + std::f32::consts::FRAC_PI_3 * k as f32;
        centre + Vec2::new(a.cos(), a.sin()) * HEX_R
    })
}

/// Compress + cap: turn real metres of relief above sea into a column height. A soft
/// saturating curve — quick to rise for foothills, flattening to [`MAX_LAND_H`] for peaks —
/// so the board keeps its 3-D relief without spiking into unlandable towers.
pub fn land_height(relief_m: f32) -> f32 {
    let span = MAX_LAND_H - MIN_LAND_H;
    MIN_LAND_H + span * (1.0 - (-relief_m.max(0.0) / HEIGHT_KNEE_M).exp())
}

/// The world-height of a tile's top: sea level (0) for ocean, the compressed relief for land.
/// The single height every system reads when it needs to stand something on a tile.
pub fn tile_top(gw: &GameWorld, c: Coord) -> f32 {
    let sea = gw.params().sea_level;
    let elev = gw.elevation(c);
    if Terrain::of(elev, sea) == Terrain::Ocean { 0.0 } else { land_height(elev - sea) }
}
