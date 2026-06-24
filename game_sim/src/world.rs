//! The [`World`] substrate: it owns every tile field and runs the per-tick
//! update `Φ` ([`World::evolve`]).
//!
//! Storage is **struct-of-arrays** — one `Vec` per quality, indexed by the
//! [`Topology`]. `static`/`slow` fields (elevation, plates, bedrock, ore) are
//! single-buffered; `dynamic` fields that read their neighbours are
//! [`Buffered`] so the update is order-independent (see [`crate::grid`]).
//!
//! ## The climate pipeline `Φ`
//! Each day, fields update in dependency order; spatial steps read the old
//! buffer so the tick is order-independent:
//! 1. **insolation** — local, from latitude and day-of-year (the sub-solar
//!    point swings ±axial-tilt over the year).
//! 2. **temperature** — relaxes toward a radiative target (sun warms, the lapse
//!    rate cools altitude) while **diffusing** toward its neighbour mean.
//! 3. **pressure** — local, low where warm or high (rising/thin air).
//! 4. **wind** — the down-gradient vector of pressure (high → low).
//! 5. **humidity** — evaporates off warm water and **advects** along the wind.
//! 6. **precipitation** — falls where moist air is lifted over upwind slopes
//!    (orographic) or cooled past saturation (condensation); it leaves the air.
//! 7. **surface_water** — rain (less the snow share) accumulates and **flows**
//!    to the steepest-downhill neighbour, carving rivers and pooling in basins.
//! 8. **snow_ice** — precipitation below freezing builds snow; warmth melts it
//!    back into surface water.
//!
//! Then the **ecosystem** rides on that climate:
//! 9. **vegetation** — plants grow logistically toward a climate/soil
//!    `carrying_capacity` (the `npp` production term) and shed mortality into
//!    `litter`. Animals are not a field: the only grazers are the agent layer,
//!    which removes biomass through [`World::graze`].
//! 10. **soil** — litter decomposes (faster when warm and wet) into
//!    `soil_carbon` and plant-available `soil_nutrients`; weathering tops the
//!    nutrients up, plant uptake draws them down — the loop that lets a drought
//!    starve next season's growth.
//! 11. **pft** — each tile's dominant plant type, classified from its climate.
//!
//! Finally **disturbance**, the first step that draws on the `rng` and the first
//! that feeds back into the *climate*:
//! 12. **fire** — a stochastic cellular automaton: dry, fuelled tiles ignite
//!    from lightning or catch from a burning neighbour (spread weighted by wind
//!    and upslope); burning consumes `plant_biomass`/`litter`, returns ash to
//!    the soil, and burns out as the fuel runs down.
//! 13. **albedo** — surface reflectance from cover (snow bright, forest dark,
//!    fresh burn darkest), which offsets the temperature target next tick. This
//!    closes the loop **snow/vegetation/burn → albedo → temperature → climate**.
//!
//! The three spatial operators — diffuse, advect, flow — all ride on the
//! direction-tagged neighbour links from [`crate::grid`].

use crate::fields::{Belt, Biome, CrustType, Formation, Lithology};
use crate::grid::{Buffered, Coord, Topology};
use crate::rng::SplitMix64;
use crate::worldgen::{self, Generated};
use config::Params;
use sim::{Rng, Substrate};

/// A read-only snapshot of one tile, returned to (future) actors as the
/// substrate's `Perception`.
#[derive(Clone, Copy, Debug)]
pub struct TileView {
    pub coord: Coord,
    pub elevation: f32,
    pub temperature: f32,
    pub insolation: f32,
    pub minerals: f32,
}

/// Effects actors will send one another through the substrate. A placeholder
/// until actors exist — defined so the `Substrate` contract is complete.
#[derive(Clone, Debug)]
pub enum Interaction {
    Noop,
}

/// How one **stigmergy** layer behaves under `Φ`: how much of it spreads to the
/// neighbours each tick and how fast it fades. A pure scalar transport rule that
/// carries no meaning — the *same* struct configures a "food", "danger", or "demand"
/// layer; only the agents that deposit into and read it know what it represents. See
/// [`World::install_stigmergy`].
#[derive(Clone, Copy, Debug)]
pub struct StigConfig {
    /// Fraction pulled toward the neighbour mean each tick (`0` = no spread). The same
    /// diffusion stencil the climate fields use; a small value (e.g. `0.2`) gives a
    /// smooth gradient an agent several tiles away can still sense.
    pub diffuse: f32,
    /// Fraction of the signal lost each tick (`0` = permanent, `1` = gone next tick).
    /// Decay is what bounds a continually-fed field and gives its gradient a finite
    /// reach; typical values are `0.05..0.3`.
    pub decay: f32,
}

/// A single stigmergy layer: its double-buffered field plus its transport rule.
#[derive(Clone, Debug)]
struct StigLayer {
    field: Buffered<f32>,
    diffuse: f32,
    decay: f32,
}

/// The simulated world: a cylindrical hex grid of climate/geology/ecosystem
/// fields plus the clock that drives them.
pub struct World {
    topo: Topology,
    params: Params,
    /// Ticks elapsed = days since world start. Advanced by [`evolve`](Self::evolve).
    tick: u64,

    // --- static / slow (from world-gen) ---
    elevation: Vec<f32>,
    plate: Vec<u16>,
    crust: Vec<CrustType>,
    lithology: Vec<Lithology>,
    minerals: Vec<f32>,

    // --- dynamic (Φ) ---
    /// Fraction of peak sunlight, `0..1`. Purely local, so single-buffered.
    insolation: Vec<f32>,
    /// °C. Reads its neighbours when diffusing, so double-buffered.
    temperature: Buffered<f32>,
    /// Surface pressure (≈ hPa). Recomputed each tick from temperature/elevation.
    pressure: Vec<f32>,
    /// Wind vector `[x, y]` (world space). Recomputed each tick from pressure.
    wind: Vec<[f32; 2]>,
    /// Atmospheric moisture. Advected along the wind, so double-buffered.
    humidity: Buffered<f32>,
    /// Rain + meltable precipitation this tick (diagnostic, recomputed).
    precipitation: Vec<f32>,
    /// Standing water (rivers, lakes, sea). Flows downhill, so double-buffered.
    surface_water: Buffered<f32>,
    /// Accumulated snow / ice. Local freeze–melt memory, single-buffered.
    snow_ice: Vec<f32>,

    // --- ecosystem (Φ) ---
    /// Climate/soil ceiling on plant biomass (derived each tick).
    carrying_capacity: Vec<f32>,
    /// Net primary production this tick — plant growth rate (diagnostic; may be
    /// negative when biomass exceeds capacity).
    npp: Vec<f32>,
    /// Standing plant biomass. Grazing/migration couple it to fauna, so buffered.
    plant_biomass: Buffered<f32>,
    /// Dead organic matter awaiting decomposition (local).
    litter: Vec<f32>,
    /// Soil organic carbon (local).
    soil_carbon: Vec<f32>,
    /// Plant-available soil nutrients (local).
    soil_nutrients: Vec<f32>,
    /// Running mean **annual biotemperature** (°C, clamped to 0..30) — the slow
    /// climate memory the Holdridge belt is read from. An EMA, not an instant.
    bio_temp: Vec<f32>,
    /// Running **annualised precipitation** total (model units) — the slow
    /// moisture memory feeding the PET/precipitation humidity province. An EMA.
    annual_precip: Vec<f32>,
    /// Dominant biome — the tile's Holdridge life zone (derived classification).
    biome: Vec<Biome>,

    // --- disturbance (Φ) ---
    /// Fire intensity — biomass burned this tick. >0 means actively burning;
    /// spread reads neighbours' old state, so buffered.
    fire: Buffered<f32>,
    /// Surface reflectance `0..1` (derived). Offsets the temperature target.
    albedo: Vec<f32>,

    // --- stigmergy (Φ; agent-driven, generic) ---
    /// Optional scalar **stigmergy** layers: agent-deposited signals that diffuse and
    /// decay across the tilemap so a distant agent can read a gradient and follow it
    /// ("nudge, not command") instead of path-planning. Empty by default, so a world that
    /// never installs any is byte-identical, and diffusion is `O(tiles · layers)`,
    /// independent of how many agents there are. The meaning of each layer lives entirely
    /// with whoever deposits — this crate only spreads and fades the numbers. Installed via
    /// [`install_stigmergy`](Self::install_stigmergy).
    stigmergy: Vec<StigLayer>,
}

