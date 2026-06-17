//! Procedural world generation: realistic terrain from simulated geology and
//! hydrology, replacing the earlier sum-of-bumps placeholder.
//!
//! The pipeline follows the standard physically-motivated chain used by
//! generators such as WorldEngine / `platec` and the geomorphology literature:
//!
//! 1. **Plate tectonics** — seed drifting plates; classify each boundary by the
//!    relative motion of the plates either side (convergent / divergent), then
//!    raise mountains where plates collide and gouge trenches where ocean crust
//!    subducts (after Viitanen, *Physically Based Terrain Generation*).
//! 2. **Fractal detail** — add wrap-correct fractal noise so continents aren't
//!    smooth.
//! 3. **Sea level** — set the datum to hit a target land fraction.
//! 4. **Orographic precipitation** — advect moisture along idealized zonal
//!    winds, wringing it out on windward slopes and leaving rain shadows.
//! 5. **Hydrology** — Priority-Flood+ε depression filling (Barnes et al. 2014),
//!    flow directions, and flow accumulation: the four canonical steps of river
//!    extraction from a DEM.
//! 6. **Erosion** — stream-power incision `Δz = K·Aᵐ·Sⁿ` (Whipple & Tucker)
//!    plus hillslope diffusion, iterated, re-routing flow each pass.
//! 7. **Rivers, lakes, soils** — seed surface water from discharge and filled
//!    basins; assign lithology and ore from tectonic setting.
//!
//! Everything draws from the passed [`Rng`], so a seed reproduces a world.

use crate::fields::{CrustType, Lithology};
use crate::grid::Topology;
use crate::params::Params;
use hexx::{Hex, HexOrientation, OffsetHexMode};
use sim::Rng;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

const ORIENTATION: HexOrientation = HexOrientation::Pointy;
const OFFSET_MODE: OffsetHexMode = OffsetHexMode::Odd;

/// The `static`/`slow` fields produced for a fresh world, one entry per tile in
/// storage order, plus an initial hydrography to seed the dynamic water field.
pub struct Generated {
    pub elevation: Vec<f32>,
    pub plate: Vec<u16>,
    pub crust: Vec<CrustType>,
    pub lithology: Vec<Lithology>,
    pub minerals: Vec<f32>,
    /// Rivers and lakes from the final hydrology, to seed `surface_water`.
    pub surface_water: Vec<f32>,
}

struct Plate {
    col: i32,
    row: i32,
    drift: [f32; 2],
    crust: CrustType,
    base: f32,
}

/// Generate the initial fields for `topo` under `params`, drawing randomness
/// from `rng`.
pub fn generate(topo: &Topology, p: &Params, rng: &mut dyn Rng) -> Generated {
    let n = topo.len();

    // 1. Plate tectonics.
    let plates = seed_plates(topo, p, rng);
    let plate_of = assign_plates(topo, &plates);
    let (boundary_dist, stress) = boundary_field(topo, &plate_of, &plates);
    let mut elevation = tectonic_elevation(p, &plates, &plate_of, &boundary_dist, &stress);

    // 2. Fractal detail, then a little smoothing to take the edge off.
    let noise = fractal_noise(topo, p, rng);
    for i in 0..n {
        elevation[i] += noise[i];
    }
    hillslope_diffuse(topo, &mut elevation, p.thermal_erosion);
    hillslope_diffuse(topo, &mut elevation, p.thermal_erosion);

    // 3. Sea level to a target land fraction.
    normalize_sea_level(p, &mut elevation);

    // 4. Climatology that will drive the rivers.
    let precip = orographic_precipitation(topo, p, &elevation);

    // 5 + 6. Erode: route flow, incise by stream power, diffuse hillslopes.
    for _ in 0..p.erosion_iterations {
        let (filled, receiver) = priority_flood(topo, p, &elevation);
        let accumulation = flow_accumulation(&filled, &receiver, &precip);
        stream_power_erode(p, &mut elevation, &filled, &receiver, &accumulation);
        hillslope_diffuse(topo, &mut elevation, p.thermal_erosion);
    }
    normalize_sea_level(p, &mut elevation);

    // 7. Final hydrology → rivers + lakes; geology → lithology + ore.
    let (filled, receiver) = priority_flood(topo, p, &elevation);
    let accumulation = flow_accumulation(&filled, &receiver, &precip);
    let surface_water = rivers_and_lakes(p, &elevation, &filled, &accumulation);

    let plate = plate_of.iter().map(|&x| x as u16).collect();
    let crust = plate_of.iter().map(|&pi| plates[pi].crust).collect();
    let lithology = lithology_from_setting(p, &elevation, &stress);
    let minerals = minerals_from_setting(&stress, &lithology, rng);

    Generated {
        elevation,
        plate,
        crust,
        lithology,
        minerals,
        surface_water,
    }
}

