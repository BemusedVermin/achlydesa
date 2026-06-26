//! **The conversation panel.** When the avatar falls into talk with a soul (T, or the Talk action),
//! a framed panel opens over the centre of the view — laid out to the mockup:
//!
//! * **Speaker's Portrait** (left) — the soul's sigil disc, name, and live disposition toward you,
//! * **Dialog** (top-right) — the exchange so far, revealed RPG-style,
//! * **Speak Choices** (bottom-right) — clickable social acts (deterministic, model-free) plus the
//!   free-text line you type when the voice model is up.
//!
//! The panel is purely a *view*: it reads the open [`crate::Game::convo`] and writes nothing but the
//! social effect of a clicked choice (the same `player_talk` the keys used). It is hidden whenever
//! there is no open conversation, so a build without it is unaffected.

use app::theme::{self, ThemeFonts};
use bevy::prelude::*;
use bevy::ui::{BorderRadius, GlobalZIndex, Overflow};

use crate::hud;
use crate::{Game, Line};

// ── Components ───────────────────────────────────────────────────────────────────────────────────

#[derive(Component)]
pub struct ConvoRoot;
#[derive(Component)]
pub struct ConvoPortrait;
/// A clickable speak choice — an index into [`crate::QUICK_ACTS`].
#[derive(Component)]
pub struct SpeakChoice(pub usize);

/// A clickable **intervention** — directly move the soul's drama (`true` = counsel toward peace,
/// `false` = stoke its grievance). The lever on the director's threads (see `player_counsel`).
#[derive(Component)]
pub struct CounselChoice(pub bool);

/// The "take up the charge" button — shown only when the soul in focus is offering a quest
/// ([`crate::Convo::offer`]). Accepting adds it to the avatar's charges.
#[derive(Component)]
pub struct QuestAccept;
#[derive(Component)]
pub struct QuestAcceptLabel;

/// The "speak with whom?" chooser shown when several souls are in reach.
#[derive(Component)]
pub struct TalkChooserRoot;
/// One chooser row (a pooled button) — `usize` indexes the candidate list.
#[derive(Component)]
pub struct TalkRow(pub usize);
#[derive(Component)]
pub struct TalkRowLabel(pub usize);

/// How many nearby souls the chooser can list at once.
const TALK_ROWS: usize = 8;

/// The text roles in the panel, so one query can fill them all (each node carries its own font/colour
/// from spawn; the update only swaps the string).
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum ConvoText {
    Name,
    Dispo,
    Dialog,
    Footer,
}

// ── Palette / geometry ──────────────────────────────────────────────────────────────────────────

const PORTRAIT_D: f32 = 168.0;
const CHOICE_BG: Color = theme::INK_RAISED;
const CHOICE_BG_HOT: Color = Color::srgba(0.18, 0.21, 0.28, 0.96);

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

// ── Spawn ────────────────────────────────────────────────────────────────────────────────────────

