//! The narrative director `Γ` running a **season** — and the **Gödel point** the whole
//! design turns on (`docs/narrative_director.md`, `docs/narrative_director_v2.md`).
//!
//! `Γ` is omnipotent and runs a few **threads** at once, each a *groom → climax → fall*
//! arc around a figure whose **prominence it manufactures on purpose** — it makes the
//! audience love someone, then reverses it. It maximizes *drama* (stakes × attachment ×
//! reversal), times its climaxes onto highs, and lets betrayal dominate *because it scores
//! highest*. Every beat has an in-world alibi; only the **pattern** gives it away — the
//! rhythm, and fortune too well-shaped. *The player should feel manipulated.*
//!
//! And yet there is **no off-switch**. The only way to quiet it is to bring the world to a
//! state it can find no drama in. A *freed* world — provisioned, forgiving, stateless,
//! unthroned — leaves the same fully-armed director surveying and finding nothing worth
//! telling. Its completeness contains a state it cannot author its way out of. That
//! freedom is a property of the world, reached by ordinary life — never a button.
//!
//! `cargo run -p agents --example director_demo --release`

use agents::{DirectorConfig, FactionConfig, Goals, Norms, Registry, Setup, Simulation};
use std::collections::BTreeMap;

fn peaceful_goals(reg: &Registry, with_throne: bool) -> Goals {
    let rule = if with_throne {
        r#",(name: "rule", condition: Verb(verb: "rule", target: Me),
             appeal: [(input: Trait("ambition"), curve: Linear(m: 0.7, b: 0.0)), (input: Deficit, curve: Linear(m: 1.0, b: 0.0))])"#
    } else {
        ""
    };
    Goals::from_ron(
        &format!(
            r#"[
                (name: "sustained", condition: Sustenance(at_least: 70), appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                (name: "rested",    condition: Rest(at_least: 70),        appeal: [(input: Deficit, curve: Power(exp: 2.0))]),
                (name: "stocked",   condition: Holding(good: Edible, at_least: 12), appeal: [(input: Deficit, curve: Linear(m: 0.6, b: 0.0))]),
                (name: "solvent",   condition: Money(at_least: 200),      appeal: [(input: Deficit, curve: Linear(m: 0.5, b: 0.0))]),
                (name: "avenge",    condition: Verb(verb: "avenge", target: Foe),
                    appeal: [(input: Deficit,  curve: Linear(m: 0.55, b: 0.0)),
                             (input: Sanction, curve: Linear(m: -1.0, b: 1.0))]){rule}
            ]"#
        ),
        reg,
    )
    .unwrap()
}

/// A harsh world the director feeds on: a contested throne, ambitious claimants, factions.
fn volatile_world() -> Simulation {
    let reg = Registry::bundled();
    let goals = peaceful_goals(&reg, true);
    Simulation::new(Setup {
        width: 48,
        height: 36,
        seed: 11,
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
            beat_interval: 10,
            ..Default::default()
        },
        ..Default::default()
    })
}

/// A freed world: provisioned, forgiving, stateless, unthroned. The same director.
fn freed_world() -> Simulation {
    let reg = Registry::bundled();
    let goals = peaceful_goals(&reg, false);
    let norms = Norms::from_ron(r#"[(act: "avenge", modality: Forbidden)]"#, &reg).unwrap();
    Simulation::new(Setup {
        width: 44,
        height: 32,
        seed: 11,
        warmup: 200,
        npcs: 40,
        markets: 6,
        markets_on_settlements: false,
        throne: false,
        ambitious: 0,
        initial_food: 30,
        initial_market_stock: 80,
        faction_cfg: FactionConfig {
            period: 0,
            ..Default::default()
        },
        goals,
        norms,
        registry: reg,
        director: true,
        director_cfg: DirectorConfig {
            beat_interval: 10,
            ..Default::default()
        },
        ..Default::default()
    })
}

/// The registers the director told, most-told first — the shape of the season. Betrayal
/// and its kin should top it *emergently*, never by a rule.
fn register_histogram(sim: &Simulation) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for c in sim.director_cadence() {
        *counts
            .entry(sim.register_name(c.register).to_string())
            .or_insert(0) += 1;
    }
    let mut v: Vec<(String, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

fn readout(label: &str, sim: &mut Simulation) {
    let proto = sim.protagonist();
    let proto_prom = proto.map_or(0.0, |p| sim.director_prominence(p));
    let collisions = sim
        .director_cadence()
        .iter()
        .filter(|c| c.collision)
        .count();
    println!(
        "  {label:<10} beats {:>3} ({:>2} distinct)  suffering {:>5.0}  staged {:>5.0}  collisions {:>2}  proto-prominence {:.2}  ambition {:.2}  vengeance {:.2}  alive {}",
        sim.director_beats_fired(),
        sim.director_distinct_beats(),
        sim.gratuitous_total(),
        sim.director_staged_total(),
        collisions,
        proto_prom,
        sim.mean_trait("ambition"),
        sim.mean_trait("vengeance"),
        sim.npc_count(),
    );
}

fn main() {
    let mut volatile = volatile_world();
    volatile.run(700);

    println!(
        "A world the director feeds on — the season it staged (first 28 beats, with its hidden cadence):\n"
    );
    println!(
        "  {:>4}  {:<26} {:<11} {:<7} thread  prom   collision",
        "day", "beat", "register", "phase"
    );
    for c in volatile.director_cadence().iter().take(28) {
        println!(
            "  {:>4}  {:<26} {:<11} {:<7} #{:<4}  {:>4.1}   {}",
            c.tick,
            c.beat.replace('_', " "),
            volatile.register_name(c.register),
            format!("{:?}", c.phase),
            c.thread,
            c.lead_prominence,
            if c.collision {
                "← collision (timed onto a high)"
            } else {
                ""
            },
        );
    }
    println!(
        "\n  The registers it reached for, most-told first (betrayal dominates *emergently*):"
    );
    for (reg, n) in register_histogram(&volatile) {
        println!("    {reg:<12} {n:>3}");
    }
    println!();
    readout("volatile:", &mut volatile);

    // The same omnipotent director, a world it can find no purchase in.
    let mut freed = freed_world();
    freed.run(700);
    println!(
        "\nThe SAME director, loosed on a freed world (provisioned, forgiving, stateless, unthroned):\n"
    );
    readout("freed:", &mut freed);
    println!(
        "\n  It is not disabled and its library is whole — it surveys every {} days and finds\n  little above its impact floor worth telling. The world authors its own, owned life.",
        10
    );
}
