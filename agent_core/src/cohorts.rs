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
    Bond, EconRes, Inventory, Liege, Market, Needs, Npc, Patron, Personality, Plan, Skills, price,
};
use crate::scalar::Fx;
use crate::{Position, Registry, Substrate};
use bevy_ecs::prelude::*;
use game_sim::{Coord, SplitMix64, World as GameWorld};
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
    /// The region's **carrying capacity** — the population its land (fertility) can sustain. Fixed at
    /// seeding. The fixed resource that makes the population *converge* here instead of running away:
    /// below it the land runs a surplus (wellbeing rises → births), above it a deficit (→ deaths).
    pub capacity: u32,
    /// Coins held by the un-crystallized population (its share of the world's money).
    pub pool: i64,
    /// Aggregate wellbeing, `0` (starving) to `100` (sated).
    pub sustenance: Fx,
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
    /// Mean skill level a crystallized member is reconstructed with in its calling (its history is
    /// lost to the aggregate — the "pop-in" the design flags; a prototype reconstructs plausibly).
    pub crystallize_skill: Fx,
    /// Spread of crystallized members' calling skill around [`Self::crystallize_skill`] (`±spread`,
    /// clamped to the skill's cap), so a promoted cast has novices and veterans rather than clones.
    pub crystallize_skill_spread: Fx,
    /// Units of the staple food a crystallized member is provisioned with — drawn from the regional
    /// market (so goods are conserved), scaled by the region's wellbeing. A member no longer pops in
    /// empty-handed.
    pub crystallize_larder: u32,
    /// Fraction of a crystallized cast that arrives with a **bond** to another member — the existing
    /// friendships of a settled community (which the director can later strain). `0` disables it and
    /// draws no RNG (so disabling stays byte-identical, not merely silent).
    pub crystallize_bond_frac: Fx,
    /// Fraction of a crystallized cast that arrives as a **vassal** of another member — the local
    /// hierarchy (so a lord's death sends a grudge down the chain). `0` disables it and draws no RNG.
    pub crystallize_vassal_frac: Fx,
    /// Goods produced per person per tick, by their calling (sold into the regional market).
    pub productivity: Fx,
    /// Food units one person consumes per tick (bought from the regional market).
    pub consume_per_capita: Fx,
    /// Sustenance gained per tick when fully fed (and the symmetric drain when starving).
    pub feed_rate: Fx,
    /// Above this sustenance the population grows; at/below `100 - birth_band` from full it starves.
    pub birth_sustenance: Fx,
    /// At/below this sustenance the population shrinks.
    pub death_sustenance: Fx,
    /// Fraction of the population added per tick when well-fed.
    pub birth_rate: Fx,
    /// Fraction removed per tick when starving (their coins are a sink — deaths only).
    pub death_rate: Fx,
    /// Fraction that migrates per tick toward the best-fed reachable region.
    pub migrate_rate: Fx,
}

impl Default for CohortConfig {
    fn default() -> Self {
        Self {
            promote_radius: 6,
            crystallize_cap: 24,
            crystallize_skill: Fx::from_num(0.5),
            crystallize_skill_spread: Fx::from_num(0.3),
            crystallize_larder: 4,
            crystallize_bond_frac: Fx::from_num(0.2),
            crystallize_vassal_frac: Fx::from_num(0.15),
            productivity: Fx::from_num(1.0),
            consume_per_capita: Fx::from_num(0.9),
            feed_rate: Fx::from_num(6.0),
            birth_sustenance: Fx::from_num(70.0),
            death_sustenance: Fx::from_num(20.0),
            birth_rate: Fx::from_num(0.01),
            death_rate: Fx::from_num(0.02),
            migrate_rate: Fx::from_num(0.02),
        }
    }
}

