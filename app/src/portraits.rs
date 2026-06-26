//! Procedural pixel-art **busts** for dialogue portraits — every soul gets a distinct head-and-
//! shoulders face, composed deterministically from its identity + archetype and software-rasterised
//! to an RGBA image. No art assets, no AI: the same procedural spirit as `props.rs` and the sprite
//! placeholder. A soul's face is stable for a run (seeded from its name + archetype, the same key
//! the sigil tint used), so you recognise who you're talking to.
//!
//! Rendered low-resolution (pixel art) with a transparent background, so the bust sits on the
//! conversation panel; the convo UI point-samples it, keeping the pixels crisp.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const W: usize = 64;
const H: usize = 72;

// ── A tiny SplitMix64, like props::Rng ──────────────────────────────────────────────────────────
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn int(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, p: f32) -> bool {
        ((self.next() >> 40) as f32 / (1u64 << 24) as f32) < p
    }
    /// Pick an entry from a slice.
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.int(xs.len())]
    }
}

/// A stable 64-bit seed from a soul's identity string (FNV-1a) — matches what the sigil hashed.
pub fn seed_of(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ── Raster helpers (RGBA8, opaque writes — pixel art, no AA) ─────────────────────────────────────
type Buf = Vec<u8>;
fn put(buf: &mut Buf, x: i32, y: i32, c: [u8; 4]) {
    if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
        return;
    }
    let i = (y as usize * W + x as usize) * 4;
    buf[i..i + 4].copy_from_slice(&c);
}
fn ellipse(buf: &mut Buf, cx: f32, cy: f32, rx: f32, ry: f32, c: [u8; 4]) {
    let (x0, x1) = ((cx - rx).floor() as i32, (cx + rx).ceil() as i32);
    let (y0, y1) = ((cy - ry).floor() as i32, (cy + ry).ceil() as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = (x as f32 + 0.5 - cx) / rx;
            let dy = (y as f32 + 0.5 - cy) / ry;
            if dx * dx + dy * dy <= 1.0 {
                put(buf, x, y, c);
            }
        }
    }
}
fn rect(buf: &mut Buf, x0: i32, y0: i32, x1: i32, y1: i32, c: [u8; 4]) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            put(buf, x, y, c);
        }
    }
}
fn shade(c: [u8; 4], f: f32) -> [u8; 4] {
    [
        (c[0] as f32 * f) as u8,
        (c[1] as f32 * f) as u8,
        (c[2] as f32 * f) as u8,
        c[3],
    ]
}

// ── Palettes (muted, to share the world's cool mood) ────────────────────────────────────────────
const SKIN: &[[u8; 4]] = &[
    [240, 214, 188, 255],
    [224, 184, 152, 255],
    [198, 150, 118, 255],
    [160, 112, 82, 255],
    [120, 84, 60, 255],
];
const HAIR: &[[u8; 4]] = &[
    [34, 30, 34, 255],    // black
    [60, 42, 30, 255],    // dark brown
    [104, 72, 44, 255],   // brown
    [168, 136, 84, 255],  // blond
    [176, 176, 182, 255], // grey
    [128, 56, 40, 255],   // auburn
    [232, 232, 235, 255], // white
];
const CLOTHES: &[[u8; 4]] = &[
    [86, 70, 56, 255],   // homespun brown
    [70, 88, 104, 255],  // slate blue
    [96, 64, 72, 255],   // muted maroon
    [78, 96, 78, 255],   // moss
    [110, 102, 84, 255], // tan
    [58, 62, 74, 255],   // charcoal
];
const EYE_DARK: [u8; 4] = [40, 40, 48, 255];
const EYE_WHITE: [u8; 4] = [222, 224, 226, 255];

/// Map an archetype hint to a clothing colour, so a class reads at a glance; `None` falls back to a
/// seed-chosen colour.
fn clothes_for(archetype: &str) -> Option<[u8; 4]> {
    let a = archetype.to_ascii_lowercase();
    let pick = |k: &str| a.contains(k);
    Some(
        if pick("noble") || pick("lord") || pick("king") || pick("court") {
            [96, 64, 72, 255] // maroon finery
        } else if pick("priest") || pick("monk") || pick("seer") || pick("cleric") {
            [58, 62, 74, 255] // dark robe
        } else if pick("smith") || pick("craft") || pick("labor") || pick("farm") || pick("hunt") {
            [86, 70, 56, 255] // homespun / leather
        } else if pick("soldier") || pick("guard") || pick("warrior") || pick("warlord") {
            [70, 78, 86, 255] // steely
        } else {
            return None;
        },
    )
}

