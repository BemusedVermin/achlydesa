//! The **POI scene mode** — stepping *into* a place. Where the overworld is a board you read from
//! above, this is a true-3D toon diorama you stand inside: the settlement's huts and hall, the
//! souls who live there milling in the square, the slates you can read. It is the embodied
//! perception organ — you learn the world's drama by being *among* its people and reading what the
//! place itself recorded, not by skimming a ranked log.
//!
//! It is a first-class mode, built like [`crate::combat`] but over a real second camera rather than
//! a rasterised field: a dedicated [`Camera3d`] on its own [`RenderLayers`] (layer 1) with its own
//! light and orbit rig, composited opaque over the overworld while active. The diorama lives at a
//! clean local origin; nothing is smuggled into the overworld's coordinate space. The authoritative
//! content (who is here, what there is to read) comes from [`agents::Simulation::scene_at`]; the
//! view invents none of it.

use agents::{Coord, SceneView};
use app::theme::{self, ThemeFonts};
use bevy::camera::ScalingMode;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::prelude::*;
use bevy::ui::GlobalZIndex;
use std::f32::consts::TAU;

use crate::props::{Prop, PropLibrary, Rng};
use crate::toon::{ToonMaterial, toon};
use crate::{Game, GameMode};

/// The render layer the diorama and its camera/light share — invisible to the overworld camera.
const SCENE_LAYER: usize = 1;
/// The diorama's local origin: everything is laid out around here.
const ORIGIN: Vec3 = Vec3::ZERO;
/// How many residents the scene shows as figures (and roster rows).
const MAX_FIGURES: usize = 12;

// ── Shared diorama assets (built once at startup) ──────────────────────────────────────────────

/// The meshes + materials the diorama reuses every time a scene opens (so entering a place allocates
/// nothing). Buildings come from the shared [`PropLibrary`]; these are the pieces it lacks.
#[derive(Resource)]
pub(crate) struct PoiAssets {
    ground_mesh: Handle<Mesh>,
    ground_mat: Handle<ToonMaterial>,
    plaza_mesh: Handle<Mesh>,
    plaza_mat: Handle<ToonMaterial>,
    slate_mesh: Handle<Mesh>,
    slate_mat: Handle<ToonMaterial>,
    /// An upright quad for the HD-2D character **billboards** — feet at the base, faces the camera.
    sprite_mesh: Handle<Mesh>,
    /// The avatar's sprite material — a procedural body sprite (or a dropped-in `avatar.png`).
    /// Residents get their *own* per-soul materials, cached in [`ProcSpriteCache`].
    sprite_avatar_mat: Handle<StandardMaterial>,
}

/// Per-soul resident sprite materials, keyed by the soul's seed and built on first sight — so a soul
/// keeps the same procedural body across visits, and re-entering a place allocates nothing new.
#[derive(Resource, Default)]
pub(crate) struct ProcSpriteCache(std::collections::HashMap<u64, Handle<StandardMaterial>>);

/// Build an unlit, alpha-masked billboard material from a real sprite image when present, else a
/// **procedural body sprite** seeded per soul. The mask makes the depth prepass — hence the cel
/// outline — trace the figure's silhouette.
fn body_material(
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    real: Option<Handle<Image>>,
    seed: u64,
    archetype: &str,
) -> Handle<StandardMaterial> {
    let texture =
        real.unwrap_or_else(|| images.add(crate::sprites::procedural_body_sprite(seed, archetype)));
    materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(texture),
        alpha_mode: AlphaMode::Mask(0.5),
        unlit: true,
        ..default()
    })
}

/// The billboard quad's height in world units — a person, tall enough to read beside the buildings.
const SPRITE_H: f32 = 3.0;
const SPRITE_W: f32 = 1.7;

/// The asset root the running game reads from — mirrors `AssetPlugin.file_path` (`../assets`
/// resolved from `CARGO_MANIFEST_DIR`) — so a drop-in file is detected at the *same* place the asset
/// server will load it. Compile-time, so it's cwd-independent under `cargo run`.
const ASSET_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../assets");

