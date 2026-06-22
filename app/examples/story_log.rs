//! **The director's story, voiced by the LLM** — a compact, reviewable season transcript.
//!
//! Unlike the ambient dialogue log (every soul talking, mostly small-talk that repeats), this
//! pulls **only the beats the narrative director `Γ` staged** — the actual story it is telling —
//! and renders the lines `Γ` *puts in mouths* (its `Effect::Voice` betrayals/threats/accusations)
//! through the on-device **SLM** (the `voice` crate's `candle` model), so they read fresh instead
//! of the deterministic grammar. The simulation itself stays byte-identical; the model only
//! colours the words (see `docs/dialogue.md` §4b).
//!
//!   cargo run -p app --example story_log --release                  # 600 days, seed 11, LLM on
//!   cargo run -p app --example story_log --release -- 900 7         # 900 days, seed 7
//!   cargo run -p app --example story_log --release --features cuda  # GPU (RTX 50xx: CUDA_COMPUTE_CAP=120)
//!   cargo run -p app --example story_log --no-default-features      # compile the SLM out → grammar
//!
//! The first run with the LLM downloads the model (~1 GB) per `assets/config/voice.ron`. The
//! compact transcript is also written to `achlydesa_story.txt`.

use agents::{DialogueConfig, DirectorConfig, Goals, Registry, Setup, Simulation, Utterance};
use std::fmt::Write as _;

/// The wants that let the myth play out: survival, plus `avenge` (a grudge becomes a hunt) and
/// `rule` (the false throne is coveted) — the verbs the director's betrayal/ambition beats lean on.
fn myth_goals(reg: &Registry) -> Goals {
    Goals::from_ron(
        r#"[
            (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
            (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
            (name: "stocked",   condition: Holding(good: Edible, at_least: 12), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
            (name: "solvent",   condition: Money(at_least: 200),      appeal: [(input: Deficit, curve: Linear(m: 0.5, b: 0.0))]),
            (name: "avenge",    condition: Verb(verb: "avenge", target: Foe),
                appeal: [(input: Deficit, curve: Linear(m: 0.55, b: 0.0)), (input: Sanction, curve: Linear(m: -1.0, b: 1.0))]),
            (name: "rule",      condition: Verb(verb: "rule", target: Me),
                appeal: [(input: Trait("ambition"), curve: Linear(m: 0.7, b: 0.0)), (input: Deficit, curve: Linear(m: 1.0, b: 0.0))]),
        ]"#,
        reg,
    )
    .unwrap()
}

fn world(seed: u64) -> Simulation {
    let reg = Registry::bundled();
    let goals = myth_goals(&reg);
    Simulation::new(Setup {
        width: 48,
        height: 36,
        seed,
        warmup: 200,
        npcs: 70,
        markets: 6,
        markets_on_settlements: true,
        throne: true,
        ambitious: 8,
        goals,
        registry: reg,
        director: true,
        director_cfg: DirectorConfig {
            beat_interval: 9,
            ..Default::default()
        },
        // The dialogue layer is on so the director's `Voice` beats actually produce lines for
        // the LLM to re-voice — but we never print the ambient chatter, only Γ's own lines.
        dialogue: true,
        dialogue_cfg: DialogueConfig::default(),
        ..Default::default()
    })
}

/// Re-voice the director's spoken lines (the `Effect::Voice` utterances) through the on-device
/// SLM, keyed by utterance tick. Falls back to the grammar surface for any line the model can't
/// serve. Returns `(map tick→line, a status note for the header)`.
#[cfg(feature = "voice")]
fn voice_the_directors_lines(
    forced: &[Utterance],
) -> (std::collections::HashMap<usize, String>, String) {
    use agents::dialogue::state_hash;
    use std::collections::HashMap;
    use voice::{Voice, VoiceStatus};

    // Indexed by occurrence (not by meaning), so a recurring betrayal gets its own line.
    let mut lines: HashMap<usize, String> = forced
        .iter()
        .enumerate()
        .map(|(i, u)| (i, u.surface.clone()))
        .collect();
    if forced.is_empty() {
        return (lines, "the director voiced no lines this season".into());
    }

    let v = Voice::spawn_from_config();
    // Wait for the model to load (the first run downloads ~1 GB), but don't hang forever.
    let mut waited = 0;
    while v.status() == VoiceStatus::Loading && waited < 1200 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        waited += 1;
    }
    let note = match v.status() {
        VoiceStatus::Ready => {
            "the director's lines re-voiced by the on-device LLM (per-occurrence, so repeats differ)"
        }
        VoiceStatus::Off => "LLM disabled in voice.ron — director's lines on the grammar floor",
        VoiceStatus::Failed(ref e) => {
            return (lines, format!("LLM unavailable ({e}) — grammar floor"));
        }
        VoiceStatus::Loading => "LLM still loading — director's lines on the grammar floor",
    };
    if v.status() != VoiceStatus::Ready {
        return (lines, note.to_string());
    }

    // Dispatch each line keyed by *occurrence* — `state_hash` (the meaning) XORed with a hash of
    // the tick — so the same betrayal twice is two distinct cache keys (and two distinct sampling
    // seeds at temperature 0.7), i.e. two distinct lines. Still deterministic: same run, same ticks.
    for (i, u) in forced.iter().enumerate() {
        let key = state_hash(u) ^ u.tick.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        v.request_keyed(i as u64, u, None, key);
    }
    let mut got = 0;
    let mut tries = 0;
    while got < forced.len() && tries < 2400 {
        for (id, text) in v.poll() {
            lines.insert(id as usize, text);
            got += 1;
        }
        if got < forced.len() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            tries += 1;
        }
    }
    (lines, note.to_string())
}