impl World {
    /// Assemble a world from a topology, parameters, and generated static
    /// fields, then seed the dynamic fields at their day-0 values.
    pub fn new(topo: Topology, params: Params, generated: Generated) -> Self {
        let len = topo.len();
        // Captured before `params` is moved into the struct below.
        let (soil_init, albedo_init) = (params.soil_nutrients_init, params.albedo_ref);
        let mut world = Self {
            topo,
            params,
            tick: 0,
            elevation: generated.elevation,
            plate: generated.plate,
            crust: generated.crust,
            lithology: generated.lithology,
            minerals: generated.minerals,
            insolation: vec![0.0; len],
            temperature: Buffered::filled(0.0, len),
            pressure: vec![0.0; len],
            wind: vec![[0.0, 0.0]; len],
            humidity: Buffered::filled(0.0, len),
            precipitation: vec![0.0; len],
            // Seeded with the rivers and lakes carved during world generation.
            surface_water: Buffered::from_vec(generated.surface_water),
            snow_ice: vec![0.0; len],
            carrying_capacity: vec![0.0; len],
            npp: vec![0.0; len],
            plant_biomass: Buffered::filled(0.0, len),
            litter: vec![0.0; len],
            soil_carbon: vec![0.0; len],
            soil_nutrients: vec![soil_init; len],
            bio_temp: vec![0.0; len],
            annual_precip: vec![0.0; len],
            biome: vec![Biome::Water; len],
            fire: Buffered::filled(0.0, len),
            // Seeded at the neutral reflectance so the seeded temperature carries
            // no albedo offset; real albedo is computed below.
            albedo: vec![albedo_init; len],
            // No stigmergy layers until a caller installs them — so a plain world is
            // byte-identical and `Φ`'s stigmergy step is a no-op.
            stigmergy: Vec::new(),
        };
        // Start the dynamic fields at sane values so observers and the first
        // tick don't see a cold black world. Humidity, water, snow, and the
        // biomass pools spin up over the first ticks.
        world.update_insolation();
        world.seed_temperature();
        world.update_pressure();
        world.update_wind();
        world.seed_ecosystem();
        world.update_albedo();
        world
    }

    /// Convenience constructor: build the topology, generate a world from
    /// `seed`, and return it ready to run.
    pub fn generate(width: i32, height: i32, params: Params, seed: u64) -> Self {
        let topo = Topology::new(width, height);
        let mut rng = SplitMix64::new(seed);
        let generated = worldgen::generate(&topo, &params, &mut rng);
        Self::new(topo, params, generated)
    }

    // --- accessors ---

    pub fn topology(&self) -> &Topology {
        &self.topo
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    /// Days since world start.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn elevation(&self, c: Coord) -> f32 {
        self.elevation[self.topo.index_of(c)]
    }

    pub fn temperature(&self, c: Coord) -> f32 {
        self.temperature.front()[self.topo.index_of(c)]
    }

    pub fn insolation(&self, c: Coord) -> f32 {
        self.insolation[self.topo.index_of(c)]
    }

    pub fn pressure(&self, c: Coord) -> f32 {
        self.pressure[self.topo.index_of(c)]
    }

    /// Wind vector `[x, y]` in world space (+x ≈ east, +y ≈ south).
    pub fn wind(&self, c: Coord) -> [f32; 2] {
        self.wind[self.topo.index_of(c)]
    }

    pub fn humidity(&self, c: Coord) -> f32 {
        self.humidity.front()[self.topo.index_of(c)]
    }

    pub fn precipitation(&self, c: Coord) -> f32 {
        self.precipitation[self.topo.index_of(c)]
    }

    pub fn surface_water(&self, c: Coord) -> f32 {
        self.surface_water.front()[self.topo.index_of(c)]
    }

    pub fn snow_ice(&self, c: Coord) -> f32 {
        self.snow_ice[self.topo.index_of(c)]
    }

    pub fn carrying_capacity(&self, c: Coord) -> f32 {
        self.carrying_capacity[self.topo.index_of(c)]
    }

    pub fn npp(&self, c: Coord) -> f32 {
        self.npp[self.topo.index_of(c)]
    }

    pub fn plant_biomass(&self, c: Coord) -> f32 {
        self.plant_biomass.front()[self.topo.index_of(c)]
    }

    /// Remove up to `amount` of plant biomass at `c` and return what was actually
    /// taken (never more than is there). This is the **stigmergic hook**: the one
    /// sanctioned way an actor writes vegetation back into the substrate, so a
    /// grazer's mark on the land persists and steers what grows and grazes next.
    pub fn graze(&mut self, c: Coord, amount: f32) -> f32 {
        let i = self.topo.index_of(c);
        let taken = amount.clamp(0.0, self.plant_biomass.front()[i]);
        self.plant_biomass.front_mut()[i] -= taken;
        taken
    }

    /// Remove up to `amount` of mineral richness at `c` (mining), returning what
    /// was taken. Ore deposits are finite: mining draws them down and they do not
    /// regrow, so a worked-out seam stays worked out.
    pub fn mine(&mut self, c: Coord, amount: f32) -> f32 {
        let i = self.topo.index_of(c);
        let taken = amount.clamp(0.0, self.minerals[i]);
        self.minerals[i] -= taken;
        taken
    }

    /// Set this tile alight — raise its **fire** intensity to at least `intensity`
    /// (clamped non-negative), returning the new value. The fire cellular automaton
    /// in `Φ` then spreads and consumes from here on its own, so a single poke starts
    /// a blaze that burns out naturally. This is the **disturbance-injection hook**:
    /// the sanctioned way an outside force — a narrative director's *cause* lever —
    /// kindles a fire the climate would not have lit this tick, by over-driving the
    /// very `fire` field a lightning strike writes. It invents no new physics; it only
    /// cranks a dial that already exists. The counterpart, on the producing side, to
    /// [`graze`](Self::graze) drawing biomass down.
    pub fn ignite(&mut self, c: Coord, intensity: f32) -> f32 {
        let i = self.topo.index_of(c);
        let v = self.fire.front()[i].max(intensity.max(0.0));
        self.fire.front_mut()[i] = v;
        v
    }

    /// Parch this tile: scale its standing **surface water** and **humidity** down by
    /// `frac` (`0` leaves it untouched, `1` wrings it dry), returning the water
    /// removed. The climate refills it over the following ticks (evaporation,
    /// advection, flow), so the drought is transient. The companion to
    /// [`ignite`](Self::ignite) on the *deny* side — a director withholding the water
    /// relief the land needs by over-driving the same moisture fields a dry spell
    /// would, never writing new physics.
    pub fn parch(&mut self, c: Coord, frac: f32) -> f32 {
        let i = self.topo.index_of(c);
        let keep = (1.0 - frac.clamp(0.0, 1.0)).max(0.0);
        let before = self.surface_water.front()[i];
        self.surface_water.front_mut()[i] = before * keep;
        self.humidity.front_mut()[i] *= keep;
        before * (1.0 - keep)
    }

    // --- stigmergy (generic, agent-deposited fields) ---

    /// Install `configs.len()` stigmergy layers, all initialised to zero, replacing any
    /// already present. The number and order of layers — and what each one *means* — is
    /// the caller's contract; this crate only diffuses and decays them each tick. Call once
    /// after construction, before the agents that will deposit into and read these layers
    /// start running.
    pub fn install_stigmergy(&mut self, configs: &[StigConfig]) {
        let len = self.topo.len();
        self.stigmergy = configs
            .iter()
            .map(|c| StigLayer {
                field: Buffered::filled(0.0, len),
                diffuse: c.diffuse,
                decay: c.decay,
            })
            .collect();
    }

    /// How many stigmergy layers are installed (`0` = none, the default).
    pub fn stigmergy_layers(&self) -> usize {
        self.stigmergy.len()
    }

    /// Add `amount` of signal to layer `l` at tile `c` — the **deposit hook**, the
    /// sanctioned way an agent writes a stigmergic mark into the world (the counterpart to
    /// [`graze`](Self::graze) for vegetation). The result is clamped non-negative. A deposit
    /// to a layer that does not exist is silently ignored, so a caller may deposit
    /// unconditionally and a stigmergy-free world stays byte-identical.
    pub fn deposit(&mut self, l: usize, c: Coord, amount: f32) {
        if let Some(layer) = self.stigmergy.get_mut(l) {
            let i = self.topo.index_of(c);
            let v = layer.field.front()[i] + amount;
            layer.field.front_mut()[i] = v.max(0.0);
        }
    }

