//! **Achlydesa — a playable window onto the living world.** A first interactive front-end:
//! you spawn a body in the simulated world and *explore* it — walk the land, lift the fog,
//! watch a populace live and talk around you. The simulation (`agents`) is authoritative
//! and bevy-free; this Bevy shell is a thin view over it (the pattern the reference
//! strategy-tactics game uses). Presentation: true-3D hex columns whose height is the
//! land's real elevation, vertex-coloured by terrain, with only the explored map drawn.
//!
//! Controls: **click a tile to travel** · **Space** wait · **T** speak to a soul nearby ·
//! **A/D** orbit · **W/S** tilt · **scroll** zoom. Turn-based — the world moves when you act.
//!
//! `cargo run -p app --release`

use agents::{Coord, Goals, Registry, Setup, Simulation, Terrain};
use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use game_sim::World as GameWorld;
use std::collections::HashSet;

mod palette;

const SQRT3: f32 = 1.732_050_8;
/// Real metres of elevation → world units of column height (≈5000 m → 13 units).
const HEIGHT_SCALE: f32 = 0.0026;
const MIN_LAND_H: f32 = 0.18;
/// Sim ticks per real second while the world is running.
const TICK_DT: f32 = 0.12;

// =====================================================================================
// The simulation, held as a NonSend resource (it wraps a bevy_ecs world of its own; we
// drive it by hand each frame and never let the outer app schedule it).
// =====================================================================================

struct Game {
    sim: Simulation,
    avatar_pos: Coord,
    last_explored: usize,
    last_tick: u64,
    accum: f32,
    status: String,
    convo: Option<Convo>,
}

/// An open, turn-based conversation with one soul within reach. The player is the avatar's
/// mind — the menu is the whole repertoire, *unranked* (this is a role-playing game; the
/// choosing is the human's). Each spoken exchange costs one tick.
struct Convo {
    listener: Entity,
    name: String,
    /// `(intent id, player-facing label)`, in the repertoire's stable order.
    options: Vec<(String, String)>,
    cursor: usize,
    /// The last few lines of *this* conversation, newest last.
    transcript: Vec<String>,
}

#[derive(Resource)]
struct RenderAssets {
    map_mat: Handle<StandardMaterial>,
    avatar_mesh: Handle<Mesh>,
    avatar_mat: Handle<StandardMaterial>,
    npc_mesh: Handle<Mesh>,
    npc_mat: Handle<StandardMaterial>,
}

#[derive(Component)]
struct MapMesh;
#[derive(Component)]
struct Marker;
#[derive(Component)]
struct CamRig {
    dist: f32,
    yaw: f32,
    pitch: f32,
}
#[derive(Component, Clone, Copy)]
enum HudKind {
    Look,
    Talk,
    Help,
}

fn main() {
    let mut sim = build_world();
    sim.spawn_player(None);
    let avatar_pos = sim.player_position().unwrap_or(Coord::new(0, 0));
    let game = Game {
        sim,
        avatar_pos,
        last_explored: usize::MAX,
        last_tick: u64::MAX,
        accum: 0.0,
        status: "Welcome. Click a tile to set out - the world moves when you do.".into(),
        convo: None,
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Achlydesa — exploration".into(), ..default() }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.02, 0.03, 0.05)))
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (drive_sim, talk_input, wait_input, click_travel, rebuild_map, rebuild_markers, camera_control, update_hud).chain(),
    );
    app.world_mut().insert_non_send_resource(game);
    app.run();
}