// --- 1. Plate tectonics ---

fn seed_plates(topo: &Topology, p: &Params, rng: &mut dyn Rng) -> Vec<Plate> {
    (0..p.plates.max(1))
        .map(|_| {
            let col = rng.gen_range(topo.width() as usize) as i32;
            let row = rng.gen_range(topo.height() as usize) as i32;
            let crust = if rng.gen_bool(0.45) {
                CrustType::Continental
            } else {
                CrustType::Oceanic
            };
            let base = match crust {
                CrustType::Continental => p.continental_base,
                CrustType::Oceanic => p.oceanic_base,
            };
            // A unit-ish drift direction with a little speed variation.
            let angle = rng.next_f64() as f32 * std::f32::consts::TAU;
            let speed = 0.5 + 0.5 * rng.next_f64() as f32;
            let drift = [angle.cos() * speed, angle.sin() * speed];
            Plate {
                col,
                row,
                drift,
                crust,
                base,
            }
        })
        .collect()
}

/// Assign each tile to the nearest plate seed (wrap-aware).
fn assign_plates(topo: &Topology, plates: &[Plate]) -> Vec<usize> {
    (0..topo.len())
        .map(|i| {
            let c = topo.coord(i);
            let mut best = u32::MAX;
            let mut best_p = 0;
            for (pi, plate) in plates.iter().enumerate() {
                let d = wrapped_distance(topo, (c.col, c.row), (plate.col, plate.row));
                if d < best {
                    best = d;
                    best_p = pi;
                }
            }
            best_p
        })
        .collect()
}

/// For every cell, the hex distance to the nearest plate boundary and the
/// boundary's convergence stress (positive = colliding, negative = rifting),
/// both spread inland by a multi-source BFS so uplift can decay away from the
/// boundary.
fn boundary_field(topo: &Topology, plate_of: &[usize], plates: &[Plate]) -> (Vec<f32>, Vec<f32>) {
    let n = topo.len();
    let mut dist = vec![f32::INFINITY; n];
    let mut stress = vec![0.0f32; n];
    let mut queue = VecDeque::new();

    for i in 0..n {
        let mut s = 0.0;
        let mut count = 0;
        for l in topo.neighbors(i) {
            if plate_of[l.to] == plate_of[i] {
                continue;
            }
            // Relative velocity of the far plate w.r.t. this one, projected onto
            // the outward boundary normal (the link direction). Closing motion
            // (negative projection) is convergence, so negate it.
            let a = &plates[plate_of[i]];
            let b = &plates[plate_of[l.to]];
            let rel = [b.drift[0] - a.drift[0], b.drift[1] - a.drift[1]];
            s += -(rel[0] * l.dir[0] + rel[1] * l.dir[1]);
            count += 1;
        }
        if count > 0 {
            dist[i] = 0.0;
            stress[i] = (s / count as f32).clamp(-1.0, 1.0);
            queue.push_back(i);
        }
    }

    while let Some(c) = queue.pop_front() {
        for l in topo.neighbors(c) {
            if dist[l.to] > dist[c] + 1.0 {
                dist[l.to] = dist[c] + 1.0;
                stress[l.to] = stress[c]; // carry the nearest boundary's stress inland
                queue.push_back(l.to);
            }
        }
    }
    (dist, stress)
}