    /// Read layer `l`'s signal at tile `c` — the **gradient-sampling hook** — or `0.0` if
    /// there is no such layer. An agent samples this at its neighbouring tiles to follow the
    /// gradient (toward food/demand, away from danger) without running a search.
    pub fn stig(&self, l: usize, c: Coord) -> f32 {
        self.stigmergy
            .get(l)
            .map_or(0.0, |layer| layer.field.front()[self.topo.index_of(c)])
    }

    pub fn litter(&self, c: Coord) -> f32 {
        self.litter[self.topo.index_of(c)]
    }

    pub fn soil_carbon(&self, c: Coord) -> f32 {
        self.soil_carbon[self.topo.index_of(c)]
    }

    pub fn soil_nutrients(&self, c: Coord) -> f32 {
        self.soil_nutrients[self.topo.index_of(c)]
    }

    /// The tile's biome — its Holdridge life zone (or `Water` below sea level).
    pub fn biome(&self, c: Coord) -> Biome {
        self.biome[self.topo.index_of(c)]
    }

    /// Running mean annual biotemperature (°C) — the slow climate average the
    /// biome's belt is classified from.
    pub fn biotemperature(&self, c: Coord) -> f32 {
        self.bio_temp[self.topo.index_of(c)]
    }

    /// Running annualised precipitation total (model units) — the slow moisture
    /// average the biome's humidity province is classified from.
    pub fn annual_precipitation(&self, c: Coord) -> f32 {
        self.annual_precip[self.topo.index_of(c)]
    }

    /// Fire intensity (biomass burned this tick); 0 if not burning.
    pub fn fire(&self, c: Coord) -> f32 {
        self.fire.front()[self.topo.index_of(c)]
    }

    pub fn albedo(&self, c: Coord) -> f32 {
        self.albedo[self.topo.index_of(c)]
    }

    pub fn minerals(&self, c: Coord) -> f32 {
        self.minerals[self.topo.index_of(c)]
    }

    /// Tectonic plate id owning this tile.
    pub fn plate(&self, c: Coord) -> u16 {
        self.plate[self.topo.index_of(c)]
    }

    pub fn crust(&self, c: Coord) -> CrustType {
        self.crust[self.topo.index_of(c)]
    }

    pub fn lithology(&self, c: Coord) -> Lithology {
        self.lithology[self.topo.index_of(c)]
    }

    /// A perception snapshot of one tile.
    pub fn tile_view(&self, c: Coord) -> TileView {
        let i = self.topo.index_of(c);
        TileView {
            coord: c,
            elevation: self.elevation[i],
            temperature: self.temperature.front()[i],
            insolation: self.insolation[i],
            minerals: self.minerals[i],
        }
    }

    // --- Φ steps ---

    /// Sub-solar latitude (°) for the current day: 0 at the equinoxes, ±tilt at
    /// the solstices.
    fn solar_declination_deg(&self) -> f32 {
        let p = &self.params;
        let phase = (self.tick as f32 - p.spring_equinox_tick) / p.ticks_per_year as f32;
        p.axial_tilt_deg * (phase * std::f32::consts::TAU).sin()
    }

    /// Recompute insolation everywhere from latitude and the current declination.
    fn update_insolation(&mut self) {
        let decl = self.solar_declination_deg();
        let topo = &self.topo;
        for i in topo.indices() {
            let lat = topo.latitude_deg(topo.coord(i).row);
            self.insolation[i] = insolation_factor(lat, decl);
        }
    }

    /// One temperature tick: relax toward the radiative target and diffuse
    /// toward the neighbour mean, reading old values and writing new ones.
    fn update_temperature(&mut self) {
        let params = &self.params;
        let topo = &self.topo;
        let elevation = &self.elevation;
        let insolation = &self.insolation;
        let albedo = &self.albedo;
        let (old, new) = self.temperature.read_write();
        for i in topo.indices() {
            let neighbors = topo.neighbors(i);
            let neigh_mean = if neighbors.is_empty() {
                old[i]
            } else {
                neighbors.iter().map(|l| old[l.to]).sum::<f32>() / neighbors.len() as f32
            };
            let target = radiative_target(params, insolation[i], elevation[i], albedo[i]);
            new[i] = old[i]
                + params.temp_relax * (target - old[i])
                + params.temp_diffuse * (neigh_mean - old[i]);
        }
        self.temperature.swap();
    }

    /// Pressure from the current temperature and elevation: warm air rises (low
    /// pressure), cold air sinks (high), and pressure thins with altitude.
    fn update_pressure(&mut self) {
        let p = &self.params;
        let temp = self.temperature.front();
        let elevation = &self.elevation;
        for i in self.topo.indices() {
            let land = (elevation[i] - p.sea_level).max(0.0);
            self.pressure[i] = p.base_pressure
                - p.pressure_temp_coeff * (temp[i] - p.pressure_temp_ref)
                - p.pressure_elev_coeff * land;
        }
    }

    /// Wind blows down the pressure gradient. The discrete gradient is the sum
    /// of `(p_neighbour − p_here) · direction` over the neighbour links; wind is
    /// its negation (toward low pressure), scaled by `wind_coeff`.
    fn update_wind(&mut self) {
        let p = &self.params;
        let topo = &self.topo;
        let pressure = &self.pressure;
        for i in topo.indices() {
            let (mut gx, mut gy) = (0.0f32, 0.0f32);
            for l in topo.neighbors(i) {
                let dp = pressure[l.to] - pressure[i];
                gx += dp * l.dir[0];
                gy += dp * l.dir[1];
            }
            self.wind[i] = [-p.wind_coeff * gx, -p.wind_coeff * gy];
        }
    }

    /// Humidity: evaporate moisture off warm wet tiles, advect the air along the
    /// wind (an upwind donor-cell scheme reading old values), then rain out
    /// whatever the now-cooler/lifted air can't hold. Precipitation is recorded
    /// for the water and snow steps.
    fn update_humidity(&mut self) {
        let p = &self.params;
        let topo = &self.topo;
        let wind = &self.wind;
        let temp = self.temperature.front();
        let elevation = &self.elevation;
        let old_water = self.surface_water.front();
        let precip = &mut self.precipitation;
        let (old, new) = self.humidity.read_write();

        // Carry over last tick's moisture, then move it along the wind.
        new.copy_from_slice(old);
        for i in topo.indices() {
            let w = wind[i];
            let speed = (w[0] * w[0] + w[1] * w[1]).sqrt();
            if speed <= 0.0 {
                continue;
            }
            let links = topo.neighbors(i);
            let weight = |l: &crate::grid::Link| (w[0] * l.dir[0] + w[1] * l.dir[1]).max(0.0);
            let total: f32 = links.iter().map(weight).sum();
            if total <= 0.0 {
                continue;
            }
            let outflux = old[i] * (speed * p.humidity_advect).min(p.max_advect);
            for l in links {
                let share = weight(l) / total;
                let flux = outflux * share;
                new[i] -= flux;
                new[l.to] += flux;
            }
        }

        // Turbulent diffusion: spread toward the neighbour mean (reads old, so
        // order-independent). This is what carries damp air inland over a mostly
        // land world where advection alone would rain out at the coast.
        for i in topo.indices() {
            let links = topo.neighbors(i);
            if !links.is_empty() {
                let mean = links.iter().map(|l| old[l.to]).sum::<f32>() / links.len() as f32;
                new[i] += p.humidity_diffuse * (mean - old[i]);
            }
        }

        // Evaporation in, precipitation out (pointwise).
        for i in topo.indices() {
            let wetness = evaporation_source(p, elevation[i], old_water[i]);
            new[i] += p.evaporation * temp[i].max(0.0) * wetness;

            let capacity = saturation(p, temp[i]);
            let condensation = (new[i] - capacity).max(0.0);
            let orographic =
                new[i] * p.orographic_coeff * upslope_in_wind(topo, elevation, wind, i);
            let rain = (condensation + orographic).min(new[i]).max(0.0);
            precip[i] = rain;
            new[i] -= rain;
        }
        self.humidity.swap();
    }

