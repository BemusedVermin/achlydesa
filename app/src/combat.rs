//! The combat mode — a **separate, self-contained 2D battle scene** over the headless
//! `combat_core` engine. Nothing of the exploration view shows through: an opaque backdrop covers
//! the screen and the overworld HUD is hidden while fighting.
//!
//! The fight is a continuous 2D field (`combat_field`): combatants are tokens; a move targets a
//! *person* and travels the 1D line to them, landing only within reach (else it whiffs). The
//! player reads the board, previews exactly what a move will do, commits or bends the line, and
//! then **watches the next few seconds play out** — tokens slide, blows land, the timeline
//! playhead sweeps — pausing again the moment *any* actor (player or enemy) must decide.

use agents::combat_core as cc;
use agents::combat_core::Controller;
use agents::{CombatContent, Encounter};
use app::theme::{self, ThemeFonts};
use bevy::prelude::*;
use bevy::ui::GlobalZIndex;
use bevy::ui::widget::ImageNode;
use std::collections::{HashMap, VecDeque};

use crate::Game;
use crate::combat_field::{self, FieldView, Token};

const TRAY_SLOTS: usize = 8;
const ROSTER_ROWS: usize = 8;
const LANES: usize = 8;
const CELLS: usize = 32;
const FIELD_W: u32 = 820;
const FIELD_H: u32 = 470;
/// Engine ticks played per real second during a burst.
const TICKS_PER_SEC: f32 = 7.0;
/// World units a token glides per second toward its new position.
const GLIDE: f32 = 14.0;
const FLASH_TIME: f32 = 0.5;
/// How many ticks of Slow/Haste one press buys.
const EDIT_TICKS: u32 = 3;

const C_ALLY: [u8; 3] = [96, 150, 230];
const C_AVATAR: [u8; 3] = [120, 200, 255];
const C_FOE: [u8; 3] = [222, 96, 90];

// ── State ────────────────────────────────────────────────────────────────────────────────────

/// The live fight + its presentation state.
pub(crate) struct CombatUi {
    enc: Encounter,
    ai: cc::EliteAi,
    content: CombatContent,
    /// The foe the player's next move/edit lands on.
    target: Option<cc::ActorId>,
    /// The tray slot currently previewed (selected).
    sel: usize,
    log: Vec<String>,

    // Playback.
    clock: f32,
    pending: VecDeque<cc::Event>,
    paused: Option<Paused>,
    /// The view to render the board/timeline from (the latest decision's).
    view: Option<cc::ForesightView>,
    ending: Option<Ending>,

    // Visuals (field units).
    vis: HashMap<u32, Vec2>,
    vis_to: HashMap<u32, Vec2>,
    flash: HashMap<u32, f32>,
}

struct Paused {
    decision: cc::Decision,
    is_player: bool,
    /// The enemy AI's chosen command, shown before it is applied.
    enemy_cmd: Option<cc::Command>,
    /// Seconds this (enemy) decision has been shown — it auto-advances after a readable beat.
    dwell: f32,
}

struct Ending {
    victory: bool,
    avatar_down: bool,
}

impl CombatUi {
    pub fn new(enc: Encounter, content: CombatContent) -> Self {
        let ai = cc::EliteAi::new(enc.sim.library().clone(), *enc.sim.config());
        let target = enc
            .combatants
            .iter()
            .find(|c| !c.is_player_side)
            .map(|c| c.actor);
        let mut ui = Self {
            enc,
            ai,
            content,
            target,
            sel: 0,
            log: Vec::new(),
            clock: 0.0,
            pending: VecDeque::new(),
            paused: None,
            view: None,
            ending: None,
            vis: HashMap::new(),
            vis_to: HashMap::new(),
            flash: HashMap::new(),
        };
        // Seed visual positions from the starting field positions.
        let seeds: Vec<(u32, Vec2)> = ui
            .enc
            .combatants
            .iter()
            .filter_map(|c| ui.enc.sim.actor(c.actor).map(|a| (c.actor.0, world(a.pos))))
            .collect();
        for (id, p) in seeds {
            ui.vis.insert(id, p);
            ui.vis_to.insert(id, p);
        }
        ui
    }

    fn push_log(&mut self, line: String) {
        self.log.push(line);
        let n = self.log.len();
        if n > 8 {
            self.log.drain(0..n - 8);
        }
    }

    /// Advance the visuals by `dt` and release any burst events whose tick the playhead has passed.
    fn animate(&mut self, dt: f32) {
        // Glide tokens toward their targets.
        let step = (GLIDE * dt).min(1.0);
        let ids: Vec<u32> = self.vis_to.keys().copied().collect();
        for id in ids {
            let to = self.vis_to[&id];
            let cur = *self.vis.entry(id).or_insert(to);
            self.vis.insert(id, cur.lerp(to, step));
        }
        // Decay hit flashes.
        for f in self.flash.values_mut() {
            *f = (*f - dt / FLASH_TIME).max(0.0);
        }
        // Play the burst.
        if !self.pending.is_empty() {
            self.clock += dt * TICKS_PER_SEC;
            while let Some(front) = self.pending.front() {
                let due = match front {
                    cc::Event::TickAdvanced { to } => (to.0 as f32) <= self.clock,
                    _ => true,
                };
                if !due {
                    break;
                }
                let ev = self.pending.pop_front().unwrap();
                self.apply_visual(&ev);
            }
        }
        // Snap the playhead onto the decision once the burst is drained.
        if self.pending.is_empty()
            && let Some(p) = &self.paused
        {
            self.clock = p.decision.tick.0 as f32;
        }
    }

