//! The **coupling ratchet**: a `syn`-based source lint for the "domain instances baked into code"
//! anti-pattern — the shape the `Register` → `registers.ron` refactor removed.
//!
//! It hunts three structural signals, each a face of the same smell (per-variant content living in
//! code instead of data, so adding an instance needs a Rust change):
//!
//! - **`self_match`** — an `enum` with many variants that carries a per-variant metadata table as a
//!   `match self { Variant => … }` (the old `RegisterDef::def`, `Biome::name`, `SpeechAct::key`).
//!   The *arm count* is the tell: a grouped classifier (`A | B | C => …`) scores low; one arm per
//!   variant scores high.
//! - **`const_all`** — a parallel `const NAME: [Enum; N]` array shadowing an enum (the old `SPINES`,
//!   `Biome::ALL`). Adding an instance means editing the array too.
//! - **`string_ids`** — a file with many `*_id("literal")` lookups (`mood_id("joy")`,
//!   `trait_id("ambition")`): content names hardcoded as code constants, silently broken by a rename.
//!
//! It is a **ratchet**, not an absolute gate: the current offenders the audit found are recorded in
//! `baseline.txt`, and the lint fails only when a finding is *new* or *grows* past its baseline. Pay
//! one down and you may lower its baseline; you can never silently add more. Run `coupling-lint
//! --bless` to regenerate the baseline after an intentional change.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use syn::visit::Visit;

/// Minimum variants for an enum's `match self` to count as a content table (small rule-enums like a
/// 3-way `Government` are out of scope — the target is high-cardinality authored content).
pub const MIN_ENUM_VARIANTS: usize = 5;
/// Minimum arms in a `match self` before it reads as a per-variant table rather than a classifier.
pub const MIN_SELF_MATCH_ARMS: usize = 5;
/// Minimum length of a `const [Enum; N]` array before it reads as an instance roster.
pub const MIN_CONST_ARRAY: usize = 5;
/// Minimum `*_id("literal")` lookups in one file before the cluster is worth flagging.
pub const MIN_STRING_IDS: usize = 3;

/// The registry-lookup methods whose string-literal arguments are hardcoded content names.
pub const ID_METHODS: &[&str] = &[
    "mood_id",
    "trait_id",
    "good_id",
    "skill_id",
    "predicate_id",
    "register_id",
    "attr_id",
];

/// One detected coupling site.
#[derive(Debug, Clone)]
pub struct Finding {
    /// `"self_match"` | `"const_all"` | `"string_ids"`.
    pub detector: &'static str,
    /// Stable identity for the ratchet: `"rel/path.rs::Symbol"` (or just the file for `string_ids`).
    pub key: String,
    /// The magnitude (match arms / array length / literal count) — the ratcheted quantity.
    pub score: usize,
    /// A human-readable one-liner for the report.
    pub note: String,
}

/// The workspace root (two levels up from this crate's manifest dir).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("coupling-lint lives at <root>/tools/coupling-lint")
        .to_path_buf()
}

/// The checked-in baseline file path.
pub fn baseline_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("baseline.txt")
}

/// Whether a path is crate source we should scan: a `.rs` file under some crate's `src/`, never
/// under `target/` and never this linter's own source (it deliberately contains the patterns).
fn is_scannable(path: &Path, root: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("rs") {
        return false;
    }
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let comps: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if comps.iter().any(|c| c == "target") {
        return false;
    }
    if comps.first().map(String::as_str) == Some("tools")
        && comps.get(1).map(String::as_str) == Some("coupling-lint")
    {
        return false;
    }
    comps.iter().any(|c| c == "src")
}

/// A path relative to the workspace root, with forward slashes (so keys are stable across OSes).
fn rel_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Scan every crate-source file in the workspace and return the coupling findings (sorted).
pub fn scan_workspace() -> Vec<Finding> {
    let root = workspace_root();
    let mut findings = Vec::new();
    for entry in walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !is_scannable(path, &root) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        // A file syn can't parse is skipped rather than fatal — the sanity test guards that the
        // load-bearing files (e.g. game_sim's Biome table) still parse and surface.
        let Ok(file) = syn::parse_file(&text) else {
            continue;
        };
        scan_file(&file, &rel_path(path, &root), &mut findings);
    }
    findings.sort_by(|a, b| a.detector.cmp(b.detector).then_with(|| a.key.cmp(&b.key)));
    findings
}

