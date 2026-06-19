//! **Incremental explored-state + chunked ground.** Two things the whole view layer leans on so
//! that walking stays smooth no matter how much has been uncovered:
//!
//! * [`Explored`] — the revealed tiles as an O(1)-lookup set plus the *delta* revealed this frame,
//!   diffed from the sim once per frame ([`track_explored`]). Every other system reads this instead
//!   of rescanning the whole explored set itself.
//! * [`Ground`] — the 3-D surface split into fixed square chunks; a newly-revealed tile rebuilds
//!   only the chunk it lands in (plus boundary neighbours), never the whole world mesh.
//!
//! Pure view: it reads the explored set and the substrate and writes nothing back to the sim.

use crate::layout::tile_world;
use crate::{Game, MapMesh, RenderAssets};
use agents::Coord;
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

/// Tiles per chunk side. A revealed tile rebuilds at most a 3×3 block of these.
const CHUNK: i32 = 12;

/// The shared explored-tile state, grown incrementally so no system rescans the whole set.
#[derive(Resource, Default)]
pub struct Explored {
    /// Every revealed tile, for O(1) "is this explored?" checks.
    pub set: HashSet<(i32, i32)>,
    /// Tiles revealed *this frame* — the delta the per-tile builders consume.
    pub fresh: Vec<Coord>,
    count: usize,
}

/// Diff the sim's explored set once per frame, recording the newly-revealed tiles. This is the
/// single O(n) pass that replaces the per-system rescans — and it only does real work on the steps
/// that actually lift fog. Must run before the systems that read [`Explored`].
pub fn track_explored(mut ex: ResMut<Explored>, game: NonSend<Game>) {
    ex.fresh.clear();
    let count = game.sim.player_explored_count();
    if count == ex.count {
        return;
    }
    ex.count = count;
    for c in game.sim.player_explored() {
        if ex.set.insert((c.col, c.row)) {
            ex.fresh.push(c);
        }
    }
}

/// Accumulated ground geometry, grown chunk-by-chunk as the fog lifts.
#[derive(Resource, Default)]
pub struct Ground {
    /// Every revealed tile's top height, keyed by `qkey(world centre)` — the neighbour heights the
    /// wall builder needs, shared across chunks so seams stay seamless.
    heights: HashMap<(i32, i32), f32>,
    /// Revealed tiles grouped by chunk, so a rebuild touches only that chunk's tiles.
    tiles: HashMap<(i32, i32), Vec<Coord>>,
    /// The live mesh entity per chunk (despawned + respawned on rebuild).
    chunks: HashMap<(i32, i32), Entity>,
}

fn chunk_of(col: i32, row: i32) -> (i32, i32) {
    (col.div_euclid(CHUNK), row.div_euclid(CHUNK))
}

/// Fold this frame's freshly-revealed tiles into the surface, rebuilding only the chunks they
/// touch. A no-op on frames that reveal nothing.
pub fn rebuild_ground(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    ra: Res<RenderAssets>,
    mut ground: ResMut<Ground>,
    ex: Res<Explored>,
    game: NonSend<Game>,
) {
    if ex.fresh.is_empty() {
        return;
    }
    let mut dirty: HashSet<(i32, i32)> = HashSet::new();
    for &c in &ex.fresh {
        let h = crate::world_mesh::top_of(&game.sim, c);
        ground.heights.insert(crate::world_mesh::qkey(tile_world(c.col, c.row)), h);
        let ch = chunk_of(c.col, c.row);
        ground.tiles.entry(ch).or_default().push(c);
        dirty.insert(ch);
        // A tile on a chunk edge can bury a wall in the neighbouring chunk, so rebuild those too.
        let (lx, ly) = (c.col.rem_euclid(CHUNK), c.row.rem_euclid(CHUNK));
        if lx == 0 || lx == CHUNK - 1 || ly == 0 || ly == CHUNK - 1 {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    dirty.insert((ch.0 + dx, ch.1 + dy));
                }
            }
        }
    }

    let to_build: Vec<(i32, i32)> = dirty.into_iter().filter(|ch| ground.tiles.contains_key(ch)).collect();
    for ch in to_build {
        if let Some(old) = ground.chunks.remove(&ch) {
            commands.entity(old).despawn();
        }
        let coords = ground.tiles.get(&ch).cloned().unwrap_or_default();
        let mesh = crate::world_mesh::build_mesh(&game.sim, &coords, &ground.heights);
        let e = commands
            .spawn((MapMesh, Mesh3d(meshes.add(mesh)), MeshMaterial3d(ra.map_mat.clone()), Transform::IDENTITY))
            .id();
        ground.chunks.insert(ch, e);
    }
}
