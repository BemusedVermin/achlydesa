//! The game's **tunable configuration** — every behaviour knob, as plain data.
//!
//! These are the struct-shaped counterparts to the list-shaped content in
//! [`crate::assets`]: economy rates, need decay, fauna dynamics, the drama
//! manager's weights, and so on. They are deliberately **Bevy-free primitives**
//! — `serde` in, `serde` out, no ECS — so this crate never drags the engine in.
//! The crate that runs the simulation wraps them in its own ECS resources.
//!
//! Each is loaded the same layered way (see [`load`]): start from the built-in
//! [`Default`], then merge any RON override found in `assets/config/` on top,
//! via [`figment`]. So a knob can be retuned by authoring a small RON file,
//! without recompiling — and a missing file just means the defaults.

use crate::Params;
use figment::Figment;
use figment::providers::Serialized;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};

/// Number of feature categories. The fixed length of [`FeatureConfig::density`].
///
/// This must stay equal to the simulation's `Category` count; the consuming
/// crate asserts that at compile time so the two can't drift.
pub const FEATURE_CATEGORY_COUNT: usize = 4;

/// Global economic behaviour. Per-good prices, per-recipe yields, and per-skill
/// learning rates all live in the registry; this holds only economy-wide knobs.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EconConfig {
    /// Price clamp as a fraction of a good's base price.
    pub price_floor_frac: f32,
    pub price_ceil_frac: f32,
    /// Whole units moved per trade.
    pub trade_lot: u32,
    /// How fast a market's `price_basis` chases its real stock each tick (`0..1`):
    /// `1` = no smoothing (price tracks stock instantly, the old cobweb-prone
    /// behaviour), smaller = more inertia. Damps synchronized over/under-production.
    pub price_smoothing: f32,
}

impl Default for EconConfig {
    fn default() -> Self {
        Self {
            price_floor_frac: 0.1,
            price_ceil_frac: 10.0,
            trade_lot: 5,
            price_smoothing: 0.15,
        }
    }
}

/// Global need behaviour.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct NeedsConfig {
    pub initial_sustenance: f32,
    pub initial_rest: f32,
    pub hunger_rate: f32,
    pub fatigue_rate: f32,
    pub rest_recovery: f32,
    /// Sustenance from grazing the tile — the subsistence floor.
    pub eat_grass_relief: f32,
}

impl Default for NeedsConfig {
    fn default() -> Self {
        Self {
            initial_sustenance: 60.0,
            initial_rest: 60.0,
            hunger_rate: 2.0,
            fatigue_rate: 1.5,
            rest_recovery: 20.0,
            eat_grass_relief: 9.0,
        }
    }
}

/// Tunable fauna behaviour (global knobs only).
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FaunaConfig {
    pub initial_energy: f32,
    pub metabolism: f32,
    pub intake: f32,
    pub eat_rate: f32,
    pub repro_threshold: f32,
    pub repro_cost: f32,
    /// Most herbivores that may breed on one tile in a tick — a crowding cap, the
    /// density-dependent regulation that stops the herd overshooting its forage and
    /// mass-starving (the boom-bust a pure logistic alone suffers).
    pub herd_cap: usize,
    /// How strongly herbivores are drawn to **company** when choosing where to move,
    /// relative to forage — a herding instinct. It makes scattered animals coalesce
    /// into moving herds dense enough to graze efficiently *and* to be worth a
    /// predator's hunt, so prey isn't so thinly spread the trophic loop starves at the
    /// top. `0` = pure ideal-free dispersal by forage alone.
    pub herd_cohesion: f32,

    // --- Carnivores ---
    pub carn_initial_energy: f32,
    pub carn_metabolism: f32,
    /// Holling type-II **attack rate** `a` and **handling time** `h`: per-tick kills
    /// are `a·N / (1 + a·h·N)` for `N` prey on the tile — so intake *saturates* as
    /// prey gets dense (a predator can only process so many), the response that
    /// stabilises real predator–prey systems where a linear (type-I) one wouldn't.
    pub carn_attack: f32,
    pub carn_handling: f32,
    /// Energy a predator gains per kill.
    pub carn_energy_per_kill: f32,
    pub carn_repro_threshold: f32,
    pub carn_repro_cost: f32,
    /// Prey on a tile holding fewer than this many are safe — a **spatial refuge**
    /// (scattered animals hide where a pack can't profitably hunt). It is what stops
    /// predators chasing the herd to total extinction, so the loop can persist
    /// instead of collapsing (a known stabiliser against the paradox of enrichment).
    pub carn_prey_refuge: usize,
}

