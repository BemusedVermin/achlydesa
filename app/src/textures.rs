//! Procedural **tileable environment textures** — grass, plaza cobblestone, slate — generated in
//! code from noise (value/FBM for organic fields, Worley/cellular for stones). The proc-gen
//! counterpart to the character sprites and busts: no art assets, deterministic, a real PNG dropped
//! in still overrides. The albedo sits *under* the cel pass, so the palettes are mid-value and
//! low-contrast — the toon banding supplies the contrast (a busy texture would fight it).
//!
//! Everything tiles seamlessly: the lattice hash wraps to the period (`rem_euclid`), so value/FBM and
//! Worley all repeat over their period.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const SIZE: usize = 256;

/// Which surface to draw — each maps to a noise recipe + palette below.
#[derive(Clone, Copy)]
pub enum TextureKind {
    Grass,
    Plaza,
    Slate,
}

// Dedicated noise streams (xor a distinct constant per surface — the determinism convention).
const GRASS_SEED: u64 = 0x6_7A55_0001;
const PLAZA_SEED: u64 = 0x5_70E5_0002;
const SLATE_SEED: u64 = 0x5_1A7E_0003;

// ── Tileable noise ───────────────────────────────────────────────────────────────────────────────

/// A hashed value in `0..1` at integer lattice point `(ix, iy)`, **wrapped to `period`** so the
/// field tiles seamlessly.
fn lattice(ix: i32, iy: i32, period: i32, seed: u64) -> f32 {
    let p = period.max(1);
    let x = ix.rem_euclid(p) as u64;
    let y = iy.rem_euclid(p) as u64;
    let mut h =
        seed ^ x.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ y.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h & 0xFF_FFFF) as f32 / 0xFF_FFFF as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise sampled over the unit square, tiling every `period` cells.
fn value(u: f32, v: f32, period: i32, seed: u64) -> f32 {
    let (su, sv) = (u * period as f32, v * period as f32);
    let (x0, y0) = (su.floor() as i32, sv.floor() as i32);
    let (fx, fy) = (smooth(su - x0 as f32), smooth(sv - y0 as f32));
    let a = lattice(x0, y0, period, seed);
    let b = lattice(x0 + 1, y0, period, seed);
    let c = lattice(x0, y0 + 1, period, seed);
    let d = lattice(x0 + 1, y0 + 1, period, seed);
    let ab = a + (b - a) * fx;
    let cd = c + (d - c) * fx;
    ab + (cd - ab) * fy
}

/// Fractional Brownian motion over the unit square — `octaves` of value noise, each double the
/// frequency. Tiles because every octave's period is `base * 2^o`.
fn fbm(u: f32, v: f32, base: i32, octaves: u32, seed: u64) -> f32 {
    let (mut sum, mut amp, mut norm, mut freq) = (0.0, 0.5f32, 0.0, 1i32);
    for o in 0..octaves {
        sum += amp
            * value(
                u,
                v,
                base * freq,
                seed ^ (o as u64).wrapping_mul(0x6D2B_79F5),
            );
        norm += amp;
        amp *= 0.5;
        freq *= 2;
    }
    sum / norm
}

/// Worley/cellular over the unit square: the nearest and second-nearest feature-point distances (in
/// cell units) and the **nearest cell** (for per-cell colour). Tiles: a wrapped cell's feature point
/// is the tile-repeat of its in-range twin, so distances stay continuous across the seam.
fn worley(u: f32, v: f32, period: i32, seed: u64) -> (f32, f32, (i32, i32)) {
    let (su, sv) = (u * period as f32, v * period as f32);
    let (xi, yi) = (su.floor() as i32, sv.floor() as i32);
    let (mut f1, mut f2) = (9.0f32, 9.0f32);
    let mut cell = (xi, yi);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (xi + dx, yi + dy);
            let px = cx as f32 + lattice(cx, cy, period, seed);
            let py = cy as f32 + lattice(cx, cy, period, seed ^ 0x5BD1_E995);
            let d = (px - su).hypot(py - sv);
            if d < f1 {
                f2 = f1;
                f1 = d;
                cell = (cx, cy);
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2, cell)
}

// ── Colour ─────────────────────────────────────────────────────────────────────────────────────

