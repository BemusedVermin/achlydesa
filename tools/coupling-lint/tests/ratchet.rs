//! The ratchet gate (runs under `cargo test`, so the CI quality gate enforces it) plus a sanity
//! test that the lint still *sees* the codebase (a silent parse-failure must not pass vacuously).

#[test]
fn no_coupling_added_beyond_baseline() {
    let findings = coupling_lint::scan_workspace();
    let baseline = coupling_lint::load_baseline(&coupling_lint::baseline_path())
        .expect("baseline.txt is present and readable");
    let violations = coupling_lint::check_against_baseline(&findings, &baseline);
    assert!(
        violations.is_empty(),
        "coupling ratchet violated — new or grown data/logic coupling:\n{}\n\n\
         Move the per-instance content into data (see assets/data/registers.ron), or re-bless with\n\
         `cargo run -p coupling-lint -- --bless` if it is genuinely intentional.",
        violations.join("\n"),
    );
}

#[test]
fn the_lint_still_sees_the_codebase() {
    let findings = coupling_lint::scan_workspace();
    // It must find *something* — otherwise a parse regression would let everything pass vacuously.
    assert!(
        findings.len() >= 5,
        "expected the lint to flag the known backlog offenders, got {} findings",
        findings.len()
    );
    // The Biome life-zone table (game_sim) is the canonical surviving offender — its presence proves
    // the self_match/const_all detectors still resolve a real, load-bearing file.
    assert!(
        findings.iter().any(|f| f.key.contains("Biome")),
        "the Biome table should still be flagged (did game_sim/src/fields.rs stop parsing?)",
    );
}

#[test]
fn the_register_domain_is_no_longer_flagged() {
    // The whole point of the refactor: `Register`/`RegisterDef::def`/`SPINES` are data now, so they
    // must NOT appear as coupling. This locks the win in — a regression that re-hardcodes registers
    // would resurrect these findings and fail here.
    let findings = coupling_lint::scan_workspace();
    let resurrected: Vec<&str> = findings
        .iter()
        .filter(|f| {
            f.key.ends_with("::Register")
                || f.key.ends_with("::RegisterDef")
                || f.key.ends_with("::SPINES")
        })
        .map(|f| f.key.as_str())
        .collect();
    assert!(
        resurrected.is_empty(),
        "the register domain is supposed to be data-driven, but the lint flagged: {resurrected:?}",
    );
}