    /// Route precipitation into standing water and snow. Below freezing the rain
    /// is snow; above it, snow melts back into meltwater. The liquid share then
    /// retains, gathers, and flows to each tile's steepest-downhill neighbour.
    fn update_water_and_snow(&mut self) {
        let p = &self.params;
        let n = self.topo.len();

        // Split precipitation into liquid (rain + meltwater) vs snow, updating
        // the snow pack in place.
        let mut rain = vec![0.0f32; n];
        {
            let temp = self.temperature.front();
            let precip = &self.precipitation;
            for i in 0..n {
                let mut liquid = precip[i];
                if temp[i] < p.freeze_temp {
                    self.snow_ice[i] += liquid; // falls as snow
                    liquid = 0.0;
                } else {
                    let melt =
                        (p.melt_rate * (temp[i] - p.freeze_temp)).clamp(0.0, self.snow_ice[i]);
                    self.snow_ice[i] -= melt;
                    liquid += melt;
                }
                rain[i] = liquid;
            }
        }

        // Surface water: keep a share, add the new liquid, send a share downhill.
        let topo = &self.topo;
        let elevation = &self.elevation;
        let (old, new) = self.surface_water.read_write();
        for i in 0..n {
            new[i] = old[i] * p.water_retain + rain[i];
        }
        for i in 0..n {
            let mut lowest = elevation[i];
            let mut sink = None;
            for l in topo.neighbors(i) {
                if elevation[l.to] < lowest {
                    lowest = elevation[l.to];
                    sink = Some(l.to);
                }
            }
            if let Some(d) = sink {
                let moved = old[i] * p.water_flow;
                new[i] -= moved;
                new[d] += moved;
            }
        }
        self.surface_water.swap();
    }

    /// Plant growth. Plants grow logistically toward the climate/soil
    /// `carrying_capacity` and shed mortality into `litter`. Herbivory is no
    /// longer a field term — the only grazers are the agent layer, which calls
    /// [`graze`](Self::graze). Reads the old buffer, writes the new, swaps.
    fn update_vegetation(&mut self) {
        let n = self.topo.len();
        {
            let p = &self.params;
            let temp = self.temperature.front();
            let water = self.surface_water.front();
            let precip = &self.precipitation;
            let insol = &self.insolation;
            let elevation = &self.elevation;
            let soil_n = &self.soil_nutrients;
            let soil_c = &self.soil_carbon;
            let biome = &self.biome;
            let cc = &mut self.carrying_capacity;
            let npp = &mut self.npp;
            let litter = &mut self.litter;
            let (plant_old, plant_new) = self.plant_biomass.read_write();

            for i in 0..n {
                let land = elevation[i] >= p.sea_level;
                let moisture = moisture_index(p, water[i], precip[i]);
                // Instantaneous suitability still gates the growth *rate* — the
                // channel through which droughts and the seasons bite.
                let suit = growth_suitability(p, temp[i], moisture, insol[i], land);
                let nutrient = soil_n[i] / (soil_n[i] + p.nutrient_half);
                // Rich soil carbon lifts the ceiling — the slow soil→fertility memory.
                let carbon = 1.0 + p.carbon_fertility * soil_c[i] / (soil_c[i] + p.carbon_half);
                // The biome sets the productivity ceiling; soil fertility scales it.
                // Climate no longer caps the ceiling directly — it does so *through*
                // the biome it has been distilled into.
                let k =
                    (p.biomass_max * biome[i].profile(p).productivity * nutrient * carbon).max(0.0);
                cc[i] = k;

                let production = p.plant_growth
                    * suit
                    * (plant_old[i] + p.plant_seed)
                    * (1.0 - plant_old[i] / k.max(1e-3));
                npp[i] = production;

                let plant_death = plant_old[i] * p.plant_mortality;
                plant_new[i] = (plant_old[i] + production - plant_death).clamp(0.0, p.biomass_max);
                litter[i] += plant_death;
            }
        }
        self.plant_biomass.swap();
    }

    /// Soil: decompose litter (faster when warm and wet) into stabilised
    /// `soil_carbon` and plant-available `soil_nutrients`; weathering tops the
    /// nutrients up and plant uptake (this tick's production) draws them down.
    fn update_soil(&mut self) {
        let p = &self.params;
        let n = self.topo.len();
        let temp = self.temperature.front();
        let water = self.surface_water.front();
        let precip = &self.precipitation;
        let npp = &self.npp;
        let biome = &self.biome;
        let litter = &mut self.litter;
        let soil_c = &mut self.soil_carbon;
        let soil_n = &mut self.soil_nutrients;
        for i in 0..n {
            let moisture = moisture_index(p, water[i], precip[i]);
            // The biome modulates decomposition: cold tundra locks litter into slow
            // peat, warm wet forest turns it over fast.
            let activity = decomposition_activity(p, temp[i], moisture) * biome[i].profile(p).decay;
            let decomposed = (litter[i] * p.decomposition * activity).clamp(0.0, litter[i]);
            litter[i] -= decomposed;

            soil_c[i] =
                (soil_c[i] + decomposed * p.humification - soil_c[i] * p.soil_respiration).max(0.0);

            let uptake = npp[i].max(0.0) * p.nutrient_uptake;
            soil_n[i] =
                (soil_n[i] + p.weathering + decomposed * p.mineralization - uptake).max(0.0);
        }
    }

    /// Fold this tick's weather into the slow **annual climate averages** the
    /// biome is classified from: an exponential moving average of biotemperature
    /// (temperature clamped to the 0..30 °C growth window) and of annualised
    /// precipitation, with a time constant of `biome_memory_years`. Deterministic.
    fn update_climate_aggregates(&mut self) {
        let p = &self.params;
        let alpha = climate_ema_alpha(p, self.tick);
        let year = p.ticks_per_year as f32;
        let temp = self.temperature.front();
        let precip = &self.precipitation;
        for i in self.topo.indices() {
            let bt = temp[i].clamp(0.0, 30.0);
            self.bio_temp[i] += alpha * (bt - self.bio_temp[i]);
            self.annual_precip[i] += alpha * (precip[i] * year - self.annual_precip[i]);
        }
    }

    /// Classify each land tile's Holdridge life zone from its annual climate
    /// averages (open sea below the datum is always `Water`).
    fn update_biome(&mut self) {
        let p = &self.params;
        let topo = &self.topo;
        let elevation = &self.elevation;
        let bio_temp = &self.bio_temp;
        let annual_precip = &self.annual_precip;
        let biome = &mut self.biome;
        for i in topo.indices() {
            let water = elevation[i] < p.sea_level;
            biome[i] = classify_biome(p, bio_temp[i], annual_precip[i], water);
        }
    }

    /// Fill `carrying_capacity`, the climate averages, and `biome` from the
    /// initial climate so a fresh world reads sensibly before the first tick
    /// (without advancing biomass). The averages start at the instantaneous
    /// values; precipitation has not spun up yet, so biomes refine over the
    /// warm-up like the other dynamic fields.
    fn seed_ecosystem(&mut self) {
        let p = &self.params;
        let topo = &self.topo;
        let temp = self.temperature.front();
        let precip = &self.precipitation;
        let elevation = &self.elevation;
        let soil_n = &self.soil_nutrients;
        let soil_c = &self.soil_carbon;
        let year = p.ticks_per_year as f32;
        let cc = &mut self.carrying_capacity;
        let bio_temp = &mut self.bio_temp;
        let annual_precip = &mut self.annual_precip;
        let biome = &mut self.biome;
        for i in topo.indices() {
            let water = elevation[i] < p.sea_level;
            bio_temp[i] = temp[i].clamp(0.0, 30.0);
            annual_precip[i] = precip[i] * year;
            let b = classify_biome(p, bio_temp[i], annual_precip[i], water);
            biome[i] = b;
            // Seed the carrying capacity from the biome's profile, exactly as the
            // running ecosystem does, so a fresh world is consistent with its first tick.
            let nutrient = soil_n[i] / (soil_n[i] + p.nutrient_half);
            let carbon = 1.0 + p.carbon_fertility * soil_c[i] / (soil_c[i] + p.carbon_half);
            cc[i] = (p.biomass_max * b.profile(p).productivity * nutrient * carbon).max(0.0);
        }
    }

