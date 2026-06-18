//! Categorical tile qualities — the non-numeric `static`/`slow` fields.
//!
//! Numeric fields (elevation, temperature, …) live as plain `f32` vectors on
//! the [`World`](crate::world::World); these enums cover qualities that are a
//! choice among kinds. `CrustType` and `Lithology` are set by worldgen; the
//! [`Biome`] (a tile's Holdridge life zone) is reclassified every tick from the
//! climate, with [`Belt`], [`HumidityProvince`] and [`Formation`] as its axes
//! and coarse grouping. The biome also carries a [`BiomeProfile`] — the
//! ecological constants the ecosystem update reads, so the biome *organises* the
//! ecology rather than being a passive label.

use config::Params;

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

/// One of the seven **latitudinal regions / altitudinal belts** of the Holdridge
/// life-zone system, set by a tile's mean annual *biotemperature* — colder tiles
/// (high latitude or high elevation) fall in the earlier belts. The boundary
/// temperatures (1.5, 3, 6, 12, 18, 24 °C) are tunable in `Params`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Belt {
    Polar,
    Subpolar,
    Boreal,
    CoolTemperate,
    WarmTemperate,
    Subtropical,
    Tropical,
}

impl Belt {
    /// The seven belts, coldest → warmest.
    pub const ALL: [Belt; 7] = [
        Belt::Polar,
        Belt::Subpolar,
        Belt::Boreal,
        Belt::CoolTemperate,
        Belt::WarmTemperate,
        Belt::Subtropical,
        Belt::Tropical,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Belt::Polar => "polar",
            Belt::Subpolar => "subpolar",
            Belt::Boreal => "boreal",
            Belt::CoolTemperate => "cool temperate",
            Belt::WarmTemperate => "warm temperate",
            Belt::Subtropical => "subtropical",
            Belt::Tropical => "tropical",
        }
    }
}

/// A Holdridge **humidity province**: how wet a tile is, read off the ratio of
/// potential evapotranspiration to precipitation (a high ratio is dry). Ordered
/// driest → wettest, the index into a belt's life-zone row.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum HumidityProvince {
    Superarid,
    Perarid,
    Arid,
    Semiarid,
    Subhumid,
    Humid,
    Perhumid,
    Superhumid,
}

impl HumidityProvince {
    /// The eight provinces, driest → wettest; the position is the row index a
    /// PET/precipitation ratio resolves to.
    pub const ALL: [HumidityProvince; 8] = [
        HumidityProvince::Superarid,
        HumidityProvince::Perarid,
        HumidityProvince::Arid,
        HumidityProvince::Semiarid,
        HumidityProvince::Subhumid,
        HumidityProvince::Humid,
        HumidityProvince::Perhumid,
        HumidityProvince::Superhumid,
    ];

    pub fn name(self) -> &'static str {
        match self {
            HumidityProvince::Superarid => "superarid",
            HumidityProvince::Perarid => "perarid",
            HumidityProvince::Arid => "arid",
            HumidityProvince::Semiarid => "semiarid",
            HumidityProvince::Subhumid => "subhumid",
            HumidityProvince::Humid => "humid",
            HumidityProvince::Perhumid => "perhumid",
            HumidityProvince::Superhumid => "superhumid",
        }
    }
}

/// A coarse **physiognomic grouping** of biomes — the structural look of the
/// vegetation, shared across many life zones. This is the handle the renderer,
/// vegetation scatter, and agent signals read; the full [`Biome`] carries the
/// finer life-zone identity beneath it. (It is the rough heir of the old coarse
/// `Pft`, with rainforest split off from forest.)
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

/// The **ecological character** of a biome — the constants the ecosystem update
/// reads once the climate has been distilled into a life zone. This is what makes
/// the biome the organising layer of the ecology rather than a passive label: a
/// rainforest is lush, fast-rotting and too wet to burn; a desert is barren and
/// inert; tundra is sparse and freezes its litter into slow peat.
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

