//! The colour language of the map — terrain by relief, greened by fertility, washed
//! where the player has only glimpsed it. Mirrors the reference game's vertex-colour
//! approach (one white material; the mesh carries the colour).

use agents::Terrain;
use bevy::prelude::Color;

/// Base colour for a relief band.
pub fn terrain_color(t: Terrain) -> Color {
    match t {
        Terrain::Ocean => Color::srgb(0.086, 0.192, 0.306), // deep navy
        Terrain::Coast => Color::srgb(0.44, 0.41, 0.27), // sand/tan shore
        Terrain::Lowland => Color::srgb(0.26, 0.48, 0.22), // grass
        Terrain::Highland => Color::srgb(0.40, 0.35, 0.26), // heath/moor brown
        Terrain::Mountain => Color::srgb(0.40, 0.39, 0.38), // bare rock
    }
}

const GRASS: [f32; 3] = [0.20, 0.50, 0.18];

pub fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// The final tile colour: terrain band greened by how fertile the ground is (land only).
pub fn tile_rgb(t: Terrain, fertility: f32) -> [f32; 3] {
    let c = terrain_color(t).to_srgba();
    let base = [c.red, c.green, c.blue];
    if matches!(t, Terrain::Ocean) {
        base
    } else {
        lerp(base, GRASS, fertility.clamp(0.0, 1.0) * 0.5)
    }
}
