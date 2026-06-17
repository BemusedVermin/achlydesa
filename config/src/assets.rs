//! Locating and reading the game's authored RON content.
//!
//! [`Asset`] is the single registry of *what authored content exists* — one
//! variant per file in `assets/data/`. An [`AssetSource`] turns an [`Asset`]
//! into its RON text; [`Config`] picks which source a run uses. Loaders take a
//! `&Config`, ask it for the text they need, and parse it themselves.

use serde::de::DeserializeOwned;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Why turning an asset into a value failed: it couldn't be read, or its RON
/// couldn't be parsed into the requested shape.
#[derive(Debug)]
pub enum ConfigError {
    /// The asset's bytes couldn't be obtained (missing file, missing in-memory
    /// entry, …).
    Io(std::io::Error),
    /// The RON didn't parse into the requested type.
    Parse(ron::error::SpannedError),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "reading asset: {e}"),
            ConfigError::Parse(e) => write!(f, "parsing asset: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io(e) => Some(e),
            ConfigError::Parse(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}

impl From<ron::error::SpannedError> for ConfigError {
    fn from(e: ron::error::SpannedError) -> Self {
        ConfigError::Parse(e)
    }
}

/// Parse a RON string into the requested shape. The one place the project's
/// serialization format is named — callers state the shape, config does the
/// deserialization. Use [`Config::load`] for RON that comes from an [`Asset`].
pub fn parse<T: DeserializeOwned>(ron: &str) -> Result<T, ConfigError> {
    ron::from_str(ron).map_err(ConfigError::Parse)
}

/// One authored RON document. Each variant maps 1:1 to a file in `assets/data/`.
///
/// This enum is the canonical list of authored content: adding a new data file
/// means adding a variant here (and the file), rather than scattering another
/// `include_str!`/`read_to_string` somewhere. [`Asset::ALL`] then picks it up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Asset {
    Appraisals,
    Beats,
    Features,
    Goals,
    Goods,
    Grammar,
    Intents,
    Moods,
    Norms,
    Predicates,
    Recipes,
    Skills,
    Traits,
    Verbs,
}

impl Asset {
    /// Every asset, in declaration order — handy for "load them all" sweeps and
    /// for tests that assert the bundled set is complete.
    pub const ALL: [Asset; 14] = [
        Asset::Appraisals,
        Asset::Beats,
        Asset::Features,
        Asset::Goals,
        Asset::Goods,
        Asset::Grammar,
        Asset::Intents,
        Asset::Moods,
        Asset::Norms,
        Asset::Predicates,
        Asset::Recipes,
        Asset::Skills,
        Asset::Traits,
        Asset::Verbs,
    ];

    /// The file this asset is stored under within a data directory, e.g.
    /// `"beats.ron"`.
    pub const fn file_name(self) -> &'static str {
        match self {
            Asset::Appraisals => "appraisals.ron",
            Asset::Beats => "beats.ron",
            Asset::Features => "features.ron",
            Asset::Goals => "goals.ron",
            Asset::Goods => "goods.ron",
            Asset::Grammar => "grammar.ron",
            Asset::Intents => "intents.ron",
            Asset::Moods => "moods.ron",
            Asset::Norms => "norms.ron",
            Asset::Predicates => "predicates.ron",
            Asset::Recipes => "recipes.ron",
            Asset::Skills => "skills.ron",
            Asset::Traits => "traits.ron",
            Asset::Verbs => "verbs.ron",
        }
    }
}

/// The contract for obtaining an asset's RON text. Implemented by [`Bundled`]
/// (compile-time), [`DirSource`] (runtime), and [`InMemory`] (tests). A loader
/// written against this trait — or against [`Config`], which wraps one — works
/// with any of them, which is what makes the content set swappable.
pub trait AssetSource {
    /// The RON text for `asset`.
    ///
    /// Returns [`Cow`] so a compile-time source can hand back a borrowed
    /// `&'static str` with no allocation, while a runtime source owns the
    /// `String` it read. Failure is reported as [`std::io::Error`] — a missing
    /// file (or, for [`InMemory`], a missing entry) is `NotFound`.
    fn text(&self, asset: Asset) -> std::io::Result<Cow<'static, str>>;
}

/// The content baked into the binary at compile time — the shipping default.
///
/// The bytes live in *this* crate's compilation (via `include_str!`), so a
/// shipped binary needs no `assets/` folder beside it. Reading is therefore
/// infallible; [`Bundled::get`] returns the text directly.
pub struct Bundled;

impl Bundled {
    /// The RON text for `asset`, baked in at compile time.
    pub fn get(asset: Asset) -> &'static str {
        // The path is anchored at this crate's manifest dir so it resolves the
        // same no matter which crate or working directory the build runs from.
        macro_rules! bundled {
            ($file:literal) => {
                include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/data/", $file))
            };
        }
        match asset {
            Asset::Appraisals => bundled!("appraisals.ron"),
            Asset::Beats => bundled!("beats.ron"),
            Asset::Features => bundled!("features.ron"),
            Asset::Goals => bundled!("goals.ron"),
            Asset::Goods => bundled!("goods.ron"),
            Asset::Grammar => bundled!("grammar.ron"),
            Asset::Intents => bundled!("intents.ron"),
            Asset::Moods => bundled!("moods.ron"),
            Asset::Norms => bundled!("norms.ron"),
            Asset::Predicates => bundled!("predicates.ron"),
            Asset::Recipes => bundled!("recipes.ron"),
            Asset::Skills => bundled!("skills.ron"),
            Asset::Traits => bundled!("traits.ron"),
            Asset::Verbs => bundled!("verbs.ron"),
        }
    }
}

