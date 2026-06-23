//! Procedural, flat-shaded low-poly props — the trees, scrub, and rock the land wears. Each
//! kind has a small **library** of pre-generated variants (built once at startup); the scatter
//! step then instances them with a seeded transform, so the world has variety without ever
//! re-running a generator per tile. View-only and deterministic: a kind's variants come from a
//! fixed seed, so the same build always yields the same forest.
//!
//! Buildings (settlements, courts, ruins) are generated here too but mapped from feature kinds
//! by the extensible registry in [`crate::feature_art`].

use crate::mesh::MeshBuf;
use crate::palette::tinted;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use std::f32::consts::TAU;

// ── A tiny self-contained PRNG (SplitMix64), so the renderer needs no sim Rng trait ──────────

pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x2545_F491_4F6C_DD1D,
        }
    }
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.unit()
    }
    /// Uniform integer in `0..n`.
    pub fn int(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as u32
        }
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }
}

/// A stable seed for a tile's decoration (mixes the coord and a per-purpose salt).
pub fn tile_seed(col: i32, row: i32, salt: u64) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for v in [col as i64 as u64, row as i64 as u64, salt] {
        h = (h ^ v).wrapping_mul(0x100_0000_01B3);
    }
    h
}

// ── Low-poly geometry primitives ─────────────────────────────────────────────────────────────

/// Two unit vectors spanning the plane perpendicular to `dir`.
fn basis(dir: Vec3) -> (Vec3, Vec3) {
    let a = if dir.x.abs() < 0.9 { Vec3::X } else { Vec3::Z };
    let u = dir.cross(a).normalize_or_zero();
    let v = dir.cross(u).normalize_or_zero();
    (u, v)
}

/// A tapered n-gon prism (a limb / trunk) from `p0` (radius `r0`) to `p1` (radius `r1`).
pub fn prism(
    buf: &mut MeshBuf,
    p0: Vec3,
    p1: Vec3,
    r0: f32,
    r1: f32,
    sides: usize,
    color: [f32; 3],
) {
    let axis = (p1 - p0).normalize_or_zero();
    let (u, v) = basis(axis);
    let ring = |c: Vec3, r: f32| -> Vec<Vec3> {
        (0..sides)
            .map(|k| {
                let a = TAU * k as f32 / sides as f32;
                c + (u * a.cos() + v * a.sin()) * r
            })
            .collect()
    };
    let (lo, hi) = (ring(p0, r0), ring(p1, r1));
    for k in 0..sides {
        let kn = (k + 1) % sides;
        buf.quad(lo[k], hi[k], hi[kn], lo[kn], color);
    }
}

/// A cone rising `height` from `base` along `dir`, base radius `radius`.
pub fn cone(
    buf: &mut MeshBuf,
    base: Vec3,
    dir: Vec3,
    height: f32,
    radius: f32,
    sides: usize,
    color: [f32; 3],
) {
    let (u, v) = basis(dir.normalize_or_zero());
    let apex = base + dir.normalize_or_zero() * height;
    let ring: Vec<Vec3> = (0..sides)
        .map(|k| {
            let a = TAU * k as f32 / sides as f32;
            base + (u * a.cos() + v * a.sin()) * radius
        })
        .collect();
    for k in 0..sides {
        let kn = (k + 1) % sides;
        buf.tri(ring[k], apex, ring[kn], color);
    }
}

/// The twelve vertices of a unit icosahedron (golden-ratio layout) and its twenty faces — the
/// base shape every faceted blob (foliage puff, boulder) is grown from.
fn ico() -> ([Vec3; 12], [[usize; 3]; 20]) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let v = [
        Vec3::new(-1.0, t, 0.0),
        Vec3::new(1.0, t, 0.0),
        Vec3::new(-1.0, -t, 0.0),
        Vec3::new(1.0, -t, 0.0),
        Vec3::new(0.0, -1.0, t),
        Vec3::new(0.0, 1.0, t),
        Vec3::new(0.0, -1.0, -t),
        Vec3::new(0.0, 1.0, -t),
        Vec3::new(t, 0.0, -1.0),
        Vec3::new(t, 0.0, 1.0),
        Vec3::new(-t, 0.0, -1.0),
        Vec3::new(-t, 0.0, 1.0),
    ]
    .map(|p| p.normalize());
    let f = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    (v, f)
}

