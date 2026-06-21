//! Factions — higher-order actors with **government**, **law**, **enforcement**, and
//! **war**. A bloc of people loyal to a **court** (a tile feature) runs the same
//! perceive/decide/interact loop one level up (the *Worlds Without Number* faction
//! turn), but now it is a *governed* body: it is led according to its **government**
//! (a monarch, an oligarchic council, or an elected representative), it lays **laws**
//! on its members (taboos that extend their norms; exclusions that forbid belonging to
//! a rival), it **enforces** those laws through detention and worse, and it makes
//! **war** on its rivals. People may belong to **several** factions at once.
//!
//! Factions are **persistent by seat**: a court's government, laws, and wars carry
//! across turns; only the membership is recomputed (loyalty follows power, fairness,
//! and the pull of a near court). Deterministic — no RNG. The only economy effect is
//! tribute flowing member→leader (conserved); war and enforcement remove people.

use crate::chronicle::EpisodeKind;
use crate::data::{PredicateId, Registry};
use crate::features::{Category, FeatureCatalog, Features};
use crate::people::{Grievance, Inventory, Npc, Personality};
use crate::{Position, Substrate};
use bevy_ecs::prelude::*;
use game_sim::{Coord, Topology};
use smallvec::SmallVec;
use std::collections::HashMap;

/// How a faction is led — which sets who rules, who legislates, and how hard it taxes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Government {
    /// One ruler for life — the most ambitious member. Rules alone and takes full tribute.
    Monarchy,
    /// A council of the wealthiest and most ambitious. Shares rule; taxes a little lighter.
    Oligarchy,
    /// A leader elected by the members — the one most representative of them. Lightest tribute.
    Democracy,
}

impl Government {
    /// Tribute this government levies, as a fraction of the base `tax_rate` — power
    /// taxes hardest, an accountable democracy least.
    fn tribute_mult(self) -> f32 {
        match self {
            Government::Monarchy => 1.0,
            Government::Oligarchy => 0.8,
            Government::Democracy => 0.5,
        }
    }
}

/// A statute a faction lays on its members.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Law {
    /// Members may not also belong to the faction seated at this court — a rivalry made
    /// law (declared when the two go to war).
    Exclude(Coord),
    /// An act forbidden to members: the `(predicate, value)` it makes true (e.g. the
    /// `avenge` act is `alive(foe) = 0`). It extends members' deontic norms, and
    /// breaking it draws the enforcers.
    Taboo(PredicateId, i64),
}

/// One person's bond to a faction it belongs to (people may hold several).
#[derive(Clone, Copy, Debug)]
pub struct Bond {
    pub seat: Coord,
    pub loyalty: f32,
}

/// The factions a person belongs to — **multiple** allowed (up to
/// [`FactionConfig::max_factions`]), each with its own loyalty. Empty = unaffiliated.
#[derive(Component, Clone, Debug, Default)]
pub struct Allegiance(pub SmallVec<[Bond; 4]>);

impl Allegiance {
    /// Is this person a member of the faction seated here?
    pub fn belongs_to(&self, seat: Coord) -> bool {
        self.0.iter().any(|b| b.seat == seat)
    }
}

/// Held by a faction's enforcers for breaking its law — the person cannot act while
/// detained. Counts down each tick ([`detention_countdown`]).
#[derive(Component, Clone, Copy, Debug)]
pub struct Detained {
    pub ticks: u32,
}

/// A person's directed **opinion** of others it has dealt with (`-1..1`) — sparse, and
/// kept to the figures that matter (the leaders it has served, the rivals it has
/// fought). Serving a leader warms the opinion of them; warring against one sours it;
/// all opinions fade toward indifference. It is what makes allegiance follow
/// *relationships* and not only power: people are drawn to leaders they like and shun
/// those they loathe. The seed of the wider relationship graph.
#[derive(Component, Clone, Debug, Default)]
pub struct Opinion(pub HashMap<Entity, f32>);

