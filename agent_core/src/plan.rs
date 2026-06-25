//! The **GOAP planning layer**: turn a target *condition* into a *sequence* of
//! actions via forward A\* search.
//!
//! ## How it layers on the utility AI
//! The utility layer ([`goals`](crate::goals)) answers **what do I want most** —
//! it scores authored goals and picks one. Every goal is just a [`Condition`] (a
//! desired state of the world) plus an appeal. This layer answers **how do I make
//! that condition true**: from the agent's current situation it searches the
//! actions available and returns the cheapest plan that satisfies it. So
//! multi-step behaviour (bake then eat; walk to market, buy, walk home; buy cheap
//! here and haul it to sell dear there; travel somewhere and *do* a deed)
//! *emerges* from search rather than being hand-written.
//!
//! ## One abstraction for every goal
//! The planner never names "bread", "the market", or "the throne". It plans
//! toward a [`Condition`] over a shared fact vocabulary — needs, money, holdings,
//! and an open [`PlanState::facts`] vector for anything else (offices held, a foe
//! alive, …). "Have ≥ 10 bread", "money ≥ 1000", and `fact[throne] == me` are the
//! same kind of target; they differ only in which facts they name and which
//! operators can change those facts. Add a fact + an operator that sets it + an
//! authored goal, and a brand-new objective rides these exact rails.
//!
//! ## The operator seam
//! A state is expanded into successors by asking each operator what it can do.
//! Production operators come straight from the [`Registry`](crate::data::Registry)
//! (author a recipe and the planner can use it); the primitive operators (eat,
//! graze, rest, buy, sell, move) are generic over every good, market, and
//! neighbour; and [`Deed`]s are generic place-based operators that set an abstract
//! fact (the seam for non-economic mechanics). Nothing here is domain-specific.
//!
//! ## Purity
//! Nothing here touches `bevy_ecs` or mutates the substrate. The world is read
//! through plain closures ([`PlanCtx`]), so the planner is deterministic and
//! unit-testable without a running simulation. Execution — applying a plan's
//! first step to the real world — lives in [`people`](crate::people) and mirrors
//! the effects modelled here.

use crate::data::{GoodId, Recipe, Registry, ResourceKind};
use crate::people::{EconConfig, NeedsConfig, price};
use crate::scalar::Fx;
use game_sim::Coord;
use pathfinding::directed::astar::astar;
use smallvec::SmallVec;

/// A [`PlanState`]'s good-quantities, inline up to this many goods. The A\* search
/// clones the whole state for *every* successor of every node, so keeping the common
/// case (few goods) off the heap is the difference between two heap allocations per
/// node and none. Larger inventories simply spill to the heap as a normal `Vec`
/// would — correct, just not allocation-free.
pub type Stock = SmallVec<[u32; 8]>;
/// A [`PlanState`]'s abstract facts, inline up to this many slots — see [`Stock`].
pub type Facts = SmallVec<[i64; 8]>;

/// A read-only snapshot of one market for planning. Prices are computed from this
/// snapshot and held fixed across a plan (a short-horizon approximation; live
/// prices are used at execution).
#[derive(Clone, Debug)]
pub struct MarketSnapshot {
    pub pos: Coord,
    /// Real stock — governs what can be bought or sold here.
    pub stock: Vec<u32>,
    /// The smoothed stock prices are read from (see [`crate::people::Market`]).
    pub price_basis: Vec<u32>,
    pub money: i64,
}

/// A place-based operator that sets an abstract [fact](PlanState::facts): standing
/// on `at`, the agent may perform a deed that writes `value` into `fact`. This is
/// the generic, non-economic operator — "seize the throne" is a deed at the palace
/// setting `throne = me`; "avenge" is a deed at the foe setting `alive(foe) = 0`.
/// The economy uses none; supply some and they join planning automatically.
#[derive(Clone, Copy, Debug)]
pub struct Deed {
    pub at: Coord,
    pub fact: usize,
    pub value: i64,
}

/// A need meter an affordance can restore (mirrors [`PlanState`]'s meters).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Need {
    Sustenance,
    Rest,
}

/// What a feature's **affordance** does when used — the smart-object effect a place
/// advertises into the planner. `Relieve` restores a need (an oasis feeds you, an
/// inn rests you); `Yield` gathers a good here, optionally gated by (and only for) a
/// calling (a mine yields ore to a miner). The fact-setting effect keeps living in
/// [`Deed`], so the two seams stay separate.
#[derive(Clone, Copy, Debug)]
pub enum AffordEffect {
    Relieve {
        need: Need,
        amount: i32,
    },
    Yield {
        good: GoodId,
        units: u32,
        skill: Option<usize>,
    },
    /// Teach a calling — a guild or master lifts a skill the agent lacks above zero,
    /// so trades it could never run become possible. Occupational mobility: the one
    /// way a born calling widens.
    Teach {
        skill: usize,
    },
}

/// A place-based affordance available to the planner: standing on `at`, the agent
/// may perform `effect`. `available` is false for a depletable site that has been
/// worked out, so the planner won't route to a dry well. `needs_discovery` marks an
/// affordance on a Hidden/Secret feature — usable only once the agent has personally
/// found it (the planner won't route to a place it doesn't know about). Indexed by
/// [`Step::Use`].
#[derive(Clone, Copy, Debug)]
pub struct Affordance {
    pub at: Coord,
    /// Topology index of `at` (the planner has no topology, so it's precomputed) —
    /// the key the per-agent knowledge check uses.
    pub tile: usize,
    pub effect: AffordEffect,
    pub available: bool,
    pub needs_discovery: bool,
}

/// The slice of the world the planner can change, in symbolic form. Needs are
/// quantized to whole points so the state is hashable and the search space stays
/// finite; money and goods are already whole. `facts` is the open vocabulary for
/// anything beyond the economy (sized to the world's fact set; empty by default).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct PlanState {
    /// Sustenance meter, `0..=100`.
    pub sustenance: i32,
    /// Rest meter, `0..=100`.
    pub rest: i32,
    pub money: i64,
    pub stock: Stock,
    pub pos: Coord,
    /// Abstract world facts, indexed by fact id (offices, flags, …).
    pub facts: Facts,
    /// Skills *learned mid-plan* at a teaching affordance (per skill id, `0`/`1`),
    /// on top of the agent's born callings — so the planner can find "learn the
    /// trade, then practise it" without skills being fully mutable state.
    pub learned: Stock,
}

impl PlanState {
    /// Read fact `id` (absent facts read as `0`).
    fn fact(&self, id: usize) -> i64 {
        self.facts.get(id).copied().unwrap_or(0)
    }
}