/// Without the `voice` feature there is no LLM — the director's lines stay on the grammar floor.
#[cfg(not(feature = "voice"))]
fn voice_the_directors_lines(
    forced: &[Utterance],
) -> (std::collections::HashMap<usize, String>, String) {
    let lines = forced
        .iter()
        .enumerate()
        .map(|(i, u)| (i, u.surface.clone()))
        .collect();
    (
        lines,
        "built --no-default-features — the SLM is compiled out, grammar floor".into(),
    )
}

fn main() {
    let mut args = std::env::args().skip(1);
    let days: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(600);
    let seed: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(11);

    let mut sim = world(seed);
    sim.run(days);
    let proto_name = sim.protagonist().map(|p| sim.display_name(p));

    // ONLY the director's lines (Effect::Voice) — never the ambient population chatter.
    let forced: Vec<Utterance> = sim
        .dialogue_log()
        .iter()
        .filter(|u| u.forced)
        .cloned()
        .collect();
    let (voiced, note) = voice_the_directors_lines(&forced);

    let mut out = String::new();
    let _ = writeln!(
        out,
        "ACHLYDESA — the season the director staged (seed {seed}, {days} days)"
    );
    if let Some(name) = &proto_name {
        let _ = writeln!(
            out,
            "  the drama is woven around {name} — and whoever Γ makes you love"
        );
    }
    let _ = writeln!(
        out,
        "  {} beats · {} distinct · {} voiced lines · {} collisions  [{}]\n",
        sim.director_beats_fired(),
        sim.director_distinct_beats(),
        forced.len(),
        sim.director_cadence()
            .iter()
            .filter(|c| c.collision)
            .count(),
        note,
    );

    // The story spine: each beat the director pulled, in order — and beneath any beat that put
    // words in a mouth, the line itself (LLM-voiced). Match a voiced line to the latest beat at
    // or before its tick.
    let beats: Vec<_> = sim.director_cadence().to_vec();
    for (bi, c) in beats.iter().enumerate() {
        let mark = if c.collision {
            "   \u{2190} collision (timed onto a high)"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "  day {:>4}  [{:<11} {:<7}]  {}{}",
            c.tick,
            sim.register_name(c.register),
            format!("{:?}", c.phase),
            c.beat.replace('_', " "),
            mark,
        );
        // Any director-voiced line whose tick falls in this beat's window (up to the next beat).
        let next_tick = beats.get(bi + 1).map(|n| n.tick).unwrap_or(u64::MAX);
        for (fi, u) in forced
            .iter()
            .enumerate()
            .filter(|(_, u)| u.tick >= c.tick && u.tick < next_tick)
        {
            let line = voiced
                .get(&fi)
                .cloned()
                .unwrap_or_else(|| u.surface.clone());
            let _ = writeln!(
                out,
                "            \u{201c}{}\u{201d}  \u{2014} {} to {}",
                line, u.speaker_name, u.listener_name
            );
        }
    }

    print!("{out}");
    let path = "achlydesa_story.txt";
    match std::fs::write(path, &out) {
        Ok(()) => {
            let abs = std::fs::canonicalize(path)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| path.to_string());
            println!("\nWritten to {abs}");
        }
        Err(e) => eprintln!("(could not write {path}: {e})"),
    }
}