pub fn spawn(commands: &mut Commands, f: &ThemeFonts) {
    let px = Val::Px;
    let well = || {
        (
            Node {
                padding: UiRect::all(px(theme::SP_MD)),
                border: UiRect::all(px(theme::BORDER_W)),
                border_radius: BorderRadius::all(px(theme::RADIUS_SM)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::INK_SUNKEN),
            BorderColor::all(theme::BORDER),
        )
    };

    commands
        .spawn((
            ConvoRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(hud::LEFT_W + 56.0),
                right: px(hud::RIGHT_W + 56.0),
                top: px(hud::TOP_H + 64.0),
                bottom: px(hud::BOTTOM_H + 32.0),
                padding: UiRect::all(px(theme::SP_LG)),
                border: UiRect::all(px(2.0)),
                border_radius: BorderRadius::all(px(theme::RADIUS)),
                flex_direction: FlexDirection::Row,
                column_gap: px(theme::SP_LG),
                ..default()
            },
            theme::panel_chrome(),
            GlobalZIndex(50),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            // Speaker's Portrait (left, full height).
            root.spawn(Node {
                width: px(224.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(theme::SP_MD),
                ..default()
            })
            .with_children(|col| {
                // The speaker's procedural pixel **bust** — head and shoulders, sitting on the panel.
                // The image is filled per-conversation by `update_convo_portrait`.
                col.spawn((
                    ConvoPortrait,
                    Node {
                        width: px(PORTRAIT_D * 1.1),
                        height: px(PORTRAIT_D * 1.25),
                        ..default()
                    },
                    ImageNode::default(),
                ));
                col.spawn((
                    ConvoText::Name,
                    theme::serif(f, "", theme::T_TITLE, theme::HEADING),
                ));
                col.spawn((
                    ConvoText::Dispo,
                    Node {
                        max_width: px(210.0),
                        ..default()
                    },
                    Text::new(""),
                    TextFont {
                        font: f.mono.clone(),
                        font_size: theme::T_LABEL,
                        ..default()
                    },
                    TextColor(theme::TEXT_DIM),
                ));
            });

            // Right column: Dialog (grows) over Speak Choices.
            root.spawn(Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(theme::SP_MD),
                ..default()
            })
            .with_children(|right| {
                // Dialog well.
                right
                    .spawn((
                        Node {
                            flex_grow: 1.0,
                            ..well().0
                        },
                        well().1,
                        well().2,
                    ))
                    .with_children(|b| {
                        b.spawn((
                            ConvoText::Dialog,
                            Node {
                                width: Val::Percent(100.0),
                                ..default()
                            },
                            Text::new(""),
                            TextFont {
                                font: f.mono.clone(),
                                font_size: theme::T_BODY,
                                ..default()
                            },
                            TextColor(theme::TEXT),
                        ));
                    });
                // Speak-choices well.
                right
                    .spawn((
                        Node {
                            height: px(190.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(theme::SP_SM),
                            justify_content: JustifyContent::SpaceBetween,
                            ..well().0
                        },
                        well().1,
                        well().2,
                    ))
                    .with_children(|sc| {
                        // The charge on offer (a thread figure's request) — shown only when there
                        // is one (toggled by `update_quest_offer`).
                        sc.spawn((
                            QuestAccept,
                            Button,
                            Node {
                                display: Display::None,
                                width: Val::Percent(100.0),
                                padding: UiRect::axes(px(theme::SP_MD), px(theme::SP_XS)),
                                border: UiRect::all(px(theme::BORDER_W)),
                                border_radius: BorderRadius::all(px(theme::RADIUS_SM)),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(CHOICE_BG),
                            BorderColor::all(theme::AWE),
                        ))
                        .with_children(|b| {
                            b.spawn((
                                QuestAcceptLabel,
                                theme::mono(f, "", theme::T_LABEL, theme::HEADING),
                            ));
                        });
                        // The deterministic social acts, as clickable choices.
                        sc.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(theme::SP_SM),
                            row_gap: px(theme::SP_XS),
                            ..default()
                        })
                        .with_children(|row| {
                            for (i, (_, _, verb)) in crate::QUICK_ACTS.iter().enumerate() {
                                row.spawn((
                                    SpeakChoice(i),
                                    Button,
                                    Node {
                                        padding: UiRect::axes(px(theme::SP_MD), px(theme::SP_XS)),
                                        border: UiRect::all(px(theme::BORDER_W)),
                                        border_radius: BorderRadius::all(px(theme::RADIUS_SM)),
                                        ..default()
                                    },
                                    BackgroundColor(CHOICE_BG),
                                    BorderColor::all(theme::BORDER),
                                ))
                                .with_children(|b| {
                                    b.spawn(theme::mono(f, cap(verb), theme::T_LABEL, theme::TEXT));
                                });
                            }
                        });
                        // Intervene in the drama: talk the soul down, or feed its grievance — a
                        // real lever on the director's threads (persuasion-scaled).
                        sc.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(theme::SP_SM),
                            ..default()
                        })
                        .with_children(|row| {
                            for (calm, label) in
                                [(true, "counsel peace"), (false, "stoke grievance")]
                            {
                                let edge = if calm {
                                    theme::AWE
                                } else {
                                    Color::srgb(0.80, 0.42, 0.38)
                                };
                                row.spawn((
                                    CounselChoice(calm),
                                    Button,
                                    Node {
                                        padding: UiRect::axes(px(theme::SP_MD), px(theme::SP_XS)),
                                        border: UiRect::all(px(theme::BORDER_W)),
                                        border_radius: BorderRadius::all(px(theme::RADIUS_SM)),
                                        ..default()
                                    },
                                    BackgroundColor(CHOICE_BG),
                                    BorderColor::all(edge),
                                ))
                                .with_children(|b| {
                                    b.spawn(theme::mono(
                                        f,
                                        cap(label),
                                        theme::T_LABEL,
                                        theme::TEXT,
                                    ));
                                });
                            }
                        });
                        // The free-text line / prompt.
                        sc.spawn((
                            ConvoText::Footer,
                            Text::new(""),
                            TextFont {
                                font: f.mono.clone(),
                                font_size: theme::T_LABEL,
                                ..default()
                            },
                            TextColor(theme::TEXT_DIM),
                        ));
                    });
            });
        });

    spawn_chooser(commands, f);
}

