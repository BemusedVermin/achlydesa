//! A small software-rasterised **hex minimap**. Explored tiles are drawn as filled pointy-top
//! hexagons coloured by terrain (sharing the 3D view's palette), with a marker per discovered
//! feature and a bright pip + ring on the avatar's tile. Rendered to an `Image` over a **window**
//! of the world — a `center` (world units) at a given `world_per_px` zoom — so the HUD minimap can
//! track the player and the Map tab can be panned/zoomed, instead of cramming the whole explored
//! continent into one unreadable frame.

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

/// Render a `w`×`h` window of the explored world centred on `center` (world units) at `world_per_px`
/// zoom (smaller = more zoomed in). The avatar's pip is drawn when it falls inside the window.
pub fn render(
    sim: &Simulation,
    center: Vec2,
    world_per_px: f32,
    avatar: Coord,
    w: u32,
    h: u32,
) -> Image {
    let mut buf = vec![0u8; (w * h * 4) as usize];
    for px in buf.chunks_exact_mut(4) {
        px.copy_from_slice(&FOG);
    }

    let gw = sim.substrate();
    let sea = gw.params().sea_level;
    let wpp = world_per_px.max(1e-3);
    let half = Vec2::new(w as f32, h as f32) * 0.5;
    // World → texture pixel (the window maps `center` to the frame's middle).
    let to_px = |p: Vec2| half + (p - center) / wpp;
    let hex_r = (layout::HEX_R / wpp) * 1.05; // a hair of overlap so the hexes tile seamlessly
    let cull = hex_r + 2.0;
    let inside =
        |p: Vec2| p.x >= -cull && p.y >= -cull && p.x <= w as f32 + cull && p.y <= h as f32 + cull;

    let explored = sim.player_explored();
    // Tiles in the window.
    for &c in &explored {
        let p = to_px(tile_world(c.col, c.row));
        if inside(p) {
            fill_hex(&mut buf, w, h, p, hex_r, tile_rgb(sim, c, sea));
        }
    }
    // A marker per discovered feature, coloured by category.
    let cat = sim.feature_catalog();
    for &c in &explored {
        let p = to_px(tile_world(c.col, c.row));
        if inside(p)
            && let Some(category) = sim
                .features_at(c)
                .iter()
                .find(|f| f.discovered)
                .map(|f| cat.def(f.kind).category)
        {
            disc(
                &mut buf,
                w,
                h,
                p,
                (hex_r * 0.42).max(1.5),
                marker_rgb(category),
            );
        }
    }
    // Recent drama the avatar can sense — a crimson pip drawing the eye toward unrest (drawn even
    // over fog, since the lure is to go and find it). Brighter for fresher / nearer events.
    for (c, fid) in sim.drama_marks() {
        let p = to_px(tile_world(c.col, c.row));
        if inside(p) {
            let r = (hex_r * 0.5).max(2.0);
            let v = (110.0 + 140.0 * fid.clamp(0.0, 1.0)) as u8;
            disc(&mut buf, w, h, p, r, [v, 36, 44]);
            ring(&mut buf, w, h, p, (r * 1.6).max(3.0), [224, 72, 72]);
        }
    }
    // The avatar: a gold pip in a white ring, when it's within the window.
    let ap = to_px(tile_world(avatar.col, avatar.row));
    if ap.x >= 0.0 && ap.y >= 0.0 && ap.x < w as f32 && ap.y < h as f32 {
        disc(&mut buf, w, h, ap, (hex_r * 0.55).max(2.0), [255, 214, 92]);
        ring(&mut buf, w, h, ap, (hex_r * 0.95).max(3.0), [250, 250, 250]);
    }

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
        palette::snow_blend(
            palette::ground_rgb(terrain, gw.biome(c).formation(), fert),
            elev - sea,
        )
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
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
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
    let (x0, x1) = (
        (c.x - r - 1.0).floor() as i32,
        (c.x + r + 1.0).ceil() as i32,
    );
    let (y0, y1) = (
        (c.y - r - 1.0).floor() as i32,
        (c.y + r + 1.0).ceil() as i32,
    );
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = Vec2::new(x as f32 + 0.5 - c.x, y as f32 + 0.5 - c.y).length();
            if (d - r).abs() <= 1.0 {
                put(buf, w, h, x, y, rgb);
            }
        }
    }
}
