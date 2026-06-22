//! The **extensible feature-art registry**: how a placed feature (a thorp, a temple, a barrow,
//! a beacon) is drawn on the map. One small table maps a feature *kind* to a [`Plan`]; to give
//! a new kind its own look, add a row to [`plan_for`] (or extend the category fallback). All of
//! it is a view concern — it lives here in `app`, never in the Bevy-free sim crates — and reads
//! only discovered features, so it honours the fog of war.

use crate::layout::{HEX_R, tile_top, tile_world};
use crate::props::{Prop, PropLibrary, Rng, tile_seed};
use crate::scatter::Decor;
use agents::{Category, Coord, Simulation};
use bevy::prelude::*;
use std::collections::HashSet;
use std::f32::consts::TAU;

/// How a discovered feature shows on the map.
pub enum Plan {
    /// A cluster of dwellings, scaled by tier (1 = hamlet … 4 = city).
    Settlement(u8),
    /// A single structure at (near) the tile centre.
    Landmark(Prop),
    /// Nothing built — the terrain, vegetation, and floating label carry it (most natural
    /// wonders: waterfalls, springs, groves, hazards).
    Nothing,
}

/// The registry. Signature kinds get a bespoke look here; everything else falls back by
/// category. **This match is the extension point** — one row per kind.
pub fn plan_for(name: &str, category: Category) -> Plan {
    use Prop::*;
    match name {
        "pale_beacon" => return Plan::Landmark(Beacon),
        "monastery" | "temple" | "high_temple" | "temple_of_the_demiurge" => {
            return Plan::Landmark(Temple);
        }
        "broken_tower" | "ruined_lighthouse" | "seers_seat" => return Plan::Landmark(Tower),
        "standing_stones" | "choir_of_the_drowned" => return Plan::Landmark(StoneRing),
        "pillar_of_the_boast"
        | "effaced_monument"
        | "salt_pillars"
        | "gate_of_the_seven"
        | "toppled_arch"
        | "archon_throne" => {
            return Plan::Landmark(Obelisk);
        }
        "barrow" | "bone_orchard" | "serpent_mound" | "plague_pit" => return Plan::Landmark(Cairn),
        "frontier_fort" | "barons_keep" | "warlords_redoubt" | "ruined_keep" => {
            return Plan::Landmark(Keep);
        }
        _ => {}
    }
    match category {
        Category::Community => Plan::Settlement(settlement_tier(name)),
        Category::Court => Plan::Landmark(court_prop(name)),
        Category::Ruin => Plan::Landmark(Ruin),
        Category::Wilderness => Plan::Nothing,
    }
}

/// Settlement size from its name (the farming hierarchy thorp→city, plus the themed kinds).
fn settlement_tier(name: &str) -> u8 {
    if name.contains("city") {
        4
    } else if name.contains("town") {
        3
    } else if name.contains("village")
        || name.contains("market")
        || name.contains("waystation")
        || name.contains("roost")
        || name.contains("colony")
    {
        2
    } else {
        1
    }
}

/// Which structure a court reads as.
fn court_prop(name: &str) -> Prop {
    if name.contains("temple")
        || name.contains("shrine")
        || name.contains("almshouse")
        || name.contains("mendicant")
        || name.contains("hall")
    {
        Prop::Temple
    } else if name.contains("tower") || name.contains("seat") {
        Prop::Tower
    } else {
        Prop::Keep // a generic seat of power: keep, guild, court, throne, redoubt
    }
}

/// Tiles whose discovered features have already been built (keyed by coord + feature kind id),
/// so each feature raises its structures exactly once.
#[derive(Resource, Default)]
pub struct Built(pub HashSet<(i32, i32, usize)>);

