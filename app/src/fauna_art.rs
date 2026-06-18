//! Procedural creatures — the fauna the player meets while exploring. Each species
//! gets one low-poly mesh built from its authored [`Form`] (body plan / gait), size
//! and colour, using the same primitives as the props ([`crate::props`]). Creatures
//! are then **animated procedurally** every frame with sine-wave locomotion — a
//! footfall bob, a half-frequency hip roll, a forward lean and squash while moving,
//! plus form-specific motion (serpents slither, drifters hover) and a gentle idle
//! breath when settled — the cheap, scalable technique low-poly games favour over
//! canned clips. Single entity per creature: no skeleton, just a lively transform.
//!
//! The mesh is shared per species; per-creature variety is a seeded scale/phase on
//! the [`Fauna`] component, and smooth movement comes from matching each creature to
//! its stable census id so it *walks* between tiles instead of teleporting.

use crate::mesh::MeshBuf;
use crate::palette::tinted;
use crate::props::{blob, cone, prism, Rng};
use agents::{Bestiary, Form};
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

/// World units a creature glides per second between tiles (a tile is ~1.7 wide, so
/// this crosses one in roughly half a second — a step per world tick).
const GLIDE_SPEED: f32 = 3.6;

// ── Creature meshes ───────────────────────────────────────────────────────────────

/// Proportions of a four-legged body, in local units (forward = +Z, up = +Y, feet
/// at y = 0). Scaled by the species' `size`.
struct Quad {
    body: Vec3,
    body_y: f32,
    leg_r: f32,
    neck_len: f32,
    head_r: f32,
    head_rise: f32,
    tail_len: f32,
}

fn quad_for(form: Form, s: f32) -> Quad {
    match form {
        Form::Strider => Quad {
            body: Vec3::new(0.16, 0.15, 0.30) * s,
            body_y: 0.46 * s,
            leg_r: 0.038 * s,
            neck_len: 0.26 * s,
            head_r: 0.10 * s,
            head_rise: 0.10 * s,
            tail_len: 0.26 * s,
        },
        Form::Lumberer => Quad {
            body: Vec3::new(0.25, 0.23, 0.40) * s,
            body_y: 0.40 * s,
            leg_r: 0.07 * s,
            neck_len: 0.13 * s,
            head_r: 0.15 * s,
            head_rise: -0.05 * s,
            tail_len: 0.12 * s,
        },
        Form::Prowler => Quad {
            body: Vec3::new(0.15, 0.13, 0.40) * s,
            body_y: 0.30 * s,
            leg_r: 0.042 * s,
            neck_len: 0.16 * s,
            head_r: 0.10 * s,
            head_rise: -0.10 * s,
            tail_len: 0.40 * s,
        },
        // Critter and anything else fall back to a tiny scurrier.
        _ => Quad {
            body: Vec3::new(0.14, 0.13, 0.17) * s,
            body_y: 0.16 * s,
            leg_r: 0.028 * s,
            neck_len: 0.05 * s,
            head_r: 0.11 * s,
            head_rise: 0.03 * s,
            tail_len: 0.10 * s,
        },
    }
}

