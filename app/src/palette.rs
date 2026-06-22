//! The colour language of the map — terrain by relief and vegetation class, greened by
//! fertility, then washed with a faint cool tint and haze so the living world still reads
//! as a dream half-drowned in fog. One white material; the mesh carries the colour.

use agents::Terrain;
use game_sim::fields::Formation;

/// The horizon/haze colour the world fades into — a cold, pale fog.
pub const FOG_RGB: [f32; 3] = [0.36, 0.40, 0.46];
/// The void above the fog.
pub const SKY_RGB: [f32; 3] = [0.05, 0.07, 0.10];

/// A faint **otherworldly** cast laid over every tile colour — a dream-purgatory violet-slate
/// that pulls the whole palette a step off Earth without draining the jewel greens or the
/// spice of the wastes. Kept light so the biome hues stay distinct from one another.
const COOL: [f32; 3] = [0.38, 0.35, 0.48];
const COOL_AMOUNT: f32 = 0.07;

pub fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Lay the shared cool cast over a colour — the world's single unifying tint.
pub fn tinted(rgb: [f32; 3]) -> [f32; 3] {
    lerp(rgb, COOL, COOL_AMOUNT)
}

/// The colour a structural formation settles the ground into — pushed off-Earth into a
/// fantastical register: spice-amber wastes (dry is *Dune*, not dull), jewel-green islands,
/// alien-pale frost. Saturated and well-separated so the classes never blur into one another.
pub fn formation_color(f: Formation) -> [f32; 3] {
    match f {
        Formation::Water => [0.07, 0.18, 0.31],     // deep indigo sea
        Formation::Desert => [0.66, 0.42, 0.22],    // Dune: rich spice-amber ochre
        Formation::Tundra => [0.55, 0.57, 0.63],    // alien pale frost, faintly violet
        Formation::Grassland => [0.43, 0.52, 0.21], // dry gold-green under a strange sun
        Formation::Shrubland => [0.50, 0.45, 0.23], // dusty olive-amber scrub
        Formation::Forest => [0.13, 0.40, 0.21],    // vivid exotic canopy
        Formation::Rainforest => [0.05, 0.34, 0.20], // deep jewel-emerald, the lush island
    }
}

const GRASS: [f32; 3] = [0.30, 0.56, 0.20];

/// The final land-tile colour: the vegetation class, swung toward cool slate rock on mountains,
/// warm brown heath on the heights, and bright sand on the shore, greened a touch by fertility,
/// then lightly cool-tinted. The terrain bands carry deliberately distinct hues.
pub fn ground_rgb(t: Terrain, f: Formation, fertility: f32) -> [f32; 3] {
    let mut base = formation_color(f);
    match t {
        Terrain::Mountain => base = [0.42, 0.40, 0.50], // alien violet-grey rock
        Terrain::Highland => base = lerp(base, [0.50, 0.38, 0.27], 0.55), // amber heath/moor
        Terrain::Coast => base = lerp(base, [0.74, 0.55, 0.30], 0.6), // Dune spice-sand
        _ => {}
    }
    if !matches!(
        f,
        Formation::Forest | Formation::Rainforest | Formation::Water
    ) {
        base = lerp(base, GRASS, fertility.clamp(0.0, 1.0) * 0.4);
    }
    tinted(base)
}

/// Open-water colour, darkening with depth (`depth01`: 0 at the shore, 1 in the deeps).
pub fn water_rgb(depth01: f32) -> [f32; 3] {
    let shallow = [0.16, 0.34, 0.42];
    let deep = [0.05, 0.13, 0.24];
    tinted(lerp(shallow, deep, depth01.clamp(0.0, 1.0)))
}

/// Snow white for the high caps.
pub const SNOW: [f32; 3] = [0.90, 0.92, 0.97];

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Whiten the high peaks — snow caps for strong contrast against the rock below. `relief_m` is
/// metres above sea.
pub fn snow_blend(rgb: [f32; 3], relief_m: f32) -> [f32; 3] {
    lerp(rgb, SNOW, smoothstep(1700.0, 3300.0, relief_m) * 0.9)
}

/// A small deterministic per-tile brightness jitter (`0.88..1.12`), to break up flat
/// same-colour expanses so a sweep of rock or grass has life in it. View-only.
pub fn vary(rgb: [f32; 3], seed: u64) -> [f32; 3] {
    let h = (seed ^ (seed >> 29)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let f = 0.88 + (h >> 40) as f32 / (1u64 << 24) as f32 * 0.24;
    [
        (rgb[0] * f).min(1.0),
        (rgb[1] * f).min(1.0),
        (rgb[2] * f).min(1.0),
    ]
}

/// How much to darken a column's side walls relative to its top (cheap directional shading).
pub const SIDE_SHADE: f32 = 0.62;