/// The "speak with whom?" chooser: a centred panel with a pooled list of soul buttons, hidden until
/// several souls are in reach when Talk is pressed.
fn spawn_chooser(commands: &mut Commands, f: &ThemeFonts) {
    let px = Val::Px;
    commands
        .spawn((
            TalkChooserRoot,
            Node {
                position_type: PositionType::Absolute,
                top: px(0.0),
                left: px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.45)),
            GlobalZIndex(60),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(380.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(theme::SP_SM),
                    padding: UiRect::all(px(theme::SP_LG)),
                    border: UiRect::all(px(2.0)),
                    border_radius: BorderRadius::all(px(theme::RADIUS)),
                    ..default()
                },
                theme::panel_chrome(),
            ))
            .with_children(|panel| {
                panel.spawn(theme::serif(
                    f,
                    "Speak with whom?",
                    theme::T_TITLE,
                    theme::HEADING,
                ));
                panel.spawn(theme::divider());
                for i in 0..TALK_ROWS {
                    panel
                        .spawn((
                            TalkRow(i),
                            Button,
                            Node {
                                width: Val::Percent(100.0),
                                padding: UiRect::axes(px(theme::SP_MD), px(theme::SP_SM)),
                                border: UiRect::all(px(theme::BORDER_W)),
                                border_radius: BorderRadius::all(px(theme::RADIUS_SM)),
                                ..default()
                            },
                            BackgroundColor(CHOICE_BG),
                            BorderColor::all(theme::BORDER),
                        ))
                        .with_children(|b| {
                            b.spawn((
                                TalkRowLabel(i),
                                Text::new(""),
                                TextFont {
                                    font: f.mono.clone(),
                                    font_size: theme::T_BODY,
                                    ..default()
                                },
                                TextColor(theme::TEXT),
                            ));
                        });
                }
                panel.spawn(theme::micro(f, "Esc to cancel"));
            });
        });
}

// ── Update ───────────────────────────────────────────────────────────────────────────────────────

/// Fill the panel from the open conversation (or hide it). Reads only — the social effect of a
/// clicked choice lands in [`speak_choice_click`].
pub fn update_convo_panel(
    game: NonSend<Game>,
    time: Res<Time>,
    mut root: Query<&mut Visibility, With<ConvoRoot>>,
    mut texts: Query<(&ConvoText, &mut Text)>,
) {
    let Some(c) = &game.convo else {
        if let Ok(mut v) = root.single_mut() {
            *v = Visibility::Hidden;
        }
        return;
    };
    if let Ok(mut v) = root.single_mut() {
        *v = Visibility::Visible;
    }

    // The soul's live disposition toward you, shifting as choices land.
    let avatar = game.sim.player_avatar();
    let dispo = {
        let op = avatar
            .and_then(|a| game.sim.opinion_of(c.listener, a))
            .unwrap_or(0.0);
        let word = crate::disposition_word(op);
        if avatar.is_some_and(|a| game.sim.bears_grudge(c.listener, a)) {
            format!("{word}, and bears an old grudge")
        } else {
            word.to_string()
        }
    };

    // (The speaker's bust is drawn by `update_convo_portrait`, not here.)

    // The transcript, revealed RPG-style; an animated ellipsis while a reply is still generating.
    let dots = [".", "..", "..."][((time.elapsed_secs() * 3.0) as usize) % 3];
    let mut dialog = String::new();
    for ln in &c.transcript {
        dialog.push_str(&ln.prefix);
        match &ln.text {
            Some(t) => dialog.extend(t.chars().take(ln.reveal as usize)),
            None => dialog.push_str(dots),
        }
        dialog.push('\n');
    }

    let footer = if game.voice.is_ready() {
        format!(
            "> {}_\n(type & Enter to speak · click a choice · Esc to leave)",
            c.input
        )
    } else {
        "the voice sleeps — choose what to say, or Esc to leave".to_string()
    };

    for (role, mut text) in &mut texts {
        text.0 = match role {
            ConvoText::Name => c.name.clone(),
            ConvoText::Dispo => dispo.clone(),
            ConvoText::Dialog => dialog.clone(),
            ConvoText::Footer => footer.clone(),
        };
    }
}