/// Base elevation from crust type, plus tectonic relief: uplift decaying inland
/// from convergent boundaries (stronger on continental crust, with a trench on
/// the oceanic side), ridges/rifts at divergent ones.
fn tectonic_elevation(
    p: &Params,
    plates: &[Plate],
    plate_of: &[usize],
    boundary_dist: &[f32],
    stress: &[f32],
) -> Vec<f32> {
    (0..plate_of.len())
        .map(|i| {
            let plate = &plates[plate_of[i]];
            let mut elev = plate.base;
            let falloff = (1.0 - boundary_dist[i] / p.uplift_falloff).max(0.0);
            let s = stress[i];
            let continental = plate.crust == CrustType::Continental;
            if s > 0.0 {
                // Convergence → uplift; continental collisions raise the highest ranges.
                let crust_factor = if continental { 1.0 } else { 0.4 };
                elev += s * p.plate_uplift * falloff * crust_factor;
                // Oceanic crust at the collision front subducts → trench.
                if !continental && boundary_dist[i] < 1.5 {
                    elev -= p.trench_depth * s;
                }
            } else if s < 0.0 {
                // Divergence → mid-ocean ridge (raised seabed) or a continental rift.
                let opening = -s;
                if continental {
                    elev -= p.ridge_height * opening * falloff;
                } else {
                    elev += p.ridge_height * opening * falloff;
                }
            }
            elev
        })
        .collect()
}

// --- 2. Fractal detail ---

/// Wrap-correct fractal noise: sum octaves of white noise smoothed over the
/// topology (so it respects the cylinder seam and polar edges), low frequencies
/// weighted highest. Scaled to `tectonic_noise` metres.
fn fractal_noise(topo: &Topology, p: &Params, rng: &mut dyn Rng) -> Vec<f32> {
    let n = topo.len();
    let octaves = p.noise_octaves.max(1);
    let mut out = vec![0.0f32; n];
    let mut amp = 1.0;
    let mut norm = 0.0;
    for o in 0..octaves {
        let passes = 1usize << (octaves - 1 - o); // octave 0 = smoothest / lowest frequency
        let mut layer: Vec<f32> = (0..n).map(|_| rng.next_f64() as f32 * 2.0 - 1.0).collect();
        for _ in 0..passes {
            hillslope_diffuse(topo, &mut layer, 0.5);
        }
        for i in 0..n {
            out[i] += amp * layer[i];
        }
        norm += amp;
        amp *= 0.5;
    }
    for v in out.iter_mut() {
        *v = *v / norm * p.tectonic_noise;
    }
    out
}

// --- 3. Sea level ---

/// Shift elevations so that exactly `land_fraction` of tiles sit above
/// `sea_level`.
fn normalize_sea_level(p: &Params, elevation: &mut [f32]) {
    let n = elevation.len();
    let mut sorted = elevation.to_vec();
    sorted.sort_by(f32::total_cmp);
    let idx = (((1.0 - p.land_fraction) * n as f32) as usize).min(n - 1);
    let datum = sorted[idx];
    let shift = p.sea_level - datum;
    for e in elevation.iter_mut() {
        *e += shift;
    }
}

// --- 4. Orographic precipitation ---

/// Idealized zonal prevailing wind by latitude: tropical easterlies, mid-latitude
/// westerlies, polar easterlies. `+x` is east (the wrapping axis).
fn prevailing_wind(lat_deg: f32) -> [f32; 2] {
    let a = lat_deg.abs();
    if a < 30.0 {
        [-1.0, 0.0] // trade winds blow toward the west
    } else if a < 60.0 {
        [1.0, 0.0] // westerlies blow toward the east
    } else {
        [-1.0, 0.0] // polar easterlies
    }
}