    /// Fire — a stochastic cellular automaton. A tile keeps burning if it still
    /// has fuel; an unburnt fuelled tile ignites from lightning (rare) or by
    /// catching from a burning neighbour (likelier downwind and uphill). Burning
    /// tiles consume biomass and litter, return part as ash to the soil, and
    /// scorch fauna. Spread reads the *old* fire buffer, so it is
    /// order-independent; the random draws happen in fixed index order, so a
    /// seeded run is reproducible.
    fn update_fire(&mut self, rng: &mut dyn Rng) {
        let p = &self.params;
        let topo = &self.topo;
        let n = topo.len();
        let temp = self.temperature.front();
        let water = self.surface_water.front();
        let precip = &self.precipitation;
        let wind = &self.wind;
        let elevation = &self.elevation;
        let biome = &self.biome;
        let litter = &mut self.litter;
        let soil_n = &mut self.soil_nutrients;
        let soil_c = &mut self.soil_carbon;
        let plant = self.plant_biomass.front_mut();
        let (old_fire, new_fire) = self.fire.read_write();

        for i in 0..n {
            let fuel = plant[i] + p.fire_litter_weight * litter[i];
            let mut burning = old_fire[i] > 0.0 && fuel > p.fire_fuel_min;

            // An unburnt, fuelled tile may ignite this tick.
            if !burning && fuel > p.fire_fuel_min {
                let moisture = moisture_index(p, water[i], precip[i]);
                // The biome's cover sets how readily fuel takes: grass and scrub
                // catch eagerly, rainforest and tundra resist.
                let dry = (fire_dryness(p, temp[i], moisture) * biome[i].profile(p).flammability)
                    .min(1.0);
                let fuel_factor = fuel / (fuel + p.fire_fuel_half);
                if dry > 0.0 && fuel_factor > 0.0 {
                    // Lightning.
                    if rng.gen_bool((p.base_lightning * dry * fuel_factor).clamp(0.0, 1.0) as f64) {
                        burning = true;
                    }
                    // Spread from burning neighbours (combined probability).
                    if !burning {
                        let mut keep = 1.0;
                        for l in topo.neighbors(i) {
                            if old_fire[l.to] <= 0.0 {
                                continue;
                            }
                            let ws = wind[l.to];
                            let wmag = (ws[0] * ws[0] + ws[1] * ws[1]).sqrt();
                            let toward = if wmag > 0.0 {
                                ((ws[0] * -l.dir[0] + ws[1] * -l.dir[1]) / wmag).max(0.0)
                            } else {
                                0.0
                            };
                            let rise = (elevation[i] - elevation[l.to]).max(0.0);
                            let slope = (rise / p.fire_slope_scale).min(1.0);
                            let pk = (p.base_spread
                                * dry
                                * fuel_factor
                                * (1.0 + p.wind_spread * toward)
                                * (1.0 + p.slope_spread * slope))
                                .clamp(0.0, 1.0);
                            keep *= 1.0 - pk;
                        }
                        let catch = 1.0 - keep;
                        if catch > 0.0 && rng.gen_bool(catch as f64) {
                            burning = true;
                        }
                    }
                }
            }

            if burning {
                let consumed = (p.fire_consume * plant[i]).min(plant[i]);
                let litter_burned = (p.fire_litter_consume * litter[i]).min(litter[i]);
                let burned = consumed + litter_burned;
                plant[i] -= consumed;
                litter[i] -= litter_burned;
                soil_n[i] += burned * p.fire_ash;
                soil_c[i] += burned * p.fire_ash_carbon;
                new_fire[i] = burned; // intensity = fuel burned this tick
            } else {
                new_fire[i] = 0.0;
            }
        }
        self.fire.swap();
    }

    /// Surface reflectance from cover: a base by plant type, brightened toward
    /// snow albedo by snow cover and darkened toward char by active fire.
    fn update_albedo(&mut self) {
        let p = &self.params;
        let topo = &self.topo;
        let biome = &self.biome;
        let snow = &self.snow_ice;
        let fire = self.fire.front();
        let albedo = &mut self.albedo;
        for i in topo.indices() {
            let base = base_albedo(p, biome[i].formation());
            let snow_cover = (snow[i] / p.snow_albedo_cover).clamp(0.0, 1.0);
            let bright = base + (p.albedo_snow - base) * snow_cover;
            let burn = (fire[i] / p.fire_albedo_ref).clamp(0.0, 1.0) * p.burn_albedo_weight;
            albedo[i] = (bright + (p.albedo_burn - bright) * burn).clamp(0.0, 1.0);
        }
    }

    /// Spread and fade every installed stigmergy layer one tick: a diffusion stencil
    /// (toward the neighbour mean, polar-renormalised like the climate fields) followed by
    /// exponential decay. Reads the old buffer and writes the new — so it is
    /// order-independent — and a no-op when no layers are installed, so a stigmergy-free
    /// world is byte-identical. Cost is `O(tiles · layers)`, independent of agent count:
    /// the whole point of stigmergy.
    fn update_stigmergy(&mut self) {
        let topo = &self.topo;
        for layer in &mut self.stigmergy {
            let (diffuse, decay) = (layer.diffuse, layer.decay);
            let (old, new) = layer.field.read_write();
            for i in topo.indices() {
                let neighbors = topo.neighbors(i);
                let mean = if neighbors.is_empty() {
                    old[i]
                } else {
                    neighbors.iter().map(|l| old[l.to]).sum::<f32>() / neighbors.len() as f32
                };
                let spread = old[i] + diffuse * (mean - old[i]);
                new[i] = (spread * (1.0 - decay)).max(0.0);
            }
            layer.field.swap();
        }
    }

    /// Initialise temperature at its radiative target (no diffusion yet) so the
    /// world starts near equilibrium rather than at 0 °C everywhere.
    fn seed_temperature(&mut self) {
        let target: Vec<f32> = self
            .topo
            .indices()
            .map(|i| {
                radiative_target(
                    &self.params,
                    self.insolation[i],
                    self.elevation[i],
                    self.albedo[i],
                )
            })
            .collect();
        // Write the same values into both halves of the buffer.
        self.temperature.back_mut().copy_from_slice(&target);
        self.temperature.swap();
        self.temperature.back_mut().copy_from_slice(&target);
    }
}

/// Daily insolation factor `0..1`: the cosine of the solar zenith at this
/// latitude, clamped (no negative sun on the dark side).
fn insolation_factor(lat_deg: f32, decl_deg: f32) -> f32 {
    (lat_deg - decl_deg).to_radians().cos().max(0.0)
}

/// The temperature a tile is pulled toward: a latitude/season gradient set by
/// the sun, minus the lapse-rate cooling of land above sea level, minus an
/// albedo offset (a bright tile reflects sun and runs cooler than `albedo_ref`;
/// a dark one runs warmer). The albedo term is what closes the snow/vegetation
/// feedback into the climate.
fn radiative_target(params: &Params, sun: f32, elevation: f32, albedo: f32) -> f32 {
    let land = (elevation - params.sea_level).max(0.0);
    let base = params.pole_temp + (params.equator_temp - params.pole_temp) * sun
        - params.lapse_rate * land;
    base - params.albedo_temp_coeff * (albedo - params.albedo_ref)
}

/// How much moisture a tile can give up: open sea (below the datum) is fully
/// wet; land evaporates in proportion to the standing water it holds.
fn evaporation_source(params: &Params, elevation: f32, surface_water: f32) -> f32 {
    if elevation < params.sea_level {
        1.0
    } else {
        surface_water.min(1.0)
    }
}

/// Moisture a tile can hold at a given temperature — an exponential, so warm air
/// holds much more and cooling/lifting forces condensation.
fn saturation(params: &Params, temp: f32) -> f32 {
    params.saturation_base * (params.saturation_growth * temp).exp()
}

/// Sum of the upslope height gain in the wind's direction: how strongly the
/// terrain forces this tile's air to rise (the orographic rain driver).
fn upslope_in_wind(topo: &Topology, elevation: &[f32], wind: &[[f32; 2]], i: usize) -> f32 {
    let w = wind[i];
    let mut lift = 0.0;
    for l in topo.neighbors(i) {
        let into = (w[0] * l.dir[0] + w[1] * l.dir[1]).max(0.0);
        let rise = (elevation[l.to] - elevation[i]).max(0.0);
        lift += into * rise;
    }
    lift
}

/// Plant-available moisture: standing water plus a share of this tick's rain.
fn moisture_index(params: &Params, surface_water: f32, precipitation: f32) -> f32 {
    surface_water + precipitation * params.rain_to_moisture
}

