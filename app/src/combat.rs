//! The combat mode — the playable face of the headless `combat_core` engine.
//!
//! When the avatar attacks (or is ambushed), the app asks `agents` to [`begin_combat`] and holds
//! the live [`agents::Encounter`] here in [`CombatUi`]. This module drives that fight to the next
//! *player* decision (enemies act through `combat_core::StubAi`), renders the tactical **timeline
//! ribbon** plus the combatant roster, move tray, and position map (the `fighting.png` HUD), and
//! turns the player's clicks/keys into engine commands. When the fight ends it asks `agents` to
//! write the outcome back to the world (deaths, persisted HP) and returns to the overworld.
//!
//! The simulation is authoritative and deterministic; this is a thin view + input surface over it,
//! exactly like the rest of the app. The timeline ribbon is a *slim band* docked at the bottom of
//! the field — the rest of the field is left for the future 3D fight rendering.
//!
//! [`begin_combat`]: agents::Simulation::begin_combat

// The combat UI's marker components are crate-private but appear in `pub(crate)` system
// signatures registered from `main.rs`; that is intentional, so quiet the interface lint.
#![allow(private_interfaces)]

use agents::combat_core as cc;
use agents::combat_core::Controller; // brings the `decide` method into scope
use app::theme::{self, ThemeFonts};
use bevy::prelude::*;
use bevy::ui::GlobalZIndex;

use crate::Game;

// Fixed-slot budgets (the project's dynamic-UI pattern: pre-spawn slots, toggle `display`).
const MAX_LANES: usize = 6;
const CELLS: usize = 30;
const MOVE_BTNS: usize = 6;
const ROSTER_ROWS: usize = 8;

/// How many engine ticks of Slow/Haste one press buys.
const EDIT_TICKS: u32 = 3;

// ── State held in `Game` ─────────────────────────────────────────────────────────────────────

/// The active fight and its interaction state. Lives in `Game::combat` while a fight is on.
pub(crate) struct CombatUi {
    pub enc: agents::Encounter,
    /// The enemy controller — a deterministic policy that dilates (elites bend the line).
    ai: cc::EliteAi,
    /// The pending *player* decision, if the engine is waiting on us.
    pending: Option<cc::Decision>,
    /// The fogged view for the pending decision (what to render + reason over).
    view: Option<cc::ForesightView>,
    /// The enemy the player's next move/edit will land on.
    target: Option<cc::ActorId>,
    /// Recent combat-log lines (newest last).
    log: Vec<String>,
    /// `Some` once the fight has ended and been written back — a banner awaiting dismissal.
    ending: Option<Ending>,
}

pub(crate) struct Ending {
    victory: bool,
    avatar_down: bool,
}

impl CombatUi {
    pub fn new(enc: agents::Encounter) -> Self {
        let ai = cc::EliteAi::new(enc.sim.library().clone(), *enc.sim.config());
        let target = enc
            .combatants
            .iter()
            .find(|c| !c.is_player_side)
            .map(|c| c.actor);
        Self {
            enc,
            ai,
            pending: None,
            view: None,
            target,
            log: Vec::new(),
            ending: None,
        }
    }

    fn push_log(&mut self, line: String) {
        self.log.push(line);
        let n = self.log.len();
        if n > 7 {
            self.log.drain(0..n - 7);
        }
    }
}

/// Start a fight from the overworld: build the encounter and enter combat mode. No-op if combat
/// can't begin (layer off, no avatar, no enemies present).
pub(crate) fn start(game: &mut Game, enemies: Vec<bevy::ecs::entity::Entity>) {
    if game.combat.is_some() {
        return;
    }
    if let Some(enc) = game.sim.begin_combat(enemies) {
        let n = enc.combatants.len();
        game.combat = Some(CombatUi::new(enc));
        game.status = format!("You are drawn into battle — {n} stand in the fray.");
    }
}

// ── Driving the fight ────────────────────────────────────────────────────────────────────────

/// Advance the encounter to the next *player* decision (auto-resolving enemy turns with the stub
/// AI), or finish the fight and write it back to the world. Runs every frame; cheap when idle.
pub(crate) fn combat_step(mut game: NonSendMut<Game>) {
    let g = &mut *game;
    let Some(ui) = g.combat.as_mut() else {
        return;
    };
    if ui.pending.is_some() || ui.ending.is_some() {
        return; // waiting on the player, or showing the result banner
    }

    loop {
        match ui.enc.sim.run_until_decision_or_end() {
            cc::StepResult::Decision { decision, view } => {
                if decision.faction == cc::FactionId::PLAYER {
                    ui.target = ui.target.filter(|t| is_alive(&view, *t)).or_else(|| {
                        view.actors
                            .iter()
                            .find(|a| a.faction != view.own_faction && a.hp > 0)
                            .map(|a| a.id)
                    });
                    ui.pending = Some(decision);
                    ui.view = Some(view);
                    drain_log(ui);
                    return;
                }
                // Enemy: the stub AI decides, deterministically.
                let cmd = ui.ai.decide(&decision, &view);
                ui.enc.sim.submit(cmd);
            }
            cc::StepResult::Ended(outcome) => {
                drain_log(ui);
                let victory = matches!(outcome, cc::Outcome::Victory { faction } if faction == cc::FactionId::PLAYER);
                let res = g.sim.finish_combat(&ui.enc);
                let avatar_down = res.as_ref().map(|r| r.avatar_down).unwrap_or(false);
                ui.ending = Some(Ending {
                    victory,
                    avatar_down,
                });
                ui.push_log(if avatar_down {
                    "You fall.".into()
                } else if victory {
                    "The field is yours.".into()
                } else {
                    "The fight is done.".into()
                });
                return;
            }
        }
    }
}