    fn apply_visual(&mut self, ev: &cc::Event) {
        match ev {
            cc::Event::Moved { actor, to, .. } => {
                self.vis_to.insert(actor.0, world(*to));
            }
            cc::Event::Hit { target, .. } => {
                self.flash.insert(target.0, 1.0);
            }
            _ => {}
        }
        if let Some(line) = describe(&self.enc, ev) {
            self.push_log(line);
        }
    }

    fn at_decision(&self) -> bool {
        self.paused.is_some() && self.pending.is_empty()
    }

    /// The player must choose now.
    fn awaiting_player(&self) -> bool {
        self.at_decision() && self.paused.as_ref().is_some_and(|p| p.is_player)
    }

    /// Ensure the selected target is a living foe.
    fn ensure_target(&mut self) {
        let ok = self
            .target
            .and_then(|t| self.enc.sim.actor(t))
            .is_some_and(|a| a.alive());
        if !ok {
            self.target = self
                .enc
                .combatants
                .iter()
                .filter(|c| !c.is_player_side)
                .find(|c| self.enc.sim.actor(c.actor).is_some_and(|a| a.alive()))
                .map(|c| c.actor);
        }
    }

    /// Submit a command for the pending decision and release the engine to play the next burst.
    fn submit(&mut self, cmd: cc::Command) {
        self.enc.sim.submit(cmd);
        self.paused = None;
    }
}

/// `combat_core` position → field-space `Vec2` (fixed-point → float, presentation only).
fn world(p: cc::Pos) -> Vec2 {
    Vec2::new(p.x.0 as f32 / 65536.0, p.y.0 as f32 / 65536.0)
}

/// Begin a fight from the overworld.
pub(crate) fn start(game: &mut Game, enemies: Vec<bevy::ecs::entity::Entity>) {
    if game.combat.is_some() {
        return;
    }
    let content = game.sim.combat_content();
    if let (Some(enc), Some(content)) = (game.sim.begin_combat(enemies), content) {
        let n = enc.combatants.len();
        game.combat = Some(CombatUi::new(enc, content));
        game.status = format!("Battle — {n} join the fray.");
    }
}

// ── The driver: step the engine + animate ────────────────────────────────────────────────────

pub(crate) fn combat_tick(time: Res<Time>, mut game: NonSendMut<Game>) {
    let dt = time.delta_secs().min(0.05);
    let g = &mut *game;
    let Some(ui) = g.combat.as_mut() else {
        return;
    };
    ui.animate(dt);

    // Step the engine to the next decision (or end) once the current burst is fully played and we
    // are not already paused waiting on someone.
    if ui.pending.is_empty() && ui.paused.is_none() && ui.ending.is_none() {
        match ui.enc.sim.run_until_decision_or_end() {
            cc::StepResult::Decision { decision, view } => {
                for ev in ui.enc.sim.drain_events() {
                    ui.pending.push_back(ev);
                }
                let is_player = decision.faction == cc::FactionId::PLAYER;
                let enemy_cmd = (!is_player).then(|| ui.ai.decide(&decision, &view));
                ui.view = Some(view);
                if is_player {
                    ui.ensure_target();
                    ui.sel = 0;
                }
                ui.paused = Some(Paused {
                    decision,
                    is_player,
                    enemy_cmd,
                    dwell: 0.0,
                });
            }
            cc::StepResult::Ended(outcome) => {
                for ev in ui.enc.sim.drain_events() {
                    ui.pending.push_back(ev);
                }
                let victory = matches!(outcome, cc::Outcome::Victory { faction } if faction == cc::FactionId::PLAYER);
                let res = g.sim.finish_combat(&ui.enc);
                let avatar_down = res.as_ref().is_some_and(|r| r.avatar_down);
                ui.ending = Some(Ending {
                    victory,
                    avatar_down,
                });
            }
        }
    }

    // A foe's decision pauses so it can be read, then auto-advances after a beat (the player can
    // press Enter/Space to skip). Player decisions wait for input.
    if ui.at_decision()
        && let Some(p) = ui.paused.as_mut()
        && !p.is_player
    {
        p.dwell += dt;
        if p.dwell > AUTO_DWELL
            && let Some(cmd) = p.enemy_cmd
        {
            ui.submit(cmd);
        }
    }
}

/// How long a foe's chosen move is shown before it auto-advances.
const AUTO_DWELL: f32 = 0.9;

