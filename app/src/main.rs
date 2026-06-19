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
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

mod fauna_art;
mod feature_art;
mod layout;
mod mesh;
mod palette;
mod props;
mod scatter;
mod ui;
mod world_mesh;

use layout::{tile_top, tile_world};

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
        pub fn request_chat(&self, req_id: u64, card: &str, history: &[(bool, String)], player_msg: &str, fallback: &str) -> bool {
            let turns: Vec<voice::ChatTurn> =
                history.iter().map(|(from_player, text)| voice::ChatTurn { from_player: *from_player, text: text.clone() }).collect();
            self.0.request_chat(req_id, card, &turns, player_msg, fallback)
        }
        /// Classify what the player said into one of `labels` (for the social effect).
        pub fn request_classify(&self, req_id: u64, name: &str, message: &str, labels: &[&str], fallback: &str) -> bool {
            self.0.request_classify(req_id, name, message, labels, fallback)
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
                VoiceStatus::Failed(e) => Some(format!("voice: off ({})", e.lines().next().unwrap_or("failed"))),
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
        pub fn request_chat(&self, _req_id: u64, _card: &str, _history: &[(bool, String)], _player_msg: &str, _fallback: &str) -> bool {
            false
        }
        pub fn request_classify(&self, _req_id: u64, _name: &str, _message: &str, _labels: &[&str], _fallback: &str) -> bool {
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
    last_explored: usize,
    last_tick: u64,
    /// The world tick the rendered fauna were last synced to (so creatures only
    /// re-target when the world has actually moved).
    last_fauna_tick: u64,
    accum: f32,
    status: String,
    convo: Option<Convo>,
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
    /// Is the discoveries journal open?
    journal_open: bool,
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
        last_fauna_tick: u64::MAX,
        accum: 0.0,
        status: "Welcome. Click a tile to set out - the world moves when you do.".into(),
        convo: None,
        voice: voice_bridge::Bridge::spawn(),
        req_seq: 0,
        classify: HashMap::new(),
        hovered: None,
        selected: None,
        journal_open: false,
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Achlydesa — exploration".into(), ..default() }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(palette::SKY_RGB[0], palette::SKY_RGB[1], palette::SKY_RGB[2])))
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
            journal_input,
            ui::tile_interact,
            rebuild_map,
            scatter_props,
            build_features,
            rebuild_markers,
            camera_control,
            ui::update_highlights,
            ui::update_tooltip,
            ui::update_inspect,
            ui::update_labels,
            ui::update_journal,
            update_hud,
        )
            .chain(),
    )
    // The fauna layer runs as its own group: `sync_fauna` re-targets creatures when
    // the world ticks (idempotent in between, so loose ordering is fine) and
    // `animate_fauna` is purely visual.
    .add_systems(Update, (sync_fauna, fauna_art::animate_fauna).chain());
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
    // The app owns world generation. Build the large, US-scale `game_sim` world here —
    // ~1 hex ≈ a day's walk, so crossing the main landmass is multiple months on foot;
    // few tectonic plates raise a handful of huge continents, and the wider uplift falloff
    // keeps mountain belts from thinning to ribbons at this scale — then hand it to the
    // agent simulation via `from_world`. Worldgen itself lives in `game_sim`; `agents` only
    // drives the substrate it is given. (Starting values — tune to taste.)
    let (width, height, seed) = (192, 144, 7);
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
            fauna: 1000,
            carnivores: 200,
            npcs: 300,
            markets: 12,
            markets_on_settlements: true,
            dialogue: true,
            // The RPG, party and exploration layers are on for the game: the avatar and every NPC
            // roll Worlds-Without-Number stats; the avatar can recruit companions who travel as a
            // stack; and travel is cost-paced over a road network with terrain/elevation gates.
            // Survival stays OFF until NPCs can seek water/shelter (see the design doc) — else the
            // mostly-arid world depopulates.
            rpg: true,
            party: true,
            exploration: true,
            goals,
            registry: reg,
            ..Default::default()
        },
    )
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
    // The procedural prop library (trees, scrub, rock) shares the map's matte vertex-colour
    // material; the meshes carry their own colour.
    let prop_lib = props::build_library(&mut meshes, map_mat.clone());
    commands.insert_resource(prop_lib);
    // One procedural creature mesh per species, sharing the matte vertex-colour material.
    let fauna_art = fauna_art::build_fauna_art(&mut meshes, map_mat.clone(), game.sim.bestiary());
    commands.insert_resource(fauna_art);
    commands.init_resource::<scatter::Decorated>();
    commands.init_resource::<feature_art::Built>();
    commands.insert_resource(RenderAssets {
        map_mat,
        avatar_mesh: meshes.add(Capsule3d::new(0.5, 1.8)),
        avatar_mat,
        npc_mesh: meshes.add(Cylinder::new(0.2, 0.55)),
        npc_mat,
    });

    let aw = tile_world(game.avatar_pos.col, game.avatar_pos.row);
    let rig = CamRig { dist: 42.0, yaw: 0.0, pitch: 0.92 };
    let fog = palette::FOG_RGB;
    commands.spawn((
        Camera3d::default(),
        cam_transform(Vec3::new(aw.x, 0.0, aw.y), &rig),
        rig,
        // A cool ambient and a pale distance haze — the dream half-drowned in fog.
        AmbientLight { brightness: 200.0, color: Color::srgb(0.72, 0.79, 0.9), ..default() },
        DistanceFog { color: Color::srgb(fog[0], fog[1], fog[2]), falloff: FogFalloff::Linear { start: 70.0, end: 300.0 }, ..default() },
    ));

    commands.spawn((
        DirectionalLight { illuminance: 6200.0, shadows_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.6, -0.95, 0.0)),
    ));

    ui::setup_ui(&mut commands, &mut meshes, &mut materials);
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

