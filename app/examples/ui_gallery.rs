//! UI gallery + screenshot harness.
//!
//! `cargo run -p app --example ui_gallery` lays out every HUD surface with representative mock
//! data (no sim, no world-gen — boots instantly), styled through [`app::theme`], then captures
//! the window to a PNG and exits. This is the fast, eyes-on iteration surface for restyling the
//! HUD: change a token in `theme.rs`, re-run, look at the image, adjust.
//!
//! - Output path: `ACHLYDESA_SHOT` env var (default `target/ui_gallery.png`).
//! - Set `ACHLYDESA_HOLD=1` to keep the window open instead of exiting (to eyeball it live).

use app::theme::{self, ThemeFonts};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::text::Font;
use bevy::ui::{BorderColor, BorderRadius};
use bevy::window::WindowResolution;

fn shot_path() -> String {
    std::env::var("ACHLYDESA_SHOT").unwrap_or_else(|_| "target/ui_gallery.png".into())
}

#[derive(Resource, Default)]
struct Frames(u32);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Achlydesa — UI gallery".into(),
                resolution: WindowResolution::new(1380, 1340),
                ..default()
            }),
            ..default()
        }))
        // A fog-grey backdrop, so the panels read in something like their in-world context.
        .insert_resource(ClearColor(Color::srgb(0.105, 0.115, 0.135)))
        .init_resource::<Frames>()
        .add_systems(Startup, setup)
        .add_systems(Update, capture_and_exit)
        .run();
}

/// Capture on an early frame (once layout + glyph atlases have settled), then exit a few frames
/// later so the async GPU readback + file write completes. `ACHLYDESA_HOLD` keeps it open.
fn capture_and_exit(mut frames: ResMut<Frames>, mut commands: Commands, mut exit: MessageWriter<AppExit>) {
    frames.0 += 1;
    if frames.0 == 6 {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(shot_path()));
    }
    if frames.0 >= 30 && std::env::var("ACHLYDESA_HOLD").is_err() {
        exit.write(AppExit::Success);
    }
}

fn setup(mut commands: Commands, mut font_assets: ResMut<Assets<Font>>) {
    let f = ThemeFonts::embed(&mut font_assets);
    commands.spawn(Camera2d);
    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(theme::SP_XL)),
            row_gap: Val::Px(theme::SP_LG),
            ..default()
        },
        children![
            theme::display(&f, "Achlydesa — UI gallery"),
            theme::label(&f, "mock data · styled through app::theme · captured by the screenshot harness"),
            (
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(theme::SP_LG),
                    row_gap: Val::Px(theme::SP_LG),
                    align_items: AlignItems::FlexStart,
                    ..default()
                },
                children![
                    story(&f, "Legend", legend(&f)),
                    story(&f, "Inspect — settlement", inspect(&f)),
                    story(&f, "Cursor tooltip", tooltip(&f)),
                    story(&f, "Journal", journal(&f)),
                    story(&f, "Conversation", conversation(&f)),
                    story(&f, "Character sheet", sheet(&f)),
                    story(&f, "Status line", status(&f)),
                ],
            ),
            theme::heading(&f, "Menus & overlays"),
            (
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(theme::SP_LG),
                    row_gap: Val::Px(theme::SP_LG),
                    align_items: AlignItems::FlexStart,
                    ..default()
                },
                children![
                    story(&f, "Menu — Journal tab", menu_journal(&f)),
                    story(&f, "Menu — Map tab", menu_map(&f)),
                    story(&f, "Tooltip (hover hint)", rich_tooltip(&f)),
                ],
            ),
        ],
    ));
}

/// A labelled gallery card: a faint caption above the real widget.
fn story(f: &ThemeFonts, caption: &str, widget: impl Bundle) -> impl Bundle {
    (
        Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(theme::SP_SM), ..default() },
        children![theme::micro(f, caption.to_uppercase()), widget],
    )
}

// ── The surfaces (mock data) ────────────────────────────────────────────────────────────────────