impl Default for FaunaConfig {
    fn default() -> Self {
        Self {
            initial_energy: 50.0,
            metabolism: 0.3,
            intake: 0.8,
            eat_rate: 8.0,
            repro_threshold: 80.0,
            repro_cost: 40.0,
            herd_cap: 5,
            herd_cohesion: 3.0,

            carn_initial_energy: 60.0,
            // Patient predators: low upkeep so they ride out lean stretches between
            // kills, moderate attack and slow breeding so a pack tracks the herd
            // rather than over-culling it and starving (the predator–prey collapse).
            carn_metabolism: 0.55,
            carn_attack: 0.5,
            carn_handling: 1.2,
            carn_energy_per_kill: 18.0,
            carn_repro_threshold: 170.0,
            carn_repro_cost: 80.0,
            carn_prey_refuge: 2,
        }
    }
}

/// Knobs for the dialogue layer. Off by default → a dialogue-free world is unchanged.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DialogueConfig {
    pub enabled: bool,
    /// Ticks a speaker waits between utterances (so the world isn't a wall of chatter).
    pub cooldown: u64,
    /// The least appeal an intent must reach for an agent to bother saying it — below
    /// this it has nothing worth saying (the conversational analogue of the impact floor).
    pub appeal_floor: f32,
    /// How many memories an NPC keeps; and how fast a memory's strength fades per tick.
    pub memory_cap: usize,
    pub forget_rate: f32,
    /// Penalty to re-saying the same act to the same listener too soon (anti-repetition).
    pub echo_penalty: f32,
}

impl Default for DialogueConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cooldown: 6,
            appeal_floor: 0.18,
            memory_cap: 16,
            forget_rate: 0.01,
            echo_penalty: 0.5,
        }
    }
}

/// Knobs for the drama manager. Off by default, so a director-free world is unchanged.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DirectorConfig {
    pub enabled: bool,
    /// Ticks between beats — the cadence at which `Γ` pushes the story onward.
    pub beat_interval: u64,
    /// Hexes around the protagonist a beat's wake is watched (for attribution) and a
    /// marvel/region disaster reaches by default.
    pub reach: i32,

    /// The least **impact** (`drama × cast-fit`) a beat must reach for the director to
    /// tell it. Below this, no castable beat is dramatic enough — so a peaceful,
    /// provisioned, forgiving world `Γ` can find no leverage in falls **silent**. The
    /// knob that makes the *world*, not a button, the thing that quiets the director.
    pub impact_floor: f32,

    /// Novelty heat added to a told beat (and its register/tags), and how fast it cools —
    /// the diversity pressure that keeps the register rotating.
    pub novelty_heat: f32,
    pub novelty_cool: f32,
    /// Sample among the top-`shortlist` scored beats, so the telling varies.
    pub shortlist: usize,

    /// How many threads run at once (decision #13: a few interleaved stories).
    pub max_threads: usize,
    /// Drama multiplier for **trunk** (betrayal/vengeance) beats, so betrayal dominates
    /// *emergently* (decision #17) rather than by a hard rule.
    pub trunk_bonus: f32,
    /// Scoring multipliers: a beat that suits the active thread's phase is favoured; one
    /// that doesn't is damped; one whose register *is* the thread's spine is favoured.
    pub phase_match: f32,
    pub phase_miss: f32,
    pub spine_match: f32,
    /// When a climax is **timed onto another thread's high** (a collision), this bonus,
    /// fired with this probability.
    pub collision_bonus: f32,
    pub collision_chance: f32,

    /// Attachment = `1 + prominence / prom_scale`. The audience's investment, manufactured.
    pub prom_scale: f32,
    /// Prominence trickled to every living soul each interval (mere presence), and the
    /// fraction of all prominence that persists each interval (slow fade, so the
    /// audience's attachment lingers past the avatar's death).
    pub presence_gain: f32,
    pub prominence_decay: f32,
    /// Prominence a beat confers on each cast member (being *featured*), and the extra a
    /// thread's *Setup* grooming confers on its chosen victim — *the game grooms your
    /// affection on purpose.*
    pub feature_gain: f32,
    pub groom_gain: f32,
    /// A prominence floor the protagonist is held to (the avatar is always somewhat the
    /// audience's), and the ceiling all prominence is clamped to.
    pub proto_seed: f32,
    pub prom_cap: f32,
    /// Heat a thread must bank before it ripens to a climax — scaled up by the lead's
    /// prominence, so the most-invested figure gets the longest slow burn (variable
    /// tempo, decision #18).
    pub ripeness_base: f32,

    /// Opinion past which someone casts as an Ally (warm) or a Foe (cold).
    pub ally_threshold: f32,
    pub foe_threshold: f32,
    /// Sustenance/rest below which the protagonist reads as imperilled (ambient readout).
    pub peril: f32,
    /// EMA factor for the ambient tension readout (inspection only; not the objective).
    pub tension_smoothing: f32,
    /// Moral arithmetic: suffering per manufactured wound is scaled by this; grief per
    /// death in a beat's wake; how long that wake is watched; the weight brighter affect
    /// carries in the *staged-experience* total (suffering carries 1.0).
    pub anguish_scale: f32,
    pub grief_per_death: f32,
    pub wake_ttl: u64,
    pub bright_weight: f32,
}