/// Load `assets/<rel>` **iff the file is actually there**, else `None`. This is the whole drop-in
/// mechanism: author a real sprite/texture, save it to the catalogued path, and it replaces its
/// placeholder on the next run with no code change. A missing file would otherwise load as a broken
/// (opaque white) texture and wreck the alpha mask, so the existence check matters.
///
/// `nearest` selects point sampling — on for **character sprites** so a low-resolution pixel-art PNG
/// renders as crisp blocks instead of a blurry smear; off (linear) for tiling environment textures.
fn loaded_if_present(
    asset_server: &AssetServer,
    rel: &str,
    nearest: bool,
) -> Option<Handle<Image>> {
    if !std::path::Path::new(ASSET_ROOT).join(rel).exists() {
        return None;
    }
    Some(if nearest {
        asset_server.load_with_settings(
            rel.to_string(),
            |s: &mut bevy::image::ImageLoaderSettings| {
                s.sampler = bevy::image::ImageSampler::nearest();
            },
        )
    } else {
        asset_server.load(rel.to_string())
    })
}

/// Build the diorama's shared assets — the cel-shaded ground/plaza/slate, plus the HD-2D character
/// **sprite** pieces. Each surface/sprite uses a real authored file when present (see
/// `assets/sprites/`, `assets/textures/`), otherwise a procedural placeholder.
pub(crate) fn build_assets(
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    toon_mats: &mut Assets<ToonMaterial>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> PoiAssets {
    // A cel surface: a real tileable texture when present (untinted), else the flat fallback colour.
    let surface = |rel: &str, fallback: Color, roughness: f32| {
        let (base_color, base_color_texture) = match loaded_if_present(asset_server, rel, false) {
            Some(h) => (Color::WHITE, Some(h)),
            None => (fallback, None),
        };
        toon(StandardMaterial {
            base_color,
            base_color_texture,
            perceptual_roughness: roughness,
            ..default()
        })
    };
    PoiAssets {
        ground_mesh: meshes.add(Cylinder::new(15.0, 0.4)),
        ground_mat: toon_mats.add(surface(
            "textures/ground_grass.png",
            Color::srgb(0.34, 0.40, 0.26),
            0.97,
        )),
        plaza_mesh: meshes.add(Cylinder::new(7.0, 0.18)),
        plaza_mat: toon_mats.add(surface(
            "textures/plaza_stone.png",
            Color::srgb(0.46, 0.44, 0.40),
            0.95,
        )),
        slate_mesh: meshes.add(Cuboid::new(1.3, 2.0, 0.28)),
        slate_mat: toon_mats.add(surface(
            "textures/slate_face.png",
            Color::srgb(0.30, 0.31, 0.36),
            0.9,
        )),
        sprite_mesh: meshes.add(Rectangle::new(SPRITE_W, SPRITE_H)),
        sprite_avatar_mat: body_material(
            images,
            materials,
            loaded_if_present(asset_server, "sprites/avatar.png", true),
            crate::sprites::seed_of("avatar"),
            "traveller",
        ),
    }
}

// ── State ──────────────────────────────────────────────────────────────────────────────────────

/// The live scene over a place: its authoritative content plus the small bit of view state (which
/// slate is open, what was just learned). A **resource present only in [`GameMode::PoiScene`]** —
/// inserted on enter, removed on exit — so "are we in a scene" is the state, never a nullable field.
#[derive(Resource)]
pub(crate) struct PoiScene {
    view: SceneView,
    /// The slate the player is reading, if any (index into `view.readables`).
    reading: Option<usize>,
    /// Lore the open slate just taught (shown under its text).
    learned: Vec<String>,
}

/// Where a pending [`GameMode::PoiScene`] transition is headed — set by a trigger, read by the
/// `OnEnter` builder. Decouples "ask to enter" from "build the diorama".
#[derive(Resource, Default)]
pub(crate) struct PoiTarget(pub Option<Coord>);

// ── Markers ──────────────────────────────────────────────────────────────────────────────────

/// The scene's dedicated camera (layer 1), toggled active only while a scene is open.
#[derive(Component)]
pub(crate) struct PoiCam;
/// The scene camera's orbit rig (separate from the overworld's [`crate::CamRig`]).
#[derive(Component)]
pub(crate) struct PoiCamRig {
    yaw: f32,
    pitch: f32,
    dist: f32,
    zoom: f32,
}
/// A diorama entity — despawned wholesale when the scene closes.
#[derive(Component)]
pub(crate) struct PoiProp;

// ── Triggers ───────────────────────────────────────────────────────────────────────────────────

/// Ask to step into the place at `c`: record the target and request the [`GameMode::PoiScene`]
/// transition. The `OnEnter` builder ([`enter_scene`]) does the rest next frame.
pub(crate) fn request_enter(target: &mut PoiTarget, next: &mut NextState<GameMode>, c: Coord) {
    target.0 = Some(c);
    next.set(GameMode::PoiScene);
}

// ── Startup: the scene camera, its light, and the overlay UI ─────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_infra(
    commands: &mut Commands,
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    toon_mats: &mut Assets<ToonMaterial>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    fonts: &ThemeFonts,
) {
    commands.insert_resource(build_assets(
        asset_server,
        meshes,
        toon_mats,
        images,
        materials,
    ));
    commands.init_resource::<ProcSpriteCache>();

    // The diorama camera: orthographic 2.5-D like the overworld, on layer 1, inactive until a scene
    // opens. The depth prepass earns the cel outline for free (see `outline.rs`); HDR + Bloom give
    // the soft HD-2D glow. `Tonemapping` keeps the cel colours where they were under HDR.
    let rig = PoiCamRig {
        yaw: 0.45,
        pitch: 0.62,
        dist: 48.0,
        zoom: 20.0,
    };
    commands.spawn((
        PoiCam,
        Camera3d::default(),
        Camera {
            order: 1,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.50, 0.56, 0.64)),
            ..default()
        },
        // NOTE: HDR + Bloom (the soft HD-2D glow) are deferred — a secondary HDR camera composited
        // over the LDR overworld through the custom outline post-process renders black; it needs the
        // render graph sorted out. The outline is already format-aware (LDR/HDR) for when it lands.
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 1.0,
            },
            scale: rig.zoom,
            ..OrthographicProjection::default_3d()
        }),
        DepthPrepass,
        Msaa::Off,
        poi_cam_transform(&rig),
        rig,
        RenderLayers::layer(SCENE_LAYER),
        AmbientLight {
            brightness: 900.0,
            color: Color::srgb(0.78, 0.83, 0.92),
            ..default()
        },
    ));
    // A key light for the diorama (layer 1 only — the overworld's light never reaches it).
    commands.spawn((
        DirectionalLight {
            illuminance: 9500.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.7, -0.85, 0.0)),
        RenderLayers::layer(SCENE_LAYER),
    ));

    spawn_overlay(commands, fonts);
}

