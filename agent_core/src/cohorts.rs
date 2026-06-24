//! **Tier 2 — statistical cohorts and the regional economy** (`docs/scaling.md`, Track 2 / 2a+2c).
//!
//! The millions are not entities. Each **region** (a settlement and its market) holds a
//! [`Cohort`]: a population *count by calling*, a coin *pool*, and an aggregate *sustenance*. Its
//! economy runs as **integer flows** — production sells into the regional market, consumption buys
//! food back out, births/deaths grow or shrink the count, and people migrate toward better-fed
//! regions ([`cohort_step`]). Cost is `O(regions · callings)` plus `O(regions²)` migration —
//! **independent of population size**, so a region of thirty souls and one of thirty million cost
//! the same. That is the only thing that reaches millions.
//!
//! Individuals **crystallize** into real ECS entities when the avatar comes near, and **dissolve**
//! back into the count when it leaves ([`cohort_crystallize`]) — the [`Tier 0/1`](crate::Drifter)
//! cast is drawn from, and returned to, the managed mass. Promotion is bounded
//! ([`CohortConfig::crystallize_cap`]), so the live entity count stays small however large the
//! world's stated population.
//!
//! **Determinism & the integer economy.** Every coin flow is an explicit integer transfer between a
//! pool, a market, and entity purses, so `total_money()` (now counting cohort pools) is conserved
//! exactly — **deaths are the only sink**, just as for individuals. Crystallization draws
//! personalities from a dedicated [`CohortRng`] stream (its own xor constant), so it perturbs no
//! other stream. The whole layer is gated on [`CohortConfig`]: absent ⇒ every system early-returns,
//! no region exists ⇒ byte-identical.
//!
//! **Sharding (2c).** The per-region produce/consume/vital step writes only its own region+market,
//! so it is embarrassingly parallel; migration is computed from a start-of-step snapshot and applied
//! as deltas, so it too is order-independent. Kept serial here for simplicity (the step is already
//! `O(regions)`, dwarfed by the crystallized cast's GOAP), but the structure is shard-ready.

use crate::people::{
    EconRes, Inventory, Market, Needs, Npc, Patron, Personality, Plan, Skills, price,
};
use crate::{Position, Registry, Substrate};
use bevy_ecs::prelude::*;
use game_sim::{Coord, SplitMix64};
use sim::Rng;

/// One region's managed mass: a population by calling, the coins it holds, and how well it is fed.
/// Its market entity is shared with any crystallized members and with Tier-0/1 agents that trade
/// there, so the coin pool, the market purse, and entity purses are one conserved system.
#[derive(Clone, Debug)]
pub struct Cohort {
    /// The regional market's tile — the seat distance is measured from for crystallization.
    pub seat: Coord,
    /// The regional market entity (holds the traded goods + money).
    pub market: Entity,
    /// Population by calling (skill id); `pop.len() == skill_count`. The *un-crystallized* remainder.
    pub pop: Vec<u32>,
    /// Coins held by the un-crystallized population (its share of the world's money).
    pub pool: i64,
    /// Aggregate wellbeing, `0` (starving) to `100` (sated).
    pub sustenance: f32,
    /// Whether this region currently has a crystallized cast of real entities near the avatar.
    pub crystallized: bool,
}

impl Cohort {
    /// Total un-crystallized headcount.
    pub fn total(&self) -> u64 {
        self.pop.iter().map(|&n| n as u64).sum()
    }
}

/// Every region's cohort. Present only when the Tier-2 layer is woken.
#[derive(Resource, Default)]
pub struct Regions(pub Vec<Cohort>);

/// Tags a crystallized entity with the region index it belongs to, so it can be folded back into
/// that cohort when the avatar leaves. Carried only by promoted cohort members.
#[derive(Component, Clone, Copy, Debug)]
pub struct CohortMember(pub usize);

/// A dedicated RNG stream for crystallization (reconstructing a promoted member's personality), so
/// the Tier-2 layer perturbs no other subsystem's randomness.
#[derive(Resource)]
pub struct CohortRng(pub SplitMix64);

