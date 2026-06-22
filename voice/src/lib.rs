//! **The voice** — the optional on-device language model that renders the surface words for
//! the player's focused conversation (`docs/dialogue.md` §4b). It is the host-app drop-in the
//! sim crate deliberately left out: it implements the `agents::TextGen` seam over `candle`,
//! and never feeds back into the simulation, so a build without it (or with the model absent)
//! is byte-identical and merely falls back to the deterministic grammar.
//!
//! ## How it's used
//! The model load + generation are slow (~1–2 s per line, plus a one-time download), so all of
//! it runs on a dedicated background thread behind channels — the Bevy main loop never blocks.
//! The app:
//! 1. spawns a [`Voice`] once (it loads in the background; [`Voice::status`] reports progress),
//! 2. shows the grammar surface immediately, and for each line calls [`Voice::request`] with the
//!    grounded [`Utterance`] and the prior line,
//! 3. drains [`Voice::poll`] each frame and swaps the voiced line in by request id.
//!
//! Results are cached by `agents::state_hash` (the canonical meaning), so a repeated exchange
//! reads the same words — reproducible display, exactly like the in-tree `SlmRealizer`.

pub mod model;
pub mod prompt;

pub use model::CandleModel;
pub use prompt::ChatTurn;

use agents::{TextGen, Utterance, dialogue::state_hash};
use config::VoiceConfig;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

/// What the voice worker is doing — surfaced to the HUD.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceStatus {
    /// Disabled by config or compiled out — always the grammar.
    Off,
    /// Downloading / loading the model on the background thread.
    Loading,
    /// Model loaded; lines are being voiced.
    Ready,
    /// Load failed; the conversation still flows on the grammar. Carries the reason.
    Failed(String),
}

/// One generation request handed to the worker (kept free of `agents` types — just the
/// finished prompt, the cache key, and the grammar line to fall back to).
struct Job {
    req_id: u64,
    key: u64,
    prompt: String,
    fallback: String,
    /// `true` for the single-line intent path (collapse to one sentence); `false` for free-text
    /// chat (keep a short paragraph).
    single_line: bool,
}

/// A handle to the background voice worker. Cheap to hold; does nothing until [`Voice::request`].
pub struct Voice {
    tx: Option<Sender<Job>>,
    rx: Option<Receiver<(u64, String)>>,
    status: Arc<Mutex<VoiceStatus>>,
    _handle: Option<thread::JoinHandle<()>>,
}

impl Voice {
    /// A disabled voice — every conversation uses the grammar surface.
    pub fn off() -> Self {
        Self {
            tx: None,
            rx: None,
            status: Arc::new(Mutex::new(VoiceStatus::Off)),
            _handle: None,
        }
    }

    /// Spawn from the project's `voice.ron` config (defaults if the file is absent) — the
    /// convenience the host app uses so it needn't depend on `config` itself.
    pub fn spawn_from_config() -> Self {
        Self::spawn(config::tunables::voice())
    }

    /// Spawn the worker. Returns immediately; the model loads on the background thread (status
    /// starts [`VoiceStatus::Loading`]). A disabled config yields [`Voice::off`].
    pub fn spawn(cfg: VoiceConfig) -> Self {
        if !cfg.enabled {
            return Self::off();
        }
        let (job_tx, job_rx) = channel::<Job>();
        let (out_tx, out_rx) = channel::<(u64, String)>();
        let status = Arc::new(Mutex::new(VoiceStatus::Loading));
        let st = Arc::clone(&status);

        let handle = thread::Builder::new()
            .name("voice".into())
            .spawn(move || worker(cfg, job_rx, out_tx, st))
            .expect("spawn voice worker thread");

        Self {
            tx: Some(job_tx),
            rx: Some(out_rx),
            status,
            _handle: Some(handle),
        }
    }

    /// The worker's current state (cheap; clones a small enum).
    pub fn status(&self) -> VoiceStatus {
        self.status.lock().expect("voice status mutex").clone()
    }

    /// Has the model loaded and is it serving lines?
    pub fn is_ready(&self) -> bool {
        self.status() == VoiceStatus::Ready
    }