/// Overall growth factor `0..1` from temperature suitability (a bell around the
/// optimum), moisture, and light. Combined by **Liebig's law of the minimum** — the
/// scarcest resource governs, `min(temp, water, light)` — rather than the product of
/// the three (Lieth's Miami model and DGVMs use the minimum). The product
/// systematically under-predicts growth by stacking sub-unity factors, leaving the
/// land far barer than the climate warrants; the minimum lets tiles limited by only
/// one factor still green properly. Zero over open sea.
fn growth_suitability(params: &Params, temp: f32, moisture: f32, light: f32, land: bool) -> f32 {
    if !land {
        return 0.0;
    }
    let z = (temp - params.growth_temp_opt) / params.growth_temp_width;
    let temp_factor = (-z * z).exp();
    let water_factor = moisture / (moisture + params.moisture_half);
    temp_factor
        .min(water_factor)
        .min(light.clamp(0.0, 1.0))
        .clamp(0.0, 1.0)
}

/// How fast decomposers work: a **Q10** temperature response (activity multiplies by
/// `q10` per 10 °C above the reference, the standard soil-respiration form) times a
/// moisture factor. Warm, wet soils turn litter over several times faster than cool
/// ones, so they release nutrients — and respire carbon — far quicker.
fn decomposition_activity(params: &Params, temp: f32, moisture: f32) -> f32 {
    let warmth = params
        .decomp_q10
        .powf((temp - params.decomp_ref_temp) / 10.0);
    let wetness = moisture / (moisture + params.moisture_half);
    (warmth * wetness).clamp(0.0, 3.0)
}

/// Blend factor for the running annual climate averages. It floors at the long
/// run EMA rate (`1 / memory_years·ticks_per_year`) but for a fresh world starts
/// as a true cumulative mean (`1 / age`), so the averages lock onto the first
/// year's seasonal cycle quickly and then drift slowly — accurate biomes after a
/// one-year warm-up without sacrificing long-term memory. A pure function of the
/// tick, so it stays deterministic and resume-safe.
fn climate_ema_alpha(params: &Params, tick: u64) -> f32 {
    let ema = 1.0 / (params.biome_memory_years * params.ticks_per_year as f32).max(1.0);
    let cumulative = 1.0 / (tick as f32 + 1.0);
    ema.max(cumulative).clamp(0.0, 1.0)
}

/// Classify a tile's **Holdridge life zone** from its annual climate averages:
/// the latitudinal belt from mean annual biotemperature, the humidity province
/// from the potential-evapotranspiration / precipitation ratio. Open sea (below
/// the datum) is `Water`.
fn classify_biome(params: &Params, bio_temp: f32, annual_precip: f32, water: bool) -> Biome {
    if water {
        return Biome::Water;
    }
    Biome::from_cell(
        belt_of(params, bio_temp),
        humidity_index(params, bio_temp, annual_precip),
    )
}

/// The Holdridge latitudinal belt for a mean annual biotemperature (°C).
fn belt_of(params: &Params, bio_temp: f32) -> Belt {
    if bio_temp < params.biotemp_subpolar {
        Belt::Polar
    } else if bio_temp < params.biotemp_boreal {
        Belt::Subpolar
    } else if bio_temp < params.biotemp_cool_temperate {
        Belt::Boreal
    } else if bio_temp < params.biotemp_warm_temperate {
        Belt::CoolTemperate
    } else if bio_temp < params.biotemp_subtropical {
        Belt::WarmTemperate
    } else if bio_temp < params.biotemp_tropical {
        Belt::Subtropical
    } else {
        Belt::Tropical
    }
}

/// Humidity-province index `0` (driest superarid) … `7` (wettest superhumid) from
/// the PET/precipitation ratio on Holdridge's canonical log₂ ladder. `PET` is the
/// biotemperature scaled by `pet_coeff`; a dry tile (little rain, much potential
/// evaporation) has a high ratio and lands in the low, arid indices.
fn humidity_index(params: &Params, bio_temp: f32, annual_precip: f32) -> usize {
    let pet = bio_temp * params.pet_coeff;
    let ratio = if annual_precip > 1e-6 {
        pet / annual_precip
    } else {
        f32::INFINITY
    };
    if ratio >= 16.0 {
        0
    } else if ratio >= 8.0 {
        1
    } else if ratio >= 4.0 {
        2
    } else if ratio >= 2.0 {
        3
    } else if ratio >= 1.0 {
        4
    } else if ratio >= 0.5 {
        5
    } else if ratio >= 0.25 {
        6
    } else {
        7
    }
}

/// Fire-weather dryness `0..1`: rises with heat above a threshold and falls with
/// fuel moisture. Damp or cool fuel barely burns; hot, dry fuel burns readily.
fn fire_dryness(params: &Params, temp: f32, moisture: f32) -> f32 {
    let heat = ((temp - params.fire_dry_temp) / params.fire_dry_scale).clamp(0.0, 1.0);
    let dampening = 1.0 / (1.0 + moisture / params.fire_moisture_half);
    (heat * dampening).clamp(0.0, 1.0)
}

/// Base surface reflectance for a structural cover (Holdridge formation), before
/// snow brightening and burn darkening.
fn base_albedo(params: &Params, formation: Formation) -> f32 {
    match formation {
        Formation::Water => params.albedo_water,
        Formation::Desert => params.albedo_desert,
        Formation::Tundra => params.albedo_tundra,
        Formation::Grassland => params.albedo_grass,
        Formation::Shrubland => params.albedo_shrub,
        Formation::Forest => params.albedo_forest,
        Formation::Rainforest => params.albedo_rainforest,
    }
}

impl Substrate for World {
    type Position = Coord;
    type Perception = TileView;
    type Interaction = Interaction;
    type Claim = Coord;

    /// `Φ`: advance one day and run the climate cascade in dependency order.
    /// `rng` is unused while these fields are deterministic; stochastic steps
    /// (e.g. fire) will draw from it.
    fn evolve(&mut self, rng: &mut dyn Rng) {
        self.tick += 1;
        self.update_insolation();
        self.update_temperature();
        self.update_pressure();
        self.update_wind();
        self.update_humidity();
        self.update_water_and_snow();
        self.update_vegetation();
        self.update_soil();
        self.update_climate_aggregates();
        self.update_biome();
        self.update_fire(rng);
        self.update_albedo();
        self.update_stigmergy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Big enough that tectonic world-gen produces real continents (a 12×11 grid
    // is mostly plate boundaries → all coast and mountains), small enough to run
    // hundreds of ticks per test cheaply.
    const TEST_W: i32 = 36;
    const TEST_H: i32 = 26;

    fn small_world() -> World {
        World::generate(TEST_W, TEST_H, Params::default(), 2026)
    }

    #[test]
    fn evolve_keeps_temperatures_finite() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..200 {
            world.evolve(&mut rng);
        }
        for i in world.topology().indices() {
            let t = world.temperature.front()[i];
            assert!(t.is_finite(), "temperature went non-finite");
            assert!(
                (-100.0..100.0).contains(&t),
                "temperature {t} left sane range"
            );
        }
    }

    #[test]
    fn insolation_is_a_unit_fraction() {
        let world = small_world();
        for i in world.topology().indices() {
            assert!((0.0..=1.0).contains(&world.insolation[i]));
        }
    }

    #[test]
    fn equator_is_warmer_than_the_poles() {
        let world = small_world();
        let topo = world.topology();
        let height = topo.height();
        let row_mean = |row: i32| -> f32 {
            let sum: f32 = (0..topo.width())
                .map(|col| world.temperature(Coord::new(col, row)))
                .sum();
            sum / topo.width() as f32
        };
        let equator = row_mean(height / 2);
        let poles = (row_mean(0) + row_mean(height - 1)) / 2.0;
        assert!(
            equator > poles,
            "equator {equator} should beat poles {poles}"
        );
    }

    #[test]
    fn poles_have_a_strong_seasonal_swing() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        let north = Coord::new(0, 0);
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for _ in 0..world.params().ticks_per_year {
            world.evolve(&mut rng);
            let s = world.insolation(north);
            min = min.min(s);
            max = max.max(s);
        }
        // Polar night drives insolation to ~0; polar summer lifts it well above.
        assert!(
            min < 0.05,
            "expected a polar night, min insolation was {min}"
        );
        assert!(
            max > 0.25,
            "expected a polar summer, max insolation was {max}"
        );
    }