/// Light the speak-choice buttons on hover.
pub fn style_speak_choices(mut q: Query<(&Interaction, &mut BackgroundColor), With<SpeakChoice>>) {
    for (interaction, mut bg) in &mut q {
        bg.0 = if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            CHOICE_BG_HOT
        } else {
            CHOICE_BG
        };
    }
}

/// Apply a clicked speak choice — the deterministic, model-free social act on the soul you're talking
/// to. Lands the authored intent (scaled by the avatar's speech skill) and notes it in the dialog.
pub fn speak_choice_click(
    mut game: NonSendMut<Game>,
    q: Query<(&SpeakChoice, &Interaction), Changed<Interaction>>,
) {
    let Some(idx) = q
        .iter()
        .find(|(_, i)| **i == Interaction::Pressed)
        .map(|(s, _)| s.0)
    else {
        return;
    };
    let Some(&(_, intent, verb)) = crate::QUICK_ACTS.get(idx) else {
        return;
    };
    let g = &mut *game;
    let Some(npc) = g.convo.as_ref().map(|c| c.listener) else {
        return;
    };
    let name = g.sim.display_name(npc);
    g.sim.player_talk(npc, intent);
    if let Some(c) = g.convo.as_mut() {
        c.transcript.push(Line {
            from_player: true,
            prefix: String::new(),
            text: Some(format!("[you {verb} {name}]")),
            reveal: f32::MAX,
            pending: None,
        });
        let overflow = c.transcript.len().saturating_sub(10);
        if overflow > 0 {
            c.transcript.drain(0..overflow);
        }
    }
    let dispo = g
        .sim
        .player_avatar()
        .and_then(|a| g.sim.opinion_of(npc, a))
        .map(crate::disposition_word)
        .unwrap_or("unmoved");
    g.status = format!("You {verb} {name}. {name} now {dispo}.");
}

/// Light the intervention buttons on hover (the same feel as the speak choices).
pub fn style_counsel_choices(
    mut q: Query<(&Interaction, &mut BackgroundColor), With<CounselChoice>>,
) {
    for (interaction, mut bg) in &mut q {
        bg.0 = if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            CHOICE_BG_HOT
        } else {
            CHOICE_BG
        };
    }
}

/// Apply a clicked intervention — counsel the soul toward peace, or stoke its grievance — through
/// `player_counsel` (persuasion-scaled), noting what your words did in the dialog and the status.
pub fn counsel_click(
    mut game: NonSendMut<Game>,
    q: Query<(&CounselChoice, &Interaction), Changed<Interaction>>,
) {
    let Some(calm) = q
        .iter()
        .find(|(_, i)| **i == Interaction::Pressed)
        .map(|(c, _)| c.0)
    else {
        return;
    };
    let g = &mut *game;
    let Some(npc) = g.convo.as_ref().map(|c| c.listener) else {
        return;
    };
    let name = g.sim.display_name(npc);
    if let Some(outcome) = g.sim.player_counsel(npc, calm) {
        if let Some(c) = g.convo.as_mut() {
            let verb = if calm { "counsel peace with" } else { "goad" };
            c.transcript.push(Line {
                from_player: true,
                prefix: String::new(),
                text: Some(format!("[you {verb} {name}]")),
                reveal: f32::MAX,
                pending: None,
            });
            let overflow = c.transcript.len().saturating_sub(10);
            if overflow > 0 {
                c.transcript.drain(0..overflow);
            }
        }
        g.status = outcome;
    }
}

