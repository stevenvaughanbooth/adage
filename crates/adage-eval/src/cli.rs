//! `adage eval`: the command line.
//!
//! ```text
//! adage eval <dataset-dir> [--split <split>] [--thresholds demo|default] [--params <file>]
//!            [--window <turns>] [--include-steered] [--index <index.json>] [--json]
//! adage eval routes <dataset-dir> [--split <split>] [--thresholds demo|default] [--params <file>]
//!            [--index <index.json>] [--json]
//! adage eval bench run|sweep|report|bless …   (the L0 bench: `crate::bench::cli`)
//! adage eval fidelity <dataset-dir> --round <round1|round2> [--exact-keys] [--json]
//! adage eval types <gen-dir | dataset-dir> [--json]   (learned value types: `crate::value_types`)
//! adage eval generate <synthetic|adversarial> <out-dir>
//!
//! <split>: <name> | hash:<share>[:<seed>] | group:task:<k>:<fold> | temporal[:<share>] | all
//! ```
//!
//! `--params <file.toml|json>`: engine parameters (`adage_tracks::params::EngineParams`, any
//! subset) laid over the thresholds preset.
//!
//! Read-only apart from `generate` (which writes only `<out-dir>`), and it runs nothing: safe
//! to run anywhere, including inside an agent session.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

/// `--params <file>`: an override object for the engine parameters.
fn params(args: &[String]) -> Result<Option<serde_json::Value>> {
    flag(args, "--params")
        .map(|p| adage_tracks::params::EngineParams::read_overrides(std::path::Path::new(&p)))
        .transpose()
}

const USAGE: &str = "usage: adage eval <dataset-dir> [--split <split>] [--thresholds demo|default] [--params <file>] [--window <n>] [--include-steered] [--index <file>] [--json]\n       adage eval routes <dataset-dir> [--split <split>] [--thresholds demo|default] [--params <file>] [--index <file>] [--json]\n       adage eval bench run|sweep|report|bless … (adage eval bench --help)\n       adage eval fidelity <dataset-dir> --round <round1|round2> [--exact-keys] [--json]\n       adage eval types <gen-dir|dataset-dir> [--json]\n       adage eval generate <synthetic|adversarial> <out-dir>\n<split>: <name> | hash:<share>[:<seed>] | group:task:<k>:<fold> | temporal[:<share>] | all";

pub fn run(args: &[String]) -> Result<()> {
    let json = args.iter().any(|a| a == "--json");
    match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("bench") => crate::bench::cli::run(&args[1..]),
        Some("routes") => {
            let dir = args.get(1).filter(|d| !d.starts_with("--")).context(USAGE)?;
            let opts = crate::Options {
                split: flag(args, "--split"),
                thresholds: flag(args, "--thresholds"),
                index: flag(args, "--index").map(PathBuf::from),
                params: params(args)?,
                ..Default::default()
            };
            let r = crate::conventions::run(&PathBuf::from(dir), &opts)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", crate::conventions::render_text(&r));
            }
            Ok(())
        }
        Some("types") => {
            let dir = args.get(1).filter(|d| !d.starts_with("--")).context(USAGE)?;
            let r = crate::value_types::run(&PathBuf::from(dir))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", crate::value_types::render_text(&r));
            }
            Ok(())
        }
        Some("fidelity") => {
            let dir = args.get(1).context(USAGE)?;
            let round = flag(args, "--round").unwrap_or_else(|| "round2".into());
            let exact_keys = args.iter().any(|a| a == "--exact-keys");
            let r = crate::fidelity::run_with(&PathBuf::from(dir), &round, exact_keys)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", crate::fidelity::render_text(&r));
            }
            Ok(())
        }
        Some("generate") => {
            let (Some(which), Some(out)) = (args.get(1), args.get(2)) else { bail!(USAGE) };
            let out = PathBuf::from(out);
            match which.as_str() {
                "synthetic" => crate::generate::synthetic(&out)?,
                "adversarial" => crate::generate::adversarial(&out)?,
                other => bail!("unknown dataset {other:?} (synthetic or adversarial)"),
            }
            println!("wrote {}", out.display());
            Ok(())
        }
        Some(dir) => {
            let window = match flag(args, "--window") {
                Some(w) => w.parse().context("--window takes a number of turns")?,
                None => 3,
            };
            let opts = crate::Options {
                split: flag(args, "--split"),
                thresholds: flag(args, "--thresholds"),
                window,
                include_steered: args.iter().any(|a| a == "--include-steered"),
                index: flag(args, "--index").map(PathBuf::from),
                params: params(args)?,
            };
            let r = crate::run(&PathBuf::from(dir), &opts)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", crate::render_text(&r));
            }
            Ok(())
        }
    }
}
