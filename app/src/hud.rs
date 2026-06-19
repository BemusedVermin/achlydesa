//! **The framed exploration HUD.** A console-RPG frame around the 3D view: an opaque
//! `grassy_rock` border on all four edges with the world showing through the centre. The four
//! trays are
//!
//! * **left** — the party as circular sigil portraits (the avatar gold-ringed),
//! * **top** — the menu tabs (click to open the parchment menu at that tab),
//! * **right** — context-sensitive **tile-action** buttons (greyed when not valid),
//! * **bottom** — the survival vitals (hydration/warmth/stamina) and a one-line status, with an
//!   always-on circular **minimap** tucked into the corner.
//!
//! Everything is authored in *reference pixels* (against [`REF_W`]×[`REF_H`]) and scaled as one by
//! a global [`UiScale`] driven from the window size ([`scale_ui`]), so the whole frame keeps the
//! same proportions at any resolution. Because tile picking reads the raw cursor (not Bevy's UI
//! picking), [`in_center`] — computed from that *same* scale — gates the world click model to the
//! centre rectangle, so a click on a tray never also moves the avatar.

use app::theme::{self, ThemeFonts};
use bevy::prelude::*;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{BorderRadius, GlobalZIndex, Overflow, UiScale};

use crate::Game;
use agents::FindState;
use crate::minimap;

// ── Frame geometry (reference pixels, against REF_W×REF_H) ───────────────────────────────────────

/// The reference window the layout is authored against; [`scale_ui`] scales everything from here.
pub const REF_W: f32 = 1440.0;
pub const REF_H: f32 = 900.0;
/// Tray thicknesses.
pub const LEFT_W: f32 = 156.0;
pub const RIGHT_W: f32 = 156.0;
pub const TOP_H: f32 = 54.0;
pub const BOTTOM_H: f32 = 116.0;
/// A portrait disc and the corner minimap, in reference pixels.
const PORTRAIT_D: f32 = 104.0;
const MINIMAP_D: f32 = 168.0;
/// Portrait slots: the avatar plus up to four companions.
const PORTRAIT_SLOTS: usize = 5;

// ── Frame palette ────────────────────────────────────────────────────────────────────────────────

/// A carved, near-black edge around each tray.
const FRAME_EDGE: Color = Color::srgb(0.10, 0.11, 0.09);
/// Multiplies the grassy photo, dimming it so light text reads on top while the rock still shows.
const TRAY_TINT: Color = Color::srgb(0.56, 0.58, 0.52);
/// Bright text for labels sitting directly on the tray.
const TRAY_TEXT: Color = Color::srgb(0.93, 0.95, 0.90);
/// Action-button fills: idle, hovered, and disabled.
const BTN_BG: Color = Color::srgba(0.06, 0.07, 0.10, 0.82);
const BTN_BG_HOVER: Color = Color::srgba(0.15, 0.17, 0.23, 0.92);
const BTN_BG_DISABLED: Color = Color::srgba(0.05, 0.06, 0.08, 0.5);

// ── Components ───────────────────────────────────────────────────────────────────────────────────

#[derive(Component)]
pub struct Tray;
#[derive(Component)]
pub struct PortraitSlot(pub usize);
#[derive(Component)]
pub struct PortraitInitial(pub usize);
#[derive(Component)]
pub struct TopTab(pub usize);
#[derive(Component)]
pub struct HudMinimap;
#[derive(Component)]
pub struct VitalFill(pub Vital);
#[derive(Component)]
pub struct ActionButton(pub ActionKind);
#[derive(Component)]
pub struct ActionLabel(pub ActionKind);