/// A tile's **biome**: one of the 38 Holdridge life zones, or open `Water`.
/// Reclassified each tick from the tile's mean annual biotemperature and the
/// PET/precipitation ratio (see `World::classify_biome`). Replaces the old coarse
/// Whittaker `Pft`; the structural look is recovered with [`Biome::formation`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Biome {
    /// Open sea or lake — any tile below the sea-level datum.
    Water,

    // --- Polar ---
    PolarDesert,

    // --- Subpolar ---
    SubpolarDryTundra,
    SubpolarMoistTundra,
    SubpolarWetTundra,
    SubpolarRainTundra,

    // --- Boreal ---
    BorealDesert,
    BorealDryScrub,
    BorealMoistForest,
    BorealWetForest,
    BorealRainForest,

    // --- Cool temperate ---
    CoolTemperateDesert,
    CoolTemperateDesertScrub,
    CoolTemperateSteppe,
    CoolTemperateMoistForest,
    CoolTemperateWetForest,
    CoolTemperateRainForest,

    // --- Warm temperate ---
    WarmTemperateDesert,
    WarmTemperateDesertScrub,
    WarmTemperateThornScrub,
    WarmTemperateDryForest,
    WarmTemperateMoistForest,
    WarmTemperateWetForest,
    WarmTemperateRainForest,

    // --- Subtropical ---
    SubtropicalDesert,
    SubtropicalDesertScrub,
    SubtropicalThornWoodland,
    SubtropicalDryForest,
    SubtropicalMoistForest,
    SubtropicalWetForest,
    SubtropicalRainForest,

    // --- Tropical ---
    TropicalDesert,
    TropicalDesertScrub,
    TropicalThornWoodland,
    TropicalVeryDryForest,
    TropicalDryForest,
    TropicalMoistForest,
    TropicalWetForest,
    TropicalRainForest,
}

impl Biome {
    /// Every biome — open water plus the 38 land life zones — for iteration
    /// (histograms, palette tables, profile lookups).
    pub const ALL: [Biome; 39] = [
        Biome::Water,
        Biome::PolarDesert,
        Biome::SubpolarDryTundra,
        Biome::SubpolarMoistTundra,
        Biome::SubpolarWetTundra,
        Biome::SubpolarRainTundra,
        Biome::BorealDesert,
        Biome::BorealDryScrub,
        Biome::BorealMoistForest,
        Biome::BorealWetForest,
        Biome::BorealRainForest,
        Biome::CoolTemperateDesert,
        Biome::CoolTemperateDesertScrub,
        Biome::CoolTemperateSteppe,
        Biome::CoolTemperateMoistForest,
        Biome::CoolTemperateWetForest,
        Biome::CoolTemperateRainForest,
        Biome::WarmTemperateDesert,
        Biome::WarmTemperateDesertScrub,
        Biome::WarmTemperateThornScrub,
        Biome::WarmTemperateDryForest,
        Biome::WarmTemperateMoistForest,
        Biome::WarmTemperateWetForest,
        Biome::WarmTemperateRainForest,
        Biome::SubtropicalDesert,
        Biome::SubtropicalDesertScrub,
        Biome::SubtropicalThornWoodland,
        Biome::SubtropicalDryForest,
        Biome::SubtropicalMoistForest,
        Biome::SubtropicalWetForest,
        Biome::SubtropicalRainForest,
        Biome::TropicalDesert,
        Biome::TropicalDesertScrub,
        Biome::TropicalThornWoodland,
        Biome::TropicalVeryDryForest,
        Biome::TropicalDryForest,
        Biome::TropicalMoistForest,
        Biome::TropicalWetForest,
        Biome::TropicalRainForest,
    ];

