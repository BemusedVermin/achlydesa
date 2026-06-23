//! Categorical tile qualities — the non-numeric `static`/`slow` fields.
//!
//! Numeric fields (elevation, temperature, …) live as plain `f32` vectors on
//! the [`World`](crate::world::World); these enums cover qualities that are a
//! choice among kinds. `CrustType` and `Lithology` are set by worldgen; the
//! [`Biome`] (a tile's Holdridge life zone) is reclassified every tick from the
//! climate. The Holdridge life-zone **content** — the 39 zones, the latitudinal
//! belts, the humidity provinces, and the belt×humidity classifier — is **data**
//! ([`biomes`](crate::biomes), `assets/data/biomes.ron`); a `Biome` here is just
//! the dense id into that registry. [`Formation`] (the coarse structural look the
//! renderer, vegetation scatter and agent signals read) stays a small closed
//! vocabulary — each variant has real behaviour in those consumers.

/// Whether a tile's bedrock is continental (light, high-standing) or oceanic
/// (dense, low-standing). Set at world-gen from the owning plate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrustType {
    Continental,
    Oceanic,
}

/// Bedrock class. Gates soil fertility and which minerals appear; for now it is
/// assigned per plate and read by ore generation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lithology {
    Igneous,
    Sedimentary,
    Metamorphic,
}

impl Lithology {
    /// The three classes, for round-robin / indexed assignment at world-gen.
    pub const ALL: [Lithology; 3] = [
        Lithology::Igneous,
        Lithology::Sedimentary,
        Lithology::Metamorphic,
    ];
}

/// A coarse **physiognomic grouping** of biomes — the structural look of the
/// vegetation, shared across many life zones. This is the handle the renderer,
/// vegetation scatter, and agent signals read; the full [`Biome`] carries the
/// finer life-zone identity beneath it.
///
/// A small **closed vocabulary**, not content: each variant has real behaviour in
/// its consumers (a per-formation colour, vegetation scatter, travel cost, fauna
/// habitat, fire/albedo constants), so adding one is a code change, not a data row.
/// Biomes *name* their formation in `biomes.ron`, resolved here via [`Formation::from_name`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Formation {
    Water,
    Desert,
    Tundra,
    Grassland,
    Shrubland,
    Forest,
    Rainforest,
}

impl Formation {
    /// The seven formations, in id order (the index [`Formation::idx`] returns).
    pub const ALL: [Formation; 7] = [
        Formation::Water,
        Formation::Desert,
        Formation::Tundra,
        Formation::Grassland,
        Formation::Shrubland,
        Formation::Forest,
        Formation::Rainforest,
    ];

    /// A dense index `0..7` — for the per-formation tables the biome registry holds.
    pub fn idx(self) -> usize {
        self as usize
    }

    /// Resolve a formation by its lowercase name (`"forest"`) — the form `biomes.ron`
    /// and `bestiary.ron` author. `None` for an unknown name.
    pub fn from_name(name: &str) -> Option<Formation> {
        Some(match name {
            "water" => Formation::Water,
            "desert" => Formation::Desert,
            "tundra" => Formation::Tundra,
            "grassland" => Formation::Grassland,
            "shrubland" => Formation::Shrubland,
            "forest" => Formation::Forest,
            "rainforest" => Formation::Rainforest,
            _ => return None,
        })
    }
}

/// The **ecological character** of a biome — the constants the ecosystem update
/// reads once the climate has been distilled into a life zone. This is what makes
/// the biome the organising layer of the ecology rather than a passive label: a
/// rainforest is lush, fast-rotting and too wet to burn; a desert is barren and
/// inert; tundra is sparse and freezes its litter into slow peat. Composed by
/// [`Biomes::profile`](crate::biomes::Biomes::profile) from the formation + belt.
#[derive(Clone, Copy, Debug)]
pub struct BiomeProfile {
    /// Productivity ceiling — the fraction of `biomass_max` this biome can carry.
    /// The slow, climate-distilled cap on lushness (instantaneous weather still
    /// governs the *rate* a tile greens toward it, so droughts and seasons bite).
    pub productivity: f32,
    /// Relative flammability — how readily the cover ignites and carries fire
    /// (grass and scrub burn eagerly; rainforest, tundra and bare ground resist).
    pub flammability: f32,
    /// Relative litter decomposition rate — warm, wet biomes turn litter over fast;
    /// cold ones lock it into slow soil carbon (peat).
    pub decay: f32,
}

/// A tile's **biome** — its Holdridge life zone, as a dense id into the data-driven
/// [`Biomes`](crate::biomes::Biomes) registry (`assets/data/biomes.ron`). `Biome(0)`
/// is open water by convention (the registry validates it). Reclassified each tick
/// from the tile's mean annual biotemperature and PET/precipitation ratio
/// (`World::update_biome`); the structural look is recovered with
/// [`Biomes::formation`](crate::biomes::Biomes::formation).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Biome(pub u16);

impl Biome {
    /// Open water — id `0`, the first row of `biomes.ron`. Used to fill the grid
    /// before the first classification; every tile is reclassified each tick.
    pub const WATER: Biome = Biome(0);
}