impl Default for DirectorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            beat_interval: 14,
            reach: 3,
            impact_floor: 1.0,
            novelty_heat: 2.0,
            novelty_cool: 0.03,
            shortlist: 3,
            max_threads: 3,
            trunk_bonus: 2.0,
            phase_match: 1.6,
            // Strong penalty for a beat told out of its arc phase. Kept low so a high-stakes
            // *climax* beat does not out-score the Rising beats and fire early — which would
            // lurch the thread Rising → Fall and skip the Climax phase the director needs to
            // pass through (and time onto a high). Climaxes belong to the climax.
            phase_miss: 0.3,
            spine_match: 1.4,
            collision_bonus: 1.7,
            collision_chance: 0.5,
            prom_scale: 1.5,
            presence_gain: 0.006,
            prominence_decay: 0.985,
            feature_gain: 0.5,
            groom_gain: 0.9,
            proto_seed: 0.6,
            prom_cap: 8.0,
            // The heat a thread banks before it ripens to a climax. Kept low enough that a
            // thread reaches the **Climax phase** before a high-stakes climax beat would
            // otherwise fire during its Rising — so the director genuinely passes through
            // climaxes (and can time them onto highs) rather than lurching Rising → Fall.
            // Scaled up per the lead's prominence (the most-invested figure gets the longest
            // burn), so a modest base still yields a slow build for the groomed protagonist.
            ripeness_base: 1.0,
            ally_threshold: 0.08,
            foe_threshold: -0.08,
            peril: 25.0,
            tension_smoothing: 0.25,
            anguish_scale: 1.0,
            grief_per_death: 4.0,
            wake_ttl: 24,
            bright_weight: 0.3,
        }
    }
}

