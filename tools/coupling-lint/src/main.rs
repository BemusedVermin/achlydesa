//! `coupling-lint` CLI — see the [`coupling_lint`] crate docs.
//!
//! - `cargo run -p coupling-lint` — print the report and exit non-zero on a ratchet violation.
//! - `cargo run -p coupling-lint -- --bless` — regenerate `baseline.txt` from the current tree.
//!
//! The same check runs as a `#[test]` (so `cargo test` / the CI quality gate enforces it).

fn main() {
    let bless = std::env::args().any(|a| a == "--bless" || a == "--update-baseline");
    let findings = coupling_lint::scan_workspace();
    let path = coupling_lint::baseline_path();

    if bless {
        coupling_lint::write_baseline(&path, &findings).expect("write baseline");
        println!(
            "blessed: wrote {} findings to {}",
            findings.len(),
            path.display()
        );
        return;
    }

    coupling_lint::print_report(&findings);

    let baseline = coupling_lint::load_baseline(&path).unwrap_or_default();
    let violations = coupling_lint::check_against_baseline(&findings, &baseline);
    if violations.is_empty() {
        println!("\nratchet: clean (no coupling added beyond baseline).");
    } else {
        eprintln!("\nratchet: {} VIOLATION(S)", violations.len());
        for v in &violations {
            eprintln!("  {v}");
        }
        eprintln!(
            "\nData-drive the new content (see assets/data/registers.ron for the pattern), or — if\n\
             this coupling is intentional — re-bless: cargo run -p coupling-lint -- --bless"
        );
        std::process::exit(1);
    }
}