/// Tunables for the cohort / regional-economy layer. Its presence is the on/off switch.
#[derive(Resource, Clone, Copy, Debug)]
pub struct CohortConfig {
    /// Crystallize a region's cohort when its seat is within this many hexes of the avatar.
    pub promote_radius: i32,
    /// Most entities to spawn from one region on promotion — the live cast stays bounded however
    /// large the cohort.
    pub crystallize_cap: u32,
    /// Skill level a crystallized member is reconstructed with in its calling (its history is lost
    /// to the aggregate — the "pop-in" the design flags; a prototype reconstructs plausibly).
    pub crystallize_skill: f32,
    /// Goods produced per person per tick, by their calling (sold into the regional market).
    pub productivity: f32,
    /// Food units one person consumes per tick (bought from the regional market).
    pub consume_per_capita: f32,
    /// Sustenance gained per tick when fully fed (and the symmetric drain when starving).
    pub feed_rate: f32,
    /// Above this sustenance the population grows; at/below `100 - birth_band` from full it starves.
    pub birth_sustenance: f32,
    /// At/below this sustenance the population shrinks.
    pub death_sustenance: f32,
    /// Fraction of the population added per tick when well-fed.
    pub birth_rate: f32,
    /// Fraction removed per tick when starving (their coins are a sink — deaths only).
    pub death_rate: f32,
    /// Fraction that migrates per tick toward the best-fed reachable region.
    pub migrate_rate: f32,
}

impl Default for CohortConfig {
    fn default() -> Self {
        Self {
            promote_radius: 6,
            crystallize_cap: 24,
            crystallize_skill: 0.5,
            productivity: 1.0,
            consume_per_capita: 0.9,
            feed_rate: 6.0,
            birth_sustenance: 70.0,
            death_sustenance: 20.0,
            birth_rate: 0.01,
            death_rate: 0.02,
            migrate_rate: 0.02,
        }
    }
}

/// Seed one region per market, splitting `population` evenly across the markets and, within each,
/// round-robin across callings so every trade is represented. Each region starts at `pool` coins and
/// a comfortable sustenance. Deterministic (no RNG): the split is arithmetic.
pub fn seed_regions(
    markets: &[(Entity, Coord)],
    skill_count: usize,
    population: u64,
    pool_each: i64,
) -> Regions {
    if markets.is_empty() || skill_count == 0 {
        return Regions::default();
    }
    let per_region = population / markets.len() as u64;
    let cohorts = markets
        .iter()
        .map(|&(market, seat)| {
            let mut pop = vec![0u32; skill_count];
            // Spread this region's people round-robin across callings.
            for c in pop.iter_mut() {
                *c = (per_region / skill_count as u64) as u32;
            }
            // Any remainder lands on the first calling, so the headcount is exact.
            let assigned: u64 = pop.iter().map(|&n| n as u64).sum();
            pop[0] += (per_region - assigned) as u32;
            Cohort {
                seat,
                market,
                pop,
                pool: pool_each,
                sustenance: 80.0,
                crystallized: false,
            }
        })
        .collect();
    Regions(cohorts)
}

/// The good a calling produces (the primary output of its first recipe), and the world's staple food
/// (the most nutritious good). Recomputed each tick — cheap (recipes are few), and keeps no state.
fn economy_maps(reg: &Registry) -> (Vec<Option<usize>>, Option<usize>) {
    let mut output = vec![None; reg.skill_count()];
    for r in reg.recipes() {
        if r.skill < output.len()
            && output[r.skill].is_none()
            && let Some(&(g, _)) = r.outputs.first()
        {
            output[r.skill] = Some(g);
        }
    }
    let food = (0..reg.good_count())
        .filter(|&g| reg.good(g).nutrition > 0.0)
        .max_by(|&a, &b| reg.good(a).nutrition.total_cmp(&reg.good(b).nutrition));
    (output, food)
}