/// Dev/screenshot helper: auto-play `decisions` turns with the stub AI on *both* sides, leaving the
/// fight mid-stream so the timeline ribbon shows committed actions. Not used in normal play.
pub(crate) fn dev_autoplay(ui: &mut CombatUi, decisions: usize) {
    let mut ai = cc::StubAi::new(ui.enc.sim.library().clone());
    for _ in 0..decisions {
        match ui.enc.sim.run_until_decision_or_end() {
            cc::StepResult::Decision { decision, view } => {
                let cmd = ai.decide(&decision, &view);
                ui.enc.sim.submit(cmd);
            }
            cc::StepResult::Ended(_) => break,
        }
    }
    drain_log(ui);
}

/// Drain the engine's fresh events into the human-readable combat log.
fn drain_log(ui: &mut CombatUi) {
    let events = ui.enc.sim.drain_events();
    for ev in &events {
        if let Some(line) = describe(&ui.enc, ev) {
            ui.push_log(line);
        }
    }
}

fn name_of(enc: &agents::Encounter, actor: cc::ActorId) -> String {
    enc.of(actor).map(|c| c.name.clone()).unwrap_or_default()
}

/// A short combat-log line for an engine event (or `None` for the structural ones).
fn describe(enc: &agents::Encounter, ev: &cc::Event) -> Option<String> {
    use cc::Event::*;
    Some(match ev {
        Hit {
            attacker,
            target,
            amount,
            ..
        } => format!(
            "{} strikes {} for {amount}.",
            name_of(enc, *attacker),
            name_of(enc, *target)
        ),
        Interrupted { by, .. } => format!("{} cuts off the wind-up!", name_of(enc, *by)),
        LineShoved { target, ticks, .. } => {
            format!("{} is shoved {ticks} down the line.", name_of(enc, *target))
        }
        WindowOpened { actor, .. } => format!("{} is left exposed.", name_of(enc, *actor)),
        ActorStaggered { actor, .. } => format!("{} reels, staggered.", name_of(enc, *actor)),
        ActionFizzled { .. } => "A blow finds only air.".into(),
        ActorDowned { actor, .. } => format!("{} falls.", name_of(enc, *actor)),
        TempoChanged { actor, delta, .. } if *delta > 0 => {
            format!("{} seizes the tempo (+{delta}).", name_of(enc, *actor))
        }
        _ => return None,
    })
}

fn is_alive(view: &cc::ForesightView, actor: cc::ActorId) -> bool {
    view.actors.iter().any(|a| a.id == actor && a.hp > 0)
}

// ── Player commands ──────────────────────────────────────────────────────────────────────────

/// Reposition the active actor to `zone` — a readiness action (it occupies tick-time like any
/// move). Ignored on a dilation turn.
fn reposition(ui: &mut CombatUi, zone: u8) {
    let Some(decision) = ui.pending else { return };
    if decision.kind != cc::DecisionKind::Readiness {
        return;
    }
    let mv = agents::combat::reposition_move(zone);
    ui.enc
        .sim
        .submit(cc::Command::CommitAction { mv, target: None });
    ui.pending = None;
    ui.view = None;
}

/// The active actor's current zone, from the pending view.
fn active_zone(ui: &CombatUi) -> Option<u8> {
    let view = ui.view.as_ref()?;
    view.actors
        .iter()
        .find(|a| a.id == view.observer)
        .map(|a| a.zone)
}

/// Submit the player's choice for slot `i` of the action tray, given the pending decision's kind.
fn play_slot(ui: &mut CombatUi, i: usize) {
    let Some(decision) = ui.pending else {
        return;
    };
    let Some(view) = ui.view.clone() else {
        return;
    };
    let cmd = match decision.kind {
        cc::DecisionKind::Readiness => readiness_cmd(&view, ui.target, i),
        cc::DecisionKind::Dilation => dilation_cmd(&view, ui.target, i),
    };
    if let Some(cmd) = cmd {
        ui.enc.sim.submit(cmd);
        ui.pending = None;
        ui.view = None;
    }
}

