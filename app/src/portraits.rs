//! Procedural pixel-art **busts** for dialogue portraits — every soul gets a distinct head-and-
//! shoulders face, composed deterministically from its identity + archetype and software-rasterised
//! to an RGBA image. No art assets, no AI: the same procedural spirit as `props.rs`. A soul's face is
//! seeded from its **entity id**, so it is stable for the run and recognisably theirs.
//!
//! Rendered low-resolution (pixel art) with a transparent background, so the bust sits on the
//! conversation panel; the convo UI point-samples it, keeping the pixels crisp.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::props::Rng;

const W: usize = 64;
const H: usize = 72;

/// XORed into the seed so the bust draws from its own stream, distinct from the body sprite's.
const FACE_STREAM: u64 = 0x0FACE_B057_5EED;

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
/// Pick a palette entry from the shared `props::Rng`.
fn pick(rng: &mut Rng, xs: &[[u8; 4]]) -> [u8; 4] {
    xs[rng.int(xs.len() as u32) as usize]
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

/// The collar colour. When the soul has an archetype, its **typed edge id** indexes the palette (so a
/// class reads consistently); otherwise it's seed-chosen. No string matching — the id is the identity,
/// a display name is for humans. The seeded colour is drawn unconditionally so the archetype only
/// swaps the result, never the RNG position (else features would drift by archetype).
fn clothes_for(archetype: Option<usize>, rng: &mut Rng) -> [u8; 4] {
    let seeded = pick(rng, CLOTHES);
    match archetype {
        Some(id) => CLOTHES[id.wrapping_mul(0x9E37_79B1) % CLOTHES.len()],
        None => seeded,
    }
}

/// Render a bust portrait for a soul. `seed` is a stable per-soul value (the entity bits); the typed
/// `archetype` edge id (if any) keys the collar colour. Transparent background, nearest-sampled.
pub fn procedural_bust(seed: u64, archetype: Option<usize>) -> Image {
    let mut rng = Rng::new(seed ^ FACE_STREAM);
    let mut buf: Buf = vec![0; W * H * 4];

    let skin = pick(&mut rng, SKIN);
    let skin_dk = shade(skin, 0.82);
    let hair = pick(&mut rng, HAIR);
    let clothes = clothes_for(archetype, &mut rng);
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
    if rng.chance(0.45) {
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
    if !rng.chance(0.12) {
        ellipse(
            &mut buf,
            cx,
            head_cy - 6.0,
            head_rx + 1.0,
            head_ry - 3.0,
            hair,
        );
        let fringe = head_cy - 12.0 + rng.int(7) as f32; // fringe height varies
        ellipse(
            &mut buf,
            cx,
            head_cy + 3.0,
            head_rx - 1.0,
            head_ry - 2.0,
            skin,
        );
        ellipse(&mut buf, cx, fringe, head_rx, 3.0, hair); // thin top band
    }

    // Eyes, brows, nose, mouth.
    let eye_y = (head_cy + 1.0) as i32;
    for s in [-1, 1] {
        let ex = cx as i32 + s * 6;
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
    rect(
        &mut buf,
        cx as i32,
        eye_y + 2,
        cx as i32,
        eye_y + 5,
        skin_dk,
    ); // nose
    rect(
        &mut buf,
        cx as i32 - 3,
        eye_y + 8,
        cx as i32 + 2,
        eye_y + 8,
        shade(skin, 0.6),
    ); // mouth

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
        let a = procedural_bust(101, Some(2));
        assert_eq!(
            a.data,
            procedural_bust(101, Some(2)).data,
            "same soul → same face"
        );
        let drawn = a
            .data
            .as_ref()
            .unwrap()
            .chunks(4)
            .filter(|px| px[3] > 0)
            .count();
        assert!(drawn > 200, "the bust should draw something ({drawn} px)");
        assert_ne!(
            a.data,
            procedural_bust(202, Some(5)).data,
            "different souls → different faces"
        );
    }

    #[test]
    fn same_archetype_same_collar() {
        // The typed id keys the collar, regardless of the seed; distinct ids generally differ.
        let mut r1 = Rng::new(1);
        let mut r2 = Rng::new(2);
        assert_eq!(clothes_for(Some(3), &mut r1), clothes_for(Some(3), &mut r2));
        let mut r = Rng::new(0);
        assert_ne!(clothes_for(Some(0), &mut r), clothes_for(Some(1), &mut r));
    }

    /// Preview: writes sample busts as raw RGBA to `target/`. Run `cargo test -p app -- --ignored
    /// dump_bust_preview`, then `magick -size 64x72 -depth 8 rgba:target/bust_N.rgba out.png`.
    #[test]
    #[ignore = "dev preview, writes files"]
    fn dump_bust_preview() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../target");
        for (i, (seed, arch)) in [
            (11u64, Some(0)),
            (22, Some(1)),
            (33, Some(2)),
            (44, Some(3)),
            (55, Some(4)),
            (66, None),
            (77, None),
            (88, Some(5)),
        ]
        .into_iter()
        .enumerate()
        {
            let img = procedural_bust(seed, arch);
            std::fs::write(format!("{dir}/bust_{i}.rgba"), img.data.as_ref().unwrap()).unwrap();
        }
    }
}
