//! `coupling-lint` CLI — see the [`coupling_lint`] crate docs.
//!
//! `cargo run -p coupling-lint` prints the report and exits non-zero if any coupling finding lacks a
//! justified `// coupling-lint:allow <detector> <Symbol>: <reason>` directive at its site. The same
//! check runs as a `#[test]` (so `cargo test` / the CI quality gate enforces it).

fn main() {
    let raw = coupling_lint::scan_workspace_raw();
    let unresolved = coupling_lint::scan_workspace();
    let suppressed = raw.len() - unresolved.len();

    coupling_lint::print_report(&unresolved);
    println!("\n{suppressed} finding(s) suppressed by inline allows.");

    if unresolved.is_empty() {
        println!("clean: no unannotated data/logic coupling.");
    } else {
        eprintln!(
            "\n{} UNANNOTATED coupling finding(s) above. Data-drive the content (see\n\
             assets/data/registers.ron for the pattern), or — if it is legitimately code, not\n\
             content — add a justified `// coupling-lint:allow <detector> <Symbol>: <reason>` at\n\
             the site.",
            unresolved.len()
        );
        std::process::exit(1);
    }
}