fn rgb(r: f32, g: f32, b: f32) -> [u8; 4] {
    let c = |x: f32| (x.clamp(0.0, 1.0) * 255.0) as u8;
    [c(r), c(g), c(b), 255]
}
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [u8; 4] {
    rgb(
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    )
}

/// Mottled grass — low-frequency patches plus a touch of fine blade noise, around the same green the
/// flat fallback used.
fn grass(u: f32, v: f32) -> [u8; 4] {
    let patch = fbm(u, v, 6, 4, GRASS_SEED);
    let blade = value(u, v, 48, GRASS_SEED ^ 0xB1AD);
    let t = (patch * 0.8 + blade * 0.2).clamp(0.0, 1.0);
    lerp3([0.26, 0.34, 0.20], [0.40, 0.49, 0.29], t)
}

/// Cobblestone — Worley cells, each a slightly different grey, darkened toward the mortar lines
/// (where the two nearest feature points are close, `f2 - f1` is small).
fn plaza(u: f32, v: f32) -> [u8; 4] {
    const P: i32 = 20;
    let (f1, f2, cell) = worley(u, v, P, PLAZA_SEED);
    let _ = f1;
    let stone = 0.40 + lattice(cell.0, cell.1, P, PLAZA_SEED ^ 0xCE11) * 0.12;
    let edge = smooth(((f2 - f1) / 0.10).clamp(0.0, 1.0)); // 0 in the mortar, 1 in a stone's interior
    let g = stone * (0.55 + 0.45 * edge);
    rgb(g + 0.015, g, g + 0.03) // a faint cool tint
}

/// Slate — dark blue-grey with faint horizontal strata (the noise is stretched vertically).
fn slate(u: f32, v: f32) -> [u8; 4] {
    let body = fbm(u, v, 4, 3, SLATE_SEED);
    let strata = value(u * 0.5, v * 3.0, 16, SLATE_SEED ^ 0x57A7);
    let t = (body * 0.6 + strata * 0.4).clamp(0.0, 1.0);
    lerp3([0.24, 0.26, 0.32], [0.34, 0.36, 0.43], t)
}

/// Generate the albedo for a surface — a 256² tileable sRGB image. Sits under the cel pass.
pub fn procedural_surface(kind: TextureKind) -> Image {
    let mut buf = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = x as f32 / SIZE as f32;
            let v = y as f32 / SIZE as f32;
            let c = match kind {
                TextureKind::Grass => grass(u, v),
                TextureKind::Plaza => plaza(u, v),
                TextureKind::Slate => slate(u, v),
            };
            let i = (y * SIZE + x) * 4;
            buf[i..i + 4].copy_from_slice(&c);
        }
    }
    Image::new(
        Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        buf,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surfaces_are_drawn_and_deterministic() {
        for kind in [TextureKind::Grass, TextureKind::Plaza, TextureKind::Slate] {
            let a = procedural_surface(kind);
            assert_eq!(
                a.data,
                procedural_surface(kind).data,
                "same kind → same texture"
            );
            assert_eq!(a.data.as_ref().unwrap().len(), SIZE * SIZE * 4);
        }
    }

    #[test]
    fn value_noise_tiles() {
        // The field must match across the period seam (u=0 and u=1 sample the same lattice).
        let l = value(0.0, 0.37, 8, GRASS_SEED);
        let r = value(1.0, 0.37, 8, GRASS_SEED);
        assert!((l - r).abs() < 1e-5, "value noise should wrap: {l} vs {r}");
    }

    /// Preview: raw RGBA of each surface to `target/`. Run `cargo test -p app -- --ignored
    /// dump_texture_preview`, then `magick -size 256x256 -depth 8 rgba:target/tex_grass.rgba out.png`.
    #[test]
    #[ignore = "dev preview, writes files"]
    fn dump_texture_preview() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../target");
        for (name, kind) in [
            ("grass", TextureKind::Grass),
            ("plaza", TextureKind::Plaza),
            ("slate", TextureKind::Slate),
        ] {
            let img = procedural_surface(kind);
            std::fs::write(format!("{dir}/tex_{name}.rgba"), img.data.as_ref().unwrap()).unwrap();
        }
    }
}
