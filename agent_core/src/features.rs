//! **Tile features** — the points of interest layered onto the substrate that
//! make the world worth exploring, for players and NPCs alike. After *Worlds
//! Without Number*'s four categories — **Community** (settlements), **Court**
//! (powers & factions), **Ruin** (adventure sites), and **Wilderness** (wonders,
//! lairs, hazards) — a single hex can carry one of each, so a tile may host a
//! city, the royal court that rules from it, the catacombs beneath it, and a
//! wonder nearby. "Multiple landmarks on one hex," exactly as the design intends.
//!
//! Everything here is **authored data** (`assets/data/features.ron`):
//! the catalog of feature kinds, each with a category, a [`Discovery`] tier, and a
//! **suitability** — a list of [`Term`]s that read tile [`Signal`]s through the
//! same response [`Curve`](crate::ai::Curve) the agent utility scorer uses. Add a
//! kind to the RON and it is placed; no Rust changes.
//!
//! ## Placement (`place`)
//! Deterministic and seeded, in two passes so relational features resolve:
//! 1. **Communities** — scored on static tile signals, with an **inhibition
//!    radius** ([`FeatureConfig::community_spacing`]) so settlements space out
//!    instead of clumping into the one best basin (the central-place insight; the
//!    independent-draw alternative is the classic biome-clumping failure).
//! 2. **Courts, ruins, wilderness** — scored with [`Signal::Remoteness`] now
//!    available (hex distance to the nearest community) and with `host`
//!    constraints honoured (a royal court only inside a city).
//!
//! A feature's [`Discovery`] tier sets whether it is known on placement: a
//! **Landmark** is seen automatically; **Hidden** and **Secret** start latent and
//! are revealed through play (an NPC searching a tile finds the Hidden ones — see
//! [`people::discover_features`](crate::people::discover_features)).

use crate::ai::Curve;
use bevy_ecs::prelude::Resource;
use game_sim::fields::Formation;
use config::{Asset, Config};
use game_sim::{Coord, SplitMix64, Topology, World as GameWorld};
use serde::Deserialize;
use sim::Rng;
use std::collections::{HashSet, VecDeque};

/// Index of a feature kind in the [`FeatureCatalog`].
pub type FeatureId = usize;

/// *Worlds Without Number*'s four point-of-interest categories. A hex carries at
/// most one feature per category, so up to four can share a tile.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Category {
    Community,
    Court,
    Ruin,
    Wilderness,
}

impl Category {
    pub const ALL: [Category; 4] = [Category::Community, Category::Court, Category::Ruin, Category::Wilderness];
    pub const COUNT: usize = 4;
    pub fn idx(self) -> usize {
        self as usize
    }
}

/// What a player's search of a tile would turn up, given the lore they hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FindState {
    /// Nothing left to find here.
    Nothing,
    /// Something undiscovered is here and the player's knowledge is enough to find it.
    Findable,
    /// Something is here, but the player lacks the lore the gate demands — the lure.
    Locked,
}

/// How a feature reveals itself when an agent enters its hex.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Discovery {
    /// Seen automatically — the obvious structure or terrain.
    Landmark,
    /// Found by spending a turn searching (a lair, a buried entrance).
    Hidden,
    /// Needs luck, insight, or a skill check (a vault, the power behind a court).
    Secret,
}

/// A tile quality a suitability [`Term`] can read. Values are in documented
/// natural units; the authored [`Curve`] maps each to a `0..1` score.
///
/// Ranges: `Elevation` = metres above sea; `Slope` = steepest metre drop to a
/// neighbour; `Temperature` = °C; `SoilNutrients`/`Minerals` are raw substrate
/// levels; `Fertility`/`Vegetation` are `carrying_capacity`/`plant_biomass`
/// normalised to `0..1`; `Coast`/`Forest`/`Grass` are `0` or `1`; `Aridity` =
/// `1 − Fertility`; `Remoteness` is hex distance to the nearest community,
/// normalised by [`FeatureConfig::remoteness_scale`] (only meaningful in the
/// second placement pass — communities are placed before it is known).
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signal {
    Elevation,
    Slope,
    Temperature,
    SoilNutrients,
    Fertility,
    Vegetation,
    Minerals,
    SurfaceWater,
    Coast,
    Forest,
    Grass,
    Aridity,
    Remoteness,
}

impl Signal {
    pub const COUNT: usize = 13;
    fn idx(self) -> usize {
        self as usize
    }
}