/// A faceted blob: an icosahedron scaled by `radii`, each vertex pushed in/out by up to
/// `jitter`, flat-shaded. Foliage when round and green; a boulder when squat and grey.
pub fn blob(
    buf: &mut MeshBuf,
    center: Vec3,
    radii: Vec3,
    color: [f32; 3],
    rng: &mut Rng,
    jitter: f32,
) {
    let (v, faces) = ico();
    let disp: [f32; 12] = std::array::from_fn(|_| 1.0 + rng.range(-jitter, jitter));
    let p = |i: usize| center + (v[i] * disp[i]) * radii;
    for f in faces {
        buf.tri(p(f[0]), p(f[1]), p(f[2]), color);
    }
}

// ── Colour helpers (all cool-tinted to share the world's mood) ────────────────────────────────

fn wood(rng: &mut Rng) -> [f32; 3] {
    tinted([
        rng.range(0.24, 0.34),
        rng.range(0.17, 0.23),
        rng.range(0.10, 0.14),
    ])
}
fn leaf(rng: &mut Rng) -> [f32; 3] {
    let g = rng.range(0.30, 0.46);
    tinted([g * rng.range(0.45, 0.65), g, g * rng.range(0.30, 0.45)])
}
fn needle(rng: &mut Rng) -> [f32; 3] {
    tinted([
        rng.range(0.12, 0.20),
        rng.range(0.28, 0.38),
        rng.range(0.20, 0.28),
    ])
}
fn scrub(rng: &mut Rng) -> [f32; 3] {
    tinted([
        rng.range(0.30, 0.40),
        rng.range(0.36, 0.46),
        rng.range(0.20, 0.28),
    ])
}
fn deadwood(rng: &mut Rng) -> [f32; 3] {
    let g = rng.range(0.52, 0.66);
    tinted([g, g * 0.97, g * 0.92])
}
fn stone(rng: &mut Rng) -> [f32; 3] {
    // A wider value range plus a little warm/cool drift, so a scatter of rock isn't one flat grey.
    let g = rng.range(0.32, 0.56);
    let warm = rng.range(-0.05, 0.07);
    tinted([
        (g + warm).clamp(0.0, 1.0),
        g,
        (g - warm * 0.6).clamp(0.0, 1.0),
    ])
}

// ── The prop kinds and their generators ───────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Prop {
    // Natural cover.
    Broadleaf,
    Conifer,
    Shrub,
    DeadTree,
    GrassTuft,
    Boulder,
    // Built structures (placed by the feature-art registry).
    Hut,
    House,
    Hall,
    Keep,
    Temple,
    Tower,
    Beacon,
    StoneRing,
    Obelisk,
    Cairn,
    Ruin,
    Shrine,
}

impl Prop {
    /// The vegetation/rock kinds and how many variants of each to pre-generate.
    pub const NATURAL: &'static [(Prop, u32)] = &[
        (Prop::Broadleaf, 6),
        (Prop::Conifer, 6),
        (Prop::Shrub, 5),
        (Prop::DeadTree, 5),
        (Prop::GrassTuft, 4),
        (Prop::Boulder, 6),
    ];

    /// The built-structure kinds and their variant counts.
    pub const BUILDINGS: &'static [(Prop, u32)] = &[
        (Prop::Hut, 4),
        (Prop::House, 4),
        (Prop::Hall, 3),
        (Prop::Keep, 3),
        (Prop::Temple, 3),
        (Prop::Tower, 3),
        (Prop::Beacon, 2),
        (Prop::StoneRing, 2),
        (Prop::Obelisk, 3),
        (Prop::Cairn, 3),
        (Prop::Ruin, 4),
        (Prop::Shrine, 2),
    ];
}