/// Dev/screenshot only: auto-act for the player (strike a foe in reach, else close in) so a fight
/// plays out headlessly.
pub(crate) fn dev_auto_player(ui: &mut CombatUi) {
    if !ui.awaiting_player() {
        return;
    }
    let Some(view) = ui.view.clone() else { return };
    let actor = ui.paused.as_ref().map(|p| p.decision.actor);
    let mut chosen = None;
    for (slot, &mv) in view.own_moves.iter().enumerate() {
        let Some(def) = ui.content.move_def(mv) else {
            continue;
        };
        let damages = def
            .effects
            .iter()
            .any(|e| matches!(e, cc::Effect::Damage { .. }));
        if damages
            && def.requires_tag.is_none()
            && let (Some(a), Some(t)) = (actor, ui.target)
        {
            let after = pos_after_move(ui, a, t, &def);
            if ui
                .enc
                .sim
                .actor(t)
                .is_some_and(|tt| after.within(tt.pos, def.reach))
            {
                chosen = Some(slot);
                break;
            }
        }
    }
    if chosen.is_none() {
        chosen = view.own_moves.iter().position(|&mv| {
            ui.content.move_def(mv).is_some_and(|d| {
                d.effects
                    .iter()
                    .any(|e| matches!(e, cc::Effect::Approach { .. }))
            })
        });
    }
    execute(ui, chosen.unwrap_or(0));
}

// ── Commands & previews ──────────────────────────────────────────────────────────────────────

fn name_of(enc: &Encounter, actor: cc::ActorId) -> String {
    enc.of(actor).map(|c| c.name.clone()).unwrap_or_default()
}

fn describe(enc: &Encounter, ev: &cc::Event) -> Option<String> {
    use cc::Event::*;
    Some(match ev {
        Hit {
            attacker,
            target,
            amount,
            ..
        } => {
            format!(
                "{} hits {} for {amount}.",
                name_of(enc, *attacker),
                name_of(enc, *target)
            )
        }
        ActionFizzled {
            reason: cc::FizzleReason::OutOfReach,
            ..
        } => "A blow falls short — whiff!".into(),
        ActionFizzled { .. } => "A blow finds only air.".into(),
        Interrupted { by, .. } => format!("{} cuts off the wind-up!", name_of(enc, *by)),
        LineShoved { target, ticks, .. } => {
            format!("{} is shoved {ticks} late.", name_of(enc, *target))
        }
        WindowOpened { actor, .. } => format!("{} is left exposed.", name_of(enc, *actor)),
        ActorStaggered { actor, .. } => format!("{} reels.", name_of(enc, *actor)),
        ActorDowned { actor, .. } => format!("{} falls.", name_of(enc, *actor)),
        _ => return None,
    })
}