/// Raise the structures for newly-discovered features on the given tiles — the tiles revealed this
/// frame, plus the avatar's own tile (so a feature uncovered by *searching* an already-explored tile
/// is built too). The `built` set makes each feature raise its structures exactly once.
pub fn build_on(
    commands: &mut Commands,
    lib: &PropLibrary,
    sim: &Simulation,
    built: &mut Built,
    tiles: impl IntoIterator<Item = Coord>,
) {
    let gw = sim.substrate();
    let cat = sim.feature_catalog();
    for c in tiles {
        let feats = sim.features_at(c);
        if feats.is_empty() {
            continue;
        }
        let centre = tile_world(c.col, c.row);
        let top = tile_top(gw, c);
        for f in feats {
            if !f.discovered || !built.0.insert((c.col, c.row, f.kind)) {
                continue;
            }
            let def = cat.def(f.kind);
            let mut rng = Rng::new(tile_seed(c.col, c.row, 0x8B17_u64 ^ f.kind as u64));
            match plan_for(&def.name, def.category) {
                Plan::Settlement(tier) => {
                    place_settlement(commands, lib, centre, top, tier, &mut rng)
                }
                Plan::Landmark(prop) => {
                    // Fan stacked landmarks off-centre by a kind-derived direction so a court and
                    // a ruin sharing one tile don't occupy the very same spot.
                    let ang = f.kind as f32 * 2.399_963; // golden angle
                    let off =
                        Vec2::new(ang.cos(), ang.sin()) * HEX_R * 0.16 * (f.kind.min(3) as f32);
                    place_one(
                        commands,
                        lib,
                        prop,
                        centre + off,
                        top,
                        &mut rng,
                        (0.95, 1.15),
                    );
                }
                Plan::Nothing => {}
            }
        }
    }
}

/// A scatter of dwellings around the tile centre, denser and taller for a city than a thorp.
fn place_settlement(
    commands: &mut Commands,
    lib: &PropLibrary,
    centre: Vec2,
    top: f32,
    tier: u8,
    rng: &mut Rng,
) {
    let count = match tier {
        1 => 2 + rng.int(3),
        2 => 4 + rng.int(3),
        3 => 6 + rng.int(4),
        _ => 9 + rng.int(5),
    };
    for _ in 0..count {
        let a = rng.range(0.0, TAU);
        let r = HEX_R * rng.range(0.12, 0.66);
        let pos = centre + Vec2::new(r * a.cos(), r * a.sin());
        let prop = if rng.chance(0.45) {
            Prop::Hut
        } else {
            Prop::House
        };
        place_one(commands, lib, prop, pos, top, rng, (0.85, 1.1));
    }
    // A hall anchors the larger centres.
    if tier >= 3 {
        place_one(commands, lib, Prop::Hall, centre, top, rng, (0.95, 1.1));
    }
}

fn place_one(
    commands: &mut Commands,
    lib: &PropLibrary,
    prop: Prop,
    pos: Vec2,
    top: f32,
    rng: &mut Rng,
    scale: (f32, f32),
) {
    let Some(mesh) = lib.pick(prop, rng) else {
        return;
    };
    commands.spawn((
        Decor,
        Mesh3d(mesh),
        MeshMaterial3d(lib.material.clone()),
        Transform {
            translation: Vec3::new(pos.x, top, pos.y),
            rotation: Quat::from_rotation_y(rng.range(0.0, TAU)),
            scale: Vec3::splat(rng.range(scale.0, scale.1)),
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry always yields a plan, for both signature kinds and unknown ones (via the
    /// category fallback) — the extension point can't leave a discovered feature unmapped.
    #[test]
    fn registry_maps_signature_and_fallback() {
        assert!(matches!(
            plan_for("city", Category::Community),
            Plan::Settlement(4)
        ));
        assert!(matches!(
            plan_for("thorp", Category::Community),
            Plan::Settlement(1)
        ));
        assert!(matches!(
            plan_for("pale_beacon", Category::Wilderness),
            Plan::Landmark(Prop::Beacon)
        ));
        assert!(matches!(
            plan_for("standing_stones", Category::Wilderness),
            Plan::Landmark(Prop::StoneRing)
        ));
        assert!(matches!(
            plan_for("temple_of_the_demiurge", Category::Court),
            Plan::Landmark(Prop::Temple)
        ));
        // Unknown kinds still resolve through the category fallback.
        assert!(matches!(
            plan_for("some_new_wonder", Category::Wilderness),
            Plan::Nothing
        ));
        assert!(matches!(
            plan_for("some_new_ruin", Category::Ruin),
            Plan::Landmark(Prop::Ruin)
        ));
        assert!(matches!(
            plan_for("some_new_court", Category::Court),
            Plan::Landmark(_)
        ));
    }
}