/// The diorama camera's transform from its rig, orbiting the local origin at eye height.
fn poi_cam_transform(rig: &PoiCamRig) -> Transform {
    let focus = ORIGIN + Vec3::Y * 1.2;
    let rot = Quat::from_axis_angle(Vec3::Y, rig.yaw) * Quat::from_axis_angle(Vec3::X, -rig.pitch);
    Transform::from_translation(focus + rot * (Vec3::Z * rig.dist)).looking_at(focus, Vec3::Y)
}

// ── Lifecycle: OnEnter / OnExit(GameMode::PoiScene) ───────────────────────────────────────────

/// `OnEnter(PoiScene)`: build the diorama for the targeted place, wake the scene camera, and insert
/// the [`PoiScene`] resource the in-scene systems read.
pub(crate) fn enter_scene(
    mut commands: Commands,
    mut game: NonSendMut<Game>,
    mut target: ResMut<PoiTarget>,
    assets: Res<PoiAssets>,
    lib: Res<PropLibrary>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<ProcSpriteCache>,
    mut cam: Query<&mut Camera, With<PoiCam>>,
) {
    // The only caller (`request_enter`) always sets the target before the transition; if it didn't,
    // `in_state(PoiScene)` systems would panic on the missing `PoiScene` resource a frame later, so
    // fail loudly here at the actual bug instead.
    let Some(place) = target.0.take() else {
        panic!("OnEnter(PoiScene) fired with no PoiTarget set — caller must request_enter first");
    };
    let view = game.sim.scene_at(place);
    build_diorama(
        &mut commands,
        &assets,
        &lib,
        &view,
        &mut images,
        &mut materials,
        &mut cache,
    );
    if let Ok(mut c) = cam.single_mut() {
        c.is_active = true;
    }
    game.status = format!("You step into {}.", view.title);
    commands.insert_resource(PoiScene {
        view,
        reading: None,
        learned: Vec::new(),
    });
}