/// The three party-scoped survival vitals shown as bars.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Vital {
    Hydration,
    Warmth,
    Stamina,
}
const VITALS: [Vital; 3] = [Vital::Hydration, Vital::Warmth, Vital::Stamina];
impl Vital {
    fn label(self) -> &'static str {
        match self {
            Vital::Hydration => "Hydration",
            Vital::Warmth => "Warmth",
            Vital::Stamina => "Stamina",
        }
    }
    fn color(self) -> Color {
        match self {
            Vital::Hydration => Color::srgb(0.36, 0.66, 0.86),
            Vital::Warmth => Color::srgb(0.88, 0.62, 0.30),
            Vital::Stamina => Color::srgb(0.50, 0.74, 0.42),
        }
    }
}

/// The context-sensitive tile actions, in tray order. Each mirrors a keyboard verb and is enabled
/// only when valid for the current tile / nearby soul (see [`enabled`]).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Inspect,
    Travel,
    Search,
    Use,
    Wait,
    Talk,
    Recruit,
}
const ACTIONS: [ActionKind; 7] = [
    ActionKind::Inspect,
    ActionKind::Travel,
    ActionKind::Search,
    ActionKind::Use,
    ActionKind::Wait,
    ActionKind::Talk,
    ActionKind::Recruit,
];
impl ActionKind {
    fn label(self) -> &'static str {
        match self {
            ActionKind::Inspect => "Inspect",
            ActionKind::Travel => "Travel",
            ActionKind::Search => "Search",
            ActionKind::Use => "Use",
            ActionKind::Wait => "Wait",
            ActionKind::Talk => "Talk",
            ActionKind::Recruit => "Recruit",
        }
    }
}

// ── Scaling: one UiScale for the whole frame, and the matching pick-gate ─────────────────────────

/// The uniform scale for the frame at this window size — the largest that keeps the reference
/// layout fitting both axes, so every tray/font/bar grows at the same ratio.
pub fn ui_scale_for(window: &Window) -> f32 {
    (window.width() / REF_W).min(window.height() / REF_H).max(0.1)
}

/// Drive the global [`UiScale`] from the window so the frame scales as one (only on real change,
/// to avoid needless relayouts).
pub fn scale_ui(windows: Query<&Window>, mut ui_scale: ResMut<UiScale>) {
    let Ok(window) = windows.single() else { return };
    let s = ui_scale_for(window);
    if (ui_scale.0 - s).abs() > 1e-3 {
        ui_scale.0 = s;
    }
}

/// Is a cursor point inside the centre (world) rectangle — i.e. not over a tray? Computed from the
/// same scale [`scale_ui`] applies, since a `Px` value occupies `px * ui_scale` in cursor space.
pub fn in_center(window: &Window, p: Vec2) -> bool {
    let s = ui_scale_for(window);
    p.x >= LEFT_W * s && p.x <= window.width() - RIGHT_W * s && p.y >= TOP_H * s && p.y <= window.height() - BOTTOM_H * s
}

// ── Spawn the frame ──────────────────────────────────────────────────────────────────────────────

fn tray_bg(grassy: &Handle<Image>) -> (ImageNode, BorderColor) {
    (
        ImageNode { image: grassy.clone(), image_mode: NodeImageMode::Stretch, color: TRAY_TINT, ..default() },
        BorderColor::all(FRAME_EDGE),
    )
}

/// Hash a character's archetype+name into a muted, mid-dark portrait tint — deterministic, so the
/// same soul always wears the same colour. Shared with the conversation panel's speaker sigil.
pub fn archetype_tint(seed: &str) -> Color {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Color::hsl((h % 360) as f32, 0.34, 0.40)
}

