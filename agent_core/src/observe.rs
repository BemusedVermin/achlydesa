//! The **observer** and its verification-and-validation harness.
//!
//! An emergent simulation is only as trustworthy as your ability to tell a genuine
//! emergent result from a bug or an accidental artefact of some "non-significant"
//! modelling choice (Galán & Edmonds, *Errors and Artefacts in Agent-Based
//! Modelling*, JASSS 2009). The defence is instrumentation plus invariants: a macro
//! [`Census`] of the world each step, and a [`check`] of the laws that *must* hold
//! between steps. If an invariant ever trips, what looked like emergence was a
//! defect — so the harness runs in the test suite over long simulations.
//!
//! The census doubles as the read-out the design has always wanted (the `Observer`
//! `M` in the `sim` legend): population, wealth, the goods in circulation, the
//! *emergent professions* (who actually practises what), and how hard the world's
//! feature affordances are being worked.

use crate::Substrate;
use crate::beats::Register;
use crate::chronicle::{Chronicle, Episode};
use crate::data::Registry;
use crate::features::Features;
use crate::people::{EconRes, Inventory, Market, Npc, Skills, WorldAffordances, price};
use crate::sift::{self, SiftStatus};
use bevy_ecs::prelude::*;
use std::collections::BTreeMap;

/// A read-only macro snapshot of the simulation at one tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Census {
    pub tick: u64,
    /// Living NPCs.
    pub population: usize,
    /// Total coins across NPCs and markets (conserved by trade; only death removes it).
    pub money: i64,
    /// Total goods held across NPCs and markets.
    pub goods: u64,
    pub markets: usize,
    /// Tile features placed in the world.
    pub features: usize,
    /// Feature affordances (smart-object actions) available.
    pub affordance_sites: usize,
    /// Affordance sites currently worked out (no use left this tick).
    pub worked_out_sites: usize,
    /// Cumulative affordance uses — proof the POIs are *used*, not scenery.
    pub affordance_uses: u64,
    /// Practitioners (proficiency > 0.1) per skill id — the emergent professions.
    pub professions: Vec<usize>,
    /// Market prices found outside the authored `[floor, ceil]` band — must be `0`.
    pub price_band_breaches: usize,
    /// Power blocs that have formed around courts.
    pub factions: usize,
    /// Members in the largest faction — how far consolidation has gone.
    pub largest_faction: usize,
}

impl Census {
    /// Take a snapshot of `world`. Needs `&mut` because ECS queries build state, as
    /// every accessor on `Simulation` does.
    pub fn take(world: &mut World) -> Census {
        let tick = world.resource::<Substrate>().0.tick();
        let skill_count = world.resource::<Registry>().skill_count();
        let features = world.get_resource::<Features>().map_or(0, Features::total);
        let (factions, largest_faction) = world
            .get_resource::<crate::factions::Factions>()
            .map(|f| (f.0.len(), f.0.iter().map(|x| x.members.len()).max().unwrap_or(0)))
            .unwrap_or((0, 0));
        let (affordance_sites, worked_out_sites, affordance_uses) = world
            .get_resource::<WorldAffordances>()
            .map(|wa| {
                (wa.0.len(), wa.0.iter().filter(|s| !s.available()).count(), wa.0.iter().map(|s| s.uses).sum::<u64>())
            })
            .unwrap_or((0, 0, 0));

        let (mut population, mut money, mut goods) = (0usize, 0i64, 0u64);
        {
            let mut q = world.query_filtered::<&Inventory, With<Npc>>();
            for inv in q.iter(world) {
                population += 1;
                money += inv.money;
                goods += inv.stock.iter().map(|&s| s as u64).sum::<u64>();
            }
        }
        // Market price bases (rounded), to price the goods, plus their money/goods.
        let mut markets = 0usize;
        let mut market_bases: Vec<Vec<u32>> = Vec::new();
        {
            let mut q = world.query::<&Market>();
            for m in q.iter(world) {
                markets += 1;
                money += m.money;
                goods += m.stock.iter().map(|&s| s as u64).sum::<u64>();
                market_bases.push(m.price_basis.iter().map(|&x| x.round().max(0.0) as u32).collect());
            }
        }
        let mut professions = vec![0usize; skill_count];
        {
            let mut q = world.query_filtered::<&Skills, With<Npc>>();
            for s in q.iter(world) {
                for (i, &v) in s.0.iter().enumerate() {
                    if v > 0.1 {
                        professions[i] += 1;
                    }
                }
            }
        }
        // Prices must sit inside the authored band; `price` clamps, so this verifies
        // the clamp rather than ever expecting a breach.
        let mut price_band_breaches = 0;
        {
            let reg = world.resource::<Registry>();
            let econ = world.resource::<EconRes>();
            for stock in &market_bases {
                for (g, &basis) in stock.iter().enumerate() {
                    let p = price(reg, econ, g, basis) as f64;
                    let floor = (reg.good(g).base_price as f64 * econ.price_floor_frac as f64).max(1.0);
                    let ceil = reg.good(g).base_price as f64 * econ.price_ceil_frac as f64;
                    if p < floor - 0.5 || p > ceil + 0.5 {
                        price_band_breaches += 1;
                    }
                }
            }
        }

        Census {
            tick,
            population,
            money,
            goods,
            markets,
            features,
            affordance_sites,
            worked_out_sites,
            affordance_uses,
            professions,
            price_band_breaches,
            factions,
            largest_faction,
        }
    }