/// Recursive branching: a tapered limb, then either a foliage cluster (leaf) or child limbs.
fn branch(
    buf: &mut MeshBuf,
    base: Vec3,
    dir: Vec3,
    len: f32,
    rad: f32,
    depth: u32,
    rng: &mut Rng,
    wd: [f32; 3],
    lf: [f32; 3],
) {
    let dir = dir.normalize_or_zero();
    let tip = base + dir * len;
    prism(buf, base, tip, rad, rad * 0.62, 5, wd);
    if depth == 0 {
        let puffs = 1 + rng.int(2);
        for _ in 0..puffs {
            let off = Vec3::new(
                rng.range(-0.25, 0.25),
                rng.range(-0.05, 0.25),
                rng.range(-0.25, 0.25),
            ) * len;
            let r = len * rng.range(0.55, 0.85);
            blob(buf, tip + off, Vec3::new(r, r * 0.85, r), lf, rng, 0.22);
        }
        return;
    }
    let kids = 2 + rng.int(2);
    for _ in 0..kids {
        let nd = dir
            + Vec3::new(
                rng.range(-1.0, 1.0),
                rng.range(0.1, 0.7),
                rng.range(-1.0, 1.0),
            ) * 0.6;
        branch(
            buf,
            tip,
            nd,
            len * rng.range(0.6, 0.78),
            rad * 0.62,
            depth - 1,
            rng,
            wd,
            lf,
        );
    }
}

fn gen_broadleaf(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let (wd, lf) = (wood(rng), leaf(rng));
    let h = rng.range(0.9, 1.5);
    branch(
        &mut b,
        Vec3::ZERO,
        Vec3::Y,
        h * 0.5,
        h * 0.06,
        1,
        rng,
        wd,
        lf,
    );
    b
}

fn gen_conifer(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let (wd, nd) = (wood(rng), needle(rng));
    let h = rng.range(1.1, 1.9);
    prism(&mut b, Vec3::ZERO, Vec3::Y * h, h * 0.045, h * 0.02, 5, wd);
    let tiers = 3 + rng.int(2);
    for i in 0..tiers {
        let frac = i as f32 / tiers as f32;
        let y = h * (0.2 + frac * 0.62);
        let r = (1.0 - frac) * h * 0.26 + 0.07;
        cone(&mut b, Vec3::Y * y, Vec3::Y, h * 0.34, r, 6, nd);
    }
    b
}

fn gen_shrub(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let c = scrub(rng);
    let lobes = 1 + rng.int(2);
    for _ in 0..lobes {
        let off = Vec3::new(rng.range(-0.18, 0.18), 0.0, rng.range(-0.18, 0.18));
        let r = rng.range(0.22, 0.38);
        blob(
            &mut b,
            off + Vec3::Y * r * 0.8,
            Vec3::new(r, r * 0.7, r),
            c,
            rng,
            0.28,
        );
    }
    b
}

fn gen_dead_tree(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let wd = deadwood(rng);
    let h = rng.range(0.9, 1.6);
    // No foliage: a bare, gnarled fork — the ash-grove / petrified look.
    dead_branch(&mut b, Vec3::ZERO, Vec3::Y, h * 0.5, h * 0.055, 2, rng, wd);
    b
}

fn dead_branch(
    buf: &mut MeshBuf,
    base: Vec3,
    dir: Vec3,
    len: f32,
    rad: f32,
    depth: u32,
    rng: &mut Rng,
    wd: [f32; 3],
) {
    let dir = dir.normalize_or_zero();
    let tip = base + dir * len;
    prism(buf, base, tip, rad, rad * 0.6, 5, wd);
    if depth == 0 {
        return;
    }
    let kids = 2 + rng.int(2);
    for _ in 0..kids {
        let nd = dir
            + Vec3::new(
                rng.range(-1.2, 1.2),
                rng.range(0.0, 0.5),
                rng.range(-1.2, 1.2),
            ) * 0.8;
        dead_branch(
            buf,
            tip,
            nd,
            len * rng.range(0.55, 0.72),
            rad * 0.6,
            depth - 1,
            rng,
            wd,
        );
    }
}

fn gen_grass_tuft(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let blades = 3 + rng.int(3);
    for _ in 0..blades {
        let c = tinted([
            rng.range(0.30, 0.42),
            rng.range(0.42, 0.55),
            rng.range(0.18, 0.26),
        ]);
        let base = Vec3::new(rng.range(-0.12, 0.12), 0.0, rng.range(-0.12, 0.12));
        let h = rng.range(0.15, 0.3);
        let lean = Vec3::new(rng.range(-0.08, 0.08), h, rng.range(-0.08, 0.08));
        let w = 0.025;
        let side = Vec3::new(w, 0.0, w);
        // A thin two-sided blade (a quad), drawn both ways so it reads from any angle.
        let (a, bb, tip) = (base - side, base + side, base + lean);
        b.tri(a, bb, tip, c);
        b.tri(bb, a, tip, c);
    }
    b
}

