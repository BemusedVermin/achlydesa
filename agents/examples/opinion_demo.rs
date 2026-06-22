//! Headless look at the **opinion path** to recruiting: the avatar can't order a stranger into
//! its party — it must *win them over* through speech first. We pick a soul, speak kindly to it
//! turn after turn (deterministic, model-free speech acts whose effect scales with the avatar's
//! Charisma + Convince/Lead), watch its opinion climb, and recruit only once it has come round.
//!
//! Run: `cargo run -p agents --example opinion_demo --release`

use agents::{Setup, Simulation};

fn main() {
    let mut sim = Simulation::new(Setup {
        width: 48,
        height: 36,
        seed: 7,
        npcs: 80,
        // The opinion path needs the dialogue layer (the authored intents whose moves shift
        // opinion) alongside the RPG (speech check) and party (recruit) layers.
        dialogue: true,
        rpg: true,
        party: true,
        ..Default::default()
    });
    let avatar = sim.spawn_player(None);
    println!(
        "avatar: {} ({}) — CHA {:+}, Convince {}, Lead {}\n",
        sim.display_name(avatar),
        sim.archetype_of(avatar).unwrap_or("?"),
        sim.abilities_of(avatar).map(|a| a.modifier(5)).unwrap_or(0),
        sim.proficiency_of(avatar, "Convince").unwrap_or(-1),
        sim.proficiency_of(avatar, "Lead").unwrap_or(-1),
    );

    let Some((npc, _)) = sim.player_nearby_npcs().into_iter().next() else {
        println!("no soul nearby — rerun with more npcs");
        return;
    };
    let name = sim.display_name(npc);
    let opinion = |sim: &Simulation| sim.opinion_of(npc, avatar).unwrap_or(0.0);

    // A neutral stranger won't follow on a whim — recruiting is earned, not ordered.
    println!(
        "{name} is a stranger (opinion {:+.2}); recruiting outright succeeds: {}",
        opinion(&sim),
        sim.player_recruit(npc)
    );
    println!("so we win them over, a kind word at a time:");

    let mut joined_on = None;
    for turn in 1..=40 {
        sim.player_talk(npc, "a_word_of_praise");
        if sim.player_recruit(npc) {
            joined_on = Some(turn);
            break;
        }
        if turn % 2 == 0 {
            println!(
                "  after {turn:>2} kind words: {name} is at opinion {:+.2}",
                opinion(&sim)
            );
        }
    }

    match joined_on {
        Some(t) => println!(
            "\n{name} came round and joined after {t} turns (opinion {:+.2}).",
            opinion(&sim)
        ),
        None => println!("\n{name} never came round (opinion {:+.2}).", opinion(&sim)),
    }
    println!("party size: {}", sim.party_size());
}
