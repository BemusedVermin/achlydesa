//! **Achlydesa — a playable window onto the living world.** A first interactive front-end:
//! you spawn a body in the simulated world and *explore* it — walk the land, lift the fog,
//! watch a populace live and talk around you. The simulation (`agents`) is authoritative
//! and bevy-free; this Bevy shell is a thin view over it (the pattern the reference
//! strategy-tactics game uses). Presentation: hex columns at compressed-but-real relief (peaks
//! flattened so a piece can land on any top), coloured by terrain and vegetation class, dressed
//! with **procedurally generated** trees, scrub, rock, and the buildings of settlements, courts,
//! and ruins (see `props`, `scatter`, `feature_art`). Only the explored map is drawn, under a
//! cool haze.
//!
//! Controls: **hover** a tile to look · **left-click** to inspect it · **right-click** to travel ·
//! **Space** wait · **T** speak to a soul nearby · **A/D** orbit · **W/S** tilt · **scroll** zoom.
//! Turn-based — the world moves when you act.
//!
//! `cargo run -p app --release`

use agents::{Coord, FindState, Goals, Registry, Setup, Simulation};
use bevy::asset::AssetPlugin;
use bevy::camera::ScalingMode;
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::text::Font;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{BorderRadius, BoxShadow, GlobalZIndex};
use bevy::window::WindowResolution;
use std::collections::HashMap;

use app::theme::{self, ThemeFonts};

mod combat;
mod combat_field;
mod convo_ui;
mod fauna_art;
mod feature_art;
mod ground;
mod hud;
mod layout;
mod mesh;
mod minimap;
mod outline;
mod palette;
mod props;
mod scatter;
mod toon;
mod ui;
mod world_mesh;

use layout::{tile_top, tile_world};
use toon::{ToonMaterial, toon};

// =====================================================================================
// The dialogue voice (optional SLM). All candle-touching code is confined to the `voice`
// crate and reached only through this thin bridge, so the app compiles and runs identically
// with the feature off — the conversation then simply stays on the deterministic grammar.
// =====================================================================================

#[cfg(feature = "voice")]
mod voice_bridge {
    use voice::{Voice, VoiceStatus};

    /// Owns the background voice worker. Cheap to hold; loads the model off-thread.
    pub struct Bridge(Voice);

    impl Bridge {
        pub fn spawn() -> Self {
            Self(Voice::spawn_from_config())
        }
        /// Is the model loaded and serving?
        pub fn is_ready(&self) -> bool {
            self.0.is_ready()
        }
        /// Queue a free-text conversation turn: the character described by `card` answers
        /// `player_msg` given the prior `history` (`(from_player, text)` turns). `true` if dispatched.
        pub fn request_chat(
            &self,
            req_id: u64,
            card: &str,
            history: &[(bool, String)],
            player_msg: &str,
            fallback: &str,
        ) -> bool {
            let turns: Vec<voice::ChatTurn> = history
                .iter()
                .map(|(from_player, text)| voice::ChatTurn {
                    from_player: *from_player,
                    text: text.clone(),
                })
                .collect();
            self.0
                .request_chat(req_id, card, &turns, player_msg, fallback)
        }
        /// Classify what the player said into one of `labels` (for the social effect).
        pub fn request_classify(
            &self,
            req_id: u64,
            name: &str,
            message: &str,
            labels: &[&str],
            fallback: &str,
        ) -> bool {
            self.0
                .request_classify(req_id, name, message, labels, fallback)
        }
        /// Drain finished generations: `(req_id, voiced line)`.
        pub fn poll(&self) -> Vec<(u64, String)> {
            self.0.poll()
        }
        /// A short HUD status, or `None` when the voice is off.
        pub fn status_line(&self) -> Option<String> {
            match self.0.status() {
                VoiceStatus::Off => None,
                VoiceStatus::Loading => Some("voice: loading model…".into()),
                VoiceStatus::Ready => Some("voice: on".into()),
                VoiceStatus::Failed(e) => Some(format!(
                    "voice: off ({})",
                    e.lines().next().unwrap_or("failed")
                )),
            }
        }
    }
}

#[cfg(not(feature = "voice"))]
mod voice_bridge {
    /// A no-op stand-in when the voice feature is compiled out.
    pub struct Bridge;

    impl Bridge {
        pub fn spawn() -> Self {
            Self
        }
        pub fn is_ready(&self) -> bool {
            false
        }
        pub fn request_chat(
            &self,
            _req_id: u64,
            _card: &str,
            _history: &[(bool, String)],
            _player_msg: &str,
            _fallback: &str,
        ) -> bool {
            false
        }
        pub fn request_classify(
            &self,
            _req_id: u64,
            _name: &str,
            _message: &str,
            _labels: &[&str],
            _fallback: &str,
        ) -> bool {
            false
        }
        pub fn poll(&self) -> Vec<(u64, String)> {
            Vec::new()
        }
        pub fn status_line(&self) -> Option<String> {
            None
        }
    }
}

/// Sim ticks per real second while the world is running.
const TICK_DT: f32 = 0.12;
/// Characters per second the conversation text types in at (the RPG-style reveal that also
/// masks the model's generation latency).
const REVEAL_CPS: f32 = 45.0;

// =====================================================================================
// The simulation, held as a NonSend resource (it wraps a bevy_ecs world of its own; we
// drive it by hand each frame and never let the outer app schedule it).
// =====================================================================================

struct Game {
    sim: Simulation,
    avatar_pos: Coord,
    /// The smoothed world position the camera and the avatar figure glide toward — eased each
    /// frame to the tile the avatar stands on, so movement reads as a walk, not a hex-step.
    avatar_render: Vec3,
    last_tick: u64,
    /// The world tick the rendered fauna were last synced to (so creatures only
    /// re-target when the world has actually moved).
    last_fauna_tick: u64,
    accum: f32,
    status: String,
    convo: Option<Convo>,
    /// The active fight, when the avatar is in combat — a modal over the world like a conversation.
    /// The world clock is suspended while it is `Some`; the player drives the headless engine inside.
    combat: Option<combat::CombatUi>,
    /// The optional on-device voice; renders the focused conversation's words.
    voice: voice_bridge::Bridge,
    /// Monotonic id stamped on each voicing request, so an async result can be matched
    /// back to the transcript line it belongs to.
    req_seq: u64,
    /// In-flight intent classifications: request id → the NPC the effect lands on. When the
    /// result arrives, it drives `apply_conversational_intent` (the conversation's social effect).
    classify: HashMap<u64, Entity>,
    /// The tile under the cursor right now (hover), and the tile the player has clicked to
    /// inspect — the two halves of the new pick model (hover = look, click = select).
    hovered: Option<Coord>,
    selected: Option<Coord>,
    /// When several souls are within reach, Talk opens a chooser instead of grabbing the nearest:
    /// the candidates, snapshotted while the picker is up (`None` when not choosing).
    talk_choices: Option<Vec<Entity>>,
    /// Is the pause menu (Esc) open? While paused, gameplay input is suspended and a tabbed
    /// parchment menu overlays the view. The world is turn-based, so this is a modal, not a clock.
    paused: bool,
    /// The active menu tab (index into `MENU_TABS`: Journal/Character/Inventory/Map/System).
    menu_tab: usize,
    /// Whose sheet the Character tab shows — a clicked party portrait's entity, or `None` for the
    /// avatar (the default; reset whenever the tab is opened by key or the top bar).
    sheet_subject: Option<Entity>,
    /// The cursor row within the System tab (Resume/Quit).
    sys_cursor: usize,
    /// The Map tab's view: the world point at the frame's centre, the zoom (world units per pixel),
    /// and whether a drag is in progress. Re-centred on the avatar each time the tab opens, then
    /// pannable by dragging and zoomable by scroll.
    map_center: Vec2,
    map_zoom: f32,
    map_dragging: bool,
    /// A key of what the Map tab was last rendered for (centre, zoom, explored count), so it only
    /// re-renders when the view or the fog actually changed.
    last_map_render: Option<(i32, i32, i32, usize)>,
    /// The explored count + avatar tile the always-on HUD minimap was last drawn for, so it only
    /// re-renders when new ground is uncovered or the avatar moves (the pip follows).
    last_hud_explored: usize,
    last_hud_avatar: Coord,
    /// The souls the avatar has met (spoken with), in the order first met — the **ledger**: a who's-who
    /// of acquaintances, shown on the Journal tab with where each one's story stands now. Player-side
    /// memory, so it never feeds the sim; a dead acquaintance is remembered as gone.
    met: Vec<Entity>,
    /// The **charges** the avatar has taken up (the director's drama as goals), and the lines of those
    /// it has closed — player-side, shown in the HUD objective and the Journal. Never feed the sim.
    quests: Vec<agents::Quest>,
    done_quests: Vec<String>,
    /// `(giver, other)` pairs already closed, so a giver doesn't re-offer the same charge.
    quest_done_pairs: std::collections::HashSet<(Entity, Entity)>,
}

/// An open, free-text conversation with one soul within reach. The player *types* to the
/// character and it answers in its own voice, grounded in its real sim state (the `card`). The
/// conversation is read-only — it reflects the world but does not (yet) mutate the deterministic
/// social state — and time is paused while it is open.
struct Convo {
    /// The soul being spoken to — the entity the conversation's social effects land on.
    listener: Entity,
    name: String,
    /// The character's grounded state, assembled once from the sim — the model's system context.
    card: String,
    /// The exchange so far, newest last (capped to the recent few).
    transcript: Vec<Line>,
    /// What the player is currently typing.
    input: String,
    /// A **charge** this soul is offering (it leads a live thread) — `None` if it has none, or it has
    /// already been taken. The "take up the charge" button reads this.
    offer: Option<agents::Quest>,
}

/// One line of the open conversation, revealed like classic RPG text. The character's reply
/// shows an animated "considering" ellipsis while the model generates (`text == None`), then
/// types its words out (`reveal` advancing over `text`). The player's own typed words have
/// `text` set from the start and appear at once.
struct Line {
    /// `true` if the player typed it — used both to render and to build the chat history.
    from_player: bool,
    prefix: String,
    /// The words to reveal once known. `None` while a reply is still being generated.
    text: Option<String>,
    /// Characters of `text` revealed so far — a float, so the typewriter is frame-rate independent.
    reveal: f32,
    /// The in-flight chat request id this line is waiting on, if any.
    pending: Option<u64>,
}