fn gen_boulder(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let c = stone(rng);
    let r = rng.range(0.3, 0.55);
    blob(
        &mut b,
        Vec3::Y * r * 0.55,
        Vec3::new(r, r * rng.range(0.55, 0.8), r * rng.range(0.85, 1.15)),
        c,
        rng,
        0.34,
    );
    if rng.chance(0.6) {
        let r2 = r * rng.range(0.4, 0.7);
        let off = Vec3::new(rng.range(-0.3, 0.3), 0.0, rng.range(-0.3, 0.3));
        blob(
            &mut b,
            off + Vec3::Y * r2 * 0.5,
            Vec3::new(r2, r2 * 0.7, r2),
            stone(rng),
            rng,
            0.34,
        );
    }
    b
}

// ── Building primitives & material palette ────────────────────────────────────────────────────

fn wattle(rng: &mut Rng) -> [f32; 3] {
    tinted([
        rng.range(0.50, 0.60),
        rng.range(0.42, 0.50),
        rng.range(0.28, 0.34),
    ])
}
fn thatch(rng: &mut Rng) -> [f32; 3] {
    tinted([
        rng.range(0.40, 0.50),
        rng.range(0.33, 0.40),
        rng.range(0.18, 0.24),
    ])
}
fn slate(rng: &mut Rng) -> [f32; 3] {
    tinted([
        rng.range(0.27, 0.35),
        rng.range(0.29, 0.35),
        rng.range(0.33, 0.40),
    ])
}
/// The cult's false gold — a cold, pale gilt.
fn gold() -> [f32; 3] {
    tinted([0.82, 0.68, 0.27])
}

/// An axis-aligned box centred at `c` with half-extents `he`.
fn cuboid(buf: &mut MeshBuf, c: Vec3, he: Vec3, color: [f32; 3]) {
    let p = |sx: f32, sy: f32, sz: f32| c + Vec3::new(sx * he.x, sy * he.y, sz * he.z);
    buf.quad(
        p(1.0, -1.0, -1.0),
        p(1.0, 1.0, -1.0),
        p(1.0, 1.0, 1.0),
        p(1.0, -1.0, 1.0),
        color,
    ); // +X
    buf.quad(
        p(-1.0, -1.0, 1.0),
        p(-1.0, 1.0, 1.0),
        p(-1.0, 1.0, -1.0),
        p(-1.0, -1.0, -1.0),
        color,
    ); // -X
    buf.quad(
        p(-1.0, -1.0, 1.0),
        p(1.0, -1.0, 1.0),
        p(1.0, 1.0, 1.0),
        p(-1.0, 1.0, 1.0),
        color,
    ); // +Z
    buf.quad(
        p(1.0, -1.0, -1.0),
        p(-1.0, -1.0, -1.0),
        p(-1.0, 1.0, -1.0),
        p(1.0, 1.0, -1.0),
        color,
    ); // -Z
    buf.quad(
        p(-1.0, 1.0, -1.0),
        p(-1.0, 1.0, 1.0),
        p(1.0, 1.0, 1.0),
        p(1.0, 1.0, -1.0),
        color,
    ); // +Y
    buf.quad(
        p(-1.0, -1.0, -1.0),
        p(1.0, -1.0, -1.0),
        p(1.0, -1.0, 1.0),
        p(-1.0, -1.0, 1.0),
        color,
    ); // -Y
}

/// A four-sided pyramid roof rising `h` above a rectangle (half-widths `hx`,`hz`) at `base`.
fn pyramid_roof(buf: &mut MeshBuf, base: Vec3, hx: f32, hz: f32, h: f32, color: [f32; 3]) {
    let apex = base + Vec3::Y * h;
    let q = |sx: f32, sz: f32| base + Vec3::new(sx * hx, 0.0, sz * hz);
    let (c00, c10, c11, c01) = (q(-1.0, -1.0), q(1.0, -1.0), q(1.0, 1.0), q(-1.0, 1.0));
    buf.tri(c10, c00, apex, color); // -Z
    buf.tri(c11, c10, apex, color); // +X
    buf.tri(c01, c11, apex, color); // +Z
    buf.tri(c00, c01, apex, color); // -X
}