fn build_quadruped(b: &mut MeshBuf, form: Form, s: f32, color: [f32; 3], dark: [f32; 3], rng: &mut Rng) {
    let p = quad_for(form, s);
    // Torso.
    blob(b, Vec3::new(0.0, p.body_y, 0.0), p.body, color, rng, 0.12);
    // Neck and head, reaching forward (+Z).
    let neck_base = Vec3::new(0.0, p.body_y + p.body.y * 0.35, p.body.z * 0.8);
    let head = neck_base + Vec3::new(0.0, p.head_rise, p.neck_len);
    prism(b, neck_base, head, p.leg_r * 1.2, p.head_r * 0.7, 5, color);
    blob(b, head, Vec3::splat(p.head_r), color, rng, 0.1);
    // Four legs, hips near the body's underside, feet on the ground.
    let hipx = p.body.x * 0.78;
    let hipz = p.body.z * 0.62;
    for (sx, sz) in [(-1.0, 1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
        let hip = Vec3::new(sx * hipx, p.body_y - p.body.y * 0.25, sz * hipz);
        let foot = Vec3::new(hip.x, 0.0, hip.z);
        prism(b, hip, foot, p.leg_r, p.leg_r * 0.8, 4, dark);
    }
    // Tail, trailing back (-Z) and down.
    let tail_base = Vec3::new(0.0, p.body_y + p.body.y * 0.2, -p.body.z * 0.85);
    let tail_tip = tail_base + Vec3::new(0.0, -p.body_y * 0.35, -p.tail_len);
    prism(b, tail_base, tail_tip, p.leg_r * 0.9, p.leg_r * 0.3, 4, color);
}

fn build_serpent(b: &mut MeshBuf, s: f32, color: [f32; 3], rng: &mut Rng) {
    let segs = 7;
    let len = s * 1.0;
    let amp = s * 0.16;
    let y = s * 0.11;
    let mut prev = Vec3::new(0.0, y, -len * 0.5);
    for i in 1..=segs {
        let f = i as f32 / segs as f32;
        let z = -len * 0.5 + len * f;
        let x = (f * PI * 2.2).sin() * amp; // a resting S-curve
        let r = s * 0.12 * (1.0 - 0.55 * f);
        let p = Vec3::new(x, y, z);
        prism(b, prev, p, r * 1.1, r, 5, color);
        prev = p;
    }
    blob(b, prev, Vec3::splat(s * 0.13), color, rng, 0.1);
}

fn build_drifter(b: &mut MeshBuf, s: f32, color: [f32; 3], dark: [f32; 3], rng: &mut Rng) {
    let y = s * 0.55; // it floats above the ground
    // A domed bell body.
    blob(b, Vec3::new(0.0, y, 0.0), Vec3::new(s * 0.30, s * 0.36, s * 0.30), color, rng, 0.16);
    cone(b, Vec3::new(0.0, y - s * 0.34, 0.0), -Vec3::Y, s * 0.18, s * 0.26, 6, color);
    // Hanging tendrils.
    let n = 4;
    for k in 0..n {
        let a = TAU * k as f32 / n as f32;
        let (px, pz) = (a.cos() * s * 0.16, a.sin() * s * 0.16);
        let top = Vec3::new(px, y - s * 0.30, pz);
        let bot = Vec3::new(px * 1.6, y - s * 0.7, pz * 1.6);
        prism(b, top, bot, s * 0.028, s * 0.008, 3, dark);
    }
}

/// One creature mesh for a species, built from its form, size and colour.
pub fn build_creature(form: Form, size: f32, color: [f32; 3], seed: u64) -> MeshBuf {
    let mut rng = Rng::new(seed ^ 0xFA00_A001);
    let mut b = MeshBuf::default();
    // A touch larger on screen than the bare body size, and never microscopic.
    let s = (size * 0.85).max(0.5);
    let base = tinted(color);
    let dark = tinted([color[0] * 0.7, color[1] * 0.7, color[2] * 0.7]);
    match form {
        Form::Serpent => build_serpent(&mut b, s, base, &mut rng),
        Form::Drifter => build_drifter(&mut b, s, base, dark, &mut rng),
        other => build_quadruped(&mut b, other, s, base, dark, &mut rng),
    }
    b
}

// ── The art library ────────────────────────────────────────────────────────────────

/// One pre-built mesh per species (indexed by species id) plus the shared matte
/// vertex-colour material the creatures wear (the mesh carries the colour).
#[derive(Resource)]
pub struct FaunaArt {
    pub meshes: Vec<Handle<Mesh>>,
    pub material: Handle<StandardMaterial>,
}

/// Build a creature mesh for every species in the bestiary.
pub fn build_fauna_art(meshes: &mut Assets<Mesh>, material: Handle<StandardMaterial>, bestiary: &Bestiary) -> FaunaArt {
    let handles = bestiary
        .species
        .iter()
        .enumerate()
        .map(|(i, sp)| meshes.add(build_creature(sp.form, sp.size, sp.color, i as u64 + 1).into_mesh()))
        .collect();
    FaunaArt { meshes: handles, material }
}

// ── Live creatures: component + animation ────────────────────────────────────────────

/// A rendered creature, tracked by its stable census `id` so it can be moved
/// smoothly rather than respawned each tick.
#[derive(Component)]
pub struct Fauna {
    pub id: u64,
    pub form: Form,
    /// Gait phase (radians), advanced by movement.
    pub phase: f32,
    /// Per-creature size jitter, so a herd isn't identical.
    pub scale: f32,
    /// Current and target ground positions (world space, y = tile top).
    pub pos: Vec3,
    pub target: Vec3,
    /// Heading (yaw, radians); eased toward the direction of travel.
    pub facing: f32,
}

/// Seed a creature's phase and size jitter deterministically from its id.
fn jitter(id: u64) -> (f32, f32) {
    let h = (id ^ (id >> 29)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let phase = (h >> 40) as f32 / (1u64 << 24) as f32 * TAU;
    let scale = 0.88 + ((h >> 16) & 0xFFFF) as f32 / 65535.0 * 0.24; // 0.88..1.12
    (phase, scale)
}

/// Spawn a creature entity at `pos`.
pub fn spawn_creature(commands: &mut Commands, art: &FaunaArt, id: u64, species: usize, form: Form, pos: Vec3) {
    let (phase, scale) = jitter(id);
    commands.spawn((
        Fauna { id, form, phase, scale, pos, target: pos, facing: 0.0 },
        Mesh3d(art.meshes[species].clone()),
        MeshMaterial3d(art.material.clone()),
        Transform::from_translation(pos),
        Visibility::Visible,
    ));
}

/// Shortest-path angular ease from `a` toward `b` by fraction `t`.
fn ease_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = (b - a) % TAU;
    if d > PI {
        d -= TAU;
    } else if d < -PI {
        d += TAU;
    }
    a + d * t
}

/// Per-frame procedural animation: glide toward target, face the way of travel, and
/// lay a sine-wave gait over the body (form-specific), with an idle breath at rest.
pub fn animate_fauna(time: Res<Time>, mut q: Query<(&mut Transform, &mut Fauna)>) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    for (mut tf, mut f) in &mut q {
        // Glide toward the target tile.
        let delta = f.target - f.pos;
        let dist = delta.length();
        let moving = dist > 0.03;
        if moving {
            let step = (GLIDE_SPEED * dt).min(dist);
            f.pos += delta / dist * step;
            let want = delta.x.atan2(delta.z);
            f.facing = ease_angle(f.facing, want, (dt * 7.0).min(1.0));
        }

        // Advance the gait phase — faster afoot, idling slow.
        let m = if moving { 1.0 } else { 0.0 };
        let freq = match f.form {
            Form::Critter => 11.0,
            Form::Strider => 7.0,
            Form::Prowler => 6.0,
            Form::Lumberer => 4.5,
            Form::Serpent => 5.0,
            Form::Drifter => 1.6,
        };
        f.phase += dt * freq * (0.25 + 0.75 * m);
        let ph = f.phase;

        // Sine-wave gait, laid over the eased heading.
        let (mut bob, mut roll, mut pitch, mut yaw_extra) = (0.0, 0.0, 0.0, 0.0);
        let mut scale_y = 1.0;
        match f.form {
            Form::Serpent => {
                yaw_extra = m * 0.35 * ph.sin(); // slither sway
                bob = m * 0.02 * (ph * 2.0).sin();
            }
            Form::Drifter => {
                bob = 0.06 * (t * 1.3 + ph).sin(); // a constant lazy hover
                yaw_extra = 0.25 * (t * 0.4).sin(); // slow turning drift
                roll = 0.05 * (t * 0.9 + ph).sin();
            }
            _ => {
                // Quadruped: footfall bounce, half-frequency hip roll, lean and squash.
                bob = m * 0.07 * ph.sin().abs();
                roll = m * 0.12 * (ph * 0.5).sin();
                pitch = m * 0.06 * ph.cos();
                scale_y = 1.0 - m * 0.06 * ph.sin().max(0.0);
            }
        }
        // A gentle breath when standing still, so nothing is ever frozen.
        scale_y += (1.0 - m) * 0.03 * (t * 1.7 + ph).sin();

        let s = f.scale;
        tf.translation = f.pos + Vec3::Y * bob * s;
        tf.rotation = Quat::from_axis_angle(Vec3::Y, f.facing + yaw_extra)
            * Quat::from_axis_angle(Vec3::X, pitch)
            * Quat::from_axis_angle(Vec3::Z, roll);
        tf.scale = Vec3::new(s, s * scale_y, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_form_builds_real_geometry() {
        for form in [Form::Strider, Form::Lumberer, Form::Prowler, Form::Critter, Form::Serpent, Form::Drifter] {
            let n = build_creature(form, 1.0, [0.5, 0.5, 0.5], 7).into_mesh().count_vertices();
            assert!(n > 0, "{form:?} produced an empty creature mesh");
        }
    }

    #[test]
    fn creature_generation_is_deterministic() {
        let count = || build_creature(Form::Strider, 1.2, [0.4, 0.5, 0.3], 3).into_mesh().count_vertices();
        assert_eq!(count(), count());
    }
}