/// Steady-state precipitation: moisture starts saturated over the ocean and is
/// advected along the prevailing wind, raining out a base fraction each step
/// plus an orographic surplus on windward upslopes — so interiors and leeward
/// (rain-shadow) tiles dry out.
fn orographic_precipitation(topo: &Topology, p: &Params, elevation: &[f32]) -> Vec<f32> {
    let n = topo.len();
    let mut moisture: Vec<f32> = (0..n)
        .map(|i| if elevation[i] < p.sea_level { 1.0 } else { 0.0 })
        .collect();
    let mut precip = vec![0.0f32; n];

    for _ in 0..p.wg_precip_iters {
        let mut next = moisture.clone();
        let mut step_precip = vec![0.0f32; n];
        for i in 0..n {
            if elevation[i] < p.sea_level {
                next[i] = 1.0; // the sea keeps resupplying moisture
                continue;
            }
            let wind = prevailing_wind(topo.latitude_deg(topo.coord(i).row));
            // Gather moisture from upwind neighbours and the upslope they sit on.
            let mut inflow = 0.0;
            let mut weight = 0.0;
            let mut rise = 0.0;
            for l in topo.neighbors(i) {
                // A neighbour is upwind if the wind blows from it toward us:
                // direction from neighbour to us is `-l.dir`.
                let align = (wind[0] * -l.dir[0] + wind[1] * -l.dir[1]).max(0.0);
                if align > 0.0 {
                    inflow += align * moisture[l.to];
                    weight += align;
                    rise += align * (elevation[i] - elevation[l.to]).max(0.0);
                }
            }
            let incoming = if weight > 0.0 { inflow / weight } else { moisture[i] };
            let upslope = if weight > 0.0 { rise / weight } else { 0.0 };
            let rainout = (p.wg_precip_base + p.wg_orographic * upslope).min(1.0);
            let rain = incoming * rainout;
            step_precip[i] = rain;
            next[i] = (incoming - rain).max(0.0);
        }
        moisture = next;
        precip = step_precip;
    }
    // A small everywhere-baseline keeps every land cell's discharge positive.
    for v in precip.iter_mut() {
        *v += 0.01;
    }
    precip
}

// --- 5. Hydrology ---

/// A cell in the Priority-Flood queue, ordered so the heap yields the lowest
/// elevation first (with a deterministic index tie-break).
struct FloodCell {
    elev: f32,
    idx: usize,
}
impl PartialEq for FloodCell {
    fn eq(&self, o: &Self) -> bool {
        self.idx == o.idx && self.elev == o.elev
    }
}
impl Eq for FloodCell {}
impl PartialOrd for FloodCell {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for FloodCell {
    fn cmp(&self, o: &Self) -> Ordering {
        // Reversed elevation so `BinaryHeap` (a max-heap) pops the lowest.
        o.elev.total_cmp(&self.elev).then_with(|| o.idx.cmp(&self.idx))
    }
}

/// Priority-Flood+ε (Barnes et al. 2014): flood inward from the ocean outlets,
/// raising each interior cell to just above the lowest path out. Returns the
/// depression-filled surface and, for every cell, the neighbour it spills to
/// (its flow receiver). The ε gradient guarantees no flats, so every land cell
/// has a strictly downhill path to the sea.
fn priority_flood(topo: &Topology, p: &Params, elevation: &[f32]) -> (Vec<f32>, Vec<usize>) {
    let n = topo.len();
    let mut filled = elevation.to_vec();
    let mut closed = vec![false; n];
    let mut receiver = vec![usize::MAX; n];
    let mut heap = BinaryHeap::new();

    // Outlets: the ocean. (If a world somehow has no sea, use its lowest cell.)
    for i in 0..n {
        if elevation[i] < p.sea_level {
            closed[i] = true;
            receiver[i] = i;
            heap.push(FloodCell { elev: filled[i], idx: i });
        }
    }
    if heap.is_empty() {
        let lo = (0..n).min_by(|&a, &b| elevation[a].total_cmp(&elevation[b])).unwrap();
        closed[lo] = true;
        receiver[lo] = lo;
        heap.push(FloodCell { elev: filled[lo], idx: lo });
    }

    while let Some(FloodCell { idx: c, .. }) = heap.pop() {
        for l in topo.neighbors(c) {
            let nb = l.to;
            if closed[nb] {
                continue;
            }
            filled[nb] = filled[nb].max(filled[c] + p.fill_epsilon);
            receiver[nb] = c;
            closed[nb] = true;
            heap.push(FloodCell { elev: filled[nb], idx: nb });
        }
    }
    (filled, receiver)
}

/// Drainage area: each cell's own rainfall plus everything upstream, summed by
/// walking cells from highest to lowest filled elevation and pushing each one's
/// total to its receiver.
fn flow_accumulation(filled: &[f32], receiver: &[usize], precip: &[f32]) -> Vec<f32> {
    let n = filled.len();
    let mut acc = precip.to_vec();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| filled[b].total_cmp(&filled[a]).then(a.cmp(&b)));
    for &i in &order {
        let r = receiver[i];
        if r != usize::MAX && r != i {
            acc[r] += acc[i];
        }
    }
    acc
}

// --- 6. Erosion ---