/// **Search** — press **F** to search the tile you stand on. Reveals the hidden things here you
/// have the knowledge to find, and you learn whatever lore they hold. A locked place tells you
/// it is there but withholds itself until you know more. One action, one tick (like waiting).
fn search_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.convo.is_some() || !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    let g = &mut *game;
    if g.sim.player_traveling() {
        return;
    }
    let out = g.sim.player_search();
    if out.is_empty() {
        g.status = match g.sim.player_find_state() {
            FindState::Locked => "Something is hidden here, but you lack the knowledge to find it.".into(),
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

/// **Journal** — press **J** to open or close the discoveries journal (what you've found and the
/// lore you hold). A look at the world, not an action: time does not pass.
fn journal_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.convo.is_none() && keys.just_pressed(KeyCode::KeyJ) {
        game.journal_open = !game.journal_open;
    }
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
const EFFECT_LABELS: &[&str] =
    &["greet", "praise", "confide", "console", "reconcile", "plead", "accuse", "threaten", "dismiss", "boast", "gossip", "mourn"];

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
    let traits: Vec<&str> =
        TRAIT_WORDS.iter().filter(|(t, _)| sim.trait_of(npc, t).is_some_and(|v| v > 0.55)).map(|(_, w)| *w).take(3).collect();
    let mood = MOOD_WORDS
        .iter()
        .filter_map(|(m, w)| sim.mood_of(npc, m).map(|v| (v, *w)))
        .filter(|(v, _)| *v > 0.15)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, w)| w);
    let bears_grudge = sim.player_avatar().is_some_and(|me| sim.grudges().iter().any(|(h, t)| *h == npc && *t == me));

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