/// Everything the planner reads but does not change: authored data, config, the
/// agent's skills, the markets and deeds available, and closures onto the world.
pub struct PlanCtx<'a> {
    pub reg: &'a Registry,
    pub econ: &'a EconConfig,
    pub needs_cfg: &'a NeedsConfig,
    /// Per-skill proficiency. Doubles as the agent's **calling**: a skill at `0` is
    /// one it was never taught and cannot practise (a baker has `farming == 0`), so
    /// the recipes it can run are gated by where it has any proficiency. The same
    /// number then scales the yield of the recipes it *can* run.
    pub skills: &'a [Fx],
    /// Markets the agent can trade at (by physical proximity); indexed by [`Step`].
    pub markets: &'a [MarketSnapshot],
    /// Place-based fact-setting operators available (usually empty).
    pub deeds: &'a [Deed],
    /// Place-based feature affordances available — gather/relieve actions a POI
    /// advertises. Indexed by [`Step::Use`]; usable only when standing on the site.
    pub affordances: &'a [Affordance],
    /// Does this agent know the (Hidden/Secret) feature at this tile index? Landmark
    /// affordances ignore it; the rest are usable only where this returns true, so an
    /// agent never routes to a place it hasn't discovered.
    pub known: &'a dyn Fn(usize) -> bool,
    /// Natural-resource levels at a tile, indexed by [`ResourceKind`]. Backed by
    /// a per-tick cache, so this is an O(1) read, not a fresh computation.
    pub resources: &'a dyn Fn(Coord) -> [f32; ResourceKind::COUNT],
    /// Land tiles reachable in one step from a tile, as a borrowed slice into a
    /// per-tick adjacency cache (no allocation per call — this is on the hot path).
    pub neighbors: &'a dyn Fn(Coord) -> &'a [Coord],
    /// Hard cap on nodes expanded, so planning is real-time safe.
    pub node_budget: usize,
}

impl PlanCtx<'_> {
    /// Can an agent standing at `pos` trade at market `m`? Yes if it stands on the
    /// market tile or an adjacent one (its catchment).
    fn at_market(&self, pos: Coord, m: &MarketSnapshot) -> bool {
        pos == m.pos || (self.neighbors)(pos).contains(&m.pos)
    }

    /// May this agent practise `skill`? Yes if it has any proficiency in it — its
    /// calling. Untaught skills sit at `0` and gate out the recipes that need them.
    fn can_practise(&self, skill: usize) -> bool {
        self.skills.get(skill).is_some_and(|&s| s > Fx::ZERO)
    }
}

/// One grounded action in a plan. Markets and deeds are referenced by index into
/// [`PlanCtx`] so the planner stays free of ECS handles; execution maps the index
/// back to the real entity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// Eat one unit of an edible good from inventory.
    Eat(GoodId),
    /// Graze the tile (subsistence floor).
    Graze,
    Rest,
    /// Run recipe `i` from the registry.
    Make(usize),
    /// Buy `units` of a good at a known market.
    Buy {
        good: GoodId,
        units: u32,
        market: usize,
    },
    /// Sell `units` of a good at a known market.
    Sell {
        good: GoodId,
        units: u32,
        market: usize,
    },
    /// Walk to an adjacent tile.
    Move(Coord),
    /// Perform deed `i` (sets an abstract fact; must be at the deed's tile).
    Do(usize),
    /// Use feature affordance `i` (gather/relieve; must be at the affordance's tile).
    Use(usize),
}

/// Which good(s) a holding condition counts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GoodSel {
    /// Any edible good (nutrition > 0) — a food larder.
    Edible,
    /// One specific good.
    Named(GoodId),
}

impl GoodSel {
    fn includes(self, g: GoodId, reg: &Registry) -> bool {
        match self {
            GoodSel::Edible => reg.good(g).nutrition > 0.0,
            GoodSel::Named(id) => id == g,
        }
    }
    /// How many matching units are held.
    fn count(self, s: &PlanState, reg: &Registry) -> u32 {
        match self {
            GoodSel::Edible => (0..reg.good_count())
                .filter(|&g| reg.good(g).nutrition > 0.0)
                .map(|g| s.stock[g])
                .sum(),
            GoodSel::Named(id) => s.stock[id],
        }
    }
}

/// A desired state of the world — the thing a goal aims to make true, and the only
/// thing the planner plans toward. Every goal is one of these over the shared fact
/// vocabulary, so "be fed", "keep a larder", "stay solvent", and "hold the throne"
/// are all the same kind of object.
// coupling-lint:allow self_match Condition: a generic planner-state vocabulary, not content — the
// economy content (goods/recipes/skills) is data. A new condition is a planning mechanic, not a row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Condition {
    /// Sustenance meter at least this (`0..=100`).
    Sustenance { at_least: i32 },
    /// Rest meter at least this.
    Rest { at_least: i32 },
    /// Coins at least this.
    Money { at_least: i64 },
    /// Hold at least this many of the selected good(s).
    Holding { good: GoodSel, at_least: u32 },
    /// An abstract fact equals this value (an office held, a flag set, …).
    Fact { fact: usize, equals: i64 },
}

impl Condition {
    /// Is the condition met in this state?
    pub fn satisfied(&self, s: &PlanState, reg: &Registry) -> bool {
        match *self {
            Condition::Sustenance { at_least } => s.sustenance >= at_least,
            Condition::Rest { at_least } => s.rest >= at_least,
            Condition::Money { at_least } => s.money >= at_least,
            Condition::Holding { good, at_least } => good.count(s, reg) >= at_least,
            Condition::Fact { fact, equals } => s.fact(fact) == equals,
        }
    }

    /// How far from satisfied, normalized `0..1` (`0` = met). This is the single
    /// generic feature the utility layer scores a goal's appeal from — the same
    /// axis whether the gap is hunger, an empty larder, or thin savings.
    pub fn deficit(&self, s: &PlanState, reg: &Registry) -> Fx {
        // The gap and target are whole units (meters, coins, stock), so the ratio is an exact
        // fixed-point division — no float intermediate on the appraisal path.
        let frac = |short: i64, target: i64| {
            (Fx::from_num(short.max(0)) / Fx::from_num(target.max(1))).clamp(Fx::ZERO, Fx::ONE)
        };
        match *self {
            Condition::Sustenance { at_least } => {
                frac((at_least - s.sustenance) as i64, at_least as i64)
            }
            Condition::Rest { at_least } => frac((at_least - s.rest) as i64, at_least as i64),
            Condition::Money { at_least } => frac(at_least - s.money, at_least),
            Condition::Holding { good, at_least } => {
                frac(at_least as i64 - good.count(s, reg) as i64, at_least as i64)
            }
            Condition::Fact { .. } => {
                if self.satisfied(s, reg) {
                    Fx::ZERO
                } else {
                    Fx::ONE
                }
            }
        }
    }

