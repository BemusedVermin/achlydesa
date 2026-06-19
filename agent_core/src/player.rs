//! The **player avatar** — exploration (`docs/dialogue.md` lineage: the player is an
//! ordinary body in the world, not a god above it).
//!
//! For now this is *exploration only*: a controllable avatar that walks the land, reveals
//! the map as it goes (fog of war), and discovers the hidden features it stumbles on —
//! the same discovery the NPCs get by visiting. It is a real ECS entity (so the social
//! and combat layers can later see it like any other body), but it is **not** an [`Npc`],
//! so every AI system (planning, dialogue, the director) skips it for free — the human is
//! its planner.
//!
//! Off by default: the [`PlayerState`] resource exists with no avatar until
//! [`Simulation::spawn_player`](crate::Simulation::spawn_player) is called, and
//! [`player_travel`] early-returns until then — so a world with no player is byte-identical.

use crate::features::{Discovery, FeatureCatalog, FeatureId, Features, FindState};
use crate::people::{Known, MoveGraph, Npc};
use crate::{Position, Substrate};
use bevy_ecs::prelude::*;
use game_sim::{Coord, Topology, World as GameWorld};
use std::collections::{HashMap, HashSet, VecDeque};

/// Marks the avatar the human controls. Not an [`Npc`], so the AI never touches it.
#[derive(Component, Clone, Copy, Debug)]
pub struct Player;

/// **What the player knows.** The heart of knowledge-gated discovery: a growing set of *lore*
/// facts (names, passwords — the keys that open gates) and *rumours* (places heard-of but not
/// yet pinpointed). Lives as a resource alongside [`PlayerState`]; empty and inert until an
/// avatar is spawned and goes looking, so a world with no player stays byte-identical.
#[derive(Resource, Default)]
pub struct PlayerKnowledge {
    /// Lore tokens held. A feature is found/entered only once its [`requires`](crate::FeatureDef::requires)
    /// are all in here.
    pub lore: HashSet<String>,
    /// Places the player has heard of but not located — journal hints, not yet on the map.
    pub rumors: Vec<Rumor>,
}

/// A place the player has heard tell of but cannot yet point to on the map.
#[derive(Clone, Debug)]
pub struct Rumor {
    /// A short line naming what was heard of (the feature kind, prettified by the view layer).
    pub subject: String,
    /// The tile it refers to, if the source knew it — locating it later resolves the rumour.
    pub target: Option<Coord>,
}

/// What a player's `search` turned up: the names of features newly found, and the lore those
/// places taught — for the front-end to announce and log.
#[derive(Default, Clone, Debug)]
pub struct SearchOutcome {
    pub found: Vec<String>,
    pub lore_gained: Vec<String>,
}

impl SearchOutcome {
    pub fn is_empty(&self) -> bool {
        self.found.is_empty()
    }
}

/// The avatar's travel orders and the map it has uncovered. One player; lives as a
/// resource (no singleton component), absent-avatar by default.
#[derive(Resource, Default)]
pub struct PlayerState {
    avatar: Option<Entity>,
    /// The remaining hexes of the current route, in order (excludes the current tile).
    path: VecDeque<Coord>,
    destination: Option<Coord>,
    /// Tiles the player has laid eyes on — the revealed map (topology storage indices).
    explored: HashSet<usize>,
    /// How many hexes the player sees around itself, and how many it walks per tick.
    sight: i32,
    speed: u32,
    /// Set by the assembler when the avatar's Notice is keen enough: it then passively spots
    /// (lore-met) Secret features as it travels, instead of having to stop and actively search.
    perceptive: bool,
}

impl PlayerState {
    fn fresh() -> Self {
        Self {
            avatar: None,
            path: VecDeque::new(),
            destination: None,
            explored: HashSet::new(),
            sight: 3,
            speed: 1,
            perceptive: false,
        }
    }
    /// The avatar entity, if one has been spawned.
    pub fn avatar(&self) -> Option<Entity> {
        self.avatar
    }
    /// Is the avatar currently walking a route?
    pub fn traveling(&self) -> bool {
        !self.path.is_empty()
    }
    /// Where it is headed, if anywhere.
    pub fn destination(&self) -> Option<Coord> {
        self.destination
    }
    /// How many tiles have been revealed.
    pub fn explored_count(&self) -> usize {
        self.explored.len()
    }
    /// Whether a tile has been seen.
    pub fn is_explored(&self, topo: &Topology, c: Coord) -> bool {
        self.explored.contains(&topo.index_of(c))
    }
    /// Every revealed tile, in a deterministic order (for a renderer to draw the map).
    pub fn explored_tiles(&self, topo: &Topology) -> Vec<Coord> {
        let mut v: Vec<Coord> = self.explored.iter().map(|&i| topo.coord(i)).collect();
        v.sort_by_key(|c| (c.row, c.col));
        v
    }
    pub fn set_path(&mut self, to: Coord, path: VecDeque<Coord>) {
        self.destination = Some(to);
        self.path = path;
    }
    pub fn halt(&mut self) {
        self.path.clear();
        self.destination = None;
    }
    /// How far the avatar reveals the map each step (sight radius, ≥ 1). The assembler sets this
    /// from the avatar's Notice skill when the RPG layer is on — a keener scout sees further.
    pub fn set_sight(&mut self, sight: i32) {
        self.sight = sight.max(1);
    }
    /// The avatar's current sight radius.
    pub fn sight(&self) -> i32 {
        self.sight
    }
    /// Whether the avatar passively spots lore-met Secret features as it travels (set from a keen
    /// Notice skill). Off by default — it must actively `search` to uncover them.
    pub fn perceptive(&self) -> bool {
        self.perceptive
    }
    pub fn set_perceptive(&mut self, on: bool) {
        self.perceptive = on;
    }
}

