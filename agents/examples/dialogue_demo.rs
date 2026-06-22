//! **Emergent dialogue** — the NPC's inner life, spoken (`docs/dialogue.md`).
//!
//! Speaking is acting: each co-located soul says the thing it most *wants* to say to the
//! other, the wanting scored by the same IAUS utility that ranks its goals — from its
//! traits, its mood, its opinion of the listener, and the grudges between them. The words
//! are *composed* from a generative grammar (never a phrasebook), coloured by who is
//! speaking and how they feel. Wake the narrative director too and its manufactured
//! betrayals are finally *heard*: a grudge it engineers becomes an accusation spoken aloud.
//!
//! `cargo run -p agents --example dialogue_demo --release`

use agents::{
    DialogueConfig, DirectorConfig, Goals, Registry, Setup, Simulation, TextGen, Utterance,
};

fn throne_goals(reg: &Registry) -> Goals {
    Goals::from_ron(
        r#"[
            (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
            (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
            (name: "stocked",   condition: Holding(good: Edible, at_least: 12), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
            (name: "solvent",   condition: Money(at_least: 200),      appeal: [(input: Deficit, curve: Linear(m: 0.5, b: 0.0))]),
            (name: "avenge",    condition: Verb(verb: "avenge", target: Foe),
                appeal: [(input: Deficit, curve: Linear(m: 0.55, b: 0.0)), (input: Sanction, curve: Linear(m: -1.0, b: 1.0))]),
            (name: "rule",      condition: Verb(verb: "rule", target: Me),
                appeal: [(input: Trait("ambition"), curve: Linear(m: 0.7, b: 0.0)), (input: Deficit, curve: Linear(m: 1.0, b: 0.0))])
        ]"#,
        reg,
    )
    .unwrap()
}

/// A society seated in its settlements (so souls cluster and talk), with a director
/// stirring the drama the talk then voices.
fn world() -> Simulation {
    let reg = Registry::bundled();
    let goals = throne_goals(&reg);
    Simulation::new(Setup {
        width: 44,
        height: 32,
        seed: 11,
        warmup: 200,
        npcs: 60,
        markets: 6,
        markets_on_settlements: true,
        throne: true,
        ambitious: 6,
        goals,
        registry: reg,
        director: true,
        director_cfg: DirectorConfig {
            beat_interval: 9,
            ..Default::default()
        },
        dialogue: true,
        dialogue_cfg: DialogueConfig::default(),
        ..Default::default()
    })
}

/// A stand-in for a host-supplied on-device SLM (the real one is a `candle`/`llama.cpp`
/// adapter). It only proves the seam: the simulation hands it the grounded card + draft;
/// here we just flag the swap. A real model would *rephrase in the character's voice*.
struct DemoModel;
impl TextGen for DemoModel {
    fn generate(&self, _prompt: &str, _seed: u64) -> String {
        // A real SLM returns an emergent line; the demo returns empty so the realizer
        // shows its grammar fallback (so the demo stays deterministic and model-free).
        String::new()
    }
}

fn line(u: &Utterance) -> String {
    let voice = if u.forced {
        "  (the director's hand)"
    } else {
        ""
    };
    format!(
        "  day {:>4}  {:>8} → {:<8}  [{:?}]  {}{}",
        u.tick, u.speaker_name, u.listener_name, u.act, u.surface, voice
    )
}

fn main() {
    let mut sim = world();
    sim.run(500);

    let log = sim.dialogue_log();
    println!(
        "The world spoke {} lines. A scene from the middle of it:\n",
        log.len()
    );
    for u in log.iter().skip(log.len().saturating_sub(150)).take(26) {
        println!("{}", line(u));
    }

    // Show the words are grounded: the accusations carry real motive and standing.
    if let Some(u) = log.iter().rev().find(|u| {
        matches!(
            u.act,
            agents::SpeechAct::Accuse | agents::SpeechAct::Threaten
        )
    }) {
        println!(
            "\n  A grounded line — {} ({}) {} {}:\n    \"{}\"",
            u.speaker_name,
            if u.motive.is_empty() {
                "plain"
            } else {
                &u.motive[0]
            },
            u.relation_word,
            u.listener_name,
            u.surface,
        );
    }

    // The player avatar speaks. This is a role-playing game: the *player* is the avatar's
    // mind. The avatar carries no traits or mood, and the sim does not score what it "wants"
    // to say — it is offered the whole repertoire and chooses. The sim renders the words and
    // visits the consequence on the soul addressed; the chosen NPC answers in kind.
    sim.spawn_player(None); // the avatar lands where the people are — a soul stands near
    if let Some((listener, _)) = sim.player_nearby_npcs().first().copied() {
        let lname = sim.display_name(listener);
        let menu = sim.player_intents();
        println!(
            "\n  A soul, {lname}, stands within reach. The player may say any of {} things — a few:",
            menu.len()
        );
        for id in menu.iter().take(5) {
            println!("    {id}");
        }
        // The player *chooses* — here, to reach out warmly (not what any mood would dictate).
        if let Some((said, reply)) = sim.player_talk(listener, "a_greeting") {
            println!("  → the player chooses to greet: \"{}\"", said.surface);
            match reply {
                Some(r) => println!("  → {lname} answers: \"{}\"", r.surface),
                None => println!("  → {lname} has nothing to say in return."),
            }
        }
    }

    // The SLM realizer seam (out of band): swap the surface generator for the foreground.
    let mut realizer = agents::SlmRealizer::new(DemoModel);
    if let Some(u) = sim.dialogue_log().last() {
        println!(
            "\n  The same line through the optional SLM realizer (grammar fallback shown):\n    \"{}\"",
            realizer.realize(u)
        );
    }
}
