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
use crate::{Game, GameMode, RenderAssets};

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
    figure_mesh: Handle<Mesh>,
}

/// Build the diorama's shared assets — a grassy ground disc, a paved plaza, a stone slate, and a
/// person-sized figure capsule (the cel material bands them like the rest of the world).
pub(crate) fn build_assets(
    meshes: &mut Assets<Mesh>,
    toon_mats: &mut Assets<ToonMaterial>,
) -> PoiAssets {
    PoiAssets {
        ground_mesh: meshes.add(Cylinder::new(15.0, 0.4)),
        ground_mat: toon_mats.add(toon(StandardMaterial {
            base_color: Color::srgb(0.34, 0.40, 0.26),
            perceptual_roughness: 0.97,
            ..default()
        })),
        plaza_mesh: meshes.add(Cylinder::new(7.0, 0.18)),
        plaza_mat: toon_mats.add(toon(StandardMaterial {
            base_color: Color::srgb(0.46, 0.44, 0.40),
            perceptual_roughness: 0.95,
            ..default()
        })),
        slate_mesh: meshes.add(Cuboid::new(1.3, 2.0, 0.28)),
        slate_mat: toon_mats.add(toon(StandardMaterial {
            base_color: Color::srgb(0.30, 0.31, 0.36),
            perceptual_roughness: 0.9,
            ..default()
        })),
        figure_mesh: meshes.add(Capsule3d::new(0.42, 1.5)),
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

pub(crate) fn spawn_infra(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    toon_mats: &mut Assets<ToonMaterial>,
    fonts: &ThemeFonts,
) {
    commands.insert_resource(build_assets(meshes, toon_mats));

    // The diorama camera: orthographic 2.5-D like the overworld, on layer 1, inactive until a scene
    // opens. The depth prepass earns the cel outline for free (see `outline.rs`).
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
    ra: Res<RenderAssets>,
    mut cam: Query<&mut Camera, With<PoiCam>>,
) {
    let Some(place) = target.0.take() else {
        return;
    };
    let view = game.sim.scene_at(place);
    build_diorama(&mut commands, &assets, &lib, &ra, &view);
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
    ra: &RenderAssets,
    view: &SceneView,
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
        commands.spawn((
            PoiProp,
            Mesh3d(assets.figure_mesh.clone()),
            MeshMaterial3d(ra.npc_mat.clone()),
            Transform::from_translation(pos + Vec3::Y * 1.17)
                .with_rotation(Quat::from_rotation_y(rng.range(-0.4, 0.4))),
            layer.clone(),
        ));
        let _ = (i, res);
    }

    // You, at the mouth of the square.
    commands.spawn((
        PoiProp,
        Mesh3d(assets.figure_mesh.clone()),
        MeshMaterial3d(ra.avatar_mat.clone()),
        Transform::from_translation(Vec3::new(0.0, 1.17, 7.0)),
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
pub(crate) struct PoiHint;
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
pub(crate) struct PoiReadingText;

const ROSTER_ROWS: usize = MAX_FIGURES;
const READ_ROWS: usize = 4;

fn spawn_overlay(commands: &mut Commands, fonts: &ThemeFonts) {
    // A transparent full-screen frame: the diorama shows through the centre; panels sit at the edges.
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
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::all(Val::Px(theme::SP_LG)),
                ..default()
            },
            GlobalZIndex(70),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            // Title bar.
            root.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(theme::SP_MD), Val::Px(theme::SP_SM)),
                    align_self: AlignSelf::FlexStart,
                    ..default()
                },
                theme::panel_chrome(),
            ))
            .with_children(|bar| {
                bar.spawn((theme::display(fonts, ""), PoiTitle));
            });

            // Middle: the two interaction panels, left (people) and right (readables), centre open.
            root.spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::FlexStart,
                padding: UiRect::vertical(Val::Px(theme::SP_MD)),
                ..default()
            })
            .with_children(|mid| {
                spawn_list(mid, fonts, "Those here", ROSTER_ROWS, false);
                spawn_list(mid, fonts, "To read", READ_ROWS, true);
            });

            // Hint.
            root.spawn((
                theme::micro(
                    fonts,
                    "click someone to speak \u{00b7} a slate to read \u{00b7} A/D/W/S orbit \u{00b7} Esc to leave",
                ),
                PoiHint,
            ));
        });

    // The reading panel — a centred slate, shown only while reading.
    commands
        .spawn((
            PoiReadingPanel,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(30.0),
                top: Val::Percent(28.0),
                width: Val::Percent(40.0),
                padding: UiRect::all(Val::Px(theme::SP_LG)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme::SP_SM),
                ..default()
            },
            theme::panel_chrome(),
            GlobalZIndex(78),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn((theme::body(fonts, ""), PoiReadingText));
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
                width: Val::Px(260.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme::SP_XS),
                padding: UiRect::all(Val::Px(theme::SP_SM)),
                ..default()
            },
            theme::panel_chrome(),
        ))
        .with_children(|col| {
            col.spawn(theme::label(fonts, heading));
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
                        padding: UiRect::axes(Val::Px(theme::SP_SM), Val::Px(4.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(theme::RADIUS_SM)),
                        display: Display::None,
                        ..default()
                    },
                    BackgroundColor(theme::INK_RAISED),
                    BorderColor::all(theme::BORDER),
                ))
                .with_children(|b| {
                    b.spawn((theme::body(fonts, ""), PoiRowLabel(i, readable)));
                });
            }
        });
}