/// One axis of a feature's suitability: read a tile [`Signal`], shape it through a
/// [`Curve`]. A `Linear(m: 1, b: 0)` on a `0/1` signal is a *requirement*; a
/// `Linear(m: 0.4, b: 0.6)` is a *bonus*.
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Term {
    pub signal: Signal,
    pub curve: Curve,
}

/// A need an [`AffordanceDef`] can restore (mirrors the planner's need meters).
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum NeedKind {
    Sustenance,
    Rest,
}

/// What an affordance does, as authored (goods/skills named, resolved against the
/// registry at build time). `Relieve` restores a need; `Yield` gathers a good,
/// optionally gated by a calling.
#[derive(Deserialize, Clone, Debug)]
pub enum EffectDef {
    Relieve { need: NeedKind, amount: i32 },
    Yield { good: String, units: u32, #[serde(default)] skill: Option<String> },
    /// Teach a calling — a guild lifts a skill the user lacks above zero.
    Teach { skill: String },
}

/// A **smart-object affordance** a feature advertises: a named action and what it
/// does for an agent standing on the tile. A depletable site (`capacity > 0`) is
/// worked down by use and recovers `regen` uses per tick; `capacity == 0` is an
/// inexhaustible service (a temple's rest never runs out).
#[derive(Deserialize, Clone, Debug)]
pub struct AffordanceDef {
    pub action: String,
    pub effect: EffectDef,
    #[serde(default)]
    pub capacity: u32,
    #[serde(default)]
    pub regen: f32,
}

/// A feature kind as authored in `features.ron`.
#[derive(Deserialize, Clone, Debug)]
pub struct FeatureDef {
    pub name: String,
    pub category: Category,
    pub discovery: Discovery,
    /// Base placement rate on a perfectly-suited tile, before the per-category
    /// knob in [`FeatureConfig`].
    pub density: f32,
    /// "Favoured by": the suitability terms, combined as a compensated product.
    pub favoured: Vec<Term>,
    /// If set, this feature only appears co-located with a feature of this
    /// category (a royal court inside a city, catacombs under a town).
    #[serde(default)]
    pub host: Option<Category>,
    /// The actions this feature advertises to nearby agents (a smart object). Empty
    /// for pure scenery.
    #[serde(default)]
    pub affordances: Vec<AffordanceDef>,
    /// **Knowledge gate.** Lore facts the player must already hold before this feature can be
    /// found (or entered) — the seven passwords before the gate of the seven, an Archon's name
    /// before its throne. Empty = no gate (the default, so existing content is unaffected).
    #[serde(default)]
    pub requires: Vec<String>,
    /// **Knowledge granted.** Lore facts the player gains by discovering this feature — gnosis
    /// spreading from the place that holds it. Empty = it teaches nothing.
    #[serde(default)]
    pub reveals: Vec<String>,
}

/// A placed feature: which kind, whether it has been discovered yet, and whether it has
/// been ruined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feature {
    pub kind: FeatureId,
    pub discovered: bool,
    /// Desecrated by a [`Defile`](crate::beats::Effect::Defile) beat — the dark twin of
    /// discovery. A defiled marvel stays on the map (still `discovered`) but its wonder is
    /// spoilt; the surface/art layer may show it scarred. Defaults `false`.
    pub defiled: bool,
}

/// The placed features of a world, one list per tile (indexed by topology storage
/// index, parallel to the substrate's fields). A resource on the ECS world.
#[derive(Resource, Clone, Debug, Default)]
pub struct Features {
    tiles: Vec<Vec<Feature>>,
}

impl Features {
    /// The features on tile `i` (topology storage index).
    pub fn at_index(&self, i: usize) -> &[Feature] {
        self.tiles.get(i).map_or(&[], Vec::as_slice)
    }

    /// The features on the hex at `c`.
    pub fn at(&self, topo: &Topology, c: Coord) -> &[Feature] {
        self.at_index(topo.index_of(c))
    }