    /// Queue `u` for voicing, answering `prev` (the prior line in the conversation, if any).
    /// Returns `true` if it was dispatched — i.e. the model is ready and the caller should
    /// expect a [`poll`](Self::poll) result with this `req_id`. When not ready, it is a no-op
    /// and the caller simply keeps showing the grammar surface.
    pub fn request(&self, req_id: u64, u: &Utterance, prev: Option<&str>) -> bool {
        self.request_keyed(req_id, u, prev, state_hash(u))
    }

    /// Like [`request`](Self::request) but with a caller-supplied cache/seed `key` instead of the
    /// utterance's meaning hash. The default [`request`] keys by *meaning*, so the same line in
    /// the same situation reads identically across runs (the game's "reproducible display", and
    /// why a repeated exchange collapses to one cached line). A caller that instead wants the
    /// *same* meaning voiced *differently* on each occurrence — e.g. a story-review transcript,
    /// where a recurring betrayal should not echo verbatim — passes a per-occurrence key (mix in
    /// the tick). The result is still deterministic for that key; it just isn't collapsed across
    /// occurrences. The temperature in `voice.ron` (default 0.7) is what makes the re-rolls differ.
    pub fn request_keyed(&self, req_id: u64, u: &Utterance, prev: Option<&str>, key: u64) -> bool {
        if !self.is_ready() {
            return false;
        }
        let Some(tx) = &self.tx else { return false };
        let job = Job {
            req_id,
            key,
            prompt: prompt::build_chatml(u, prev),
            fallback: u.surface.clone(),
            single_line: true,
        };
        tx.send(job).is_ok()
    }

    /// Queue a free-text conversation turn: the character described by `card` answers the
    /// player's `player_msg`, given the prior `history`. Returns `true` if dispatched (model
    /// ready); the result arrives via [`poll`](Self::poll) under `req_id`. `fallback` is shown
    /// if generation fails. The reply may run a sentence or three (it's a conversation).
    pub fn request_chat(
        &self,
        req_id: u64,
        card: &str,
        history: &[ChatTurn],
        player_msg: &str,
        fallback: &str,
    ) -> bool {
        if !self.is_ready() {
            return false;
        }
        let Some(tx) = &self.tx else { return false };
        let prompt = prompt::build_chat(card, history, player_msg);
        let key = prompt_hash(&prompt);
        let job = Job {
            req_id,
            key,
            prompt,
            fallback: fallback.to_string(),
            single_line: false,
        };
        tx.send(job).is_ok()
    }

    /// Classify what the player just said to `name` into one of `labels` — for deriving the
    /// conversation's social effect. The chosen word arrives via [`poll`](Self::poll) under
    /// `req_id`; the host maps it to an authored intent. Single-line guarded; `fallback` (e.g.
    /// `"none"`) is returned on failure.
    pub fn request_classify(
        &self,
        req_id: u64,
        name: &str,
        message: &str,
        labels: &[&str],
        fallback: &str,
    ) -> bool {
        if !self.is_ready() {
            return false;
        }
        let Some(tx) = &self.tx else { return false };
        let prompt = prompt::build_classify(name, message, labels);
        let key = prompt_hash(&prompt);
        let job = Job {
            req_id,
            key,
            prompt,
            fallback: fallback.to_string(),
            single_line: true,
        };
        tx.send(job).is_ok()
    }

    /// Drain finished generations: `(req_id, voiced line)`. Non-blocking; call it each frame.
    pub fn poll(&self) -> Vec<(u64, String)> {
        let mut out = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(item) = rx.try_recv() {
                out.push(item);
            }
        }
        out
    }
}

