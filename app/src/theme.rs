//! UI design tokens, fonts, and widget chrome for achlydesa's HUD.
//!
//! One place for the palette, fonts, spacing scale, type ramp, and panel chrome — so the look
//! is tuned from a handful of constants instead of values scattered through `ui.rs`. The mood
//! is the dream-purgatory's: muted fog-ink panels, **literary-serif headings over austere
//! monospace body** (a fallen manuscript kept in a clerk's typewriter), and two rare accents —
//! a warm [`AWE`] and a cold [`DREAD`] — held back for the moments that earn them.
//!
//! Shared by the live HUD and the `ui_gallery` example (the screenshot harness), so what the
//! gallery shows is what the game renders. Load [`ThemeFonts`] once at startup, then build a
//! panel as `(Node { ..theme::panel_node() }, theme::panel_chrome())` and fill it with
//! `theme::heading(&fonts, ..)`, `theme::body(&fonts, ..)`, etc.

use bevy::prelude::*;
use bevy::text::Font;
use bevy::ui::{BorderRadius, BoxShadow};

// ── Palette ────────────────────────────────────────────────────────────────────────────────

/// Panel fill — deep, desaturated fog-ink. Translucent, so the world reads behind it.
pub const INK: Color = Color::srgba(0.050, 0.062, 0.090, 0.88);
/// A raised surface (header strips, chips, selected rows) — a step lighter than [`INK`].
pub const INK_RAISED: Color = Color::srgba(0.094, 0.107, 0.150, 0.94);
/// A recessed well (stat cells, input fields) — a step darker than [`INK`].
pub const INK_SUNKEN: Color = Color::srgba(0.028, 0.036, 0.057, 0.92);
/// Hairline borders and dividers — cool grey, low contrast on purpose.
pub const BORDER: Color = Color::srgba(0.38, 0.43, 0.52, 0.55);

/// Primary text — parchment-pale, easy on fog.
pub const TEXT: Color = Color::srgb(0.86, 0.88, 0.92);
/// Secondary text — labels, captions, the quieter half of a line.
pub const TEXT_DIM: Color = Color::srgb(0.60, 0.65, 0.73);
/// Tertiary text — hints, placeholders, the barely-there.
pub const TEXT_FAINT: Color = Color::srgb(0.44, 0.49, 0.57);
/// Headings — a touch warmer than body, so titles feel lit rather than loud.
pub const HEADING: Color = Color::srgb(0.91, 0.87, 0.79);

/// The warm accent — *awe*. Gilded, held back for the transcendent (a wonder found, a
/// climax, the rare good thing). Use sparingly; its scarcity is its power.
pub const AWE: Color = Color::srgb(0.93, 0.79, 0.46);
/// The cold accent — *dread*. A pale, sickly teal for warnings, the Archons, the wrong.
pub const DREAD: Color = Color::srgb(0.47, 0.75, 0.79);
/// Danger / grievance — a muted, bloodless crimson for harm and feud.
pub const BLOOD: Color = Color::srgb(0.81, 0.38, 0.36);

// ── Spacing scale (px) ───────────────────────────────────────────────────────────────────────
pub const SP_XS: f32 = 4.0;
pub const SP_SM: f32 = 8.0;
pub const SP_MD: f32 = 12.0;
pub const SP_LG: f32 = 18.0;
pub const SP_XL: f32 = 28.0;

// ── Type ramp (px) ───────────────────────────────────────────────────────────────────────────
// Serif headings read well a little larger; the mono body sits a touch smaller so the wider
// monospace advance still fits dense panels.
pub const T_DISPLAY: f32 = 24.0;
pub const T_TITLE: f32 = 17.0;
pub const T_BODY: f32 = 13.0;
pub const T_LABEL: f32 = 12.0;
pub const T_MICRO: f32 = 11.0;

// ── Geometry ──────────────────────────────────────────────────────────────────────────────────
pub const RADIUS: f32 = 6.0;
pub const RADIUS_SM: f32 = 4.0;
pub const BORDER_W: f32 = 1.0;

// ── Fonts ──────────────────────────────────────────────────────────────────────────────────────

/// The two bundled OFL faces, as asset handles: a literary **serif** (EB Garamond) for
/// headings and a clerical **monospace** (Courier Prime) for body copy. Load once at startup
/// with [`ThemeFonts::embed`] and keep as a resource; the text builders clone the right handle.
#[derive(Resource, Clone)]
pub struct ThemeFonts {
    pub serif: Handle<Font>,
    pub mono: Handle<Font>,
}

