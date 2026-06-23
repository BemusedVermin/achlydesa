//! The exploration UI: a tile you can hover (outline + cursor tooltip), left-click to inspect
//! (a detail panel), and right-click to travel to; floating names over discovered settlements;
//! and a standing legend. All of it reads the sim — it never writes — and is dark to the fog of
//! war (only explored tiles pick, only discovered features label).

use crate::layout::{tile_top, tile_world};
use crate::mesh::MeshBuf;
use crate::{CamRig, Game};
use agents::{Category, Coord, Simulation, Terrain};
use app::theme::{self, ThemeFonts};
use bevy::prelude::*;
use bevy::ui::{BorderRadius, UiScale};
use std::f32::consts::{FRAC_PI_3, FRAC_PI_6};

const LABEL_POOL: usize = 28;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RingKind {
    Hover,
    Select,
}
#[derive(Component)]
pub struct Ring(pub RingKind);
#[derive(Component)]
pub struct TooltipText;
#[derive(Component)]
pub struct InspectText;
#[derive(Component)]
pub struct MapLabel;
/// Tag for always-on overlays (the inspect panel) that should vanish behind the pause menu.
#[derive(Component)]
pub struct HideOnPause;

// ── Setup: the highlight rings and the screen panels ──────────────────────────────────────────

/// A flat hex border ring (white verts; the material tints it), to lay over a tile.
fn ring_mesh() -> Mesh {
    let mut b = MeshBuf::default();
    let at = |k: usize, r: f32| {
        let a = FRAC_PI_6 + FRAC_PI_3 * k as f32;
        Vec3::new(a.cos() * r, 0.0, a.sin() * r)
    };
    let (ro, ri) = (1.05, 0.84);
    for k in 0..6 {
        b.quad(
            at(k, ro),
            at(k + 1, ro),
            at(k + 1, ri),
            at(k, ri),
            [1.0, 1.0, 1.0],
        );
    }
    b.into_mesh()
}

pub fn setup_ui(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    f: &ThemeFonts,
) {
    // Highlight rings (unlit + two-sided via cull_mode None, so winding never hides them).
    let ring = meshes.add(ring_mesh());
    let mut ring_mat = |rgba: Color| {
        materials.add(StandardMaterial {
            base_color: rgba,
            unlit: true,
            cull_mode: None,
            ..default()
        })
    };
    commands.spawn((
        Ring(RingKind::Hover),
        Mesh3d(ring.clone()),
        MeshMaterial3d(ring_mat(Color::srgb(0.55, 0.9, 1.0))),
        Transform::IDENTITY,
        Visibility::Hidden,
    ));
    commands.spawn((
        Ring(RingKind::Select),
        Mesh3d(ring),
        MeshMaterial3d(ring_mat(Color::srgb(1.0, 0.82, 0.32))),
        Transform::IDENTITY,
        Visibility::Hidden,
    ));

    // A consistent themed panel node: bordered, rounded fog-ink (paired with `panel_chrome`).
    let panel = |w: f32| Node {
        position_type: PositionType::Absolute,
        padding: UiRect::all(Val::Px(theme::SP_SM)),
        border: UiRect::all(Val::Px(theme::BORDER_W)),
        border_radius: BorderRadius::all(Val::Px(theme::RADIUS)),
        max_width: Val::Px(w),
        ..default()
    };
    let text = |size: f32, color: Color| {
        (
            TextFont {
                font: f.mono.clone(),
                font_size: size,
                ..default()
            },
            TextColor(color),
        )
    };

    // Inspect panel — top-right of the centre view (clear of the right action tray).
    commands.spawn((
        InspectText,
        HideOnPause,
        Node {
            right: Val::Px(crate::hud::RIGHT_W + theme::SP_MD),
            top: Val::Px(crate::hud::TOP_H + theme::SP_MD),
            ..panel(270.0)
        },
        theme::panel_chrome(),
        Text::new(""),
        text(13.0, theme::TEXT),
    ));

    // Cursor tooltip — floats, moved each frame.
    commands.spawn((
        TooltipText,
        Node {
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            ..panel(240.0)
        },
        theme::panel_chrome(),
        Text::new(""),
        text(12.0, theme::TEXT),
        Visibility::Hidden,
    ));

    // Floating-label pool — reused across discovered settlements (serif place-names).
    for _ in 0..LABEL_POOL {
        commands.spawn((
            MapLabel,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                ..default()
            },
            Text::new(""),
            TextFont {
                font: f.serif.clone(),
                font_size: 14.0,
                ..default()
            },
            TextColor(theme::HEADING),
            Visibility::Hidden,
        ));
    }
}

