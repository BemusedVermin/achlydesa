//! Shared **billboard-sprite** scaffolding for the HD-2D shift (Star Ocean: The Second Story R
//! style — 2-D character sprites composited into the lit 3-D dioramas).
//!
//! Characters are drawn as **procedural pixel figures** composed in code ([`procedural_body_sprite`])
//! — deterministic per soul, so every resident is a distinct person with no art assets and no AI
//! pipeline. The material is **alpha-masked**, so the depth prepass writes only the figure and the
//! existing cel **ink outline traces the character** — the cel+sprite combo the hybrid look wants. A
//! real authored sprite can still drop in over the procedural one by supplying its texture.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// A flat quad that always turns to face the rendering camera, yaw-only (it stays upright on the
/// ground rather than tipping flat as the camera pitches) — the SO2R billboard behaviour.
#[derive(Component)]
pub struct Billboard;

/// Re-orient every [`Billboard`] to face `cam_pos` horizontally. The quad's front is `+Z`, so the
/// yaw that points `+Z` at the camera in the XZ plane is `atan2(dx, dz)`.
pub fn face_camera<'a>(cam_pos: Vec3, sprites: impl Iterator<Item = Mut<'a, Transform>>) {
    for mut tf in sprites {
        let d = cam_pos - tf.translation;
        tf.rotation = Quat::from_rotation_y(d.x.atan2(d.z));
    }
}

// ── Procedural character sprites ─────────────────────────────────────────────────────────────────
// A full-body pixel figure composed in code — the compositional/modular approach the research favours
// for *readable humanoids* (over GANs / cellular automata). Deterministic per soul, so every resident
// is a distinct person, with no art assets and no AI pipeline (the same spirit as the bust portraits).

const BW: i32 = 40;
const BH: i32 = 72;
const EYE: [u8; 4] = [40, 40, 48, 255];

/// A stable 64-bit seed from a soul's identity string (FNV-1a).
pub fn seed_of(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn bput(buf: &mut [u8], x: i32, y: i32, c: [u8; 4]) {
    if x < 0 || y < 0 || x >= BW || y >= BH {
        return;
    }
    let i = ((y * BW + x) * 4) as usize;
    buf[i..i + 4].copy_from_slice(&c);
}
fn brect(buf: &mut [u8], x0: i32, y0: i32, x1: i32, y1: i32, c: [u8; 4]) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            bput(buf, x, y, c);
        }
    }
}
fn bellipse(buf: &mut [u8], cx: f32, cy: f32, rx: f32, ry: f32, c: [u8; 4]) {
    let (x0, x1) = ((cx - rx).floor() as i32, (cx + rx).ceil() as i32);
    let (y0, y1) = ((cy - ry).floor() as i32, (cy + ry).ceil() as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = (x as f32 + 0.5 - cx) / rx;
            let dy = (y as f32 + 0.5 - cy) / ry;
            if dx * dx + dy * dy <= 1.0 {
                bput(buf, x, y, c);
            }
        }
    }
}
/// A vertical trapezoid (the torso): half-width lerps from `hw0` at `y0` to `hw1` at `y1`.
fn btrap(buf: &mut [u8], cx: i32, y0: i32, y1: i32, hw0: f32, hw1: f32, c: [u8; 4]) {
    for y in y0..=y1 {
        let t = if y1 > y0 {
            (y - y0) as f32 / (y1 - y0) as f32
        } else {
            0.0
        };
        let hw = (hw0 + (hw1 - hw0) * t).round() as i32;
        brect(buf, cx - hw, y, cx + hw, y, c);
    }
}
fn bshade(c: [u8; 4], f: f32) -> [u8; 4] {
    [
        (c[0] as f32 * f) as u8,
        (c[1] as f32 * f) as u8,
        (c[2] as f32 * f) as u8,
        c[3],
    ]
}

const B_SKIN: &[[u8; 4]] = &[
    [240, 214, 188, 255],
    [224, 184, 152, 255],
    [198, 150, 118, 255],
    [160, 112, 82, 255],
    [120, 84, 60, 255],
];
const B_HAIR: &[[u8; 4]] = &[
    [34, 30, 34, 255],
    [60, 42, 30, 255],
    [104, 72, 44, 255],
    [168, 136, 84, 255],
    [176, 176, 182, 255],
    [128, 56, 40, 255],
    [232, 232, 235, 255],
];
const B_SHIRT: &[[u8; 4]] = &[
    [86, 70, 56, 255],
    [70, 88, 104, 255],
    [96, 64, 72, 255],
    [78, 96, 78, 255],
    [110, 102, 84, 255],
    [58, 62, 74, 255],
    [120, 96, 72, 255],
];
const B_TROUSER: &[[u8; 4]] = &[
    [74, 60, 48, 255],
    [64, 66, 72, 255],
    [96, 84, 64, 255],
    [58, 52, 46, 255],
];
const B_BOOT: [u8; 4] = [56, 42, 32, 255];

fn pick(rng: &mut crate::props::Rng, xs: &[[u8; 4]]) -> [u8; 4] {
    xs[rng.int(xs.len() as u32) as usize]
}