impl ThemeFonts {
    /// Embed the bundled fonts at compile time and register them as [`Font`] assets. Call once
    /// from a startup system with `ResMut<Assets<Font>>`. Embedding (rather than
    /// `AssetServer::load`) keeps the HUD path-independent and matches the project's
    /// bake-content-in convention (cf. `Bundled`).
    pub fn embed(fonts: &mut Assets<Font>) -> Self {
        let serif = Font::try_from_bytes(include_bytes!("../../assets/fonts/EBGaramond-SemiBold.ttf").to_vec())
            .expect("bundled EB Garamond is a valid font");
        let mono = Font::try_from_bytes(include_bytes!("../../assets/fonts/CourierPrime-Regular.ttf").to_vec())
            .expect("bundled Courier Prime is a valid font");
        Self { serif: fonts.add(serif), mono: fonts.add(mono) }
    }
}

// ── Chrome ──────────────────────────────────────────────────────────────────────────────────────

/// The layout half of a floating panel: a column with comfortable padding, a hairline border,
/// rounded corners, and a small gap between children. Spread it and set position:
/// `Node { position_type: PositionType::Absolute, left: .., top: .., ..theme::panel_node() }`.
pub fn panel_node() -> Node {
    Node {
        padding: UiRect::all(Val::Px(SP_MD)),
        border: UiRect::all(Val::Px(BORDER_W)),
        border_radius: BorderRadius::all(Val::Px(RADIUS)),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(SP_SM),
        ..default()
    }
}

/// The paint half of a floating panel: fog-ink fill, bordered, with a soft drop shadow so it
/// lifts off the world. Pair with [`panel_node`]: `(Node { ..panel_node() }, panel_chrome())`.
pub fn panel_chrome() -> impl Bundle {
    (
        BackgroundColor(INK),
        BorderColor::all(BORDER),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.45), Val::Px(0.0), Val::Px(4.0), Val::Px(0.0), Val::Px(14.0)),
    )
}

/// A small rounded tag — the container; add a [`micro`]/[`label`] text child for the content.
pub fn chip_node() -> impl Bundle {
    (
        Node {
            padding: UiRect::axes(Val::Px(SP_SM), Val::Px(2.0)),
            border: UiRect::all(Val::Px(BORDER_W)),
            border_radius: BorderRadius::all(Val::Px(RADIUS_SM)),
            ..default()
        },
        BackgroundColor(INK_RAISED),
        BorderColor::all(BORDER),
    )
}

/// A thin horizontal divider for separating sections inside a panel.
pub fn divider() -> impl Bundle {
    (
        Node { width: Val::Percent(100.0), height: Val::Px(1.0), margin: UiRect::axes(Val::Px(0.0), Val::Px(SP_XS)), ..default() },
        BackgroundColor(BORDER),
    )
}

// ── Text builders ─────────────────────────────────────────────────────────────────────────────
// Two low-level faces (`serif`, `mono`) + ramp presets. Headings use the serif; everything in
// the body uses the monospace. Special cases call `serif`/`mono` directly with a size + accent.

/// A serif run (EB Garamond) at an explicit size + colour — for headings and titled accents.
pub fn serif(f: &ThemeFonts, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont { font: f.serif.clone(), font_size: size, ..default() }, TextColor(color))
}

/// A monospace run (Courier Prime) at an explicit size + colour — for body, labels, accents.
pub fn mono(f: &ThemeFonts, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont { font: f.mono.clone(), font_size: size, ..default() }, TextColor(color))
}

/// A page/title display line — serif, largest, warm.
pub fn display(f: &ThemeFonts, s: impl Into<String>) -> impl Bundle {
    serif(f, s, T_DISPLAY, HEADING)
}
/// A panel/section heading — serif.
pub fn heading(f: &ThemeFonts, s: impl Into<String>) -> impl Bundle {
    serif(f, s, T_TITLE, HEADING)
}
/// Body copy — monospace, the default reading text.
pub fn body(f: &ThemeFonts, s: impl Into<String>) -> impl Bundle {
    mono(f, s, T_BODY, TEXT)
}
/// A dimmed label / caption — monospace.
pub fn label(f: &ThemeFonts, s: impl Into<String>) -> impl Bundle {
    mono(f, s, T_LABEL, TEXT_DIM)
}
/// The faintest tier — overlines, hints, "(press J to close)" — monospace.
pub fn micro(f: &ThemeFonts, s: impl Into<String>) -> impl Bundle {
    mono(f, s, T_MICRO, TEXT_FAINT)
}