    /// A nearer, reachable version of this condition to actually plan toward — the
    /// next *leg* of a standing goal rather than the whole marathon. Filling a
    /// larder to 15 or saving to 200 is many steps; planning the lot in one search
    /// is slow and over-commits, so we aim a short hop ahead and replan as we go
    /// (this is the planning horizon; survival and one-shot goals are unchanged).
    pub fn planning_target(&self, s: &PlanState, reg: &Registry) -> Condition {
        /// Roughly how many units / coins a short plan can add in one leg.
        const HOLD_STEP: u32 = 3;
        const MONEY_STEP: i64 = 80;
        match *self {
            Condition::Holding { good, at_least } => Condition::Holding {
                good,
                at_least: at_least.min(good.count(s, reg) + HOLD_STEP),
            },
            Condition::Money { at_least } => Condition::Money {
                at_least: at_least.min(s.money + MONEY_STEP),
            },
            other => other,
        }
    }

    /// Optimistic remaining cost (in action-units) to satisfy the condition: the
    /// gap over the best single-action progress available. Guides search; need not
    /// be tight.
    fn heuristic(&self, s: &PlanState, ctx: &PlanCtx) -> f32 {
        match *self {
            Condition::Sustenance { at_least } => {
                (at_least - s.sustenance).max(0) as f32 / best_food_gain(s, ctx).max(1.0)
            }
            Condition::Rest { at_least } => {
                (at_least - s.rest).max(0) as f32 / ctx.needs_cfg.rest_recovery.max(1.0)
            }
            Condition::Money { at_least } => {
                (at_least - s.money).max(0) as f32 / best_coin_gain(s, ctx).max(1.0)
            }
            Condition::Holding { good, at_least } => {
                let short = at_least.saturating_sub(good.count(s, ctx.reg)) as f32;
                short / best_acquire(good, ctx).max(1.0)
            }
            Condition::Fact { fact, equals } => {
                if self.satisfied(s, ctx.reg) {
                    return 0.0;
                }
                // Head for the nearest deed that would set this fact (the throne,
                // the foe). Without this, the heuristic is flat and A\* can't find
                // a distant target within budget. `+1` for the deed itself. Capped
                // finite so that when no deed can satisfy it (unreachable), the
                // integer cost A\* uses doesn't overflow.
                ctx.deeds
                    .iter()
                    .filter(|d| d.fact == fact && d.value == equals)
                    .map(|d| tile_distance(s.pos, d.at) + 1.0)
                    .fold(f32::INFINITY, f32::min)
                    .min(1.0e6)
            }
        }
    }
}

/// A cheap step-count estimate between two tiles for guiding spatial search:
/// column + row offset distance. Not exact on the hex cylinder (it ignores the
/// east–west wrap), but it only needs to point search the right way.
fn tile_distance(a: Coord, b: Coord) -> f32 {
    ((a.col - b.col).abs() + (a.row - b.row).abs()) as f32
}

/// Best sustenance a single action could add from this state: the richest edible
/// good in hand, grazing if the tile has plants, or the most nourishing thing a
/// runnable recipe could make. Scales the sustenance heuristic.
fn best_food_gain(s: &PlanState, ctx: &PlanCtx) -> f32 {
    let in_hand = (0..ctx.reg.good_count())
        .filter(|&g| s.stock[g] > 0)
        .map(|g| ctx.reg.good(g).nutrition)
        .fold(0.0, f32::max);
    let grazing = if (ctx.resources)(s.pos)[ResourceKind::Vegetation.idx()] > 0.0 {
        ctx.needs_cfg.eat_grass_relief
    } else {
        0.0
    };
    let producible = ctx
        .reg
        .recipes()
        .iter()
        .filter(|r| ctx.can_practise(r.skill))
        .filter_map(|r| resource_scale(s, ctx, r).map(|_| r))
        .flat_map(|r| r.outputs.iter())
        .map(|&(g, _)| ctx.reg.good(g).nutrition)
        .fold(0.0, f32::max);
    // A forage-style affordance on this very tile feeds directly.
    let afforded = ctx
        .affordances
        .iter()
        .filter(|a| a.available && a.at == s.pos)
        .filter_map(|a| match a.effect {
            AffordEffect::Relieve {
                need: Need::Sustenance,
                amount,
            } => Some(amount as f32),
            _ => None,
        })
        .fold(0.0, f32::max);
    in_hand.max(grazing).max(producible).max(afforded)
}

/// Best coins a single action could add: the dearest sellable unit at a reachable
/// market. Scales the money heuristic.
fn best_coin_gain(s: &PlanState, ctx: &PlanCtx) -> f32 {
    let mut best = 0.0_f32;
    for m in ctx.markets {
        if !ctx.at_market(s.pos, m) {
            continue;
        }
        for g in 0..ctx.reg.good_count() {
            if s.stock[g] > 0 {
                best = best.max(price(ctx.reg, ctx.econ, g, m.price_basis[g]) as f32);
            }
        }
    }
    best.max(1.0)
}

/// Rough best units of the selected good a single action could add (produce or
/// buy a lot). Scales the holding heuristic.
fn best_acquire(sel: GoodSel, ctx: &PlanCtx) -> f32 {
    let produce = ctx
        .reg
        .recipes()
        .iter()
        .filter(|r| ctx.can_practise(r.skill))
        .flat_map(|r| r.outputs.iter())
        .filter(|&&(g, _)| sel.includes(g, ctx.reg))
        .map(|&(_, q)| q as f32)
        .fold(0.0, f32::max);
    // A feature that yields the good (and that this agent has the calling for).
    let afforded = ctx
        .affordances
        .iter()
        .filter(|a| a.available)
        .filter_map(|a| match a.effect {
            AffordEffect::Yield { good, units, skill }
                if sel.includes(good, ctx.reg) && skill.is_none_or(|sk| ctx.can_practise(sk)) =>
            {
                Some(units as f32)
            }
            _ => None,
        })
        .fold(0.0, f32::max);
    produce
        .max(afforded)
        .max(ctx.econ.trade_lot as f32)
        .max(1.0)
}