/// Faction-turn knobs.
#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FactionConfig {
    /// Ticks between faction turns — factions act on a slower clock than people.
    pub period: u64,
    /// Members a court needs before it counts as a faction at all.
    pub min_members: usize,
    /// How far (hexes) a court's pull reaches to recruit.
    pub reach: i32,
    /// Most factions one person may belong to at once.
    pub max_factions: usize,
    /// Size of an oligarchy's ruling council.
    pub council_size: usize,
    /// Fraction of a member's coin its leader levies each faction turn (scaled by the
    /// government). Paid to a leader personally, so money is conserved — it flows up.
    pub tax_rate: f32,
    /// Loyalty lost per unit of effective tax — the resentment tribute breeds.
    pub tax_pain: f32,
    /// Loyalty a person rests at (and joins a new faction at).
    pub loyalty_base: f32,
    /// Per-turn pull of loyalty back toward baseline.
    pub loyalty_decay: f32,
    /// How much pride in a strong bloc (high Force) raises loyalty.
    pub strength_pride: f32,
    /// How much loyalty bends a member toward (and, soured, off) its current faction.
    pub loyalty_inertia: f32,
    /// Force ratio above which the stronger of two neighbouring factions declares war.
    pub war_force_ratio: f32,
    /// Ticks a detained law-breaker is held.
    pub detain_ticks: u32,
    /// Force a faction needs to *execute* (not merely detain) a repeat law-breaker.
    pub execute_force: f32,
    /// How fast a person's opinion of a leader it serves moves toward how it feels about
    /// the bloc (its loyalty).
    pub opinion_gain: f32,
    /// Opinion lost toward an enemy leader each turn at war.
    pub war_enmity: f32,
    /// Per-turn fade of every opinion toward indifference.
    pub opinion_decay: f32,
    /// How much opinion of a court's leader bends a person toward (or off) joining it.
    pub opinion_weight: f32,
}

impl Default for FactionConfig {
    fn default() -> Self {
        Self {
            period: 20,
            min_members: 3,
            reach: 6,
            max_factions: 2,
            council_size: 3,
            tax_rate: 0.05,
            tax_pain: 1.5,
            loyalty_base: 0.5,
            loyalty_decay: 0.1,
            strength_pride: 0.15,
            loyalty_inertia: 1.2,
            war_force_ratio: 2.0,
            detain_ticks: 30,
            execute_force: 8.0,
            opinion_gain: 0.2,
            war_enmity: 0.25,
            opinion_decay: 0.05,
            opinion_weight: 0.6,
        }
    }
}

/// Global feature-placement knobs (no per-feature data — that lives in the catalog).
#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FeatureConfig {
    /// Per-category multiplier on placement rate (index by category).
    pub density: [f32; FEATURE_CATEGORY_COUNT],
    /// Minimum hexes between communities — the inhibition radius that spaces
    /// settlements out (anti-clumping). `0` disables spacing.
    pub community_spacing: u32,
    /// Hexes over which remoteness saturates to `1`.
    pub remoteness_scale: f32,
    /// Exponent biasing the weighted choice toward the best-fitting kind (higher =
    /// more decisive).
    pub sharpness: f32,
}

impl Default for FeatureConfig {
    fn default() -> Self {
        Self {
            density: [1.0; FEATURE_CATEGORY_COUNT],
            community_spacing: 3,
            remoteness_scale: 8.0,
            sharpness: 3.0,
        }
    }
}

// =====================================================================================
// Loading — built-in defaults, with optional per-knob RON overrides layered on via
// figment. The one place the project reads tunable config; everything else takes the
// returned value.
// =====================================================================================

/// The directory holding the tunables' RON files (`assets/config/`), as known at
/// compile time. A shipped binary that wants overrides should be handed an
/// explicit path instead; absent the folder, the in-code defaults stand.
pub fn config_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("assets")
        .join("config")
}

/// Load one tunable: its built-in [`Default`], with `assets/config/<file>`
/// layered on top via [`figment`]. The file is natural RON struct syntax —
/// `(trade_lot: 99, ...)` — and may give any subset of fields; the rest keep
/// their default (the structs are `#[serde(default)]`). A missing file is not an
/// error (the defaults stand); a malformed one is.
///
/// figment composes the two layers, so further providers (env vars, a second
/// profile) can be slotted in here later without touching call sites.
pub fn load<T>(file: &str) -> Result<T, crate::ConfigError>
where
    T: Default + Serialize + DeserializeOwned,
{
    let path = config_dir().join(file);
    let mut fig = Figment::from(Serialized::defaults(T::default()));
    if path.exists() {
        let overlay: T = crate::parse(&std::fs::read_to_string(&path)?)?;
        fig = fig.merge(Serialized::defaults(overlay));
    }
    // Re-extracting a value we just serialized into figment cannot fail.
    Ok(fig.extract().expect("serialized config round-trips"))
}