/// The broad lie of the land at a tile — relief banded by elevation above sea level.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Terrain {
    Ocean,
    Coast,
    Lowland,
    Highland,
    Mountain,
}

impl Terrain {
    /// Classify a tile's relief from its elevation above sea level.
    pub fn of(elevation: f32, sea: f32) -> Terrain {
        match elevation - sea {
            h if h < 0.0 => Terrain::Ocean,
            h if h < 50.0 => Terrain::Coast,
            h if h < 500.0 => Terrain::Lowland,
            h if h < 1500.0 => Terrain::Highland,
            _ => Terrain::Mountain,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Terrain::Ocean => "ocean",
            Terrain::Coast => "coast",
            Terrain::Lowland => "lowland",
            Terrain::Highland => "highland",
            Terrain::Mountain => "mountain",
        }
    }
}

/// What the player perceives at one tile.
#[derive(Clone, Debug)]
pub struct TileInfo {
    pub coord: Coord,
    pub terrain: Terrain,
    pub elevation: f32,
    /// Carrying capacity (`0..1`-ish) — how fertile/green the ground is.
    pub fertility: f32,
    pub vegetation: f32,
    pub surface_water: f32,
    /// The features the player can make out here — landmarks always, hidden ones only
    /// once it has visited the tile.
    pub features: Vec<String>,
}

/// A snapshot of what the player sees right now — the "look" verb.
#[derive(Clone, Debug)]
pub struct PlayerView {
    pub pos: Coord,
    pub here: TileInfo,
    /// The tiles in sight (excluding the one underfoot), nearest-first.
    pub surroundings: Vec<TileInfo>,
    /// Other bodies within sight — `(entity, where)`.
    pub nearby: Vec<(Entity, Coord)>,
    /// How much of the whole map has been revealed so far.
    pub explored: usize,
}

/// The hexes within `radius` of `centre`, nearest-first (BFS), including the centre.
fn ring(topo: &Topology, centre: Coord, radius: i32) -> Vec<Coord> {
    let start = topo.index_of(centre);
    let mut seen: HashSet<usize> = HashSet::from([start]);
    let mut order = vec![centre];
    let mut frontier = vec![start];
    for _ in 0..radius.max(0) {
        let mut next = Vec::new();
        for &i in &frontier {
            for l in topo.neighbors(i) {
                if seen.insert(l.to) {
                    order.push(topo.coord(l.to));
                    next.push(l.to);
                }
            }
        }
        frontier = next;
    }
    order
}

/// Breadth-first route over the **land** movement graph from `from` to `to` — the steps to
/// walk, in order (empty if already there). `None` if `to` is unreachable (e.g. across
/// water): the avatar is a body, and bodies do not walk on the sea. Deterministic (BFS in
/// the graph's fixed neighbour order).
pub fn path_to(mg: &MoveGraph, topo: &Topology, from: Coord, to: Coord) -> Option<VecDeque<Coord>> {
    if from == to {
        return Some(VecDeque::new());
    }
    let (start, goal) = (topo.index_of(from), topo.index_of(to));
    let mut prev: HashMap<usize, usize> = HashMap::new();
    let mut visited: HashSet<usize> = HashSet::from([start]);
    let mut queue: VecDeque<usize> = VecDeque::from([start]);
    while let Some(u) = queue.pop_front() {
        for &nc in mg.neighbors(u) {
            let v = topo.index_of(nc);
            if visited.insert(v) {
                prev.insert(v, u);
                if v == goal {
                    let mut path = VecDeque::new();
                    let mut cur = goal;
                    while cur != start {
                        path.push_front(topo.coord(cur));
                        cur = prev[&cur];
                    }
                    return Some(path);
                }
                queue.push_back(v);
            }
        }
    }
    None
}

