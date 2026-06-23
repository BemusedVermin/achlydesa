//! The **Holdridge life-zone registry** — the world's ecology classification, resolved from
//! `assets/data/biomes.ron`.
//!
//! A [`Biome`] is a dense id into this registry; the per-tick classifier and the ecosystem update
//! read the resolved tables here (belt warmth, the per-formation ecology constants, the
//! belt×humidity classifier grid), so the *content* — the 39 life zones, the 7 belts, the 8 humidity
//! provinces — is data and adding a life zone is pure RON. The *logic* (the EMA classification, the
//! profile math) stays in code, reading these tables, so the numbers are byte-identical to the old
//! hardcoded `Biome::name`/`belt`/`formation`/`from_cell`/`profile` tables this replaced.

use crate::fields::{Biome, BiomeProfile, Formation};
use config::{Asset, Bundled, Params};
use serde::Deserialize;
use std::collections::HashMap;

/// Index of a Holdridge belt (`0` = the coldest belt … the warmest last) — also the row index into
/// the [`Biomes::from_cell`] classifier.
pub type BeltId = usize;

/// The fixed Holdridge belts, coldest → warmest. `world::belt_of` maps biotemperature onto a
/// *positional* `BeltId` via the ordered `Params` boundaries, so `biomes.ron` MUST author its belts
/// in exactly this order — a reorder would silently misclassify the whole world. Enforced at load
/// (the belt *names* are also the fixed classification axis the `Params` `biotemp_*` boundaries
/// encode, not free content). Element type `&str` keeps it clear of the `const_all` lint.
const CANONICAL_BELTS: [&str; 7] = [
    "polar",
    "subpolar",
    "boreal",
    "cool temperate",
    "warm temperate",
    "subtropical",
    "tropical",
];

#[derive(Deserialize)]
struct BiomesDoc {
    belts: Vec<BeltDef>,
    provinces: Vec<String>,
    formations: Vec<FormationDef>,
    biomes: Vec<BiomeDef>,
    classifier: Vec<ClassifierRow>,
}

#[derive(Deserialize)]
struct BeltDef {
    name: String,
    warmth: f32,
}

#[derive(Deserialize)]
struct FormationDef {
    name: String,
    flammability: f32,
    decay: f32,
}

#[derive(Deserialize)]
struct BiomeDef {
    name: String,
    formation: String,
    belt: Option<String>,
}

#[derive(Deserialize)]
struct ClassifierRow {
    belt: String,
    /// The 8 life-zone names, driest → wettest (a list in RON, validated to length 8 on load).
    row: Vec<String>,
}

/// A resolved life zone — its name, structural formation, and belt (`None` for open water).
#[derive(Clone, Debug)]
struct Zone {
    name: String,
    formation: Formation,
    belt: Option<BeltId>,
}

/// The resolved Holdridge registry. Owned by the [`World`](crate::world::World), shared read-only.
#[derive(Clone, Debug)]
pub struct Biomes {
    zones: Vec<Zone>,
    ids: HashMap<String, Biome>,
    belt_names: Vec<String>,
    belt_warmth: Vec<f32>,
    provinces: Vec<String>,
    /// `(flammability, base_decay)` per [`Formation`], indexed by [`Formation::idx`].
    formation_consts: [(f32, f32); Formation::ALL.len()],
    /// `classifier[belt]` = the belt's row of 8 life-zone ids, driest → wettest.
    classifier: Vec<[Biome; 8]>,
    water: Biome,
}

impl Biomes {
    /// The defaults shipped with the crate (`biomes.ron`).
    pub fn bundled() -> Self {
        Self::from_ron(Bundled::get(Asset::Biomes)).expect("bundled biomes are valid RON")
    }