impl Opinion {
    /// This person's opinion of `other` (0 if they have none).
    pub fn of(&self, other: Entity) -> f32 {
        self.0.get(&other).copied().unwrap_or(0.0)
    }
}

/// A faction: a bloc around a court, persistent by seat. Government, laws, and wars
/// carry across turns; membership is recomputed each turn.
#[derive(Clone, Debug)]
pub struct Faction {
    pub seat: Coord,
    pub government: Government,
    /// The ruling body — one for a monarchy/democracy, several for an oligarchy; the
    /// head is `leaders[0]`.
    pub leaders: SmallVec<[Entity; 4]>,
    pub members: Vec<Entity>,
    pub laws: SmallVec<[Law; 4]>,
    /// Rival factions this one is at war with (their seats).
    pub at_war: SmallVec<[Coord; 4]>,
    /// WWN stats, summed from members: Force (numbers), Cunning (Σ ambition), Wealth.
    pub force: f32,
    pub cunning: f32,
    pub wealth: i64,
}

impl Faction {
    /// The head of the faction (the monarch, the senior oligarch, the elected leader).
    pub fn head(&self) -> Option<Entity> {
        self.leaders.first().copied()
    }

    /// Does a law of this faction forbid the act `(predicate, value)`?
    pub fn forbids(&self, act: (PredicateId, i64)) -> bool {
        self.laws.iter().any(|l| matches!(l, Law::Taboo(p, v) if (*p, *v) == act))
    }

    /// Does a law of this faction bar belonging to the faction seated at `seat`?
    pub fn excludes(&self, seat: Coord) -> bool {
        self.laws.iter().any(|l| matches!(l, Law::Exclude(s) if *s == seat))
    }
}

/// Every faction in the world, persistent by seat.
#[derive(Resource, Clone, Debug, Default)]
pub struct Factions(pub Vec<Faction>);

impl Factions {
    pub fn at(&self, seat: Coord) -> Option<&Faction> {
        self.0.iter().find(|f| f.seat == seat)
    }
}

// Faction-turn knobs ([`FactionConfig`]) live Bevy-free in the `config` crate;
// re-exported here and wrapped in an ECS-resource newtype.
pub use config::FactionConfig;

/// ECS-resource handle for the [`FactionConfig`] knobs. Derefs to the config.
#[derive(Resource, Clone, Copy, Debug)]
pub struct FactionRes(pub FactionConfig);

impl std::ops::Deref for FactionRes {
    type Target = FactionConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Approximate hex distance between two tiles, shortest way around the E–W wrap.
fn hex_dist(topo: &Topology, a: Coord, b: Coord) -> i32 {
    let w = topo.width();
    let dcol = (a.col - b.col).abs();
    let dcol = dcol.min(w - dcol);
    dcol + (a.row - b.row).abs()
}

/// The government a court of this kind keeps — might, money, or the vote.
fn government_for(court: &str) -> Government {
    match court {
        "guild" | "thieves_guild" | "merchant_consortium" => Government::Oligarchy,
        "temple" | "druid_circle" => Government::Democracy,
        _ => Government::Monarchy, // royal_court, barons_keep, and the rest
    }
}

/// The taboo a court of this kind lays on its members, if any. Law-abiding courts
/// forbid killing — the `avenge` act, `alive(foe) = 0`.
fn taboo_for(court: &str, reg: &Registry) -> Option<Law> {
    match court {
        "temple" | "druid_circle" | "royal_court" => reg.predicate_id("alive").map(|p| Law::Taboo(p, 0)),
        _ => None,
    }
}

/// Personality distance between two people (L1 over their trait vectors) — small means
/// alike. Drives a democracy's election: the most *representative* member wins.
fn trait_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum()
}

/// A member's data gathered for the turn (owned, so the compute is borrow-free).
#[derive(Clone)]
struct MemberInfo {
    entity: Entity,
    ambition: f32,
    money: i64,
    personality: Vec<f32>,
    grievance: bool,
    detained: bool,
}