/// Map a tray slot to a readiness command: slots 0..5 commit the kit's moves at the target;
/// the last slot Holds.
fn readiness_cmd(
    view: &cc::ForesightView,
    target: Option<cc::ActorId>,
    i: usize,
) -> Option<cc::Command> {
    if i >= view.own_moves.len() {
        return Some(cc::Command::Hold);
    }
    let mv = view.own_moves[i];
    Some(cc::Command::CommitAction { mv, target })
}

/// Map a tray slot to a dilation (edit) command: Slow / Haste / Interrupt the target's committed
/// action, Insert a quick strike, or Pass.
fn dilation_cmd(
    view: &cc::ForesightView,
    target: Option<cc::ActorId>,
    i: usize,
) -> Option<cc::Command> {
    let target = target?;
    let enemy_instance = view
        .instances
        .iter()
        .find(|inst| inst.actor == target && !inst.own)
        .map(|inst| inst.id);
    let verb = match i {
        0 => enemy_instance.map(|instance| cc::EditVerb::Slow {
            instance,
            ticks: EDIT_TICKS,
        }),
        1 => enemy_instance.map(|instance| cc::EditVerb::Haste {
            instance,
            ticks: EDIT_TICKS,
        }),
        2 => enemy_instance.map(|instance| cc::EditVerb::Interrupt { instance }),
        3 => view.own_moves.first().map(|&mv| cc::EditVerb::Insert {
            actor: view.observer,
            mv,
            target: Some(target),
        }),
        _ => None, // Pass
    };
    Some(match verb {
        Some(v) => cc::Command::EditVerb(v),
        None => cc::Command::Pass,
    })
}

// ── Input (keys + buttons) ───────────────────────────────────────────────────────────────────

/// Keyboard control during a fight: 1-6 pick a tray slot, Tab cycles the target, Enter dismisses
/// the result banner.
pub(crate) fn combat_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    if game.combat.is_none() {
        return;
    }
    // Dismiss the end-of-fight banner.
    if game.combat.as_ref().is_some_and(|u| u.ending.is_some()) {
        if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
            game.combat = None;
        }
        return;
    }
    let g = &mut *game;
    let Some(ui) = g.combat.as_mut() else { return };
    if ui.pending.is_none() {
        return;
    }
    if keys.just_pressed(KeyCode::Tab) {
        cycle_target(ui);
    }
    // Left/Right reposition the active actor one zone over (a readiness action).
    if let Some(z) = active_zone(ui) {
        if keys.just_pressed(KeyCode::ArrowLeft) {
            reposition(ui, z.saturating_sub(1));
            return;
        }
        if keys.just_pressed(KeyCode::ArrowRight) {
            reposition(ui, (z + 1).min(agents::combat::ZONE_COUNT - 1));
            return;
        }
    }
    // coupling-lint:allow const_all SLOT_KEYS: a keyboard binding table (the digit keys 1..6 → the
    // tray slots), not content — what each slot *does* is the kit/edit-verb, which is data-driven.
    const SLOT_KEYS: [KeyCode; MOVE_BTNS] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
    ];
    for (i, k) in SLOT_KEYS.iter().enumerate() {
        if keys.just_pressed(*k) {
            play_slot(ui, i);
            break;
        }
    }
}

fn cycle_target(ui: &mut CombatUi) {
    let Some(view) = ui.view.as_ref() else { return };
    let enemies: Vec<cc::ActorId> = view
        .actors
        .iter()
        .filter(|a| a.faction != view.own_faction && a.hp > 0)
        .map(|a| a.id)
        .collect();
    if enemies.is_empty() {
        return;
    }
    let cur = ui.target.and_then(|t| enemies.iter().position(|&e| e == t));
    let next = cur.map(|i| (i + 1) % enemies.len()).unwrap_or(0);
    ui.target = Some(enemies[next]);
}

/// Clicking a tray button plays that slot; clicking a roster row selects that enemy as the target.
pub(crate) fn combat_clicks(
    mut game: NonSendMut<Game>,
    moves: Query<(&CombatMoveBtn, &Interaction), Changed<Interaction>>,
    rows: Query<(&CombatRosterRow, &Interaction), Changed<Interaction>>,
    zones: Query<(&CombatZoneBtn, &Interaction), Changed<Interaction>>,
) {
    if game.combat.is_none() {
        return;
    }
    // End banner: any tray click dismisses.
    if game.combat.as_ref().is_some_and(|u| u.ending.is_some()) {
        if moves.iter().any(|(_, i)| *i == Interaction::Pressed) {
            game.combat = None;
        }
        return;
    }
    let g = &mut *game;
    let Some(ui) = g.combat.as_mut() else { return };

    // Clicking a position-map zone repositions the active actor there (readiness only).
    for (zone, interaction) in &zones {
        if *interaction == Interaction::Pressed {
            reposition(ui, zone.0 as u8);
            return;
        }
    }

    for (row, interaction) in &rows {
        if *interaction == Interaction::Pressed
            && let Some(c) = ui.enc.combatants.get(row.0)
            && !c.is_player_side
        {
            ui.target = Some(c.actor);
        }
    }
    if ui.pending.is_none() {
        return;
    }
    for (btn, interaction) in &moves {
        if *interaction == Interaction::Pressed {
            play_slot(ui, btn.0);
            break;
        }
    }
}