/// The shirt colour, biased by archetype so a class reads at a glance; otherwise seed-chosen.
/// Draws the seed-chosen colour **unconditionally** so the archetype only swaps the result, never the
/// RNG position — otherwise the same seed would yield different hair/beard for a smith vs. a farmer.
fn shirt_for(archetype: &str, rng: &mut crate::props::Rng) -> [u8; 4] {
    let seeded = pick(rng, B_SHIRT);
    let a = archetype.to_ascii_lowercase();
    if a.contains("noble") || a.contains("court") || a.contains("lord") {
        [96, 64, 72, 255]
    } else if a.contains("priest") || a.contains("monk") || a.contains("seer") {
        [58, 62, 74, 255]
    } else if a.contains("soldier") || a.contains("guard") || a.contains("warrior") {
        [70, 78, 86, 255]
    } else if a.contains("smith")
        || a.contains("farm")
        || a.contains("craft")
        || a.contains("labor")
        || a.contains("hunt")
    {
        [86, 70, 56, 255]
    } else {
        seeded
    }
}

/// A full-body pixel character — **feet at the base, front-facing, transparent** — composed
/// deterministically from `seed`; `archetype` biases the clothing. Nearest-sampled, so it stays crisp
/// on the billboard quad; the cel ink-outline traces its alpha silhouette.
pub fn procedural_body_sprite(seed: u64, archetype: &str) -> Image {
    let mut rng = crate::props::Rng::new(seed ^ 0xB0D1_5EED);
    let mut buf = vec![0u8; (BW * BH * 4) as usize];

    let skin = pick(&mut rng, B_SKIN);
    let skin_dk = bshade(skin, 0.85);
    let hair = pick(&mut rng, B_HAIR);
    let shirt = shirt_for(archetype, &mut rng);
    let shirt_dk = bshade(shirt, 0.82);
    let trouser = pick(&mut rng, B_TROUSER);
    let cx = 20;

    // Legs + boots.
    brect(&mut buf, 14, 45, 19, 68, trouser);
    brect(&mut buf, 21, 45, 26, 68, trouser);
    brect(&mut buf, 14, 68, 19, 71, B_BOOT);
    brect(&mut buf, 21, 68, 26, 71, B_BOOT);

    // Torso (shoulders → waist) + a belt.
    btrap(&mut buf, cx, 22, 46, 9.0, 7.0, shirt);
    brect(&mut buf, 13, 45, 27, 46, bshade(trouser, 0.7));

    // Arms (sleeves + hands).
    for s in [-1, 1] {
        let ax = cx + s * 10;
        brect(&mut buf, ax - 1, 24, ax + 1, 40, shirt_dk);
        brect(&mut buf, ax - 1, 40, ax + 1, 43, skin);
    }

    // A vest / apron panel for some — a quick class read. Draw the chance unconditionally (before the
    // `smith` short-circuit) so the figure's later hair/baldness/beard stay put across archetypes.
    let seeded_vest = rng.chance(0.25);
    if archetype.contains("smith") || seeded_vest {
        brect(&mut buf, cx - 4, 24, cx + 4, 42, bshade(shirt, 0.7));
    }

    // Neck + head.
    brect(&mut buf, cx - 2, 18, cx + 1, 22, skin_dk);
    bellipse(&mut buf, cx as f32, 12.0, 6.0, 7.0, skin);
    // Hair cap, carved back to leave a face; a top band; sometimes bald.
    if !rng.chance(0.12) {
        bellipse(&mut buf, cx as f32, 9.0, 6.5, 5.0, hair);
        bellipse(&mut buf, cx as f32, 13.0, 5.0, 5.0, skin);
        bellipse(&mut buf, cx as f32, 7.5, 6.0, 2.5, hair);
    }
    bput(&mut buf, cx - 2, 12, EYE);
    bput(&mut buf, cx + 2, 12, EYE);
    if rng.chance(0.35) {
        bellipse(&mut buf, cx as f32, 15.0, 4.5, 4.0, bshade(hair, 0.92)); // beard
    }

    let mut img = Image::new(
        Extent3d {
            width: BW as u32,
            height: BH as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        buf,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::nearest();
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_sprite_is_drawn_and_per_soul() {
        let a = procedural_body_sprite(seed_of("Maren"), "farmer");
        assert_eq!(
            a.data,
            procedural_body_sprite(seed_of("Maren"), "farmer").data
        );
        let drawn = a
            .data
            .as_ref()
            .unwrap()
            .chunks(4)
            .filter(|p| p[3] > 0)
            .count();
        assert!(drawn > 200, "the figure should draw something ({drawn} px)");
        assert_ne!(
            a.data,
            procedural_body_sprite(seed_of("Bram"), "soldier").data,
            "different souls → different sprites"
        );
    }

    /// Preview: raw RGBA of sample figures to `target/`. Run `cargo test -p app -- --ignored
    /// dump_body_preview`, then `magick -size 40x72 -depth 8 rgba:target/body_X.rgba out.png`.
    #[test]
    #[ignore = "dev preview, writes files"]
    fn dump_body_preview() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../target");
        for (name, arch) in [
            ("Maren", "farmer"),
            ("Bram", "soldier"),
            ("Yalda", "noble"),
            ("Coil", "priest"),
            ("Zoe", "smith"),
            ("Vesper", ""),
            ("Ossa", ""),
            ("Nebro", "court"),
        ] {
            let img = procedural_body_sprite(seed_of(name), arch);
            std::fs::write(
                format!("{dir}/body_{name}.rgba"),
                img.data.as_ref().unwrap(),
            )
            .unwrap();
        }
    }
}