/// [`load`] a tunable, treating a malformed file as a fatal authoring error
/// (like the bundled content assets, a checked-in config must be valid).
fn loaded<T>(file: &str) -> T
where
    T: Default + Serialize + DeserializeOwned,
{
    load(file).unwrap_or_else(|e| panic!("invalid config '{file}': {e}"))
}

/// World-physics parameters, defaults with optional `assets/config/params.ron`.
pub fn params() -> Params {
    loaded("params.ron")
}
/// Economy knobs, defaults with optional `assets/config/econ.ron`.
pub fn econ() -> EconConfig {
    loaded("econ.ron")
}
/// Need knobs, defaults with optional `assets/config/needs.ron`.
pub fn needs() -> NeedsConfig {
    loaded("needs.ron")
}
/// Fauna knobs, defaults with optional `assets/config/fauna.ron`.
pub fn fauna() -> FaunaConfig {
    loaded("fauna.ron")
}
/// Dialogue knobs, defaults with optional `assets/config/dialogue.ron`.
pub fn dialogue() -> DialogueConfig {
    loaded("dialogue.ron")
}
/// Director knobs, defaults with optional `assets/config/director.ron`.
pub fn director() -> DirectorConfig {
    loaded("director.ron")
}
/// Faction knobs, defaults with optional `assets/config/faction.ron`.
pub fn faction() -> FactionConfig {
    loaded("faction.ron")
}
/// Feature-placement knobs, defaults with optional `assets/config/feature.ron`.
pub fn feature() -> FeatureConfig {
    loaded("feature.ron")
}

/// Knobs for the **narrative sifter** (and its Chronicle ring). Off by default, so a sift-free
/// world is unchanged.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SiftConfig {
    /// Wake the sift layer — the Chronicle ring, the sifter, and the eval harness.
    pub enabled: bool,
    /// Maximum episodes the Chronicle ring holds: the sifter's window onto recent history.
    pub ring_cap: usize,
    /// **The director graft.** Let the director *consult* the sifter — seed threads on, and lower
    /// resistance toward, the stories the world is already forming. Default off: a sift-on world
    /// with this off runs its director byte-identically to a sift-off world (the sifter only
    /// observes). Requires [`enabled`](Self::enabled).
    pub graft: bool,
    /// The most the graft may multiply a beat's score when its cast rides a live forming story —
    /// the trajectory bias layered atop the snapshot salience. `1.0` = no bias.
    pub max_bias: f32,
    /// A candidate must reach this interest to seed a thread or bias a beat (the noise floor that
    /// keeps single-episode seeds from steering the director).
    pub min_interest: f32,
    /// **The manufactured-thread floor** (the doc's S5 restraint): this many threads stay
    /// director-*authored* (never sift-seeded), so Gamma keeps inventing and never degenerates into
    /// a pure curator. The protagonist's own thread is the first such.
    pub manufactured_floor: usize,
}

impl Default for SiftConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ring_cap: 4096,
            graft: false,
            max_bias: 3.0,
            min_interest: 0.5,
            manufactured_floor: 1,
        }
    }
}

/// Sifter / Chronicle knobs, defaults with optional `assets/config/sift.ron`.
pub fn sift() -> SiftConfig {
    loaded("sift.ron")
}