    /// Every placed feature with its tile index.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &Feature)> {
        self.tiles.iter().enumerate().flat_map(|(i, fs)| fs.iter().map(move |f| (i, f)))
    }

    /// Total features placed across the world.
    pub fn total(&self) -> usize {
        self.tiles.iter().map(Vec::len).sum()
    }

    /// How many placed features belong to `category`.
    pub fn count_of(&self, catalog: &FeatureCatalog, category: Category) -> usize {
        self.iter().filter(|(_, f)| catalog.def(f.kind).category == category).count()
    }

    /// The hexes carrying a feature of `category`, in storage order (deterministic).
    pub fn tiles_of(&self, catalog: &FeatureCatalog, category: Category, topo: &Topology) -> Vec<Coord> {
        self.tiles
            .iter()
            .enumerate()
            .filter(|(_, fs)| fs.iter().any(|f| catalog.def(f.kind).category == category))
            .map(|(i, _)| topo.coord(i))
            .collect()
    }

    /// **Player search at tile `i`.** Reveal every still-undiscovered feature here whose
    /// knowledge gate ([`FeatureDef::requires`]) the player already satisfies — Hidden places
    /// (usually ungated) and any Secret whose lore the player holds. Returns the kinds newly
    /// revealed, so the caller can harvest their [`FeatureDef::reveals`]. The deterministic,
    /// knowledge-gated heart of player discovery (no luck involved).
    pub fn search_at_index(&mut self, catalog: &FeatureCatalog, i: usize, lore: &HashSet<String>) -> Vec<FeatureId> {
        let mut found = Vec::new();
        if let Some(fs) = self.tiles.get_mut(i) {
            for f in fs.iter_mut() {
                if !f.discovered && catalog.def(f.kind).requires.iter().all(|r| lore.contains(r)) {
                    f.discovered = true;
                    found.push(f.kind);
                }
            }
        }
        found
    }

    /// Whether searching tile `i` would turn anything up, given the lore the player holds:
    /// `Findable` (something undiscovered here whose gate is met), `Locked` (something is
    /// here but the player lacks the knowledge to find it — the lure), or `Nothing`.
    pub fn find_state_at_index(&self, catalog: &FeatureCatalog, i: usize, lore: &HashSet<String>) -> FindState {
        let mut locked = false;
        for f in self.at_index(i) {
            if !f.discovered {
                if catalog.def(f.kind).requires.iter().all(|r| lore.contains(r)) {
                    return FindState::Findable;
                }
                locked = true;
            }
        }
        if locked { FindState::Locked } else { FindState::Nothing }
    }

    /// Reveal every still-hidden feature on tile `i` whose discovery tier is at or
    /// below `tier` (Landmark < Hidden < Secret). Returns how many were newly
    /// revealed. The mechanism behind [`people::discover_features`](crate::people::discover_features).
    pub fn discover_at_index(&mut self, catalog: &FeatureCatalog, i: usize, tier: Discovery) -> usize {
        let mut revealed = 0;
        if let Some(fs) = self.tiles.get_mut(i) {
            for f in fs {
                if !f.discovered && discovery_rank(catalog.def(f.kind).discovery) <= discovery_rank(tier) {
                    f.discovered = true;
                    revealed += 1;
                }
            }
        }
        revealed
    }

    /// Defile the first discovered, unspoilt feature on tile `i` — the dark twin of
    /// [`discover_at_index`](Self::discover_at_index): mark it `defiled` and return its kind,
    /// or `None` if there is no marvel here to ruin. Used by the
    /// [`Defile`](crate::beats::Effect::Defile) beat effect.
    pub fn defile_at_index(&mut self, i: usize) -> Option<FeatureId> {
        if let Some(fs) = self.tiles.get_mut(i) {
            for f in fs {
                if f.discovered && !f.defiled {
                    f.defiled = true;
                    return Some(f.kind);
                }
            }
        }
        None
    }
}

/// Ordering of discovery tiers, so "reveal up to Hidden" is a comparison.
fn discovery_rank(d: Discovery) -> u8 {
    match d {
        Discovery::Landmark => 0,
        Discovery::Hidden => 1,
        Discovery::Secret => 2,
    }
}

/// The resolved feature catalog, shared as a resource.
#[derive(Resource, Clone, Debug)]
pub struct FeatureCatalog {
    defs: Vec<FeatureDef>,
}

impl FeatureCatalog {
    /// The defaults shipped with the crate — the bundled content set.
    pub fn bundled() -> Self {
        Self::load(&Config::bundled()).expect("bundled features are valid")
    }

    /// Load the catalog from a [`Config`]'s content source.
    pub fn load(cfg: &Config) -> Result<Self, FeatureError> {
        Ok(Self { defs: cfg.load(Asset::Features)? })
    }

    /// Parse a catalog from RON text.
    pub fn from_ron(s: &str) -> Result<Self, config::ConfigError> {
        Ok(Self { defs: config::parse(s)? })
    }

