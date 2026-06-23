//! The gate (runs under `cargo test`, so the CI quality gate enforces it): zero coupling findings
//! may go unannotated. Plus sanity tests that the lint still *sees* the codebase, and that the
//! data-driven domains stay data-driven.

#[test]
fn no_unannotated_coupling() {
    let findings = coupling_lint::scan_workspace();
    let report: String = findings
        .iter()
        .map(|f| {
            format!(
                "  [{}] {} (score {}) — {}",
                f.detector, f.key, f.score, f.note
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        findings.is_empty(),
        "unannotated data/logic coupling — data-drive it (see assets/data/registers.ron), or add a\n\
         justified `// coupling-lint:allow <detector> <Symbol>: <reason>` at the site:\n{report}",
    );
}

#[test]
fn the_lint_still_sees_the_codebase() {
    // Use the *raw* findings (pre-allow): a parse regression that zeroed them must not pass
    // vacuously. The `Asset` file registry (config) is a stable, always-present self-match offender,
    // so its presence proves the detectors still resolve a real, load-bearing file.
    let raw = coupling_lint::scan_workspace_raw();
    assert!(
        raw.len() >= 5,
        "expected the detectors to flag the known offenders, got {} raw findings",
        raw.len()
    );
    assert!(
        raw.iter().any(|f| f.key.ends_with("::Asset")),
        "the config Asset registry should still be flagged (did config/src/assets.rs stop parsing?)",
    );
}

#[test]
fn the_data_driven_domains_stay_data_driven() {
    // Register, SpeechAct, and the Biome/Belt/HumidityProvince Holdridge tables became data; a
    // regression that re-hardcoded any of them would resurrect these findings (and they'd be
    // unannotated, failing the gate above too). Lock the wins in.
    let raw = coupling_lint::scan_workspace_raw();
    let resurrected: Vec<&str> = raw
        .iter()
        .filter(|f| {
            f.key.ends_with("::Register")
                || f.key.ends_with("::RegisterDef")
                || f.key.ends_with("::SPINES")
                || f.key.ends_with("::SpeechAct")
                || f.key.ends_with("::Biome")
                || f.key.ends_with("::Belt")
                || f.key.ends_with("::HumidityProvince")
        })
        .map(|f| f.key.as_str())
        .collect();
    assert!(
        resurrected.is_empty(),
        "these domains are supposed to be data-driven, but the lint flagged: {resurrected:?}",
    );
}