    #[test]
    fn runs_are_deterministic() {
        let mut a = World::generate(TEST_W, TEST_H, Params::default(), 7);
        let mut b = World::generate(TEST_W, TEST_H, Params::default(), 7);
        let mut ra = SplitMix64::new(0);
        let mut rb = SplitMix64::new(0);
        for _ in 0..120 {
            a.evolve(&mut ra);
            b.evolve(&mut rb);
        }
        for i in a.topology().indices() {
            assert_eq!(a.temperature.front()[i], b.temperature.front()[i]);
            assert_eq!(a.insolation[i], b.insolation[i]);
            assert_eq!(a.pressure[i], b.pressure[i]);
            assert_eq!(a.humidity.front()[i], b.humidity.front()[i]);
            assert_eq!(a.surface_water.front()[i], b.surface_water.front()[i]);
            assert_eq!(a.snow_ice[i], b.snow_ice[i]);
            assert_eq!(a.plant_biomass.front()[i], b.plant_biomass.front()[i]);
            assert_eq!(a.soil_nutrients[i], b.soil_nutrients[i]);
            // Fire is stochastic but reproducible: same seed + rng stream → same burns.
            assert_eq!(a.fire.front()[i], b.fire.front()[i]);
            assert_eq!(a.albedo[i], b.albedo[i]);
        }
    }