fn legend(f: &ThemeFonts) -> impl Bundle {
    (
        Node { max_width: Val::Px(240.0), ..theme::panel_node() },
        theme::panel_chrome(),
        children![
            theme::heading(f, "The land"),
            theme::body(f, "rocks — heights & peaks\ntrees — woods & scrub\nhouses — a settlement\nkeep — a court\nbroken stones — a ruin"),
            theme::divider(),
            theme::heading(f, "Controls"),
            theme::label(f, "hover — look\nL-click — inspect\nR-click — travel\nSpace — wait   T — speak\nWASD — camera   scroll — zoom"),
        ],
    )
}

fn inspect(f: &ThemeFonts) -> impl Bundle {
    (
        Node { max_width: Val::Px(290.0), ..theme::panel_node() },
        theme::panel_chrome(),
        children![
            theme::heading(f, "Mist-Drowned Town"),
            theme::label(f, "(14, 9) · settlement"),
            theme::divider(),
            (
                Node { flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(theme::SP_XS), row_gap: Val::Px(theme::SP_XS), ..default() },
                children![chip(f, "marsh"), chip(f, "fertile 0.62"), chip(f, "water 0.81")],
            ),
            theme::body(f, "Leaning houses where the fog pools thickest. The wheel has turned over this place more times than its souls can count."),
            theme::divider(),
            theme::micro(f, "HERE"),
            theme::body(f, "• Pilgrim's Rest (court)\n• The Sunken Shrine (ruin)"),
        ],
    )
}

fn tooltip(f: &ThemeFonts) -> impl Bundle {
    (
        Node { max_width: Val::Px(220.0), ..theme::panel_node() },
        theme::panel_chrome(),
        children![theme::body(f, "(14, 9)  Marsh"), theme::label(f, "Mist-Drowned Town")],
    )
}

fn journal(f: &ThemeFonts) -> impl Bundle {
    (
        Node { max_width: Val::Px(350.0), ..theme::panel_node() },
        theme::panel_chrome(),
        children![
            theme::heading(f, "Journal"),
            theme::label(f, "Places found: 7"),
            theme::body(f, "Settlements — Mist-Drowned Town, Ashfall\nCourts — Pilgrim's Rest\nRuins — The Sunken Shrine"),
            theme::mono(f, "Wonders — The Crack in the Sky", theme::T_BODY, theme::AWE),
            theme::divider(),
            theme::label(f, "Lore known: 3"),
            theme::body(f, "• The seven Archons\n• The cup of forgetting\n• The password of Yao"),
            theme::micro(f, "(press J to close)"),
        ],
    )
}

fn conversation(f: &ThemeFonts) -> impl Bundle {
    (
        Node { width: Val::Px(450.0), ..theme::panel_node() },
        theme::panel_chrome(),
        children![
            (
                Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(theme::SP_SM), align_items: AlignItems::Baseline, ..default() },
                children![theme::serif(f, "Sophia", theme::T_TITLE, theme::DREAD), theme::label(f, "— wary of you")],
            ),
            theme::body(f, "\"You think I have forgotten the broken oath? The mist does not reach that far.\""),
            theme::divider(),
            theme::micro(f, "YOU MIGHT SAY"),
            option(f, "1", "Reconcile — let it lie"),
            option(f, "2", "Accuse — name the wrong"),
            option(f, "3", "Deflect — say nothing"),
        ],
    )
}

fn sheet(f: &ThemeFonts) -> impl Bundle {
    (
        Node { width: Val::Px(340.0), ..theme::panel_node() },
        theme::panel_chrome(),
        children![
            theme::heading(f, "Iao — the Wanderer"),
            theme::label(f, "level 2 · 9 HP · awakening (gnosis 0.3)"),
            theme::divider(),
            theme::micro(f, "ATTRIBUTES"),
            (
                Node { flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(theme::SP_SM), row_gap: Val::Px(theme::SP_SM), ..default() },
                children![stat(f, "STR", "10"), stat(f, "DEX", "13"), stat(f, "CON", "11"), stat(f, "INT", "12"), stat(f, "WIS", "14"), stat(f, "CHA", "9")],
            ),
            theme::divider(),
            theme::micro(f, "SKILLS"),
            theme::body(f, "Survive +2   Notice +1   Talk +2\nSneak +0   Heal +1   Magic —"),
        ],
    )
}