/// Output multiplier from a recipe's natural resource at this tile (`1` for a
/// craft); `None` if the tile lacks the minimum the recipe needs.
fn resource_scale(s: &PlanState, ctx: &PlanCtx, r: &Recipe) -> Option<f32> {
    match r.resource {
        None => Some(1.0),
        Some(kind) => {
            let level = (ctx.resources)(s.pos)[kind.idx()];
            (level >= r.min_resource).then_some(level)
        }
    }
}

/// Can the agent practise `skill`, given its born callings *and* anything learned so
/// far in this plan? The state-aware companion to [`PlanCtx::can_practise`] (which
/// knows only the born callings, for the heuristics that have no plan state to hand).
fn practises(ctx: &PlanCtx, s: &PlanState, skill: usize) -> bool {
    ctx.skills.get(skill).is_some_and(|&v| v > Fx::ZERO)
        || s.learned.get(skill).copied().unwrap_or(0) > 0
}

/// Apply a step to a state, returning the resulting state and its action cost, or
/// `None` if the step's preconditions don't hold. This is the single source of
/// truth for what each action *means* during planning; execution mirrors it.
fn apply(step: Step, s: &PlanState, ctx: &PlanCtx) -> Option<(PlanState, f32)> {
    let mut next = s.clone();
    let cost;
    match step {
        Step::Eat(g) => {
            if s.stock[g] == 0 || ctx.reg.good(g).nutrition <= 0.0 {
                return None;
            }
            next.stock[g] -= 1;
            next.sustenance = (s.sustenance + ctx.reg.good(g).nutrition.round() as i32).min(100);
            cost = 1.0;
        }
        Step::Graze => {
            if (ctx.resources)(s.pos)[ResourceKind::Vegetation.idx()] <= 0.0 {
                return None;
            }
            next.sustenance =
                (s.sustenance + ctx.needs_cfg.eat_grass_relief.round() as i32).min(100);
            cost = 1.0;
        }
        Step::Rest => {
            next.rest = (s.rest + ctx.needs_cfg.rest_recovery.round() as i32).min(100);
            cost = 1.0;
        }
        Step::Make(i) => {
            let r = &ctx.reg.recipes()[i];
            if !practises(ctx, s, r.skill) {
                return None;
            }
            let scale = resource_scale(s, ctx, r)?;
            if r.inputs.iter().any(|&(g, qty)| s.stock[g] < qty) {
                return None;
            }
            for &(g, qty) in &r.inputs {
                next.stock[g] -= qty;
            }
            let skill = ctx.skills.get(r.skill).copied().unwrap_or(Fx::ZERO);
            for &(g, qty) in &r.outputs {
                next.stock[g] += (Fx::saturating_from_num(qty)
                    * (Fx::ONE + skill)
                    * Fx::saturating_from_num(scale))
                .round()
                .saturating_to_num::<u32>();
            }
            cost = r.effort;
        }
        Step::Buy {
            good,
            units,
            market,
        } => {
            let m = &ctx.markets[market];
            if !ctx.at_market(s.pos, m) || m.stock[good] < units {
                return None;
            }
            let owed = units as i64 * price(ctx.reg, ctx.econ, good, m.price_basis[good]);
            if s.money < owed {
                return None;
            }
            next.money -= owed;
            next.stock[good] += units;
            cost = 1.0;
        }
        Step::Sell {
            good,
            units,
            market,
        } => {
            let m = &ctx.markets[market];
            if !ctx.at_market(s.pos, m) || s.stock[good] < units {
                return None;
            }
            let p = price(ctx.reg, ctx.econ, good, m.price_basis[good]);
            let payable = (m.money / p.max(1)) as u32;
            let units = units.min(payable);
            if units == 0 {
                return None;
            }
            next.money += units as i64 * p;
            next.stock[good] -= units;
            cost = 1.0;
        }
        Step::Move(to) => {
            if !(ctx.neighbors)(s.pos).contains(&to) {
                return None;
            }
            next.pos = to;
            cost = 1.0;
        }
        Step::Do(i) => {
            let d = ctx.deeds.get(i)?;
            if s.pos != d.at || s.fact(d.fact) == d.value || d.fact >= s.facts.len() {
                return None;
            }
            next.facts[d.fact] = d.value;
            cost = 1.0;
        }
        Step::Use(i) => {
            let a = ctx.affordances.get(i)?;
            if !a.available || s.pos != a.at || (a.needs_discovery && !(ctx.known)(a.tile)) {
                return None;
            }
            match a.effect {
                AffordEffect::Relieve { need, amount } => match need {
                    Need::Sustenance => {
                        if s.sustenance >= 100 {
                            return None; // already full — no point
                        }
                        next.sustenance = (s.sustenance + amount).min(100);
                    }
                    Need::Rest => {
                        if s.rest >= 100 {
                            return None;
                        }
                        next.rest = (s.rest + amount).min(100);
                    }
                },
                AffordEffect::Yield { good, units, skill } => {
                    if skill.is_some_and(|sk| !practises(ctx, s, sk)) {
                        return None; // takes a calling this agent doesn't have
                    }
                    next.stock[good] += units;
                }
                AffordEffect::Teach { skill } => {
                    if practises(ctx, s, skill) || skill >= next.learned.len() {
                        return None; // already has this calling — nothing to learn
                    }
                    next.learned[skill] = 1;
                }
            }
            cost = 1.0;
        }
    }
    Some((next, cost))
}

/// Every action available from a state, each already applied. Operators are
/// listed in a fixed order so planning is deterministic. New mechanics extend the
/// list; the search loop never changes.
fn successors(s: &PlanState, ctx: &PlanCtx) -> Vec<(Step, PlanState, f32)> {
    let lot = ctx.econ.trade_lot;
    let mut out = Vec::new();
    let mut push = |step: Step| {
        if let Some((next, cost)) = apply(step, s, ctx) {
            out.push((step, next, cost));
        }
    };

    // Consume — eat each edible good actually in hand, then graze where the tile bears
    // something. Both gates mirror exactly what `apply` rejects (an empty larder, a barren
    // tile), so skipping the operator here is byte-identical to generating it and having
    // `apply` return `None` — but it avoids the `PlanState` clone `apply` pays up front for
    // every successor, which is the dominant per-tick allocation (see `docs/scaling.md`).
    for g in 0..ctx.reg.good_count() {
        if ctx.reg.good(g).nutrition > 0.0 && s.stock[g] > 0 {
            push(Step::Eat(g));
        }
    }
    if (ctx.resources)(s.pos)[ResourceKind::Vegetation.idx()] > 0.0 {
        push(Step::Graze);
    }

    // Recover.
    push(Step::Rest);

    // Produce — one operator per recipe this agent can *practise* (its born callings, plus
    // anything learned earlier in the plan). A recipe outside its callings is rejected by
    // `apply` after a clone; gating here on the same `practises` check skips that clone for
    // the recipes that make up the bulk of a registry, leaving the search tree identical.
    for i in 0..ctx.reg.recipes().len() {
        if practises(ctx, s, ctx.reg.recipes()[i].skill) {
            push(Step::Make(i));
        }
    }

    // Trade — every good at every reachable market.
    for (market, m) in ctx.markets.iter().enumerate() {
        if !ctx.at_market(s.pos, m) {
            continue;
        }
        for g in 0..ctx.reg.good_count() {
            push(Step::Buy {
                good: g,
                units: lot.min(m.stock[g]),
                market,
            });
            push(Step::Sell {
                good: g,
                units: lot.min(s.stock[g]),
                market,
            });
        }
    }

    // Deeds — any place-based fact-setting action available here.
    for i in 0..ctx.deeds.len() {
        push(Step::Do(i));
    }

    // Affordances — any feature action (gather/relieve) available here.
    for i in 0..ctx.affordances.len() {
        push(Step::Use(i));
    }

    // Move to each land neighbour.
    for &to in (ctx.neighbors)(s.pos) {
        push(Step::Move(to));
    }

    out
}