impl AssetSource for Bundled {
    fn text(&self, asset: Asset) -> std::io::Result<Cow<'static, str>> {
        Ok(Cow::Borrowed(Bundled::get(asset)))
    }
}

/// Content read from a directory at runtime — a live-edited tree, or a test
/// fixture set. The directory holds the same `*.ron` files (by [`Asset::file_name`])
/// as the checked-in `assets/data/`.
#[derive(Clone, Debug)]
pub struct DirSource {
    dir: PathBuf,
}

impl DirSource {
    /// Read assets from `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
}

impl AssetSource for DirSource {
    fn text(&self, asset: Asset) -> std::io::Result<Cow<'static, str>> {
        std::fs::read_to_string(self.dir.join(asset.file_name())).map(Cow::Owned)
    }
}

/// An explicit, in-memory set of asset texts — the easy seam for tests that want
/// to run against a hand-written content set without going through the
/// filesystem. Any asset not inserted reports `NotFound`.
#[derive(Clone, Debug, Default)]
pub struct InMemory {
    texts: HashMap<Asset, String>,
}

impl InMemory {
    /// An empty set; add entries with [`InMemory::with`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Provide the text for one asset (builder style).
    pub fn with(mut self, asset: Asset, text: impl Into<String>) -> Self {
        self.texts.insert(asset, text.into());
        self
    }
}

impl AssetSource for InMemory {
    fn text(&self, asset: Asset) -> std::io::Result<Cow<'static, str>> {
        self.texts.get(&asset).cloned().map(Cow::Owned).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no in-memory text for asset {asset:?} ({})", asset.file_name()),
            )
        })
    }
}

/// The configured content source for a run: the shipping defaults, a live
/// directory, or a bespoke test set. Loaders take `&Config` and call
/// [`Config::text`]; swapping the entire content set is swapping the `Config`
/// you build — the seam the rest of the project pivots on.
pub struct Config {
    source: Box<dyn AssetSource + Send + Sync>,
}

impl Config {
    /// Production default: content baked into the binary ([`Bundled`]).
    pub fn bundled() -> Self {
        Self { source: Box::new(Bundled) }
    }

    /// Read content from `dir` at runtime (live editing, or test fixtures).
    pub fn from_dir(dir: impl Into<PathBuf>) -> Self {
        Self { source: Box::new(DirSource::new(dir)) }
    }

    /// Wrap any custom [`AssetSource`] — e.g. an [`InMemory`] set in a test.
    pub fn from_source(source: impl AssetSource + Send + Sync + 'static) -> Self {
        Self { source: Box::new(source) }
    }

    /// The RON text for `asset`, from whichever source backs this config.
    pub fn text(&self, asset: Asset) -> std::io::Result<Cow<'static, str>> {
        self.source.text(asset)
    }

    /// Source `asset` and parse it into the requested shape `T` in one step —
    /// the typed counterpart to [`Config::text`]. This is where a content asset
    /// becomes the plain DTO a caller asked for; resolving that DTO into a
    /// domain object (id interning, cross-validation) is the caller's job.
    pub fn load<T: DeserializeOwned>(&self, asset: Asset) -> Result<T, ConfigError> {
        parse(&self.text(asset)?)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::bundled()
    }
}

/// The on-disk location of the checked-in `assets/data/` folder, as known at
/// compile time. Useful for live-editing in a dev tree
/// (`Config::from_dir(assets_data_dir())`); a shipped binary should be handed an
/// explicit path or use [`Config::bundled`] instead.
pub fn assets_data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("assets").join("data")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_asset_has_a_distinct_ron_file_name() {
        let mut names: Vec<&str> = Asset::ALL.iter().map(|a| a.file_name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "asset file names must be unique");
        assert!(names.iter().all(|n| n.ends_with(".ron")));
    }

    #[test]
    fn bundled_text_is_present_for_every_asset() {
        for asset in Asset::ALL {
            let bundled = Bundled::get(asset);
            assert!(!bundled.trim().is_empty(), "bundled {asset:?} is empty");
            // The Config/AssetSource path must agree with the direct accessor.
            assert_eq!(Config::bundled().text(asset).unwrap(), bundled);
        }
    }

    #[test]
    fn dir_source_reads_the_checked_in_assets() {
        let cfg = Config::from_dir(assets_data_dir());
        for asset in Asset::ALL {
            assert_eq!(
                cfg.text(asset).expect("checked-in asset reads"),
                Bundled::get(asset),
                "on-disk {asset:?} must match the bundled copy",
            );
        }
    }

    #[test]
    fn in_memory_source_swaps_one_asset_and_reports_the_rest_missing() {
        let cfg = Config::from_source(InMemory::new().with(Asset::Goods, "[]"));
        assert_eq!(cfg.text(Asset::Goods).unwrap(), "[]");
        assert_eq!(cfg.text(Asset::Beats).unwrap_err().kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn load_parses_an_asset_into_the_requested_shape() {
        // predicates.ron is a bare list of names — extract it as the shape we ask for.
        let predicates: Vec<String> = Config::bundled().load(Asset::Predicates).unwrap();
        assert!(predicates.iter().all(|p| !p.is_empty()));
    }

    #[test]
    fn load_surfaces_a_parse_error_for_the_wrong_shape() {
        // predicates is a list of strings, not a map of ints — a shape mismatch is a Parse error.
        let wrong: Result<std::collections::HashMap<String, i32>, _> = Config::bundled().load(Asset::Predicates);
        assert!(matches!(wrong, Err(ConfigError::Parse(_))));
    }

    #[test]
    fn parse_reads_an_inline_ron_string() {
        let names: Vec<String> = parse(r#"["a", "b"]"#).unwrap();
        assert_eq!(names, ["a", "b"]);
    }
}