fn status(f: &ThemeFonts) -> impl Bundle {
    (
        Node {
            width: Val::Px(640.0),
            padding: UiRect::axes(Val::Px(theme::SP_MD), Val::Px(theme::SP_SM)),
            border: UiRect::all(Val::Px(theme::BORDER_W)),
            border_radius: BorderRadius::all(Val::Px(theme::RADIUS)),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            ..default()
        },
        theme::panel_chrome(),
        children![theme::body(f, "Welcome. Click a tile to set out — the world moves when you do.")],
    )
}

// ── Small composite widgets ─────────────────────────────────────────────────────────────────────

fn chip(f: &ThemeFonts, s: &str) -> impl Bundle {
    (theme::chip_node(), children![theme::micro(f, s.to_string())])
}

fn stat(f: &ThemeFonts, name: &str, val: &str) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(2.0),
            padding: UiRect::axes(Val::Px(theme::SP_SM), Val::Px(theme::SP_XS)),
            border: UiRect::all(Val::Px(theme::BORDER_W)),
            border_radius: BorderRadius::all(Val::Px(theme::RADIUS_SM)),
            min_width: Val::Px(46.0),
            ..default()
        },
        BackgroundColor(theme::INK_SUNKEN),
        BorderColor::all(theme::BORDER),
        children![theme::micro(f, name.to_string()), theme::mono(f, val.to_string(), theme::T_TITLE, theme::TEXT)],
    )
}

fn option(f: &ThemeFonts, key: &str, s: &str) -> impl Bundle {
    (
        Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(theme::SP_SM), align_items: AlignItems::Center, ..default() },
        children![chip(f, key), theme::body(f, s.to_string())],
    )
}

// ── Oblivion-style tabbed menu ────────────────────────────────────────────────────────────────

const MENU_TABS: [&str; 4] = ["Journal", "Character", "Map", "System"];

/// The menu shell: a (parchment-ready) panel with a serif tab bar across the top, the active tab's
/// content, and a footer. The parchment skin drops in later as a 9-sliced ImageNode over this base.
fn menu_with(f: &ThemeFonts, active: usize, content: impl Bundle) -> impl Bundle {
    (
        Node {
            width: Val::Px(580.0),
            padding: UiRect::all(Val::Px(theme::SP_LG)),
            border: UiRect::all(Val::Px(theme::BORDER_W)),
            border_radius: BorderRadius::all(Val::Px(theme::RADIUS)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(theme::SP_MD),
            ..default()
        },
        theme::panel_chrome(),
        children![
            tab_bar(f, active),
            theme::divider(),
            content,
            theme::divider(),
            theme::micro(f, "Q / E  page tabs        Enter  select        Esc  resume"),
        ],
    )
}

/// The row of serif tab labels; the active tab is lit and underlined with the warm accent.
fn tab_bar(f: &ThemeFonts, active: usize) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(theme::SP_XL),
            align_items: AlignItems::FlexEnd,
            ..default()
        },
        children![
            tab(f, MENU_TABS[0], active == 0),
            tab(f, MENU_TABS[1], active == 1),
            tab(f, MENU_TABS[2], active == 2),
            tab(f, MENU_TABS[3], active == 3),
        ],
    )
}

fn tab(f: &ThemeFonts, label: &str, active: bool) -> impl Bundle {
    let (color, bar) = if active { (theme::HEADING, theme::AWE) } else { (theme::TEXT_DIM, Color::NONE) };
    (
        Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(theme::SP_XS), ..default() },
        children![
            theme::serif(f, label.to_string(), theme::T_TITLE, color),
            (Node { width: Val::Percent(100.0), height: Val::Px(2.0), ..default() }, BackgroundColor(bar)),
        ],
    )
}