/// A peopled, settled, *living* world to walk through — dialogue on so the populace talks
/// as you pass; the director left asleep so the world stays whole while you explore.
fn build_world() -> Simulation {
    let reg = Registry::bundled();
    let goals = Goals::from_ron(
        r#"[
            (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
            (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
            (name: "stocked",   condition: Holding(good: Edible, at_least: 12), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
            (name: "solvent",   condition: Money(at_least: 200),      appeal: [(input: Deficit, curve: Linear(m: 0.5, b: 0.0))])
        ]"#,
        &reg,
    )
    .unwrap();
    Simulation::new(Setup {
        width: 48,
        height: 36,
        seed: 7,
        warmup: 150,
        npcs: 60,
        markets: 6,
        markets_on_settlements: true,
        dialogue: true,
        goals,
        registry: reg,
        ..Default::default()
    })
}

// =====================================================================================
// Hex layout — match the sim exactly (pointy-top, odd-offset, via hexx).
// =====================================================================================

fn tile_world(col: i32, row: i32) -> Vec2 {
    let h = hexx::Hex::from_offset_coordinates([col, row], hexx::OffsetHexMode::Odd, hexx::HexOrientation::Pointy);
    let (q, r) = (h.x as f32, h.y as f32);
    Vec2::new(SQRT3 * (q + r / 2.0), 1.5 * r)
}

/// The world-height of a tile's top (sea level for ocean, real relief for land).
fn land_top(gw: &GameWorld, c: Coord) -> f32 {
    let sea = gw.params().sea_level;
    let elev = gw.elevation(c);
    if Terrain::of(elev, sea) == Terrain::Ocean { 0.0 } else { ((elev - sea) * HEIGHT_SCALE).max(MIN_LAND_H) }
}

// =====================================================================================
// Setup
// =====================================================================================

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, game: NonSend<Game>) {
    let map_mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.96, ..default() });
    let avatar_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.82, 0.25),
        emissive: LinearRgba::rgb(0.7, 0.5, 0.12),
        ..default()
    });
    let npc_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.78, 0.80, 0.88), ..default() });
    commands.insert_resource(RenderAssets {
        map_mat,
        avatar_mesh: meshes.add(Capsule3d::new(0.5, 1.8)),
        avatar_mat,
        npc_mesh: meshes.add(Cylinder::new(0.2, 0.55)),
        npc_mat,
    });

    let aw = tile_world(game.avatar_pos.col, game.avatar_pos.row);
    let rig = CamRig { dist: 42.0, yaw: 0.0, pitch: 0.92 };
    commands.spawn((
        Camera3d::default(),
        cam_transform(Vec3::new(aw.x, 0.0, aw.y), &rig),
        rig,
        AmbientLight { brightness: 220.0, ..default() },
    ));

    commands.spawn((
        DirectionalLight { illuminance: 6200.0, shadows_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.6, -0.95, 0.0)),
    ));

    spawn_hud(&mut commands);
}

fn spawn_hud(commands: &mut Commands) {
    let bg = || BackgroundColor(Color::srgba(0.04, 0.05, 0.08, 0.82));
    let bright = || TextColor(Color::srgb(0.90, 0.93, 0.96));
    // Look — top-left.
    commands.spawn((
        HudKind::Look,
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), top: Val::Px(12.0), padding: UiRect::all(Val::Px(8.0)), max_width: Val::Px(360.0), ..default() },
        bg(),
        Text::new(""),
        TextFont { font_size: 16.0, ..default() },
        bright(),
    ));
    // Talk — bottom-left.
    commands.spawn((
        HudKind::Talk,
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), bottom: Val::Px(44.0), padding: UiRect::all(Val::Px(8.0)), max_width: Val::Px(520.0), ..default() },
        bg(),
        Text::new(""),
        TextFont { font_size: 13.0, ..default() },
        bright(),
    ));
    // Help / status — bottom strip.
    commands.spawn((
        HudKind::Help,
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), bottom: Val::Px(10.0), padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)), ..default() },
        bg(),
        Text::new(""),
        TextFont { font_size: 13.0, ..default() },
        TextColor(Color::srgb(0.72, 0.76, 0.82)),
    ));
}

// =====================================================================================
// Driving the simulation
// =====================================================================================

/// **Action-driven time.** The world is turn-based: it advances *exactly one tick per hex
/// the player steps into*, and stands still otherwise. A `sim.step()` both moves the avatar
/// one hex along its route and advances the world one tick — so total ticks always equal
/// total hexes walked. While idle the clock is frozen; the player's action *is* the clock.
/// (Steps are paced by `TICK_DT` only so the walk is watchable — it remains one tick / hex.)
fn drive_sim(mut game: NonSendMut<Game>, time: Res<Time>) {
    let g = &mut *game;
    if !g.sim.player_traveling() {
        g.accum = 0.0;
        return;
    }
    g.accum += time.delta_secs();
    if g.accum >= TICK_DT {
        g.accum -= TICK_DT;
        g.sim.step(); // one hex moved == one world tick
        if let Some(p) = g.sim.player_position() {
            g.avatar_pos = p;
        }
    }
}

/// **Wait** — the second player action. Tap **Space** to let one tick pass where you stand:
/// the avatar holds its ground and the world lives a single moment around it (one action,
/// one tick — the same cost as stepping a hex). Ignored mid-journey (time is already
/// flowing as you walk) and when no avatar is in the world.
fn wait_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.convo.is_some() || !keys.just_pressed(KeyCode::Space) {
        return;
    }
    let g = &mut *game;
    if g.sim.player_traveling() {
        return;
    }
    if g.sim.player_wait() {
        g.status = "You wait, and the world moves a moment on.".into();
    }
}