/// Show/hide the "take up the charge" button from the conversation's pending offer, and label it with
/// the objective.
pub fn update_quest_offer(
    game: NonSend<Game>,
    mut btn: Query<&mut Node, With<QuestAccept>>,
    mut label: Query<&mut Text, With<QuestAcceptLabel>>,
) {
    let offer = game.convo.as_ref().and_then(|c| c.offer.as_ref());
    if let Ok(mut node) = btn.single_mut() {
        node.display = if offer.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Ok(mut text) = label.single_mut() {
        text.0 = offer
            .map(|q| format!("Take up the charge: {}", q.objective))
            .unwrap_or_default();
    }
}

/// Accept the offered charge — move it from the conversation's offer into the avatar's charges, and
/// note it in the dialog and the status line.
pub fn quest_accept_click(
    mut game: NonSendMut<Game>,
    q: Query<&Interaction, (With<QuestAccept>, Changed<Interaction>)>,
) {
    if !q.iter().any(|i| *i == Interaction::Pressed) {
        return;
    }
    let g = &mut *game;
    let Some(offer) = g.convo.as_mut().and_then(|c| c.offer.take()) else {
        return;
    };
    let obj = offer.objective.clone();
    let giver = offer.giver_name.clone();
    g.quests.push(offer);
    if let Some(c) = g.convo.as_mut() {
        c.transcript.push(Line {
            from_player: true,
            prefix: String::new(),
            text: Some(format!("[you take up {giver}'s charge]")),
            reveal: f32::MAX,
            pending: None,
        });
    }
    g.status = format!("Charge taken \u{2014} {obj}");
}

// ── The who-to-talk-to chooser ────────────────────────────────────────────────────────────────────

/// Show/fill the chooser from the snapshotted candidates: a row per soul (name + archetype), the
/// rest hidden, lit on hover.
pub fn update_talk_chooser(
    game: NonSend<Game>,
    mut root: Query<&mut Visibility, With<TalkChooserRoot>>,
    mut rows: Query<(&TalkRow, &Interaction, &mut Node, &mut BackgroundColor)>,
    mut labels: Query<(&TalkRowLabel, &mut Text)>,
) {
    let active = game.talk_choices.is_some() && game.convo.is_none() && !game.paused;
    if let Ok(mut v) = root.single_mut() {
        *v = if active {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !active {
        return;
    }
    let choices = game.talk_choices.as_ref().unwrap();
    let names: Vec<String> = choices
        .iter()
        .take(TALK_ROWS)
        .map(|&e| {
            let n = game.sim.display_name(e);
            // A soul the director has woven into a story wears its arc here ("the Betrayed"), so the
            // figures who matter stand out from the crowd; otherwise its archetype.
            match game
                .sim
                .npc_epithet(e)
                .or_else(|| game.sim.archetype_of(e).map(str::to_string))
            {
                Some(tag) => format!("{n}  —  {tag}"),
                None => n,
            }
        })
        .collect();
    for (row, interaction, mut node, mut bg) in &mut rows {
        let shown = row.0 < names.len();
        node.display = if shown { Display::Flex } else { Display::None };
        bg.0 = if shown && matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            CHOICE_BG_HOT
        } else {
            CHOICE_BG
        };
    }
    for (lbl, mut text) in &mut labels {
        text.0 = names.get(lbl.0).cloned().unwrap_or_default();
    }
}

/// Clicking a chooser row opens the conversation with that soul.
pub fn talk_row_click(
    mut game: NonSendMut<Game>,
    q: Query<(&TalkRow, &Interaction), Changed<Interaction>>,
) {
    let Some(i) = q
        .iter()
        .find(|(_, it)| **it == Interaction::Pressed)
        .map(|(r, _)| r.0)
    else {
        return;
    };
    let pick = game.talk_choices.as_ref().and_then(|v| v.get(i)).copied();
    if let Some(e) = pick {
        let g = &mut *game;
        g.talk_choices = None;
        crate::open_conversation_with(g, e);
    }
}
