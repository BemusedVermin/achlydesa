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
    // vacuously. The Biome life-zone table (game_sim) is the canonical surviving offender.
    let raw = coupling_lint::scan_workspace_raw();
    assert!(
        raw.len() >= 5,
        "expected the detectors to flag the known offenders, got {} raw findings",
        raw.len()
    );
    assert!(
        raw.iter().any(|f| f.key.contains("Biome")),
        "the Biome table should still be flagged (did game_sim/src/fields.rs stop parsing?)",
    );
}

#[test]
fn name_match_detector_fires() {
    // The `name_match` detector must catch branching on a name by string-literal matching. The
    // combat name handling (annotated as known debt) is the canonical surviving offender — if this
    // stops firing, the detector regressed (or combat was refactored, in which case update this).
    // (The former anchor, app/src/feature_art.rs, was removed in the text-front-end conversion.)
    let raw = coupling_lint::scan_workspace_raw();
    let hits: Vec<&str> = raw
        .iter()
        .filter(|f| f.detector == "name_match")
        .map(|f| f.key.as_str())
        .collect();
    assert!(
        hits.iter().any(|k| k.contains("combat")),
        "name_match should flag combat.rs's name-string matching; got {hits:?}",
    );
}

#[test]
fn the_data_driven_domains_stay_data_driven() {
    // Register and SpeechAct became data; a regression that re-hardcoded them would resurrect these
    // findings (and they'd be unannotated, failing the gate above too). Lock the win in.
    let raw = coupling_lint::scan_workspace_raw();
    let resurrected: Vec<&str> = raw
        .iter()
        .filter(|f| {
            f.key.ends_with("::Register")
                || f.key.ends_with("::RegisterDef")
                || f.key.ends_with("::SPINES")
                || f.key.ends_with("::SpeechAct")
        })
        .map(|f| f.key.as_str())
        .collect();
    assert!(
        resurrected.is_empty(),
        "these domains are supposed to be data-driven, but the lint flagged: {resurrected:?}",
    );
}
