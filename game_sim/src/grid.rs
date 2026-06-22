//! Addressing & adjacency for the cylindrical hex world, plus the
//! double-buffer used by dynamic fields. No physics lives here — this layer
//! only answers "what cell is at (col, row)?" and "who are its neighbours?".
//!
//! ## Topology
//! The world is a `width × height` grid of **pointy-top, odd-row offset**
//! hexes. It is a *cylinder*: columns wrap east–west (`col` is taken mod
//! `width`), while rows do **not** wrap — the top and bottom rows are the polar
//! edges. A tile there simply has fewer than six neighbours (the ones that
//! would fall off the pole are dropped), and every operator that averages over
//! neighbours renormalises by the count that actually exists.
//!
//! Neighbour math is delegated to `hexx`: each cell is converted to an axial
//! [`Hex`], `all_neighbors()` gives the six in cube space, and we convert back
//! to offset, apply the column wrap, and drop off-grid rows. Adjacency is
//! computed once at construction and cached.
//!
//! Each cached neighbour also carries the **unit direction** pointing toward it
//! in world space (+x ≈ east, +y ≈ south). That is what lets the physics layer
//! build the three spatial operators on top of this one:
//! - *diffuse* — average a field over the neighbours (direction unused);
//! - *advect* — push a field toward neighbours aligned with the wind vector
//!   (`dot(wind, dir)`);
//! - *flow* — send a field to the steepest-downhill neighbour.

use hexx::{Hex, HexOrientation, OffsetHexMode};

/// Hex layout used throughout: pointy-topped, odd rows shoved right.
const ORIENTATION: HexOrientation = HexOrientation::Pointy;
const OFFSET_MODE: OffsetHexMode = OffsetHexMode::Odd;
const SQRT3: f32 = 1.732_050_8;

/// A cached edge from a tile to one neighbour: its storage index plus the unit
/// vector pointing at it in world space.
#[derive(Clone, Copy, Debug)]
pub struct Link {
    pub to: usize,
    pub dir: [f32; 2],
}

/// World-space position of a hex under a unit pointy-top layout. Only relative
/// positions matter here (we take differences to get directions), so the
/// absolute origin and scale are irrelevant.
fn hex_world(hex: Hex) -> [f32; 2] {
    let (q, r) = (hex.x as f32, hex.y as f32);
    [SQRT3 * (q + r / 2.0), 1.5 * r]
}

/// A tile address in offset coordinates. `col` wraps; `row` is a latitude band
/// (row 0 = north edge, `height-1` = south edge).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Coord {
    pub col: i32,
    pub row: i32,
}

impl Coord {
    pub fn new(col: i32, row: i32) -> Self {
        Self { col, row }
    }
}

/// The grid's shape and precomputed adjacency. Cheap to clone is *not* a goal;
/// build one per world and share it by reference.
#[derive(Debug, Clone)]
pub struct Topology {
    width: i32,
    height: i32,
    /// `adjacency[i]` = cell `i`'s existing neighbour links (3–6 of them),
    /// deduplicated and self-excluded, each with its world-space direction.
    adjacency: Vec<Vec<Link>>,
}