/// Quantize a float cost/heuristic to an integer (A\* needs an `Ord` cost; ours
/// are small f32s). 1 milli-unit resolution.
fn key(f: f32) -> i64 {
    (f * 1000.0).round() as i64
}

/// Plan a sequence of [`Step`]s that makes `condition` true from `start`, via A\*
/// (the [`pathfinding`] crate) over [`successors`].
///
/// The search is bounded by `node_budget` (a counter on expansions) so it always
/// returns promptly. There is no depth cap, so the heuristic leads it straight to
/// far-but-reachable goals — accumulating a larder, hauling for profit — in a single
/// plan.
///
/// When the goal lies *beyond* the budget (a foe or a throne far across the map), the
/// whole path can't be found in one search. Rather than abandon it, the planner steps
/// toward it: it remembers the closest the search came (the lowest-heuristic state)
/// and returns a walk to *that* waypoint. Replanning each tick draws the agent in, leg
/// by leg, until the goal finally falls within a single search's reach — an
/// incremental, anytime pursuit that needs neither a depth bound nor a bespoke search.
///
/// Plans stay deterministic: `successors` is generated in a fixed order and the
/// search has no randomness.
pub fn plan(condition: &Condition, start: &PlanState, ctx: &PlanCtx) -> Vec<Step> {
    if condition.satisfied(start, ctx.reg) {
        return Vec::new();
    }

    let mut budget = ctx.node_budget;
    // The nearest the search comes to the goal (lowest heuristic seen), so a goal
    // past the budget can be approached rather than given up on.
    let mut nearest: Option<(i64, PlanState)> = None;
    let found = astar(
        start,
        |s| {
            // Exhaust the budget by refusing to expand further; A\* then ends.
            if budget == 0 {
                return Vec::new();
            }
            budget -= 1;
            successors(s, ctx)
                .into_iter()
                .map(|(_, next, cost)| (next, key(cost)))
                .collect::<Vec<_>>()
        },
        |s| {
            let h = key(condition.heuristic(s, ctx));
            if nearest.as_ref().is_none_or(|(best, _)| h < *best) {
                nearest = Some((h, s.clone()));
            }
            h
        },
        |s| condition.satisfied(s, ctx.reg),
    );

    match found {
        Some((states, _cost)) => steps_between(&states, ctx),
        // Out of reach this tick: walk toward the nearest point reached, if it's any
        // closer than where we stand (else there's nothing useful to do — replan).
        None => match nearest {
            Some((_, waypoint)) if waypoint.pos != start.pos => approach(start, waypoint.pos, ctx),
            _ => Vec::new(),
        },
    }
}

/// Plan the walk to `tile` — the legs toward a goal that lies past a single search's
/// budget. A purely spatial search (`tile_distance` guides it, and only [`Step::Move`]
/// reduces it), so it yields the path and nothing else.
fn approach(start: &PlanState, tile: Coord, ctx: &PlanCtx) -> Vec<Step> {
    let mut budget = ctx.node_budget;
    let found = astar(
        start,
        |s| {
            if budget == 0 {
                return Vec::new();
            }
            budget -= 1;
            successors(s, ctx)
                .into_iter()
                .map(|(_, next, cost)| (next, key(cost)))
                .collect::<Vec<_>>()
        },
        |s| key(tile_distance(s.pos, tile)),
        |s| s.pos == tile,
    );
    found
        .map(|(states, _)| steps_between(&states, ctx))
        .unwrap_or_default()
}

