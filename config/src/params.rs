//! Every tunable constant in one place, so the physics in `Φ` reads cleanly and
//! the model is balanced by editing data, not code.
//!
//! Values are *gamey but grounded* (per the chosen fidelity): real mechanisms —
//! environmental lapse rate, axial-tilt seasonality, a latitude temperature
//! gradient — expressed as simple constants. Elevation is in metres so the
//! lapse rate is the familiar ~6.5 °C/km; temperatures are in °C.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Params {
    // --- Time ---
    /// Ticks in one orbit. One tick = one day, so 365 by default. Drives the
    /// seasonal cycle; changing it rescales seasonality, not the day length.
    pub ticks_per_year: u32,
    /// Axial tilt in degrees — the amplitude of the sub-solar latitude's
    /// seasonal swing (Earth ≈ 23.5°).
    pub axial_tilt_deg: f32,
    /// Tick (day-of-year) at which the sub-solar point crosses the equator
    /// heading north — the spring equinox. Phases the seasons.
    pub spring_equinox_tick: f32,

    // --- Geology / world-gen ---
    /// Elevation datum: tiles below this are sea, above are land (metres).
    pub sea_level: f32,
    /// Number of tectonic plates the generator seeds.
    pub plates: u32,
    /// Base elevation of continental vs oceanic crust (metres).
    pub continental_base: f32,
    pub oceanic_base: f32,
    /// Fraction of the surface that should end up above sea level; the generator
    /// sets the datum to hit this (≈0.29 like Earth).
    pub land_fraction: f32,

    // --- World-gen: tectonics ---
    /// Peak uplift (metres) a head-on convergent boundary adds, and how many
    /// hexes inland that uplift decays over.
    pub plate_uplift: f32,
    pub uplift_falloff: f32,
    /// Height of a mid-ocean / continental ridge at a divergent boundary (m).
    pub ridge_height: f32,
    /// Depth of the trench gouged where an oceanic plate subducts (m).
    pub trench_depth: f32,
    /// Amplitude (m) and octave count of the wrap-correct fractal detail noise.
    pub tectonic_noise: f32,
    pub noise_octaves: u32,

    // --- World-gen: erosion ---
    /// Number of stream-power erosion passes (each re-routes flow).
    pub erosion_iterations: u32,
    /// Stream-power law `Δz = K · Aᵐ · Sⁿ`: incision rate and the drainage-area /
    /// slope exponents (m≈0.5, n≈1 are the standard detachment-limited values).
    pub stream_power_k: f32,
    pub stream_power_m: f32,
    pub stream_power_n: f32,
    /// Hillslope (thermal) diffusion strength per pass — rounds ridges, fills
    /// hollows, the creep that complements river incision.
    pub thermal_erosion: f32,
    /// Tiny gradient imposed by Priority-Flood+ε so filled basins still drain.
    pub fill_epsilon: f32,

    // --- World-gen: precipitation & rivers ---
    /// Iterations of moisture advection along the prevailing winds.
    pub wg_precip_iters: u32,
    /// Base fraction of moisture that rains out per step, and the extra rained
    /// out per metre of windward upslope (the orographic term).
    pub wg_precip_base: f32,
    pub wg_orographic: f32,
    /// Flow accumulation above which a cell counts as a river channel.
    pub river_threshold: f32,
    /// Standing water assigned per unit of river discharge and per metre a lake
    /// basin was filled.
    pub river_water: f32,
    pub lake_water: f32,

    // --- Climate: temperature ---
    /// Environmental lapse rate: °C lost per metre of elevation above sea level
    /// (~0.0065 ≈ 6.5 °C/km).
    pub lapse_rate: f32,
    /// Radiative target temperatures (°C) at full sun and no sun, before lapse —
    /// the warm and cold ends of the latitude/season gradient.
    pub equator_temp: f32,
    pub pole_temp: f32,
    /// Per-tick relaxation of temperature toward its radiative target (0..1):
    /// thermal inertia. Higher = the climate chases the sun faster.
    pub temp_relax: f32,
    /// Per-tick horizontal diffusion of temperature toward the neighbour mean
    /// (0..1): winds/oceans smearing heat across latitudes.
    pub temp_diffuse: f32,

    // --- Climate: pressure & wind ---
    /// Reference surface pressure (arbitrary units ≈ hPa) at the reference
    /// temperature and sea level.
    pub base_pressure: f32,
    /// Pressure drop per °C above the reference: warm air rises → low pressure.
    pub pressure_temp_coeff: f32,
    /// Temperature (°C) at which a sea-level tile sits at `base_pressure`.
    pub pressure_temp_ref: f32,
    /// Pressure drop per metre of elevation (thinner air higher up). Kept small:
    /// surface pressure for wind is essentially reduced to sea level, so terrain
    /// nudges the flow rather than becoming a bottomless low that winds rush into.
    pub pressure_elev_coeff: f32,
    /// Wind speed produced per unit of pressure gradient (wind blows down-gradient).
    pub wind_coeff: f32,

    // --- Climate: humidity & precipitation ---
    /// Moisture added per tick by a warm, fully-wet tile (ocean / standing water).
    pub evaporation: f32,
    /// Advected fraction of a tile's moisture per unit wind speed each tick…
    pub humidity_advect: f32,
    /// …capped here, so a strong wind can't move more than this share in a tick
    /// (numerical stability).
    pub max_advect: f32,
    /// Wind-independent spreading of moisture toward the neighbour mean each tick
    /// (turbulent mixing) — lets damp air reach inland even under weak wind.
    pub humidity_diffuse: f32,
    /// Saturation moisture a tile can hold at 0 °C…
    pub saturation_base: f32,
    /// …growing by this factor per °C (a Clausius–Clapeyron-style exponential):
    /// warm air holds much more, so cooling or lifting wrings out rain.
    pub saturation_growth: f32,
    /// Extra rain per metre of upslope terrain in the wind's path (orographic).
    pub orographic_coeff: f32,

    // --- Climate: surface water & snow ---
    /// Fraction of standing water a tile keeps from tick to tick (the rest is
    /// available to flow on); models infiltration/evaporation losses indirectly.
    pub water_retain: f32,
    /// Fraction of a tile's water that flows to its steepest-downhill neighbour
    /// each tick — the river-forming move.
    pub water_flow: f32,
    /// Temperature (°C) below which precipitation falls as snow and water freezes.
    pub freeze_temp: f32,
    /// Snow melted per °C above freezing per tick (meltwater rejoins surface water).
    pub melt_rate: f32,

    // --- Ecosystem: plant growth ---
    /// Ceiling on standing plant biomass — the lushest a tile can ever be.
    pub biomass_max: f32,
    /// Intrinsic growth rate `r` in the logistic production term.
    pub plant_growth: f32,
    /// Colonisation floor so bare-but-fertile ground can green from nothing.
    pub plant_seed: f32,
    /// Fraction of standing biomass that dies to litter each tick.
    pub plant_mortality: f32,
    /// Optimal growth temperature (°C) and the width of the tolerance bell.
    pub growth_temp_opt: f32,
    pub growth_temp_width: f32,
    /// Rain contributes this many moisture units per unit of precipitation
    /// (standing water counts directly).
    pub rain_to_moisture: f32,
    /// Moisture at which the water-limited growth factor is half its max.
    pub moisture_half: f32,
    /// Soil nutrient level at which the nutrient growth factor is half its max.
    pub nutrient_half: f32,

    // --- Ecosystem: soil ---
    /// Starting soil nutrient stock at world-gen.
    pub soil_nutrients_init: f32,
    /// Mineral nutrient added by bedrock weathering each tick.
    pub weathering: f32,
    /// Base fraction of litter decomposed per tick (scaled by warmth & moisture).
    pub decomposition: f32,
    /// Share of decomposed matter stabilised into soil carbon (the rest is lost as CO₂).
    pub humification: f32,
    /// Fraction of soil carbon respired away each tick.
    pub soil_respiration: f32,
    /// Share of decomposition released as plant-available nutrients.
    pub mineralization: f32,
    /// Soil nutrients drawn down per unit of plant production.
    pub nutrient_uptake: f32,
    /// Q10 temperature sensitivity of decomposition — the factor decomposers speed up
    /// per 10 °C (≈2, the near-universal soil default), replacing a linear warmth term
    /// so warm soils respire and cycle nutrients much faster than cool ones.
    pub decomp_q10: f32,
    /// Reference temperature (°C) the Q10 response is normalised to (activity = 1 here).
    pub decomp_ref_temp: f32,
    /// How much accumulated soil carbon lifts a tile's carrying capacity — the slow
    /// soil→fertility feedback that gives the ecosystem *memory*: a long-vegetated tile
    /// builds rich soil that supports more growth, so productivity persists and damps
    /// the boom-bust a climate-only ceiling suffers. The maximum fractional boost.
    pub carbon_fertility: f32,
    /// Soil carbon at which that fertility boost is half its maximum.
    pub carbon_half: f32,

    // --- Ecosystem: Holdridge biome classification ---
    /// Mean-annual *biotemperature* (°C) boundaries between the Holdridge
    /// latitudinal belts (polar→subpolar→boreal→cool→warm→subtropical→tropical).
    /// Real degrees — the model's temperatures are already in °C, so these are the
    /// canonical 1.5/3/6/12/18/24 chart lines.
    pub biotemp_subpolar: f32,
    pub biotemp_boreal: f32,
    pub biotemp_cool_temperate: f32,
    pub biotemp_warm_temperate: f32,
    pub biotemp_subtropical: f32,
    pub biotemp_tropical: f32,
    /// Potential-evapotranspiration coefficient: `PET = biotemperature × pet_coeff`
    /// (Holdridge's 58.93 mm·°C⁻¹, rescaled to the model's precipitation units).
    /// The single knob that calibrates wet↔dry: the resulting PET/precipitation
    /// ratio is bucketed into the eight humidity provinces on a log₂ ladder.
    pub pet_coeff: f32,
    /// How many years of climate a tile's biome remembers — the time constant of
    /// the running annual biotemperature / precipitation averages it is classified
    /// from. Larger = biomes lag the weather more and drift more slowly.
    pub biome_memory_years: f32,

    // --- Ecosystem: per-biome ecology (the biome organises the ecosystem) ---
    /// Per-**formation** productivity ceiling — the fraction of `biomass_max` a
    /// biome of that structural class can carry, before a warmth scaling by belt.
    /// The biome (distilled from the climate) — not the raw climate curve — now
    /// sets how lush a tile may become: barren desert, sparse tundra, rich grass
    /// and shrub, full forest, teeming rainforest. The single strongest lever on
    /// how green (and how fantastical) a world reads.
    pub prod_desert: f32,
    pub prod_tundra: f32,
    pub prod_grass: f32,
    pub prod_shrub: f32,
    pub prod_forest: f32,
    pub prod_rainforest: f32,

    // --- Disturbance: fire (stochastic CA) ---
    /// Per-tick lightning ignition chance on a maximally dry, fuelled tile.
    pub base_lightning: f32,
    /// Temperature (°C) where fuel starts drying, and the span to fully dry.
    pub fire_dry_temp: f32,
    pub fire_dry_scale: f32,
    /// Moisture at which dryness is halved (damp fuel resists ignition).
    pub fire_moisture_half: f32,
    /// Fuel (biomass-equivalent) at which the fuel factor is half its max.
    pub fire_fuel_half: f32,
    /// Minimum fuel needed to ignite or keep burning.
    pub fire_fuel_min: f32,
    /// Weight of litter as fuel relative to standing biomass.
    pub fire_litter_weight: f32,
    /// Base chance to catch from a single burning neighbour…
    pub base_spread: f32,
    /// …multiplied up when that neighbour's wind blows toward this tile…
    pub wind_spread: f32,
    /// …and when this tile is uphill of it (fire climbs).
    pub slope_spread: f32,
    /// Metres of rise that give the full uphill spread bonus.
    pub fire_slope_scale: f32,
    /// Fraction of standing biomass / litter a burning tile consumes per tick.
    pub fire_consume: f32,
    pub fire_litter_consume: f32,
    /// Share of burned matter returned to soil as nutrients (ash) and charcoal;
    /// the remainder is lost to the atmosphere (fire is not conservative).
    pub fire_ash: f32,
    pub fire_ash_carbon: f32,

    // --- Disturbance: albedo (and its temperature feedback) ---
    /// Base surface reflectance by structural cover (the Holdridge `Formation`):
    /// dark canopies, mid scrub/grass, bright bare ground and cold pale tundra.
    pub albedo_water: f32,
    pub albedo_desert: f32,
    pub albedo_tundra: f32,
    pub albedo_grass: f32,
    pub albedo_shrub: f32,
    pub albedo_forest: f32,
    pub albedo_rainforest: f32,
    /// Reflectance of full snow cover and of a fresh (charred) burn.
    pub albedo_snow: f32,
    pub albedo_burn: f32,
    /// Snow depth giving full snow albedo, and the fire intensity giving a full
    /// burn darkening.
    pub snow_albedo_cover: f32,
    pub fire_albedo_ref: f32,
    /// How strongly a burn darkens the surface (0..1 blend toward `albedo_burn`).
    pub burn_albedo_weight: f32,
    /// Albedo that applies no temperature offset; departures cool (brighter) or
    /// warm (darker) the tile by `albedo_temp_coeff` °C per unit.
    pub albedo_ref: f32,
    pub albedo_temp_coeff: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            ticks_per_year: 365,
            axial_tilt_deg: 23.5,
            spring_equinox_tick: 80.0, // ≈ 21 March

            sea_level: 0.0,
            plates: 8,
            continental_base: 300.0,
            oceanic_base: -2000.0,
            land_fraction: 0.29,

            plate_uplift: 5000.0,
            uplift_falloff: 4.0,
            ridge_height: 800.0,
            trench_depth: 3000.0,
            tectonic_noise: 600.0,
            noise_octaves: 5,

            erosion_iterations: 25,
            stream_power_k: 0.30,
            stream_power_m: 0.5,
            stream_power_n: 1.0,
            thermal_erosion: 0.10,
            fill_epsilon: 0.01,

            wg_precip_iters: 40,
            wg_precip_base: 0.08,
            wg_orographic: 0.0015,
            river_threshold: 0.4,
            river_water: 2.0,
            lake_water: 0.5,

            lapse_rate: 0.0065,
            equator_temp: 30.0,
            pole_temp: -25.0,
            temp_relax: 0.15,
            temp_diffuse: 0.10,

            base_pressure: 1013.0,
            pressure_temp_coeff: 1.2,
            pressure_temp_ref: 15.0,
            pressure_elev_coeff: 0.003,
            wind_coeff: 0.1,

            evaporation: 0.06,
            humidity_advect: 0.20,
            max_advect: 0.6,
            humidity_diffuse: 0.12,
            saturation_base: 5.0,
            saturation_growth: 0.06,
            orographic_coeff: 0.0006,

            water_retain: 0.5,
            water_flow: 0.4,
            freeze_temp: 0.0,
            melt_rate: 0.05,

            biomass_max: 10.0,
            plant_growth: 0.25,
            plant_seed: 0.02,
            plant_mortality: 0.03,
            growth_temp_opt: 25.0,
            growth_temp_width: 14.0,
            rain_to_moisture: 2.0,
            moisture_half: 1.5,
            nutrient_half: 0.3,

            soil_nutrients_init: 0.4,
            weathering: 0.004,
            decomposition: 0.10,
            humification: 0.30,
            soil_respiration: 0.02,
            mineralization: 0.50,
            nutrient_uptake: 0.04,
            decomp_q10: 2.0,
            decomp_ref_temp: 15.0,
            carbon_fertility: 1.0,
            carbon_half: 2.0,

            biotemp_subpolar: 1.5,
            biotemp_boreal: 3.0,
            biotemp_cool_temperate: 6.0,
            biotemp_warm_temperate: 12.0,
            biotemp_subtropical: 18.0,
            biotemp_tropical: 24.0,
            pet_coeff: 3.0,
            biome_memory_years: 1.0,

            prod_desert: 0.05,
            prod_tundra: 0.25,
            prod_grass: 0.6,
            prod_shrub: 0.4,
            prod_forest: 0.9,
            prod_rainforest: 1.2,

            base_lightning: 0.0008,
            fire_dry_temp: 12.0,
            fire_dry_scale: 25.0,
            fire_moisture_half: 0.6,
            fire_fuel_half: 2.0,
            fire_fuel_min: 0.3,
            fire_litter_weight: 0.5,
            base_spread: 0.35,
            wind_spread: 1.5,
            slope_spread: 1.0,
            fire_slope_scale: 600.0,
            fire_consume: 0.5,
            fire_litter_consume: 0.5,
            fire_ash: 0.10,
            fire_ash_carbon: 0.05,

            albedo_water: 0.06,
            albedo_desert: 0.35,
            albedo_tundra: 0.30,
            albedo_grass: 0.20,
            albedo_shrub: 0.25,
            albedo_forest: 0.13,
            albedo_rainforest: 0.11,
            albedo_snow: 0.70,
            albedo_burn: 0.06,
            snow_albedo_cover: 0.5,
            fire_albedo_ref: 1.0,
            burn_albedo_weight: 1.0,
            albedo_ref: 0.20,
            albedo_temp_coeff: 15.0,
        }
    }
}