    /// How many distinct trades are actually being practised — the division of
    /// labour, emergent from skill and the market.
    pub fn trades_in_use(&self) -> usize {
        self.professions.iter().filter(|&&n| n > 0).count()
    }
}

/// An invariant the model must never break. A trip means a bug or an artefact, not
/// emergence — the whole point of the harness is to catch these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    /// Coins appeared from nowhere (trade must only move money; death removes it).
    MoneyCreated { prev: i64, now: i64 },
    /// The population grew with no birth mechanism — entities spawned spuriously.
    PopulationGrew { prev: usize, now: usize },
    /// The cumulative affordance-use counter fell — uses can only accumulate.
    AffordanceUsesFell { prev: u64, now: u64 },
    /// A market priced a good outside its authored `[floor, ceil]` band.
    PriceOutOfBand { count: usize },
}

// --- The narrative eval harness (docs/narrative_sifter.md S7) ---
//
// The field evaluates emergent-narrative systems by their *retellings*, not by moment-to-moment
// text (Kreminski et al., FDG 2019). These read-outs are **dev-only** — never shown to the player
// (a visible interest score converts Narrative into Submission and kills the apex aesthetic). They
// read the [`Sift`](crate::sift::Sift) layer and change no sim state.

/// One story the sifter perceived, assembled for *reading*: its tension label, the spine it leans
/// to, how far it formed, the cast, and the very [`Episode`]s that constitute it. A human seeing
/// only this should be able to say "that's a story" — if not, the interest heuristic is wrong (fix
/// the weights, not the prose).
#[derive(Clone, Debug)]
pub struct RetoldThread {
    pub tension: String,
    pub register: Register,
    pub status: SiftStatus,
    pub cast: Vec<Entity>,
    pub support: Vec<Episode>,
    pub interest: f32,
}

/// The ranked stories a run produced — the retelling dump. Print it and *read* it.
#[derive(Clone, Debug, Default)]
pub struct Retelling {
    pub threads: Vec<RetoldThread>,
}

impl Retelling {
    /// Run the retrospective sifter over the world's Chronicle and assemble the ranked threads with
    /// interest ≥ `min_interest`. Empty (not an error) when the sift layer is off. Dev/eval only —
    /// it reads the world and perturbs nothing.
    pub fn dump(world: &mut World, min_interest: f32) -> Retelling {
        let Some(sift) = sift::run_retrospective(world) else { return Retelling::default() };
        let ring: Vec<Episode> =
            world.get_resource::<Chronicle>().map(|c| c.recent().copied().collect()).unwrap_or_default();
        let by_id: BTreeMap<u64, Episode> = ring.iter().map(|e| (e.id, *e)).collect();
        let threads = sift
            .ranked(min_interest)
            .into_iter()
            .map(|c| RetoldThread {
                tension: c.tension.clone(),
                register: c.register,
                status: c.status,
                cast: c.cast.to_vec(),
                support: c.support.iter().filter_map(|id| by_id.get(id).copied()).collect(),
                interest: c.interest,
            })
            .collect();
        Retelling { threads }
    }

    /// How many stories of each tension were surfaced — the expressive-range read-out (run across
    /// many seeds to flag **monotony** (the same story every time) and **incoherence** (none fire)).
    pub fn tension_histogram(&self) -> BTreeMap<String, usize> {
        let mut h = BTreeMap::new();
        for t in &self.threads {
            *h.entry(t.tension.clone()).or_insert(0) += 1;
        }
        h
    }

    /// A readable, ASCII-only transcript of the run's stories — what you actually *read* to judge
    /// whether the patterns and weights are any good.
    pub fn render(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        if self.threads.is_empty() {
            return "(no stories surfaced)\n".to_string();
        }
        for (i, t) in self.threads.iter().enumerate() {
            let cast: Vec<String> = t.cast.iter().map(|e| format!("#{}", e.index())).collect();
            let _ = writeln!(
                s,
                "{}. [{:?}] {} ({:?}) interest={:.2} cast=[{}]",
                i + 1,
                t.register,
                t.tension,
                t.status,
                t.interest,
                cast.join(", "),
            );
            for ep in &t.support {
                let who: Vec<String> =
                    ep.parties.iter().flatten().map(|e| format!("#{}", e.index())).collect();
                let _ = writeln!(s, "     t{:<5} {:?} [{}]", ep.tick, ep.kind, who.join(", "));
            }
        }
        s
    }
}

/// Check the laws that must hold between two consecutive censuses (and within the
/// latter). An empty result is a clean step; any [`Violation`] is a defect to fix.
pub fn check(prev: &Census, now: &Census) -> Vec<Violation> {
    let mut v = Vec::new();
    if now.money > prev.money {
        v.push(Violation::MoneyCreated { prev: prev.money, now: now.money });
    }
    if now.population > prev.population {
        v.push(Violation::PopulationGrew { prev: prev.population, now: now.population });
    }
    if now.affordance_uses < prev.affordance_uses {
        v.push(Violation::AffordanceUsesFell { prev: prev.affordance_uses, now: now.affordance_uses });
    }
    if now.price_band_breaches > 0 {
        v.push(Violation::PriceOutOfBand { count: now.price_band_breaches });
    }
    v
}