/// `OnExit(PoiScene)`: tear the diorama down, sleep the scene camera, drop the resource.
pub(crate) fn exit_scene(
    mut commands: Commands,
    mut game: NonSendMut<Game>,
    props: Query<Entity, With<PoiProp>>,
    mut cam: Query<&mut Camera, With<PoiCam>>,
) {
    for e in &props {
        commands.entity(e).despawn();
    }
    if let Ok(mut c) = cam.single_mut() {
        c.is_active = false;
    }
    commands.remove_resource::<PoiScene>();
    game.status = "You step back into the open.".into();
}

/// Spawn the diorama — ground, plaza, a ring of buildings, the resident figures, the avatar, and the
/// slates — at the local origin on the scene layer.
fn build_diorama(
    commands: &mut Commands,
    assets: &PoiAssets,
    lib: &PropLibrary,
    view: &SceneView,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    cache: &mut ProcSpriteCache,
) {
    let layer = RenderLayers::layer(SCENE_LAYER);
    let mut rng = Rng::new(0x5CE7_1107 ^ tile_salt(view.place));

    // Ground + plaza.
    commands.spawn((
        PoiProp,
        Mesh3d(assets.ground_mesh.clone()),
        MeshMaterial3d(assets.ground_mat.clone()),
        Transform::from_translation(ORIGIN - Vec3::Y * 0.2),
        layer.clone(),
    ));
    commands.spawn((
        PoiProp,
        Mesh3d(assets.plaza_mesh.clone()),
        MeshMaterial3d(assets.plaza_mat.clone()),
        Transform::from_translation(ORIGIN + Vec3::Y * 0.05),
        layer.clone(),
    ));

    // A ring of dwellings around the square, each facing inward; a hall anchors the back.
    let homes = 7 + rng.int(3) as usize;
    for i in 0..homes {
        let a = TAU * i as f32 / homes as f32 + rng.range(-0.18, 0.18);
        let r = rng.range(9.5, 12.0);
        let pos = Vec3::new(r * a.cos(), 0.0, r * a.sin());
        let prop = if rng.chance(0.4) {
            Prop::Hut
        } else {
            Prop::House
        };
        spawn_building(
            commands,
            lib,
            &layer,
            prop,
            pos,
            a,
            rng.range(3.0, 3.8),
            &mut rng,
        );
    }
    spawn_building(
        commands,
        lib,
        &layer,
        Prop::Hall,
        Vec3::new(0.0, 0.0, -12.5),
        0.0,
        4.2,
        &mut rng,
    );

    // The residents, in a loose arc across the square facing the viewer.
    let shown = view.residents.len().min(MAX_FIGURES);
    for (i, res) in view.residents.iter().take(shown).enumerate() {
        let t = if shown > 1 {
            i as f32 / (shown - 1) as f32
        } else {
            0.5
        };
        let spread = 1.1; // radians of arc
        let a = std::f32::consts::FRAC_PI_2 + (t - 0.5) * spread;
        let r = rng.range(3.0, 5.0);
        let pos = Vec3::new(r * a.cos(), 0.0, r * a.sin());
        // Each resident is their *own* procedural figure, keyed by the **ECS-stable entity id** (two
        // souls can share a name), cached so a soul keeps its body across visits.
        let seed = res.entity.to_bits();
        let mat = match cache.0.get(&seed) {
            Some(h) => h.clone(),
            None => {
                let h = body_material(images, materials, None, seed, &res.archetype);
                cache.0.insert(seed, h.clone());
                h
            }
        };
        commands.spawn((
            PoiProp,
            crate::sprites::Billboard,
            Mesh3d(assets.sprite_mesh.clone()),
            MeshMaterial3d(mat),
            Transform::from_translation(pos + Vec3::Y * (SPRITE_H * 0.5)),
            layer.clone(),
        ));
    }

    // You, at the mouth of the square — the gold sprite.
    commands.spawn((
        PoiProp,
        crate::sprites::Billboard,
        Mesh3d(assets.sprite_mesh.clone()),
        MeshMaterial3d(assets.sprite_avatar_mat.clone()),
        Transform::from_translation(Vec3::new(0.0, SPRITE_H * 0.5, 7.0)),
        layer.clone(),
    ));

    // The slates to read, in a short row to one side.
    for (i, _) in view.readables.iter().enumerate() {
        let pos = Vec3::new(-5.5 + i as f32 * 2.4, 0.0, 4.0);
        commands.spawn((
            PoiProp,
            Mesh3d(assets.slate_mesh.clone()),
            MeshMaterial3d(assets.slate_mat.clone()),
            Transform::from_translation(pos + Vec3::Y * 1.0)
                .with_rotation(Quat::from_rotation_y(0.25)),
            layer.clone(),
        ));
    }
}