/// Turn an intent id into a player-facing verb: `"an_accusation"` -> `"accusation"`.
fn pretty_intent(id: &str) -> String {
    let stem = id.strip_prefix("an_").or_else(|| id.strip_prefix("a_")).unwrap_or(id);
    stem.replace('_', " ")
}

/// **Talk** — the third player action. With a soul within reach and the avatar idle, **T**
/// opens a conversation; the player chooses a line (**↑/↓**, **Enter**) and the soul answers,
/// each exchange costing one tick. **Esc** steps back. The player is the avatar's mind: the
/// menu is the whole repertoire, *unranked* — the choosing is yours, not the avatar's traits'.
fn talk_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    let g = &mut *game;

    // Not yet talking: T opens a conversation with the nearest soul in reach (idle only).
    if g.convo.is_none() {
        if g.sim.player_traveling() || !keys.just_pressed(KeyCode::KeyT) {
            return;
        }
        let Some((listener, _)) = g.sim.player_nearby_npcs().into_iter().next() else {
            g.status = "There is no one close enough to speak with.".into();
            return;
        };
        let options: Vec<(String, String)> =
            g.sim.player_intents().into_iter().map(|id| { let label = pretty_intent(&id); (id, label) }).collect();
        if options.is_empty() {
            g.status = "You have no words to offer.".into();
            return;
        }
        let name = g.sim.display_name(listener);
        g.status = format!("You turn to {name}.");
        g.convo = Some(Convo { listener, name, options, cursor: 0, transcript: Vec::new() });
        return;
    }

    // In a conversation. Esc steps back.
    if keys.just_pressed(KeyCode::Escape) {
        let name = g.convo.as_ref().map(|c| c.name.clone()).unwrap_or_default();
        g.convo = None;
        g.status = format!("You take your leave of {name}.");
        return;
    }
    let n = g.convo.as_ref().map_or(0, |c| c.options.len());
    if n == 0 {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowDown)
        && let Some(c) = g.convo.as_mut()
    {
        c.cursor = (c.cursor + 1) % n;
    }
    if keys.just_pressed(KeyCode::ArrowUp)
        && let Some(c) = g.convo.as_mut()
    {
        c.cursor = (c.cursor + n - 1) % n;
    }
    if keys.just_pressed(KeyCode::Enter) {
        // Read the choice out (owned), then speak — the exchange costs one tick.
        let (id, listener, name) = {
            let c = g.convo.as_ref().unwrap();
            (c.options[c.cursor].0.clone(), c.listener, c.name.clone())
        };
        if let Some((line, reply)) = g.sim.player_talk(listener, &id) {
            if let Some(c) = g.convo.as_mut() {
                c.transcript.push(format!("You -> {name}: {}", line.surface));
                match &reply {
                    Some(r) => c.transcript.push(format!("{name}: {}", r.surface)),
                    None => c.transcript.push(format!("{name} keeps their peace.")),
                }
                let overflow = c.transcript.len().saturating_sub(6);
                if overflow > 0 {
                    c.transcript.drain(0..overflow);
                }
            }
            if let Some(p) = g.sim.player_position() {
                g.avatar_pos = p;
            }
            // If the soul wandered out of reach on the tick that just passed, it is over.
            if !g.sim.player_nearby_npcs().iter().any(|(e, _)| *e == listener) {
                g.convo = None;
                g.status = format!("{name} moves on.");
            }
        }
    }
}

fn click_travel(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cams: Query<(&Camera, &GlobalTransform), With<CamRig>>,
    mut game: NonSendMut<Game>,
) {
    if game.convo.is_some() || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok((cam, cam_tf)) = cams.single() else { return };
    let Ok(ray) = cam.viewport_to_world(cam_tf, cursor) else { return };
    let dir = *ray.direction;
    if dir.y.abs() < 1e-5 {
        return;
    }
    let t = -ray.origin.y / dir.y;
    if t < 0.0 {
        return;
    }
    let hit = ray.origin + dir * t;
    let target = Vec2::new(hit.x, hit.z);
    let g = &mut *game;
    let nearest = g.sim.player_explored().into_iter().min_by(|a, b| {
        let da = (tile_world(a.col, a.row) - target).length_squared();
        let db = (tile_world(b.col, b.row) - target).length_squared();
        da.total_cmp(&db)
    });
    if let Some(c) = nearest {
        if g.sim.player_travel_to(c) {
            g.status = format!("Setting out for ({}, {}).", c.col, c.row);
        } else {
            g.status = "No path leads there on foot.".into();
        }
    }
}