#[derive(Resource)]
struct RenderAssets {
    map_mat: Handle<ToonMaterial>,
    avatar_mesh: Handle<Mesh>,
    avatar_mat: Handle<ToonMaterial>,
    npc_mesh: Handle<Mesh>,
    npc_mat: Handle<ToonMaterial>,
}

#[derive(Component)]
struct MapMesh;
#[derive(Component)]
struct Marker;
/// The persistent avatar figure — moved/glided every frame (not rebuilt with the tick markers),
/// so the walk reads smoothly instead of stepping a hex at a time.
#[derive(Component)]
pub(crate) struct AvatarFig;
#[derive(Component)]
struct CamRig {
    /// How far back the camera sits from the focus. With an orthographic projection this no longer
    /// changes apparent size (parallel rays) — it only keeps the scene between the near/far planes
    /// and sets how far into the distance fog the view reads; `zoom` is what the scroll wheel drives.
    dist: f32,
    yaw: f32,
    pitch: f32,
    /// The orthographic framing: world units shown vertically. Smaller = zoomed in.
    zoom: f32,
}
#[derive(Component, Clone, Copy)]
enum HudKind {
    Look,
    Help,
    /// The narrative banner under the tabs — the world's drama pushed at the player as it moves
    /// (gossip overheard, or the unrest it can sense). See [`Simulation::tidings`].
    Tidings,
}

fn main() {
    let mut sim = build_world();
    sim.spawn_player(None);
    let avatar_pos = sim.player_position().unwrap_or(Coord::new(0, 0));
    let avatar_render = {
        let aw = tile_world(avatar_pos.col, avatar_pos.row);
        Vec3::new(aw.x, tile_top(sim.substrate(), avatar_pos) + 1.2, aw.y)
    };
    let game = Game {
        sim,
        avatar_pos,
        avatar_render,
        last_tick: u64::MAX,
        last_fauna_tick: u64::MAX,
        accum: 0.0,
        status: "Welcome. Click a tile to set out - the world moves when you do.".into(),
        convo: None,
        combat: None,
        voice: voice_bridge::Bridge::spawn(),
        req_seq: 0,
        classify: HashMap::new(),
        hovered: None,
        selected: None,
        talk_choices: None,
        // Dev hooks: `ACHLYDESA_PAUSE` starts on the pause menu, `ACHLYDESA_TAB=N` on tab N.
        paused: std::env::var("ACHLYDESA_PAUSE").is_ok(),
        menu_tab: std::env::var("ACHLYDESA_TAB")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        sheet_subject: None,
        sys_cursor: 0,
        map_center: Vec2::ZERO,
        map_zoom: MAP_WPP_DEFAULT,
        map_dragging: false,
        last_map_render: None,
        last_hud_explored: usize::MAX,
        last_hud_avatar: Coord::new(i32::MIN, i32::MIN),
        met: Vec::new(),
        quests: Vec::new(),
        done_quests: Vec::new(),
        quest_done_pairs: std::collections::HashSet::new(),
    };

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Achlydesa — exploration".into(),
                    // The reference size the framed HUD is authored against; the whole frame
                    // scales as one with the window from here (see `hud::scale_ui`). `ACHLYDESA_RES=WxH`
                    // overrides it (handy for eyeballing the scaling at other sizes).
                    resolution: hud_resolution(),
                    ..default()
                }),
                ..default()
            })
            // Runtime assets (the user's parchment, any images) live in the workspace `assets/`
            // folder — one up from this crate — alongside the baked RON. Point Bevy there.
            .set(AssetPlugin {
                file_path: "../assets".into(),
                ..default()
            }),
    )
    // The cel pass over the world: a material plugin for the toon-extended StandardMaterial the
    // terrain/props/fauna/figures all share (selection rings stay plain StandardMaterial), plus the
    // post-process outline that inks edges off the depth + normal prepass.
    .add_plugins((
        MaterialPlugin::<ToonMaterial>::default(),
        outline::OutlinePlugin,
    ))
    .insert_resource(ClearColor(Color::srgb(
        palette::SKY_RGB[0],
        palette::SKY_RGB[1],
        palette::SKY_RGB[2],
    )))
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            drive_sim,
            talk_input,
            poll_voice,
            tick_typewriter,
            wait_input,
            search_input,
            use_input,
            journal_input,
            ui::tile_interact,
            // Diff the explored set once up front; the builders below read this frame's delta + set.
            ground::track_explored,
            ground::rebuild_ground,
            scatter_props,
            build_features,
            rebuild_markers,
            camera_control,
            ui::update_highlights,
            ui::update_tooltip,
            ui::update_inspect,
            ui::update_labels,
            update_hud,
        )
            .chain(),
    )
    // The fauna layer runs as its own group: `sync_fauna` re-targets creatures when
    // the world ticks (idempotent in between, so loose ordering is fine) and
    // `animate_fauna` is purely visual.
    .add_systems(
        Update,
        (
            smooth_follow,
            sync_fauna,
            fauna_art::animate_fauna,
            recruit_input,
            sheet_input,
        )
            .chain(),
    )
    // The conversation panel + the who-to-talk-to chooser: fill them, style and handle their buttons.
    .add_systems(
        Update,
        (
            convo_ui::update_convo_panel,
            convo_ui::style_speak_choices,
            convo_ui::speak_choice_click,
            convo_ui::style_counsel_choices,
            convo_ui::counsel_click,
            convo_ui::update_quest_offer,
            convo_ui::quest_accept_click,
            convo_ui::update_talk_chooser,
            convo_ui::talk_row_click,
        ),
    )
    // The pause layer: Esc/back + menu nav run *before* `talk_input` (which also reads Esc, to
    // leave a conversation), then the overlay's visibility + the dev screenshot hook.
    .add_systems(Update, (pause_input, menu_input).chain().before(talk_input))
    .add_systems(
        Update,
        (
            update_menu,
            update_map,
            map_drag,
            hide_overlays_when_paused,
            dev_capture,
            dev_fight,
            dev_open_convo,
            dev_talk_pick,
            dev_walk,
            update_quests,
        ),
    )
    // The combat mode: detect the start of a fight, drive the engine to the next player decision,
    // take the player's commands, and paint the timeline-ribbon HUD. Runs after the action inputs.
    .add_systems(
        Update,
        (
            attack_input,
            combat::combat_tick,
            dev_combat_autoplay,
            combat::combat_input,
            combat::combat_clicks,
            combat::combat_render_field,
            combat::update_combat_chrome,
            combat::update_combat_roster,
            combat::update_combat_tray,
            combat::update_combat_timeline,
            hide_hud_in_combat,
        )
            .chain(),
    )
    // The framed HUD: keep the whole frame scaled to the window, then refresh the trays.
    .add_systems(
        Update,
        (
            hud::scale_ui,
            hud::update_portraits,
            hud::update_vitals,
            hud::update_action_buttons,
            hud::update_hud_minimap,
            hud::action_button_click,
            hud::top_tab_click,
            hud::portrait_click,
        ),
    )
    .init_resource::<ground::Ground>()
    .init_resource::<ground::Explored>()
    .init_resource::<ui::LabelCache>()
    .init_resource::<NpcMarkers>()
    .init_resource::<CaptureClock>();
    app.world_mut().insert_non_send_resource(game);
    app.run();
}