impl Topology {
    /// Build a `width × height` cylinder and precompute neighbours.
    /// Both dimensions must be positive.
    pub fn new(width: i32, height: i32) -> Self {
        assert!(width > 0 && height > 0, "grid dimensions must be positive");
        let mut topo = Self {
            width,
            height,
            adjacency: Vec::new(),
        };
        let mut adjacency = Vec::with_capacity(topo.len());
        for row in 0..height {
            for col in 0..width {
                adjacency.push(topo.compute_neighbors(col, row));
            }
        }
        topo.adjacency = adjacency;
        topo
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    /// Number of tiles = `width * height`.
    pub fn len(&self) -> usize {
        (self.width * self.height) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Storage index of `(col, row)`. `col` is wrapped into range; `row` must be
    /// in `0..height`.
    pub fn index(&self, col: i32, row: i32) -> usize {
        debug_assert!((0..self.height).contains(&row), "row out of range");
        let col = col.rem_euclid(self.width);
        (row * self.width + col) as usize
    }

    pub fn index_of(&self, c: Coord) -> usize {
        self.index(c.col, c.row)
    }

    /// Inverse of [`index`](Self::index).
    pub fn coord(&self, i: usize) -> Coord {
        let i = i as i32;
        Coord::new(i % self.width, i / self.width)
    }

    /// Existing neighbours of cell `i` as links (precomputed): each carries the
    /// neighbour's storage index and the unit direction toward it.
    pub fn neighbors(&self, i: usize) -> &[Link] {
        &self.adjacency[i]
    }

    /// Iterator over every storage index `0..len`.
    pub fn indices(&self) -> std::ops::Range<usize> {
        0..self.len()
    }

    /// Latitude of a row in degrees: `+90` at the north edge, `-90` at the
    /// south. A single-row world sits on the equator.
    pub fn latitude_deg(&self, row: i32) -> f32 {
        if self.height <= 1 {
            return 0.0;
        }
        90.0 - 180.0 * (row as f32) / ((self.height - 1) as f32)
    }

    /// The six hex neighbours, wrapped E–W and clipped at the poles, each tagged
    /// with the unit direction toward it. The direction is taken from the *true*
    /// (unwrapped) neighbour hex, so a tile that wraps across the seam still
    /// points the correct way (e.g. "east"), even though its storage index lands
    /// at the far column.
    fn compute_neighbors(&self, col: i32, row: i32) -> Vec<Link> {
        let hex = Hex::from_offset_coordinates([col, row], OFFSET_MODE, ORIENTATION);
        let here = self.index(col, row);
        let origin = hex_world(hex);
        let mut out: Vec<Link> = Vec::with_capacity(6);
        for n in hex.all_neighbors() {
            let [nc, nr] = n.to_offset_coordinates(OFFSET_MODE, ORIENTATION);
            if !(0..self.height).contains(&nr) {
                continue; // fell off a pole — no neighbour there
            }
            let to = self.index(nc, nr); // index() wraps the column
            // On very narrow worlds the wrap can fold a neighbour onto self or
            // onto an already-seen cell; keep each distinct cell once.
            if to == here || out.iter().any(|l| l.to == to) {
                continue;
            }
            let w = hex_world(n);
            let (dx, dy) = (w[0] - origin[0], w[1] - origin[1]);
            let len = (dx * dx + dy * dy).sqrt();
            let dir = if len > 0.0 {
                [dx / len, dy / len]
            } else {
                [0.0, 0.0]
            };
            out.push(Link { to, dir });
        }
        out
    }
}

/// A double-buffered field for `dynamic` qualities. `Φ` reads the old values
/// (`front`) and writes the new ones (`back`), then [`swap`](Self::swap)s — so
/// no cell ever sees an already-updated neighbour and the tick is
/// order-independent.
#[derive(Debug, Clone)]
pub struct Buffered<T> {
    front: Vec<T>,
    back: Vec<T>,
}

impl<T: Clone> Buffered<T> {
    /// A buffer of `len` cells, both halves initialised to `value`.
    pub fn filled(value: T, len: usize) -> Self {
        Self {
            front: vec![value.clone(); len],
            back: vec![value; len],
        }
    }

    /// A buffer whose current (and shadow) values are `values` — used to seed a
    /// dynamic field from world generation.
    pub fn from_vec(values: Vec<T>) -> Self {
        Self {
            back: values.clone(),
            front: values,
        }
    }

    /// Current (old) values — what actors and observers read.
    pub fn front(&self) -> &[T] {
        &self.front
    }

    /// Mutable view of the current values, for a *local* in-place edit after the
    /// tick's spatial update has already swapped (e.g. fire consuming the
    /// biomass on its own tile). Not for neighbour-reading steps.
    pub fn front_mut(&mut self) -> &mut [T] {
        &mut self.front
    }

    /// The buffer being written this tick.
    pub fn back_mut(&mut self) -> &mut [T] {
        &mut self.back
    }

    /// Disjoint borrows of old (read) and new (write) in one call — the shape an
    /// update step wants: `let (old, new) = buf.read_write();`.
    pub fn read_write(&mut self) -> (&[T], &mut [T]) {
        (&self.front, &mut self.back)
    }

    /// Promote the freshly written buffer to current. Call once per field per
    /// tick, after writing every cell.
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.front, &mut self.back);
    }

