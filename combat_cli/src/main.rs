//! `combat_cli` — the headless driver (spec §16). Load a scenario JSON, run it to completion with
//! the scripted controller (and `StubAi` for any AI factions), and write the event trace as pretty
//! JSON to stdout or a file. This is the human-inspectable proof the engine works with no renderer.
//!
//! ```text
//! combat_cli <scenario.json> [out.json]
//! ```

use combat_core::scenario::{self, Scenario};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: combat_cli <scenario.json> [out.json]");
        return ExitCode::FAILURE;
    };
    let out = args.next();

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("could not read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let sc: Scenario = match serde_json::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("could not parse {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let trace = scenario::run(&sc);
    let json = serde_json::to_string_pretty(&trace).expect("events serialize");

    match out {
        Some(o) => {
            if let Err(e) = std::fs::write(&o, json) {
                eprintln!("could not write {o}: {e}");
                return ExitCode::FAILURE;
            }
            eprintln!("wrote {} events to {o}", trace.len());
        }
        None => println!("{json}"),
    }
    ExitCode::SUCCESS
}
