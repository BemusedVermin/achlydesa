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
use crate::data::Registry;
use crate::features::Features;
use crate::people::{EconRes, Inventory, Market, Npc, Skills, WorldAffordances, price};
use bevy_ecs::prelude::*;

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