    /// The Holdridge life zone in belt `belt` at humidity-province index `h`
    /// (`0` = driest superarid … `7` = wettest superhumid). Each belt's row is
    /// authored from the canonical chart, with the province span clamped to the
    /// zones a belt actually has, so every cell resolves to a real life zone.
    pub fn from_cell(belt: Belt, h: usize) -> Biome {
        use Biome::*;
        let row: [Biome; 8] = match belt {
            Belt::Polar => [PolarDesert; 8],
            Belt::Subpolar => [
                SubpolarDryTundra,
                SubpolarDryTundra,
                SubpolarDryTundra,
                SubpolarDryTundra,
                SubpolarMoistTundra,
                SubpolarMoistTundra,
                SubpolarWetTundra,
                SubpolarRainTundra,
            ],
            Belt::Boreal => [
                BorealDesert,
                BorealDesert,
                BorealDryScrub,
                BorealDryScrub,
                BorealMoistForest,
                BorealMoistForest,
                BorealWetForest,
                BorealRainForest,
            ],
            Belt::CoolTemperate => [
                CoolTemperateDesert,
                CoolTemperateDesertScrub,
                CoolTemperateSteppe,
                CoolTemperateSteppe,
                CoolTemperateMoistForest,
                CoolTemperateMoistForest,
                CoolTemperateWetForest,
                CoolTemperateRainForest,
            ],
            Belt::WarmTemperate => [
                WarmTemperateDesert,
                WarmTemperateDesertScrub,
                WarmTemperateThornScrub,
                WarmTemperateDryForest,
                WarmTemperateMoistForest,
                WarmTemperateWetForest,
                WarmTemperateRainForest,
                WarmTemperateRainForest,
            ],
            Belt::Subtropical => [
                SubtropicalDesert,
                SubtropicalDesertScrub,
                SubtropicalThornWoodland,
                SubtropicalDryForest,
                SubtropicalMoistForest,
                SubtropicalWetForest,
                SubtropicalRainForest,
                SubtropicalRainForest,
            ],
            Belt::Tropical => [
                TropicalDesert,
                TropicalDesertScrub,
                TropicalThornWoodland,
                TropicalVeryDryForest,
                TropicalDryForest,
                TropicalMoistForest,
                TropicalWetForest,
                TropicalRainForest,
            ],
        };
        row[h.min(7)]
    }

    /// The latitudinal/altitudinal belt this biome sits in — `None` for open water.
    pub fn belt(self) -> Option<Belt> {
        use Biome::*;
        Some(match self {
            Water => return None,
            PolarDesert => Belt::Polar,
            SubpolarDryTundra | SubpolarMoistTundra | SubpolarWetTundra | SubpolarRainTundra => {
                Belt::Subpolar
            }
            BorealDesert | BorealDryScrub | BorealMoistForest | BorealWetForest
            | BorealRainForest => Belt::Boreal,
            CoolTemperateDesert | CoolTemperateDesertScrub | CoolTemperateSteppe
            | CoolTemperateMoistForest | CoolTemperateWetForest | CoolTemperateRainForest => {
                Belt::CoolTemperate
            }
            WarmTemperateDesert | WarmTemperateDesertScrub | WarmTemperateThornScrub
            | WarmTemperateDryForest | WarmTemperateMoistForest | WarmTemperateWetForest
            | WarmTemperateRainForest => Belt::WarmTemperate,
            SubtropicalDesert | SubtropicalDesertScrub | SubtropicalThornWoodland
            | SubtropicalDryForest | SubtropicalMoistForest | SubtropicalWetForest
            | SubtropicalRainForest => Belt::Subtropical,
            TropicalDesert | TropicalDesertScrub | TropicalThornWoodland | TropicalVeryDryForest
            | TropicalDryForest | TropicalMoistForest | TropicalWetForest | TropicalRainForest => {
                Belt::Tropical
            }
        })
    }

    /// The coarse structural look of this biome — the handle renderers and agent
    /// signals read. Cold barrens read as `Tundra` (pale) rather than warm desert.
    pub fn formation(self) -> Formation {
        use Biome::*;
        match self {
            Water => Formation::Water,
            // Cold barrens: pale, frost-bound rather than warm dune.
            PolarDesert | BorealDesert => Formation::Tundra,
            // The tundra series.
            SubpolarDryTundra | SubpolarMoistTundra | SubpolarWetTundra | SubpolarRainTundra => {
                Formation::Tundra
            }
            // Warm barrens.
            CoolTemperateDesert | WarmTemperateDesert | SubtropicalDesert | TropicalDesert => {
                Formation::Desert
            }
            // Scrub, thorn and dry-shrub zones.
            BorealDryScrub | CoolTemperateDesertScrub | WarmTemperateDesertScrub
            | WarmTemperateThornScrub | SubtropicalDesertScrub | SubtropicalThornWoodland
            | TropicalDesertScrub | TropicalThornWoodland => Formation::Shrubland,
            // Open grassy steppe.
            CoolTemperateSteppe => Formation::Grassland,
            // Closed forest.
            BorealMoistForest | BorealWetForest | CoolTemperateMoistForest
            | CoolTemperateWetForest | WarmTemperateDryForest | WarmTemperateMoistForest
            | SubtropicalDryForest | SubtropicalMoistForest | TropicalVeryDryForest
            | TropicalDryForest | TropicalMoistForest => Formation::Forest,
            // The lushest, wettest canopies.
            BorealRainForest | CoolTemperateRainForest | WarmTemperateWetForest
            | WarmTemperateRainForest | SubtropicalWetForest | SubtropicalRainForest
            | TropicalWetForest | TropicalRainForest => Formation::Rainforest,
        }
    }