/// **The Tier-2 economy.** Advance every region's cohort one tick as integer flows: production sells
/// into the regional market, consumption buys food back out (setting sustenance), births/deaths grow
/// or shrink the count, then people migrate toward the best-fed region. `O(regions · callings)` +
/// `O(regions²)` migration — independent of how many souls each cohort stands for. No-op when off.
pub(crate) fn cohort_step(
    regions: Option<ResMut<Regions>>,
    mut markets: Query<&mut Market, Without<Npc>>,
    reg: Res<Registry>,
    econ: Res<EconRes>,
    cfg: Option<Res<CohortConfig>>,
) {
    let (Some(mut regions), Some(cfg)) = (regions, cfg) else {
        return;
    };
    let (output, food) = economy_maps(&reg);

    for cohort in &mut regions.0 {
        let total = cohort.total();
        if total == 0 {
            continue;
        }
        let Ok(mut m) = markets.get_mut(cohort.market) else {
            continue;
        };

        // --- Production: each calling sells what it makes into the market (market money -> pool). ---
        for (calling, &n) in cohort.pop.iter().enumerate() {
            if n == 0 {
                continue;
            }
            let Some(good) = output[calling] else {
                continue;
            };
            let made = (n as f32 * cfg.productivity).round() as u32;
            if made == 0 {
                continue;
            }
            m.stock[good] = m.stock[good].saturating_add(made);
            let p = price(
                &reg,
                &econ,
                good,
                m.price_basis[good].round().max(0.0) as u32,
            );
            let revenue = (made as i64).saturating_mul(p);
            let paid = revenue.min(m.money); // the market can only pay what it holds
            if paid > 0 {
                m.money -= paid;
                cohort.pool += paid;
            }
        }

        // --- Consumption: buy food back out (pool -> market money), set how well-fed they are. ---
        let ratio = if let Some(g) = food {
            let need = (total as f32 * cfg.consume_per_capita).round() as i64;
            if need > 0 {
                let p = price(&reg, &econ, g, m.price_basis[g].round().max(0.0) as u32).max(1);
                let affordable = cohort.pool / p;
                let units = need.min(m.stock[g] as i64).min(affordable).max(0);
                if units > 0 {
                    m.stock[g] -= units as u32;
                    let cost = units * p;
                    cohort.pool -= cost;
                    m.money += cost;
                }
                units as f32 / need as f32
            } else {
                1.0
            }
        } else {
            1.0 // a world with no food good: cohorts don't starve on this axis
        };
        // Sustenance eases toward the fed ratio: well-fed climbs, short-fed falls.
        cohort.sustenance =
            (cohort.sustenance + cfg.feed_rate * (2.0 * ratio - 1.0)).clamp(0.0, 100.0);

        // --- Births / deaths: the population tracks how well it is fed (deaths are a money sink). ---
        if cohort.sustenance >= cfg.birth_sustenance {
            let births = (total as f32 * cfg.birth_rate).round() as u32;
            if births > 0 {
                // New mouths spread evenly across callings, bringing no coin (pool unchanged).
                // Computed in O(callings), not a per-mouth loop — a round-robin of `births` over
                // `nc` bins is exactly `births / nc` each plus one to the first `births % nc`, so
                // this is identical to that loop but keeps the step O(regions·callings) at any scale.
                let nc = cohort.pop.len() as u32;
                let base = births / nc;
                let extra = births % nc;
                for (i, c) in cohort.pop.iter_mut().enumerate() {
                    *c += base + u32::from((i as u32) < extra);
                }
            }
        } else if cohort.sustenance <= cfg.death_sustenance {
            let deaths = (total as f32 * cfg.death_rate).round() as u64;
            if deaths > 0 {
                let removed = remove_people(&mut cohort.pop, deaths);
                // The dead take their share of the pool out of the world — the one money sink.
                let sink = (cohort.pool as i128 * removed as i128 / total as i128) as i64;
                cohort.pool -= sink;
            }
        }
    }

    // --- Migration: from a snapshot, each region sends a slice to the best-fed other region. ---
    migrate(&mut regions.0, cfg.migrate_rate);
}

/// Remove `count` people from a population spread across callings, proportionally, returning how
/// many were actually removed (never more than present). Whole people only, so the headcount stays
/// exact and conserved.
fn remove_people(pop: &mut [u32], count: u64) -> u64 {
    let total: u64 = pop.iter().map(|&n| n as u64).sum();
    if total == 0 {
        return 0;
    }
    let count = count.min(total);
    let mut removed = 0u64;
    // First pass: proportional whole-number removal.
    for n in pop.iter_mut() {
        if removed >= count {
            break;
        }
        let take = ((*n as u128 * count as u128) / total as u128) as u64;
        let take = take.min(*n as u64).min(count - removed);
        *n -= take as u32;
        removed += take;
    }
    // Second pass: mop up the rounding remainder from wherever there are people.
    let mut i = 0;
    while removed < count && i < pop.len() * 2 {
        let idx = i % pop.len();
        if pop[idx] > 0 {
            pop[idx] -= 1;
            removed += 1;
        }
        i += 1;
    }
    removed
}

