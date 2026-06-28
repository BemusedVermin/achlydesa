//! **Project configuration hub.** The one crate that knows where the game's
//! authored content lives and how to read it. Every other crate gets its content
//! *through* here — they never reach into the top-level `assets/` folder
//! themselves — so swapping the content set (the shipping defaults vs. a test
//! fixture set vs. a live-edited directory) is a single decision made at one
//! seam: which [`Config`] you hand the loaders.
//!
//! The contract is deliberately narrow: **this crate sources bytes, it does not
//! parse them.** RON is deserialized into typed game data by the crate that owns
//! those types (`agents`). Keeping parsing out of here means the dependency
//! arrow only ever points one way (`agents -> config`), and lets this crate stay
//! tiny and dependency-free.
//!
//! Two ways in, one contract — see [`assets`]:
//! - [`Bundled`] — content baked into the binary at compile time (the shipping
//!   default). Infallible: [`Bundled::get`] hands back `&'static str`.
//! - [`Config::from_dir`] — content read from a directory at runtime (live
//!   editing in a dev tree, or a test fixture set). Fallible — it touches disk.
//!
//! Both satisfy [`AssetSource`], so a loader written against `&Config` neither
//! knows nor cares which backed it.
//!
//! This crate is named for the broader role it is meant to grow into — the home
//! for *all* project configuration. Authored assets are simply the first (and,
//! today, only) domain it owns; add new configuration as sibling modules.

pub mod assets;
pub mod params;
pub mod tunables;

pub use assets::{
    Asset, AssetSource, Bundled, Config, ConfigError, DirSource, InMemory, assets_data_dir, parse,
};
pub use params::Params;
pub use tunables::{
    DialogueConfig, DirectorConfig, EconConfig, FactionConfig, FaunaConfig, FeatureConfig,
    NeedsConfig, PerceptionConfig, SiftConfig,
};