/// The starting window size: the HUD reference, unless `ACHLYDESA_RES=WxH` overrides it (so the
/// frame's scaling can be eyeballed at any size; the layout stays in proportion either way).
fn hud_resolution() -> WindowResolution {
    if let Ok(spec) = std::env::var("ACHLYDESA_RES")
        && let Some((w, h)) = spec.split_once(['x', 'X'])
        && let (Ok(w), Ok(h)) = (w.trim().parse::<u32>(), h.trim().parse::<u32>())
    {
        return WindowResolution::new(w, h);
    }
    WindowResolution::new(hud::REF_W as u32, hud::REF_H as u32)
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
    // The app owns world generation. Build the large, US-scale `game_sim` world here —
    // ~1 hex ≈ a day's walk, so crossing the main landmass is multiple months on foot;
    // few tectonic plates raise a handful of huge continents, and the wider uplift falloff
    // keeps mountain belts from thinning to ribbons at this scale — then hand it to the
    // agent simulation via `from_world`. Worldgen itself lives in `game_sim`; `agents` only
    // drives the substrate it is given. (Starting values — tune to taste.)
    // Dev knobs to isolate the per-tick cost (default to the shipping values).
    let env = |k: &str, d: usize| {
        std::env::var(k)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(d)
    };
    let (width, height, seed) = (
        env("ACHLYDESA_W", 192) as i32,
        env("ACHLYDESA_H", 144) as i32,
        7,
    );
    let mut params = config::tunables::params();
    params.plates = 5; // few plates → a few huge continents
    params.uplift_falloff = 16.0; // wider mountain belts to match the larger scale
    let world = game_sim::World::generate(width, height, params, seed);

    Simulation::from_world(
        world,
        Setup {
            seed,
            // A full year-plus, so the running annual biome classifier matures before play:
            // on this vast continent the climate needs ≥365 days to spin moisture inland and
            // settle the biomes (at 120 days the world reads as immature near-total desert).
            warmup: 400,
            // The wild — herds and the packs that hunt them, sorted into the biomes that
            // suit them. Spawned generously across the vast map; many die sorting into the
            // harsh world, and only those on explored tiles are drawn, so survivors are met.
            fauna: env("ACHLYDESA_FAUNA", 1000),
            carnivores: env("ACHLYDESA_CARN", 200),
            // A denser populace so settlements bustle and you run into people (and their drama) more
            // often; LOD keeps the per-tick cost local. Tune with `ACHLYDESA_NPCS`.
            npcs: env("ACHLYDESA_NPCS", 420),
            markets: 12,
            markets_on_settlements: true,
            dialogue: true,
            // The hidden narrative director shapes drama among the populace as you explore — the
            // headline of the narrative-surfacing layer (`docs/narrative_surfacing.md`). On by
            // default; `ACHLYDESA_NODIRECTOR` disables it for plain whole-world exploration. It
            // biases its casting toward souls near the avatar (`agent_core::director`), so the
            // drama is something you can walk into rather than something unfolding off-map.
            director: std::env::var("ACHLYDESA_NODIRECTOR").is_err(),
            // The RPG, party and exploration layers are on for the game: the avatar and every NPC
            // roll Worlds-Without-Number stats; the avatar can recruit companions who travel as a
            // stack; and travel is cost-paced over a road network with terrain/elevation gates.
            rpg: true,
            party: true,
            exploration: true,
            // The combat layer: the avatar and party can fight adjacent hostiles (press **G**) and
            // are ambushed by predators/grudge-bearers they step beside. Downed enemies die; the
            // party's HP carries between fights. The fight runs in the headless `combat_core` engine.
            combat: true,
            // Survival is on but **party-scoped**: only the avatar and its companions face thirst /
            // warmth / stamina drain. The general NPC population is untouched (no Vitals, flat
            // hunger), so the mostly-arid world doesn't depopulate before NPCs can seek water/shelter
            // on their own — the world-wide variant (`survival_everyone: true`) waits on that AI.
            survival: true,
            survival_everyone: false,
            // Level-of-detail: NPCs within this many hexes of the avatar simulate in full every
            // tick; farther ones run on a coarse clock (one tick in `sim_far_stride`), so a heavily
            // peopled world stays smooth as you walk while the distant populace still lives, slowly.
            // The director sees every soul each tick, so drama is intact. `ACHLYDESA_LOD=N` sets the
            // radius (`off` disables); `ACHLYDESA_STRIDE=N` sets the coarse stride.
            sim_radius: lod_radius(),
            sim_far_stride: std::env::var("ACHLYDESA_STRIDE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(12),
            goals,
            registry: reg,
            ..Default::default()
        },
    )
}

/// The level-of-detail radius for the app, overridable with `ACHLYDESA_LOD` (`off` to disable).
fn lod_radius() -> Option<i32> {
    match std::env::var("ACHLYDESA_LOD") {
        Ok(s) if s.eq_ignore_ascii_case("off") => None,
        Ok(s) => s.parse().ok().or(Some(26)),
        Err(_) => Some(26),
    }
}

// =====================================================================================
// Setup
// =====================================================================================

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut toon_mats: ResMut<Assets<ToonMaterial>>,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    asset_server: Res<AssetServer>,
    game: NonSend<Game>,
) {
    // The world wears the cel material (terrain, props, fauna, and the figures all share it); the
    // mesh still carries the colour, the toon pass just bands the lighting. Plain `materials`
    // (StandardMaterial) lives on for the UI selection rings in `ui::setup_ui`.
    let map_mat = toon_mats.add(toon(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.96,
        ..default()
    }));
    let avatar_mat = toon_mats.add(toon(StandardMaterial {
        base_color: Color::srgb(1.0, 0.82, 0.25),
        emissive: LinearRgba::rgb(0.7, 0.5, 0.12),
        ..default()
    }));
    let npc_mat = toon_mats.add(toon(StandardMaterial {
        base_color: Color::srgb(0.78, 0.80, 0.88),
        ..default()
    }));
    // The procedural prop library (trees, scrub, rock) shares the map's matte vertex-colour
    // material; the meshes carry their own colour.
    let prop_lib = props::build_library(&mut meshes, map_mat.clone());
    commands.insert_resource(prop_lib);
    // One procedural creature mesh per species, sharing the matte vertex-colour material.
    let fauna_art = fauna_art::build_fauna_art(&mut meshes, map_mat.clone(), game.sim.bestiary());
    commands.insert_resource(fauna_art);
    commands.init_resource::<feature_art::Built>();
    let avatar_mesh = meshes.add(Capsule3d::new(0.5, 1.8));
    commands.insert_resource(RenderAssets {
        map_mat,
        avatar_mesh: avatar_mesh.clone(),
        avatar_mat: avatar_mat.clone(),
        npc_mesh: meshes.add(Cylinder::new(0.2, 0.55)),
        npc_mat,
    });
    // The avatar is one persistent figure (not rebuilt with the per-tick markers): `smooth_follow`
    // glides it — and the camera — toward the tile it stands on, so a walk reads as a glide.
    commands.spawn((
        AvatarFig,
        Mesh3d(avatar_mesh),
        MeshMaterial3d(avatar_mat),
        Transform::from_translation(game.avatar_render),
    ));

    let aw = tile_world(game.avatar_pos.col, game.avatar_pos.row);
    let rig = CamRig {
        dist: 42.0,
        yaw: 0.0,
        pitch: 0.92,
        zoom: 34.0,
    };
    // Orthographic: the flat, diorama "2.5D" read — parallel projection drops the perspective
    // convergence the orbit camera had. `zoom` is the world height the viewport spans; the scroll
    // wheel drives it (see `camera_control`). `FixedVertical` keeps that height constant as the
    // window resizes, so the framing is stable.
    let projection = Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::FixedVertical {
            viewport_height: 1.0,
        },
        scale: rig.zoom,
        ..OrthographicProjection::default_3d()
    });
    let fog = palette::FOG_RGB;
    commands.spawn((
        Camera3d::default(),
        projection,
        // The outline pass reads the depth prepass for its edge detection. MSAA is off so the depth
        // texture (and the colour target) are single-sampled — what the post-process shader's plain
        // `texture_depth_2d` binding expects.
        DepthPrepass,
        Msaa::Off,
        cam_transform(Vec3::new(aw.x, 0.0, aw.y), &rig),
        rig,
        // A cool ambient and a pale distance haze — the dream half-drowned in fog.
        AmbientLight {
            brightness: 200.0,
            color: Color::srgb(0.72, 0.79, 0.9),
            ..default()
        },
        DistanceFog {
            color: Color::srgb(fog[0], fog[1], fog[2]),
            falloff: FogFalloff::Linear {
                start: 70.0,
                end: 300.0,
            },
            ..default()
        },
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 6200.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.6, -0.95, 0.0)),
    ));

    // Load the shared HUD fonts first, so every panel is styled through the theme.
    let theme_fonts = ThemeFonts::embed(&mut fonts);
    ui::setup_ui(&mut commands, &mut meshes, &mut materials, &theme_fonts);
    // The framed HUD: an opaque grassy-rock border (trays) around the centre world view.
    let grassy = asset_server.load("ui/grassy_rock.jpg");
    hud::spawn(&mut commands, &theme_fonts, grassy);
    convo_ui::spawn(&mut commands, &theme_fonts);
    combat::spawn_combat_ui(&mut commands, &theme_fonts, &mut images);
    let parchment = asset_server.load("ui/parchment.jpg");
    spawn_pause_menu(&mut commands, &theme_fonts, parchment);
    commands.insert_resource(theme_fonts);
}

// ── The pause menu (Esc): a modal scrim + a centred hub of panels you can open ──────────────────

// ── The pause menu — an Oblivion-style tabbed parchment pane ─────────────────────────────────────

/// The menu tabs, in paging order. The top-tray tab buttons open this menu at the matching index.
const MENU_TABS: [&str; 5] = ["Journal", "Character", "Inventory", "Map", "System"];
/// Tab indices (kept in sync with `MENU_TABS`).
const TAB_INVENTORY: usize = 2;
const TAB_MAP: usize = 3;
const TAB_SYSTEM: usize = 4;
/// The System-tab rows (Resume / Quit).
const SYS_ITEMS: [(&str, &str); 2] = [("Resume", "Esc"), ("Quit to the Grey", "")];

// Ink on aged paper — dark, warm, readable on the parchment.
const PARCH_INK: Color = Color::srgb(0.20, 0.15, 0.10);
const PARCH_DIM: Color = Color::srgb(0.42, 0.34, 0.22);
const PARCH_ACCENT: Color = Color::srgb(0.56, 0.30, 0.10);

#[derive(Component)]
struct PauseRoot;
/// A tab header's label (index) — recoloured for the active tab.
#[derive(Component)]
struct TabText(usize);
/// A tab header's underline bar (index) — lit for the active tab.
#[derive(Component)]
struct TabUnderline(usize);
/// A per-tab content panel (index) — only the active one is laid out (`Display`).
#[derive(Component)]
struct TabPanel(usize);
/// A System-tab row (index) — highlighted under the cursor.
#[derive(Component)]
struct SysRow(usize);
#[derive(Component)]
struct JournalTabText;
#[derive(Component)]
struct SheetTabText;
#[derive(Component)]
struct InventoryTabText;
#[derive(Component)]
struct MapImageNode;

fn parch_divider() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(1.0),
            margin: UiRect::axes(Val::Px(0.0), Val::Px(theme::SP_XS)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.36, 0.28, 0.16, 0.6)),
    )
}