fn spawn_building(
    commands: &mut Commands,
    lib: &PropLibrary,
    layer: &RenderLayers,
    prop: Prop,
    pos: Vec3,
    inward_angle: f32,
    scale: f32,
    rng: &mut Rng,
) {
    let Some(mesh) = lib.pick(prop, rng) else {
        return;
    };
    commands.spawn((
        PoiProp,
        Mesh3d(mesh),
        MeshMaterial3d(lib.material.clone()),
        Transform {
            translation: pos,
            rotation: Quat::from_rotation_y(inward_angle + std::f32::consts::PI),
            scale: Vec3::splat(scale),
        },
        layer.clone(),
    ));
}

/// A stable per-place salt for the diorama's procedural layout.
fn tile_salt(c: Coord) -> u64 {
    ((c.col as u64) << 20) ^ (c.row as u64).wrapping_mul(0x9E37_79B9)
}

// ── Camera orbit (while a scene is open) ─────────────────────────────────────────────────────

pub(crate) fn poi_camera(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    time: Res<Time>,
    game: NonSend<Game>,
    mut q: Query<(&mut PoiCamRig, &mut Transform, &mut Projection), With<PoiCam>>,
) {
    // The orbit freezes while a conversation overlay is up (its keys belong to the dialogue).
    if game.convo.is_some() {
        return;
    }
    let Ok((mut rig, mut tf, mut proj)) = q.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    if keys.pressed(KeyCode::KeyA) {
        rig.yaw += 1.2 * dt;
    }
    if keys.pressed(KeyCode::KeyD) {
        rig.yaw -= 1.2 * dt;
    }
    if keys.pressed(KeyCode::KeyW) {
        rig.pitch = (rig.pitch + 0.8 * dt).min(1.3);
    }
    if keys.pressed(KeyCode::KeyS) {
        rig.pitch = (rig.pitch - 0.8 * dt).max(0.15);
    }
    if scroll.delta.y != 0.0 {
        rig.zoom = (rig.zoom * (1.0 - scroll.delta.y * 0.12)).clamp(8.0, 40.0);
        if let Projection::Orthographic(ortho) = &mut *proj {
            ortho.scale = rig.zoom;
        }
    }
    *tf = poi_cam_transform(&rig);
}