/// The tray entries for the current decision (move ids for readiness; edit verbs for dilation).
fn tray_labels(ui: &CombatUi) -> Vec<String> {
    match ui.paused.as_ref().map(|p| p.decision.kind) {
        Some(cc::DecisionKind::Dilation) => ["Slow", "Haste", "Interrupt", "Insert", "Pass"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        Some(cc::DecisionKind::Readiness) => {
            let mut v: Vec<String> = ui
                .view
                .as_ref()
                .map(|view| {
                    view.own_moves
                        .iter()
                        .map(|mv| ui.content.move_def(*mv).map(|d| d.name).unwrap_or_default())
                        .collect()
                })
                .unwrap_or_default();
            v.truncate(TRAY_SLOTS - 1);
            v.push("Hold".into());
            v
        }
        None => Vec::new(),
    }
}

/// A multi-line preview of exactly what tray slot `sel` will do before it is executed.
fn preview_text(ui: &CombatUi) -> String {
    let Some(p) = ui.paused.as_ref() else {
        return String::new();
    };
    let Some(view) = ui.view.as_ref() else {
        return String::new();
    };
    match p.decision.kind {
        cc::DecisionKind::Dilation => match ui.sel {
            0 => "Slow: shove the target's committed action later on the line.".into(),
            1 => "Haste: pull the target's action earlier (toward now).".into(),
            2 => "Interrupt: cancel the target's wind-up if it is unarmored.".into(),
            3 => "Insert: a quick strike now (if idle), outside your turn.".into(),
            _ => "Pass: spend no Tempo; let the line stand.".into(),
        },
        cc::DecisionKind::Readiness => {
            if ui.sel >= view.own_moves.len() {
                return "Hold — wait a beat.".into();
            }
            let mv = view.own_moves[ui.sel];
            let Some(def) = ui.content.move_def(mv) else {
                return String::new();
            };
            let mut lines = vec![def.name.clone()];
            lines.push(format!(
                "wind {} · hit {} · rec {}",
                def.frames.startup, def.frames.active, def.frames.recovery
            ));
            let reach = def.reach.0 as f32 / 65536.0;
            let mut fx: Vec<String> = Vec::new();
            for e in &def.effects {
                match e {
                    cc::Effect::Damage { amount } => fx.push(format!("{amount} damage")),
                    cc::Effect::Approach { distance } => {
                        fx.push(format!("close {}", distance.0 / 65536))
                    }
                    cc::Effect::Withdraw { distance } => {
                        fx.push(format!("back off {}", distance.0 / 65536))
                    }
                    cc::Effect::LineKnockback { ticks } => fx.push(format!("knock back {ticks}")),
                    cc::Effect::OpenWindow { .. } => fx.push("expose".into()),
                    cc::Effect::Stagger { ticks } => fx.push(format!("stagger {ticks}")),
                }
            }
            if def.requires_tag.is_some() {
                fx.push("needs: target exposed".into());
            }
            if !fx.is_empty() {
                lines.push(fx.join(", "));
            }
            lines.push(format!("reach {reach:.0}"));
            // In reach / would whiff against the current target.
            let lands = def.effects.iter().any(|e| {
                matches!(
                    e,
                    cc::Effect::Damage { .. }
                        | cc::Effect::OpenWindow { .. }
                        | cc::Effect::Stagger { .. }
                        | cc::Effect::LineKnockback { .. }
                )
            });
            if lands && let (Some(a), Some(t)) = (Some(p.decision.actor), ui.target) {
                let after = pos_after_move(ui, a, t, &def);
                let in_reach = ui
                    .enc
                    .sim
                    .actor(t)
                    .is_some_and(|tt| after.within(tt.pos, def.reach));
                lines.push(if in_reach {
                    "lands ✓".into()
                } else {
                    "would whiff ✗ — close in first".into()
                });
            }
            lines.join("\n")
        }
    }
}

/// Where the actor would be after this move's approach/withdraw, for the preview's reach test.
fn pos_after_move(
    ui: &CombatUi,
    actor: cc::ActorId,
    target: cc::ActorId,
    def: &cc::MoveDef,
) -> cc::Pos {
    let Some(mut from) = ui.enc.sim.actor(actor).map(|a| a.pos) else {
        return cc::Pos::ORIGIN;
    };
    let Some(tp) = ui.enc.sim.actor(target).map(|a| a.pos) else {
        return from;
    };
    for e in &def.effects {
        match e {
            cc::Effect::Approach { distance } => from = from.step_toward(tp, *distance),
            cc::Effect::Withdraw { distance } => from = from.step_away(tp, *distance),
            _ => {}
        }
    }
    from
}

/// Execute the previewed tray slot for the pending player decision.
fn execute(ui: &mut CombatUi, slot: usize) {
    let Some(p) = ui.paused.as_ref() else { return };
    if !p.is_player {
        return;
    }
    let Some(view) = ui.view.clone() else { return };
    let cmd = match p.decision.kind {
        cc::DecisionKind::Readiness => {
            if slot >= view.own_moves.len() {
                cc::Command::Hold
            } else {
                cc::Command::CommitAction {
                    mv: view.own_moves[slot],
                    target: ui.target,
                }
            }
        }
        cc::DecisionKind::Dilation => dilation_cmd(&view, ui.target, slot),
    };
    ui.submit(cmd);
}

fn dilation_cmd(view: &cc::ForesightView, target: Option<cc::ActorId>, slot: usize) -> cc::Command {
    let foe_inst = target.and_then(|t| {
        view.instances
            .iter()
            .find(|i| i.actor == t && !i.own)
            .map(|i| i.id)
    });
    let verb = match slot {
        0 => foe_inst.map(|instance| cc::EditVerb::Slow {
            instance,
            ticks: EDIT_TICKS,
        }),
        1 => foe_inst.map(|instance| cc::EditVerb::Haste {
            instance,
            ticks: EDIT_TICKS,
        }),
        2 => foe_inst.map(|instance| cc::EditVerb::Interrupt { instance }),
        3 => view.own_moves.first().map(|&mv| cc::EditVerb::Insert {
            actor: view.observer,
            mv,
            target,
        }),
        _ => None,
    };
    match verb {
        Some(v) => cc::Command::EditVerb(v),
        None => cc::Command::Pass,
    }
}

// ── Input ────────────────────────────────────────────────────────────────────────────────────

pub(crate) fn combat_input(keys: Res<ButtonInput<KeyCode>>, mut game: NonSendMut<Game>) {
    let g = &mut *game;
    let Some(ui) = g.combat.as_mut() else { return };

    // Dismiss the end banner.
    if ui.ending.is_some() {
        if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
            g.combat = None;
        }
        return;
    }
    if !ui.at_decision() {
        return;
    }
    // Enemy decision: any advance applies it.
    if !ui.awaiting_player() {
        if (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space))
            && let Some(cmd) = ui.paused.as_ref().and_then(|p| p.enemy_cmd)
        {
            ui.submit(cmd);
        }
        return;
    }

    // Player decision.
    if keys.just_pressed(KeyCode::Tab) {
        cycle_target(ui);
    }
    if keys.just_pressed(KeyCode::Enter) {
        execute(ui, ui.sel);
        return;
    }
    // coupling-lint:allow const_all DIGITS: a keyboard binding table (digit keys → tray slots),
    // not content — what each slot does is the kit/edit-verb, which is data-driven.
    const DIGITS: [KeyCode; 8] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
    ];
    for (i, k) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*k) {
            ui.sel = i; // select + preview; Enter (or click) commits
        }
    }
}