// ── Components (fixed slots) ─────────────────────────────────────────────────────────────────

#[derive(Component)]
pub(crate) struct CombatRoot;
#[derive(Component)]
pub(crate) struct CombatStatus;
#[derive(Component)]
pub(crate) struct CombatPrompt;
#[derive(Component)]
pub(crate) struct CombatLog;
#[derive(Component)]
pub(crate) struct CombatRosterRow(usize);
#[derive(Component)]
pub(crate) struct CombatRosterName(usize);
#[derive(Component)]
pub(crate) struct CombatHpFill(usize);
#[derive(Component)]
pub(crate) struct CombatRosterMeta(usize);
#[derive(Component)]
pub(crate) struct CombatLaneLabel(usize);
#[derive(Component)]
pub(crate) struct CombatCell {
    lane: usize,
    cell: usize,
}
#[derive(Component)]
pub(crate) struct CombatMoveBtn(usize);
#[derive(Component)]
pub(crate) struct CombatMoveLabel(usize);
#[derive(Component)]
pub(crate) struct CombatZoneBtn(usize);
#[derive(Component)]
pub(crate) struct CombatZoneOccupants(usize);

// ── Spawn the overlay (once, hidden) ─────────────────────────────────────────────────────────

/// Build the combat overlay's fixed slots, hidden until a fight begins. Called once at startup.
pub(crate) fn spawn_combat_ui(commands: &mut Commands, fonts: &ThemeFonts) {
    commands
        .spawn((
            CombatRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(theme::SP_LG)),
                row_gap: Val::Px(theme::SP_SM),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.58)),
            GlobalZIndex(80),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            // Top: title + the live status/prompt.
            root.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|bar| {
                bar.spawn(theme::display(fonts, "Battle"));
                bar.spawn((theme::heading(fonts, ""), CombatStatus));
            });

            // Middle: roster | field+timeline | move tray.
            root.spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                column_gap: Val::Px(theme::SP_MD),
                ..default()
            })
            .with_children(|mid| {
                spawn_roster(mid, fonts);
                spawn_field(mid, fonts);
                spawn_tray(mid, fonts);
            });

            // Bottom: the combat log + prompt (left) beside the position map (right).
            root.spawn(Node {
                width: Val::Percent(100.0),
                height: Val::Px(112.0),
                column_gap: Val::Px(theme::SP_MD),
                ..default()
            })
            .with_children(|bottom| {
                bottom
                    .spawn((
                        Node {
                            flex_grow: 1.0,
                            padding: UiRect::all(Val::Px(theme::SP_SM)),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(2.0),
                            ..default()
                        },
                        theme::panel_chrome(),
                    ))
                    .with_children(|b| {
                        b.spawn((theme::body(fonts, ""), CombatLog));
                        b.spawn((theme::micro(fonts, ""), CombatPrompt));
                    });
                spawn_position_map(bottom, fonts);
            });
        });
}

/// The position map (the mockup's "Positioning Map with Directions"): a Left/Center/Right strip of
/// clickable zones showing who stands where. Clicking a zone repositions the active actor there.
fn spawn_position_map(parent: &mut ChildSpawnerCommands, fonts: &ThemeFonts) {
    const ZONE_NAMES: [&str; 3] = ["Left", "Center", "Right"];
    parent
        .spawn((
            Node {
                width: Val::Px(360.0),
                padding: UiRect::all(Val::Px(theme::SP_SM)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme::SP_XS),
                ..default()
            },
            theme::panel_chrome(),
        ))
        .with_children(|panel| {
            panel.spawn(theme::label(fonts, "Position  ◄ ►"));
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    column_gap: Val::Px(theme::SP_XS),
                    ..default()
                })
                .with_children(|row| {
                    for z in 0..3usize {
                        row.spawn((
                            CombatZoneBtn(z),
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                padding: UiRect::all(Val::Px(4.0)),
                                row_gap: Val::Px(3.0),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(theme::RADIUS_SM)),
                                ..default()
                            },
                            BackgroundColor(theme::INK_SUNKEN),
                            BorderColor::all(theme::BORDER),
                        ))
                        .with_children(|cell| {
                            cell.spawn(theme::micro(fonts, ZONE_NAMES[z]));
                            cell.spawn((theme::micro(fonts, ""), CombatZoneOccupants(z)));
                        });
                    }
                });
        });
}