    pub fn def(&self, id: FeatureId) -> &FeatureDef {
        &self.defs[id]
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &FeatureDef> {
        self.defs.iter()
    }

    /// The id of the kind named `name`, if any.
    pub fn id_of(&self, name: &str) -> Option<FeatureId> {
        self.defs.iter().position(|d| d.name == name)
    }

    /// The name of a feature kind.
    pub fn name(&self, id: FeatureId) -> &str {
        &self.defs[id].name
    }
}

impl Default for FeatureCatalog {
    fn default() -> Self {
        Self::bundled()
    }
}

// Feature-placement knobs ([`FeatureConfig`]) live Bevy-free in the `config`
// crate; re-exported here. It's passed to [`place`] by reference (never stored
// as an ECS resource), so it needs no newtype. Its `density` array is fixed at
// `FEATURE_CATEGORY_COUNT`, which must equal our [`Category`] count — asserted
// at compile time below so the two can never drift.
pub use config::FeatureConfig;

const _: () = assert!(Category::COUNT == config::tunables::FEATURE_CATEGORY_COUNT);

/// Suitability of a feature here: a compensated product of its terms (mirroring
/// the IAUS makeup factor in [`ai::score`](crate::ai::score), so feature fit and
/// agent appeal use one algebra). No terms → no suitability.
fn suitability(terms: &[Term], signal: impl Fn(Signal) -> f32) -> f32 {
    if terms.is_empty() {
        return 0.0;
    }
    let mod_factor = 1.0 - 1.0 / terms.len() as f32;
    let mut total = 1.0;
    for t in terms {
        let s = t.curve.eval(signal(t.signal));
        total *= s + (1.0 - s) * mod_factor * s;
    }
    total
}

/// Place features over a substrate. Deterministic given `rng`; uses its own RNG
/// stream so feature placement never perturbs the economy's.
pub fn place(substrate: &GameWorld, catalog: &FeatureCatalog, cfg: &FeatureConfig, rng: &mut SplitMix64) -> Features {
    let topo = substrate.topology();
    let n = topo.len();
    let sea = substrate.params().sea_level;
    let biomass_max = substrate.params().biomass_max.max(1e-3);

    // Per-tile signals (Remoteness filled in after the community pass). Sea tiles
    // carry no features, so they keep all-zero signals.
    let mut sig = vec![[0.0f32; Signal::COUNT]; n];
    let mut land = vec![false; n];
    for i in 0..n {
        let c = topo.coord(i);
        let elev = substrate.elevation(c);
        if elev < sea {
            continue;
        }
        land[i] = true;
        let slope = topo
            .neighbors(i)
            .iter()
            .map(|l| (elev - substrate.elevation(topo.coord(l.to))).max(0.0))
            .fold(0.0f32, f32::max);
        let coast = topo.neighbors(i).iter().any(|l| substrate.elevation(topo.coord(l.to)) < sea);
        let fertility = (substrate.carrying_capacity(c) / biomass_max).clamp(0.0, 1.0);
        let veg = (substrate.plant_biomass(c) / biomass_max).clamp(0.0, 1.0);
        let formation = substrate.biome(c).formation();
        let s = &mut sig[i];
        s[Signal::Elevation.idx()] = elev - sea;
        s[Signal::Slope.idx()] = slope;
        s[Signal::Temperature.idx()] = substrate.temperature(c);
        s[Signal::SoilNutrients.idx()] = substrate.soil_nutrients(c);
        s[Signal::Fertility.idx()] = fertility;
        s[Signal::Vegetation.idx()] = veg;
        s[Signal::Minerals.idx()] = substrate.minerals(c);
        s[Signal::SurfaceWater.idx()] = substrate.surface_water(c);
        s[Signal::Coast.idx()] = if coast { 1.0 } else { 0.0 };
        s[Signal::Forest.idx()] =
            if matches!(formation, Formation::Forest | Formation::Rainforest) { 1.0 } else { 0.0 };
        s[Signal::Grass.idx()] = if formation == Formation::Grassland { 1.0 } else { 0.0 };
        s[Signal::Aridity.idx()] = 1.0 - fertility;
    }

    let mut tiles: Vec<Vec<Feature>> = vec![Vec::new(); n];

    // Pass 1 — communities, with an inhibition radius so they space out.
    let mut blocked = vec![false; n];
    for i in 0..n {
        if !land[i] || blocked[i] {
            continue;
        }
        if let Some(kind) = choose(Category::Community, i, &sig, &tiles, catalog, cfg, rng) {
            let discovered = catalog.def(kind).discovery == Discovery::Landmark;
            tiles[i].push(Feature { kind, discovered, defiled: false });
            block_within(topo, &mut blocked, i, cfg.community_spacing);
        }
    }

    // Remoteness = hex distance to the nearest community, normalised.
    fill_remoteness(topo, catalog, &tiles, &mut sig, cfg.remoteness_scale);

    // Pass 2 — the relational categories, in fixed order.
    for &cat in &[Category::Court, Category::Ruin, Category::Wilderness] {
        for i in 0..n {
            if !land[i] {
                continue;
            }
            if let Some(kind) = choose(cat, i, &sig, &tiles, catalog, cfg, rng) {
                let discovered = catalog.def(kind).discovery == Discovery::Landmark;
                tiles[i].push(Feature { kind, discovered, defiled: false });
            }
        }
    }

    Features { tiles }
}

/// Pick a feature of `cat` for tile `i`, or `None`. Honours one-per-category and
/// `host` constraints, gates on the best candidate's rate, then chooses among the
/// suitable kinds weighted by fit. Draws from `rng` only when candidates exist, so
/// the stream is stable for a given catalog.
fn choose(
    cat: Category,
    i: usize,
    sig: &[[f32; Signal::COUNT]],
    tiles: &[Vec<Feature>],
    catalog: &FeatureCatalog,
    cfg: &FeatureConfig,
    rng: &mut SplitMix64,
) -> Option<FeatureId> {
    if tiles[i].iter().any(|f| catalog.def(f.kind).category == cat) {
        return None; // one feature per category per hex
    }
    let mut cands: Vec<(FeatureId, f32)> = Vec::new();
    let mut best = 0.0f32;
    for (id, d) in catalog.iter().enumerate() {
        if d.category != cat {
            continue;
        }
        if let Some(h) = d.host
            && !tiles[i].iter().any(|f| catalog.def(f.kind).category == h)
        {
            continue;
        }
        let rate = suitability(&d.favoured, |s| sig[i][s.idx()]) * d.density;
        if rate > 0.0 {
            cands.push((id, rate));
            best = best.max(rate);
        }
    }
    if cands.is_empty() {
        return None;
    }
    let p = (cfg.density[cat.idx()] * best).clamp(0.0, 1.0);
    if !rng.gen_bool(p as f64) {
        return None;
    }
    let weight = |r: f32| r.powf(cfg.sharpness);
    let total: f32 = cands.iter().map(|&(_, r)| weight(r)).sum();
    if total <= 0.0 {
        return None;
    }
    let mut t = rng.next_f64() as f32 * total;
    for &(id, r) in &cands {
        let w = weight(r);
        if t < w {
            return Some(id);
        }
        t -= w;
    }
    Some(cands.last().unwrap().0)
}

/// Mark every tile within `radius` hexes of `start` (inclusive) as blocked — the
/// community inhibition radius (a bounded BFS over the topology).
fn block_within(topo: &Topology, blocked: &mut [bool], start: usize, radius: u32) {
    blocked[start] = true;
    if radius == 0 {
        return;
    }
    let mut dist = vec![u32::MAX; blocked.len()];
    dist[start] = 0;
    let mut q = VecDeque::from([start]);
    while let Some(c) = q.pop_front() {
        if dist[c] == radius {
            continue;
        }
        for l in topo.neighbors(c) {
            if dist[l.to] == u32::MAX {
                dist[l.to] = dist[c] + 1;
                blocked[l.to] = true;
                q.push_back(l.to);
            }
        }
    }
}

/// Fill the [`Signal::Remoteness`] slot from a multi-source BFS out of the
/// community tiles. With no communities, everywhere reads fully remote.
fn fill_remoteness(
    topo: &Topology,
    catalog: &FeatureCatalog,
    tiles: &[Vec<Feature>],
    sig: &mut [[f32; Signal::COUNT]],
    scale: f32,
) {
    let n = topo.len();
    let mut dist = vec![u32::MAX; n];
    let mut q = VecDeque::new();
    for (i, fs) in tiles.iter().enumerate() {
        if fs.iter().any(|f| catalog.def(f.kind).category == Category::Community) {
            dist[i] = 0;
            q.push_back(i);
        }
    }
    while let Some(c) = q.pop_front() {
        for l in topo.neighbors(c) {
            if dist[l.to] > dist[c] + 1 {
                dist[l.to] = dist[c] + 1;
                q.push_back(l.to);
            }
        }
    }
    let scale = scale.max(1e-3);
    for i in 0..n {
        sig[i][Signal::Remoteness.idx()] = if dist[i] == u32::MAX { 1.0 } else { (dist[i] as f32 / scale).min(1.0) };
    }
}

/// Why loading a feature catalog failed.
#[derive(Debug)]
pub enum FeatureError {
    Config(config::ConfigError),
}

impl std::fmt::Display for FeatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeatureError::Config(e) => write!(f, "loading features: {e}"),
        }
    }
}