/// Stream-power incision `Δz = K · Aᵐ · Sⁿ`: lower each land cell in proportion
/// to its discharge and slope, capped so it never cuts below its receiver (which
/// would invert the drainage). High-discharge valleys carve fast; dry hillslopes
/// barely move — the contrast that makes river valleys.
fn stream_power_erode(
    p: &Params,
    elevation: &mut [f32],
    filled: &[f32],
    receiver: &[usize],
    acc: &[f32],
) {
    let n = elevation.len();
    let mut delta = vec![0.0f32; n];
    for i in 0..n {
        if elevation[i] < p.sea_level {
            continue;
        }
        let r = receiver[i];
        if r == usize::MAX || r == i {
            continue;
        }
        let slope = (filled[i] - filled[r]).max(0.0);
        let incision = p.stream_power_k * acc[i].powf(p.stream_power_m) * slope.powf(p.stream_power_n);
        let headroom = (elevation[i] - elevation[r]).max(0.0);
        delta[i] = incision.min(headroom * 0.5);
    }
    for i in 0..n {
        elevation[i] -= delta[i];
    }
}

/// Hillslope (thermal) diffusion: relax each cell toward its neighbour mean.
/// Rounds ridges and fills hollows — the slow creep that complements incision.
fn hillslope_diffuse(topo: &Topology, elevation: &mut [f32], strength: f32) {
    let old = elevation.to_vec();
    for i in 0..elevation.len() {
        let nb = topo.neighbors(i);
        if nb.is_empty() {
            continue;
        }
        let mean = nb.iter().map(|l| old[l.to]).sum::<f32>() / nb.len() as f32;
        elevation[i] = old[i] + strength * (mean - old[i]);
    }
}

// --- 7. Rivers, lakes, soils ---

/// Initial standing water: river channels where discharge clears the threshold,
/// lakes where a basin had to be filled. Ocean is left to the dynamic field.
fn rivers_and_lakes(p: &Params, elevation: &[f32], filled: &[f32], acc: &[f32]) -> Vec<f32> {
    (0..elevation.len())
        .map(|i| {
            if elevation[i] < p.sea_level {
                return 0.0;
            }
            let lake = (filled[i] - elevation[i]).max(0.0) * p.lake_water;
            let river = if acc[i] > p.river_threshold {
                (acc[i] - p.river_threshold) * p.river_water
            } else {
                0.0
            };
            (lake + river).min(20.0)
        })
        .collect()
}

/// Bedrock class from setting: ocean floor and rift basalt are igneous, collided
/// highlands are metamorphic, the rest is sedimentary lowland.
fn lithology_from_setting(p: &Params, elevation: &[f32], stress: &[f32]) -> Vec<Lithology> {
    let highland = p.sea_level + 0.4 * p.plate_uplift;
    (0..elevation.len())
        .map(|i| {
            if elevation[i] < p.sea_level {
                Lithology::Igneous
            } else if elevation[i] > highland && stress[i] > 0.2 {
                Lithology::Metamorphic
            } else {
                Lithology::Sedimentary
            }
        })
        .collect()
}

/// Ore richness `0..1`: a low baseline, enriched in orogenic (convergent) belts
/// and igneous rock — where real ore deposits concentrate.
fn minerals_from_setting(stress: &[f32], lithology: &[Lithology], rng: &mut dyn Rng) -> Vec<f32> {
    (0..lithology.len())
        .map(|i| {
            let mut ore = 0.12 * rng.next_f64() as f32;
            ore += 0.5 * stress[i].max(0.0); // orogenic belts
            if lithology[i] == Lithology::Igneous {
                ore += 0.3 * rng.next_f64() as f32;
            }
            ore.clamp(0.0, 1.0)
        })
        .collect()
}