/// A gable (ridge-along-X) roof above a rectangle, ridge height `h`.
fn gable_roof(buf: &mut MeshBuf, base: Vec3, hx: f32, hz: f32, h: f32, color: [f32; 3]) {
    let q = |sx: f32, sz: f32| base + Vec3::new(sx * hx, 0.0, sz * hz);
    let (c00, c10, c11, c01) = (q(-1.0, -1.0), q(1.0, -1.0), q(1.0, 1.0), q(-1.0, 1.0));
    let r0 = base + Vec3::new(-hx, h, 0.0);
    let r1 = base + Vec3::new(hx, h, 0.0);
    buf.quad(c01, c11, r1, r0, color); // +Z slope
    buf.quad(c10, c00, r0, r1, color); // -Z slope
    buf.tri(c11, c10, r1, color); // +X gable
    buf.tri(c00, c01, r0, color); // -X gable
}

// ── Building generators ────────────────────────────────────────────────────────────────────────

fn gen_hut(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let (w, d, h) = (
        rng.range(0.22, 0.30),
        rng.range(0.22, 0.30),
        rng.range(0.20, 0.28),
    );
    cuboid(&mut b, Vec3::Y * h, Vec3::new(w, h, d), wattle(rng));
    pyramid_roof(
        &mut b,
        Vec3::Y * (2.0 * h),
        w * 1.18,
        d * 1.18,
        rng.range(0.18, 0.28),
        thatch(rng),
    );
    b
}

fn gen_house(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let (w, d, h) = (
        rng.range(0.30, 0.40),
        rng.range(0.22, 0.30),
        rng.range(0.26, 0.34),
    );
    let wall = if rng.chance(0.5) {
        wattle(rng)
    } else {
        stone(rng)
    };
    cuboid(&mut b, Vec3::Y * h, Vec3::new(w, h, d), wall);
    let roof = if rng.chance(0.5) {
        slate(rng)
    } else {
        thatch(rng)
    };
    gable_roof(
        &mut b,
        Vec3::Y * (2.0 * h),
        w * 1.1,
        d * 1.12,
        rng.range(0.2, 0.32),
        roof,
    );
    b
}

fn gen_hall(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let (w, d, h) = (
        rng.range(0.45, 0.60),
        rng.range(0.26, 0.34),
        rng.range(0.32, 0.42),
    );
    cuboid(&mut b, Vec3::Y * h, Vec3::new(w, h, d), stone(rng));
    gable_roof(
        &mut b,
        Vec3::Y * (2.0 * h),
        w * 1.06,
        d * 1.12,
        rng.range(0.22, 0.34),
        slate(rng),
    );
    b
}

fn gen_keep(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let r = rng.range(0.26, 0.34);
    let h = rng.range(0.7, 1.0);
    cuboid(
        &mut b,
        Vec3::Y * (h * 0.5),
        Vec3::new(r, h * 0.5, r),
        stone(rng),
    );
    let cr = r * 0.26;
    for (sx, sz) in [
        (-1.0, -1.0),
        (1.0, -1.0),
        (1.0, 1.0),
        (-1.0, 1.0),
        (0.0, -1.0),
        (1.0, 0.0),
        (0.0, 1.0),
        (-1.0, 0.0),
    ] {
        cuboid(
            &mut b,
            Vec3::new(sx * r, h + cr, sz * r),
            Vec3::splat(cr),
            stone(rng),
        );
    }
    b
}