fn spawn_pause_menu(commands: &mut Commands, f: &ThemeFonts, parchment: Handle<Image>) {
    commands
        .spawn((
            PauseRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.62)),
            GlobalZIndex(100),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            // The parchment pane (assets/ui/parchment.jpg) under a warm border.
            root.spawn((
                Node {
                    width: Val::Px(620.0),
                    min_height: Val::Px(440.0),
                    padding: UiRect::all(Val::Px(theme::SP_LG)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(theme::RADIUS)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(theme::SP_MD),
                    ..default()
                },
                ImageNode { image: parchment, image_mode: NodeImageMode::Stretch, ..default() },
                BorderColor::all(Color::srgb(0.36, 0.28, 0.16)),
                BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.55), Val::Px(0.0), Val::Px(6.0), Val::Px(0.0), Val::Px(22.0)),
            ))
            .with_children(|pane| {
                // Tab bar.
                pane.spawn(Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    column_gap: Val::Px(theme::SP_XL),
                    align_items: AlignItems::FlexEnd,
                    ..default()
                })
                .with_children(|bar| {
                    for (i, label) in MENU_TABS.iter().enumerate() {
                        bar.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(theme::SP_XS), ..default() })
                            .with_children(|t| {
                                t.spawn((TabText(i), theme::serif(f, label.to_string(), theme::T_TITLE, PARCH_DIM)));
                                t.spawn((TabUnderline(i), Node { width: Val::Px(74.0), height: Val::Px(2.0), ..default() }, BackgroundColor(Color::NONE)));
                            });
                    }
                });
                pane.spawn(parch_divider());

                // Content — four stacked panels; only the active one is `Display::Flex`.
                pane.spawn(Node { width: Val::Percent(100.0), flex_grow: 1.0, ..default() }).with_children(|content| {
                    // 0 — Journal.
                    content
                        .spawn((TabPanel(0), Node { flex_direction: FlexDirection::Column, width: Val::Percent(100.0), ..default() }))
                        .with_children(|p| {
                            p.spawn((JournalTabText, theme::mono(f, "", 13.0, PARCH_INK)));
                        });
                    // 1 — Character.
                    content
                        .spawn((TabPanel(1), Node { flex_direction: FlexDirection::Column, width: Val::Percent(100.0), display: Display::None, ..default() }))
                        .with_children(|p| {
                            p.spawn((SheetTabText, theme::mono(f, "", 13.0, PARCH_INK)));
                        });
                    // 2 — Inventory.
                    content
                        .spawn((TabPanel(TAB_INVENTORY), Node { flex_direction: FlexDirection::Column, width: Val::Percent(100.0), display: Display::None, ..default() }))
                        .with_children(|p| {
                            p.spawn((InventoryTabText, theme::mono(f, "", 13.0, PARCH_INK)));
                        });
                    // 3 — Map.
                    content
                        .spawn((TabPanel(TAB_MAP), Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(theme::SP_SM), width: Val::Percent(100.0), display: Display::None, ..default() }))
                        .with_children(|p| {
                            p.spawn(theme::serif(f, "The Grey Country", theme::T_TITLE, PARCH_INK));
                            p.spawn((
                                MapImageNode,
                                Button, // so it reports hover/press for drag-to-pan and scroll-to-zoom
                                ImageNode { image: Handle::default(), image_mode: NodeImageMode::Stretch, ..default() },
                                Node { width: Val::Px(440.0), height: Val::Px(300.0), border: UiRect::all(Val::Px(theme::BORDER_W)), ..default() },
                                BorderColor::all(Color::srgb(0.36, 0.28, 0.16)),
                            ));
                            p.spawn(theme::mono(f, "drag to pan · scroll to zoom · gold court · pale town · dun ruin · cyan wonder", theme::T_LABEL, PARCH_DIM));
                        });
                    // 4 — System.
                    content
                        .spawn((TabPanel(TAB_SYSTEM), Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(theme::SP_XS), width: Val::Percent(100.0), display: Display::None, ..default() }))
                        .with_children(|p| {
                            for (i, (label, key)) in SYS_ITEMS.iter().enumerate() {
                                p.spawn((
                                    SysRow(i),
                                    Node {
                                        width: Val::Percent(100.0),
                                        flex_direction: FlexDirection::Row,
                                        justify_content: JustifyContent::SpaceBetween,
                                        padding: UiRect::axes(Val::Px(theme::SP_SM), Val::Px(theme::SP_XS)),
                                        border_radius: BorderRadius::all(Val::Px(theme::RADIUS_SM)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                ))
                                .with_children(|row| {
                                    row.spawn(theme::mono(f, label.to_string(), 14.0, PARCH_INK));
                                    row.spawn(theme::mono(f, key.to_string(), theme::T_LABEL, PARCH_DIM));
                                });
                            }
                        });
                });

                pane.spawn(parch_divider());
                pane.spawn(theme::mono(f, "Q / E  page tabs        Enter  select        Esc  resume", theme::T_MICRO, PARCH_DIM));
            });
        });
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
    if game.paused || game.combat.is_some() {
        return;
    }
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
        // A predator or grudge-bearer the avatar just stepped beside springs an ambush.
        if g.combat.is_none() {
            let ambush = g.sim.combat_ambush();
            if !ambush.is_empty() {
                g.sim.player_halt();
                combat::start(g, ambush);
            }
        }
    }
}

/// **Attack** — press **G** to set upon the bodies next to you (a foe, a beast): the world freezes
/// and the fight opens in the combat mode. Ignored mid-journey, in menus or conversation, or when
/// nothing stands adjacent.
fn attack_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.combat.is_some()
        || game.convo.is_some()
        || game.paused
        || game.talk_choices.is_some()
        || !keys.just_pressed(KeyCode::KeyG)
    {
        return;
    }
    if game.sim.player_traveling() {
        return;
    }
    let foes: Vec<Entity> = game
        .sim
        .combat_targets()
        .into_iter()
        .map(|(e, _)| e)
        .collect();
    if foes.is_empty() {
        game.status = "There is no one here to fight.".into();
        return;
    }
    combat::start(&mut game, foes);
}

/// **Wait** — the second player action. Tap **Space** to let one tick pass where you stand:
/// the avatar holds its ground and the world lives a single moment around it (one action,
/// one tick — the same cost as stepping a hex). Ignored mid-journey (time is already
/// flowing as you walk) and when no avatar is in the world.
fn wait_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.convo.is_some() || game.paused || !keys.just_pressed(KeyCode::Space) {
        return;
    }
    do_wait(&mut game);
}

/// Let one tick pass where the avatar stands (shared by Space and the Wait button).
fn do_wait(g: &mut Game) {
    if g.sim.player_traveling() || g.combat.is_some() {
        return;
    }
    if g.sim.player_wait() {
        g.status = "You wait, and the world moves a moment on.".into();
    }
}

/// **Search** — press **F** to search the tile you stand on. Reveals the hidden things here you
/// have the knowledge to find, and you learn whatever lore they hold. A locked place tells you
/// it is there but withholds itself until you know more. One action, one tick (like waiting).
fn search_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.convo.is_some() || game.paused || !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    do_search(&mut game);
}

/// Search the tile underfoot (shared by F and the Search button).
fn do_search(g: &mut Game) {
    if g.sim.player_traveling() || g.combat.is_some() {
        return;
    }
    let out = g.sim.player_search();
    if out.is_empty() {
        g.status = match g.sim.player_find_state() {
            FindState::Locked => {
                "Something is hidden here, but you lack the knowledge to find it.".into()
            }
            _ => "You search the ground, but find nothing of note.".into(),
        };
        return;
    }
    let mut s = format!("You discover: {}.", out.found.join(", "));
    if !out.lore_gained.is_empty() {
        let learned: Vec<String> = out.lore_gained.iter().map(|l| ui::pretty(l)).collect();
        s.push_str(&format!("  You come to know: {}.", learned.join(", ")));
    }
    g.status = s;
}

/// **Use** — press **E** to engage the smart-object where you stand: rest at a spring, draw water at
/// an oasis, work a craft at a hall. The place tends your body where the survival layer lets it, and
/// the world lives a tick around the act. One action, one tick (like searching or waiting).
fn use_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.convo.is_some() || game.paused || !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    do_use(&mut game);
}

/// Engage the first affordance the avatar stands on (shared by E and the Use button).
fn do_use(g: &mut Game) {
    if g.sim.player_traveling() || g.combat.is_some() {
        return;
    }
    let Some((idx, verb)) = g.sim.affordances_here().into_iter().next() else {
        g.status = "There is nothing here to put your hand to.".into();
        return;
    };
    g.status = match g.sim.player_use_affordance(idx) {
        Some(outcome) => outcome,
        None => format!("You move to {verb}, but the moment passes."),
    };
}

/// **Journal** — press **J** to open or close the discoveries journal (what you've found and the
/// lore you hold). A look at the world, not an action: time does not pass.
fn journal_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.combat.is_none() && game.convo.is_none() && keys.just_pressed(KeyCode::KeyJ) {
        game.paused = true;
        game.menu_tab = 0;
    }
}

/// **Recruit** — press **R** beside a soul to ask it into your party. A live Worlds-Without-Number
/// social check (the avatar's Charisma + the better of Convince/Lead vs the soul's disposition)
/// decides; on a pass it joins and travels with you as a stack. Spends the turn either way.
fn recruit_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.convo.is_some() || game.paused || !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    do_recruit(&mut game);
}

/// Ask the nearest soul into the party (shared by R and the Recruit button).
fn do_recruit(g: &mut Game) {
    if g.sim.player_traveling() || g.combat.is_some() {
        return;
    }
    let Some((npc, _)) = g.sim.player_nearby_npcs().into_iter().next() else {
        g.status = "No soul near to recruit.".into();
        return;
    };
    let name = g.sim.display_name(npc);
    g.status = if g.sim.player_recruit(npc) {
        format!("{name} joins your party.")
    } else {
        format!("{name} will not follow you — not yet.")
    };
}

/// The friendly **speech acts** offered as clickable *speak choices* in the conversation panel —
/// the model-free way to build a soul's opinion (toward recruiting). Each is a real authored intent
/// whose moves land scaled by the avatar's speech skill (so a silver tongue persuades faster). The
/// leading `KeyCode` is unused now the acts are mouse choices, kept only to document their old keys.
/// (key, intent id, the verb shown on the choice and in the status line.)
pub const QUICK_ACTS: &[(KeyCode, &str, &str)] = &[
    (KeyCode::Digit1, "a_greeting", "greet"),
    (KeyCode::Digit2, "a_word_of_praise", "praise"),
    (KeyCode::Digit3, "a_confidence_shared", "confide in"),
    (KeyCode::Digit4, "a_consolation", "console"),
    (KeyCode::Digit5, "an_overture_of_peace", "make peace with"),
];

/// **Character sheet** — press **C** to open or close the avatar's Worlds-Without-Number sheet
/// (attributes, trained skills, gear, vitals, party). A look, not an action: no time passes.
fn sheet_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.combat.is_none() && game.convo.is_none() && keys.just_pressed(KeyCode::KeyC) {
        game.paused = true;
        game.menu_tab = 1;
        game.sheet_subject = None; // C always opens the avatar's own sheet
    }
}

/// **Esc** — the back button. A conversation handles its own Esc (in `talk_input`, which runs
/// after this); otherwise Esc closes an open panel (journal/sheet), or toggles the pause menu.
fn pause_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.convo.is_some() || game.combat.is_some() || !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    // Esc first dismisses the who-to-talk-to chooser; otherwise it toggles the pause menu.
    if game.talk_choices.is_some() {
        game.talk_choices = None;
        game.status = "You hold your tongue.".into();
        return;
    }
    game.paused = !game.paused;
}

/// Page the menu tabs (Q/E or Left/Right); in the System tab, W/S move the cursor and Enter acts.
fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut game: NonSendMut<Game>,
    mut exit: MessageWriter<AppExit>,
) {
    if !game.paused {
        return;
    }
    let tabs = MENU_TABS.len();
    if keys.just_pressed(KeyCode::KeyE) || keys.just_pressed(KeyCode::ArrowRight) {
        game.menu_tab = (game.menu_tab + 1) % tabs;
    }
    if keys.just_pressed(KeyCode::KeyQ) || keys.just_pressed(KeyCode::ArrowLeft) {
        game.menu_tab = (game.menu_tab + tabs - 1) % tabs;
    }
    if game.menu_tab == TAB_SYSTEM {
        let n = SYS_ITEMS.len();
        if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::ArrowUp) {
            game.sys_cursor = (game.sys_cursor + n - 1) % n;
        }
        if keys.just_pressed(KeyCode::KeyS) || keys.just_pressed(KeyCode::ArrowDown) {
            game.sys_cursor = (game.sys_cursor + 1) % n;
        }
        if keys.just_pressed(KeyCode::Enter) {
            match game.sys_cursor {
                0 => game.paused = false,
                1 => {
                    exit.write(AppExit::Success);
                }
                _ => {}
            }
        }
    }
}