/// Move a `rate` slice of each region's population (and its proportional pool share) toward the
/// best-fed *other* region. Computed from a start-of-tick snapshot and applied as deltas, so it is
/// order-independent and conserves both headcount and coins exactly.
fn migrate(regions: &mut [Cohort], rate: f32) {
    let n = regions.len();
    if n < 2 || rate <= 0.0 {
        return;
    }
    // Snapshot the pull (sustenance) of each region before anyone moves.
    let pull: Vec<f32> = regions.iter().map(|c| c.sustenance).collect();
    // For each source, pick the best-pull destination that beats it.
    let mut moves: Vec<(usize, usize, Vec<u32>, i64)> = Vec::new();
    for (src, cohort) in regions.iter().enumerate() {
        let total = cohort.total();
        if total == 0 {
            continue;
        }
        let Some(dst) = (0..n)
            .filter(|&d| d != src && pull[d] > pull[src])
            .max_by(|&a, &b| pull[a].total_cmp(&pull[b]))
        else {
            continue;
        };
        let movers = (total as f32 * rate) as u64;
        if movers == 0 {
            continue;
        }
        // The slice that leaves = the people `remove_people` would take (before − after).
        let mut after = cohort.pop.clone();
        remove_people(&mut after, movers);
        let leaving: Vec<u32> = cohort
            .pop
            .iter()
            .zip(after.iter())
            .map(|(&before, &remaining)| before - remaining)
            .collect();
        let moved_count: u64 = leaving.iter().map(|&x| x as u64).sum();
        if moved_count == 0 {
            continue;
        }
        let moved_pool = (cohort.pool as i128 * moved_count as i128 / total as i128) as i64;
        moves.push((src, dst, leaving, moved_pool));
    }
    // Apply: subtract from sources, add to destinations. Headcount and coins are conserved.
    for (src, dst, slice, moved_pool) in moves {
        for (i, &m) in slice.iter().enumerate() {
            regions[src].pop[i] -= m;
            regions[dst].pop[i] += m;
        }
        regions[src].pool -= moved_pool;
        regions[dst].pool += moved_pool;
    }
}

/// **Crystallize / dissolve.** Promote a bounded cast of real entities from each region the avatar
/// has come near, and fold the cast back into the count when it leaves. Promotion moves coins from
/// the pool into entity purses (conserved); dissolution moves the survivors' purses back and returns
/// their goods to the market (a death while crystallized is the normal entity sink). No-op when off.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn cohort_crystallize(
    mut commands: Commands,
    regions: Option<ResMut<Regions>>,
    cfg: Option<Res<CohortConfig>>,
    crng: Option<ResMut<CohortRng>>,
    player: Res<crate::player::PlayerState>,
    substrate: Res<Substrate>,
    reg: Res<Registry>,
    positions: Query<&Position>,
    members: Query<(Entity, &CohortMember, &Skills, &Inventory, &Needs)>,
    mut markets: Query<&mut Market, Without<Npc>>,
) {
    let (Some(mut regions), Some(cfg), Some(mut crng)) = (regions, cfg, crng) else {
        return;
    };
    let Some(avatar) = player.avatar() else {
        return;
    };
    let Ok(&Position(ac)) = positions.get(avatar) else {
        return;
    };
    let width = substrate.0.topology().width();

    // Gather the live cast per region, so a dissolving region can fold its survivors back.
    let mut cast: Vec<Vec<(Entity, usize, i64, Vec<u32>, f32)>> = vec![Vec::new(); regions.0.len()];
    for (e, m, skills, inv, needs) in &members {
        if let Some(slot) = cast.get_mut(m.0) {
            let calling = primary_calling(&skills.0);
            slot.push((e, calling, inv.money, inv.stock.clone(), needs.sustenance));
        }
    }

    for (ri, cohort) in regions.0.iter_mut().enumerate() {
        let near = within(ac, cohort.seat, cfg.promote_radius, width);
        match (near, cohort.crystallized) {
            // Promote: pull a bounded cast out of the count into real entities.
            (true, false) => {
                let total = cohort.total();
                if total == 0 {
                    cohort.crystallized = true;
                    continue;
                }
                let k = (cfg.crystallize_cap as u64).min(total);
                // Which callings the k come from — drawn proportionally from the count.
                let take = pick_callings(&cohort.pop, k);
                let moved_pool = (cohort.pool as i128 * k as i128 / total as i128) as i64;
                let mut purse_left = moved_pool;
                let mut spawned = 0u64;
                let skill_count = cohort.pop.len();
                for (calling, cnt) in take.iter().enumerate() {
                    for _ in 0..*cnt {
                        cohort.pop[calling] -= 1;
                        // Even split of the moved pool, remainder to the earliest spawned.
                        let remaining = k - spawned;
                        let share = purse_left / remaining as i64;
                        purse_left -= share;
                        spawned += 1;
                        let mut skills = vec![0.0f32; skill_count];
                        skills[calling] = cfg.crystallize_skill;
                        commands.spawn((
                            Npc,
                            Position(cohort.seat),
                            Needs {
                                sustenance: cohort.sustenance,
                                rest: 100.0,
                            },
                            Skills(skills),
                            Inventory {
                                money: share,
                                stock: vec![0u32; reg.good_count()],
                            },
                            Plan::default(),
                            Patron(cohort.market),
                            Personality(roll_personality(&reg, &mut crng.0)),
                            crate::people::Mood(vec![0.0; reg.mood_count()]),
                            crate::people::Known::default(),
                            crate::factions::Allegiance::default(),
                            crate::factions::Opinion::default(),
                            CohortMember(ri),
                        ));
                    }
                }
                cohort.pool -= moved_pool;
                cohort.crystallized = true;
            }
            // Dissolve: fold the survivors back into the count and despawn them.
            (false, true) => {
                // Fold in entity-id order, not ECS-iteration (archetype) order: the f32
                // `sustenance_sum` below is order-sensitive (float addition isn't associative), so a
                // future archetype-layout change would otherwise silently shift the fingerprint
                // without changing the sim. Entity id is the same deterministic key used elsewhere.
                cast[ri].sort_unstable_by_key(|t| t.0.to_bits());
                // The un-crystallized remainder kept evolving its own sustenance while the cast was
                // away; capture its headcount before folding the cast back, so we *blend* rather than
                // overwrite — a small cast must not clobber the whole region's evolved wellbeing.
                let remainder = cohort.total();
                let mut sustenance_sum = 0.0;
                let mut folded = 0u64;
                for &(e, calling, money, ref stock, sustenance) in &cast[ri] {
                    if calling < cohort.pop.len() {
                        cohort.pop[calling] += 1;
                    }
                    cohort.pool += money;
                    // Return the member's goods to the regional market (goods are conserved).
                    if let Ok(mut m) = markets.get_mut(cohort.market) {
                        for (g, &q) in stock.iter().enumerate() {
                            m.stock[g] = m.stock[g].saturating_add(q);
                        }
                    }
                    sustenance_sum += sustenance;
                    folded += 1;
                    commands.entity(e).despawn();
                }
                // Headcount-weighted blend: the remainder keeps its evolved sustenance, the returning
                // cast contributes weighted by its size. When the whole cohort was crystallized
                // (remainder == 0) this collapses to the cast average; when nobody returns, unchanged.
                let total_after = remainder + folded;
                if total_after > 0 {
                    cohort.sustenance = (cohort.sustenance * remainder as f32 + sustenance_sum)
                        / total_after as f32;
                }
                cohort.crystallized = false;
            }
            _ => {}
        }
    }
}