/// Turn every character billboard to face the scene camera, so the 2-D sprites read as standing
/// people from any orbit angle — the SO2R billboard behaviour.
#[allow(clippy::type_complexity)]
pub(crate) fn billboard_sprites(
    cam: Query<&Transform, (With<PoiCam>, Without<crate::sprites::Billboard>)>,
    mut sprites: Query<&mut Transform, (With<crate::sprites::Billboard>, Without<PoiCam>)>,
) {
    if let Ok(cam_tf) = cam.single() {
        crate::sprites::face_camera(cam_tf.translation, sprites.iter_mut());
    }
}

// ── Input: Esc backs out (reading → conversation → leave the place) ──────────────────────────

pub(crate) fn poi_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut game: NonSendMut<Game>,
    mut poi: ResMut<PoiScene>,
    mut next: ResMut<NextState<GameMode>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    // A conversation overlay owns Esc first (close it, stay in the scene).
    if game.convo.is_some() {
        game.convo = None;
    } else if poi.reading.is_some() {
        poi.reading = None;
        poi.learned.clear();
    } else {
        next.set(GameMode::Overworld);
    }
}

// ── Clicks on the roster / readables rows ────────────────────────────────────────────────────

pub(crate) fn poi_clicks(
    mut game: NonSendMut<Game>,
    mut poi: ResMut<PoiScene>,
    rows: Query<(&PoiRow, &Interaction), Changed<Interaction>>,
) {
    let mut action = None;
    for (row, i) in &rows {
        if *i == Interaction::Pressed {
            action = Some(row.0);
            break;
        }
    }
    let Some(row) = action else { return };
    match row {
        RowKind::Talk(idx) => {
            if let Some(e) = poi.view.residents.get(idx).map(|r| r.entity) {
                crate::open_conversation_with(&mut game, e);
            }
        }
        RowKind::Read(idx) => {
            let grants = poi
                .view
                .readables
                .get(idx)
                .map(|r| r.grants.clone())
                .unwrap_or_default();
            poi.learned = game.sim.learn_lore(&grants);
            poi.reading = Some(idx);
        }
    }
}

/// Dev only: leave the scene a few frames after entering, so the exit teardown can be screenshotted
/// (`ACHLYDESA_POI_LEAVE`).
pub(crate) fn dev_leave(mut next: ResMut<NextState<GameMode>>, mut frames: Local<u32>) {
    if std::env::var("ACHLYDESA_POI_LEAVE").is_err() {
        return;
    }
    *frames += 1;
    if *frames == 6 {
        next.set(GameMode::Overworld);
    }
}

/// The **single-scene invariant**: away from [`GameMode::PoiScene`], the scene camera must be asleep
/// and no diorama entity may exist — so only one 3-D scene (the overworld *or* the diorama) is ever
/// rendering. Returns `true` when it holds. Drives both the runtime guard and a test.
pub(crate) fn scene_invariant_holds(
    in_poi: bool,
    poi_cam_active: bool,
    diorama_count: usize,
) -> bool {
    in_poi || (!poi_cam_active && diorama_count == 0)
}

#[cfg(test)]
mod tests {
    use super::scene_invariant_holds;

    #[test]
    fn only_one_scene_renders() {
        // In the scene: the diorama and its active camera are expected.
        assert!(scene_invariant_holds(true, true, 12));
        // Out of the scene: a clean teardown — camera asleep, no diorama.
        assert!(scene_invariant_holds(false, false, 0));
        // Out of the scene but the camera never slept → two scenes drawing (the reported bug).
        assert!(!scene_invariant_holds(false, true, 0));
        // Out of the scene but diorama entities leaked → the town keeps rendering.
        assert!(!scene_invariant_holds(false, false, 5));
    }
}

/// Dev/screenshot only: open the first slate so the reading panel can be captured (`ACHLYDESA_POI_READ`).
pub(crate) fn dev_read(mut game: NonSendMut<Game>, mut poi: ResMut<PoiScene>) {
    if poi.reading.is_some() || std::env::var("ACHLYDESA_POI_READ").is_err() {
        return;
    }
    let grants = poi
        .view
        .readables
        .first()
        .map(|r| r.grants.clone())
        .unwrap_or_default();
    poi.learned = game.sim.learn_lore(&grants);
    poi.reading = Some(0);
}