fn spawn_roster(parent: &mut ChildSpawnerCommands, fonts: &ThemeFonts) {
    parent
        .spawn((
            Node {
                width: Val::Px(220.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme::SP_XS),
                padding: UiRect::all(Val::Px(theme::SP_SM)),
                ..default()
            },
            theme::panel_chrome(),
        ))
        .with_children(|col| {
            col.spawn(theme::label(fonts, "Combatants"));
            for i in 0..ROSTER_ROWS {
                col.spawn((
                    CombatRosterRow(i),
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(4.0)),
                        row_gap: Val::Px(2.0),
                        display: Display::None,
                        ..default()
                    },
                    BackgroundColor(theme::INK_SUNKEN),
                    BorderColor::all(theme::BORDER),
                ))
                .with_children(|row| {
                    row.spawn((theme::label(fonts, ""), CombatRosterName(i)));
                    // HP well + fill.
                    row.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(6.0),
                            ..default()
                        },
                        BackgroundColor(theme::INK),
                    ))
                    .with_children(|well| {
                        well.spawn((
                            CombatHpFill(i),
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(theme::BLOOD),
                        ));
                    });
                    row.spawn((theme::micro(fonts, ""), CombatRosterMeta(i)));
                });
            }
        });
}

fn spawn_field(parent: &mut ChildSpawnerCommands, fonts: &ThemeFonts) {
    parent
        .spawn(Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|col| {
            // The field proper — reserved for the future 3D fight. A faint placeholder only.
            col.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::FlexStart,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.04, 0.05, 0.07, 0.0)),
            ))
            .with_children(|field| {
                field.spawn(theme::micro(fonts, "— the field —"));
            });
            // The slim timeline ribbon band, docked at the bottom of the field.
            col.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(118.0),
                    margin: UiRect::top(Val::Px(theme::SP_SM)),
                    padding: UiRect::all(Val::Px(theme::SP_SM)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                },
                theme::panel_chrome(),
            ))
            .with_children(|band| {
                band.spawn(theme::label(fonts, "The next few seconds"));
                for lane in 0..MAX_LANES {
                    band.spawn(Node {
                        width: Val::Percent(100.0),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        display: Display::None,
                        ..default()
                    })
                    .insert(LaneRow(lane))
                    .with_children(|row| {
                        row.spawn((
                            CombatLaneLabel(lane),
                            Node {
                                width: Val::Px(96.0),
                                ..default()
                            },
                            Text::new(""),
                            TextFont {
                                font: fonts.mono.clone(),
                                font_size: theme::T_MICRO,
                                ..default()
                            },
                            TextColor(theme::TEXT_DIM),
                        ));
                        row.spawn(Node {
                            flex_grow: 1.0,
                            column_gap: Val::Px(1.0),
                            ..default()
                        })
                        .with_children(|cells| {
                            for cell in 0..CELLS {
                                cells.spawn((
                                    CombatCell { lane, cell },
                                    Node {
                                        flex_grow: 1.0,
                                        height: Val::Px(10.0),
                                        ..default()
                                    },
                                    BackgroundColor(theme::INK_SUNKEN),
                                ));
                            }
                        });
                    });
                }
            });
        });
}

#[derive(Component)]
pub(crate) struct LaneRow(usize);

fn spawn_tray(parent: &mut ChildSpawnerCommands, fonts: &ThemeFonts) {
    parent
        .spawn((
            Node {
                width: Val::Px(180.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme::SP_SM),
                padding: UiRect::all(Val::Px(theme::SP_SM)),
                ..default()
            },
            theme::panel_chrome(),
        ))
        .with_children(|col| {
            col.spawn(theme::label(fonts, "Moves"));
            for i in 0..MOVE_BTNS {
                col.spawn((
                    CombatMoveBtn(i),
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::all(Val::Px(theme::SP_SM)),
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(theme::RADIUS_SM)),
                        ..default()
                    },
                    BackgroundColor(theme::INK_RAISED),
                    BorderColor::all(theme::BORDER),
                ))
                .with_children(|b| {
                    b.spawn((theme::body(fonts, ""), CombatMoveLabel(i)));
                });
            }
        });
}

// ── Render the overlay from state ────────────────────────────────────────────────────────────