// =====================================================================================
// Rendering the world
// =====================================================================================

fn rebuild_map(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    ra: Res<RenderAssets>,
    mut game: NonSendMut<Game>,
    old: Query<Entity, With<MapMesh>>,
) {
    let count = game.sim.player_explored_count();
    if count == game.last_explored {
        return;
    }
    game.last_explored = count;
    for e in &old {
        commands.entity(e).despawn();
    }
    let mesh = build_map_mesh(&game.sim);
    commands.spawn((MapMesh, Mesh3d(meshes.add(mesh)), MeshMaterial3d(ra.map_mat.clone()), Transform::IDENTITY));
}

fn build_map_mesh(sim: &Simulation) -> Mesh {
    let gw = sim.substrate();
    let sea = gw.params().sea_level;
    let (mut pos, mut nor, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for c in sim.player_explored() {
        let elev = gw.elevation(c);
        let terrain = Terrain::of(elev, sea);
        let rgb = palette::tile_rgb(terrain, gw.carrying_capacity(c));
        let centre = tile_world(c.col, c.row);
        let top = if terrain == Terrain::Ocean { 0.0 } else { ((elev - sea) * HEIGHT_SCALE).max(MIN_LAND_H) };
        add_column(&mut pos, &mut nor, &mut col, &mut idx, centre, top, rgb);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx))
}

/// One pointy-top hex column: a fan-triangulated top face plus six side walls (darkened),
/// vertex-coloured. Mirrors the reference game's prism builder.
fn add_column(pos: &mut Vec<[f32; 3]>, nor: &mut Vec<[f32; 3]>, col: &mut Vec<[f32; 4]>, idx: &mut Vec<u32>, centre: Vec2, top: f32, rgb: [f32; 3]) {
    let r = 0.97; // a hairline gap between tiles
    let corners: [Vec2; 6] = std::array::from_fn(|k| {
        let a = std::f32::consts::FRAC_PI_6 + std::f32::consts::FRAC_PI_3 * k as f32;
        centre + Vec2::new(a.cos(), a.sin()) * r
    });
    let rgba = [rgb[0], rgb[1], rgb[2], 1.0];
    let base = pos.len() as u32;
    pos.push([centre.x, top, centre.y]);
    nor.push([0.0, 1.0, 0.0]);
    col.push(rgba);
    for cn in corners {
        pos.push([cn.x, top, cn.y]);
        nor.push([0.0, 1.0, 0.0]);
        col.push(rgba);
    }
    for k in 0..6u32 {
        idx.extend([base, base + 1 + (k + 1) % 6, base + 1 + k]);
    }
    if top <= 0.0 {
        return;
    }
    let side = [rgb[0] * 0.6, rgb[1] * 0.6, rgb[2] * 0.6, 1.0];
    for k in 0..6 {
        let a = corners[k];
        let b = corners[(k + 1) % 6];
        let mid = (a + b) * 0.5 - centre;
        let n = mid.normalize_or_zero();
        let normal = [n.x, 0.0, n.y];
        let bi = pos.len() as u32;
        for (x, y, z) in [(a.x, top, a.y), (b.x, top, b.y), (b.x, 0.0, b.y), (a.x, 0.0, a.y)] {
            pos.push([x, y, z]);
            nor.push(normal);
            col.push(side);
        }
        idx.extend([bi, bi + 2, bi + 1]);
        idx.extend([bi, bi + 3, bi + 2]);
    }
}

fn rebuild_markers(mut commands: Commands, ra: Res<RenderAssets>, mut game: NonSendMut<Game>, old: Query<Entity, With<Marker>>) {
    let tick = game.sim.substrate().tick();
    if tick == game.last_tick {
        return;
    }
    game.last_tick = tick;
    for e in &old {
        commands.entity(e).despawn();
    }
    let g = &mut *game;
    // The avatar — a bright capsule standing on its tile.
    let ap = g.avatar_pos;
    let atop = land_top(g.sim.substrate(), ap);
    let aw = tile_world(ap.col, ap.row);
    commands.spawn((Marker, Mesh3d(ra.avatar_mesh.clone()), MeshMaterial3d(ra.avatar_mat.clone()), Transform::from_xyz(aw.x, atop + 1.2, aw.y)));
    // The populace — only where the player has been (where the fog is lifted).
    let explored: HashSet<(i32, i32)> = g.sim.player_explored().iter().map(|c| (c.col, c.row)).collect();
    for c in g.sim.npc_positions() {
        if !explored.contains(&(c.col, c.row)) {
            continue;
        }
        let top = land_top(g.sim.substrate(), c);
        let w = tile_world(c.col, c.row);
        commands.spawn((Marker, Mesh3d(ra.npc_mesh.clone()), MeshMaterial3d(ra.npc_mat.clone()), Transform::from_xyz(w.x, top + 0.3, w.y)));
    }
}