    pub fn len(&self) -> usize {
        self.front.len()
    }

    pub fn is_empty(&self) -> bool {
        self.front.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interior_tile_has_six_neighbors() {
        let topo = Topology::new(10, 10);
        let i = topo.index(5, 5);
        assert_eq!(topo.neighbors(i).len(), 6, "interior hex should have 6");
    }

    #[test]
    fn polar_rows_have_fewer_neighbors() {
        let topo = Topology::new(10, 10);
        for col in 0..10 {
            let north = topo.index(col, 0);
            let south = topo.index(col, 9);
            assert!(topo.neighbors(north).len() < 6, "north edge must clip");
            assert!(topo.neighbors(south).len() < 6, "south edge must clip");
        }
    }

    #[test]
    fn columns_wrap_east_west() {
        let topo = Topology::new(10, 10);
        // A tile on the west edge must count a wrapped east-edge tile among its
        // neighbours (row 2 is interior, so no polar clipping interferes).
        let west = topo.index(0, 2);
        let neighbors = topo.neighbors(west);
        let wraps = neighbors
            .iter()
            .any(|l| topo.coord(l.to).col == topo.width() - 1);
        assert!(wraps, "west-edge tile should wrap to the east edge");
    }

    #[test]
    fn index_and_coord_round_trip() {
        let topo = Topology::new(7, 5);
        for i in topo.indices() {
            assert_eq!(topo.index_of(topo.coord(i)), i);
        }
    }

    #[test]
    fn neighbors_are_symmetric() {
        // If A is a neighbour of B, B is a neighbour of A.
        let topo = Topology::new(8, 6);
        for i in topo.indices() {
            for link in topo.neighbors(i) {
                assert!(
                    topo.neighbors(link.to).iter().any(|l| l.to == i),
                    "adjacency must be symmetric: {i} -> {} but not back",
                    link.to
                );
            }
        }
    }

    #[test]
    fn interior_directions_are_unit_and_balanced() {
        let topo = Topology::new(12, 12);
        let i = topo.index(6, 6);
        let links = topo.neighbors(i);
        assert_eq!(links.len(), 6);
        // Each direction is a unit vector...
        for l in links {
            let len = (l.dir[0] * l.dir[0] + l.dir[1] * l.dir[1]).sqrt();
            assert!((len - 1.0).abs() < 1e-3, "direction must be unit length");
        }
        // ...and the six of them cancel out (the star is symmetric).
        let sx: f32 = links.iter().map(|l| l.dir[0]).sum();
        let sy: f32 = links.iter().map(|l| l.dir[1]).sum();
        assert!(
            sx.abs() < 1e-3 && sy.abs() < 1e-3,
            "directions should cancel"
        );
    }

    #[test]
    fn latitude_spans_pole_to_pole() {
        let topo = Topology::new(4, 11);
        assert!((topo.latitude_deg(0) - 90.0).abs() < 1e-3);
        assert!((topo.latitude_deg(10) + 90.0).abs() < 1e-3);
        assert!(topo.latitude_deg(5).abs() < 1e-3); // equator
    }

    #[test]
    fn buffered_swap_exchanges_halves() {
        let mut buf = Buffered::filled(0.0_f32, 4);
        buf.back_mut().copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);
        buf.swap();
        assert_eq!(buf.front(), &[1.0, 2.0, 3.0, 4.0]);
    }
}