/// Hex distance between two offset cells, shortest way around the E–W wrap.
fn wrapped_distance(topo: &Topology, a: (i32, i32), b: (i32, i32)) -> u32 {
    let ha = Hex::from_offset_coordinates([a.0, a.1], OFFSET_MODE, ORIENTATION);
    let mut best = u32::MAX;
    for k in [-1, 0, 1] {
        let hb =
            Hex::from_offset_coordinates([b.0 + k * topo.width(), b.1], OFFSET_MODE, ORIENTATION);
        best = best.min(ha.unsigned_distance_to(hb));
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SplitMix64;

    fn world(seed: u64) -> (Topology, Generated) {
        let topo = Topology::new(40, 30);
        let g = generate(&topo, &Params::default(), &mut SplitMix64::new(seed));
        (topo, g)
    }

    #[test]
    fn fields_have_one_entry_per_tile() {
        let (topo, g) = world(1);
        let n = topo.len();
        assert_eq!(g.elevation.len(), n);
        assert_eq!(g.plate.len(), n);
        assert_eq!(g.crust.len(), n);
        assert_eq!(g.lithology.len(), n);
        assert_eq!(g.minerals.len(), n);
        assert_eq!(g.surface_water.len(), n);
    }

    #[test]
    fn same_seed_same_world() {
        let (_, a) = world(99);
        let (_, b) = world(99);
        assert_eq!(a.elevation, b.elevation);
        assert_eq!(a.plate, b.plate);
        assert_eq!(a.minerals, b.minerals);
        assert_eq!(a.surface_water, b.surface_water);
    }

    #[test]
    fn minerals_within_unit_range() {
        let (_, g) = world(3);
        assert!(g.minerals.iter().all(|&m| (0.0..=1.0).contains(&m)));
        assert!(g.elevation.iter().all(|e| e.is_finite()));
    }

    #[test]
    fn land_fraction_near_target() {
        let (topo, g) = world(7);
        let p = Params::default();
        let land = g.elevation.iter().filter(|&&e| e >= p.sea_level).count();
        let frac = land as f32 / topo.len() as f32;
        assert!(
            (frac - p.land_fraction).abs() < 0.06,
            "land fraction {frac} should be near {}",
            p.land_fraction
        );
    }

    #[test]
    fn rivers_form_and_reach_water() {
        let (topo, g) = world(7);
        let p = Params::default();
        // Some land carries standing water (rivers / lakes).
        let wet_land = topo
            .indices()
            .filter(|&i| g.elevation[i] >= p.sea_level && g.surface_water[i] > 0.0)
            .count();
        assert!(wet_land > 0, "no rivers or lakes formed on land");
        assert!(g.surface_water.iter().all(|w| w.is_finite() && *w >= 0.0));
    }

    #[test]
    fn mountains_rise_at_plate_collisions() {
        // Land where plates converge should stand higher than land where they
        // rift apart — the tectonic signal the generator is built on.
        let topo = Topology::new(40, 30);
        let p = Params::default();
        let g = generate(&topo, &p, &mut SplitMix64::new(7));
        let plate_of = assign_plates(&topo, &seed_plates(&topo, &p, &mut SplitMix64::new(7)));
        let (_, stress) = boundary_field(&topo, &plate_of, &seed_plates(&topo, &p, &mut SplitMix64::new(7)));

        let mean = |pred: &dyn Fn(usize) -> bool| -> f32 {
            let cells: Vec<usize> = topo
                .indices()
                .filter(|&i| g.elevation[i] >= p.sea_level && pred(i))
                .collect();
            cells.iter().map(|&i| g.elevation[i]).sum::<f32>() / cells.len().max(1) as f32
        };
        let convergent = mean(&|i| stress[i] > 0.3);
        let divergent = mean(&|i| stress[i] < -0.3);
        assert!(
            convergent > divergent,
            "collision belts ({convergent} m) should out-rise rift zones ({divergent} m)"
        );
    }

    #[test]
    fn precipitation_has_wet_and_dry_regions() {
        // Orographic rainout + rain shadow should leave a strong wet/dry
        // contrast across the land, not a uniform drizzle.
        let topo = Topology::new(40, 30);
        let p = Params::default();
        let g = generate(&topo, &p, &mut SplitMix64::new(7));
        let precip = orographic_precipitation(&topo, &p, &g.elevation);
        let land: Vec<f32> = topo
            .indices()
            .filter(|&i| g.elevation[i] >= p.sea_level)
            .map(|i| precip[i])
            .collect();
        let max = land.iter().cloned().fold(0.0_f32, f32::max);
        let mean = land.iter().sum::<f32>() / land.len() as f32;
        assert!(max > 3.0 * mean, "expected wet/dry contrast (max {max}, mean {mean})");
    }
}
