//! Categorical tile qualities — the non-numeric `static`/`slow` fields.
//!
//! Numeric fields (elevation, temperature, …) live as plain `f32` vectors on
//! the [`World`](crate::world::World); these enums cover qualities that are a
//! choice among kinds. The skeleton uses the two that worldgen sets;
//! `Boundary`, `Pft`, `Biome`, etc. join them as their pipelines come online.

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

/// Dominant plant functional type — the vegetation class a tile settles into,
/// chosen each tick from its climate (a coarse Whittaker-style classification).
/// `Water` marks open sea/lake; `Barren` is desert, rock, or ice with too little
/// growth to vegetate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pft {
    Water,
    Barren,
    Tundra,
    Grassland,
    Shrubland,
    Forest,
}