/// Knobs for the **Perception Layer** (`docs/perception_layer.md`) — the salience weights and
/// thresholds that turn the Chronicle + Sifter into player-legible `Tell`s. Off by default, so a
/// perception-free world is unchanged (the `Perception` resource is absent and its pass
/// early-returns). It reads the Chronicle + Sifter, so enabling it implies the sift layer.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PerceptionConfig {
    /// Wake the Perception Layer — derive ranked `Tell`s from the Chronicle + Sifter each tick.
    pub enabled: bool,
    /// The least salience a `Tell` must reach to be kept — the budget floor that forces restraint.
    pub min_salience: f32,
    /// Hexes within which a forming story counts as "near the avatar" (the proximity term's reach).
    /// Inert in any player-less run, so headless / V&V stays byte-identical.
    pub reach: i32,
    /// Salience weight on the Sifter's interest (the surprise + dissonance it already scores).
    pub w_dissonance: f32,
    /// Salience weight on spatial proximity to the avatar (0 when no avatar exists).
    pub w_proximity: f32,
    /// Salience weight on authorship anomaly — how much of a story Γ *wrote* vs. the world *grew*.
    pub w_authorship: f32,
    /// Salience weight on **attachment** — a story whose cast holds a `Bond` to the avatar rises, so
    /// the souls who came to care for the player surface above the indifferent crowd. The hook the
    /// *emotional story* (world-as-protagonist) leans on (`docs/gameplay_targets.md`). 0 in any
    /// player-less / bond-less run, so headless / V&V stays byte-identical.
    pub w_bond: f32,
    /// Salience weight on recurrence — a subject/motif recurring across stories (Phase 5; 0 for now).
    pub w_recurrence: f32,
}

impl Default for PerceptionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_salience: 0.0,
            reach: 6,
            w_dissonance: 1.0,
            w_proximity: 1.0,
            w_authorship: 0.75,
            w_bond: 1.5,
            w_recurrence: 0.5,
        }
    }
}

/// Perception-layer knobs, defaults with optional `assets/config/perception.ron`.
pub fn perception() -> PerceptionConfig {
    loaded("perception.ron")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_config_files_match_the_in_code_defaults() {
        // The `assets/config/*.ron` files are generated from these same defaults
        // (see `generate_default_config_files`), so loading them must reproduce
        // the defaults exactly — the round-trip that keeps file and code in sync.
        assert_eq!(
            econ().price_floor_frac,
            EconConfig::default().price_floor_frac
        );
        assert_eq!(needs().hunger_rate, NeedsConfig::default().hunger_rate);
        assert_eq!(fauna().herd_cap, FaunaConfig::default().herd_cap);
        assert_eq!(
            director().beat_interval,
            DirectorConfig::default().beat_interval
        );
        assert_eq!(faction().period, FactionConfig::default().period);
        assert_eq!(feature().density, FeatureConfig::default().density);
        assert_eq!(params().ticks_per_year, Params::default().ticks_per_year);
    }

    #[test]
    fn a_partial_ron_file_layers_onto_defaults() {
        // A file may name any subset of fields (natural struct syntax); the rest
        // fall back to the default thanks to `#[serde(default)]`.
        let econ: EconConfig = crate::parse("(trade_lot: 99)").unwrap();
        assert_eq!(econ.trade_lot, 99);
        assert_eq!(
            econ.price_floor_frac,
            EconConfig::default().price_floor_frac
        );
    }

    /// (Re)generate `assets/config/*.ron` from the in-code [`Default`]s. Not a
    /// test of behaviour — a maintenance tool. Run with:
    /// `cargo test -p config -- --ignored generate_default_config_files`
    #[test]
    #[ignore = "maintenance tool: writes assets/config/*.ron from the in-code defaults"]
    fn generate_default_config_files() {
        use ron::ser::PrettyConfig;
        let dir = config_dir();
        std::fs::create_dir_all(&dir).expect("create assets/config");
        fn write<T: Serialize>(dir: &Path, file: &str, val: &T) {
            let body =
                ron::ser::to_string_pretty(val, PrettyConfig::new()).expect("serialize config");
            std::fs::write(dir.join(file), format!("{body}\n")).expect("write config file");
        }
        write(&dir, "params.ron", &Params::default());
        write(&dir, "econ.ron", &EconConfig::default());
        write(&dir, "needs.ron", &NeedsConfig::default());
        write(&dir, "fauna.ron", &FaunaConfig::default());
        write(&dir, "dialogue.ron", &DialogueConfig::default());
        write(&dir, "director.ron", &DirectorConfig::default());
        write(&dir, "faction.ron", &FactionConfig::default());
        write(&dir, "feature.ron", &FeatureConfig::default());
    }
}