/// Fill the overlay each frame: visibility, title, the two row lists, and the reading panel.
#[allow(clippy::type_complexity)]
pub(crate) fn update_poi_overlay(
    poi: Option<Res<PoiScene>>,
    mut root: Query<&mut Visibility, (With<PoiRoot>, Without<PoiReadingPanel>)>,
    mut reading_vis: Query<&mut Visibility, (With<PoiReadingPanel>, Without<PoiRoot>)>,
    mut title: Query<
        &mut Text,
        (
            With<PoiTitle>,
            Without<PoiReadingText>,
            Without<PoiRowLabel>,
        ),
    >,
    mut reading_text: Query<
        &mut Text,
        (
            With<PoiReadingText>,
            Without<PoiTitle>,
            Without<PoiRowLabel>,
        ),
    >,
    mut rows: Query<(&PoiRow, &mut Node, &mut BackgroundColor)>,
    mut labels: Query<(&PoiRowLabel, &mut Text), (Without<PoiTitle>, Without<PoiReadingText>)>,
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

    // Show/hide each row by whether the scene has that resident / readable, and label it.
    for (row, mut node, mut bg) in &mut rows {
        let has = match row.0 {
            RowKind::Talk(i) => i < poi.view.residents.len().min(ROSTER_ROWS),
            RowKind::Read(i) => i < poi.view.readables.len().min(READ_ROWS),
        };
        node.display = if has { Display::Flex } else { Display::None };
        bg.0 = theme::INK_RAISED;
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
                    Some(d) => format!("{} \u{2014} {d}", r.name),
                    None => r.name.clone(),
                })
                .unwrap_or_default()
        };
    }

    // The reading panel.
    let reading = poi.reading.and_then(|i| poi.view.readables.get(i));
    if let Ok(mut v) = reading_vis.single_mut() {
        *v = if reading.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if let (Ok(mut t), Some(r)) = (reading_text.single_mut(), reading) {
        let mut body = format!("{}\n\n{}", r.title, r.lines.join("\n"));
        if !poi.learned.is_empty() {
            body.push_str(&format!("\n\nYou now know: {}", poi.learned.join("; ")));
        }
        body.push_str("\n\n(Esc to set it down)");
        t.0 = body;
    }
}
