//! Headless look at the party layer: an avatar tries to recruit the people around it — a
//! deterministic Convince/Lead check against each one's disposition — and we print who joined,
//! then confirm the companions travel as a stack at the avatar's side.
//!
//! Run: `cargo run -p agents --example party_demo --release`

use agents::{PartyConfig, Setup, Simulation};

fn main() {
    let mut sim = Simulation::new(Setup {
        width: 48,
        height: 36,
        seed: 7,
        npcs: 80,
        rpg: true,
        party: true,
        // A low bar so even a non-social avatar can gather a small band for the demo. In play
        // the difficulty is the WWN ladder (6/8/10/12…) shifted by the stranger's opinion of
        // you — a charismatic hero, or one who's earned goodwill, recruits where this one can't.
        party_cfg: PartyConfig { recruit_difficulty: 0, ..Default::default() },
        ..Default::default()
    });
    let avatar = sim.spawn_player(None);
    println!(
        "avatar: {} ({}) — Convince {}, Lead {}\n",
        sim.display_name(avatar),
        sim.archetype_of(avatar).unwrap_or("?"),
        sim.proficiency_of(avatar, "Convince").unwrap_or(-1),
        sim.proficiency_of(avatar, "Lead").unwrap_or(-1),
    );

    // Ask the people in reach (fall back to the wider population if few are nearby).
    let nearby: Vec<_> = sim.player_nearby_npcs().into_iter().map(|(e, _)| e).collect();
    let candidates = if nearby.len() >= 8 { nearby } else { sim.npcs() };

    let mut asked = 0;
    for e in candidates.into_iter().take(8) {
        if sim.is_party_member(e) {
            continue;
        }
        asked += 1;
        let name = sim.display_name(e);
        if sim.player_recruit(e) {
            println!("  joined: {name} ({})", sim.archetype_of(e).unwrap_or("?"));
        }
    }

    println!("\nrecruited {} of {asked} asked. Party roster:", sim.party_size());
    for e in sim.party_roster() {
        println!("  {} — {}", sim.display_name(e), sim.archetype_of(e).unwrap_or("?"));
    }

    // They follow as a stack: after a few ticks they all stand on the avatar's tile.
    sim.run(3);
    let here = sim.player_position();
    let together = sim.party_roster().iter().all(|&e| sim.position_of(e) == here);
    println!("\nall companions at the avatar's tile after travelling: {together}");
}