/// Toggle the overlay and fill every slot from the current fight state. Runs every frame; cheap.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn update_combat_ui(
    game: NonSend<Game>,
    mut root: Query<&mut Visibility, With<CombatRoot>>,
    mut status: Query<
        &mut Text,
        (
            With<CombatStatus>,
            Without<CombatPrompt>,
            Without<CombatLog>,
        ),
    >,
    mut prompt: Query<
        &mut Text,
        (
            With<CombatPrompt>,
            Without<CombatStatus>,
            Without<CombatLog>,
        ),
    >,
    mut log: Query<
        &mut Text,
        (
            With<CombatLog>,
            Without<CombatStatus>,
            Without<CombatPrompt>,
        ),
    >,
    mut rows: Query<(&CombatRosterRow, &mut Node, &mut BackgroundColor)>,
    mut names: Query<
        (&CombatRosterName, &mut Text),
        (
            Without<CombatStatus>,
            Without<CombatPrompt>,
            Without<CombatLog>,
            Without<CombatRosterMeta>,
            Without<CombatMoveLabel>,
        ),
    >,
    mut metas: Query<
        (&CombatRosterMeta, &mut Text),
        (
            Without<CombatStatus>,
            Without<CombatPrompt>,
            Without<CombatLog>,
            Without<CombatRosterName>,
            Without<CombatMoveLabel>,
        ),
    >,
    mut hp: Query<(&CombatHpFill, &mut Node), (Without<CombatRosterRow>, Without<LaneRow>)>,
    mut lanes: Query<(&LaneRow, &mut Node), (Without<CombatRosterRow>, Without<CombatHpFill>)>,
    mut lane_labels: Query<
        (&CombatLaneLabel, &mut Text),
        (
            Without<CombatStatus>,
            Without<CombatPrompt>,
            Without<CombatLog>,
            Without<CombatRosterName>,
            Without<CombatRosterMeta>,
            Without<CombatMoveLabel>,
        ),
    >,
    mut cells: Query<
        (&CombatCell, &mut BackgroundColor),
        (Without<CombatRosterRow>, Without<CombatHpFill>),
    >,
    mut move_labels: Query<
        (&CombatMoveLabel, &mut Text),
        (
            Without<CombatStatus>,
            Without<CombatPrompt>,
            Without<CombatLog>,
            Without<CombatRosterName>,
            Without<CombatRosterMeta>,
        ),
    >,
) {
    let on = game.combat.is_some();
    if let Ok(mut v) = root.single_mut() {
        *v = if on {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    let Some(ui) = game.combat.as_ref() else {
        return;
    };

    // Status + prompt.
    if let Ok(mut t) = status.single_mut() {
        t.0 = match &ui.ending {
            Some(e) if e.avatar_down => "You have fallen.".into(),
            Some(e) if e.victory => "Victory.".into(),
            Some(_) => "The fight is over.".into(),
            None => match active_decision_kind(ui) {
                Some(cc::DecisionKind::Readiness) => "Choose an action.".into(),
                Some(cc::DecisionKind::Dilation) => "Bend the line, or pass.".into(),
                None => "…".into(),
            },
        };
    }
    if let Ok(mut t) = prompt.single_mut() {
        t.0 = if ui.ending.is_some() {
            "Enter — return to the world".into()
        } else {
            "1-5 act · ◄ ► or click a zone to move · Tab target · click a foe to mark it".into()
        };
    }
    if let Ok(mut t) = log.single_mut() {
        t.0 = ui.log.join("\n");
    }

    // Roster rows.
    for (row, mut node, mut bg) in &mut rows {
        let shown = ui.enc.combatants.get(row.0);
        node.display = if shown.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(c) = shown {
            let selected = ui.target == Some(c.actor);
            bg.0 = if selected {
                theme::INK_RAISED
            } else {
                theme::INK_SUNKEN
            };
        }
    }
    for (slot, mut text) in &mut names {
        text.0 = ui
            .enc
            .combatants
            .get(slot.0)
            .map(|c| {
                let side = if c.is_player_side { "" } else { "⚔ " };
                format!("{side}{}", c.name)
            })
            .unwrap_or_default();
    }
    for (slot, mut node) in &mut hp {
        let pct = combatant_hp_pct(ui, slot.0);
        node.width = Val::Percent(pct * 100.0);
    }
    for (slot, mut text) in &mut metas {
        text.0 = combatant_meta(ui, slot.0);
    }

    // Timeline ribbon.
    render_timeline(ui, &mut lanes, &mut lane_labels, &mut cells);

    // Move tray labels (depend on the pending decision's kind).
    let labels = tray_labels(ui);
    for (slot, mut text) in &mut move_labels {
        text.0 = labels.get(slot.0).cloned().unwrap_or_default();
    }
}

fn active_decision_kind(ui: &CombatUi) -> Option<cc::DecisionKind> {
    ui.pending.as_ref().map(|d| d.kind)
}

fn combatant_hp_pct(ui: &CombatUi, slot: usize) -> f32 {
    let Some(c) = ui.enc.combatants.get(slot) else {
        return 0.0;
    };
    match ui.enc.sim.actor(c.actor) {
        Some(a) if a.vitals.max_hp > 0 => {
            (a.vitals.hp.max(0) as f32 / a.vitals.max_hp as f32).clamp(0.0, 1.0)
        }
        _ => 0.0,
    }
}

fn combatant_meta(ui: &CombatUi, slot: usize) -> String {
    let Some(c) = ui.enc.combatants.get(slot) else {
        return String::new();
    };
    match ui.enc.sim.actor(c.actor) {
        Some(a) => {
            let down = matches!(a.state, cc::ActorState::Down);
            if down {
                "down".into()
            } else if c.is_player_side {
                format!(
                    "hp {}/{}  tempo {}",
                    a.vitals.hp.max(0),
                    a.vitals.max_hp,
                    a.tempo
                )
            } else {
                format!("hp {}/{}", a.vitals.hp.max(0), a.vitals.max_hp)
            }
        }
        None => String::new(),
    }
}

/// The action-tray labels for the current decision kind.
fn tray_labels(ui: &CombatUi) -> Vec<String> {
    match active_decision_kind(ui) {
        Some(cc::DecisionKind::Dilation) => vec![
            "Slow".into(),
            "Haste".into(),
            "Interrupt".into(),
            "Insert".into(),
            "Pass".into(),
            String::new(),
        ],
        _ => {
            // Readiness: name the kit's moves, then Hold.
            let lib = ui.enc.sim.library();
            let mut v: Vec<String> = ui
                .view
                .as_ref()
                .map(|view| {
                    view.own_moves
                        .iter()
                        .map(|&mv| lib.get(mv).map(|d| d.name.clone()).unwrap_or_default())
                        .collect()
                })
                .unwrap_or_default();
            v.truncate(MOVE_BTNS - 1);
            v.push("Hold".into());
            v
        }
    }
}

#[allow(clippy::type_complexity)]
fn render_timeline(
    ui: &CombatUi,
    lanes: &mut Query<(&LaneRow, &mut Node), (Without<CombatRosterRow>, Without<CombatHpFill>)>,
    lane_labels: &mut Query<
        (&CombatLaneLabel, &mut Text),
        (
            Without<CombatStatus>,
            Without<CombatPrompt>,
            Without<CombatLog>,
            Without<CombatRosterName>,
            Without<CombatRosterMeta>,
            Without<CombatMoveLabel>,
        ),
    >,
    cells: &mut Query<
        (&CombatCell, &mut BackgroundColor),
        (Without<CombatRosterRow>, Without<CombatHpFill>),
    >,
) {
    // Lane `l` shows combatant `l` (player side first, then enemies — roster order).
    let shown = ui.enc.combatants.len().min(MAX_LANES);
    for (lane, mut node) in lanes.iter_mut() {
        node.display = if lane.0 < shown {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (label, mut text) in lane_labels.iter_mut() {
        text.0 = ui
            .enc
            .combatants
            .get(label.0)
            .map(|c| {
                let n: String = c.name.chars().take(10).collect();
                n
            })
            .unwrap_or_default();
    }

    let now = ui.view.as_ref().map(|v| v.current_tick.0).unwrap_or(0);
    for (cell, mut bg) in cells.iter_mut() {
        bg.0 = theme::INK_SUNKEN;
        let Some(c) = ui.enc.combatants.get(cell.lane) else {
            continue;
        };
        let t = now + cell.cell as u64;
        if let Some(phase) = phase_at(ui, c.actor, t) {
            bg.0 = match phase {
                CellPhase::Startup => theme::DREAD.with_alpha(0.6),
                CellPhase::Active => theme::BLOOD,
                CellPhase::Recovery => theme::TEXT_FAINT.with_alpha(0.5),
            };
        }
    }
}

enum CellPhase {
    Startup,
    Active,
    Recovery,
}

/// The phase of `actor`'s committed action at tick `t`, drawn from the foresight view.
fn phase_at(ui: &CombatUi, actor: cc::ActorId, t: u64) -> Option<CellPhase> {
    let view = ui.view.as_ref()?;
    let inst = view.instances.iter().find(|i| i.actor == actor)?;
    let (s, a0, a1, r) = (
        inst.start_tick.0,
        inst.active_start.0,
        inst.active_end.0,
        inst.recovery_end.0,
    );
    if t >= s && t < a0 {
        Some(CellPhase::Startup)
    } else if t >= a0 && t < a1 {
        Some(CellPhase::Active)
    } else if t >= a1 && t < r {
        Some(CellPhase::Recovery)
    } else {
        None
    }
}

/// Fill the position map: the occupants of each zone, with the active actor's zone lit. `@` is the
/// avatar, `+` an ally, `x` a foe.
pub(crate) fn update_position_map(
    game: NonSend<Game>,
    mut zones: Query<(&CombatZoneBtn, &mut BackgroundColor)>,
    mut occ: Query<(&CombatZoneOccupants, &mut Text)>,
) {
    let Some(ui) = game.combat.as_ref() else {
        return;
    };
    let here = active_zone(ui);
    for (btn, mut bg) in &mut zones {
        bg.0 = if Some(btn.0 as u8) == here {
            theme::INK_RAISED
        } else {
            theme::INK_SUNKEN
        };
    }
    for (cell, mut text) in &mut occ {
        let mut tokens = Vec::new();
        for c in &ui.enc.combatants {
            let Some(a) = ui.enc.sim.actor(c.actor) else {
                continue;
            };
            if matches!(a.state, cc::ActorState::Down) || a.zone as usize != cell.0 {
                continue;
            }
            let tag = if c.is_avatar {
                "@"
            } else if c.is_player_side {
                "+"
            } else {
                "x"
            };
            let initial: String = c.name.chars().take(1).collect();
            tokens.push(format!("{tag}{initial}"));
        }
        text.0 = tokens.join(" ");
    }
}

// ── 3D fight figures (rendered in the reserved field) ────────────────────────────────────────

/// Meshes/materials for the combat figures, built once at startup.
#[derive(Resource)]
pub(crate) struct CombatFigAssets {
    mesh: Handle<Mesh>,
    player_mat: Handle<StandardMaterial>,
    enemy_mat: Handle<StandardMaterial>,
    down_mat: Handle<StandardMaterial>,
}

impl CombatFigAssets {
    pub(crate) fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let lit = |c: Color| StandardMaterial {
            base_color: c,
            emissive: c.to_linear() * 0.6, // glow a little so they read through the overlay
            ..default()
        };
        Self {
            mesh: meshes.add(Capsule3d::new(0.7, 2.6)),
            player_mat: materials.add(lit(Color::srgb(0.55, 0.70, 0.95))),
            enemy_mat: materials.add(lit(Color::srgb(0.88, 0.36, 0.33))),
            down_mat: materials.add(lit(Color::srgb(0.34, 0.36, 0.40))),
        }
    }
}

/// One figure on the field, tied to its engine actor.
#[derive(Component)]
pub(crate) struct CombatFig {
    actor: cc::ActorId,
}

/// Spawn/position/colour a 3D figure per combatant in two facing rows centred on the avatar (the
/// camera's focus, so they fill the reserved field). The overworld avatar figure is hidden while
/// fighting; the figures are torn down when the fight ends.
pub(crate) fn sync_combat_figures(
    game: NonSend<Game>,
    assets: Res<CombatFigAssets>,
    mut commands: Commands,
    mut figs: Query<(
        Entity,
        &CombatFig,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
    mut avatar_fig: Query<&mut Visibility, With<crate::AvatarFig>>,
) {
    // The overworld avatar stands in for the player out of combat; hide it during a fight (a
    // dedicated figure represents the avatar in the formation).
    let in_combat = game.combat.is_some();
    for mut v in &mut avatar_fig {
        *v = if in_combat {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }

    let Some(ui) = game.combat.as_ref() else {
        for (e, _, _, _) in figs.iter() {
            commands.entity(e).despawn();
        }
        return;
    };
    let base = game.avatar_render;

    // Stable side + index for the two facing rows.
    let mut layout: std::collections::HashMap<u32, (bool, i32)> = std::collections::HashMap::new();
    let (mut pi, mut ei) = (0i32, 0i32);
    for c in &ui.enc.combatants {
        if c.is_player_side {
            layout.insert(c.actor.0, (true, pi));
            pi += 1;
        } else {
            layout.insert(c.actor.0, (false, ei));
            ei += 1;
        }
    }
    let (n_players, n_enemies) = (pi.max(1), ei.max(1));

    // Spawn a figure for any combatant that lacks one (positioned next frame).
    let present: std::collections::HashSet<u32> =
        figs.iter().map(|(_, f, _, _)| f.actor.0).collect();
    for c in &ui.enc.combatants {
        if !present.contains(&c.actor.0) {
            commands.spawn((
                CombatFig { actor: c.actor },
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(assets.player_mat.clone()),
                Transform::from_translation(base),
            ));
        }
    }

    // Place + colour the existing figures.
    for (_, fig, mut tf, mut mat) in &mut figs {
        let Some(&(player, idx)) = layout.get(&fig.actor.0) else {
            continue;
        };
        let count = if player { n_players } else { n_enemies };
        let x = (idx as f32 - (count as f32 - 1.0) / 2.0) * 2.4;
        let z = if player { 4.0 } else { -2.4 };
        let actor = ui.enc.sim.actor(fig.actor);
        let down = actor.is_some_and(|a| matches!(a.state, cc::ActorState::Down));
        let acting = actor.is_some_and(|a| matches!(a.state, cc::ActorState::Committed(_)));
        let mut pos = base + Vec3::new(x, 1.2, z);
        if down {
            pos.y -= 0.7;
            tf.rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        } else {
            tf.rotation = Quat::IDENTITY;
            if acting {
                pos.y += 0.35; // a small lift while mid-action
            }
        }
        tf.translation = pos;
        mat.0 = if down {
            assets.down_mat.clone()
        } else if player {
            assets.player_mat.clone()
        } else {
            assets.enemy_mat.clone()
        };
    }
}