impl std::error::Error for FeatureError {}

impl From<config::ConfigError> for FeatureError {
    fn from(e: config::ConfigError) -> Self {
        FeatureError::Config(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_sim::{Params, World as GameWorld};
    use sim::Substrate as _; // brings `evolve` into scope for warm-up

    /// A warmed-up world the placement can score against.
    fn world(seed: u64) -> GameWorld {
        let mut w = GameWorld::generate(48, 36, Params::default(), seed);
        let mut rng = SplitMix64::new(seed ^ 0xABCD);
        for _ in 0..200 {
            w.evolve(&mut rng);
        }
        w
    }

    #[test]
    fn search_is_knowledge_gated() {
        // A seer that teaches a password, and a gate that needs it — a minimal gnosis chain.
        let cat = FeatureCatalog::from_ron(
            r#"[
                (name: "seer", category: Court, discovery: Secret, density: 0.0, favoured: [], reveals: ["password"]),
                (name: "gate", category: Ruin, discovery: Secret, density: 0.0, favoured: [], requires: ["password"]),
            ]"#,
        )
        .unwrap();
        let (seer, gate) = (cat.id_of("seer").unwrap(), cat.id_of("gate").unwrap());
        let mut feats =
            Features { tiles: vec![vec![Feature { kind: seer, discovered: false, defiled: false }, Feature { kind: gate, discovered: false, defiled: false }]] };
        let mut lore = HashSet::new();

        // The seer is ungated → findable; searching reveals it but NOT the gate.
        assert_eq!(feats.find_state_at_index(&cat, 0, &lore), FindState::Findable);
        assert_eq!(feats.search_at_index(&cat, 0, &lore), vec![seer]);
        // Now only the gate is left, and without the password it merely lures.
        assert_eq!(feats.find_state_at_index(&cat, 0, &lore), FindState::Locked);
        assert!(feats.search_at_index(&cat, 0, &lore).is_empty());

        // Learn the password (as the seer would teach) → the gate opens to a search.
        lore.insert("password".to_string());
        assert_eq!(feats.find_state_at_index(&cat, 0, &lore), FindState::Findable);
        assert_eq!(feats.search_at_index(&cat, 0, &lore), vec![gate]);
        assert_eq!(feats.find_state_at_index(&cat, 0, &lore), FindState::Nothing);
    }

    #[test]
    fn bundled_catalog_loads_and_covers_every_category() {
        let cat = FeatureCatalog::bundled();
        assert!(cat.len() >= 20, "expected a rich catalog, got {}", cat.len());
        for c in Category::ALL {
            assert!(cat.iter().any(|d| d.category == c), "no feature in category {c:?}");
        }
        // A host-constrained kind exists (a court inside a community).
        assert!(cat.iter().any(|d| d.host == Some(Category::Community)));
    }

    #[test]
    fn placement_is_deterministic() {
        let w = world(7);
        let cat = FeatureCatalog::bundled();
        let cfg = FeatureConfig::default();
        let a = place(&w, &cat, &cfg, &mut SplitMix64::new(99));
        let b = place(&w, &cat, &cfg, &mut SplitMix64::new(99));
        let va: Vec<_> = a.iter().map(|(i, f)| (i, f.kind, f.discovered)).collect();
        let vb: Vec<_> = b.iter().map(|(i, f)| (i, f.kind, f.discovered)).collect();
        assert_eq!(va, vb, "same seed must place identical features");
    }

    #[test]
    fn features_land_and_cover_categories() {
        let w = world(7);
        let cat = FeatureCatalog::bundled();
        let feats = place(&w, &cat, &FeatureConfig::default(), &mut SplitMix64::new(1));
        assert!(feats.total() > 10, "a 48×36 world should host many features, got {}", feats.total());
        // Nothing in the sea.
        let topo = w.topology();
        let sea = w.params().sea_level;
        for (i, _) in feats.iter() {
            assert!(w.elevation(topo.coord(i)) >= sea, "a feature was placed in the sea");
        }
        // At least communities and one other category appear.
        assert!(feats.count_of(&cat, Category::Community) > 0, "no settlements formed");
        let others: usize = [Category::Court, Category::Ruin, Category::Wilderness]
            .iter()
            .map(|&c| feats.count_of(&cat, c))
            .sum();
        assert!(others > 0, "only communities formed — no courts/ruins/wilderness");
    }

    #[test]
    fn a_hex_can_carry_multiple_landmarks() {
        // Across a world, at least one tile should stack features from more than
        // one category (e.g. a city with a court, or a settlement with a ruin).
        let w = world(7);
        let cat = FeatureCatalog::bundled();
        let feats = place(&w, &cat, &FeatureConfig::default(), &mut SplitMix64::new(1));
        let stacked = feats.tiles.iter().any(|fs| {
            let mut cats = fs.iter().map(|f| cat.def(f.kind).category);
            cats.clone().count() >= 2 && {
                let first = cats.next();
                cats.any(|c| Some(c) != first)
            }
        });
        assert!(stacked, "no hex layered features from two different categories");
    }

    #[test]
    fn courts_with_a_host_sit_on_their_host() {
        let w = world(3);
        let cat = FeatureCatalog::bundled();
        let feats = place(&w, &cat, &FeatureConfig::default(), &mut SplitMix64::new(5));
        for (i, f) in feats.iter() {
            if let Some(host) = cat.def(f.kind).host {
                let has_host = feats.at_index(i).iter().any(|g| cat.def(g.kind).category == host);
                assert!(has_host, "{} requires a {host:?} on its tile but none is there", cat.name(f.kind));
            }
        }
    }

    #[test]
    fn landmarks_are_known_but_hidden_and_secret_are_not() {
        let w = world(3);
        let cat = FeatureCatalog::bundled();
        let feats = place(&w, &cat, &FeatureConfig::default(), &mut SplitMix64::new(5));
        for (_, f) in feats.iter() {
            let tier = cat.def(f.kind).discovery;
            match tier {
                Discovery::Landmark => assert!(f.discovered, "a landmark should be known on placement"),
                _ => assert!(!f.discovered, "a {tier:?} feature should start latent"),
            }
        }
    }

    #[test]
    fn searching_reveals_hidden_but_not_secret() {
        let w = world(3);
        let cat = FeatureCatalog::bundled();
        let mut feats = place(&w, &cat, &FeatureConfig::default(), &mut SplitMix64::new(5));
        // Find a tile with a still-hidden feature and a tile with a secret.
        let hidden_tile = feats
            .iter()
            .find(|(_, f)| cat.def(f.kind).discovery == Discovery::Hidden)
            .map(|(i, _)| i);
        if let Some(i) = hidden_tile {
            let revealed = feats.discover_at_index(&cat, i, Discovery::Hidden);
            assert!(revealed >= 1, "searching should reveal the hidden feature");
            // A secret on the same tile stays hidden under a Hidden-tier search.
            let still_secret = feats
                .at_index(i)
                .iter()
                .any(|f| cat.def(f.kind).discovery == Discovery::Secret && !f.discovered);
            let has_secret = feats.at_index(i).iter().any(|f| cat.def(f.kind).discovery == Discovery::Secret);
            assert_eq!(still_secret, has_secret, "a Hidden-tier search must not expose secrets");
        }
    }

    #[test]
    fn spacing_keeps_settlements_apart() {
        // With a large inhibition radius, no two communities should be adjacent.
        let w = world(7);
        let cat = FeatureCatalog::bundled();
        let cfg = FeatureConfig { community_spacing: 2, ..FeatureConfig::default() };
        let feats = place(&w, &cat, &cfg, &mut SplitMix64::new(1));
        let topo = w.topology();
        let is_community = |i: usize| feats.at_index(i).iter().any(|f| cat.def(f.kind).category == Category::Community);
        for (i, _) in feats.iter() {
            if is_community(i) {
                for l in topo.neighbors(i) {
                    assert!(!is_community(l.to), "two communities ended up adjacent despite spacing");
                }
            }
        }
    }
}
