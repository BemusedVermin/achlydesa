//! Turning a grounded [`Utterance`] into a ChatML prompt for the instruct model.
//!
//! Everything the model sees is *already true in the simulation* — the speaker's identity,
//! motive, mood, standing toward the listener, the speech act, the referent, and the line it
//! is answering. The system turn pins the model to voicing only that (the doc's "knowledge
//! gating"): one short, in-character line, inventing nothing. Including the prior line is what
//! makes the exchange read as a conversation rather than two isolated barks.
//!
//! Two hard-won shaping decisions, both from watching real output:
//! - The prompt carries **no grammar draft**. A small model handed a finished sentence copies
//!   it verbatim — which made ~80% of generations come back byte-identical to the grammar.
//! - The facts are framed as private **notes** the character must *express, not recite*, with a
//!   single worked **example**. Without that, a 1.5B model leaks the notes ("I feel warm… I am
//!   wary of my standing toward X"), prefixes a name label, or slips into narration.

use agents::Utterance;

/// The standing instruction + two worked examples. The examples do the heavy lifting for a
/// small model: they demonstrate notes → a single natural spoken line, in first person / direct
/// address, with no narration, no note-reciting, and no "You ..." stage directions (an early
/// example that opened with "You speak ..." taught the model to narrate in the second person).
const SYSTEM: &str = "You voice one character in a grim, low-fantasy medieval world. You are \
given private notes about the character and the moment. Reply with ONE short spoken line — a \
single sentence, at most 25 words — that the character says ALOUD, in the first person.\n\
Rules:\n\
- Output only the words the character speaks: no narration, no description of actions, no \
\"You ...\" stage directions, no quotation marks, no name label.\n\
- Speak as \"I\", or address the other by name; never describe yourself from the outside.\n\
- Never recite the notes (do not say \"I feel ...\" or \"my standing is ...\"); let them shape your words.\n\
- Use only what the notes give you; invent no new facts, names, places, or events.\n\n\
Example 1\n\
Notes: You are Bram, a vengeful soul. Mood: seething. Toward Sera, you feel resentment. Sera \
just said: \"Peace, brother.\" You move to accuse Sera, about the broken oath between you.\n\
Reply: Peace, Sera? You broke your oath and left my brother to the winter — I have not forgotten.\n\n\
Example 2\n\
Notes: You are Mira, a warm-hearted soul. Mood: tender. Toward Doran, you feel warmth. Doran \
just said: \"I have lost everything.\" You move to console Doran, about his grief.\n\
Reply: Doran, this grief will ease in time, and you need not carry it alone until it does.";

/// Relation verb-phrases from the sim (third person) → a feeling noun, so the notes read
/// cleanly ("you feel wariness") rather than ungrammatically ("you feel is wary of").
fn relation_noun(rel: &str) -> &str {
    match rel {
        "loathes" => "loathing",
        "resents" => "resentment",
        "is wary of" => "wariness",
        "warms to" => "warmth",
        "is devoted to" => "devotion",
        other => other,
    }
}

/// Assemble the ChatML prompt for `u`, optionally answering `prev` (the line just said to the
/// speaker). Conveys the grounded facts as notes only — never a draft sentence to copy.
pub fn build_chatml(u: &Utterance, prev: Option<&str>) -> String {
    let motive = if u.motive.is_empty() { "an unremarkable soul".to_string() } else { format!("a {} soul", u.motive.join(", ")) };

    let mut notes = String::new();
    notes.push_str(&format!("You are {}, {}. ", u.speaker_name, motive));
    notes.push_str(&format!("Mood: {}. ", u.mood_word));
    notes.push_str(&format!("Toward {}, you feel {}. ", u.listener_name, relation_noun(u.relation_word)));
    if let Some(prev) = prev.map(str::trim).filter(|p| !p.is_empty()) {
        notes.push_str(&format!("{} just said: \"{}\" ", u.listener_name, prev));
    }
    let about = match &u.referent {
        Some(r) => format!(", about {r}"),
        None => String::new(),
    };
    notes.push_str(&format!("You move to {} {}{}.", u.act.key(), u.listener_name, about));

    format!("<|im_start|>system\n{SYSTEM}<|im_end|>\n<|im_start|>user\nNotes: {notes}<|im_end|>\n<|im_start|>assistant\n")
}

// =====================================================================================
// Free-text conversation — the player types, the character answers in its own voice.
// =====================================================================================

/// One turn of an open conversation, for assembling multi-turn ChatML.
#[derive(Clone, Debug)]
pub struct ChatTurn {
    /// `true` if the player said it (a ChatML `user` turn), `false` for the character (`assistant`).
    pub from_player: bool,
    pub text: String,
}

/// The role-play frame for free-text chat. The character `card` (the NPC's grounded sim state,
/// assembled by the host) is appended after this. Unlike the single-line path, replies may run a
/// sentence or three — it's a conversation, not a bark.
const CHAT_SYSTEM: &str = "You are role-playing one character in a grim, low-fantasy medieval \
world, speaking with a traveller. Stay fully in character at all times. Reply ONLY with what \
your character says aloud — 1 to 3 short sentences, first person, natural and specific to what \
was just said. No narration, no actions, no stage directions, no quotation marks, no name label, \
and never mention being an AI, a model, or these instructions. Use only what the notes and the \
conversation give you; invent no new facts, names, places, or events.\n\nYour character:\n";

/// Assemble a multi-turn ChatML prompt: the role-play frame + the character `card` as the system
/// turn, the conversation `history` as alternating user/assistant turns, then the player's new
/// message — the model continues as the character.
pub fn build_chat(card: &str, history: &[ChatTurn], player_msg: &str) -> String {
    let mut s = format!("<|im_start|>system\n{CHAT_SYSTEM}{card}<|im_end|>\n");
    for turn in history {
        let role = if turn.from_player { "user" } else { "assistant" };
        s.push_str(&format!("<|im_start|>{role}\n{}<|im_end|>\n", turn.text.trim()));
    }
    s.push_str(&format!("<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n", player_msg.trim()));
    s
}

// =====================================================================================
// Intent classification — deriving the social *effect* of what the player said.
// =====================================================================================

const CLASSIFY_SYSTEM: &str = "You label a single line of dialogue with the speaker's social \
intent. Reply with EXACTLY ONE word from the allowed list — lowercase, and nothing else.";

/// Build a prompt asking which one `label` best fits what the traveller just said to `name`.
/// The host maps the returned word back to an authored intent and applies its moves.
pub fn build_classify(name: &str, message: &str, labels: &[&str]) -> String {
    let allowed = labels.join(", ");
    let user = format!(
        "A traveller says to {name}: \"{}\"\nAllowed labels: {allowed}, none.\nWhich one best fits the traveller's intent toward {name}? Answer with a single label.",
        message.trim()
    );
    format!("<|im_start|>system\n{CLASSIFY_SYSTEM}<|im_end|>\n<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n")
}