fn scan_file(file: &syn::File, rel: &str, out: &mut Vec<Finding>) {
    // Pass 1: every enum's variant count (across modules too).
    let mut enums = EnumCollector::default();
    enums.visit_file(file);
    // Pass 2: the detectors.
    let mut det = Detector {
        rel,
        enums: &enums.map,
        findings: Vec::new(),
        id_count: 0,
    };
    det.visit_file(file);
    if det.id_count >= MIN_STRING_IDS {
        det.findings.push(Finding {
            detector: "string_ids",
            key: rel.to_string(),
            score: det.id_count,
            note: format!("{} hardcoded `*_id(\"…\")` content-name lookups", det.id_count),
        });
    }
    out.append(&mut det.findings);
}

#[derive(Default)]
struct EnumCollector {
    map: HashMap<String, usize>,
}

impl<'ast> Visit<'ast> for EnumCollector {
    fn visit_item_enum(&mut self, e: &'ast syn::ItemEnum) {
        self.map.insert(e.ident.to_string(), e.variants.len());
        syn::visit::visit_item_enum(self, e);
    }
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if !has_cfg_test(&m.attrs) {
            syn::visit::visit_item_mod(self, m);
        }
    }
}

/// Whether an item is gated `#[cfg(test)]` — test fixtures hardcode content names freely and aren't
/// the production coupling we're ratcheting, so the detectors skip them.
fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg")
            && matches!(&a.meta, syn::Meta::List(l) if l.tokens.to_string().contains("test"))
    })
}

struct Detector<'a> {
    rel: &'a str,
    enums: &'a HashMap<String, usize>,
    findings: Vec<Finding>,
    id_count: usize,
}

impl<'ast> Visit<'ast> for Detector<'_> {
    fn visit_item_impl(&mut self, im: &'ast syn::ItemImpl) {
        // Inherent impls of a local enum with enough variants: scan for a per-variant `match self`.
        if im.trait_.is_none()
            && let Some(name) = impl_self_name(im)
            && let Some(&variants) = self.enums.get(&name)
            && variants >= MIN_ENUM_VARIANTS
        {
            let mut finder = SelfMatchFinder { max_arms: 0 };
            for item in &im.items {
                if let syn::ImplItem::Fn(f) = item {
                    finder.visit_block(&f.block);
                }
            }
            if finder.max_arms >= MIN_SELF_MATCH_ARMS {
                self.findings.push(Finding {
                    detector: "self_match",
                    key: format!("{}::{}", self.rel, name),
                    score: finder.max_arms,
                    note: format!(
                        "enum `{name}` ({variants} variants) carries a {}-arm `match self` metadata table",
                        finder.max_arms
                    ),
                });
            }
        }
        syn::visit::visit_item_impl(self, im);
    }

    fn visit_item_const(&mut self, c: &'ast syn::ItemConst) {
        if let Some((elem, n)) = enum_array(&c.ty, &c.expr)
            && n >= MIN_CONST_ARRAY
        {
            self.findings.push(Finding {
                detector: "const_all",
                key: format!("{}::{}", self.rel, c.ident),
                score: n,
                note: format!("const `{}`: a parallel `[{elem}; {n}]` enum roster", c.ident),
            });
        }
        syn::visit::visit_item_const(self, c);
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        if ID_METHODS.contains(&m.method.to_string().as_str())
            && let Some(syn::Expr::Lit(lit)) = m.args.first()
            && matches!(lit.lit, syn::Lit::Str(_))
        {
            self.id_count += 1;
        }
        syn::visit::visit_expr_method_call(self, m);
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        // Don't descend into `#[cfg(test)]` modules — test fixtures aren't production coupling.
        if !has_cfg_test(&m.attrs) {
            syn::visit::visit_item_mod(self, m);
        }
    }
}

/// Finds the widest `match self` / `match *self` in a function body — the per-variant-table tell.
struct SelfMatchFinder {
    max_arms: usize,
}

impl<'ast> Visit<'ast> for SelfMatchFinder {
    fn visit_expr_match(&mut self, m: &'ast syn::ExprMatch) {
        if is_self_scrutinee(&m.expr) {
            self.max_arms = self.max_arms.max(m.arms.len());
        }
        syn::visit::visit_expr_match(self, m);
    }
}