/// Choose a faction's ruling body for its government: the most ambitious (monarchy),
/// the wealthiest-and-keenest council (oligarchy), or the most representative member —
/// least total personality-distance to the rest (democracy). The head is first.
fn elect(gov: Government, members: &[MemberInfo], council_size: usize) -> SmallVec<[Entity; 4]> {
    let mut out: SmallVec<[Entity; 4]> = SmallVec::new();
    if members.is_empty() {
        return out;
    }
    match gov {
        Government::Monarchy => {
            let k = (0..members.len())
                .max_by(|&a, &b| {
                    members[a]
                        .ambition
                        .partial_cmp(&members[b].ambition)
                        .unwrap()
                        .then(members[a].entity.cmp(&members[b].entity))
                })
                .unwrap();
            out.push(members[k].entity);
        }
        Government::Oligarchy => {
            let mut idx: Vec<usize> = (0..members.len()).collect();
            let worth = |m: &MemberInfo| m.money as f32 + m.ambition * 1000.0;
            idx.sort_by(|&a, &b| {
                worth(&members[b])
                    .partial_cmp(&worth(&members[a]))
                    .unwrap()
                    .then(members[a].entity.cmp(&members[b].entity))
            });
            for &i in idx.iter().take(council_size.max(1)) {
                out.push(members[i].entity);
            }
        }
        Government::Democracy => {
            let total_dist =
                |i: usize| -> f32 { members.iter().map(|m| trait_distance(&members[i].personality, &m.personality)).sum() };
            let rep = (0..members.len())
                .min_by(|&a, &b| {
                    total_dist(a).partial_cmp(&total_dist(b)).unwrap().then(members[a].entity.cmp(&members[b].entity))
                })
                .unwrap();
            out.push(members[rep].entity);
        }
    }
    out
}