// ── Overlay UI ───────────────────────────────────────────────────────────────────────────────

#[derive(Component)]
pub(crate) struct PoiRoot;
#[derive(Component)]
pub(crate) struct PoiTitle;
#[derive(Component)]
pub(crate) struct PoiRow(RowKind);
#[derive(Clone, Copy)]
enum RowKind {
    Talk(usize),
    Read(usize),
}
#[derive(Component)]
pub(crate) struct PoiRowLabel(usize, bool); // (index, is_readable)
#[derive(Component)]
pub(crate) struct PoiReadingPanel;
#[derive(Component)]
pub(crate) struct PoiReadingTitle;
#[derive(Component)]
pub(crate) struct PoiReadingText;

const ROSTER_ROWS: usize = MAX_FIGURES;
const READ_ROWS: usize = 4;
/// Side-panel width (reference px).
const PANEL_W: f32 = 250.0;

fn spawn_overlay(commands: &mut Commands, fonts: &ThemeFonts) {
    commands
        .spawn((
            PoiRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            GlobalZIndex(70),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            // ── Header band: the settlement name, with the controls quietly to the right. A solid
            //    strip with a hairline base, so it reads as a title bar, not a floating chip.
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(theme::SP_XL), Val::Px(theme::SP_MD)),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    border: UiRect::bottom(Val::Px(theme::BORDER_W)),
                    ..default()
                },
                BackgroundColor(theme::INK_RAISED),
                BorderColor::all(theme::BORDER),
            ))
            .with_children(|bar| {
                bar.spawn((theme::display(fonts, ""), PoiTitle));
                bar.spawn(theme::micro(
                    fonts,
                    "A/D  W/S  look around        Esc  leave",
                ));
            });

            // ── Body: residents | the framed main stage | readables. The stage is a bordered,
            //    transparent panel so the diorama reads as the game's *main view* (per
            //    docs/UI Mockups/town.png), not bare 3-D behind floating boxes.
            root.spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                column_gap: Val::Px(theme::SP_LG),
                padding: UiRect::all(Val::Px(theme::SP_LG)),
                ..default()
            })
            .with_children(|body| {
                spawn_list(body, fonts, "Those here", ROSTER_ROWS, false);
                body.spawn((
                    Node {
                        flex_grow: 1.0,
                        height: Val::Percent(100.0),
                        border: UiRect::all(Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(theme::RADIUS)),
                        ..default()
                    },
                    BorderColor::all(theme::BORDER),
                ));
                spawn_list(body, fonts, "To read", READ_ROWS, true);
            });
        });

    // ── The reading panel — a centred document: serif title, the inscription, what it taught. ──
    commands
        .spawn((
            PoiReadingPanel,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(31.0),
                top: Val::Percent(28.0),
                width: Val::Percent(38.0),
                padding: UiRect::all(Val::Px(theme::SP_XL)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme::SP_MD),
                ..default()
            },
            theme::panel_chrome(),
            GlobalZIndex(78),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn((theme::heading(fonts, ""), PoiReadingTitle));
            p.spawn(theme::divider());
            p.spawn((theme::body(fonts, ""), PoiReadingText));
            p.spawn(theme::micro(fonts, "Esc to set it down"));
        });
}

fn spawn_list(
    parent: &mut ChildSpawnerCommands,
    fonts: &ThemeFonts,
    heading: &str,
    rows: usize,
    readable: bool,
) {
    parent
        .spawn((
            Node {
                width: Val::Px(PANEL_W),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme::SP_XS),
                padding: UiRect::all(Val::Px(theme::SP_MD)),
                ..default()
            },
            theme::panel_chrome(),
        ))
        .with_children(|col| {
            col.spawn(theme::heading(fonts, heading));
            col.spawn(theme::divider());
            for i in 0..rows {
                col.spawn((
                    PoiRow(if readable {
                        RowKind::Read(i)
                    } else {
                        RowKind::Talk(i)
                    }),
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(theme::SP_SM), Val::Px(theme::SP_XS + 1.0)),
                        border_radius: BorderRadius::all(Val::Px(theme::RADIUS_SM)),
                        display: Display::None,
                        ..default()
                    },
                    // Transparent until hovered/selected — a clean list, not a grid of buttons.
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|b| {
                    b.spawn((theme::body(fonts, ""), PoiRowLabel(i, readable)));
                });
            }
        });
}