fn gen_temple(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let (w, d) = (rng.range(0.40, 0.50), rng.range(0.30, 0.40));
    cuboid(
        &mut b,
        Vec3::Y * 0.05,
        Vec3::new(w * 1.18, 0.05, d * 1.18),
        stone(rng),
    ); // step
    cuboid(&mut b, Vec3::Y * 0.13, Vec3::new(w, 0.07, d), stone(rng)); // platform
    let colh = rng.range(0.40, 0.55);
    let st = stone(rng);
    for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let foot = Vec3::new(sx * w * 0.82, 0.2, sz * d * 0.82);
        prism(&mut b, foot, foot + Vec3::Y * colh, 0.045, 0.04, 6, st);
    }
    let roof_y = 0.2 + colh;
    cuboid(
        &mut b,
        Vec3::Y * (roof_y + 0.05),
        Vec3::new(w * 1.06, 0.05, d * 1.06),
        slate(rng),
    );
    gable_roof(
        &mut b,
        Vec3::Y * (roof_y + 0.1),
        w * 0.95,
        d * 0.95,
        rng.range(0.18, 0.26),
        gold(),
    );
    b
}

fn gen_tower(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let r = rng.range(0.18, 0.26);
    let h = rng.range(0.9, 1.4);
    prism(
        &mut b,
        Vec3::ZERO,
        Vec3::Y * h,
        r * 1.1,
        r * 0.85,
        6,
        stone(rng),
    );
    cone(
        &mut b,
        Vec3::Y * h,
        Vec3::Y,
        rng.range(0.26, 0.4),
        r * 1.3,
        6,
        slate(rng),
    );
    b
}

fn gen_beacon(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let r = rng.range(0.16, 0.22);
    let h = rng.range(0.8, 1.1);
    prism(
        &mut b,
        Vec3::ZERO,
        Vec3::Y * h,
        r * 1.1,
        r * 0.9,
        6,
        stone(rng),
    );
    // A pale flame that never feeds — bright, but not literally lit (no emissive material).
    cone(
        &mut b,
        Vec3::Y * (h + 0.03),
        Vec3::Y,
        0.24,
        0.12,
        5,
        [0.95, 0.6, 0.22],
    );
    b
}

fn gen_stone_ring(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let n = 5 + rng.int(3);
    let rad = rng.range(0.45, 0.62);
    for k in 0..n {
        let a = TAU * k as f32 / n as f32 + rng.range(-0.12, 0.12);
        let p = Vec3::new(rad * a.cos(), 0.0, rad * a.sin());
        let sh = rng.range(0.32, 0.58);
        let sw = rng.range(0.06, 0.10);
        cuboid(
            &mut b,
            p + Vec3::Y * sh,
            Vec3::new(sw, sh, sw * 1.4),
            stone(rng),
        );
    }
    b
}

fn gen_obelisk(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let h = rng.range(0.7, 1.1);
    let r = rng.range(0.08, 0.12);
    let st = stone(rng);
    prism(&mut b, Vec3::ZERO, Vec3::Y * h, r, r * 0.6, 4, st);
    pyramid_roof(&mut b, Vec3::Y * h, r * 0.6, r * 0.6, r * 1.6, st);
    b
}

fn gen_cairn(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let layers = 3 + rng.int(2);
    let (mut y, mut r) = (0.0, rng.range(0.30, 0.40));
    for _ in 0..layers {
        blob(
            &mut b,
            Vec3::Y * (y + r * 0.5),
            Vec3::new(r, r * 0.6, r),
            stone(rng),
            rng,
            0.3,
        );
        y += r * 0.7;
        r *= 0.7;
    }
    b
}

fn gen_ruin(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let (w, d) = (rng.range(0.35, 0.5), rng.range(0.3, 0.45));
    let segs = 3 + rng.int(3);
    for _ in 0..segs {
        let p = Vec3::new(rng.range(-w, w), 0.0, rng.range(-d, d));
        let hh = rng.range(0.12, 0.4);
        let (sw, sl) = (rng.range(0.05, 0.11), rng.range(0.12, 0.3));
        let he = if rng.chance(0.5) {
            Vec3::new(sl, hh, sw)
        } else {
            Vec3::new(sw, hh, sl)
        };
        cuboid(&mut b, p + Vec3::Y * hh, he, stone(rng));
    }
    b
}

fn gen_shrine(rng: &mut Rng) -> MeshBuf {
    let mut b = MeshBuf::default();
    let h = rng.range(0.25, 0.35);
    prism(&mut b, Vec3::ZERO, Vec3::Y * h, 0.05, 0.04, 4, wood(rng));
    pyramid_roof(&mut b, Vec3::Y * h, 0.1, 0.1, 0.12, thatch(rng));
    blob(
        &mut b,
        Vec3::Y * 0.05,
        Vec3::splat(0.06),
        stone(rng),
        rng,
        0.3,
    );
    b
}