fn cycle_target(ui: &mut CombatUi) {
    let foes: Vec<cc::ActorId> = ui
        .enc
        .combatants
        .iter()
        .filter(|c| !c.is_player_side && ui.enc.sim.actor(c.actor).is_some_and(|a| a.alive()))
        .map(|c| c.actor)
        .collect();
    if foes.is_empty() {
        return;
    }
    let cur = ui.target.and_then(|t| foes.iter().position(|&f| f == t));
    ui.target = Some(foes[cur.map(|i| (i + 1) % foes.len()).unwrap_or(0)]);
}

pub(crate) fn combat_clicks(
    mut game: NonSendMut<Game>,
    moves: Query<(&CombatMoveBtn, &Interaction), Changed<Interaction>>,
    hovers: Query<(&CombatMoveBtn, &Interaction)>,
    rows: Query<(&CombatRosterRow, &Interaction), Changed<Interaction>>,
) {
    let g = &mut *game;
    let Some(ui) = g.combat.as_mut() else { return };
    if ui.ending.is_some() {
        if moves.iter().any(|(_, i)| *i == Interaction::Pressed) {
            g.combat = None;
        }
        return;
    }
    // Hover previews the slot.
    for (btn, i) in &hovers {
        if *i == Interaction::Hovered {
            ui.sel = btn.0;
        }
    }
    // Click a foe row to target it.
    for (row, i) in &rows {
        if *i == Interaction::Pressed
            && let Some(c) = ui.enc.combatants.get(row.0)
            && !c.is_player_side
            && ui.enc.sim.actor(c.actor).is_some_and(|a| a.alive())
        {
            ui.target = Some(c.actor);
        }
    }
    if !ui.awaiting_player() {
        return;
    }
    for (btn, i) in &moves {
        if *i == Interaction::Pressed {
            execute(ui, btn.0);
            return;
        }
    }
}

// ── Field rendering ──────────────────────────────────────────────────────────────────────────

/// The handle of the live-rasterised field image.
#[derive(Resource)]
pub(crate) struct FieldImage(pub Handle<Image>);

pub(crate) fn combat_render_field(
    game: NonSend<Game>,
    img: Option<Res<FieldImage>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(ui), Some(img)) = (game.combat.as_ref(), img) else {
        return;
    };
    let active = ui.paused.as_ref().map(|p| p.decision.actor);
    let mut tokens = Vec::new();
    for c in &ui.enc.combatants {
        let Some(a) = ui.enc.sim.actor(c.actor) else {
            continue;
        };
        let pos = ui
            .vis
            .get(&c.actor.0)
            .copied()
            .unwrap_or_else(|| world(a.pos));
        let color = if !c.is_player_side {
            C_FOE
        } else if c.is_avatar {
            C_AVATAR
        } else {
            C_ALLY
        };
        tokens.push(Token {
            pos,
            color,
            hp_frac: if a.vitals.max_hp > 0 {
                (a.vitals.hp.max(0) as f32 / a.vitals.max_hp as f32).clamp(0.0, 1.0)
            } else {
                0.0
            },
            target: ui.target == Some(c.actor),
            active: active == Some(c.actor),
            flash: ui.flash.get(&c.actor.0).copied().unwrap_or(0.0),
            down: matches!(a.state, cc::ActorState::Down),
        });
    }

    // Reach ring + attack line for a pending player move.
    let mut reach_ring = None;
    let mut attack_line = None;
    if ui.awaiting_player()
        && let (Some(p), Some(view)) = (ui.paused.as_ref(), ui.view.as_ref())
        && view.own_moves.get(ui.sel).is_some()
    {
        let mv = view.own_moves[ui.sel];
        if let Some(def) = ui.content.move_def(mv) {
            let me = ui.vis.get(&p.decision.actor.0).copied();
            let reach = def.reach.0 as f32 / 65536.0;
            if let Some(me) = me {
                reach_ring = Some((me, reach));
                if let Some(t) = ui.target
                    && let Some(tp) = ui.vis.get(&t.0).copied()
                {
                    let after = pos_after_move(ui, p.decision.actor, t, &def);
                    let in_reach = ui
                        .enc
                        .sim
                        .actor(t)
                        .is_some_and(|tt| after.within(tt.pos, def.reach));
                    attack_line = Some((me, tp, in_reach));
                }
            }
        }
    }

    let view = FieldView {
        tokens: &tokens,
        reach_ring,
        attack_line,
    };
    let _ = images.insert(img.0.id(), combat_field::render(&view, FIELD_W, FIELD_H));
}

// ── UI nodes ─────────────────────────────────────────────────────────────────────────────────