// ── Picking and the click model ───────────────────────────────────────────────────────────────

/// The explored tile under the cursor — the one whose **top-centre projects nearest the cursor
/// on screen**. Comparing in screen space (rather than on the flat sea-level plane) makes the
/// pick correct for raised tiles at any camera angle, so what you click is what you get.
fn pick_tile(
    window: &Window,
    cam: &Camera,
    cam_tf: &GlobalTransform,
    sim: &Simulation,
) -> Option<Coord> {
    let cursor = window.cursor_position()?;
    let gw = sim.substrate();
    sim.player_explored()
        .into_iter()
        .filter_map(|c| {
            let w = tile_world(c.col, c.row);
            let world = Vec3::new(w.x, tile_top(gw, c), w.y);
            cam.world_to_viewport(cam_tf, world)
                .ok()
                .map(|p| (c, p.distance_squared(cursor)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(c, _)| c)
}

/// Hover-pick every frame; left-click selects a tile to inspect, right-click travels to it.
pub fn tile_interact(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cams: Query<(&Camera, &GlobalTransform), With<CamRig>>,
    mut game: NonSendMut<Game>,
) {
    if game.convo.is_some() || game.paused || game.talk_choices.is_some() || game.combat.is_some() {
        game.hovered = None;
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Ok((cam, cam_tf)) = cams.single() else {
        return;
    };
    // Only the centre (world) rectangle picks — a click over a tray must never move the avatar.
    match window.cursor_position() {
        Some(p) if crate::hud::in_center(window, p) => {}
        _ => {
            game.hovered = None;
            return;
        }
    }
    let hovered = pick_tile(window, cam, cam_tf, &game.sim);
    game.hovered = hovered;
    let Some(c) = hovered else { return };
    let g = &mut *game;
    if mouse.just_pressed(MouseButton::Left) {
        g.selected = Some(c);
    }
    if mouse.just_pressed(MouseButton::Right) {
        g.status = if g.sim.player_travel_to(c) {
            format!("Setting out for ({}, {}).", c.col, c.row)
        } else {
            "No path leads there on foot.".into()
        };
    }
}

// ── The overlays ─────────────────────────────────────────────────────────────────────────────

/// Lay the hover/select rings on their tiles (or hide them).
pub fn update_highlights(
    game: NonSend<Game>,
    mut q: Query<(&Ring, &mut Transform, &mut Visibility)>,
) {
    let gw = game.sim.substrate();
    for (ring, mut tf, mut vis) in &mut q {
        let coord = match ring.0 {
            RingKind::Hover => game.hovered,
            RingKind::Select => game.selected,
        };
        if let Some(c) = coord {
            let w = tile_world(c.col, c.row);
            tf.translation = Vec3::new(w.x, tile_top(gw, c) + 0.05, w.y);
            *vis = Visibility::Visible;
        } else {
            *vis = Visibility::Hidden;
        }
    }
}

/// Follow the cursor with a quick read of the hovered tile. The cursor is in *window* pixels but a
/// `Node`'s `left/top` are UI-logical (scaled by [`UiScale`]), so divide by the scale or the tooltip
/// drifts from the cursor as the UI is sized up.
pub fn update_tooltip(
    windows: Query<&Window>,
    ui_scale: Res<UiScale>,
    game: NonSend<Game>,
    mut q: Query<(&mut Node, &mut Text, &mut Visibility), With<TooltipText>>,
) {
    let Ok((mut node, mut text, mut vis)) = q.single_mut() else {
        return;
    };
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    let s = ui_scale.0.max(0.01);
    match (game.hovered, cursor) {
        (Some(c), Some(cur)) if game.convo.is_none() => {
            node.left = Val::Px(cur.x / s + 16.0);
            node.top = Val::Px(cur.y / s + 16.0);
            text.0 = tooltip_text(&game.sim, c);
            *vis = Visibility::Visible;
        }
        _ => {
            text.0.clear();
            *vis = Visibility::Hidden;
        }
    }
}

/// The detail panel for the selected tile (or a hint when nothing is selected).
pub fn update_inspect(mut game: NonSendMut<Game>, mut q: Query<&mut Text, With<InspectText>>) {
    let Ok(mut text) = q.single_mut() else { return };
    let sel = game.selected;
    text.0 = match sel {
        Some(c) => {
            let mut s = detail_text(&game.sim, c);
            // Who is here, and what they are about — a place reads as *alive* when you can see the
            // souls on it and what each is doing (the director's figures wear their arc here too).
            let souls = game.sim.souls_at(c);
            if !souls.is_empty() {
                s.push_str("\n\nhere now:\n");
                s.push_str(&souls.join("\n"));
            }
            s
        }
        None => "Left-click a tile to inspect it.\nRight-click to travel there.".into(),
    };
}

/// The discovered place-names to float over the map — accumulated as the fog lifts (never rescanned
/// each frame), so the per-frame work is only re-projecting the handful of labels onto the screen.
#[derive(Resource, Default)]
pub struct LabelCache(pub Vec<(Coord, String)>);

/// Project a name over each discovered settlement, nearest the avatar first, reusing the pool. The
/// list grows from this frame's freshly-revealed tiles; positions are converted from *window* pixels
/// to UI-logical (÷ [`UiScale`]) so the names stay pinned to their tile as the UI is sized.
pub fn update_labels(
    game: NonSend<Game>,
    ex: Res<crate::ground::Explored>,
    ui_scale: Res<UiScale>,
    mut cache: ResMut<LabelCache>,
    cams: Query<(&Camera, &GlobalTransform), With<CamRig>>,
    mut q: Query<(&mut Node, &mut Text, &mut Visibility), With<MapLabel>>,
) {
    // Grow the cache from newly-revealed tiles only (a discovered settlement shows when its tile
    // is uncovered — its `discovered` flag is set world-wide at gen).
    if !ex.fresh.is_empty() {
        let cat = game.sim.feature_catalog();
        for &c in &ex.fresh {
            for f in game.sim.features_at(c) {
                if f.discovered && cat.def(f.kind).category == Category::Community {
                    cache.0.push((c, pretty(&cat.def(f.kind).name)));
                }
            }
        }
    }

    let Ok((cam, cam_tf)) = cams.single() else {
        return;
    };
    let ap = tile_world(game.avatar_pos.col, game.avatar_pos.row);
    // Nearest-first so the closest places win the limited label pool.
    cache.0.sort_by(|a, b| {
        let da = (tile_world(a.0.col, a.0.row) - ap).length_squared();
        let db = (tile_world(b.0.col, b.0.row) - ap).length_squared();
        da.total_cmp(&db)
    });
    let gw = game.sim.substrate();
    let s = ui_scale.0.max(0.01);
    let mut iter = cache.0.iter();
    for (mut node, mut text, mut vis) in &mut q {
        loop {
            match iter.next() {
                Some((c, name)) => {
                    let w = tile_world(c.col, c.row);
                    let world = Vec3::new(w.x, tile_top(gw, *c) + 1.4, w.y);
                    if let Ok(p) = cam.world_to_viewport(cam_tf, world) {
                        node.left = Val::Px(p.x / s - 36.0);
                        node.top = Val::Px(p.y / s - 8.0);
                        text.0 = name.clone();
                        *vis = Visibility::Visible;
                        break;
                    }
                    // Behind the camera — skip this one, try the next.
                }
                None => {
                    *vis = Visibility::Hidden;
                    break;
                }
            }
        }
    }
}

/// One ledger line for a soul the avatar has met: its name, any arc honorific, and where its story
/// stands now ("Aldric, the Betrayed — still raw from a trusted friend's turning"); a soul who has
/// since died is remembered as gone. The avatar's fallible who's-who — never touches the sim.
fn ledger_line(sim: &Simulation, e: Entity) -> String {
    let name = sim.display_name(e);
    if !sim.npc_present(e) {
        return format!("{name} \u{2014} you knew them once; they are gone.");
    }
    let titled = match sim.npc_epithet(e) {
        Some(ep) => format!("{name}, {ep}"),
        None => name,
    };
    match sim.npc_situation(e) {
        Some(sit) => format!("{titled} \u{2014} {sit}"),
        None => titled,
    }
}

/// The discoveries journal — the Outer-Wilds ship-log: every place found (grouped), every lore fact
/// held, the **ledger** of souls the avatar has met (`met`), and the **charges** it has taken up
/// (`quests` active, `done` closed). Rendered into the pause menu's Journal tab (see `update_menu`).
pub fn journal_text(
    sim: &Simulation,
    met: &[Entity],
    quests: &[agents::Quest],
    done: &[String],
) -> String {
    let cat = sim.feature_catalog();
    // Discovered features on explored tiles, grouped by category.
    let mut groups: [Vec<String>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for c in sim.player_explored() {
        for f in sim.features_at(c) {
            if f.discovered {
                groups[cat.def(f.kind).category.idx()].push(pretty(&cat.def(f.kind).name));
            }
        }
    }
    for g in &mut groups {
        g.sort();
        g.dedup();
    }
    let total: usize = groups.iter().map(Vec::len).sum();
    let mut s = format!("— Journal —\n\nPlaces found: {total}\n");
    for (i, g) in groups.iter().enumerate() {
        if !g.is_empty() {
            s.push_str(&format!(
                "  {}: {}\n",
                ["Settlements", "Courts", "Ruins", "Wonders"][i],
                g.join(", ")
            ));
        }
    }
    let lore = sim.player_lore();
    s.push_str(&format!("\nLore known: {}\n", lore.len()));
    if lore.is_empty() {
        s.push_str(
            "  (you know nothing yet \u{2014} search ruins, ask the living, read the stones)\n",
        );
    } else {
        for l in &lore {
            s.push_str(&format!("  \u{2022} {}\n", pretty(l)));
        }
    }

    // The ledger — the souls the avatar has met, and where each one's story stands now.
    s.push_str(&format!("\nPeople known: {}\n", met.len()));
    if met.is_empty() {
        s.push_str("  (you have spoken with no one yet)\n");
    } else {
        for &e in met {
            s.push_str(&format!("  \u{2022} {}\n", ledger_line(sim, e)));
        }
    }

    // The charges taken — the director's drama as goals, each with a bearing to the soul to seek.
    s.push_str(&format!("\nCharges taken: {}\n", quests.len()));
    if quests.is_empty() {
        s.push_str("  (none \u{2014} take up a soul's charge in conversation)\n");
    } else {
        for q in quests {
            s.push_str(&format!(
                "  \u{2022} {} {}\n",
                q.objective,
                sim.quest_bearing(q)
            ));
        }
    }
    if !done.is_empty() {
        s.push_str(&format!("Charges closed: {}\n", done.len()));
        for d in done {
            s.push_str(&format!("  \u{00b7} {d}\n"));
        }
    }
    s
}

// ── Reading the sim into words ────────────────────────────────────────────────────────────────

fn tooltip_text(sim: &Simulation, c: Coord) -> String {
    let gw = sim.substrate();
    let sea = gw.params().sea_level;
    let terrain = Terrain::of(gw.elevation(c), sea);
    let mut s = format!("({}, {})  {}", c.col, c.row, terrain.name());
    let feats = discovered_features(sim, c);
    if !feats.is_empty() {
        s.push_str("\n");
        s.push_str(&feats.join("\n"));
    }
    s
}

fn detail_text(sim: &Simulation, c: Coord) -> String {
    let gw = sim.substrate();
    let sea = gw.params().sea_level;
    let elev = gw.elevation(c);
    let terrain = Terrain::of(elev, sea);
    let mut s = format!(
        "({}, {})\n{}   {:.0} m\nground: {}\nfertile {:.2}   water {:.2}",
        c.col,
        c.row,
        terrain.name(),
        (elev - sea).max(0.0),
        gw.biome(c).name(),
        gw.carrying_capacity(c).clamp(0.0, 1.0),
        gw.surface_water(c),
    );
    let feats = discovered_features(sim, c);
    if feats.is_empty() {
        s.push_str("\n\n(no landmarks here)");
    } else {
        s.push_str("\n\nhere:\n");
        s.push_str(&feats.join("\n"));
    }
    s
}

fn discovered_features(sim: &Simulation, c: Coord) -> Vec<String> {
    let cat = sim.feature_catalog();
    sim.features_at(c)
        .iter()
        .filter(|f| f.discovered)
        .map(|f| {
            format!(
                "  {} ({})",
                pretty(cat.name(f.kind)),
                category_word(cat.def(f.kind).category)
            )
        })
        .collect()
}

fn category_word(cat: Category) -> &'static str {
    match cat {
        Category::Community => "settlement",
        Category::Court => "court",
        Category::Ruin => "ruin",
        Category::Wilderness => "wonder",
    }
}

/// "mist_drowned_town" / "password_of_yao" → "Mist Drowned Town" / "Password Of Yao".
pub fn pretty(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut ch = w.chars();
            match ch.next() {
                Some(f) => f.to_uppercase().collect::<String>() + ch.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
