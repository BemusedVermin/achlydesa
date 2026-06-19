//! A small software-rasterised **hex minimap** for the pause menu's Map tab. Explored tiles are
//! drawn as filled pointy-top hexagons coloured by terrain (sharing the 3D view's palette), with
//! a marker per discovered feature and a bright pip + ring on the avatar's tile. Rendered to an
//! `Image` on demand (only when the Map tab is open and the explored set has grown).

use agents::{Category, Coord, Simulation, Terrain};
use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::layout::{self, tile_world};
use crate::palette;

/// Metres of depth at which open water reaches its darkest (matches the mesh's feel).
const DEPTH_SCALE: f32 = 1500.0;
/// The fog the unexplored frame is washed with.
const FOG: [u8; 4] = [14, 16, 22, 255];

/// Render the explored world to an `w`×`h` RGBA image, centred and scaled to fit.
pub fn render(sim: &Simulation, avatar: Coord, w: u32, h: u32) -> Image {
    let mut buf = vec![0u8; (w * h * 4) as usize];
    for px in buf.chunks_exact_mut(4) {
        px.copy_from_slice(&FOG);
    }

    let explored = sim.player_explored();
    if explored.is_empty() {
        return image_from(buf, w, h);
    }
    let pts: Vec<(Coord, Vec2)> = explored.iter().map(|&c| (c, tile_world(c.col, c.row))).collect();

    // Fit the explored span into the frame (uniform scale, centred), with a one-hex margin.
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for (_, p) in &pts {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    lo -= Vec2::splat(1.2);
    hi += Vec2::splat(1.2);
    let span = (hi - lo).max(Vec2::splat(1.0));
    let margin = 8.0;
    let scale = ((w as f32 - 2.0 * margin) / span.x).min((h as f32 - 2.0 * margin) / span.y);
    let origin = Vec2::new(
        margin + (w as f32 - 2.0 * margin - span.x * scale) * 0.5,
        margin + (h as f32 - 2.0 * margin - span.y * scale) * 0.5,
    );
    let to_px = |p: Vec2| origin + (p - lo) * scale;
    let hex_r = scale * layout::HEX_R * 1.05; // a hair of overlap so the hexes tile seamlessly

    let gw = sim.substrate();
    let sea = gw.params().sea_level;

    // Tiles.
    for &(c, wp) in &pts {
        fill_hex(&mut buf, w, h, to_px(wp), hex_r, tile_rgb(sim, c, sea));
    }
    // A marker per discovered feature, coloured by category.
    let cat = sim.feature_catalog();
    for &(c, wp) in &pts {
        if let Some(category) = sim.features_at(c).iter().find(|f| f.discovered).map(|f| cat.def(f.kind).category) {
            disc(&mut buf, w, h, to_px(wp), (hex_r * 0.42).max(2.0), marker_rgb(category));
        }
    }
    // The avatar: a gold pip in a white ring.
    let ap = to_px(tile_world(avatar.col, avatar.row));
    disc(&mut buf, w, h, ap, (hex_r * 0.5).max(2.5), [255, 214, 92]);
    ring(&mut buf, w, h, ap, hex_r * 0.85, [250, 250, 250]);

    image_from(buf, w, h)
}

fn tile_rgb(sim: &Simulation, c: Coord, sea: f32) -> [u8; 3] {
    let gw = sim.substrate();
    let elev = gw.elevation(c);
    let terrain = Terrain::of(elev, sea);
    let rgb = if terrain == Terrain::Ocean {
        palette::water_rgb(((sea - elev) / DEPTH_SCALE).clamp(0.0, 1.0))
    } else {
        let fert = gw.carrying_capacity(c).clamp(0.0, 1.0);
        palette::snow_blend(palette::ground_rgb(terrain, gw.biome(c).formation(), fert), elev - sea)
    };
    [to8(rgb[0]), to8(rgb[1]), to8(rgb[2])]
}

/// The feature-marker colour per category (the Map "icons").
fn marker_rgb(cat: Category) -> [u8; 3] {
    match cat {
        Category::Community => [240, 228, 198], // parchment pip — a settlement
        Category::Court => [232, 196, 92],      // gold — a keep / temple / court
        Category::Ruin => [150, 120, 110],      // dun — broken stones
        Category::Wilderness => [120, 210, 220], // cold cyan — a wonder
    }
}

fn to8(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0) as u8
}

fn image_from(data: Vec<u8>, w: u32, h: u32) -> Image {
    Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

// ── Software rasterisation ──────────────────────────────────────────────────────────────────────

#[inline]
fn put(buf: &mut [u8], w: u32, h: u32, x: i32, y: i32, rgb: [u8; 3]) {
    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
        return;
    }
    let i = ((y as u32 * w + x as u32) * 4) as usize;
    buf[i] = rgb[0];
    buf[i + 1] = rgb[1];
    buf[i + 2] = rgb[2];
    buf[i + 3] = 255;
}

/// Fill a pointy-top hexagon centred at `c`, circumradius `r` (matching the world layout).
fn fill_hex(buf: &mut [u8], w: u32, h: u32, c: Vec2, r: f32, rgb: [u8; 3]) {
    let corners: [Vec2; 6] = std::array::from_fn(|k| {
        let a = std::f32::consts::FRAC_PI_6 + std::f32::consts::FRAC_PI_3 * k as f32;
        c + Vec2::new(a.cos(), a.sin()) * r
    });
    let (x0, x1) = ((c.x - r).floor() as i32, (c.x + r).ceil() as i32);
    let (y0, y1) = ((c.y - r).floor() as i32, (c.y + r).ceil() as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            if in_convex(&corners, p) {
                put(buf, w, h, x, y, rgb);
            }
        }
    }
}

/// Point-in-convex-polygon: the point is inside if it stays on one side of every edge.
fn in_convex(poly: &[Vec2; 6], p: Vec2) -> bool {
    let (mut pos, mut neg) = (false, false);
    for i in 0..6 {
        let a = poly[i];
        let b = poly[(i + 1) % 6];
        let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if cross > 0.0 {
            pos = true;
        } else if cross < 0.0 {
            neg = true;
        }
        if pos && neg {
            return false;
        }
    }
    true
}

fn disc(buf: &mut [u8], w: u32, h: u32, c: Vec2, r: f32, rgb: [u8; 3]) {
    let (x0, x1) = ((c.x - r).floor() as i32, (c.x + r).ceil() as i32);
    let (y0, y1) = ((c.y - r).floor() as i32, (c.y + r).ceil() as i32);
    let r2 = r * r;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = Vec2::new(x as f32 + 0.5 - c.x, y as f32 + 0.5 - c.y);
            if d.length_squared() <= r2 {
                put(buf, w, h, x, y, rgb);
            }
        }
    }
}

fn ring(buf: &mut [u8], w: u32, h: u32, c: Vec2, r: f32, rgb: [u8; 3]) {
    let (x0, x1) = ((c.x - r - 1.0).floor() as i32, (c.x + r + 1.0).ceil() as i32);
    let (y0, y1) = ((c.y - r - 1.0).floor() as i32, (c.y + r + 1.0).ceil() as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = Vec2::new(x as f32 + 0.5 - c.x, y as f32 + 0.5 - c.y).length();
            if (d - r).abs() <= 1.0 {
                put(buf, w, h, x, y, rgb);
            }
        }
    }
}