/// The enum name an inherent `impl` is on (`impl Register { … }` → `"Register"`), if any.
fn impl_self_name(im: &syn::ItemImpl) -> Option<String> {
    if let syn::Type::Path(tp) = &*im.self_ty {
        tp.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}

/// Whether a match scrutinee is `self`, `*self`, or `&self` (the self-dispatch shape).
fn is_self_scrutinee(e: &syn::Expr) -> bool {
    match e {
        syn::Expr::Path(p) => p.path.is_ident("self"),
        syn::Expr::Unary(u) => is_self_scrutinee(&u.expr),
        syn::Expr::Reference(r) => is_self_scrutinee(&r.expr),
        syn::Expr::Paren(p) => is_self_scrutinee(&p.expr),
        _ => false,
    }
}

/// If `ty` is `[EnumLike; N]`, the element type name and length (from the const length, else the
/// initializer's element count). Only enum-like (capitalized) element types count — skips `[f32; N]`.
fn enum_array(ty: &syn::Type, init: &syn::Expr) -> Option<(String, usize)> {
    let syn::Type::Array(arr) = ty else {
        return None;
    };
    let syn::Type::Path(tp) = &*arr.elem else {
        return None;
    };
    let elem = tp.path.segments.last()?.ident.to_string();
    if !elem.chars().next()?.is_uppercase() {
        return None;
    }
    let n = array_len(&arr.len).or_else(|| match init {
        syn::Expr::Array(a) => Some(a.elems.len()),
        _ => None,
    })?;
    Some((elem, n))
}

fn array_len(len: &syn::Expr) -> Option<usize> {
    if let syn::Expr::Lit(l) = len
        && let syn::Lit::Int(i) = &l.lit
    {
        return i.base10_parse::<usize>().ok();
    }
    None
}

/// The checked-in allow-list of known coupling, `(detector, key) -> max allowed score`.
#[derive(Default)]
pub struct Baseline {
    entries: HashMap<(String, String), usize>,
}

impl Baseline {
    fn get(&self, detector: &str, key: &str) -> Option<usize> {
        self.entries
            .get(&(detector.to_string(), key.to_string()))
            .copied()
    }
}

/// Parse the baseline file: `detector  key  score` per line, `#` comments and blanks ignored.
pub fn load_baseline(path: &Path) -> std::io::Result<Baseline> {
    let text = std::fs::read_to_string(path)?;
    let mut entries = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        if let (Some(d), Some(k), Some(s)) = (it.next(), it.next(), it.next())
            && let Ok(score) = s.parse::<usize>()
        {
            entries.insert((d.to_string(), k.to_string()), score);
        }
    }
    Ok(Baseline { entries })
}

/// Write the baseline file from the current findings (the `--bless` path).
pub fn write_baseline(path: &Path, findings: &[Finding]) -> std::io::Result<()> {
    let mut s = String::new();
    s.push_str("# coupling-lint baseline — the known coupling this repo still carries.\n");
    s.push_str("# Format: <detector> <key> <max-score>. The ratchet fails on anything NEW or any\n");
    s.push_str("# finding above its score here. Regenerate with `cargo run -p coupling-lint -- --bless`.\n");
    s.push_str("# Lower a number when you pay coupling down; never raise one without review.\n");
    for f in findings {
        s.push_str(&format!("{}\t{}\t{}\n", f.detector, f.key, f.score));
    }
    std::fs::write(path, s)
}

/// Compare findings to the baseline; returns a human-readable line per violation (empty == clean).
pub fn check_against_baseline(findings: &[Finding], baseline: &Baseline) -> Vec<String> {
    let mut violations = Vec::new();
    for f in findings {
        match baseline.get(f.detector, &f.key) {
            Some(allowed) if f.score <= allowed => {}
            Some(allowed) => violations.push(format!(
                "GREW   [{}] {} — score {} > baseline {} ({})",
                f.detector, f.key, f.score, allowed, f.note
            )),
            None => violations.push(format!(
                "NEW    [{}] {} — score {} ({})",
                f.detector, f.key, f.score, f.note
            )),
        }
    }
    violations.sort();
    violations
}

/// Print the findings as a grouped report.
pub fn print_report(findings: &[Finding]) {
    println!("coupling-lint: {} findings\n", findings.len());
    let mut detector = "";
    for f in findings {
        if f.detector != detector {
            detector = f.detector;
            println!("[{detector}]");
        }
        println!("  {:>3}  {}  — {}", f.score, f.key, f.note);
    }
}