/// The calling an entity most embodies — the skill it is most proficient in (its primary trade).
fn primary_calling(skills: &[f32]) -> usize {
    skills
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Choose `k` people from a population by calling, proportionally — the callings a crystallized cast
/// is drawn from. Returns a per-calling count summing to `min(k, total)`.
fn pick_callings(pop: &[u32], k: u64) -> Vec<u32> {
    let mut tmp = pop.to_vec();
    let total: u64 = pop.iter().map(|&n| n as u64).sum();
    let want = k.min(total);
    let removed = remove_people(&mut tmp, want);
    // `remove_people` always reaches its target when `want <= total` (its mop-up pass covers the
    // rounding remainder), so a shortfall would mean fewer entities crystallize than intended with
    // no other signal — catch it in tests at zero release cost.
    debug_assert_eq!(removed, want, "pick_callings: removal shortfall");
    // The picked slice is what `remove_people` took: before - after.
    pop.iter().zip(tmp.iter()).map(|(&b, &a)| b - a).collect()
}

/// Roll a fresh personality near each trait's baseline (varied by its spread) — the same shape
/// `spawn_npcs` uses, drawn from the cohort's own RNG so promotion perturbs no other stream.
fn roll_personality(reg: &Registry, rng: &mut SplitMix64) -> Vec<f32> {
    (0..reg.trait_count())
        .map(|t| {
            let d = reg.trait_def(t);
            let jitter = rng.gen_range(2001) as f32 / 1000.0 - 1.0; // [-1, 1]
            (d.baseline + jitter * d.spread).clamp(0.0, 1.0)
        })
        .collect()
}

/// Wrapped Chebyshev "within `r` hexes" — the same cheap box the LOD uses (the world wraps E–W).
fn within(a: Coord, b: Coord, r: i32, width: i32) -> bool {
    let drow = (a.row - b.row).abs();
    let dcol = {
        let d = (a.col - b.col).abs();
        d.min(width - d)
    };
    drow <= r && dcol <= r
}