// =====================================================================================
// Camera (orbits the avatar) and HUD
// =====================================================================================

fn cam_transform(focus: Vec3, rig: &CamRig) -> Transform {
    let rot = Quat::from_axis_angle(Vec3::Y, rig.yaw) * Quat::from_axis_angle(Vec3::X, -rig.pitch);
    Transform::from_translation(focus + rot * (Vec3::Z * rig.dist)).looking_at(focus, Vec3::Y)
}

fn camera_control(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    game: NonSend<Game>,
    mut q: Query<(&mut CamRig, &mut Transform)>,
) {
    let Ok((mut rig, mut tf)) = q.single_mut() else { return };
    let dt = time.delta_secs();
    if keys.pressed(KeyCode::KeyA) {
        rig.yaw += 1.3 * dt;
    }
    if keys.pressed(KeyCode::KeyD) {
        rig.yaw -= 1.3 * dt;
    }
    if keys.pressed(KeyCode::KeyW) {
        rig.pitch = (rig.pitch + 0.9 * dt).min(1.45);
    }
    if keys.pressed(KeyCode::KeyS) {
        rig.pitch = (rig.pitch - 0.9 * dt).max(0.2);
    }
    if scroll.delta.y != 0.0 {
        rig.dist = (rig.dist * (1.0 - scroll.delta.y * 0.12)).clamp(8.0, 140.0);
    }
    let aw = tile_world(game.avatar_pos.col, game.avatar_pos.row);
    *tf = cam_transform(Vec3::new(aw.x, 0.0, aw.y), &rig);
}

fn update_hud(mut game: NonSendMut<Game>, mut texts: Query<(&HudKind, &mut Text)>) {
    let g = &mut *game;
    let view = g.sim.player_view();
    let day = g.sim.substrate().tick();
    let explored = g.sim.player_explored_count();
    let traveling = g.sim.player_traveling();
    let talk: Vec<String> = g
        .sim
        .dialogue_log()
        .iter()
        .rev()
        .take(5)
        .map(|u| format!("  {} -> {}: {}", u.speaker_name, u.listener_name, u.surface))
        .collect();
    let status = g.status.clone();
    let can_talk = !traveling && g.convo.is_none() && view.as_ref().is_some_and(|v| !v.nearby.is_empty());

    // The bottom-left panel: an open conversation, else the voices around you.
    let talk_panel = if let Some(c) = &g.convo {
        let mut s = format!("Speaking with {} - choose your words:\n", c.name);
        for ln in &c.transcript {
            s.push_str("  ");
            s.push_str(ln);
            s.push('\n');
        }
        if !c.transcript.is_empty() {
            s.push('\n');
        }
        for (i, (_, label)) in c.options.iter().enumerate() {
            s.push_str(if i == c.cursor { "  > " } else { "    " });
            s.push_str(label);
            s.push('\n');
        }
        s.push_str("\nUp/Down choose | Enter speak | Esc leave");
        s
    } else {
        let mut s = if talk.is_empty() { "The world is quiet for now...".to_string() } else { format!("Nearby voices:\n{}", talk.join("\n")) };
        if can_talk {
            s.push_str("\n\n(a soul is near - press T to speak)");
        }
        s
    };

    for (kind, mut text) in &mut texts {
        text.0 = match kind {
            HudKind::Look => match &view {
                Some(v) => {
                    let feats = if v.here.features.is_empty() { String::new() } else { format!("\nyou see: {}", v.here.features.join(", ")) };
                    format!(
                        "Day {day}\n({}, {})  {}  {:.0} m\nfertile {:.2}   {} soul(s) near\nfog lifted from {} tiles{}",
                        v.pos.col, v.pos.row, v.here.terrain.name(), v.here.elevation, v.here.fertility, v.nearby.len(), explored, feats,
                    )
                }
                None => "no avatar".into(),
            },
            HudKind::Talk => talk_panel.clone(),
            HudKind::Help => format!(
                "{status}\nclick travel | Space wait | T speak | A/D orbit | W/S tilt | scroll zoom  -  the world moves only when you act",
            ),
        };
    }
}