    /// The biome's [`BiomeProfile`] — its ecological constants. Productivity comes
    /// from the structural formation (a tunable `Params` base) scaled by the belt's
    /// warmth, so within a class the warm zones out-produce the cold (a tropical
    /// rainforest dwarfs a boreal one); flammability and decay are set by the
    /// formation, with decay further sped by warmth.
    pub fn profile(self, p: &Params) -> BiomeProfile {
        let (base_prod, flammability, base_decay) = match self.formation() {
            Formation::Water => (0.0, 0.0, 1.0),
            Formation::Desert => (p.prod_desert, 0.4, 0.5),
            Formation::Tundra => (p.prod_tundra, 0.2, 0.3),
            Formation::Grassland => (p.prod_grass, 1.5, 0.9),
            Formation::Shrubland => (p.prod_shrub, 1.2, 0.8),
            Formation::Forest => (p.prod_forest, 0.8, 1.0),
            Formation::Rainforest => (p.prod_rainforest, 0.25, 1.3),
        };
        let warmth = match self.belt() {
            None => 0.0,
            Some(Belt::Polar) => 0.55,
            Some(Belt::Subpolar) => 0.6,
            Some(Belt::Boreal) => 0.75,
            Some(Belt::CoolTemperate) => 0.9,
            Some(Belt::WarmTemperate) => 1.0,
            Some(Belt::Subtropical) => 1.1,
            Some(Belt::Tropical) => 1.2,
        };
        BiomeProfile {
            productivity: base_prod * warmth,
            flammability,
            decay: base_decay * (0.4 + 0.9 * warmth),
        }
    }

    /// The biome's scientific life-zone name (e.g. `"cool temperate steppe"`).
    pub fn name(self) -> &'static str {
        use Biome::*;
        match self {
            Water => "open water",
            PolarDesert => "polar desert",
            SubpolarDryTundra => "subpolar dry tundra",
            SubpolarMoistTundra => "subpolar moist tundra",
            SubpolarWetTundra => "subpolar wet tundra",
            SubpolarRainTundra => "subpolar rain tundra",
            BorealDesert => "boreal desert",
            BorealDryScrub => "boreal dry scrub",
            BorealMoistForest => "boreal moist forest",
            BorealWetForest => "boreal wet forest",
            BorealRainForest => "boreal rain forest",
            CoolTemperateDesert => "cool temperate desert",
            CoolTemperateDesertScrub => "cool temperate desert scrub",
            CoolTemperateSteppe => "cool temperate steppe",
            CoolTemperateMoistForest => "cool temperate moist forest",
            CoolTemperateWetForest => "cool temperate wet forest",
            CoolTemperateRainForest => "cool temperate rain forest",
            WarmTemperateDesert => "warm temperate desert",
            WarmTemperateDesertScrub => "warm temperate desert scrub",
            WarmTemperateThornScrub => "warm temperate thorn scrub",
            WarmTemperateDryForest => "warm temperate dry forest",
            WarmTemperateMoistForest => "warm temperate moist forest",
            WarmTemperateWetForest => "warm temperate wet forest",
            WarmTemperateRainForest => "warm temperate rain forest",
            SubtropicalDesert => "subtropical desert",
            SubtropicalDesertScrub => "subtropical desert scrub",
            SubtropicalThornWoodland => "subtropical thorn woodland",
            SubtropicalDryForest => "subtropical dry forest",
            SubtropicalMoistForest => "subtropical moist forest",
            SubtropicalWetForest => "subtropical wet forest",
            SubtropicalRainForest => "subtropical rain forest",
            TropicalDesert => "tropical desert",
            TropicalDesertScrub => "tropical desert scrub",
            TropicalThornWoodland => "tropical thorn woodland",
            TropicalVeryDryForest => "tropical very dry forest",
            TropicalDryForest => "tropical dry forest",
            TropicalMoistForest => "tropical moist forest",
            TropicalWetForest => "tropical wet forest",
            TropicalRainForest => "tropical rain forest",
        }
    }
}