/// Build one variant mesh of a kind from a deterministic per-(kind, variant) seed.
pub fn generate(prop: Prop, variant: u32) -> MeshBuf {
    let mut rng = Rng::new(tile_seed(prop as i32, variant as i32, 0x9051));
    match prop {
        Prop::Broadleaf => gen_broadleaf(&mut rng),
        Prop::Conifer => gen_conifer(&mut rng),
        Prop::Shrub => gen_shrub(&mut rng),
        Prop::DeadTree => gen_dead_tree(&mut rng),
        Prop::GrassTuft => gen_grass_tuft(&mut rng),
        Prop::Boulder => gen_boulder(&mut rng),
        Prop::Hut => gen_hut(&mut rng),
        Prop::House => gen_house(&mut rng),
        Prop::Hall => gen_hall(&mut rng),
        Prop::Keep => gen_keep(&mut rng),
        Prop::Temple => gen_temple(&mut rng),
        Prop::Tower => gen_tower(&mut rng),
        Prop::Beacon => gen_beacon(&mut rng),
        Prop::StoneRing => gen_stone_ring(&mut rng),
        Prop::Obelisk => gen_obelisk(&mut rng),
        Prop::Cairn => gen_cairn(&mut rng),
        Prop::Ruin => gen_ruin(&mut rng),
        Prop::Shrine => gen_shrine(&mut rng),
    }
}

// ── The library: handles to every pre-generated variant, plus the shared materials ────────────

#[derive(Resource)]
pub struct PropLibrary {
    variants: HashMap<Prop, Vec<Handle<Mesh>>>,
    /// The cel material every prop shares (the mesh carries the colour; the toon pass bands the
    /// lighting). The whole world rides this one [`crate::toon::ToonMaterial`].
    pub material: Handle<crate::toon::ToonMaterial>,
}

impl PropLibrary {
    /// A deterministic variant handle for a kind, chosen from `rng`. `None` if the kind has no
    /// variants registered (so callers can skip cleanly).
    pub fn pick(&self, prop: Prop, rng: &mut Rng) -> Option<Handle<Mesh>> {
        let vs = self.variants.get(&prop)?;
        if vs.is_empty() {
            return None;
        }
        Some(vs[rng.int(vs.len() as u32) as usize].clone())
    }

    pub fn register(&mut self, meshes: &mut Assets<Mesh>, prop: Prop, count: u32) {
        let handles = (0..count)
            .map(|v| meshes.add(generate(prop, v).into_mesh()))
            .collect();
        self.variants.insert(prop, handles);
    }
}

/// Build the natural-prop library (trees, scrub, rock) and the built structures, registering a
/// handful of variants of each kind.
pub fn build_library(
    meshes: &mut Assets<Mesh>,
    material: Handle<crate::toon::ToonMaterial>,
) -> PropLibrary {
    let mut lib = PropLibrary {
        variants: HashMap::default(),
        material,
    };
    for &(prop, count) in Prop::NATURAL.iter().chain(Prop::BUILDINGS) {
        lib.register(meshes, prop, count);
    }
    lib
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every prop variant generates real geometry — no empty meshes, no panics from the
    /// branching/icosphere/cuboid builders (a guard on the index math).
    #[test]
    fn every_variant_has_geometry() {
        for &(prop, count) in Prop::NATURAL.iter().chain(Prop::BUILDINGS) {
            assert!(count > 0, "{prop:?} registered with no variants");
            for v in 0..count {
                let verts = generate(prop, v).into_mesh().count_vertices();
                assert!(verts > 0, "{prop:?} variant {v} produced an empty mesh");
            }
        }
    }

    /// The same (kind, variant) seed always yields the same vertex count — the determinism the
    /// rest of the renderer relies on for a stable world.
    #[test]
    fn generation_is_deterministic() {
        for v in 0..4 {
            let a = generate(Prop::Broadleaf, v).into_mesh().count_vertices();
            let b = generate(Prop::Broadleaf, v).into_mesh().count_vertices();
            assert_eq!(a, b);
        }
    }
}