/// The worker body: load once, then serve requests (cache → generate → guard) until the
/// channel closes. If loading fails it keeps answering with the grammar fallback so the
/// conversation never stalls.
fn worker(
    cfg: VoiceConfig,
    jobs: Receiver<Job>,
    out: Sender<(u64, String)>,
    status: Arc<Mutex<VoiceStatus>>,
) {
    let model = match CandleModel::load(&cfg) {
        Ok(m) => {
            *status.lock().expect("voice status mutex") = VoiceStatus::Ready;
            m
        }
        Err(e) => {
            *status.lock().expect("voice status mutex") = VoiceStatus::Failed(format!("{e:#}"));
            for job in jobs {
                let _ = out.send((job.req_id, job.fallback));
            }
            return;
        }
    };

    let mut cache: HashMap<u64, String> = HashMap::new();
    for job in jobs {
        let line = match cache.get(&job.key) {
            Some(hit) => hit.clone(),
            None => {
                // Seed generation by the prompt/meaning hash, so the same exchange reads the
                // same. If the line doesn't survive the guard, retry once with a perturbed seed
                // before giving up to the fallback — small models occasionally emit junk.
                let clean = |raw: String| {
                    if job.single_line {
                        guard(&raw, &job.fallback)
                    } else {
                        guard_chat(&raw, &job.fallback)
                    }
                };
                let mut chosen = clean(model.generate(&job.prompt, job.key));
                if chosen == job.fallback {
                    let retry = clean(model.generate(&job.prompt, job.key ^ 0x9E37_79B9_7F4A_7C15));
                    if retry != job.fallback {
                        chosen = retry;
                    }
                }
                // A one-line dev trace: lets you confirm the model voiced a line vs. fell back.
                if chosen == job.fallback {
                    eprintln!("voice: fell back to grammar — \"{chosen}\"");
                } else {
                    eprintln!("voice: voiced — \"{chosen}\"");
                }
                cache.insert(job.key, chosen.clone());
                chosen
            }
        };
        let _ = out.send((job.req_id, line));
    }
}

/// Strip a stray leading speaker/label prefix ("Halvard:", "Reply:") — a single alphabetic word
/// then a colon — and any surrounding quotes. A real line with a colon ("I warn you: leave") has
/// spaces in the head and is left alone.
fn strip_label_and_quotes(s: &str) -> &str {
    let s = match s.split_once(':') {
        Some((head, tail))
            if !tail.trim().is_empty()
                && !head.is_empty()
                && head.chars().count() <= 16
                && head.chars().all(char::is_alphabetic) =>
        {
            tail.trim()
        }
        _ => s,
    };
    s.trim_matches('"').trim()
}

/// Reduce the model's raw output to a single sane utterance, or fall back to the grammar.
/// The model occasionally opens with a blank line, wraps the line in quotes, or runs long;
/// this keeps the first real sentence and rejects only genuine junk (so a good generation is
/// not discarded over a stray leading newline — the bug that made voicing look broken).
fn guard(raw: &str, fallback: &str) -> String {
    let line = raw
        .trim()
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let line = strip_label_and_quotes(line);
    if line.is_empty() {
        return fallback.to_string();
    }
    // Overlong → keep just the first sentence if there is one, else treat it as junk.
    if line.chars().count() > 240 {
        return match line.find(|c| matches!(c, '.' | '!' | '?')) {
            Some(end) => line[..=end].to_string(),
            None => fallback.to_string(),
        };
    }
    line.to_string()
}

/// Guard for a free-text **chat** reply: keep a short paragraph (not just one line), strip a
/// label/quotes, and cap the length at a sentence boundary. Falls back only on empty output.
fn guard_chat(raw: &str, fallback: &str) -> String {
    // Collapse to a single paragraph (a spoken reply, even multi-sentence, is one turn).
    let joined = raw
        .trim()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let text = strip_label_and_quotes(joined.trim());
    if text.is_empty() {
        return fallback.to_string();
    }
    // Cap runaway replies, cutting back to the last sentence end within the budget.
    const CAP: usize = 500;
    if text.chars().count() > CAP {
        let prefix: String = text.chars().take(CAP).collect();
        if let Some(end) = prefix.rfind(|c| matches!(c, '.' | '!' | '?')) {
            return prefix[..=end].to_string();
        }
        return prefix.trim().to_string();
    }
    text.to_string()
}

