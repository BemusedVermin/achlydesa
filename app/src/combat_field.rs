//! Software-rasterised **2D combat field** (the BG3-style arena). Combatants are tokens at
//! continuous positions; a move targets a person and travels the 1D line to them — drawn here as
//! the attack line, cyan when the target is within reach, red when it would whiff. A reach ring
//! shows how far the pending move can land. Rendered to an `Image` (the same approach as the
//! minimap) and shown in the combat scene's field area; re-rendered each frame so movement and hit
//! flashes animate.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// One combatant on the field.
pub struct Token {
    pub pos: Vec2,
    pub color: [u8; 3],
    pub hp_frac: f32,
    /// Marked as the player's current target.
    pub target: bool,
    /// The actor whose decision is pending.
    pub active: bool,
    /// Hit-flash intensity, 0..1.
    pub flash: f32,
    pub down: bool,
}

/// Everything to draw this frame.
pub struct FieldView<'a> {
    pub tokens: &'a [Token],
    /// `(centre, reach)` ring around the active actor, in world units.
    pub reach_ring: Option<(Vec2, f32)>,
    /// `(from, to, in_reach)` — the 1D line from the active actor to its target.
    pub attack_line: Option<(Vec2, Vec2, bool)>,
}

const BG: [u8; 3] = [12, 14, 20];
const FLOOR: [u8; 3] = [22, 25, 34];
const HIT: [u8; 3] = [255, 232, 150];
const IN_REACH: [u8; 3] = [110, 214, 236];
const WHIFF: [u8; 3] = [210, 90, 84];

/// Render the field to a `w`×`h` image.
pub fn render(view: &FieldView, w: u32, h: u32) -> Image {
    let mut buf = vec![0u8; (w * h * 4) as usize];
    for px in buf.chunks_exact_mut(4) {
        px.copy_from_slice(&[BG[0], BG[1], BG[2], 255]);
    }

    // World bounds = the spread of everything to draw, padded, mapped with uniform scale.
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    let mut grow = |p: Vec2| {
        lo = lo.min(p);
        hi = hi.max(p);
    };
    for t in view.tokens {
        grow(t.pos);
    }
    if let Some((c, r)) = view.reach_ring {
        grow(c - Vec2::splat(r));
        grow(c + Vec2::splat(r));
    }
    if lo.x > hi.x {
        lo = Vec2::splat(-1.0);
        hi = Vec2::splat(1.0);
    }
    let pad = 3.0;
    lo -= Vec2::splat(pad);
    hi += Vec2::splat(pad);
    let span = (hi - lo).max(Vec2::splat(1.0));
    let scale = ((w as f32) / span.x).min((h as f32) / span.y) * 0.92;
    let off = Vec2::new(w as f32, h as f32) * 0.5 - (lo + hi) * 0.5 * scale;
    let to_px = |p: Vec2| p * scale + off;

    // A faint arena floor band, so the field reads as a place rather than a void.
    let floor_y0 = to_px(Vec2::new(0.0, lo.y + pad)).y as i32;
    let floor_y1 = to_px(Vec2::new(0.0, hi.y - pad)).y as i32;
    for y in floor_y0.max(0)..floor_y1.min(h as i32) {
        for x in 0..w as i32 {
            put(&mut buf, w, h, x, y, FLOOR);
        }
    }

    // Reach ring (under the tokens).
    if let Some((c, r)) = view.reach_ring {
        ring(&mut buf, w, h, to_px(c), r * scale, [70, 86, 110], 1.0);
    }
    // The attack line.
    if let Some((a, b, ok)) = view.attack_line {
        line(
            &mut buf,
            w,
            h,
            to_px(a),
            to_px(b),
            if ok { IN_REACH } else { WHIFF },
        );
    }

    // Tokens: a coloured disc, an HP arc, and a ring for target / active.
    for t in view.tokens {
        let p = to_px(t.pos);
        let r = (scale * 0.55).clamp(6.0, 22.0);
        let base = if t.down {
            [t.color[0] / 3, t.color[1] / 3, t.color[2] / 3]
        } else {
            t.color
        };
        let col = if t.flash > 0.0 {
            mix(base, HIT, t.flash.clamp(0.0, 1.0))
        } else {
            base
        };
        disc(&mut buf, w, h, p, r, col);
        if !t.down {
            // HP ring: a brighter wedge proportional to remaining HP.
            hp_ring(&mut buf, w, h, p, r + 3.0, t.hp_frac);
        }
        if t.active {
            ring(&mut buf, w, h, p, r + 5.0, [240, 214, 120], 1.6);
        }
        if t.target {
            ring(&mut buf, w, h, p, r + 7.5, [235, 235, 245], 1.4);
        }
    }

    image_from(buf, w, h)
}

// ── Raster helpers ──────────────────────────────────────────────────────────────────────────

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    [l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2])]
}

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

fn ring(buf: &mut [u8], w: u32, h: u32, c: Vec2, r: f32, rgb: [u8; 3], thick: f32) {
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
            if (d - r).abs() <= thick {
                put(buf, w, h, x, y, rgb);
            }
        }
    }
}

/// A partial ring (clockwise from the top) covering `frac` of the circle — the HP gauge.
fn hp_ring(buf: &mut [u8], w: u32, h: u32, c: Vec2, r: f32, frac: f32) {
    let frac = frac.clamp(0.0, 1.0);
    let col = [
        (210.0 - 150.0 * frac) as u8,
        (70.0 + 150.0 * frac) as u8,
        90,
    ];
    let steps = 64;
    let lit = (steps as f32 * frac).round() as i32;
    for k in 0..lit {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * (k as f32 / steps as f32);
        let p = c + Vec2::new(a.cos(), a.sin()) * r;
        disc(buf, w, h, p, 1.4, col);
    }
}

fn line(buf: &mut [u8], w: u32, h: u32, a: Vec2, b: Vec2, rgb: [u8; 3]) {
    let n = (a.distance(b).ceil() as i32).max(1);
    for k in 0..=n {
        let p = a.lerp(b, k as f32 / n as f32);
        // a 2px-ish line
        put(buf, w, h, p.x as i32, p.y as i32, rgb);
        put(buf, w, h, p.x as i32 + 1, p.y as i32, rgb);
    }
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