#[derive(Component)]
pub(crate) struct CombatRoot;
#[derive(Component)]
pub(crate) struct CombatStatus;
#[derive(Component)]
pub(crate) struct CombatBanner;
#[derive(Component)]
pub(crate) struct CombatLog;
#[derive(Component)]
pub(crate) struct CombatPreview;
#[derive(Component)]
pub(crate) struct CombatRosterRow(usize);
#[derive(Component)]
pub(crate) struct CombatRosterName(usize);
#[derive(Component)]
pub(crate) struct CombatHpFill(usize);
#[derive(Component)]
pub(crate) struct CombatRosterMeta(usize);
#[derive(Component)]
pub(crate) struct CombatMoveBtn(usize);
#[derive(Component)]
pub(crate) struct CombatMoveLabel(usize);
#[derive(Component)]
pub(crate) struct CombatLaneRow(usize);
#[derive(Component)]
pub(crate) struct CombatLaneLabel(usize);
#[derive(Component)]
pub(crate) struct CombatCell {
    lane: usize,
    cell: usize,
}

// ── Spawn the scene (once, hidden) ───────────────────────────────────────────────────────────

pub(crate) fn spawn_combat_ui(
    commands: &mut Commands,
    fonts: &ThemeFonts,
    images: &mut Assets<Image>,
) {
    // A blank field image the renderer overwrites each frame.
    let blank = images.add(Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width: FIELD_W,
            height: FIELD_H,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        &[12, 14, 20, 255],
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    ));
    commands.insert_resource(FieldImage(blank.clone()));

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
            // Opaque — the exploration world never shows through.
            BackgroundColor(Color::srgb(0.035, 0.04, 0.055)),
            GlobalZIndex(90),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            // Header: title + status.
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

            // Middle: roster | field+timeline | tray.
            root.spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                column_gap: Val::Px(theme::SP_MD),
                ..default()
            })
            .with_children(|mid| {
                spawn_roster(mid, fonts);
                spawn_center(mid, fonts, blank.clone());
                spawn_tray(mid, fonts);
            });

            // Footer: legend + log.
            root.spawn(Node {
                width: Val::Percent(100.0),
                height: Val::Px(96.0),
                column_gap: Val::Px(theme::SP_MD),
                ..default()
            })
            .with_children(|foot| {
                foot.spawn((
                    Node {
                        width: Val::Px(330.0),
                        padding: UiRect::all(Val::Px(theme::SP_SM)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    },
                    theme::panel_chrome(),
                ))
                .with_children(|p| {
                    p.spawn(theme::label(fonts, "Legend"));
                    p.spawn(theme::micro(
                        fonts,
                        "tokens: blue you/ally · red foe · gold ring = acting · white ring = target",
                    ));
                    p.spawn(theme::micro(
                        fonts,
                        "green arc = that token's HP · attack line cyan = in reach, red = would whiff",
                    ));
                    p.spawn(theme::micro(
                        fonts,
                        "timeline: blue wind-up / red strike / grey recovery · gold = the playhead (now)",
                    ));
                });
                foot.spawn((
                    Node {
                        flex_grow: 1.0,
                        padding: UiRect::all(Val::Px(theme::SP_SM)),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    theme::panel_chrome(),
                ))
                .with_children(|p| {
                    p.spawn((theme::body(fonts, ""), CombatLog));
                });
            });
        });

    // Centre-screen banner for enemy choices / the result.
    commands
        .spawn((
            CombatBanner,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(28.0),
                top: Val::Percent(42.0),
                width: Val::Percent(44.0),
                padding: UiRect::all(Val::Px(theme::SP_MD)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            theme::panel_chrome(),
            GlobalZIndex(95),
            Visibility::Hidden,
        ))
        .with_children(|b| {
            b.spawn((theme::heading(fonts, ""), CombatBannerText));
        });
}

#[derive(Component)]
pub(crate) struct CombatBannerText;