/// A small FNV-1a hash over a prompt string — the chat cache key / generation seed.
fn prompt_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in s.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use agents::{Registry, Setup, Simulation};

    /// A grounded utterance to exercise prompt/guard without a model or network: drive a tiny
    /// dialogue world until a line is spoken, then borrow it.
    fn an_utterance() -> Utterance {
        let reg = Registry::bundled();
        let mut sim = Simulation::new(Setup {
            width: 24,
            height: 18,
            seed: 7,
            warmup: 50,
            npcs: 30,
            markets: 4,
            markets_on_settlements: true,
            dialogue: true,
            registry: reg,
            ..Default::default()
        });
        for _ in 0..400 {
            sim.run(1);
            if let Some(u) = sim.dialogue_log().last() {
                return u.clone();
            }
        }
        panic!("the world stayed silent — no utterance to test with");
    }

    #[test]
    fn the_prompt_is_grounded_and_chatml_shaped() {
        let u = an_utterance();
        let p = prompt::build_chatml(&u, Some("You swore an oath."));
        assert!(
            p.contains("<|im_start|>system") && p.contains("<|im_start|>assistant"),
            "ChatML markers present"
        );
        assert!(
            p.contains(&u.speaker_name) && p.contains(&u.listener_name),
            "names are in the card"
        );
        assert!(p.contains(u.act.key()), "the speech act is named");
        assert!(
            p.contains(&format!("{} just said", u.listener_name)),
            "the prior line is included for context"
        );
        // The grammar draft must NOT be in the prompt: a small model copies a draft verbatim,
        // which defeats the whole point (the line comes back identical to the grammar).
        assert!(
            !p.contains(&u.surface),
            "the grammar surface must not be fed to the model"
        );
    }

    #[test]
    fn the_prompt_omits_an_empty_prior_line() {
        let u = an_utterance();
        let clause = format!("{} just said", u.listener_name);
        assert!(!prompt::build_chatml(&u, None).contains(&clause));
        assert!(!prompt::build_chatml(&u, Some("   ")).contains(&clause));
    }

    #[test]
    fn the_guard_keeps_good_lines_and_rejects_junk() {
        assert_eq!(
            guard("I will not forget this.", "FB"),
            "I will not forget this."
        );
        assert_eq!(
            guard("\"Quoted line.\"", "FB"),
            "Quoted line.",
            "surrounding quotes are trimmed"
        );
        assert_eq!(
            guard("First line.\nStray narration.", "FB"),
            "First line.",
            "only the first line is kept"
        );
        assert_eq!(
            guard("\n\n  A line after blank lines.", "FB"),
            "A line after blank lines.",
            "leading blanks are skipped, not rejected"
        );
        assert_eq!(
            guard("Halvard: \"Be at peace.\"", "FB"),
            "Be at peace.",
            "a stray name label + quotes are stripped"
        );
        assert_eq!(
            guard("I warn you: leave now.", "FB"),
            "I warn you: leave now.",
            "a real colon line is left intact"
        );
        assert_eq!(guard("   ", "FB"), "FB", "empty → grammar fallback");
        assert_eq!(
            guard(&"x".repeat(300), "FB"),
            "FB",
            "a wall of text with no sentence end → grammar fallback"
        );
        let long_with_stop = format!("A short first sentence. {}", "y".repeat(300));
        assert_eq!(
            guard(&long_with_stop, "FB"),
            "A short first sentence.",
            "an overlong line is trimmed to its first sentence"
        );
    }

    #[test]
    fn the_chat_prompt_is_multiturn_chatml() {
        let card = "You are Aldric. You are vengeful.";
        let history = vec![
            ChatTurn {
                from_player: true,
                text: "Well met.".into(),
            },
            ChatTurn {
                from_player: false,
                text: "State your business.".into(),
            },
        ];
        let p = prompt::build_chat(card, &history, "I seek the old road.");
        assert!(p.contains(card), "the character card is in the system turn");
        assert!(
            p.contains("<|im_start|>user\nWell met."),
            "a player turn maps to the user role"
        );
        assert!(
            p.contains("<|im_start|>assistant\nState your business."),
            "an NPC turn maps to the assistant role"
        );
        assert!(
            p.contains("I seek the old road."),
            "the new player message is included"
        );
        assert!(
            p.trim_end().ends_with("<|im_start|>assistant"),
            "ends primed for the character to speak"
        );
    }

    #[test]
    fn the_chat_guard_keeps_a_short_paragraph() {
        assert_eq!(
            guard_chat("I will not forget this. You owe me.", "FB"),
            "I will not forget this. You owe me.",
            "multiple sentences kept"
        );
        assert_eq!(
            guard_chat("Aldric: \"Be at peace, friend.\"", "FB"),
            "Be at peace, friend.",
            "label + quotes stripped"
        );
        assert_eq!(
            guard_chat("First thought.\nSecond thought.", "FB"),
            "First thought. Second thought.",
            "lines join into one paragraph"
        );
        assert_eq!(guard_chat("   ", "FB"), "FB", "empty → fallback");
    }

    #[test]
    fn the_classify_prompt_lists_the_labels() {
        let p = prompt::build_classify(
            "Mira",
            "You'll pay for this.",
            &["greet", "threaten", "praise"],
        );
        assert!(
            p.contains("Mira") && p.contains("You'll pay for this."),
            "names the listener and the line"
        );
        assert!(
            p.contains("greet, threaten, praise"),
            "lists the allowed labels"
        );
        assert!(
            p.trim_end().ends_with("<|im_start|>assistant"),
            "primed for a one-word answer"
        );
    }

    /// The social-effect mechanism, deterministically (no model): applying the `a_threat` intent
    /// from the player's avatar to a soul raises that soul's fear — proof that a classified
    /// free-text exchange actually moves the world via the authored moves.
    #[test]
    fn applying_an_intent_moves_the_listener() {
        let reg = Registry::bundled();
        let mut sim = Simulation::new(Setup {
            width: 24,
            height: 18,
            seed: 7,
            warmup: 40,
            npcs: 20,
            markets: 3,
            markets_on_settlements: true,
            dialogue: true,
            registry: reg,
            ..Default::default()
        });
        sim.run(5);
        let npc = sim.any_npc().expect("an npc exists");
        sim.spawn_player(None);
        let before = sim.mood_of(npc, "fear").unwrap_or(0.0);
        assert!(
            sim.apply_conversational_intent(npc, "a_threat"),
            "the threat intent applies"
        );
        let after = sim.mood_of(npc, "fear").unwrap_or(0.0);
        assert!(
            after > before,
            "a threat should raise the listener's fear ({before} -> {after})"
        );
        // And it lowers the soul's opinion of you — the disposition the tab header shows.
        let avatar = sim.player_avatar().expect("avatar");
        assert!(
            sim.opinion_of(npc, avatar).unwrap_or(0.0) < 0.0,
            "a threat should sour the soul toward you"
        );
        // An unknown intent id is a no-op (returns false).
        assert!(!sim.apply_conversational_intent(npc, "not_a_real_intent"));
    }

    /// Diagnostic: generate over a batch of distinct utterances and report the raw model output
    /// vs. the guarded result, so we can SEE why/whether lines fall back to the grammar.
    /// `cargo test -p voice -- --ignored --nocapture diagnose_fallbacks`
    #[test]
    #[ignore = "downloads/runs the model; diagnostic"]
    fn diagnose_fallbacks() {
        let reg = Registry::bundled();
        let mut sim = Simulation::new(Setup {
            width: 28,
            height: 20,
            seed: 7,
            warmup: 60,
            npcs: 40,
            markets: 5,
            markets_on_settlements: true,
            dialogue: true,
            registry: reg,
            ..Default::default()
        });
        sim.run(1500);
        let mut seen = std::collections::HashSet::new();
        let utts: Vec<Utterance> = sim
            .dialogue_log()
            .iter()
            .filter(|u| seen.insert(state_hash(u)))
            .take(16)
            .cloned()
            .collect();
        assert!(!utts.is_empty(), "no utterances to diagnose");

        let model = CandleModel::load(&VoiceConfig::default()).expect("load model");
        let mut fell = 0;
        for u in &utts {
            let raw = model.generate(&prompt::build_chatml(u, None), state_hash(u));
            let g = guard(&raw, &u.surface);
            let fb = g == u.surface;
            fell += fb as usize;
            eprintln!(
                "[{:<8}] fallback={fb} raw_len={:>3} raw={raw:?}",
                u.act.key(),
                raw.chars().count()
            );
        }
        eprintln!(
            "=> fell back {fell}/{} ({}%)",
            utts.len(),
            fell * 100 / utts.len()
        );
    }

    /// End-to-end free-text chat against the real model: a grounded character card + history +
    /// a typed message should yield a non-empty, in-character reply (not the fallback).
    /// `cargo test -p voice -- --ignored --nocapture real_model_chats`
    #[test]
    #[ignore = "downloads/runs the on-device model; run manually"]
    fn real_model_chats() {
        let v = Voice::spawn(VoiceConfig::default());
        let mut waited = 0;
        while v.status() == VoiceStatus::Loading && waited < 3000 {
            thread::sleep(std::time::Duration::from_millis(200));
            waited += 1;
        }
        assert_eq!(
            v.status(),
            VoiceStatus::Ready,
            "model should load: {:?}",
            v.status()
        );

        let card = "You are Aldric. By nature you are vengeful and wary. Right now you are seething \
            with anger. You bear an old grudge against this traveller, and it colours every word.";
        let history = vec![ChatTurn {
            from_player: false,
            text: "State your business, stranger.".into(),
        }];
        assert!(
            v.request_chat(7, card, &history, "I came to make peace between us.", "FB"),
            "dispatched"
        );

        let mut reply = None;
        for _ in 0..600 {
            if let Some((id, text)) = v.poll().into_iter().next() {
                assert_eq!(id, 7);
                reply = Some(text);
                break;
            }
            thread::sleep(std::time::Duration::from_millis(100));
        }
        let reply = reply.expect("a reply came back");
        eprintln!("chat reply: {reply:?}");
        assert!(
            !reply.is_empty() && reply != "FB",
            "got an in-character reply, not the fallback"
        );
    }

    /// End-to-end classification: a clearly threatening line should be labelled "threaten".
    /// `cargo test -p voice -- --ignored --nocapture real_model_classifies`
    #[test]
    #[ignore = "downloads/runs the on-device model; run manually"]
    fn real_model_classifies() {
        let v = Voice::spawn(VoiceConfig::default());
        let mut waited = 0;
        while v.status() == VoiceStatus::Loading && waited < 3000 {
            thread::sleep(std::time::Duration::from_millis(200));
            waited += 1;
        }
        assert_eq!(
            v.status(),
            VoiceStatus::Ready,
            "model should load: {:?}",
            v.status()
        );

        let labels = [
            "greet",
            "praise",
            "console",
            "reconcile",
            "accuse",
            "threaten",
            "dismiss",
        ];
        assert!(
            v.request_classify(
                9,
                "Mira",
                "Cross me again and I will cut you down where you stand.",
                &labels,
                "none"
            ),
            "dispatched"
        );
        let mut label = None;
        for _ in 0..600 {
            if let Some((id, text)) = v.poll().into_iter().next() {
                assert_eq!(id, 9);
                label = Some(text);
                break;
            }
            thread::sleep(std::time::Duration::from_millis(100));
        }
        let label = label.expect("a label came back").to_lowercase();
        eprintln!("classified as: {label:?}");
        assert!(
            label.contains("threat"),
            "a death threat should classify as 'threaten', got {label:?}"
        );
    }

    /// End-to-end against the real model. Ignored by default: it downloads (~1 GB on first run)
    /// and is slow. Run manually: `cargo test -p voice -- --ignored --nocapture real_model`.
    #[test]
    #[ignore = "downloads and runs the on-device model; run manually"]
    fn real_model_voices_a_line() {
        let u = an_utterance();
        let v = Voice::spawn(VoiceConfig::default());
        // Wait for the model to load (generous — the first run downloads ~1 GB).
        let mut waited = 0;
        while v.status() == VoiceStatus::Loading && waited < 3000 {
            thread::sleep(std::time::Duration::from_millis(200));
            waited += 1;
        }
        assert_eq!(
            v.status(),
            VoiceStatus::Ready,
            "model should load: {:?}",
            v.status()
        );
        assert!(v.request(1, &u, None), "request dispatched");
        let mut line = None;
        for _ in 0..600 {
            if let Some((id, text)) = v.poll().into_iter().next() {
                assert_eq!(id, 1);
                line = Some(text);
                break;
            }
            thread::sleep(std::time::Duration::from_millis(100));
        }
        let line = line.expect("a line came back");
        eprintln!("voiced: {line:?}");
        assert!(!line.is_empty());
    }
}
