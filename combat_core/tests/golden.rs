//! Golden-vector + determinism tests (spec §15/§17). Each scenario is run to completion and its
//! event trace diffed against a committed `golden/<name>.golden.json`. Set `BLESS=1` to
//! regenerate the goldens intentionally:
//!
//! ```text
//! BLESS=1 cargo test -p combat_core --test golden
//! ```

use combat_core::Event;
use combat_core::scenario::{self, Scenario};
use std::path::{Path, PathBuf};

const SCENARIOS: &[&str] = &[
    "two_mook_trade",
    "interrupt",
    "line_knockback",
    "setup_payoff",
    "opposed_dilation",
];

fn scenarios_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios")
}
fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("golden")
}

fn load(name: &str) -> Scenario {
    let path = scenarios_dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {path:?}: {e}"))
}

fn check_golden(name: &str) {
    let sc = load(name);
    let trace = scenario::run(&sc);
    let golden_path = golden_dir().join(format!("{name}.golden.json"));

    if std::env::var("BLESS").is_ok() {
        std::fs::create_dir_all(golden_dir()).unwrap();
        let json = serde_json::to_string_pretty(&trace).unwrap();
        std::fs::write(&golden_path, json + "\n").unwrap();
        eprintln!("blessed {name} ({} events)", trace.len());
        return;
    }

    let text = std::fs::read_to_string(&golden_path).unwrap_or_else(|e| {
        panic!("read golden {golden_path:?}: {e} — run `BLESS=1 cargo test` to generate it")
    });
    let expected: Vec<Event> = serde_json::from_str(&text).unwrap();
    assert_eq!(
        trace, expected,
        "scenario `{name}` diverged from its golden trace"
    );
}

#[test]
fn golden_two_mook_trade() {
    check_golden("two_mook_trade");
}
#[test]
fn golden_interrupt() {
    check_golden("interrupt");
}
#[test]
fn golden_line_knockback() {
    check_golden("line_knockback");
}
#[test]
fn golden_setup_payoff() {
    check_golden("setup_payoff");
}
#[test]
fn golden_opposed_dilation() {
    check_golden("opposed_dilation");
}

/// Running a scenario twice yields a bit-identical trace (spec §17.3).
#[test]
fn reproducible_across_runs() {
    for name in SCENARIOS {
        let sc = load(name);
        assert_eq!(
            scenario::run(&sc),
            scenario::run(&sc),
            "scenario `{name}` is not reproducible"
        );
    }
}

/// The event-driven tick-jump and a forced tick-by-tick advance produce identical traces
/// (spec §17.3) — this is what proves the jump optimization is sound.
#[test]
fn tick_jump_equals_tick_by_tick() {
    for name in SCENARIOS {
        let sc = load(name);
        let jump = scenario::run_with_mode(&sc, false);
        let stepwise = scenario::run_with_mode(&sc, true);
        assert_eq!(
            jump, stepwise,
            "scenario `{name}`: tick-jump != tick-by-tick"
        );
    }
}