/// Fill the overlay each frame: visibility, title, the two row lists (with hover), the reading panel.
#[allow(clippy::type_complexity)]
pub(crate) fn update_poi_overlay(
    poi: Option<Res<PoiScene>>,
    mut root: Query<&mut Visibility, (With<PoiRoot>, Without<PoiReadingPanel>)>,
    mut reading_vis: Query<&mut Visibility, (With<PoiReadingPanel>, Without<PoiRoot>)>,
    mut title: Query<
        &mut Text,
        (
            With<PoiTitle>,
            Without<PoiRowLabel>,
            Without<PoiReadingTitle>,
            Without<PoiReadingText>,
        ),
    >,
    mut rows: Query<(&PoiRow, &Interaction, &mut Node, &mut BackgroundColor)>,
    mut labels: Query<
        (&PoiRowLabel, &mut Text),
        (
            Without<PoiTitle>,
            Without<PoiReadingTitle>,
            Without<PoiReadingText>,
        ),
    >,
    mut reading_title: Query<
        &mut Text,
        (
            With<PoiReadingTitle>,
            Without<PoiTitle>,
            Without<PoiRowLabel>,
            Without<PoiReadingText>,
        ),
    >,
    mut reading_body: Query<
        &mut Text,
        (
            With<PoiReadingText>,
            Without<PoiTitle>,
            Without<PoiRowLabel>,
            Without<PoiReadingTitle>,
        ),
    >,
) {
    let on = poi.is_some();
    if let Ok(mut v) = root.single_mut() {
        *v = if on {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    let Some(poi) = poi.as_deref() else {
        if let Ok(mut v) = reading_vis.single_mut() {
            *v = Visibility::Hidden;
        }
        return;
    };

    if let Ok(mut t) = title.single_mut() {
        t.0 = poi.view.title.clone();
    }

    // Each row: shown only if the scene has that resident/readable; lit when hovered.
    for (row, interaction, mut node, mut bg) in &mut rows {
        let has = match row.0 {
            RowKind::Talk(i) => i < poi.view.residents.len().min(ROSTER_ROWS),
            RowKind::Read(i) => i < poi.view.readables.len().min(READ_ROWS),
        };
        node.display = if has { Display::Flex } else { Display::None };
        bg.0 = if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            theme::INK_RAISED
        } else {
            Color::NONE
        };
    }
    for (label, mut text) in &mut labels {
        text.0 = if label.1 {
            poi.view
                .readables
                .get(label.0)
                .map(|r| r.title.clone())
                .unwrap_or_default()
        } else {
            poi.view
                .residents
                .get(label.0)
                .map(|r| match &r.demeanour {
                    Some(d) => format!("{}  \u{2014} {d}", r.name),
                    None => r.name.clone(),
                })
                .unwrap_or_default()
        };
    }

    // The reading panel: shown only while reading; the title, the inscription, what it taught.
    let open = poi.reading.and_then(|i| poi.view.readables.get(i));
    if let Ok(mut v) = reading_vis.single_mut() {
        *v = if open.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if let Some(r) = open {
        if let Ok(mut t) = reading_title.single_mut() {
            t.0 = r.title.clone();
        }
        if let Ok(mut t) = reading_body.single_mut() {
            let mut body = r.lines.join("\n\n");
            if !poi.learned.is_empty() {
                body.push_str(&format!("\n\nYou now know: {}", poi.learned.join("; ")));
            }
            t.0 = body;
        }
    }
}