/// The faction turn (every [`FactionConfig::period`] ticks): factions are governed,
/// taxed, legislated, fought, and policed. See the module docs for the shape.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn faction_turn(
    mut commands: Commands,
    substrate: Res<Substrate>,
    catalog: Option<Res<FeatureCatalog>>,
    features: Option<Res<Features>>,
    reg: Res<Registry>,
    config: Res<FactionRes>,
    mut factions: ResMut<Factions>,
    mut npcs: Query<
        (
            Entity,
            &Position,
            &mut Inventory,
            &Personality,
            &mut Allegiance,
            Option<&Grievance>,
            Option<&Detained>,
            &mut Opinion,
        ),
        With<Npc>,
    >,
    // Off-by-default Chronicle: hears the bloc-scale deeds — a war declared, a champion's new grudge,
    // a casualty of war or the headsman. `None` (layer off) => every tap below is a no-op.
    mut chronicle: Option<ResMut<crate::chronicle::Chronicle>>,
) {
    if config.period == 0 || !substrate.0.tick().is_multiple_of(config.period) {
        return;
    }
    let tick = substrate.0.tick();
    let topo = substrate.0.topology();
    let (Some(catalog), Some(features)) = (catalog, features) else { return };
    // Where everyone stands, so a war-death / execution / inherited grudge gets a place. Read-only.
    let pos_of: std::collections::HashMap<Entity, Coord> = npcs.iter().map(|q| (q.0, q.1.0)).collect();

    // Court seats and what each court kind implies.
    let mut seats: Vec<Coord> = Vec::new();
    let mut native_gov: Vec<Government> = Vec::new();
    let mut native_taboo: Vec<Option<Law>> = Vec::new();
    for i in topo.indices() {
        if let Some(f) = features.at_index(i).iter().find(|f| catalog.def(f.kind).category == Category::Court) {
            let name = catalog.name(f.kind);
            seats.push(topo.coord(i));
            native_gov.push(government_for(name));
            native_taboo.push(taboo_for(name, &reg));
        }
    }
    if seats.is_empty() {
        factions.0.clear();
        for (.., mut a, _, _, _) in &mut npcs {
            a.0.clear();
        }
        return;
    }

    let old: HashMap<Coord, Faction> = std::mem::take(&mut factions.0).into_iter().map(|f| (f.seat, f)).collect();
    let prev_force: HashMap<Coord, f32> = old.iter().map(|(&s, f)| (s, f.force)).collect();
    let max_force = old.values().map(|f| f.force).fold(1.0, f32::max);
    let ambition_id = reg.trait_id("ambition");
    let avenge = reg.predicate_id("alive").map(|p| (p, 0i64)); // the act a no-kill taboo forbids

    // --- 1. Tribute (member → leader, scaled by government) + loyalty drift. ---
    let mut money_delta: HashMap<Entity, i64> = HashMap::new();
    let mut loyalty: HashMap<(Entity, Coord), f32> = HashMap::new();
    for (e, _, inv, _, alleg, _, _, _) in &npcs {
        for bond in &alleg.0 {
            let mut loy = bond.loyalty;
            if let Some(f) = old.get(&bond.seat) {
                let head = f.head();
                let funds = inv.money + money_delta.get(&e).copied().unwrap_or(0);
                if head != Some(e) && config.tax_rate > 0.0 && funds > 0 {
                    let rate = config.tax_rate * f.government.tribute_mult();
                    let tax = (rate * funds as f32).floor() as i64;
                    if tax > 0 {
                        *money_delta.entry(e).or_default() -= tax;
                        if let Some(h) = head {
                            *money_delta.entry(h).or_default() += tax;
                        }
                        loy -= config.tax_pain * rate;
                    }
                }
                loy += config.strength_pride * (f.force / max_force);
            }
            loy += config.loyalty_decay * (config.loyalty_base - loy);
            loyalty.insert((e, bond.seat), loy.clamp(0.0, 1.0));
        }
    }

    // --- 2. Reassign membership (multi + exclusivity), gathering member data. ---
    let mut members: Vec<Vec<MemberInfo>> = vec![Vec::new(); seats.len()];
    let mut new_bonds: HashMap<Entity, SmallVec<[Bond; 4]>> = HashMap::new();
    for (e, pos, inv, pers, alleg, grievance, detained, opinion) in &npcs {
        let ambition = ambition_id.and_then(|a| pers.0.get(a).copied()).unwrap_or(0.0);
        let money = inv.money + money_delta.get(&e).copied().unwrap_or(0);
        let mut ranked: Vec<(usize, f32)> = Vec::new();
        for (si, &seat) in seats.iter().enumerate() {
            let d = hex_dist(topo, pos.0, seat);
            if d > config.reach {
                continue;
            }
            let mut pull = (1.0 + prev_force.get(&seat).copied().unwrap_or(0.0)) / (1.0 + d as f32);
            if alleg.belongs_to(seat) {
                let loy = loyalty.get(&(e, seat)).copied().unwrap_or(config.loyalty_base);
                pull *= 0.5 + config.loyalty_inertia * loy;
            }
            // Drawn toward a court led by someone it respects, repelled from one it loathes.
            if let Some(head) = old.get(&seat).and_then(|f| f.head()) {
                pull *= (1.0 + config.opinion_weight * opinion.of(head)).max(0.0);
            }
            ranked.push((si, pull));
        }
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(seats[a.0].col.cmp(&seats[b.0].col)));
        let mut chosen: SmallVec<[usize; 4]> = SmallVec::new();
        for &(si, _) in &ranked {
            if chosen.len() >= config.max_factions {
                break;
            }
            let seat = seats[si];
            let conflict = chosen.iter().any(|&cj| {
                let cs = seats[cj];
                old.get(&seat).is_some_and(|f| f.excludes(cs)) || old.get(&cs).is_some_and(|f| f.excludes(seat))
            });
            if !conflict {
                chosen.push(si);
            }
        }
        let mut bonds: SmallVec<[Bond; 4]> = SmallVec::new();
        for &si in &chosen {
            let seat = seats[si];
            let loy = if alleg.belongs_to(seat) {
                loyalty.get(&(e, seat)).copied().unwrap_or(config.loyalty_base)
            } else {
                config.loyalty_base
            };
            bonds.push(Bond { seat, loyalty: loy });
            members[si].push(MemberInfo {
                entity: e,
                ambition,
                money,
                personality: pers.0.clone(),
                grievance: grievance.is_some(),
                detained: detained.is_some(),
            });
        }
        new_bonds.insert(e, bonds);
    }

    // --- 3. Build factions: elect leaders, carry/adopt laws and wars. ---
    let mut built: Vec<Faction> = Vec::new();
    for (si, group) in members.iter().enumerate() {
        if group.len() < config.min_members {
            continue;
        }
        let seat = seats[si];
        let gov = old.get(&seat).map(|f| f.government).unwrap_or(native_gov[si]);
        let mut laws: SmallVec<[Law; 4]> = old.get(&seat).map(|f| f.laws.clone()).unwrap_or_default();
        if let Some(t) = native_taboo[si]
            && !laws.contains(&t)
        {
            laws.push(t);
        }
        built.push(Faction {
            seat,
            government: gov,
            leaders: elect(gov, group, config.council_size),
            members: group.iter().map(|m| m.entity).collect(),
            laws,
            at_war: old.get(&seat).map(|f| f.at_war.clone()).unwrap_or_default(),
            force: group.len() as f32,
            cunning: group.iter().map(|m| m.ambition).sum(),
            wealth: group.iter().map(|m| m.money).sum(),
        });
    }

    // --- 4. War: strong neighbours fall out; the stronger inflicts a casualty. ---
    let pos_of_seat = |s: Coord| seats.iter().position(|&x| x == s).unwrap();
    let mut casualties: Vec<Entity> = Vec::new();
    for a in 0..built.len() {
        for b in (a + 1)..built.len() {
            let (sa, sb) = (built[a].seat, built[b].seat);
            if hex_dist(topo, sa, sb) > config.reach {
                continue;
            }
            let (fa, fb) = (built[a].force, built[b].force);
            let rivals = fa.max(fb) >= config.war_force_ratio * fa.min(fb).max(1.0);
            if !(rivals || built[a].at_war.contains(&sb)) {
                continue;
            }
            // The tick this rivalry becomes a war (a's roll doesn't yet list b's seat) — recorded
            // once for the pair, cast as the two seats' heads, placed at a's seat.
            if !built[a].at_war.contains(&sb)
                && let Some(c) = chronicle.as_deref_mut()
            {
                c.record(tick, EpisodeKind::WarDeclared, [built[a].head(), built[b].head(), None], sa, None, 0);
            }
            for (x, y) in [(a, b), (b, a)] {
                let yseat = built[y].seat;
                if !built[x].at_war.contains(&yseat) {
                    built[x].at_war.push(yseat);
                }
                let ex = Law::Exclude(yseat);
                if !built[x].laws.contains(&ex) {
                    built[x].laws.push(ex);
                }
            }
            let weak = if fa >= fb { b } else { a };
            if let Some(victim) = members[pos_of_seat(built[weak].seat)]
                .iter()
                .filter(|m| !built[weak].leaders.contains(&m.entity))
                .min_by(|x, y| x.ambition.partial_cmp(&y.ambition).unwrap().then(x.entity.cmp(&y.entity)))
                .map(|m| m.entity)
            {
                casualties.push(victim);
            }
        }
    }

    // --- 5. Enforcement: a no-kill faction detains members who hold a grudge, and a
    //        strong one executes a repeat offender already in its cells. ---
    let mut detain: Vec<Entity> = Vec::new();
    let mut executed: Vec<Entity> = Vec::new();
    if let Some(act) = avenge {
        for f in built.iter().filter(|f| f.forbids(act)) {
            for m in &members[pos_of_seat(f.seat)] {
                if m.grievance && !f.leaders.contains(&m.entity) {
                    if m.detained && f.force >= config.execute_force {
                        executed.push(m.entity);
                    } else {
                        detain.push(m.entity);
                    }
                }
            }
        }
    }

    // --- 6. Command: each faction at war dispatches its keenest loyal member as a
    //        champion against the enemy's head — a directive carried out through the
    //        ordinary grudge/avenge machinery. (If the champion's own faction forbids
    //        killing, its enforcers jail it next turn — a pacifist bloc fights poorly.)
    let mut champions: Vec<(Entity, Entity)> = Vec::new();
    for f in &built {
        for &enemy_seat in &f.at_war {
            let Some(enemy_head) = built.iter().find(|g| g.seat == enemy_seat).and_then(|g| g.head()) else {
                continue;
            };
            let group = &members[pos_of_seat(f.seat)];
            let champ = group
                .iter()
                .filter(|m| !f.leaders.contains(&m.entity) && !m.grievance)
                .max_by(|a, b| {
                    let zeal = |m: &MemberInfo| loyalty.get(&(m.entity, f.seat)).copied().unwrap_or(0.0) + m.ambition;
                    zeal(a).partial_cmp(&zeal(b)).unwrap().then(a.entity.cmp(&b.entity))
                })
                .map(|m| m.entity);
            if let Some(champ) = champ {
                champions.push((champ, enemy_head));
            }
        }
    }

    // --- Apply: money; then allegiance + opinion together; then commands. ---
    for (e, _, mut inv, _, _, _, _, _) in &mut npcs {
        if let Some(&d) = money_delta.get(&e) {
            inv.money += d;
        }
    }
    let live: std::collections::HashSet<Coord> = built.iter().map(|f| f.seat).collect();
    let head_at = |seat: Coord| built.iter().find(|f| f.seat == seat).and_then(|f| f.head());
    for (e, _, _, _, mut a, _, _, mut opinion) in &mut npcs {
        let bonds = new_bonds.get(&e).cloned().unwrap_or_default();
        // Opinion: warm toward the head of each bloc it serves (toward how it feels —
        // its loyalty); sour toward the heads it is set against in war; then fade all.
        for bond in &bonds {
            let Some(f) = built.iter().find(|f| f.seat == bond.seat) else { continue };
            if let Some(head) = f.head()
                && head != e
            {
                let target = (bond.loyalty - config.loyalty_base) * 2.0;
                let cur = opinion.of(head);
                opinion.0.insert(head, (cur + config.opinion_gain * (target - cur)).clamp(-1.0, 1.0));
            }
            for &enemy_seat in &f.at_war {
                if let Some(eh) = head_at(enemy_seat)
                    && eh != e
                {
                    let cur = opinion.of(eh);
                    opinion.0.insert(eh, (cur - config.war_enmity).clamp(-1.0, 1.0));
                }
            }
        }
        opinion.0.retain(|_, v| {
            *v *= 1.0 - config.opinion_decay;
            v.abs() > 0.02
        });
        // Allegiance: keep the bonds whose faction actually formed.
        let mut bonds = bonds;
        bonds.retain(|b| live.contains(&b.seat));
        a.0 = bonds;
    }
    for (champ, enemy) in champions {
        commands.entity(champ).insert(Grievance(enemy));
        if let Some(c) = chronicle.as_deref_mut()
            && let Some(&at) = pos_of.get(&champ)
        {
            c.record(tick, EpisodeKind::GrievanceFormed, [Some(champ), Some(enemy), None], at, None, 0);
        }
    }
    for e in detain {
        commands.entity(e).insert(Detained { ticks: config.detain_ticks });
    }
    for e in casualties.into_iter().chain(executed) {
        // A death by war or the headsman — `parties[0]` the dead, placed where they stood.
        if let Some(c) = chronicle.as_deref_mut()
            && let Some(&at) = pos_of.get(&e)
        {
            c.record(tick, EpisodeKind::Death, [Some(e), None, None], at, None, 0);
        }
        commands.entity(e).despawn();
    }
    factions.0 = built;
}

/// Tick down detentions; release anyone whose term is up.
pub(crate) fn detention_countdown(mut commands: Commands, mut held: Query<(Entity, &mut Detained)>) {
    for (e, mut d) in &mut held {
        if d.ticks <= 1 {
            commands.entity(e).remove::<Detained>();
        } else {
            d.ticks -= 1;
        }
    }
}