/// Render a bust portrait for a soul. `seed` is a stable per-soul value (see [`seed_of`]); the
/// `archetype` hint biases the clothing colour. Transparent background, nearest-sampled.
pub fn procedural_bust(seed: u64, archetype: &str) -> Image {
    let mut rng = Rng::new(seed);
    let mut buf: Buf = vec![0; W * H * 4];

    let skin = *rng.pick(SKIN);
    let skin_dk = shade(skin, 0.82);
    let hair = *rng.pick(HAIR);
    let clothes = clothes_for(archetype).unwrap_or(*rng.pick(CLOTHES));
    let clothes_dk = shade(clothes, 0.8);

    let cx = W as f32 / 2.0;

    // Shoulders / chest (a broad arc rising from below the frame), with a neckline shadow.
    ellipse(&mut buf, cx, 92.0, 40.0, 32.0, clothes);
    ellipse(&mut buf, cx, 96.0, 20.0, 30.0, clothes_dk); // collar V
    // Neck.
    rect(&mut buf, 28, 44, 36, 56, skin_dk);

    let head_cy = 30.0;
    let (head_rx, head_ry) = (15.0, 18.0);
    // Long hair behind the head (sometimes), framing down to the shoulders.
    let long = rng.chance(0.45);
    if long {
        ellipse(
            &mut buf,
            cx,
            head_cy + 8.0,
            head_rx + 3.0,
            head_ry + 6.0,
            hair,
        );
    }
    // Head + ears.
    ellipse(&mut buf, cx - head_rx + 1.0, head_cy + 2.0, 3.0, 5.0, skin);
    ellipse(&mut buf, cx + head_rx - 1.0, head_cy + 2.0, 3.0, 5.0, skin);
    ellipse(&mut buf, cx, head_cy, head_rx, head_ry, skin);
    // A soft jaw shadow on one side for depth.
    ellipse(
        &mut buf,
        cx + 7.0,
        head_cy + 6.0,
        6.0,
        9.0,
        shade(skin, 0.92),
    );
    ellipse(&mut buf, cx, head_cy, head_rx - 1.0, head_ry - 1.0, skin);

    // Hair on top: a cap ellipse, then carve the face back with skin so a hairline shows.
    let bald = rng.chance(0.12);
    if !bald {
        ellipse(
            &mut buf,
            cx,
            head_cy - 6.0,
            head_rx + 1.0,
            head_ry - 3.0,
            hair,
        );
        // Fringe height varies; carve the face below it.
        let fringe = head_cy - 12.0 + rng.int(7) as f32;
        ellipse(
            &mut buf,
            cx,
            head_cy + 3.0,
            head_rx - 1.0,
            head_ry - 2.0,
            skin,
        );
        // Re-draw a thin hair band at the very top so the carve doesn't eat it all.
        ellipse(&mut buf, cx, fringe, head_rx, 3.0, hair);
    }

    // Eyes, brows, nose, mouth.
    let eye_y = (head_cy + 1.0) as i32;
    let eye_dx = 6;
    for s in [-1, 1] {
        let ex = cx as i32 + s * eye_dx;
        rect(&mut buf, ex - 2, eye_y, ex + 1, eye_y + 1, EYE_WHITE);
        rect(&mut buf, ex - 1, eye_y, ex, eye_y + 1, EYE_DARK); // iris
        rect(
            &mut buf,
            ex - 2,
            eye_y - 3,
            ex + 1,
            eye_y - 3,
            shade(hair, 0.9),
        ); // brow
    }
    // Nose.
    rect(
        &mut buf,
        cx as i32,
        eye_y + 2,
        cx as i32,
        eye_y + 5,
        skin_dk,
    );
    // Mouth.
    rect(
        &mut buf,
        cx as i32 - 3,
        eye_y + 8,
        cx as i32 + 2,
        eye_y + 8,
        shade(skin, 0.6),
    );

    // Beard (sometimes) — fill the lower face, then re-open the mouth.
    if rng.chance(0.4) {
        for y in (eye_y + 4)..=(eye_y + 14) {
            for x in (cx as i32 - 11)..=(cx as i32 + 11) {
                let dx = (x as f32 + 0.5 - cx) / 12.0;
                let dy = (y as f32 + 0.5 - (head_cy + 6.0)) / 13.0;
                if dx * dx + dy * dy <= 1.0 {
                    put(&mut buf, x, y, shade(hair, 0.92));
                }
            }
        }
        rect(
            &mut buf,
            cx as i32 - 3,
            eye_y + 8,
            cx as i32 + 2,
            eye_y + 9,
            shade(skin, 0.55),
        );
    }

    let mut img = Image::new(
        Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        buf,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::nearest(); // crisp pixels when the UI scales it up
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bust_is_drawn_and_deterministic() {
        let a = procedural_bust(seed_of("Maren/farmer"), "farmer");
        let b = procedural_bust(seed_of("Maren/farmer"), "farmer");
        assert_eq!(a.data, b.data, "same soul → same face");
        let drawn = a
            .data
            .as_ref()
            .unwrap()
            .chunks(4)
            .filter(|px| px[3] > 0)
            .count();
        assert!(
            drawn > 200,
            "the bust should actually draw something ({drawn} px)"
        );
        // Different souls differ.
        let c = procedural_bust(seed_of("Bram/soldier"), "soldier");
        assert_ne!(a.data, c.data, "different souls → different faces");
    }

    /// Preview: writes sample busts as raw RGBA to `target/` for visual inspection. Run with
    /// `cargo test -p app -- --ignored dump_bust_preview`, then e.g.
    /// `magick -size 64x72 -depth 8 rgba:target/bust_Maren_farmer.rgba out.png`.
    #[test]
    #[ignore = "dev preview, writes files"]
    fn dump_bust_preview() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../target");
        for name in [
            "Maren/farmer",
            "Bram/soldier",
            "Yalda/noble",
            "Coil/priest",
            "Zoe/smith",
            "Vesper/",
            "Ossa/",
            "Nebro/court",
        ] {
            let arch = name.split('/').nth(1).unwrap_or("");
            let img = procedural_bust(seed_of(name), arch);
            let path = format!("{dir}/bust_{}.rgba", name.replace('/', "_"));
            std::fs::write(&path, img.data.as_ref().unwrap()).unwrap();
        }
    }
}