pub fn spawn(commands: &mut Commands, f: &ThemeFonts, grassy: Handle<Image>) {
    let px = Val::Px;

    // ── LEFT: portrait column ──
    commands
        .spawn((
            Tray,
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                top: px(0.0),
                width: px(LEFT_W),
                height: Val::Percent(100.0),
                border: UiRect::right(px(2.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                padding: UiRect::axes(px(theme::SP_SM), px(TOP_H + theme::SP_MD)),
                row_gap: px(theme::SP_MD),
                ..default()
            },
            tray_bg(&grassy),
        ))
        .with_children(|t| {
            for i in 0..PORTRAIT_SLOTS {
                t.spawn((
                    PortraitSlot(i),
                    Button,
                    Node {
                        width: px(PORTRAIT_D),
                        height: px(PORTRAIT_D),
                        border: UiRect::all(px(3.0)),
                        border_radius: BorderRadius::all(px(PORTRAIT_D / 2.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.20, 0.22, 0.25)),
                    BorderColor::all(theme::BORDER),
                    Visibility::Hidden,
                ))
                .with_children(|d| {
                    d.spawn((
                        PortraitInitial(i),
                        Text::new(""),
                        TextFont { font: f.serif.clone(), font_size: PORTRAIT_D * 0.34, ..default() },
                        TextColor(theme::HEADING),
                    ));
                });
            }
        });

    // ── TOP: menu tabs ──
    commands
        .spawn((
            Tray,
            Node {
                position_type: PositionType::Absolute,
                left: px(LEFT_W),
                right: px(RIGHT_W),
                top: px(0.0),
                height: px(TOP_H),
                border: UiRect::bottom(px(2.0)),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: px(theme::SP_XL),
                ..default()
            },
            tray_bg(&grassy),
        ))
        .with_children(|bar| {
            for (i, label) in crate::MENU_TABS.iter().enumerate() {
                bar.spawn((
                    TopTab(i),
                    Button,
                    Node {
                        padding: UiRect::axes(px(theme::SP_MD), px(theme::SP_XS)),
                        border_radius: BorderRadius::all(px(theme::RADIUS_SM)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|b| {
                    b.spawn((
                        Text::new(*label),
                        TextFont { font: f.serif.clone(), font_size: theme::T_TITLE, ..default() },
                        TextColor(TRAY_TEXT),
                    ));
                });
            }
        });

    // ── RIGHT: tile-action buttons ──
    commands
        .spawn((
            Tray,
            Node {
                position_type: PositionType::Absolute,
                right: px(0.0),
                top: px(0.0),
                width: px(RIGHT_W),
                height: Val::Percent(100.0),
                border: UiRect::left(px(2.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                padding: UiRect::axes(px(theme::SP_SM), px(TOP_H + theme::SP_MD)),
                row_gap: px(theme::SP_SM),
                ..default()
            },
            tray_bg(&grassy),
        ))
        .with_children(|t| {
            for a in ACTIONS {
                t.spawn((
                    ActionButton(a),
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::all(px(theme::SP_SM)),
                        border: UiRect::all(px(theme::BORDER_W)),
                        border_radius: BorderRadius::all(px(theme::RADIUS_SM)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(BTN_BG),
                    BorderColor::all(theme::BORDER),
                ))
                .with_children(|b| {
                    b.spawn((
                        ActionLabel(a),
                        Text::new(a.label()),
                        TextFont { font: f.mono.clone(), font_size: theme::T_BODY, ..default() },
                        TextColor(TRAY_TEXT),
                    ));
                });
            }
        });

    // ── BOTTOM: vitals + status ──
    commands
        .spawn((
            Tray,
            Node {
                position_type: PositionType::Absolute,
                left: px(LEFT_W),
                right: px(RIGHT_W),
                bottom: px(0.0),
                height: px(BOTTOM_H),
                border: UiRect::top(px(2.0)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                padding: UiRect::axes(px(theme::SP_LG), px(theme::SP_SM)),
                column_gap: px(theme::SP_XL),
                ..default()
            },
            tray_bg(&grassy),
        ))
        .with_children(|t| {
            // Vitals column.
            t.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(theme::SP_XS), width: px(300.0), ..default() })
                .with_children(|col| {
                    for v in VITALS {
                        col.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: px(theme::SP_SM),
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn(Node { width: px(78.0), ..default() }).with_children(|lbl| {
                                lbl.spawn((
                                    Text::new(v.label()),
                                    TextFont { font: f.mono.clone(), font_size: theme::T_MICRO, ..default() },
                                    TextColor(TRAY_TEXT),
                                ));
                            });
                            row.spawn((
                                Node {
                                    width: px(190.0),
                                    height: px(12.0),
                                    border_radius: BorderRadius::all(px(6.0)),
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.03, 0.04, 0.06, 0.85)),
                            ))
                            .with_children(|track| {
                                track.spawn((
                                    VitalFill(v),
                                    Node {
                                        width: Val::Percent(100.0),
                                        height: Val::Percent(100.0),
                                        border_radius: BorderRadius::all(px(6.0)),
                                        ..default()
                                    },
                                    BackgroundColor(v.color()),
                                ));
                            });
                        });
                    }
                });
            // One-line status (the HUD's Help kind) on the tray.
            t.spawn((
                crate::HudKind::Help,
                Node { flex_grow: 1.0, max_width: px(560.0), ..default() },
                Text::new(""),
                TextFont { font: f.mono.clone(), font_size: theme::T_LABEL, ..default() },
                TextColor(theme::TEXT_DIM),
            ));
        });

    // ── Circular minimap, tucked into the bottom-right corner (above the trays). ──
    commands.spawn((
        HudMinimap,
        ImageNode { image: Handle::default(), image_mode: NodeImageMode::Stretch, ..default() },
        Node {
            position_type: PositionType::Absolute,
            right: px(theme::SP_SM),
            bottom: px(theme::SP_SM),
            width: px(MINIMAP_D),
            height: px(MINIMAP_D),
            border: UiRect::all(px(3.0)),
            border_radius: BorderRadius::all(px(MINIMAP_D / 2.0)),
            ..default()
        },
        BorderColor::all(theme::AWE),
        GlobalZIndex(5),
    ));

    // ── Centre overlay: the tile read-out (top-left). The conversation has its own panel. ──
    let panel = |w: f32| Node {
        position_type: PositionType::Absolute,
        padding: UiRect::all(px(theme::SP_SM)),
        border: UiRect::all(px(theme::BORDER_W)),
        border_radius: BorderRadius::all(px(theme::RADIUS)),
        max_width: px(w),
        ..default()
    };
    commands.spawn((
        crate::HudKind::Look,
        Node { left: px(LEFT_W + theme::SP_MD), top: px(TOP_H + theme::SP_MD), ..panel(360.0) },
        theme::panel_chrome(),
        Text::new(""),
        TextFont { font: f.mono.clone(), font_size: theme::T_BODY, ..default() },
        TextColor(theme::TEXT),
    ));
}

// ── Update systems ───────────────────────────────────────────────────────────────────────────────

/// Fill the portrait discs from the avatar + party roster: a muted archetype tint, the soul's
/// initial, and a gold ring on the avatar. Unused slots hide.
pub fn update_portraits(
    game: NonSend<Game>,
    mut discs: Query<(&PortraitSlot, &Interaction, &mut BackgroundColor, &mut BorderColor, &mut Visibility)>,
    mut inits: Query<(&PortraitInitial, &mut Text)>,
) {
    let avatar = game.sim.player_avatar();
    let mut members: Vec<Entity> = Vec::new();
    if let Some(a) = avatar {
        members.push(a);
    }
    members.extend(game.sim.party_roster());

    let data: Vec<Option<(String, Color, bool)>> = (0..PORTRAIT_SLOTS)
        .map(|i| {
            members.get(i).map(|&e| {
                let name = game.sim.display_name(e);
                let init = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
                let seed = format!("{}/{}", game.sim.archetype_of(e).unwrap_or(""), name);
                (init, archetype_tint(&seed), Some(e) == avatar)
            })
        })
        .collect();

    for (slot, interaction, mut bg, mut border, mut vis) in &mut discs {
        match &data[slot.0] {
            Some((_, tint, is_av)) => {
                *vis = Visibility::Inherited;
                bg.0 = *tint;
                // Gold ring on the avatar; companions' rings brighten on hover to read as clickable.
                let hot = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
                let ring = if *is_av { theme::AWE } else if hot { theme::HEADING } else { theme::BORDER };
                *border = BorderColor::all(ring);
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for (init, mut text) in &mut inits {
        text.0 = data[init.0].as_ref().map(|(s, _, _)| s.clone()).unwrap_or_default();
    }
}

/// Drive each vital bar's fill width from the avatar's live vitals (0–100 → 0–100%).
pub fn update_vitals(game: NonSend<Game>, mut q: Query<(&VitalFill, &mut Node)>) {
    let vit = game.sim.player_avatar().and_then(|a| game.sim.vitals_of(a));
    for (vf, mut node) in &mut q {
        let pct = match (vit, vf.0) {
            (Some(v), Vital::Hydration) => v.thirst,
            (Some(v), Vital::Warmth) => v.warmth,
            (Some(v), Vital::Stamina) => v.stamina,
            (None, _) => 0.0,
        }
        .clamp(0.0, 100.0);
        node.width = Val::Percent(pct);
    }
}

/// Everything an action needs to decide whether it is valid right now — gathered once a frame. While
/// a conversation is open the panel owns input, so `in_convo` disables the whole tray.
struct ActionCtx {
    traveling: bool,
    has_sel: bool,
    findable: bool,
    soul_near: bool,
    can_use: bool,
    in_convo: bool,
}

fn action_ctx(g: &mut Game) -> ActionCtx {
    let pos = g.sim.player_position();
    ActionCtx {
        traveling: g.sim.player_traveling(),
        has_sel: g.selected.is_some() && g.selected != pos,
        findable: matches!(g.sim.player_find_state(), FindState::Findable),
        soul_near: !g.sim.player_nearby_npcs().is_empty(),
        // Is there an affordance the avatar can engage where it stands (a POI to *use*)?
        can_use: !g.sim.affordances_here().is_empty(),
        // A conversation or its who-to-talk-to chooser is modal over the tray.
        in_convo: g.convo.is_some() || g.talk_choices.is_some(),
    }
}

fn enabled(c: &ActionCtx, a: ActionKind) -> bool {
    if c.in_convo {
        return false; // the conversation panel is modal over the action tray
    }
    match a {
        ActionKind::Inspect => true,
        ActionKind::Travel => c.has_sel && !c.traveling,
        ActionKind::Search => !c.traveling && c.findable,
        // Use engages a smart-object (rest, water, forage, a craft) where the avatar stands.
        ActionKind::Use => c.can_use && !c.traveling,
        ActionKind::Wait => !c.traveling,
        // Talk opens a conversation even without the voice model — the speak choices are
        // deterministic; only the free-text line needs the model.
        ActionKind::Talk => c.soul_near && !c.traveling,
        ActionKind::Recruit => c.soul_near && !c.traveling,
    }
}

/// Recolour the action buttons each frame: dim when invalid, lit on hover, and grey their labels.
pub fn update_action_buttons(
    mut game: NonSendMut<Game>,
    mut buttons: Query<(&ActionButton, &Interaction, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<(&ActionLabel, &mut TextColor)>,
) {
    let ctx = action_ctx(&mut game);
    for (b, interaction, mut bg, mut border) in &mut buttons {
        let on = enabled(&ctx, b.0);
        bg.0 = if !on {
            BTN_BG_DISABLED
        } else if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            BTN_BG_HOVER
        } else {
            BTN_BG
        };
        *border = BorderColor::all(if on { theme::BORDER } else { Color::srgba(0.20, 0.22, 0.26, 0.4) });
    }
    for (l, mut col) in &mut labels {
        col.0 = if enabled(&ctx, l.0) { TRAY_TEXT } else { theme::TEXT_FAINT };
    }
}

/// Invoke a tile action when its button is pressed (ignoring disabled actions, and never while the
/// pause menu owns the screen). Mirrors the keyboard verbs via the shared `crate::do_*` helpers.
pub fn action_button_click(mut game: NonSendMut<Game>, q: Query<(&ActionButton, &Interaction), Changed<Interaction>>) {
    if game.paused || game.convo.is_some() || game.talk_choices.is_some() {
        return;
    }
    let Some(a) = q.iter().find(|(_, i)| **i == Interaction::Pressed).map(|(b, _)| b.0) else {
        return;
    };
    let ctx = action_ctx(&mut game);
    if !enabled(&ctx, a) {
        return;
    }
    let g = &mut *game;
    match a {
        ActionKind::Inspect => {
            let pos = g.sim.player_position();
            g.selected = g.hovered.or(g.selected).or(pos);
            g.status = "You take the measure of the tile.".into();
        }
        ActionKind::Travel => {
            if let Some(c) = g.selected {
                g.status = if g.sim.player_travel_to(c) {
                    format!("Setting out for ({}, {}).", c.col, c.row)
                } else {
                    "No path leads there on foot.".into()
                };
            }
        }
        ActionKind::Search => crate::do_search(g),
        ActionKind::Use => crate::do_use(g),
        ActionKind::Wait => crate::do_wait(g),
        ActionKind::Talk => crate::start_talk(g),
        ActionKind::Recruit => crate::do_recruit(g),
    }
}

/// Clicking a top-tab opens the parchment menu at that tab. The Character tab always opens on the
/// avatar (clearing any companion the portraits had focused).
pub fn top_tab_click(mut game: NonSendMut<Game>, q: Query<(&TopTab, &Interaction), Changed<Interaction>>) {
    if let Some(i) = q.iter().find(|(_, it)| **it == Interaction::Pressed).map(|(t, _)| t.0) {
        if i == 1 {
            game.sheet_subject = None;
        }
        game.paused = true;
        game.menu_tab = i;
    }
}

/// Clicking a portrait opens the Character tab on that soul (the avatar or a companion).
pub fn portrait_click(mut game: NonSendMut<Game>, q: Query<(&PortraitSlot, &Interaction), Changed<Interaction>>) {
    let Some(slot) = q.iter().find(|(_, i)| **i == Interaction::Pressed).map(|(s, _)| s.0) else {
        return;
    };
    let mut members: Vec<Entity> = Vec::new();
    if let Some(a) = game.sim.player_avatar() {
        members.push(a);
    }
    members.extend(game.sim.party_roster());
    if let Some(&e) = members.get(slot) {
        game.sheet_subject = Some(e);
        game.menu_tab = 1; // Character
        game.paused = true;
    }
}

/// The HUD minimap's zoom (world units per texture pixel) — a tight local window so it reads as
/// the ground around you, not the whole continent.
const HUD_WPP: f32 = 0.5;

/// Re-render the always-on corner minimap when new ground is uncovered or the avatar moves. The
/// window is centred on the avatar, so the minimap tracks the player.
pub fn update_hud_minimap(mut game: NonSendMut<Game>, mut images: ResMut<Assets<Image>>, mut q: Query<&mut ImageNode, With<HudMinimap>>) {
    let count = game.sim.player_explored_count();
    let avatar = game.avatar_pos;
    if count == game.last_hud_explored && avatar == game.last_hud_avatar {
        return;
    }
    let center = crate::layout::tile_world(avatar.col, avatar.row);
    let img = minimap::render(&game.sim, center, HUD_WPP, avatar, MINIMAP_D as u32, MINIMAP_D as u32);
    let handle = images.add(img);
    if let Ok(mut node) = q.single_mut() {
        node.image = handle;
    }
    game.last_hud_explored = count;
    game.last_hud_avatar = avatar;
}