/// Seed one region per market. Each region's **carrying capacity** is `population` split by *land
/// fertility* (so fertile regions support more — and the population then settles there via
/// migration); it is seeded *at* that capacity, spread round-robin across callings, with a neutral
/// sustenance in the birth/death dead-band so it starts in its stable state rather than booming or
/// crashing into it. Falls back to an even split on uniformly barren land. Deterministic (no RNG).
pub fn seed_regions(
    markets: &[(Entity, Coord)],
    skill_count: usize,
    population: u64,
    pool_each: i64,
    substrate: &GameWorld,
) -> Regions {
    if markets.is_empty() || skill_count == 0 {
        return Regions::default();
    }
    // Fertility comes from the substrate's f32 climate field; convert it to fixed at the boundary.
    let fert: Vec<Fx> = markets
        .iter()
        // `saturating_from_num` because `from_num` panics on overflow and `.max(0.0)` doesn't filter
        // a `+inf` the terrain generator could produce on an unusual config.
        .map(|&(_, seat)| Fx::saturating_from_num(substrate.carrying_capacity(seat).max(0.0)))
        .collect();
    let total_fert: Fx = fert.iter().copied().fold(Fx::ZERO, |a, b| a + b);
    let cohorts = markets
        .iter()
        .enumerate()
        .map(|(r, &(market, seat))| {
            let capacity = if total_fert > Fx::ZERO {
                // capacity = (fert[r] / total_fert) · population, in fixed point. The fraction (≤ 1)
                // is taken first so the product can't overflow the scalar's integer range.
                ((fert[r] / total_fert) * Fx::saturating_from_num(population))
                    .round()
                    .to_num::<i64>() as u32
            } else {
                (population / markets.len() as u64) as u32
            };
            // Seed at capacity, spread evenly across callings (exact headcount: `cap / nc` each plus
            // one to the first `cap % nc`).
            let mut pop = vec![0u32; skill_count];
            let nc = skill_count as u32;
            let base = capacity / nc;
            let extra = capacity % nc;
            for (i, c) in pop.iter_mut().enumerate() {
                *c = base + u32::from((i as u32) < extra);
            }
            Cohort {
                seat,
                market,
                pop,
                capacity,
                pool: pool_each,
                sustenance: Fx::from_num(50.0), // neutral — in the dead-band, so a fed cohort holds
                crystallized: false,
            }
        })
        .collect();
    Regions(cohorts)
}

/// Per-calling output goods and the staple food good — derived **once** from the (immutable after
/// setup) registry, so `cohort_step` doesn't re-derive (and re-allocate) them every tick. Built by
/// the assembler alongside the other cohort resources; present only when the layer is woken.
#[derive(Resource)]
pub struct EconomyMaps {
    /// `output[calling]` = the primary output good of that calling's first recipe (or `None`).
    pub output: Vec<Option<usize>>,
    /// The world's staple food good — the most nutritious (or `None` if the economy has no food).
    pub food: Option<usize>,
}

impl EconomyMaps {
    /// Derive the maps from the registry.
    pub fn build(reg: &Registry) -> Self {
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
        Self { output, food }
    }
}