/// **Talk** — press **T** by a soul to open a free-text conversation, then *type* to it;
/// **Enter** sends, **Esc** leaves. The soul answers in its own voice, generated from its real
/// sim state (the card) and the exchange so far. Needs the voice model — these are the
/// character's own words — and the world pauses while you talk.
fn talk_input(keys: Res<ButtonInput<KeyCode>>, mut kb: MessageReader<KeyboardInput>, mut game: NonSendMut<Game>) {
    let g = &mut *game;

    // Not yet talking: T opens a conversation with the nearest soul in reach (idle only).
    if g.convo.is_none() {
        kb.clear(); // discard stray keystrokes (incl. the opening T) so they don't leak into the box
        if g.sim.player_traveling() || !keys.just_pressed(KeyCode::KeyT) {
            return;
        }
        let Some((npc, _)) = g.sim.player_nearby_npcs().into_iter().next() else {
            g.status = "There is no one close enough to speak with.".into();
            return;
        };
        if !g.voice.is_ready() {
            g.status = "The voice is still waking - conversation needs the model loaded.".into();
            return;
        }
        let name = g.sim.display_name(npc);
        let card = npc_card(&mut g.sim, npc);
        // The soul speaks first: an opening line generated from a scene cue (not shown verbatim).
        g.req_seq += 1;
        let req = g.req_seq;
        let fallback = format!("{name} regards you in silence.");
        let dispatched = g.voice.request_chat(req, &card, &[], "(A stranger approaches and meets your eyes.)", &fallback);
        let greeting = Line {
            from_player: false,
            prefix: format!("{name}: "),
            text: if dispatched { None } else { Some(fallback) },
            reveal: 0.0,
            pending: dispatched.then_some(req),
        };
        g.convo = Some(Convo { listener: npc, name: name.clone(), card, transcript: vec![greeting], input: String::new() });
        g.status = format!("You fall into talk with {name}.");
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
        let history: Vec<(bool, String)> = c.transcript.iter().filter_map(|l| l.text.as_ref().map(|t| (l.from_player, t.clone()))).collect();
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
    if g.voice.request_classify(creq, &name, &msg, EFFECT_LABELS, "none") {
        g.classify.insert(creq, npc);
    }

    if let Some(c) = g.convo.as_mut() {
        c.transcript.push(Line { from_player: true, prefix: "You: ".into(), text: Some(msg), reveal: f32::MAX, pending: None });
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
            if let Some((_, intent_id, flavor)) = EFFECTS.iter().find(|(stem, _, _)| lower.contains(stem))
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
    let mesh = world_mesh::build_ground_mesh(&game.sim);
    commands.spawn((MapMesh, Mesh3d(meshes.add(mesh)), MeshMaterial3d(ra.map_mat.clone()), Transform::IDENTITY));
}

/// Dress each newly-revealed tile with its trees, scrub, and rock (one-shot per tile).
fn scatter_props(mut commands: Commands, lib: Res<props::PropLibrary>, game: NonSend<Game>, mut done: ResMut<scatter::Decorated>) {
    scatter::decorate_newly_explored(&mut commands, &lib, &game.sim, &mut done);
}

/// Raise the buildings for features as the player discovers them (settlements, courts, ruins).
fn build_features(mut commands: Commands, lib: Res<props::PropLibrary>, game: NonSend<Game>, mut built: ResMut<feature_art::Built>) {
    feature_art::build_discovered(&mut commands, &lib, &game.sim, &mut built);
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
    let atop = tile_top(g.sim.substrate(), ap);
    let aw = tile_world(ap.col, ap.row);
    commands.spawn((Marker, Mesh3d(ra.avatar_mesh.clone()), MeshMaterial3d(ra.avatar_mat.clone()), Transform::from_xyz(aw.x, atop + 1.2, aw.y)));
    // The populace — only where the player has been (where the fog is lifted).
    let explored: HashSet<(i32, i32)> = g.sim.player_explored().iter().map(|c| (c.col, c.row)).collect();
    for c in g.sim.npc_positions() {
        if !explored.contains(&(c.col, c.row)) {
            continue;
        }
        let top = tile_top(g.sim.substrate(), c);
        let w = tile_world(c.col, c.row);
        commands.spawn((Marker, Mesh3d(ra.npc_mesh.clone()), MeshMaterial3d(ra.npc_mat.clone()), Transform::from_xyz(w.x, top + 0.3, w.y)));
    }
}

/// Reconcile the rendered creatures with the simulation's census each world tick:
/// re-target survivors to their new tiles (the animation walks them there), spawn the
/// newly-seen, and despawn the dead or those that slipped back into the fog. Only
/// fauna on explored tiles are drawn, so creatures fade in as the land is uncovered.
fn sync_fauna(
    mut commands: Commands,
    art: Res<fauna_art::FaunaArt>,
    mut game: NonSendMut<Game>,
    mut existing: Query<(Entity, &mut fauna_art::Fauna)>,
) {
    let tick = game.sim.substrate().tick();
    if tick == game.last_fauna_tick {
        return;
    }
    game.last_fauna_tick = tick;
    let g = &mut *game;

    let explored: HashSet<(i32, i32)> = g.sim.player_explored().iter().map(|c| (c.col, c.row)).collect();
    // id → (species, world ground position), explored tiles only.
    let mut want: HashMap<u64, (usize, Vec3)> = HashMap::new();
    for (id, sp, c) in g.sim.fauna_census() {
        if !explored.contains(&(c.col, c.row)) {
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

fn camera_control(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    game: NonSend<Game>,
    mut q: Query<(&mut CamRig, &mut Transform)>,
) {
    // While typing in a conversation, the letter keys belong to the text box, not the camera.
    if game.convo.is_some() {
        return;
    }
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

fn update_hud(mut game: NonSendMut<Game>, time: Res<Time>, mut texts: Query<(&HudKind, &mut Text)>) {
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
    let voice_line = g.voice.status_line();
    let can_talk = !traveling && g.convo.is_none() && view.as_ref().is_some_and(|v| !v.nearby.is_empty());
    // The lure: does the ground here hold something to find?
    let search_cue = match (traveling, g.sim.player_find_state()) {
        (false, FindState::Findable) => "\n‹ you sense something here — press F to search ›",
        (false, FindState::Locked) => "\n‹ something here eludes you — you lack the knowledge ›",
        _ => "",
    };

    // The soul's live disposition toward the player — it shifts as the conversation lands effects.
    let avatar = g.sim.player_avatar();
    let dispo: Option<String> = g.convo.as_ref().map(|c| {
        let op = avatar.and_then(|a| g.sim.opinion_of(c.listener, a)).unwrap_or(0.0);
        let word = disposition_word(op);
        if avatar.is_some_and(|a| g.sim.bears_grudge(c.listener, a)) { format!("{word}, and bears a grudge") } else { word.to_string() }
    });

    // The bottom-left panel: an open conversation, else the voices around you.
    let talk_panel = if let Some(c) = &g.convo {
        // Animated "considering" ellipsis while a reply is still being generated.
        let dots = [".", "..", "..."][((time.elapsed_secs() * 3.0) as usize) % 3];
        let mut s = format!("── {} · {} ──\n", c.name, dispo.as_deref().unwrap_or(""));
        for ln in &c.transcript {
            s.push_str(&ln.prefix);
            match &ln.text {
                // Type the words in (RPG-style); `reveal` is a char count.
                Some(t) => s.extend(t.chars().take(ln.reveal as usize)),
                // Not generated yet — the soul is considering.
                None => s.push_str(dots),
            }
            s.push('\n');
        }
        // The input line the player is typing, with a cursor.
        s.push_str("\n> ");
        s.push_str(&c.input);
        s.push('_');
        s.push_str("\n(type to speak | Enter send | Esc leave)");
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
                        "Day {day}\n({}, {})  {}  {:.0} m\nfertile {:.2}   {} soul(s) near\nfog lifted from {} tiles{}{}",
                        v.pos.col, v.pos.row, v.here.terrain.name(), v.here.elevation, v.here.fertility, v.nearby.len(), explored, feats, search_cue,
                    )
                }
                None => "no avatar".into(),
            },
            HudKind::Talk => talk_panel.clone(),
            HudKind::Help => {
                let mut h = format!(
                    "{status}\nL-click inspect | R-click travel | Space wait | F search | T speak | J journal | A/D/W/S camera | scroll zoom",
                );
                if let Some(v) = &voice_line {
                    h.push_str("  -  ");
                    h.push_str(v);
                }
                h
            }
        };
    }
}