    /// Parse and resolve a biomes document: intern each life-zone name to a dense [`Biome`] id and
    /// resolve every cross-reference (a biome's formation/belt, a classifier cell's biome) up front.
    pub fn from_ron(ron: &str) -> Result<Self, String> {
        let doc: BiomesDoc = config::parse(ron).map_err(|e| e.to_string())?;

        let belt_ids: HashMap<String, BeltId> = doc
            .belts
            .iter()
            .enumerate()
            .map(|(i, b)| (b.name.clone(), i))
            .collect();
        let belt_names: Vec<String> = doc.belts.iter().map(|b| b.name.clone()).collect();
        let belt_warmth: Vec<f32> = doc.belts.iter().map(|b| b.warmth).collect();

        // The belts must be the canonical Holdridge axis, in order — `belt_of` indexes the classifier
        // positionally, so a reorder (or a wrong/missing belt) would silently misclassify the world.
        if !belt_names.iter().map(String::as_str).eq(CANONICAL_BELTS) {
            return Err(format!(
                "biomes: belts must be authored coldest→warmest as {CANONICAL_BELTS:?} (the fixed \
                 Holdridge axis `belt_of` maps biotemperature onto), got {belt_names:?}"
            ));
        }

        // Track which formations were *explicitly authored*: an omitted formation's consts would
        // silently stay the (0.0, 0.0) zero-init — zero decay, so litter never decomposes.
        let mut formation_consts = [(0.0f32, 0.0f32); Formation::ALL.len()];
        let mut provided = [false; Formation::ALL.len()];
        for f in &doc.formations {
            let formation = Formation::from_name(&f.name)
                .ok_or_else(|| format!("biomes: unknown formation '{}'", f.name))?;
            formation_consts[formation.idx()] = (f.flammability, f.decay);
            provided[formation.idx()] = true;
        }

        let mut zones = Vec::with_capacity(doc.biomes.len());
        let mut ids = HashMap::new();
        for (i, b) in doc.biomes.iter().enumerate() {
            let formation = Formation::from_name(&b.formation).ok_or_else(|| {
                format!("biome '{}': unknown formation '{}'", b.name, b.formation)
            })?;
            if !provided[formation.idx()] {
                return Err(format!(
                    "biome '{}': formation '{}' has no entry in `formations`, so its ecology \
                     constants (flammability/decay) would silently default to zero",
                    b.name, b.formation
                ));
            }
            let belt = match &b.belt {
                None => None,
                Some(n) => Some(
                    *belt_ids
                        .get(n)
                        .ok_or_else(|| format!("biome '{}': unknown belt '{}'", b.name, n))?,
                ),
            };
            ids.insert(b.name.clone(), Biome(i as u16));
            zones.push(Zone {
                name: b.name.clone(),
                formation,
                belt,
            });
        }

        // Open water is the conventional id 0 — the value the grid is filled with before the first
        // classification, and what classify returns for submerged tiles.
        let water = *ids
            .get("open water")
            .ok_or("biomes: an 'open water' biome is required")?;
        if water != Biome(0) {
            return Err("biomes: 'open water' must be the first biome (id 0)".into());
        }

        let mut classifier = vec![[Biome(0); 8]; doc.belts.len()];
        for r in &doc.classifier {
            let belt = *belt_ids
                .get(&r.belt)
                .ok_or_else(|| format!("classifier: unknown belt '{}'", r.belt))?;
            if r.row.len() != 8 {
                return Err(format!(
                    "classifier belt '{}': expected 8 humidity provinces, got {}",
                    r.belt,
                    r.row.len()
                ));
            }
            let mut row = [Biome(0); 8];
            for (h, name) in r.row.iter().enumerate() {
                row[h] = *ids.get(name).ok_or_else(|| {
                    format!("classifier belt '{}': unknown biome '{}'", r.belt, name)
                })?;
            }
            classifier[belt] = row;
        }

        Ok(Self {
            zones,
            ids,
            belt_names,
            belt_warmth,
            provinces: doc.provinces,
            formation_consts,
            classifier,
            water,
        })
    }

    /// How many life zones are defined (open water + the land zones).
    pub fn count(&self) -> usize {
        self.zones.len()
    }

    /// Every life-zone id, in dense order (for histograms / palette tables).
    pub fn all(&self) -> impl Iterator<Item = Biome> + '_ {
        (0..self.zones.len()).map(|i| Biome(i as u16))
    }

    /// Open water — the id submerged tiles classify to.
    pub fn water(&self) -> Biome {
        self.water
    }

    /// The life zone of the given name (`"cool temperate steppe"`), if any.
    pub fn biome_id(&self, name: &str) -> Option<Biome> {
        self.ids.get(name).copied()
    }

    /// The biome's scientific life-zone name (e.g. `"cool temperate steppe"`).
    pub fn name(&self, b: Biome) -> &str {
        &self.zones[b.0 as usize].name
    }

    /// The biome's coarse structural [`Formation`].
    pub fn formation(&self, b: Biome) -> Formation {
        self.zones[b.0 as usize].formation
    }

    /// The biome's latitudinal/altitudinal belt — `None` for open water.
    pub fn belt(&self, b: Biome) -> Option<BeltId> {
        self.zones[b.0 as usize].belt
    }

    /// The number of belts (the classifier has one row each).
    pub fn belt_count(&self) -> usize {
        self.belt_names.len()
    }

    /// A belt's name (`"cool temperate"`).
    pub fn belt_name(&self, belt: BeltId) -> &str {
        &self.belt_names[belt]
    }

    /// A belt's warmth multiplier (scales productivity / decay within a formation).
    pub fn belt_warmth(&self, belt: BeltId) -> f32 {
        self.belt_warmth[belt]
    }

    /// A humidity province's name by index (`0` driest … `7` wettest); clamped.
    pub fn province_name(&self, i: usize) -> &str {
        &self.provinces[i.min(self.provinces.len().saturating_sub(1))]
    }

    /// The Holdridge life zone in `belt` at humidity-province index `h` (`0` driest … `7` wettest).
    pub fn from_cell(&self, belt: BeltId, h: usize) -> Biome {
        self.classifier[belt][h.min(7)]
    }

    /// The biome's [`BiomeProfile`] — its ecological constants. The base productivity comes from the
    /// structural formation (a tunable `Params` base) scaled by the belt's warmth, so within a class
    /// the warm zones out-produce the cold; flammability and decay are the formation's data
    /// constants, with decay further sped by warmth.
    pub fn profile(&self, b: Biome, p: &Params) -> BiomeProfile {
        let formation = self.formation(b);
        // Base productivity is a tunable Params field per formation (water = 0).
        let base_prod = match formation {
            Formation::Water => 0.0,
            Formation::Desert => p.prod_desert,
            Formation::Tundra => p.prod_tundra,
            Formation::Grassland => p.prod_grass,
            Formation::Shrubland => p.prod_shrub,
            Formation::Forest => p.prod_forest,
            Formation::Rainforest => p.prod_rainforest,
        };
        let (flammability, base_decay) = self.formation_consts[formation.idx()];
        let warmth = self.belt(b).map_or(0.0, |belt| self.belt_warmth[belt]);
        BiomeProfile {
            productivity: base_prod * warmth,
            flammability,
            decay: base_decay * (0.4 + 0.9 * warmth),
        }
    }
}