fn menu_journal(f: &ThemeFonts) -> impl Bundle {
    menu_with(
        f,
        0,
        (
            Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(theme::SP_SM), width: Val::Percent(100.0), ..default() },
            children![
                theme::heading(f, "Journal"),
                theme::label(f, "Places found: 7"),
                theme::body(f, "Settlements — Mist-Drowned Town, Ashfall\nCourts — Pilgrim's Rest\nRuins — The Sunken Shrine"),
                theme::mono(f, "Wonders — The Crack in the Sky", theme::T_BODY, theme::AWE),
                theme::label(f, "Lore known: 3"),
                theme::body(f, "• The seven Archons\n• The cup of forgetting\n• The password of Yao"),
            ],
        ),
    )
}

fn menu_map(f: &ThemeFonts) -> impl Bundle {
    menu_with(f, 2, mock_map(f))
}

/// The Map tab — discovered places plotted on a fog-darkened field (a mock of the real view).
fn mock_map(f: &ThemeFonts) -> impl Bundle {
    (
        Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(theme::SP_SM), width: Val::Percent(100.0), ..default() },
        children![
            theme::heading(f, "The Grey Country"),
            (
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(200.0),
                    border: UiRect::all(Val::Px(theme::BORDER_W)),
                    border_radius: BorderRadius::all(Val::Px(theme::RADIUS_SM)),
                    position_type: PositionType::Relative,
                    ..default()
                },
                BackgroundColor(theme::INK_SUNKEN),
                BorderColor::all(theme::BORDER),
                children![
                    place(f, "Mist-Drowned Town", 20.0, 56.0),
                    place(f, "Ashfall", 60.0, 26.0),
                    place(f, "Pilgrim's Rest", 42.0, 74.0),
                    here(f, 38.0, 48.0),
                ],
            ),
            theme::label(f, "7 places found · the fog hides the rest"),
        ],
    )
}

fn place(f: &ThemeFonts, name: &str, x: f32, y: f32) -> impl Bundle {
    (
        Node { position_type: PositionType::Absolute, left: Val::Percent(x), top: Val::Percent(y), flex_direction: FlexDirection::Row, column_gap: Val::Px(theme::SP_XS), align_items: AlignItems::Center, ..default() },
        children![
            (Node { width: Val::Px(7.0), height: Val::Px(7.0), border_radius: BorderRadius::all(Val::Px(4.0)), ..default() }, BackgroundColor(theme::AWE)),
            theme::serif(f, name.to_string(), theme::T_LABEL, theme::HEADING),
        ],
    )
}

fn here(f: &ThemeFonts, x: f32, y: f32) -> impl Bundle {
    (
        Node { position_type: PositionType::Absolute, left: Val::Percent(x), top: Val::Percent(y), flex_direction: FlexDirection::Row, column_gap: Val::Px(theme::SP_XS), align_items: AlignItems::Center, ..default() },
        children![
            (Node { width: Val::Px(9.0), height: Val::Px(9.0), border: UiRect::all(Val::Px(2.0)), border_radius: BorderRadius::all(Val::Px(5.0)), ..default() }, BackgroundColor(theme::DREAD), BorderColor::all(theme::HEADING)),
            theme::serif(f, "you", theme::T_LABEL, theme::DREAD),
        ],
    )
}

/// A richer hover tooltip: a serif title, a line of body, and the key hint.
fn rich_tooltip(f: &ThemeFonts) -> impl Bundle {
    (
        Node { max_width: Val::Px(260.0), ..theme::panel_node() },
        theme::panel_chrome(),
        children![
            theme::serif(f, "Search", theme::T_BODY + 3.0, theme::HEADING),
            theme::body(f, "Comb the ruins and the stones underfoot for what the fog has hidden. Costs a turn."),
            theme::micro(f, "press F"),
        ],
    )
}