/// Integer `round(count · rate)` in fixed point — exact for any population (no `f32` precision loss
/// above 2^24). `rate` is a small fixed-point factor (a per-capita rate), so `count · rate` stays
/// within the scalar's range for the populations the cohort layer carries.
fn scale(count: u64, rate: Fx) -> u64 {
    (Fx::saturating_from_num(count) * rate)
        .round()
        .to_num::<i64>()
        .max(0) as u64
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
    maps: Option<Res<EconomyMaps>>,
    cfg: Option<Res<CohortConfig>>,
) {
    let (Some(mut regions), Some(maps), Some(cfg)) = (regions, maps, cfg) else {
        return;
    };
    let (output, food) = (&maps.output, maps.food);

    for cohort in &mut regions.0 {
        let total = cohort.total();
        if total == 0 {
            continue;
        }
        let Ok(mut m) = markets.get_mut(cohort.market) else {
            continue;
        };

        // --- Trade-goods production: each calling sells its (non-food) good into the market (market
        // money -> pool). Food is *not* made per-head — it comes from the land below, so the
        // population can't bootstrap unlimited food and run away; that is what bounds it. ---
        for (calling, &n) in cohort.pop.iter().enumerate() {
            if n == 0 {
                continue;
            }
            let Some(good) = output[calling] else {
                continue;
            };
            if reg.good(good).nutrition > 0.0 {
                continue; // food is land-limited, handled below
            }
            let made = scale(n as u64, cfg.productivity).min(u32::MAX as u64) as u32;
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

        // --- Food from the land: the region yields enough to feed its *carrying capacity*, no more.
        // Sold into the market (so a crystallized cast can buy it, and money flows), then eaten by the
        // population below. The fixed cap is the anchor the population converges to. ---
        if let Some(g) = food {
            let made =
                scale(cohort.capacity as u64, cfg.consume_per_capita).min(u32::MAX as u64) as u32;
            if made > 0 {
                m.stock[g] = m.stock[g].saturating_add(made);
                let p = price(&reg, &econ, g, m.price_basis[g].round().max(0.0) as u32);
                let revenue = (made as i64).saturating_mul(p);
                let paid = revenue.min(m.money);
                if paid > 0 {
                    m.money -= paid;
                    cohort.pool += paid;
                }
            }
            // Consumption: the population eats from the market's food stock (pool -> market money).
            let need = scale(total, cfg.consume_per_capita) as i64;
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
            }
        }

        // --- Wellbeing tracks land pressure: supply (the land's capacity) over demand (headcount).
        // Below capacity → surplus → wellbeing climbs (births); above → deficit → it falls (deaths);
        // at capacity → neutral, so the population settles there. A bounded step damps the swing. ---
        let ratio = Fx::saturating_from_num(cohort.capacity) / Fx::saturating_from_num(total);
        let nudge = (ratio - Fx::ONE).clamp(-Fx::ONE, Fx::ONE);
        cohort.sustenance =
            (cohort.sustenance + cfg.feed_rate * nudge).clamp(Fx::ZERO, Fx::from_num(100));

        // --- Births / deaths: the population tracks how well it is fed (deaths are a money sink). ---
        if cohort.sustenance >= cfg.birth_sustenance {
            let births = scale(total, cfg.birth_rate).min(u32::MAX as u64) as u32;
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
            let deaths = scale(total, cfg.death_rate);
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
fn migrate(regions: &mut [Cohort], rate: Fx) {
    let n = regions.len();
    if n < 2 || rate <= Fx::ZERO {
        return;
    }
    // Snapshot the pull (sustenance) of each region before anyone moves.
    let pull: Vec<Fx> = regions.iter().map(|c| c.sustenance).collect();
    // For each source, pick the best-pull destination that beats it.
    let mut moves: Vec<(usize, usize, Vec<u32>, i64)> = Vec::new();
    for (src, cohort) in regions.iter().enumerate() {
        let total = cohort.total();
        if total == 0 {
            continue;
        }
        let Some(dst) = (0..n)
            .filter(|&d| d != src && pull[d] > pull[src])
            .max_by(|&a, &b| pull[a].cmp(&pull[b]))
        else {
            continue;
        };
        let movers = scale(total, rate);
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
    maps: Option<Res<EconomyMaps>>,
    player: Res<crate::player::PlayerState>,
    substrate: Res<Substrate>,
    reg: Res<Registry>,
    positions: Query<&Position>,
    members: Query<(Entity, &CohortMember, &Skills, &Inventory, &Needs)>,
    mut markets: Query<&mut Market, Without<Npc>>,
) {
    let (Some(mut regions), Some(cfg), Some(mut crng), Some(maps)) = (regions, cfg, crng, maps)
    else {
        return;
    };
    let Some(avatar) = player.avatar() else {
        return;
    };
    let Ok(&Position(ac)) = positions.get(avatar) else {
        return;
    };
    let width = substrate.0.topology().width();

    // Gather the live cast per region, so a dissolving region can fold its survivors back. The
    // member's f32 `Needs.sustenance` is converted to the fixed-point scalar at this boundary.
    let mut cast: Vec<Vec<(Entity, usize, i64, Vec<u32>, Fx)>> = vec![Vec::new(); regions.0.len()];
    for (e, m, skills, inv, needs) in &members {
        if let Some(slot) = cast.get_mut(m.0) {
            let calling = primary_calling(&skills.0);
            slot.push((
                e,
                calling,
                inv.money,
                inv.stock.clone(),
                Fx::saturating_from_num(needs.sustenance),
            ));
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
                let mut cast_ids: Vec<Entity> = Vec::with_capacity(k as usize);
                let skill_count = cohort.pop.len();
                // Each member is provisioned from the regional market (so goods are conserved, not
                // minted), scaled by how well-fed the region is. Integer math, no new float.
                let mut market = markets.get_mut(cohort.market).ok();
                let sust = cohort.sustenance.round().to_num::<i64>().clamp(0, 100) as u64;
                let larder = (cfg.crystallize_larder as u64 * sust / 100) as u32;
                for (calling, cnt) in take.iter().enumerate() {
                    let cap = reg.skill(calling).cap;
                    for _ in 0..*cnt {
                        cohort.pop[calling] -= 1;
                        // Even split of the moved pool, remainder to the earliest spawned.
                        let remaining = k - spawned;
                        let share = purse_left / remaining as i64;
                        purse_left -= share;
                        spawned += 1;
                        // Varied proficiency: a cast has novices and veterans, not clones — jittered
                        // from the cohort's own RNG, so it stays deterministic and perturbs no other
                        // stream. `Skills` is still f32 in the wider agent layer, so the fixed-point
                        // config is converted to f32 here at the boundary.
                        let jitter = crng.0.gen_range(2001) as f32 / 1000.0 - 1.0;
                        let mean = cfg.crystallize_skill.to_num::<f32>();
                        let spread = cfg.crystallize_skill_spread.to_num::<f32>();
                        let mut skills = vec![0.0f32; skill_count];
                        skills[calling] = (mean + jitter * spread).clamp(0.0, cap);
                        // Draw the member's larder of the staple food from the market, capped by what
                        // it holds — so a promoted soul arrives provisioned, and goods are conserved.
                        let mut stock = vec![0u32; reg.good_count()];
                        if let (Some(g), Some(m)) = (maps.food, market.as_deref_mut()) {
                            let got = larder.min(m.stock[g]);
                            m.stock[g] -= got;
                            stock[g] = got;
                        }
                        let id = commands
                            .spawn((
                                Npc,
                                Position(cohort.seat),
                                Needs {
                                    sustenance: cohort.sustenance.to_num::<f32>(),
                                    rest: 100.0,
                                },
                                Skills(skills),
                                Inventory {
                                    money: share,
                                    stock,
                                },
                                Plan::default(),
                                Patron(cohort.market),
                                Personality(roll_personality(&reg, &mut crng.0)),
                                crate::people::Mood(vec![0.0; reg.mood_count()]),
                                crate::people::Known::default(),
                                crate::factions::Allegiance::default(),
                                crate::factions::Opinion::default(),
                                CohortMember(ri),
                            ))
                            .id();
                        cast_ids.push(id);
                    }
                }
                // The community's existing social fabric: some friendships (`Bond`) and a little
                // vassalage (`Liege`), seeded among the cast from the cohort's own RNG. Plausible
                // invention, not reconstructed history (the aggregate holds no social graph) — but it
                // gives the director something to strain, and lets a lord's death send a grudge down
                // the chain. Grievances are *not* seeded: the cast shares a tile, so an avenge goal
                // would trigger an instant spawn-bloodbath — manufacturing feuds is the director's job.
                if cast_ids.len() >= 2 {
                    let pick_other = |rng: &mut SplitMix64, self_i: usize, n: usize| -> Entity {
                        // A uniform other member: draw in [0, n-1) and skip past self.
                        let mut j = rng.gen_range(n - 1);
                        if j >= self_i {
                            j += 1;
                        }
                        cast_ids[j]
                    };
                    let n = cast_ids.len();
                    for (i, &member) in cast_ids.iter().enumerate() {
                        // The `frac > 0.0` short-circuit isn't just an optimization: it skips the
                        // `gen_bool` draw entirely when a tie is disabled, so zeroing a frac consumes
                        // *no* RNG — disabling the social fabric stays byte-identical, not merely
                        // silent.
                        if cfg.crystallize_bond_frac > Fx::ZERO
                            && crng.0.gen_bool(
                                cfg.crystallize_bond_frac
                                    .clamp(Fx::ZERO, Fx::ONE)
                                    .to_num::<f64>(),
                            )
                        {
                            let other = pick_other(&mut crng.0, i, n);
                            commands.entity(member).insert(Bond(other));
                        }
                        if cfg.crystallize_vassal_frac > Fx::ZERO
                            && crng.0.gen_bool(
                                cfg.crystallize_vassal_frac
                                    .clamp(Fx::ZERO, Fx::ONE)
                                    .to_num::<f64>(),
                            )
                        {
                            let lord = pick_other(&mut crng.0, i, n);
                            commands.entity(member).insert(Liege(lord));
                        }
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
                let mut sustenance_sum = Fx::ZERO;
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
                    cohort.sustenance = (cohort.sustenance * Fx::saturating_from_num(remainder)
                        + sustenance_sum)
                        / Fx::saturating_from_num(total_after);
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