impl Default for Biomes {
    fn default() -> Self {
        Self::bundled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 7 canonical belts, in order — every valid biomes doc must author exactly these.
    const BELTS: &str = r#"[
        (name: "polar", warmth: 0.55), (name: "subpolar", warmth: 0.6),
        (name: "boreal", warmth: 0.75), (name: "cool temperate", warmth: 0.9),
        (name: "warm temperate", warmth: 1.0), (name: "subtropical", warmth: 1.1),
        (name: "tropical", warmth: 1.2),
    ]"#;

    #[test]
    fn bundled_biomes_resolve() {
        let b = Biomes::bundled();
        assert_eq!(b.count(), 39);
        assert_eq!(b.water(), Biome::WATER);
        assert_eq!(b.name(b.water()), "open water");
        assert_eq!(b.belt_count(), 7);
        // A known cell of the classifier (cool temperate, semiarid index 2 → steppe).
        let steppe = b.biome_id("cool temperate steppe").unwrap();
        let cool = 3; // belt order: polar(0) subpolar(1) boreal(2) cool temperate(3)
        assert_eq!(b.from_cell(cool, 2), steppe);
        assert_eq!(b.formation(steppe), Formation::Grassland);
    }

    #[test]
    fn a_new_life_zone_needs_no_code() {
        // A life zone authored purely in RON resolves into the registry — the whole point.
        let ron = format!(
            r#"(
            belts: {BELTS},
            provinces: ["humid"],
            formations: [(name: "water", flammability: 0.0, decay: 1.0),
                         (name: "forest", flammability: 0.8, decay: 1.0)],
            biomes: [
                (name: "open water", formation: "water", belt: None),
                (name: "cloud forest", formation: "forest", belt: Some("tropical")),
            ],
            classifier: [(belt: "tropical", row: [
                "cloud forest","cloud forest","cloud forest","cloud forest",
                "cloud forest","cloud forest","cloud forest","cloud forest"])],
        )"#
        );
        let b = Biomes::from_ron(&ron).expect("a novel life zone resolves");
        let cloud = b.biome_id("cloud forest").expect("the new zone exists");
        assert_eq!(b.formation(cloud), Formation::Forest);
        assert_eq!(b.from_cell(6, 4), cloud); // tropical is belt id 6 (the warmest)
    }

    #[test]
    fn an_unknown_formation_is_rejected() {
        let ron = format!(
            r#"(
            belts: {BELTS}, provinces: [], formations: [],
            biomes: [(name: "open water", formation: "lava", belt: None)],
            classifier: [],
        )"#
        );
        assert!(Biomes::from_ron(&ron).is_err());
    }

    #[test]
    fn belts_out_of_order_are_rejected() {
        // The canonical belts, but polar and tropical swapped — `belt_of` would index the wrong row,
        // silently misclassifying the world. It must be a load error, not silent corruption.
        let ron = r#"(
            belts: [
                (name: "tropical", warmth: 1.2), (name: "subpolar", warmth: 0.6),
                (name: "boreal", warmth: 0.75), (name: "cool temperate", warmth: 0.9),
                (name: "warm temperate", warmth: 1.0), (name: "subtropical", warmth: 1.1),
                (name: "polar", warmth: 0.55),
            ],
            provinces: [], formations: [(name: "water", flammability: 0.0, decay: 1.0)],
            biomes: [(name: "open water", formation: "water", belt: None)],
            classifier: [],
        )"#;
        assert!(
            Biomes::from_ron(ron).is_err(),
            "a belt reorder must be a load error"
        );
    }

    #[test]
    fn a_biome_using_an_unprovided_formation_is_rejected() {
        // `forest` is a valid formation name but is omitted from `formations`, so its ecology
        // constants would silently default to (0,0) — zero decay. That must be a load error.
        let ron = format!(
            r#"(
            belts: {BELTS}, provinces: ["humid"],
            formations: [(name: "water", flammability: 0.0, decay: 1.0)],
            biomes: [
                (name: "open water", formation: "water", belt: None),
                (name: "cloud forest", formation: "forest", belt: Some("tropical")),
            ],
            classifier: [],
        )"#
        );
        assert!(
            Biomes::from_ron(&ron).is_err(),
            "an unprovided formation must be a load error"
        );
    }
}
