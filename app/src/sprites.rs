//! Shared **billboard-sprite** scaffolding for the HD-2D shift (Star Ocean: The Second Story R
//! style — 2-D character sprites composited into the lit 3-D dioramas).
//!
//! Until the real art exists, characters are drawn as a procedurally-generated *placeholder
//! silhouette* (a simple pawn), so a scene reads with people standing in it before any sprite is
//! authored. The material is **alpha-masked**, so the depth prepass writes only the silhouette and
//! the existing cel **ink outline traces the character** — the cel+sprite combo the hybrid look
//! wants. A real sprite drops in by swapping the material's `base_color_texture`; nothing else
//! changes. Every needed sprite/texture is catalogued in `docs/hd2d_assets.md`.

use bevy::asset::RenderAssetUsages;
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

/// A placeholder character silhouette — a round head over a tapered body, white on transparent —
/// `w`×`h` texels. Tint it via the material's `base_color`; the real sprite swaps into the same
/// texture slot. Software-rasterised like the combat field, so it needs no asset on disk.
pub fn placeholder_silhouette(w: u32, h: u32) -> Image {
    let mut data = vec![0u8; (w * h * 4) as usize];
    let (fw, fh) = (w as f32, h as f32);
    let (head_cx, head_cy, head_r) = (fw * 0.5, fh * 0.20, fw * 0.18);
    let (body_top, body_bot) = (fh * 0.34, fh * 0.97);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let in_head = {
                let (dx, dy) = (fx - head_cx, fy - head_cy);
                dx * dx + dy * dy <= head_r * head_r
            };
            let in_body = fy >= body_top && fy <= body_bot && {
                let t = (fy - body_top) / (body_bot - body_top);
                (fx - fw * 0.5).abs() <= fw * (0.14 + 0.18 * t) // shoulders → feet
            };
            if in_head || in_body {
                let i = ((y * w + x) * 4) as usize;
                data[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
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