/// Hide the always-on overlays (legend, inspect) while paused, for a clean modal.
fn hide_overlays_when_paused(
    game: NonSend<Game>,
    mut q: Query<&mut Visibility, With<ui::HideOnPause>>,
) {
    let hide = game.paused || game.combat.is_some();
    for mut vis in &mut q {
        *vis = if hide {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}

/// Drive the tabbed menu: overlay visibility, the active-tab highlight, which content panel shows,
/// the System cursor, and the live Journal/Character text.
fn update_menu(
    game: NonSend<Game>,
    mut root: Query<&mut Visibility, With<PauseRoot>>,
    mut tabs: Query<(&TabText, &mut TextColor)>,
    mut underlines: Query<(&TabUnderline, &mut BackgroundColor), Without<SysRow>>,
    mut panels: Query<(&TabPanel, &mut Node)>,
    mut sys: Query<(&SysRow, &mut BackgroundColor), Without<TabUnderline>>,
    mut journal: Query<
        &mut Text,
        (
            With<JournalTabText>,
            Without<SheetTabText>,
            Without<InventoryTabText>,
        ),
    >,
    mut sheet: Query<
        &mut Text,
        (
            With<SheetTabText>,
            Without<JournalTabText>,
            Without<InventoryTabText>,
        ),
    >,
    mut inventory: Query<
        &mut Text,
        (
            With<InventoryTabText>,
            Without<JournalTabText>,
            Without<SheetTabText>,
        ),
    >,
) {
    if let Ok(mut vis) = root.single_mut() {
        *vis = if game.paused {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !game.paused {
        return;
    }
    for (t, mut col) in &mut tabs {
        col.0 = if t.0 == game.menu_tab {
            PARCH_INK
        } else {
            PARCH_DIM
        };
    }
    for (u, mut bg) in &mut underlines {
        bg.0 = if u.0 == game.menu_tab {
            PARCH_ACCENT
        } else {
            Color::NONE
        };
    }
    for (p, mut node) in &mut panels {
        node.display = if p.0 == game.menu_tab {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (r, mut bg) in &mut sys {
        bg.0 = if game.menu_tab == TAB_SYSTEM && r.0 == game.sys_cursor {
            Color::srgba(0.55, 0.40, 0.18, 0.35)
        } else {
            Color::NONE
        };
    }
    if let Ok(mut t) = journal.single_mut() {
        t.0 = ui::journal_text(&game.sim, &game.met, &game.quests, &game.done_quests);
    }
    if let Ok(mut t) = sheet.single_mut() {
        t.0 = sheet_text(&game);
    }
    if let Ok(mut t) = inventory.single_mut() {
        t.0 = inventory_text(&game);
    }
}

/// The Map tab's view defaults and zoom limits (world units per texture pixel; smaller = closer).
const MAP_WPP_DEFAULT: f32 = 0.7;
const MAP_WPP_MIN: f32 = 0.18;
const MAP_WPP_MAX: f32 = 3.5;
/// The Map tab's rendered image size (texels).
const MAP_W: u32 = 440;
const MAP_H: u32 = 300;

/// Re-render the Map tab's image when its view (centre/zoom) or the explored set changes.
fn update_map(
    mut game: NonSendMut<Game>,
    mut images: ResMut<Assets<Image>>,
    mut q: Query<&mut ImageNode, With<MapImageNode>>,
) {
    if !game.paused || game.menu_tab != TAB_MAP {
        return;
    }
    let count = game.sim.player_explored_count();
    let key = (
        (game.map_center.x * 2.0) as i32,
        (game.map_center.y * 2.0) as i32,
        (game.map_zoom * 100.0) as i32,
        count,
    );
    if game.last_map_render == Some(key) {
        return;
    }
    let img = minimap::render(
        &game.sim,
        game.map_center,
        game.map_zoom,
        game.avatar_pos,
        MAP_W,
        MAP_H,
    );
    let handle = images.add(img);
    if let Ok(mut node) = q.single_mut() {
        node.image = handle;
    }
    game.last_map_render = Some(key);
}

/// Pan (drag) and zoom (scroll) the Map tab, and re-centre it on the avatar each time it opens.
fn map_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    ui_scale: Res<bevy::ui::UiScale>,
    map_q: Query<&Interaction, With<MapImageNode>>,
    mut game: NonSendMut<Game>,
    mut was_open: Local<bool>,
) {
    let open = game.paused && game.menu_tab == TAB_MAP;
    // On opening the tab, re-centre on the avatar at the default zoom.
    if open && !*was_open {
        let aw = tile_world(game.avatar_pos.col, game.avatar_pos.row);
        game.map_center = aw;
        game.map_zoom = MAP_WPP_DEFAULT;
        game.map_dragging = false;
    }
    *was_open = open;
    if !open {
        game.map_dragging = false;
        return;
    }

    let over = map_q
        .single()
        .map(|i| !matches!(i, Interaction::None))
        .unwrap_or(false);
    // Scroll over the map zooms toward/away.
    if over && scroll.delta.y != 0.0 {
        game.map_zoom =
            (game.map_zoom * (1.0 - scroll.delta.y * 0.12)).clamp(MAP_WPP_MIN, MAP_WPP_MAX);
    }
    // Press over the map starts a drag; it continues until the button is released, even off-image.
    if mouse.just_pressed(MouseButton::Left) && over {
        game.map_dragging = true;
    }
    if !mouse.pressed(MouseButton::Left) {
        game.map_dragging = false;
    }
    if game.map_dragging && motion.delta != Vec2::ZERO {
        // Window-logical px → texture px (UiScale), then → world: drag moves the map under the cursor.
        let s = ui_scale.0.max(0.01);
        let world = motion.delta / s * game.map_zoom;
        game.map_center -= world;
    }
}

#[derive(Resource, Default)]
struct CaptureClock(u32);

/// Dev hook: with `ACHLYDESA_SHOT=<path>` set, capture the window once the world has rendered,
/// then exit — so the live HUD/menus can be screenshotted headlessly. `ACHLYDESA_PAUSE` (read in
/// `main`) starts on the pause menu so it can be captured.
fn dev_capture(
    mut clock: ResMut<CaptureClock>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(path) = std::env::var("ACHLYDESA_SHOT") else {
        return;
    };
    clock.0 += 1;
    if clock.0 == 18 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if clock.0 >= 48 {
        exit.write(AppExit::Success);
    }
}

/// Hide the exploration HUD frame while a fight is on, so the combat overlay reads as its own clean
/// screen (the trays otherwise sit dimly beneath the translucent overlay). Restored when it ends.
fn hide_hud_in_combat(game: NonSend<Game>, mut trays: Query<&mut Visibility, With<hud::Tray>>) {
    let hide = game.combat.is_some();
    for mut v in &mut trays {
        *v = if hide {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}

/// Dev hook: with `ACHLYDESA_FIGHT` set, auto-act for the player each frame so a fight plays out
/// headlessly (for screenshots of the animated battle). No effect in normal play.
fn dev_combat_autoplay(mut game: NonSendMut<Game>) {
    if std::env::var("ACHLYDESA_FIGHT_AUTO").is_err() {
        return;
    }
    if let Some(ui) = game.combat.as_mut() {
        combat::dev_auto_player(ui);
    }
}

/// Dev hook: with `ACHLYDESA_FIGHT` set, drop the avatar straight into a fight once at startup so
/// the combat HUD can be screenshotted headlessly. Uses souls already in the world as foes.
fn dev_fight(mut game: NonSendMut<Game>, mut done: Local<bool>) {
    if *done || std::env::var("ACHLYDESA_FIGHT").is_err() || game.combat.is_some() {
        return;
    }
    *done = true;
    let g = &mut *game;
    let mut foes: Vec<Entity> = g
        .sim
        .player_nearby_npcs()
        .into_iter()
        .map(|(e, _)| e)
        .take(3)
        .collect();
    if foes.is_empty() {
        foes.extend(g.sim.any_npc());
    }
    if !foes.is_empty() {
        combat::start(g, foes);
    }
}

/// Dev hook: with `ACHLYDESA_CONVO` set, force open a conversation with any soul once, so the
/// conversation panel can be screenshotted headlessly (it needs no nearby soul or model).
fn dev_open_convo(mut game: NonSendMut<Game>) {
    if std::env::var("ACHLYDESA_CONVO").is_err() || game.convo.is_some() {
        return;
    }
    if let Some(npc) = game.sim.any_npc() {
        let g = &mut *game;
        open_conversation_with(g, npc);
    }
}

/// Dev hook: with `ACHLYDESA_WALK=N` set, march the avatar through N rounds of frontier-walking
/// once at startup to uncover a swath of map — so the windowed minimap / Map tab (and the chunked
/// ground) can be eyeballed with real exploration headlessly. Travel only routes over explored
/// ground, so each round heads to the south-most known tile and the fog lifts a little further.
fn dev_walk(mut game: NonSendMut<Game>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Some(rounds) = std::env::var("ACHLYDESA_WALK")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
    else {
        return;
    };
    *done = true;
    let g = &mut *game;
    let mut last = None;
    let (mut tot, mut mx, mut steps) = (std::time::Duration::ZERO, std::time::Duration::ZERO, 0u32);
    for _ in 0..rounds {
        let Some(target) = g.sim.player_explored().into_iter().max_by_key(|c| c.row) else {
            break;
        };
        if Some(target) == last || !g.sim.player_travel_to(target) {
            break; // the frontier has stopped advancing
        }
        last = Some(target);
        for _ in 0..400 {
            if !g.sim.player_traveling() {
                break;
            }
            let t = std::time::Instant::now();
            g.sim.step();
            let d = t.elapsed();
            tot += d;
            if d > mx {
                mx = d;
            }
            steps += 1;
        }
    }
    let avg_ms = if steps > 0 {
        tot.as_secs_f64() * 1000.0 / steps as f64
    } else {
        0.0
    };
    eprintln!(
        "[WALK] {steps} sim.step()s · avg {:.3} ms · max {:.3} ms · explored now {}",
        avg_ms,
        mx.as_secs_f64() * 1000.0,
        g.sim.player_explored_count()
    );
    if let Some(p) = g.sim.player_position() {
        g.avatar_pos = p;
    }
}

/// Dev hook: with `ACHLYDESA_TALKPICK` set, seed the who-to-talk-to chooser with a few souls once,
/// so the chooser can be screenshotted headlessly.
fn dev_talk_pick(mut game: NonSendMut<Game>) {
    if std::env::var("ACHLYDESA_TALKPICK").is_err()
        || game.talk_choices.is_some()
        || game.convo.is_some()
    {
        return;
    }
    let some: Vec<Entity> = game.sim.npcs().into_iter().take(4).collect();
    if !some.is_empty() {
        game.talk_choices = Some(some);
    }
}

/// Compose the character sheet for whoever the Character tab is focused on — the avatar by default,
/// or a companion when their portrait was clicked (`sheet_subject`). Read straight off the sim API,
/// the same data NPCs carry, so what the sheet shows is exactly what the rules act on.
fn sheet_text(g: &Game) -> String {
    let avatar = g.sim.player_avatar();
    // The subject: the clicked portrait's soul if it's still the avatar or in the party, else the
    // avatar (so a sheet left open on a companion who has left falls back gracefully).
    let subject = match g.sheet_subject {
        Some(e) if Some(e) == avatar || g.sim.party_roster().contains(&e) => Some(e),
        _ => avatar,
    };
    let Some(e) = subject else {
        return "no character".into();
    };
    let is_avatar = Some(e) == avatar;

    let mut s = format!("— {} —\n", g.sim.display_name(e).to_uppercase());
    if let Some(arch) = g.sim.archetype_of(e) {
        s.push_str(arch);
        s.push('\n');
    }
    s.push_str(if is_avatar {
        "you, the wanderer\n"
    } else {
        "a companion in your party\n"
    });
    // Attributes — score and modifier, three to a row.
    if let Some(ab) = g.sim.abilities_of(e) {
        const NAMES: [&str; 6] = ["STR", "DEX", "CON", "INT", "WIS", "CHA"];
        s.push('\n');
        for i in 0..6 {
            s.push_str(&format!(
                "{} {:>2} ({:+})",
                NAMES[i],
                ab.scores[i],
                ab.modifier(i)
            ));
            s.push_str(if i % 3 == 2 { "\n" } else { "   " });
        }
    }
    // Trained skills (rank ≥ 0), tagged by interaction class (talk / world).
    if let Some(data) = g.sim.rpg_data() {
        let trained: Vec<String> = data
            .skills()
            .iter()
            .filter_map(|sk| {
                let rank = g.sim.proficiency_of(e, &sk.name)?;
                (rank >= 0).then(|| {
                    let tag = if sk.social {
                        " talk"
                    } else if sk.world {
                        " world"
                    } else {
                        ""
                    };
                    format!("{} +{}{}", sk.name, rank, tag)
                })
            })
            .collect();
        if !trained.is_empty() {
            s.push_str("\nSkills: ");
            s.push_str(&trained.join(", "));
            s.push('\n');
        }
    }
    // Survival vitals (only present when survival is on); gear lives on the Inventory tab.
    if let Some(v) = g.sim.vitals_of(e) {
        s.push_str(&format!(
            "\nThirst {:.0}   Warmth {:.0}   Stamina {:.0}\n",
            v.thirst, v.warmth, v.stamina
        ));
    }
    // The party roster — only on the avatar's own sheet (the party is the avatar's).
    if is_avatar {
        let roster = g.sim.party_roster();
        s.push_str(&format!("\n— PARTY ({}) —\n", roster.len()));
        if roster.is_empty() {
            s.push_str("(none — stand by a soul and press R to recruit)\n");
        } else {
            for m in &roster {
                let arch = g
                    .sim
                    .archetype_of(*m)
                    .map(|a| format!("  ({a})"))
                    .unwrap_or_default();
                s.push_str(&format!("• {}{}\n", g.sim.display_name(*m), arch));
            }
            s.push_str("\n(click a portrait to view that companion)\n");
        }
    }
    s
}

/// The **Inventory** tab — the avatar's satchel, the callings it has learned, and its carried gear,
/// read straight off the sim. The satchel and callings grow by *using* the world's POIs (forage at
/// the wilds, apprentice at a guild — the Use verb).
fn inventory_text(g: &Game) -> String {
    let mut s = String::from("— INVENTORY —\n\n");

    // The crafts economy: goods gathered at Yield sites, and the callings learned at guilds.
    let goods = g.sim.player_goods();
    if goods.is_empty() {
        s.push_str("Satchel: empty — gather at the wilds and the crafts (Use, by a feature)\n");
    } else {
        s.push_str("Satchel:\n");
        for (name, n) in &goods {
            s.push_str(&format!("  • {} \u{00d7}{}\n", ui::pretty(name), n));
        }
    }
    let callings = g.sim.player_callings();
    s.push('\n');
    if callings.is_empty() {
        s.push_str("Callings: none — apprentice at a guild to learn a craft\n");
    } else {
        s.push_str("Callings:\n");
        for (name, lvl) in &callings {
            s.push_str(&format!("  • {} ({:.2})\n", ui::pretty(name), lvl));
        }
    }

    let gear = g.sim.player_gear();
    s.push('\n');
    if gear.is_empty() {
        s.push_str("Gear: you carry nothing of note\n");
    } else {
        s.push_str(&format!("Gear ({}):\n", gear.len()));
        for it in &gear {
            s.push_str(&format!("  • {}\n", ui::pretty(it)));
        }
    }
    s
}

/// The trait/mood vocabulary, mirrored from the sim's data so the app can read an NPC's
/// disposition for the character card without reaching into the agents crate. (name, descriptor.)
const TRAIT_WORDS: &[(&str, &str)] = &[
    ("vengeance", "vengeful"),
    ("ambition", "ambitious"),
    ("greed", "grasping"),
    ("piety", "devout"),
    ("sociability", "warm-hearted"),
    ("forgiveness", "forgiving"),
    ("caution", "wary"),
    ("contentment", "content"),
];
const MOOD_WORDS: &[(&str, &str)] = &[
    ("anger", "seething with anger"),
    ("sorrow", "deep in sorrow"),
    ("fear", "afraid"),
    ("joy", "glad"),
    ("love", "tender"),
    ("hope", "hopeful"),
    ("awe", "awestruck"),
    ("calm", "at ease"),
];

/// "a", "a and b", "a, b, and c".
fn join_with_and(parts: &[&str]) -> String {
    match parts {
        [] => String::new(),
        [a] => a.to_string(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// Free-text → authored intent → flavour, the conversation's **social effect**. Field 1 is the
/// stem matched (case-insensitive) in the classifier's one-word answer; field 2 is the intent
/// whose authored `moves` are applied (speaker = avatar, listener = NPC); field 3 is the status
/// shown. Plain social acts only — the lore-specific intents (cult/gnosis) are left out of the
/// player's reach. The matching stems avoid collisions (e.g. "prais" vs "plea").
const EFFECTS: &[(&str, &str, &str)] = &[
    ("greet", "a_greeting", "warms to you a little"),
    ("prais", "a_word_of_praise", "is pleased by your words"),
    ("confid", "a_confidence_shared", "trusts you a little more"),
    ("consol", "a_consolation", "takes some comfort from you"),
    ("reconcil", "an_overture_of_peace", "softens toward you"),
    ("plea", "a_plea", "marks your appeal"),
    ("accus", "an_accusation", "bristles, and thinks less of you"),
    ("threat", "a_threat", "fears you now, and likes you less"),
    ("dismiss", "a_cold_dismissal", "grows colder toward you"),
    ("boast", "a_boast", "notes your boasting"),
    ("gossip", "an_idle_rumour", "trades the rumour with you"),
    ("mourn", "a_grief_spoken", "shares in the grief"),
];
/// The labels offered to the classifier (full words; matched by the stems above).
const EFFECT_LABELS: &[&str] = &[
    "greet",
    "praise",
    "confide",
    "console",
    "reconcile",
    "plead",
    "accuse",
    "threaten",
    "dismiss",
    "boast",
    "gossip",
    "mourn",
];

/// Bin a soul's opinion of the player (`-1..1`) into a disposition phrase for the tab header,
/// so the player can watch it move as the conversation lands effects. Mirrors the sim's own
/// opinion bins (loathes/resents/wary/warms/devoted), phrased toward "you".
fn disposition_word(op: f32) -> &'static str {
    match op {
        x if x < -0.4 => "hostile to you",
        x if x < -0.1 => "resents you",
        x if x < 0.1 => "wary of you",
        x if x < 0.4 => "warming to you",
        _ => "devoted to you",
    }
}

/// Assemble the **character card** — the NPC's grounded state in plain prose, for the model's
/// system context. Pulls real traits, dominant mood, and any grudge from the sim, so the
/// character speaks as who it actually is in the simulation (the richer grounding).
fn npc_card(sim: &mut Simulation, npc: Entity) -> String {
    let name = sim.display_name(npc);
    let traits: Vec<&str> = TRAIT_WORDS
        .iter()
        .filter(|(t, _)| sim.trait_of(npc, t).is_some_and(|v| v > 0.55))
        .map(|(_, w)| *w)
        .take(3)
        .collect();
    let mood = MOOD_WORDS
        .iter()
        .filter_map(|(m, w)| sim.mood_of(npc, m).map(|v| (v, *w)))
        .filter(|(v, _)| *v > 0.15)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, w)| w);
    let bears_grudge = sim
        .player_avatar()
        .is_some_and(|me| sim.grudges().iter().any(|(h, t)| *h == npc && *t == me));

    let mut card = format!("You are {name}.");
    if traits.is_empty() {
        card.push_str(" You are an ordinary soul of few strong leanings.");
    } else {
        card.push_str(&format!(" By nature you are {}.", join_with_and(&traits)));
    }
    if let Some(m) = mood {
        card.push_str(&format!(" Right now you are {m}."));
    }
    if bears_grudge {
        card.push_str(" You bear an old grudge against this traveller, and it colours every word.");
    } else {
        card.push_str(" You are speaking with a wandering stranger you do not yet know.");
    }
    card
}

/// Open a free-text conversation with the nearest soul in reach (shared by T and the Talk button).
/// Assembles the character card from real sim state and lets the soul speak first.
fn start_talk(g: &mut Game) {
    if g.sim.player_traveling() || g.paused || g.convo.is_some() || g.combat.is_some() {
        return;
    }
    let nearby: Vec<Entity> = g
        .sim
        .player_nearby_npcs()
        .into_iter()
        .map(|(e, _)| e)
        .collect();
    match nearby.len() {
        0 => g.status = "There is no one close enough to speak with.".into(),
        // One soul in reach — speak to them straight away.
        1 => open_conversation_with(g, nearby[0]),
        // Several — let the player choose (the conversation panel's chooser).
        _ => {
            g.status = "Choose who to speak with.".into();
            g.talk_choices = Some(nearby);
        }
    }
}

/// Open a conversation with a specific soul (the chosen one, from [`start_talk`]; any soul, for the
/// `ACHLYDESA_CONVO` dev hook). Assembles the card and lets the soul speak first.
fn open_conversation_with(g: &mut Game, npc: Entity) {
    // Remember the meeting for the ledger (the Journal tab's who's-who of acquaintances).
    if !g.met.contains(&npc) {
        g.met.push(npc);
    }
    let name = g.sim.display_name(npc);
    // The director's investment shows on the name: a thread's figure is met not as "a villager" but
    // as "Aldric, the Betrayed" — the arc made legible the moment you face them.
    let titled = match g.sim.npc_epithet(npc) {
        Some(ep) => format!("{name}, {ep}"),
        None => name.clone(),
    };
    let card = npc_card(&mut g.sim, npc);
    // The soul speaks first. The director's drama leads when there is any: the soul's own lately-
    // forced words (a `Voice` beat, heard where it lands), or a short line naming its plight — so
    // meeting a thread's figure opens on their story. Else the voice model's scene-cued opening, or
    // a neutral one so the deterministic speak choices stay reachable without the model.
    let greeting = if let Some(voiced) = g.sim.npc_voiced_line(npc) {
        Line {
            from_player: false,
            prefix: format!("{name}: "),
            text: Some(voiced),
            reveal: 0.0,
            pending: None,
        }
    } else if let Some(sit) = g.sim.npc_situation(npc) {
        Line {
            from_player: false,
            prefix: String::new(),
            text: Some(format!("{name} regards you, {sit}")),
            reveal: 0.0,
            pending: None,
        }
    } else if g.voice.is_ready() {
        g.req_seq += 1;
        let req = g.req_seq;
        let fallback = format!("{name} regards you in silence.");
        let dispatched = g.voice.request_chat(
            req,
            &card,
            &[],
            "(A stranger approaches and meets your eyes.)",
            &fallback,
        );
        Line {
            from_player: false,
            prefix: format!("{name}: "),
            text: if dispatched { None } else { Some(fallback) },
            reveal: 0.0,
            pending: dispatched.then_some(req),
        }
    } else {
        Line {
            from_player: false,
            prefix: format!("{name}: "),
            text: Some(format!("{name} meets your eyes, and waits.")),
            reveal: 0.0,
            pending: None,
        }
    };
    let mut transcript = vec![greeting];
    // The soul shares what word has reached it — a recent beat the director staged, sharp or vague
    // by how near and recent it was (the fidelity veil) — when there is news worth the telling.
    if let Some(gossip) = g.sim.overheard() {
        transcript.push(Line {
            from_player: false,
            prefix: String::new(),
            text: Some(format!("\u{2014} Word's about: {gossip}")),
            reveal: 0.0,
            pending: None,
        });
    }
    // A charge from a thread's figure — the director's drama offered as a goal. Only if this soul
    // leads a live thread and we have not already taken (or closed) its charge; spoken here.
    let offer = g.sim.quest_for(npc).filter(|q| {
        !g.quests.iter().any(|a| a.giver == npc) && !g.quest_done_pairs.contains(&(npc, q.other))
    });
    if let Some(q) = &offer {
        transcript.push(Line {
            from_player: false,
            prefix: format!("{name}: "),
            text: Some(q.request.clone()),
            reveal: 0.0,
            pending: None,
        });
    }
    g.convo = Some(Convo {
        listener: npc,
        name: titled,
        card,
        transcript,
        input: String::new(),
        offer,
    });
    g.status = format!("You fall into talk with {name}.");
}

/// **Talk** — press **T** by a soul to open a free-text conversation, then *type* to it;
/// **Enter** sends, **Esc** leaves. The soul answers in its own voice, generated from its real
/// sim state (the card) and the exchange so far. Needs the voice model — these are the
/// character's own words — and the world pauses while you talk.
fn talk_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut kb: MessageReader<KeyboardInput>,
    mut game: NonSendMut<Game>,
) {
    let g = &mut *game;

    // Not yet talking: T opens a conversation with the nearest soul in reach (idle only).
    if g.convo.is_none() {
        kb.clear(); // discard stray keystrokes (incl. the opening T) so they don't leak into the box
        if g.sim.player_traveling() || g.paused || !keys.just_pressed(KeyCode::KeyT) {
            return;
        }
        start_talk(g);
        return;
    }

    // In a conversation: edit the input line from raw key events; Enter sends, Esc leaves.
    let mut submit = false;
    let mut leave = false;
    for ev in kb.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        let Some(c) = g.convo.as_mut() else { break };
        match &ev.logical_key {
            Key::Character(s) => c.input.push_str(s.as_str()),
            Key::Space => c.input.push(' '),
            Key::Backspace => {
                c.input.pop();
            }
            Key::Enter => submit = true,
            Key::Escape => leave = true,
            _ => {}
        }
    }

    if leave {
        let name = g.convo.as_ref().map(|c| c.name.clone()).unwrap_or_default();
        g.convo = None;
        g.status = format!("You take your leave of {name}.");
        return;
    }
    if !submit {
        return;
    }

    // Send the typed line. Wait if the soul is still answering, so its reply stays in the
    // history we hand the model. The player's words show at once; the soul then considers.
    let (card, history, name, msg, npc) = {
        let Some(c) = g.convo.as_mut() else { return };
        if c.transcript.iter().any(|l| l.pending.is_some()) {
            return;
        }
        let msg = c.input.trim().to_string();
        if msg.is_empty() {
            return;
        }
        c.input.clear();
        let history: Vec<(bool, String)> = c
            .transcript
            .iter()
            .filter_map(|l| l.text.as_ref().map(|t| (l.from_player, t.clone())))
            .collect();
        (c.card.clone(), history, c.name.clone(), msg, c.listener)
    };

    g.req_seq += 1;
    let req = g.req_seq;
    let fallback = format!("{name} says nothing.");
    let dispatched = g.voice.request_chat(req, &card, &history, &msg, &fallback);

    // In parallel, classify what the player said so it can move the social state (the effect
    // lands on the NPC via the authored intent's moves; the answer is routed in `poll_voice`).
    g.req_seq += 1;
    let creq = g.req_seq;
    if g.voice
        .request_classify(creq, &name, &msg, EFFECT_LABELS, "none")
    {
        g.classify.insert(creq, npc);
    }

    if let Some(c) = g.convo.as_mut() {
        c.transcript.push(Line {
            from_player: true,
            prefix: "You: ".into(),
            text: Some(msg),
            reveal: f32::MAX,
            pending: None,
        });
        c.transcript.push(Line {
            from_player: false,
            prefix: format!("{name}: "),
            text: if dispatched { None } else { Some(fallback) },
            reveal: 0.0,
            pending: dispatched.then_some(req),
        });
        let overflow = c.transcript.len().saturating_sub(10);
        if overflow > 0 {
            c.transcript.drain(0..overflow);
        }
    }
}

/// Swap in any model-voiced lines that have finished generating, matching each by its request
/// id. Always drains the channel (so results don't pile up even after a conversation ends);
/// a result whose line is no longer on screen is simply discarded. A no-op with voice off.
fn poll_voice(mut game: NonSendMut<Game>) {
    let g = &mut *game;
    for (req_id, text) in g.voice.poll() {
        // A classification result drives the conversation's social effect on the NPC.
        if let Some(npc) = g.classify.remove(&req_id) {
            let lower = text.to_lowercase();
            if let Some((_, intent_id, flavor)) =
                EFFECTS.iter().find(|(stem, _, _)| lower.contains(stem))
                && g.sim.apply_conversational_intent(npc, intent_id)
            {
                let name = g.sim.display_name(npc);
                g.status = format!("{name} {flavor}.");
            }
            continue;
        }
        // Otherwise it's a spoken reply: the soul stops considering and types its words out.
        if let Some(c) = g.convo.as_mut()
            && let Some(line) = c.transcript.iter_mut().find(|l| l.pending == Some(req_id))
        {
            line.text = Some(text);
            line.reveal = 0.0;
            line.pending = None;
        }
    }
}

/// Advance the per-line typewriter each frame (frame-rate independent). Lines still awaiting a
/// voiced line (`text == None`) don't advance — they show the animated ellipsis instead.
fn tick_typewriter(time: Res<Time>, mut game: NonSendMut<Game>) {
    let g = &mut *game;
    let Some(c) = g.convo.as_mut() else { return };
    let step = time.delta_secs() * REVEAL_CPS;
    for line in c.transcript.iter_mut() {
        if line.text.is_some() {
            line.reveal += step;
        }
    }
}

// =====================================================================================
// Rendering the world
// =====================================================================================

/// Dress each tile revealed this frame with its trees, scrub, and rock (one-shot per tile).
fn scatter_props(
    mut commands: Commands,
    lib: Res<props::PropLibrary>,
    game: NonSend<Game>,
    ex: Res<ground::Explored>,
) {
    scatter::decorate_fresh(&mut commands, &lib, &game.sim, &ex.fresh);
}

/// Raise the buildings for features as the player discovers them (settlements, courts, ruins) — on
/// the freshly-revealed tiles, plus the avatar's own tile so a feature uncovered by *searching* an
/// already-explored tile gets built too.
fn build_features(
    mut commands: Commands,
    lib: Res<props::PropLibrary>,
    game: NonSend<Game>,
    ex: Res<ground::Explored>,
    mut built: ResMut<feature_art::Built>,
) {
    let tiles = ex
        .fresh
        .iter()
        .copied()
        .chain(std::iter::once(game.avatar_pos));
    feature_art::build_on(&mut commands, &lib, &game.sim, &mut built, tiles);
}

/// A reused pool of NPC marker entities — repositioned each world tick (never despawned/respawned),
/// so a peopled, well-explored map doesn't churn hundreds of entities per step.
#[derive(Resource, Default)]
struct NpcMarkers(Vec<Entity>);

/// Reposition the NPC markers each world tick: the populace shows only where the fog is lifted
/// (O(1) checks against the shared explored set), and markers glide in from the pool instead of
/// being torn down and rebuilt.
fn rebuild_markers(
    mut commands: Commands,
    ra: Res<RenderAssets>,
    ex: Res<ground::Explored>,
    mut pool: ResMut<NpcMarkers>,
    mut game: NonSendMut<Game>,
    mut q: Query<(&mut Transform, &mut Visibility), With<Marker>>,
) {
    let tick = game.sim.substrate().tick();
    if tick == game.last_tick {
        return;
    }
    game.last_tick = tick;
    let g = &mut *game;
    // The avatar is a persistent figure (`AvatarFig`), glided by `smooth_follow` — not handled here.
    let npcs = g.sim.npc_positions();
    let positions: Vec<Vec3> = npcs
        .into_iter()
        .filter(|c| ex.set.contains(&(c.col, c.row)))
        .map(|c| {
            let top = tile_top(g.sim.substrate(), c);
            let w = tile_world(c.col, c.row);
            Vec3::new(w.x, top + 0.3, w.y)
        })
        .collect();
    // Place each visible NPC on a pooled marker, growing the pool as the map fills; hide the rest.
    for (i, &pos) in positions.iter().enumerate() {
        if let Some(&e) = pool.0.get(i) {
            if let Ok((mut tf, mut vis)) = q.get_mut(e) {
                tf.translation = pos;
                *vis = Visibility::Inherited;
            }
        } else {
            let e = commands
                .spawn((
                    Marker,
                    Mesh3d(ra.npc_mesh.clone()),
                    MeshMaterial3d(ra.npc_mat.clone()),
                    Transform::from_translation(pos),
                ))
                .id();
            pool.0.push(e);
        }
    }
    for i in positions.len()..pool.0.len() {
        if let Ok((_, mut vis)) = q.get_mut(pool.0[i]) {
            *vis = Visibility::Hidden;
        }
    }
}

/// Reconcile the rendered creatures with the simulation's census each world tick:
/// re-target survivors to their new tiles (the animation walks them there), spawn the
/// newly-seen, and despawn the dead or those that slipped back into the fog. Only
/// fauna on explored tiles are drawn, so creatures fade in as the land is uncovered.
fn sync_fauna(
    mut commands: Commands,
    art: Res<fauna_art::FaunaArt>,
    ex: Res<ground::Explored>,
    mut game: NonSendMut<Game>,
    mut existing: Query<(Entity, &mut fauna_art::Fauna)>,
) {
    let tick = game.sim.substrate().tick();
    if tick == game.last_fauna_tick {
        return;
    }
    game.last_fauna_tick = tick;
    let g = &mut *game;

    // id → (species, world ground position), explored tiles only (O(1) checks, no per-tick rescan).
    let mut want: HashMap<u64, (usize, Vec3)> = HashMap::new();
    for (id, sp, c) in g.sim.fauna_census() {
        if !ex.set.contains(&(c.col, c.row)) {
            continue;
        }
        let top = tile_top(g.sim.substrate(), c);
        let w = tile_world(c.col, c.row);
        want.insert(id, (sp, Vec3::new(w.x, top + 0.15, w.y)));
    }

    // Re-target survivors; despawn the gone.
    for (e, mut f) in &mut existing {
        if let Some((_, pos)) = want.remove(&f.id) {
            f.target = pos;
        } else {
            commands.entity(e).despawn();
        }
    }
    // Spawn the newly-seen, each with the mesh of its species.
    for (id, (sp, pos)) in want {
        let form = g.sim.bestiary().species[sp].form;
        fauna_art::spawn_creature(&mut commands, &art, id, sp, form, pos);
    }
}

// =====================================================================================
// Camera (orbits the avatar) and HUD
// =====================================================================================

fn cam_transform(focus: Vec3, rig: &CamRig) -> Transform {
    let rot = Quat::from_axis_angle(Vec3::Y, rig.yaw) * Quat::from_axis_angle(Vec3::X, -rig.pitch);
    Transform::from_translation(focus + rot * (Vec3::Z * rig.dist)).looking_at(focus, Vec3::Y)
}

/// Glide the avatar figure — and the focus the camera orbits — toward the tile the avatar stands
/// on, easing a little each frame. The avatar's *true* position is the discrete tile (that's the
/// gameplay); this is purely the smoothed render position, so a walk reads as a glide rather than a
/// hex-by-hex jump.
fn smooth_follow(
    time: Res<Time>,
    mut game: NonSendMut<Game>,
    mut fig: Query<&mut Transform, With<AvatarFig>>,
) {
    let g = &mut *game;
    let target = {
        let aw = tile_world(g.avatar_pos.col, g.avatar_pos.row);
        Vec3::new(aw.x, tile_top(g.sim.substrate(), g.avatar_pos) + 1.2, aw.y)
    };
    // Exponential smoothing, ~0.09 s time constant: tight enough to keep up with the walk, loose
    // enough to glide. `1 - e^(-dt/τ)` makes the ease frame-rate-independent.
    let k = (1.0 - (-time.delta_secs() / 0.09).exp()).clamp(0.0, 1.0);
    g.avatar_render = g.avatar_render.lerp(target, k);
    if let Ok(mut tf) = fig.single_mut() {
        tf.translation = g.avatar_render;
    }
}

fn camera_control(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    game: NonSend<Game>,
    mut q: Query<(&mut CamRig, &mut Transform, &mut Projection)>,
) {
    // While typing in a conversation or paused, the letter keys belong elsewhere, not the camera.
    if game.convo.is_some() || game.paused {
        return;
    }
    let Ok((mut rig, mut tf, mut proj)) = q.single_mut() else {
        return;
    };
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
        // Under orthographic projection, moving the camera in/out wouldn't change apparent size, so
        // zoom is the projection's vertical extent: scroll up shrinks it (zooms in).
        rig.zoom = (rig.zoom * (1.0 - scroll.delta.y * 0.12)).clamp(6.0, 110.0);
        if let Projection::Orthographic(ortho) = &mut *proj {
            ortho.scale = rig.zoom;
        }
    }
    let f = game.avatar_render;
    *tf = cam_transform(Vec3::new(f.x, 0.0, f.z), &rig);
}

/// Check the avatar's taken **charges** each frame: a charge is fulfilled when the other is reached
/// (or gone), or moot when its giver dies. Closed charges are announced and move to the journal.
fn update_quests(mut game: NonSendMut<Game>) {
    let g = &mut *game;
    if g.quests.is_empty() {
        return;
    }
    // Decide which are closed (read-only over the sim), then prune from the back and announce.
    let mut closed: Vec<(usize, String, Entity, Entity)> = Vec::new();
    for (i, q) in g.quests.iter().enumerate() {
        let line = if !g.sim.quest_giver_alive(q) {
            format!(
                "{} — the charge passes ({} is gone)",
                q.objective, q.giver_name
            )
        } else if g.sim.quest_reached(q) {
            format!("{} — {} found", q.objective, q.other_name)
        } else if !g.sim.quest_thread_open(q) {
            // The director resolved the drama before the avatar arrived — the matter is settled.
            format!(
                "{} — the matter is settled, {}'s reckoning come and gone",
                q.objective, q.giver_name
            )
        } else {
            continue;
        };
        closed.push((i, line, q.giver, q.other));
    }
    if closed.is_empty() {
        return;
    }
    for &(_, _, giver, other) in &closed {
        g.quest_done_pairs.insert((giver, other)); // the giver won't re-offer this charge
    }
    for &(i, _, _, _) in closed.iter().rev() {
        g.quests.remove(i);
    }
    if let Some((_, last, _, _)) = closed.last() {
        g.status = format!("Charge fulfilled \u{2014} {last}");
    }
    for (_, line, _, _) in closed {
        g.done_quests.push(line);
    }
}

fn update_hud(
    mut game: NonSendMut<Game>,
    mut texts: Query<(&HudKind, &mut Text, &mut Visibility)>,
) {
    let g = &mut *game;
    let view = g.sim.player_view();
    let day = g.sim.substrate().tick();
    let explored = g.sim.player_explored_count();
    let traveling = g.sim.player_traveling();
    let in_convo = g.convo.is_some() || g.talk_choices.is_some();
    let status = g.status.clone();
    let voice_line = g.voice.status_line();
    // The lure: does the ground here hold something to find?
    let search_cue = match (traveling, g.sim.player_find_state()) {
        (false, FindState::Findable) => "\n  you sense something here — press F to search",
        (false, FindState::Locked) => "\n  something here eludes you — you lack the knowledge",
        _ => "",
    };
    // The world's current drama, pushed at the player as it moves (hidden under the menu / a talk).
    let tidings = if g.paused || in_convo {
        None
    } else {
        g.sim.tidings()
    };
    // The avatar's taken charges, as objective lines with a bearing to the soul to find.
    let charges: String = g
        .quests
        .iter()
        .map(|q| format!("\nCharge: {} {}", q.objective, g.sim.quest_bearing(q)))
        .collect();

    for (kind, mut text, mut vis) in &mut texts {
        text.0 = match kind {
            // The tile read-out: hidden under the pause menu and behind the conversation panel.
            HudKind::Look => {
                *vis = if g.paused || in_convo {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                };
                match &view {
                    Some(v) => {
                        let feats = if v.here.features.is_empty() {
                            String::new()
                        } else {
                            format!("\nyou see: {}", v.here.features.join(", "))
                        };
                        format!(
                            "Day {day}\n({}, {})  {}  {:.0} m\nfertile {:.2}   {} soul(s) near\nfog lifted from {} tiles{}{}{}",
                            v.pos.col,
                            v.pos.row,
                            v.here.terrain.name(),
                            v.here.elevation,
                            v.here.fertility,
                            v.nearby.len(),
                            explored,
                            feats,
                            search_cue,
                            charges,
                        )
                    }
                    None => "no avatar".into(),
                }
            }
            // A single status line for the bottom tray; verbs live on the action buttons, camera
            // on A/D/W/S + scroll.
            HudKind::Help => {
                *vis = if g.paused {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                };
                let mut h = status.clone();
                if let Some(v) = &voice_line {
                    h.push_str("   ·   ");
                    h.push_str(v);
                }
                h
            }
            // The narrative banner: shown only when the world has something to say (else hidden, so
            // the centre stays clear).
            HudKind::Tidings => match &tidings {
                Some(t) => {
                    *vis = Visibility::Inherited;
                    t.clone()
                }
                None => {
                    *vis = Visibility::Hidden;
                    String::new()
                }
            },
        };
    }
}