/// Reveal the map around `at` (sight rings) into the explored set, discover this tile's
/// features into `known` (and the shared map). The per-tile work both the spawn and the
/// travel system do.
fn uncover(world: &mut World, avatar: Entity, at: Coord) {
    let (here, ring_idx, _sight) = {
        let topo = world.resource::<Substrate>().0.topology();
        let sight = world.resource::<PlayerState>().sight;
        let here = topo.index_of(at);
        let ring_idx: Vec<usize> = ring(topo, at, sight).iter().map(|c| topo.index_of(*c)).collect();
        (here, ring_idx, sight)
    };
    if let Some(mut k) = world.get_mut::<Known>(avatar) {
        k.0.insert(here);
    }
    world.resource_scope::<Features, ()>(|w, mut feat| {
        if let Some(cat) = w.get_resource::<FeatureCatalog>() {
            feat.discover_at_index(cat, here, Discovery::Hidden);
        }
    });
    // A perceptive avatar (keen Notice) also spots the Secrets it already knows to look for as it
    // passes — the same lore-gated reveal an active search does, so it never bypasses a lore gate.
    // Off by default, so a world without the RPG layer reveals exactly what it did before.
    if world.resource::<PlayerState>().perceptive {
        let lore = world.resource::<PlayerKnowledge>().lore.clone();
        let found: Vec<FeatureId> = world.resource_scope::<Features, Vec<FeatureId>>(|w, mut feat| {
            match w.get_resource::<FeatureCatalog>() {
                Some(cat) => feat.search_at_index(cat, here, &lore),
                None => Vec::new(),
            }
        });
        if !found.is_empty() {
            let reveals: Vec<String> = {
                let cat = world.resource::<FeatureCatalog>();
                found.iter().flat_map(|k| cat.def(*k).reveals.iter().cloned()).collect()
            };
            let mut kn = world.resource_mut::<PlayerKnowledge>();
            for r in reveals {
                kn.lore.insert(r);
            }
        }
    }
    world.resource_mut::<PlayerState>().explored.extend(ring_idx);
}

/// Place the avatar at `at` and reveal its surroundings. Replaces any prior avatar.
pub fn spawn(world: &mut World, at: Coord) -> Entity {
    let e = world.spawn((Player, Position(at), Known::default())).id();
    {
        let mut st = world.resource_mut::<PlayerState>();
        *st = PlayerState::fresh();
        st.avatar = Some(e);
    }
    uncover(world, e, at);
    e
}

/// Build the "look" view — read-only over the world (takes `&mut World` only to run the
/// nearby-bodies query, as the other inspection accessors do).
pub fn view(world: &mut World) -> Option<PlayerView> {
    let (avatar, sight) = {
        let st = world.resource::<PlayerState>();
        (st.avatar?, st.sight)
    };
    let pos = world.get::<Position>(avatar)?.0;

    // Nearby bodies (the query needs &mut World; collect, then read resources).
    let sight_tiles: Vec<Coord> = {
        let topo = world.resource::<Substrate>().0.topology();
        ring(topo, pos, sight)
    };
    let sight_set: HashSet<usize> = {
        let topo = world.resource::<Substrate>().0.topology();
        sight_tiles.iter().map(|c| topo.index_of(*c)).collect()
    };
    let all_npcs: Vec<(Entity, Coord)> = {
        let mut q = world.query_filtered::<(Entity, &Position), With<Npc>>();
        q.iter(world).map(|(e, p)| (e, p.0)).collect()
    };
    let nearby: Vec<(Entity, Coord)> = {
        let topo = world.resource::<Substrate>().0.topology();
        all_npcs.into_iter().filter(|(_, c)| sight_set.contains(&topo.index_of(*c))).collect()
    };

    let gw = &world.resource::<Substrate>().0;
    let features = world.get_resource::<Features>();
    let catalog = world.get_resource::<FeatureCatalog>();
    let known = world.get::<Known>(avatar);
    let info = |c: Coord| tile_info(gw, features, catalog, known, c);
    let here = info(pos);
    let surroundings: Vec<TileInfo> = sight_tiles.iter().filter(|&&c| c != pos).map(|&c| info(c)).collect();
    let explored = world.resource::<PlayerState>().explored.len();

    Some(PlayerView { pos, here, surroundings, nearby, explored })
}