fn spawn_roster(parent: &mut ChildSpawnerCommands, fonts: &ThemeFonts) {
    parent
        .spawn((
            Node {
                width: Val::Px(210.0),
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
                ))
                .with_children(|row| {
                    row.spawn((theme::label(fonts, ""), CombatRosterName(i)));
                    row.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(5.0),
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

fn spawn_center(parent: &mut ChildSpawnerCommands, fonts: &ThemeFonts, field: Handle<Image>) {
    parent
        .spawn(Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(theme::SP_SM),
            ..default()
        })
        .with_children(|col| {
            // The field image.
            col.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    ..default()
                },
                ImageNode::new(field),
                theme::panel_chrome(),
            ));
            // The timeline ribbon with a playhead.
            col.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(124.0),
                    padding: UiRect::all(Val::Px(theme::SP_SM)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                },
                theme::panel_chrome(),
            ))
            .with_children(|band| {
                band.spawn(theme::label(fonts, "The next few seconds"));
                // Lanes + an overlaid playhead (absolute, spanning the lanes).
                band.spawn(Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    ..default()
                })
                .with_children(|lanes| {
                    for lane in 0..LANES {
                        lanes
                            .spawn((
                                CombatLaneRow(lane),
                                Node {
                                    width: Val::Percent(100.0),
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(6.0),
                                    display: Display::None,
                                    ..default()
                                },
                            ))
                            .with_children(|row| {
                                row.spawn((
                                    CombatLaneLabel(lane),
                                    Node {
                                        width: Val::Px(78.0),
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
                                                height: Val::Px(9.0),
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
        });
}

fn spawn_tray(parent: &mut ChildSpawnerCommands, fonts: &ThemeFonts) {
    parent
        .spawn((
            Node {
                width: Val::Px(212.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme::SP_SM),
                padding: UiRect::all(Val::Px(theme::SP_SM)),
                ..default()
            },
            theme::panel_chrome(),
        ))
        .with_children(|col| {
            col.spawn(theme::label(fonts, "Moves"));
            for i in 0..TRAY_SLOTS {
                col.spawn((
                    CombatMoveBtn(i),
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(theme::SP_SM), Val::Px(5.0)),
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(theme::RADIUS_SM)),
                        display: Display::None,
                        ..default()
                    },
                    BackgroundColor(theme::INK_RAISED),
                    BorderColor::all(theme::BORDER),
                ))
                .with_children(|b| {
                    b.spawn((theme::body(fonts, ""), CombatMoveLabel(i)));
                });
            }
            col.spawn(theme::divider());
            // The preview of the selected move.
            col.spawn((theme::micro(fonts, ""), CombatPreview));
        });
}

// ── Per-frame UI fill (split into small systems to keep query access disjoint) ────────────────

/// Root visibility, the status line, the centre banner (enemy choice / result), the log, and the
/// move preview.
#[allow(clippy::type_complexity)]
pub(crate) fn update_combat_chrome(
    game: NonSend<Game>,
    mut root: Query<&mut Visibility, (With<CombatRoot>, Without<CombatBanner>)>,
    mut banner: Query<&mut Visibility, (With<CombatBanner>, Without<CombatRoot>)>,
    mut status: Query<
        &mut Text,
        (
            With<CombatStatus>,
            Without<CombatBannerText>,
            Without<CombatLog>,
            Without<CombatPreview>,
        ),
    >,
    mut banner_text: Query<
        &mut Text,
        (
            With<CombatBannerText>,
            Without<CombatStatus>,
            Without<CombatLog>,
            Without<CombatPreview>,
        ),
    >,
    mut log: Query<
        &mut Text,
        (
            With<CombatLog>,
            Without<CombatStatus>,
            Without<CombatBannerText>,
            Without<CombatPreview>,
        ),
    >,
    mut preview: Query<
        &mut Text,
        (
            With<CombatPreview>,
            Without<CombatStatus>,
            Without<CombatBannerText>,
            Without<CombatLog>,
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
        if let Ok(mut v) = banner.single_mut() {
            *v = Visibility::Hidden;
        }
        return;
    };
    let (status_s, banner_s) = headline(ui);
    if let Ok(mut t) = status.single_mut() {
        t.0 = status_s;
    }
    if let Ok(mut v) = banner.single_mut() {
        *v = if banner_s.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if let (Ok(mut t), Some(s)) = (banner_text.single_mut(), banner_s) {
        t.0 = s;
    }
    if let Ok(mut t) = log.single_mut() {
        t.0 = ui.log.join("\n");
    }
    if let Ok(mut t) = preview.single_mut() {
        t.0 = if ui.awaiting_player() {
            preview_text(ui)
        } else {
            String::new()
        };
    }
}

/// The combatant roster: names, HP bars, and the per-combatant meta line.
pub(crate) fn update_combat_roster(
    game: NonSend<Game>,
    mut rows: Query<(&CombatRosterRow, &mut Node, &mut BackgroundColor), Without<CombatHpFill>>,
    mut names: Query<(&CombatRosterName, &mut Text), Without<CombatRosterMeta>>,
    mut metas: Query<(&CombatRosterMeta, &mut Text), Without<CombatRosterName>>,
    mut hp: Query<(&CombatHpFill, &mut Node), Without<CombatRosterRow>>,
) {
    let Some(ui) = game.combat.as_ref() else {
        return;
    };
    for (row, mut node, mut bg) in &mut rows {
        let c = ui.enc.combatants.get(row.0);
        node.display = if c.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(c) = c {
            bg.0 = if ui.target == Some(c.actor) {
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
                let tag = if c.is_player_side { "" } else { "\u{2694} " };
                format!("{tag}{}", c.name)
            })
            .unwrap_or_default();
    }
    for (slot, mut node) in &mut hp {
        node.width = Val::Percent(combatant_hp(ui, slot.0) * 100.0);
    }
    for (slot, mut text) in &mut metas {
        text.0 = combatant_meta(ui, slot.0);
    }
}

/// The move tray: which slots are shown, their labels, and the selected highlight.
pub(crate) fn update_combat_tray(
    game: NonSend<Game>,
    mut btns: Query<(&CombatMoveBtn, &mut Node, &mut BorderColor)>,
    mut labels: Query<(&CombatMoveLabel, &mut Text)>,
) {
    let Some(ui) = game.combat.as_ref() else {
        return;
    };
    let labels_v = tray_labels(ui);
    let show = ui.awaiting_player();
    for (btn, mut node, mut border) in &mut btns {
        let has = show && btn.0 < labels_v.len();
        node.display = if has { Display::Flex } else { Display::None };
        *border = BorderColor::all(if has && btn.0 == ui.sel {
            theme::AWE
        } else {
            theme::BORDER
        });
    }
    for (slot, mut text) in &mut labels {
        text.0 = labels_v.get(slot.0).cloned().unwrap_or_default();
    }
}

/// The timeline ribbon: lanes, labels, phase-coloured cells, and the playhead column.
pub(crate) fn update_combat_timeline(
    game: NonSend<Game>,
    mut lanes: Query<(&CombatLaneRow, &mut Node)>,
    mut lane_labels: Query<(&CombatLaneLabel, &mut Text)>,
    mut cells: Query<(&CombatCell, &mut BackgroundColor)>,
) {
    let Some(ui) = game.combat.as_ref() else {
        return;
    };
    let shown = ui.enc.combatants.len().min(LANES);
    for (lane, mut node) in &mut lanes {
        node.display = if lane.0 < shown {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (label, mut text) in &mut lane_labels {
        text.0 = ui
            .enc
            .combatants
            .get(label.0)
            .map(|c| c.name.chars().take(9).collect())
            .unwrap_or_default();
    }
    let now = ui.view.as_ref().map(|v| v.current_tick.0).unwrap_or(0);
    let playhead = ui.clock.max(now as f32);
    for (cell, mut bg) in &mut cells {
        bg.0 = cell_color(ui, cell, now, playhead);
    }
}

fn headline(ui: &CombatUi) -> (String, Option<String>) {
    if let Some(e) = &ui.ending {
        let s = if e.avatar_down {
            "You have fallen."
        } else if e.victory {
            "Victory."
        } else {
            "The fight is over."
        };
        return (
            s.into(),
            Some(format!("{s}\n\nEnter \u{2014} return to the world")),
        );
    }
    if !ui.at_decision() {
        return ("\u{2026}".into(), None);
    }
    if ui.awaiting_player() {
        let s = match ui.paused.as_ref().map(|p| p.decision.kind) {
            Some(cc::DecisionKind::Dilation) => "Bend the line, or pass.",
            _ => "Choose an action.",
        };
        (s.into(), None)
    } else {
        let p = ui.paused.as_ref().unwrap();
        let who = name_of(&ui.enc, p.decision.actor);
        let what = enemy_choice_text(ui, p);
        (
            "The foe acts".into(),
            Some(format!("{who}: {what}\n\nEnter \u{2014} continue")),
        )
    }
}

fn enemy_choice_text(ui: &CombatUi, p: &Paused) -> String {
    match p.enemy_cmd {
        Some(cc::Command::CommitAction { mv, target }) => {
            let name = ui.content.move_def(mv).map(|d| d.name).unwrap_or_default();
            match target {
                Some(t) => format!("{name} \u{2192} {}", name_of(&ui.enc, t)),
                None => name,
            }
        }
        Some(cc::Command::EditVerb(_)) => "bends the line".into(),
        Some(cc::Command::Hold) | None => "holds".into(),
        Some(_) => "waits".into(),
    }
}

fn combatant_hp(ui: &CombatUi, slot: usize) -> f32 {
    ui.enc
        .combatants
        .get(slot)
        .and_then(|c| ui.enc.sim.actor(c.actor))
        .map(|a| {
            if a.vitals.max_hp > 0 {
                (a.vitals.hp.max(0) as f32 / a.vitals.max_hp as f32).clamp(0.0, 1.0)
            } else {
                0.0
            }
        })
        .unwrap_or(0.0)
}

fn combatant_meta(ui: &CombatUi, slot: usize) -> String {
    let Some(c) = ui.enc.combatants.get(slot) else {
        return String::new();
    };
    match ui.enc.sim.actor(c.actor) {
        Some(a) if matches!(a.state, cc::ActorState::Down) => "down".into(),
        Some(a) if c.is_player_side => {
            format!(
                "hp {}/{}  tempo {}",
                a.vitals.hp.max(0),
                a.vitals.max_hp,
                a.tempo
            )
        }
        Some(a) => format!("hp {}/{}", a.vitals.hp.max(0), a.vitals.max_hp),
        None => String::new(),
    }
}

fn cell_color(ui: &CombatUi, cell: &CombatCell, now: u64, playhead: f32) -> Color {
    let Some(c) = ui.enc.combatants.get(cell.lane) else {
        return theme::INK_SUNKEN;
    };
    let t = now + cell.cell as u64;
    if (t as f32) <= playhead && (t as f32) > playhead - 1.0 {
        return theme::AWE; // the playhead column
    }
    let phase = ui.view.as_ref().and_then(|v| {
        v.instances.iter().find(|i| i.actor == c.actor).map(|i| {
            if t >= i.start_tick.0 && t < i.active_start.0 {
                0
            } else if t >= i.active_start.0 && t < i.active_end.0 {
                1
            } else if t >= i.active_end.0 && t < i.recovery_end.0 {
                2
            } else {
                3
            }
        })
    });
    match phase {
        Some(0) => theme::DREAD.with_alpha(0.7),
        Some(1) => theme::BLOOD,
        Some(2) => theme::TEXT_FAINT.with_alpha(0.5),
        _ => theme::INK_SUNKEN,
    }
}