/// Recover the action taken between each consecutive pair of states A\* hands back.
fn steps_between(states: &[PlanState], ctx: &PlanCtx) -> Vec<Step> {
    states
        .windows(2)
        .map(|w| {
            successors(&w[0], ctx)
                .into_iter()
                .find(|(_, next, _)| *next == w[1])
                .map(|(s, _, _)| s)
                .unwrap()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{DataFiles, Registry};

    /// A market snapshot whose price basis equals its stock (no smoothing lag — the
    /// scenario sets the price directly through the stock it hands in).
    fn market(pos: Coord, stock: Vec<u32>, money: i64) -> MarketSnapshot {
        MarketSnapshot {
            pos,
            price_basis: stock.clone(),
            stock,
            money,
        }
    }

    /// A tiny test world: a row of tiles `0..n`, with the resources, markets, and
    /// deeds the test dictates.
    struct TestWorld {
        reg: Registry,
        econ: EconConfig,
        needs: NeedsConfig,
        /// Per-skill proficiency; `0` means the agent can't practise that skill.
        /// Defaults to an all-`1.0` generalist so a test that doesn't care about
        /// callings can still make anything.
        skills: Vec<Fx>,
        /// resources per col index.
        resources: Vec<[f32; ResourceKind::COUNT]>,
        markets: Vec<MarketSnapshot>,
        deeds: Vec<Deed>,
        affordances: Vec<Affordance>,
        /// Tiles the agent has discovered; `None` = knows everything (the default).
        known_tiles: Option<Vec<usize>>,
        facts: usize,
        budget: usize,
    }

    impl TestWorld {
        fn new() -> Self {
            Self::with_reg(Registry::bundled())
        }

        fn with_reg(reg: Registry) -> Self {
            let skills = vec![Fx::ONE; reg.skill_count()];
            Self {
                reg,
                econ: EconConfig::default(),
                needs: NeedsConfig::default(),
                skills,
                resources: Vec::new(),
                markets: Vec::new(),
                deeds: Vec::new(),
                affordances: Vec::new(),
                known_tiles: None,
                facts: 0,
                budget: 4000,
            }
        }

        fn at(col: i32) -> Coord {
            Coord::new(col, 0)
        }

        /// Build this world's planning context and run `f` against it.
        fn with_ctx<R>(&self, f: impl FnOnce(&PlanCtx) -> R) -> R {
            let resources = |c: Coord| {
                self.resources
                    .get(c.col as usize)
                    .copied()
                    .unwrap_or([0.0; ResourceKind::COUNT])
            };
            // A simple line graph: col c borders c-1 and c+1 within range, cached
            // up front so `neighbors` hands back a borrowed slice (as in the sim).
            let n = self.resources.len().max(
                self.deeds
                    .iter()
                    .map(|d| d.at.col as usize + 1)
                    .max()
                    .unwrap_or(0),
            );
            let adjacency: Vec<Vec<Coord>> = (0..n as i32)
                .map(|col| {
                    [col - 1, col + 1]
                        .into_iter()
                        .filter(|&x| (0..n as i32).contains(&x))
                        .map(|x| Coord::new(x, 0))
                        .collect()
                })
                .collect();
            let neighbors = |c: Coord| adjacency[c.col as usize].as_slice();
            let known = |i: usize| self.known_tiles.as_ref().is_none_or(|k| k.contains(&i));
            let ctx = PlanCtx {
                reg: &self.reg,
                econ: &self.econ,
                needs_cfg: &self.needs,
                skills: &self.skills,
                markets: &self.markets,
                deeds: &self.deeds,
                affordances: &self.affordances,
                known: &known,
                resources: &resources,
                neighbors: &neighbors,
                node_budget: self.budget,
            };
            f(&ctx)
        }

        fn plan(&self, condition: Condition, start: PlanState) -> Vec<Step> {
            self.with_ctx(|ctx| plan(&condition, &start, ctx))
        }

        fn state(&self, sustenance: i32, money: i64, stock: Vec<u32>, col: i32) -> PlanState {
            PlanState {
                sustenance,
                rest: 100,
                money,
                stock: Stock::from_vec(stock),
                pos: TestWorld::at(col),
                facts: Facts::from_elem(0, self.facts),
                learned: Stock::from_elem(0, self.reg.skill_count()),
            }
        }

        fn empty_stock(&self) -> Vec<u32> {
            vec![0; self.reg.good_count()]
        }
    }

    #[test]
    fn a_distant_deed_is_approached_leg_by_leg() {
        // The deed (a foe to strike, a throne to seize) sits 12 tiles down the line,
        // far past what a tiny search budget can reach in one plan. The planner must
        // not give up: each plan walks part-way, and replanning from the new tile
        // closes the rest — incremental pursuit converging on a goal it can't reach
        // in a single search.
        let mut w = TestWorld::new();
        w.facts = 1;
        w.budget = 4; // can't see all 12 tiles ahead at once
        w.resources = vec![[0.0; ResourceKind::COUNT]; 13];
        w.deeds = vec![Deed {
            at: TestWorld::at(12),
            fact: 0,
            value: 1,
        }];
        let goal = Condition::Fact { fact: 0, equals: 1 };

        let mut s = w.state(100, 0, w.empty_stock(), 0);
        let mut legs = 0;
        while !goal.satisfied(&s, &w.reg) && legs < 100 {
            let plan = w.plan(goal, s.clone());
            assert!(
                !plan.is_empty(),
                "a reachable deed must always yield progress (at col {})",
                s.pos.col
            );
            // Carry out one step of the leg, as the act phase would, then replan.
            s = w
                .with_ctx(|ctx| apply(plan[0], &s, ctx))
                .expect("planned step applies")
                .0;
            legs += 1;
        }
        assert!(
            goal.satisfied(&s, &w.reg),
            "incremental approach should reach and do the distant deed"
        );
        assert!(
            s.pos.col == 12,
            "the agent should have walked all the way to the deed tile"
        );
    }

    #[test]
    fn the_hungry_with_food_just_eat() {
        let mut w = TestWorld::new();
        w.resources = vec![[0.0; ResourceKind::COUNT]]; // no grass — force eating
        let bread = w.reg.good_id("bread").unwrap();
        let mut stock = w.empty_stock();
        stock[bread] = 3;
        let p = w.plan(
            Condition::Sustenance { at_least: 70 },
            w.state(10, 0, stock, 0),
        );
        assert_eq!(p.first(), Some(&Step::Eat(bread)), "plan: {p:?}");
    }

    #[test]
    fn a_multi_step_craft_to_eat_chain_emerges() {
        // A world where the only edible good must be crafted from an *inedible*
        // input: flour (nutrition 0) bakes into a loaf (nutrition 50). A hungry
        // agent holding only flour, with no grass, must plan bake-then-eat.
        let goods = r#"[(name: "flour", base_price: 5, target_stock: 20, nutrition: 0.0),
                        (name: "loaf",  base_price: 20, target_stock: 20, nutrition: 50.0)]"#;
        let skills = r#"[(name: "baking", gain: 0.02, cap: 5.0)]"#;
        let recipes = r#"[(name: "bake", skill: "baking", inputs: [("flour", 2)],
            outputs: [("loaf", 1)], resource: None, min_resource: 0.0, deplete: 0.0, effort: 1.0)]"#;
        let mut w = TestWorld::with_reg(
            Registry::from_ron(DataFiles {
                goods,
                skills,
                recipes,
                ..Default::default()
            })
            .unwrap(),
        );
        w.resources = vec![[0.0; ResourceKind::COUNT]];
        let (flour, loaf) = (
            w.reg.good_id("flour").unwrap(),
            w.reg.good_id("loaf").unwrap(),
        );
        let bake = w
            .reg
            .recipes()
            .iter()
            .position(|r| r.name == "bake")
            .unwrap();
        let mut stock = w.empty_stock();
        stock[flour] = 4;
        // Mildly hungry: a single loaf (50) clears the sate target, so the whole
        // plan is exactly bake-then-eat.
        let p = w.plan(
            Condition::Sustenance { at_least: 70 },
            w.state(30, 0, stock, 0),
        );
        assert_eq!(p, vec![Step::Make(bake), Step::Eat(loaf)], "plan: {p:?}");
    }

    #[test]
    fn the_hungry_and_moneyed_walk_to_market_and_buy() {
        // No food and no grass anywhere; the only bread is at a market two tiles
        // away (col 2). A hungry agent at col 0 must walk there, buy, and eat.
        let mut w = TestWorld::new();
        w.resources = vec![[0.0; ResourceKind::COUNT]; 3];
        let bread = w.reg.good_id("bread").unwrap();
        let mut mstock = vec![0; w.reg.good_count()];
        mstock[bread] = 50;
        w.markets = vec![market(TestWorld::at(2), mstock, 100_000)];
        let p = w.plan(
            Condition::Sustenance { at_least: 70 },
            w.state(10, 1000, w.empty_stock(), 0),
        );
        assert!(
            p.iter().any(|s| matches!(s, Step::Move(_))),
            "should walk toward the market: {p:?}"
        );
        assert!(
            p.iter()
                .any(|s| matches!(s, Step::Buy { good, .. } if *good == bread)),
            "should buy bread: {p:?}"
        );
        assert!(p.contains(&Step::Eat(bread)), "should end up eating: {p:?}");
    }

    #[test]
    fn restocking_a_larder_is_just_a_holding_goal() {
        // Grocery shopping: not hungry at all (sustenance 100), but the pantry is
        // bare and a market next door sells bread. "Keep ≥ 6 food" is satisfied by
        // buying — no hunger in sight, just keeping the larder stocked.
        let mut w = TestWorld::new();
        w.resources = vec![[0.0; ResourceKind::COUNT]];
        let bread = w.reg.good_id("bread").unwrap();
        let mut mstock = vec![0; w.reg.good_count()];
        mstock[bread] = 50;
        w.markets = vec![market(TestWorld::at(0), mstock, 100_000)];
        let p = w.plan(
            Condition::Holding {
                good: GoodSel::Edible,
                at_least: 6,
            },
            w.state(100, 1000, w.empty_stock(), 0),
        );
        assert!(
            p.iter()
                .any(|s| matches!(s, Step::Buy { good, .. } if *good == bread)),
            "should buy bread to restock: {p:?}"
        );
    }

    #[test]
    fn the_broke_on_fertile_land_plan_to_farm_and_sell() {
        // Fertile tile, a market to sell grain at. Money target → farm then sell.
        let mut w = TestWorld::new();
        let mut res = [0.0; ResourceKind::COUNT];
        res[ResourceKind::Fertility.idx()] = 3.0;
        w.resources = vec![res];
        let grain = w.reg.good_id("grain").unwrap();
        let farm = w
            .reg
            .recipes()
            .iter()
            .position(|r| r.name == "farm")
            .unwrap();
        w.markets = vec![market(
            TestWorld::at(0),
            vec![20; w.reg.good_count()],
            100_000,
        )];
        let p = w.plan(
            Condition::Money { at_least: 30 },
            w.state(100, 0, w.empty_stock(), 0),
        );
        assert!(p.contains(&Step::Make(farm)), "should farm: {p:?}");
        assert!(
            p.iter()
                .any(|s| matches!(s, Step::Sell { good, .. } if *good == grain)),
            "should sell grain: {p:?}"
        );
    }

    #[test]
    fn arbitrage_emerges_as_buy_move_sell() {
        // Grain is cheap at the home market (col 0, glutted) and dear two tiles
        // away (col 2, nearly empty). A profit-seeker should buy here, walk, sell.
        let mut w = TestWorld::new();
        w.resources = vec![[0.0; ResourceKind::COUNT]; 3];
        let grain = w.reg.good_id("grain").unwrap();
        let mut cheap = vec![0; w.reg.good_count()];
        cheap[grain] = 400; // glut → low price
        let mut dear = vec![0; w.reg.good_count()];
        dear[grain] = 2; // scarce → high price
        w.markets = vec![
            market(TestWorld::at(0), cheap, 100_000),
            market(TestWorld::at(2), dear, 100_000),
        ];
        // Start with little money; the target is only reachable by buying low
        // here and selling high two tiles over.
        let p = w.plan(
            Condition::Money { at_least: 300 },
            w.state(100, 100, w.empty_stock(), 0),
        );
        assert!(
            p.iter().any(|s| matches!(s, Step::Buy { market: 0, .. })),
            "should buy at the cheap market: {p:?}"
        );
        assert!(
            p.iter().any(|s| matches!(s, Step::Move(_))),
            "should travel: {p:?}"
        );
        assert!(
            p.iter().any(|s| matches!(s, Step::Sell { market: 1, .. })),
            "should sell at the dear market: {p:?}"
        );
    }

    #[test]
    fn a_non_economic_goal_rides_the_same_rails() {
        // A wildly different objective on identical machinery: an abstract fact
        // `enthroned` (fact 0), a deed at the palace (col 3) that sets it, and a
        // goal "be enthroned". With no economy involved, the planner walks to the
        // palace and seizes it — proving goal/fact/operator are fully general.
        let mut w = TestWorld::new();
        w.resources = vec![[0.0; ResourceKind::COUNT]; 4];
        w.facts = 1;
        w.deeds = vec![Deed {
            at: TestWorld::at(3),
            fact: 0,
            value: 1,
        }];
        let p = w.plan(
            Condition::Fact { fact: 0, equals: 1 },
            w.state(100, 0, w.empty_stock(), 0),
        );
        assert_eq!(
            p.last(),
            Some(&Step::Do(0)),
            "plan should end by seizing the throne: {p:?}"
        );
        assert_eq!(
            p.iter().filter(|s| matches!(s, Step::Move(_))).count(),
            3,
            "should walk three tiles: {p:?}"
        );
    }

    #[test]
    fn a_hungry_agent_treks_to_a_forage_affordance() {
        // No food, no grass, no market — but an oasis three tiles away advertises a
        // forage affordance that relieves hunger. The agent should walk there and use
        // it: a feature that is a *place agents go*, not scenery.
        let mut w = TestWorld::new();
        w.resources = vec![[0.0; ResourceKind::COUNT]; 4];
        w.affordances = vec![Affordance {
            at: TestWorld::at(3),
            tile: 3,
            effect: AffordEffect::Relieve {
                need: Need::Sustenance,
                amount: 60,
            },
            available: true,
            needs_discovery: false,
        }];
        let p = w.plan(
            Condition::Sustenance { at_least: 70 },
            w.state(20, 0, w.empty_stock(), 0),
        );
        assert!(
            p.iter().any(|s| matches!(s, Step::Move(_))),
            "should walk toward the oasis: {p:?}"
        );
        assert!(
            p.contains(&Step::Use(0)),
            "should forage at the oasis: {p:?}"
        );
    }

    #[test]
    fn an_undiscovered_affordance_is_unreachable_until_known() {
        // A hidden forage cave three tiles off. An agent that hasn't found it won't
        // plan to use it; once it's in the agent's known set, the same plan appears.
        let make = |known: Option<Vec<usize>>| {
            let mut w = TestWorld::new();
            w.resources = vec![[0.0; ResourceKind::COUNT]; 4];
            w.known_tiles = known;
            w.affordances = vec![Affordance {
                at: TestWorld::at(3),
                tile: 3,
                effect: AffordEffect::Relieve {
                    need: Need::Sustenance,
                    amount: 60,
                },
                available: true,
                needs_discovery: true,
            }];
            w.plan(
                Condition::Sustenance { at_least: 70 },
                w.state(20, 0, w.empty_stock(), 0),
            )
        };
        assert!(
            !make(Some(vec![])).contains(&Step::Use(0)),
            "an unknown hidden site must not be used"
        );
        assert!(
            make(Some(vec![3])).contains(&Step::Use(0)),
            "once discovered, the site is usable"
        );
    }

    #[test]
    fn a_yield_affordance_gathers_a_good() {
        // A site two tiles away yields grain. An agent who wants grain in hand walks
        // there and gathers it — a feature as a place-based source of a good.
        let mut w = TestWorld::new();
        w.resources = vec![[0.0; ResourceKind::COUNT]; 3];
        let grain = w.reg.good_id("grain").unwrap();
        w.affordances = vec![Affordance {
            at: TestWorld::at(2),
            tile: 2,
            effect: AffordEffect::Yield {
                good: grain,
                units: 3,
                skill: None,
            },
            available: true,
            needs_discovery: false,
        }];
        let p = w.plan(
            Condition::Holding {
                good: GoodSel::Named(grain),
                at_least: 3,
            },
            w.state(100, 0, w.empty_stock(), 0),
        );
        assert!(
            p.contains(&Step::Use(0)),
            "should gather grain at the yield site: {p:?}"
        );
    }

    #[test]
    fn a_worked_out_affordance_is_ignored() {
        // The same oasis, but depleted (`available: false`): the planner must not
        // route to a dry site — it finds no plan rather than a phantom one.
        let mut w = TestWorld::new();
        w.resources = vec![[0.0; ResourceKind::COUNT]; 4];
        w.affordances = vec![Affordance {
            at: TestWorld::at(3),
            tile: 3,
            effect: AffordEffect::Relieve {
                need: Need::Sustenance,
                amount: 60,
            },
            available: false,
            needs_discovery: false,
        }];
        let p = w.plan(
            Condition::Sustenance { at_least: 70 },
            w.state(20, 0, w.empty_stock(), 0),
        );
        assert!(
            !p.contains(&Step::Use(0)),
            "a worked-out site must not be used: {p:?}"
        );
    }

    #[test]
    fn a_calling_gates_what_you_can_make() {
        // Fertile tile + a market. A farmer (has farming) plans to farm and sell;
        // a baker (baking only, farming == 0) cannot farm at all, whatever the land
        // offers — production splits by calling, so the baker must trade for grain.
        let mut w = TestWorld::new();
        let mut res = [0.0; ResourceKind::COUNT];
        res[ResourceKind::Fertility.idx()] = 3.0;
        w.resources = vec![res];
        let farm = w
            .reg
            .recipes()
            .iter()
            .position(|r| r.name == "farm")
            .unwrap();
        let farming = w.reg.skill_id("farming").unwrap();
        let baking = w.reg.skill_id("baking").unwrap();
        w.markets = vec![market(
            TestWorld::at(0),
            vec![20; w.reg.good_count()],
            100_000,
        )];

        w.skills = vec![Fx::ZERO; w.reg.skill_count()];
        w.skills[farming] = Fx::ONE;
        let farmer = w.plan(
            Condition::Money { at_least: 30 },
            w.state(100, 0, w.empty_stock(), 0),
        );
        assert!(
            farmer.contains(&Step::Make(farm)),
            "a farmer should farm: {farmer:?}"
        );

        w.skills = vec![Fx::ZERO; w.reg.skill_count()];
        w.skills[baking] = Fx::ONE;
        let baker = w.plan(
            Condition::Money { at_least: 30 },
            w.state(100, 0, w.empty_stock(), 0),
        );
        assert!(
            !baker.contains(&Step::Make(farm)),
            "a baker cannot farm: {baker:?}"
        );
    }

    #[test]
    fn a_guild_teaches_a_new_trade() {
        // A farmer (no baking) holding grain wants bread. It cannot bake — but a guild
        // one tile over teaches baking. The planner learns there, then bakes: the
        // "learn the trade, then practise it" chain occupational mobility makes possible.
        let mut w = TestWorld::new();
        w.resources = vec![[0.0; ResourceKind::COUNT]; 3];
        let baking = w.reg.skill_id("baking").unwrap();
        let farming = w.reg.skill_id("farming").unwrap();
        let (grain, bread) = (
            w.reg.good_id("grain").unwrap(),
            w.reg.good_id("bread").unwrap(),
        );
        let bake = w
            .reg
            .recipes()
            .iter()
            .position(|r| r.name == "bake")
            .unwrap();
        w.skills = vec![Fx::ZERO; w.reg.skill_count()];
        w.skills[farming] = Fx::ONE; // a farmer — cannot bake
        w.affordances = vec![Affordance {
            at: TestWorld::at(1),
            tile: 1,
            effect: AffordEffect::Teach { skill: baking },
            available: true,
            needs_discovery: false,
        }];
        let mut stock = w.empty_stock();
        stock[grain] = 4;
        let p = w.plan(
            Condition::Holding {
                good: GoodSel::Named(bread),
                at_least: 1,
            },
            w.state(100, 0, stock, 0),
        );
        assert!(
            p.contains(&Step::Use(0)),
            "should apprentice at the guild: {p:?}"
        );
        assert!(
            p.contains(&Step::Make(bake)),
            "and then bake what it learned: {p:?}"
        );
    }

    #[test]
    fn planning_is_deterministic() {
        let mut w = TestWorld::new();
        w.resources = vec![[0.0; ResourceKind::COUNT]];
        let grain = w.reg.good_id("grain").unwrap();
        let mut stock = w.empty_stock();
        stock[grain] = 4;
        let a = w.plan(
            Condition::Sustenance { at_least: 70 },
            w.state(10, 0, stock.clone(), 0),
        );
        let b = w.plan(
            Condition::Sustenance { at_least: 70 },
            w.state(10, 0, stock, 0),
        );
        assert_eq!(a, b);
    }
}