fn tile_info(
    gw: &GameWorld,
    features: Option<&Features>,
    catalog: Option<&FeatureCatalog>,
    known: Option<&Known>,
    c: Coord,
) -> TileInfo {
    let sea = gw.params().sea_level;
    let i = gw.topology().index_of(c);
    let visible: Vec<String> = match (features, catalog) {
        (Some(f), Some(cat)) => f
            .at_index(i)
            .iter()
            // A landmark is seen by anyone; a hidden/secret place only once visited.
            .filter(|ft| cat.def(ft.kind).discovery == Discovery::Landmark || known.is_some_and(|k| k.0.contains(&i)))
            .map(|ft| cat.def(ft.kind).name.clone())
            .collect(),
        _ => Vec::new(),
    };
    TileInfo {
        coord: c,
        terrain: Terrain::of(gw.elevation(c), sea),
        elevation: gw.elevation(c),
        fertility: gw.carrying_capacity(c),
        vegetation: gw.plant_biomass(c),
        surface_water: gw.surface_water(c),
        features: visible,
    }
}

/// Each tick, walk the avatar one (or `speed`) hexes along its route, revealing the map
/// and discovering features as it goes. Early-returns until an avatar exists and has a
/// route — so a world with no player is unchanged.
pub(crate) fn player_travel(
    mut state: ResMut<PlayerState>,
    substrate: Res<Substrate>,
    catalog: Option<Res<FeatureCatalog>>,
    mut features: Option<ResMut<Features>>,
    mut avatars: Query<(&mut Position, &mut Known), With<Player>>,
    // Party members travel as a stack: snap them to the avatar's tile each step it walks.
    // Empty when the party layer is off, so a partyless world is byte-identical.
    mut followers: Query<&mut Position, (With<crate::Follower>, Without<Player>)>,
) {
    let state = state.as_mut();
    let Some(avatar) = state.avatar else { return };
    if state.path.is_empty() {
        return;
    }
    let Ok((mut pos, mut known)) = avatars.get_mut(avatar) else { return };
    let topo = substrate.0.topology();
    let (sight, speed) = (state.sight, state.speed.max(1));
    for _ in 0..speed {
        let Some(next) = state.path.pop_front() else { break };
        pos.0 = next;
        let i = topo.index_of(next);
        known.0.insert(i);
        if let (Some(cat), Some(feat)) = (catalog.as_deref(), features.as_deref_mut()) {
            feat.discover_at_index(cat, i, Discovery::Hidden);
        }
        for c in ring(topo, next, sight) {
            state.explored.insert(topo.index_of(c));
        }
        if state.path.is_empty() {
            state.destination = None;
            break;
        }
    }
    let here = pos.0;
    for mut fpos in &mut followers {
        fpos.0 = here;
    }
}

/// **Search where the avatar stands.** Reveal every undiscovered feature here whose knowledge
/// gate the player satisfies, harvest the lore those places teach, and mark the tile visited.
/// Deterministic — knowledge, not luck, decides what is found. Returns what turned up (empty if
/// there is no avatar, or nothing the player can yet find here).
pub fn search(world: &mut World) -> SearchOutcome {
    let mut out = SearchOutcome::default();
    let Some(avatar) = world.resource::<PlayerState>().avatar else { return out };
    let Some(at) = world.get::<Position>(avatar).map(|p| p.0) else { return out };
    let i = world.resource::<Substrate>().0.topology().index_of(at);
    let lore = world.resource::<PlayerKnowledge>().lore.clone();

    let found: Vec<FeatureId> = world.resource_scope::<Features, _>(|w, mut feat| {
        let cat = w.resource::<FeatureCatalog>();
        feat.search_at_index(cat, i, &lore)
    });
    if found.is_empty() {
        return out;
    }
    if let Some(mut k) = world.get_mut::<Known>(avatar) {
        k.0.insert(i); // a searched tile is a visited tile
    }
    // Collect names + taught lore while borrowing the catalog, then fold into knowledge.
    let reveals: Vec<String> = {
        let cat = world.resource::<FeatureCatalog>();
        for kind in &found {
            out.found.push(cat.def(*kind).name.clone());
        }
        found.iter().flat_map(|k| cat.def(*k).reveals.iter().cloned()).collect()
    };
    let mut kn = world.resource_mut::<PlayerKnowledge>();
    for r in reveals {
        if kn.lore.insert(r.clone()) {
            out.lore_gained.push(r);
        }
    }
    out
}

/// Whether searching the avatar's current tile would find anything, given the lore it holds.
pub fn find_state(world: &World) -> FindState {
    let Some(avatar) = world.resource::<PlayerState>().avatar else { return FindState::Nothing };
    let Some(at) = world.get::<Position>(avatar).map(|p| p.0) else { return FindState::Nothing };
    let i = world.resource::<Substrate>().0.topology().index_of(at);
    let (Some(feat), Some(cat)) = (world.get_resource::<Features>(), world.get_resource::<FeatureCatalog>()) else {
        return FindState::Nothing;
    };
    feat.find_state_at_index(cat, i, &world.resource::<PlayerKnowledge>().lore)
}