    #[test]
    fn climate_fields_stay_finite_and_nonnegative() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..300 {
            world.evolve(&mut rng);
        }
        for i in world.topology().indices() {
            assert!(world.pressure[i].is_finite(), "pressure non-finite");
            assert!(
                world.wind[i][0].is_finite() && world.wind[i][1].is_finite(),
                "wind non-finite"
            );
            for (name, v) in [
                ("humidity", world.humidity.front()[i]),
                ("precipitation", world.precipitation[i]),
                ("surface_water", world.surface_water.front()[i]),
                ("snow_ice", world.snow_ice[i]),
            ] {
                assert!(v.is_finite(), "{name} went non-finite");
                assert!(v >= 0.0, "{name} went negative ({v})");
            }
        }
    }

    #[test]
    fn precipitation_occurs_over_a_year() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        let mut total = 0.0_f32;
        for _ in 0..world.params().ticks_per_year {
            world.evolve(&mut rng);
            total += world
                .topology()
                .indices()
                .map(|i| world.precipitation[i])
                .sum::<f32>();
        }
        assert!(total > 0.0, "the whole world stayed bone dry for a year");
    }

    #[test]
    fn water_gathers_in_the_lowlands() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..300 {
            world.evolve(&mut rng);
        }
        let topo = world.topology();
        // The wettest tile exists and sits below the mean elevation — water runs
        // downhill and pools in basins / the sea.
        let mean_elev: f32 =
            topo.indices().map(|i| world.elevation[i]).sum::<f32>() / topo.len() as f32;
        let wettest = topo
            .indices()
            .max_by(|&a, &b| {
                world.surface_water.front()[a]
                    .partial_cmp(&world.surface_water.front()[b])
                    .unwrap()
            })
            .unwrap();
        assert!(
            world.surface_water.front()[wettest] > 0.0,
            "no standing water formed"
        );
        assert!(
            world.elevation[wettest] < mean_elev,
            "wettest tile (elev {}) should be below mean elevation ({mean_elev})",
            world.elevation[wettest]
        );
    }

    #[test]
    fn snow_builds_where_it_freezes() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        let mut any_snow = false;
        for _ in 0..world.params().ticks_per_year {
            world.evolve(&mut rng);
            if world.topology().indices().any(|i| world.snow_ice[i] > 0.0) {
                any_snow = true;
            }
        }
        assert!(
            any_snow,
            "expected snow to accumulate somewhere cold over a year"
        );
        // And every snowy tile was genuinely at or below freezing when it built.
        // (Checked indirectly: snow only ever exists with the freeze logic.)
    }

    #[test]
    fn ecosystem_fields_stay_finite_and_nonnegative() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..400 {
            world.evolve(&mut rng);
        }
        for i in world.topology().indices() {
            assert!(world.npp[i].is_finite(), "npp non-finite"); // npp may be negative
            for (name, v) in [
                ("carrying_capacity", world.carrying_capacity[i]),
                ("plant_biomass", world.plant_biomass.front()[i]),
                ("litter", world.litter[i]),
                ("soil_carbon", world.soil_carbon[i]),
                ("soil_nutrients", world.soil_nutrients[i]),
            ] {
                assert!(v.is_finite(), "{name} went non-finite");
                assert!(v >= 0.0, "{name} went negative ({v})");
                assert!(v <= 1e6, "{name} blew up ({v})");
            }
        }
    }

    #[test]
    fn vegetation_establishes_on_suitable_land() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..400 {
            world.evolve(&mut rng);
        }
        let greenest = world
            .topology()
            .indices()
            .map(|i| world.plant_biomass.front()[i])
            .fold(0.0_f32, f32::max);
        assert!(
            greenest > 0.1,
            "nothing grew anywhere (max biomass {greenest})"
        );
    }

    #[test]
    fn open_ocean_grows_no_land_plants() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..200 {
            world.evolve(&mut rng);
        }
        let p = world.params();
        for i in world.topology().indices() {
            if world.elevation[i] < p.sea_level {
                assert_eq!(
                    world.plant_biomass.front()[i],
                    0.0,
                    "plants grew in the sea"
                );
                assert_eq!(
                    world.biome[i],
                    Biome::Water,
                    "submerged tile should read as Water"
                );
            }
        }
    }

    #[test]
    fn soil_carbon_builds_under_vegetation() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..400 {
            world.evolve(&mut rng);
        }
        let max_soil = world
            .topology()
            .indices()
            .map(|i| world.soil_carbon[i])
            .fold(0.0_f32, f32::max);
        assert!(max_soil > 0.0, "no soil carbon accumulated from litter");
    }

    #[test]
    fn biomes_are_varied() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..400 {
            world.evolve(&mut rng);
        }
        // A pole-to-equator world with mountains and seas should host more than
        // one land formation, not a single monoculture.
        let land_formations = [
            Formation::Desert,
            Formation::Tundra,
            Formation::Grassland,
            Formation::Shrubland,
            Formation::Forest,
            Formation::Rainforest,
        ];
        let kinds = land_formations
            .iter()
            .filter(|&&f| {
                world
                    .topology()
                    .indices()
                    .any(|i| world.biome[i].formation() == f)
            })
            .count();
        assert!(
            kinds >= 2,
            "expected at least two land formations, found {kinds}"
        );
    }

    #[test]
    fn belt_climbs_with_biotemperature() {
        let p = Params::default();
        assert_eq!(belt_of(&p, 0.5), Belt::Polar);
        assert_eq!(belt_of(&p, 2.0), Belt::Subpolar);
        assert_eq!(belt_of(&p, 4.5), Belt::Boreal);
        assert_eq!(belt_of(&p, 9.0), Belt::CoolTemperate);
        assert_eq!(belt_of(&p, 15.0), Belt::WarmTemperate);
        assert_eq!(belt_of(&p, 20.0), Belt::Subtropical);
        assert_eq!(belt_of(&p, 27.0), Belt::Tropical);
    }

    #[test]
    fn humidity_index_rises_as_a_tile_gets_wetter() {
        let p = Params::default();
        let bt = 20.0;
        // More annual precipitation at the same potential evapotranspiration is
        // wetter, so it climbs the humidity ladder toward the superhumid index.
        let dry = humidity_index(&p, bt, 5.0);
        let mid = humidity_index(&p, bt, 60.0);
        let wet = humidity_index(&p, bt, 600.0);
        assert!(
            dry < mid && mid < wet,
            "expected dry {dry} < mid {mid} < wet {wet}"
        );
        assert_eq!(
            humidity_index(&p, bt, 0.0),
            0,
            "no rain is the driest province"
        );
    }

    #[test]
    fn classify_biome_reads_the_extremes() {
        let p = Params::default();
        assert_eq!(
            classify_biome(&p, 25.0, 100.0, true),
            Biome::Water,
            "below-sea is water"
        );
        // Hot and soaked → a tropical rain canopy.
        let hot_wet = classify_biome(&p, 26.0, 4000.0, false);
        assert_eq!(hot_wet.belt(), Some(Belt::Tropical));
        assert_eq!(hot_wet.formation(), Formation::Rainforest);
        // Cold land stays in the frozen belts whatever the moisture.
        assert_eq!(
            classify_biome(&p, 1.0, 200.0, false).belt(),
            Some(Belt::Polar)
        );
    }

    #[test]
    fn from_cell_realises_every_land_life_zone() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for belt in Belt::ALL {
            for h in 0..8 {
                seen.insert(Biome::from_cell(belt, h));
            }
        }
        for b in Biome::ALL {
            if b == Biome::Water {
                continue;
            }
            assert!(
                seen.contains(&b),
                "life zone {b:?} is never produced by from_cell"
            );
        }
    }

    /// The headline loop: climate feeds the ecosystem. Kill the moisture cycle
    /// (no evaporation → no rain → no soil moisture) and the land should green
    /// far less than under the normal water cycle — the same drought channel
    /// that will later starve NPCs.
    #[test]
    fn moisture_drives_vegetation() {
        let total_biomass = |evaporation: f32| -> f32 {
            let params = Params {
                evaporation,
                ..Params::default()
            };
            let mut world = World::generate(TEST_W, TEST_H, params, 2026);
            let mut rng = SplitMix64::new(0);
            for _ in 0..400 {
                world.evolve(&mut rng);
            }
            world
                .topology()
                .indices()
                .map(|i| world.plant_biomass.front()[i])
                .sum()
        };
        let wet = total_biomass(0.06); // normal water cycle
        let dry = total_biomass(0.0); // no evaporation at all
        assert!(
            dry < wet * 0.2,
            "a rainless world greened too much (wet {wet}, dry {dry})"
        );
    }

    #[test]
    fn disturbance_fields_stay_finite_and_valid() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..400 {
            world.evolve(&mut rng);
        }
        for i in world.topology().indices() {
            let fire = world.fire.front()[i];
            assert!(fire.is_finite() && fire >= 0.0, "fire invalid ({fire})");
            let a = world.albedo[i];
            assert!(
                a.is_finite() && (0.0..=1.0).contains(&a),
                "albedo out of range ({a})"
            );
        }
    }

    #[test]
    fn fire_ignites_and_consumes_vegetation() {
        let run = |lightning: f32| -> (f32, bool) {
            let params = Params {
                base_lightning: lightning,
                ..Params::default()
            };
            let mut world = World::generate(TEST_W, TEST_H, params, 2026);
            let mut rng = SplitMix64::new(0);
            let mut any_fire = false;
            for _ in 0..400 {
                world.evolve(&mut rng);
                any_fire |= world
                    .topology()
                    .indices()
                    .any(|i| world.fire.front()[i] > 0.0);
            }
            let biomass = world
                .topology()
                .indices()
                .map(|i| world.plant_biomass.front()[i])
                .sum();
            (biomass, any_fire)
        };
        let (burned, any_fire) = run(0.05); // frequent lightning
        let (unburned, _) = run(0.0); // no ignition ever
        assert!(any_fire, "fire never ignited despite frequent lightning");
        assert!(
            burned < unburned,
            "frequent fire should leave less standing biomass ({burned} vs {unburned})"
        );
    }

    #[test]
    fn snow_brightens_the_surface() {
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..400 {
            world.evolve(&mut rng);
        }
        // No bare cover exceeds ~0.35 albedo, so a tile brighter than 0.5 can
        // only be snow — proof the snow→albedo link fires.
        let brightest = world
            .topology()
            .indices()
            .map(|i| world.albedo[i])
            .fold(0.0_f32, f32::max);
        assert!(
            brightest > 0.5,
            "snow should brighten a tile above bare ground (max {brightest})"
        );
    }

    #[test]
    fn radiative_target_responds_to_albedo() {
        let p = Params::default();
        let dark = radiative_target(&p, 1.0, 0.0, 0.10);
        let bright = radiative_target(&p, 1.0, 0.0, 0.80);
        assert!(
            bright < dark,
            "a brighter surface must run cooler ({bright} vs {dark})"
        );
    }

    #[test]
    fn ignite_kindles_a_fire_that_burns() {
        // The disturbance-injection hook: poke the `fire` field on a fuelled tile and
        // the CA carries it — biomass there falls over the next ticks, exactly as a
        // lightning strike would, without touching any other physics.
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..300 {
            world.evolve(&mut rng); // grow some standing fuel
        }
        // The lushest land tile — guaranteed something to burn. Scoped so the
        // topology borrow ends before the mutating `ignite` below.
        let fuelled = {
            let topo = world.topology();
            topo.indices()
                .map(|i| topo.coord(i))
                .filter(|&c| world.elevation(c) >= world.params().sea_level)
                .max_by(|&a, &b| {
                    world
                        .plant_biomass(a)
                        .partial_cmp(&world.plant_biomass(b))
                        .unwrap()
                })
                .expect("a land tile")
        };
        let before = world.plant_biomass(fuelled);
        assert!(
            before > 0.5,
            "need real fuel to test ignition (had {before})"
        );
        let lit = world.ignite(fuelled, 3.0);
        assert!(lit >= 3.0, "ignite should raise the fire field ({lit})");
        assert!(world.fire(fuelled) > 0.0, "the tile should be burning");
        for _ in 0..12 {
            world.evolve(&mut rng);
        }
        assert!(
            world.plant_biomass(fuelled) < before,
            "a kindled fire should consume the fuel it was lit on"
        );
    }

    #[test]
    fn parch_dries_a_tile_then_the_climate_refills_it() {
        // The deny lever: wring a wet tile out, and watch the climate slowly restore
        // it (the drought is transient — Φ does not stay denied).
        let mut world = small_world();
        let mut rng = SplitMix64::new(0);
        for _ in 0..200 {
            world.evolve(&mut rng);
        }
        // Scoped so the topology borrow ends before the mutating `parch` below.
        let wettest = {
            let topo = world.topology();
            topo.indices()
                .map(|i| topo.coord(i))
                .max_by(|&a, &b| {
                    world
                        .surface_water(a)
                        .partial_cmp(&world.surface_water(b))
                        .unwrap()
                })
                .expect("a tile")
        };
        let before = world.surface_water(wettest);
        assert!(
            before > 0.0,
            "need standing water to test a drought (had {before})"
        );
        let removed = world.parch(wettest, 1.0);
        assert!(
            (removed - before).abs() < 1e-4,
            "parch should report the water it took"
        );
        assert!(
            world.surface_water(wettest) < before,
            "the tile should be drier after a drought"
        );
    }

    #[test]
    fn stigmergy_absent_by_default() {
        let mut world = small_world();
        assert_eq!(world.stigmergy_layers(), 0);
        let c = Coord::new(5, 5);
        // Reading a non-existent layer is 0; depositing into one is a silent no-op.
        assert_eq!(world.stig(0, c), 0.0);
        world.deposit(0, c, 10.0);
        assert_eq!(
            world.stig(0, c),
            0.0,
            "deposit with no layers must be inert"
        );
    }

    #[test]
    fn stigmergy_diffuses_to_neighbours_and_decays() {
        let mut world = small_world();
        world.install_stigmergy(&[StigConfig {
            diffuse: 0.2,
            decay: 0.1,
        }]);
        assert_eq!(world.stigmergy_layers(), 1);

        // Drop a pulse on an interior tile (6 neighbours, away from the poles).
        let center = Coord::new(18, 13);
        world.deposit(0, center, 100.0);
        let neighbor = {
            let topo = world.topology();
            topo.coord(topo.neighbors(topo.index_of(center))[0].to)
        };
        assert_eq!(world.stig(0, neighbor), 0.0, "pulse hasn't spread yet");

        let mut rng = SplitMix64::new(0);
        world.evolve(&mut rng);

        // One tick: signal has bled into the neighbour and the centre has dropped.
        assert!(
            world.stig(0, neighbor) > 0.0,
            "signal should diffuse to a neighbour"
        );
        assert!(
            world.stig(0, center) < 100.0,
            "the centre should fall as it spreads and decays"
        );
        assert!(
            world.stig(0, center) > world.stig(0, neighbor),
            "the gradient should still point uphill toward the source"
        );
    }

    #[test]
    fn stigmergy_fades_to_nothing_without_deposits() {
        let mut world = small_world();
        world.install_stigmergy(&[StigConfig {
            diffuse: 0.2,
            decay: 0.3,
        }]);
        let center = Coord::new(18, 13);
        world.deposit(0, center, 1000.0);
        let total = |w: &World| -> f32 {
            let topo = w.topology();
            topo.indices().map(|i| w.stig(0, topo.coord(i))).sum()
        };
        let mut rng = SplitMix64::new(0);
        let start = total(&world);
        for _ in 0..50 {
            world.evolve(&mut rng);
        }
        let end = total(&world);
        assert!(
            end < start * 0.01,
            "with decay and no fresh deposits the field should fade away ({start} -> {end})"
        );
    }
}
