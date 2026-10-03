//! `adage eval`: the command line.
//!
//! ```text
//! adage eval <dataset-dir> [--split <split>] [--thresholds demo|default] [--params <file>]
//!            [--window <turns>] [--include-steered] [--index <index.json>] [--exec-e1] [--json]
//!            [--instincts | --instincts-only] [--sandbox-dir <dir>]
//! adage eval routes <dataset-dir | gen-bundle | gen-suite> [--split <split>] [--thresholds demo|default]
//!            [--params <file>] [--index <index.json>] [--json]
//! adage eval bench run|sweep|report|bless …   (the L0 bench: `crate::bench::cli`)
//! adage eval fidelity <dataset-dir> --round <round1|round2> [--exact-keys] [--learned] [--json]
//! adage eval types <gen-dir | dataset-dir> [--end-to-end] [--json]   (learned value types: `crate::value_types`)
//! adage eval mine <dataset-dir | gen-dir> [--split <split>] [--top <n>] [--json]   (`crate::graph_mine`)
//! adage eval generate <synthetic|adversarial> <out-dir>
//! adage eval replay-savings <manifest.json> …   (the §5.2 savings metric: `crate::savings_replay`)
//! adage eval embeddings <manifest.json>… --models <dir>   (embedding ablation: `crate::embed_eval`)
//! adage eval paired <study.json>   (held-out task quality, token and billed-cost qualification)
//!
//! <split>: <name> | hash:<share>[:<seed>] | group:task:<k>:<fold> | temporal[:<share>] | all
//! ```
//!
//! `--params <file.toml|json>`: engine parameters (`adage_tracks::params::EngineParams`, any
//! subset) laid over the thresholds preset.
//!
//! Instincts are off by default, so the numbers are those of conditioned reflexes. `--instincts`
//! binds them after conditioning, as `adage condition` does; `--instincts-only` conditions nothing
//! on the train traces and binds only instincts, from the repository's files (`--sandbox-dir`,
//! default `evals/sandboxes/<repo>`), then replays the test traces: what Adage says on day 0.
//!
//! Human-readable text uses the lexicon's words (reflexes, fire/prime/inhibit, control); the JSON,
//! the datasets and the subcommand names (`routes`) keep their field names.
//!
//! Read-only apart from `generate` (which writes only `<out-dir>`), and it runs nothing: safe
//! to run anywhere, including inside an agent session.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

/// `--instincts` (learned routes, then instincts) or `--instincts-only` (instincts alone, nothing
/// learned: day 0); neither is the default, learned routes only.
fn instinct_mode(args: &[String]) -> crate::InstinctMode {
    if args.iter().any(|a| a == "--instincts-only") {
        crate::InstinctMode::Only
    } else if args.iter().any(|a| a == "--instincts") {
        crate::InstinctMode::With
    } else {
        crate::InstinctMode::Off
    }
}

/// `--params <file>`: an override object for the engine parameters.
fn params(args: &[String]) -> Result<Option<serde_json::Value>> {
    flag(args, "--params")
        .map(|p| adage_tracks::params::EngineParams::read_overrides(std::path::Path::new(&p)))
        .transpose()
}

const USAGE: &str = "\
usage: adage eval <dataset-dir> [--split <split>] [--thresholds demo|default] [--params <file>] [--window <n>] [--include-steered] [--index <file>] [--instincts | --instincts-only] [--sandbox-dir <dir>] [--exec-e1] [--json]
         condition reflexes on the train traces, replay the test traces, and score what Adage would have said
       adage eval routes <dataset-dir|gen-bundle|gen-suite> [--split <split>] [--thresholds demo|default] [--params <file>] [--index <file>] [--json]
         list the reflexes conditioned on the train traces, checked against the dataset's conventions
       adage eval bench run|sweep|report|bless … (adage eval bench --help)
         the parameter bench
       adage eval paired <study.json>
         paired held-out task quality, token and billed-cost qualification (JSON)
       adage eval fidelity <dataset-dir> --round <round1|round2> [--exact-keys] [--learned] [--json]
         replay recorded traces and compare with the hook's live decisions (treated and control arms)
       adage eval types <gen-dir|dataset-dir> [--end-to-end] [--json]
         the value types conditioned for lookup slots
       adage eval mine <dataset-dir|gen-dir> [--split <split>] [--top <n>] [--json]
         the patterns the graph engine mines from the traces
       adage eval generate <synthetic|adversarial> <out-dir>
         write a generated dataset
<split>: <name> | hash:<share>[:<seed>] | group:task:<k>:<fold> | temporal[:<share>] | all
--instincts binds instincts after conditioning; --instincts-only conditions nothing and binds only instincts (day 0).";

/// What a usage error points to: the full usage is `adage eval --help`.
const SEE_HELP: &str = "see `adage eval --help`";

fn missing_dir() -> String {
    format!("missing <dataset-dir>: {SEE_HELP}")
}

pub fn run(args: &[String]) -> Result<()> {
    let json = args.iter().any(|a| a == "--json");
    match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("bench") => crate::bench::cli::run(&args[1..]),
        Some("paired") => crate::paired::cli(&args[1..]),
        Some("replay-savings") => crate::savings_replay::cli(&args[1..]),
        Some("embeddings") => crate::embed_eval::cli(&args[1..]),
        Some("routes") => {
            let dir = args.get(1).filter(|d| !d.starts_with("--")).with_context(missing_dir)?;
            let opts = crate::Options {
                split: flag(args, "--split"),
                thresholds: flag(args, "--thresholds"),
                index: flag(args, "--index").map(PathBuf::from),
                params: params(args)?,
                instincts: instinct_mode(args),
                sandbox_dir: flag(args, "--sandbox-dir").map(PathBuf::from),
                ..Default::default()
            };
            let dir = PathBuf::from(dir);
            // A generated bundle or suite (`evals/gen`): its rehearsal transcripts, every route.
            let bundles: Vec<PathBuf> = if dir.join("spec.json").is_file() {
                vec![dir.clone()]
            } else if dir.join("suite.json").is_file() {
                let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)?
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.join("spec.json").is_file() && p.join("rehearsal/transcripts").is_dir())
                    .collect();
                v.sort();
                v
            } else {
                vec![]
            };
            if !bundles.is_empty() {
                let reports: Vec<crate::conventions::RoutesReport> =
                    bundles.iter().map(|b| crate::conventions::run_bundle(b)).collect::<Result<_>>()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&reports)?);
                } else {
                    for r in &reports {
                        print!("{}", crate::conventions::render_text(r));
                    }
                }
                return Ok(());
            }
            let r = crate::conventions::run(&dir, &opts)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", crate::conventions::render_text(&r));
            }
            Ok(())
        }
        Some("types") => {
            let dir = args.get(1).filter(|d| !d.starts_with("--")).with_context(missing_dir)?;
            let e2e = args.iter().any(|a| a == "--end-to-end");
            let r = crate::value_types::run_with(&PathBuf::from(dir), e2e)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", crate::value_types::render_text(&r));
            }
            Ok(())
        }
        Some("mine") => {
            let dir = args.get(1).filter(|d| !d.starts_with("--")).with_context(missing_dir)?;
            let r = crate::graph_mine::run(&PathBuf::from(dir), flag(args, "--split").as_deref())?;
            let top = flag(args, "--top").and_then(|t| t.parse().ok()).unwrap_or(12);
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", crate::graph_mine::render_text(&r, top));
            }
            Ok(())
        }
        Some("fidelity") => {
            let dir = args.get(1).filter(|d| !d.starts_with("--")).with_context(missing_dir)?;
            let round = flag(args, "--round").unwrap_or_else(|| "round2".into());
            let exact_keys = args.iter().any(|a| a == "--exact-keys");
            if args.iter().any(|a| a == "--learned") {
                let r = crate::fidelity::run_learned(&PathBuf::from(dir), &round, exact_keys)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&r)?);
                } else {
                    print!("{}", crate::fidelity::render_learned(&r));
                }
                return Ok(());
            }
            let r = crate::fidelity::run_with(&PathBuf::from(dir), &round, exact_keys)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", crate::fidelity::render_text(&r));
            }
            Ok(())
        }
        Some("generate") => {
            let (Some(which), Some(out)) = (args.get(1), args.get(2)) else {
                bail!("adage eval generate needs <synthetic|adversarial> and an <out-dir>: {SEE_HELP}")
            };
            let out = PathBuf::from(out);
            match which.as_str() {
                "synthetic" => crate::generate::synthetic(&out)?,
                "adversarial" => crate::generate::adversarial(&out)?,
                other => bail!("unknown dataset {other:?} (synthetic or adversarial)"),
            }
            println!("wrote {}", out.display());
            Ok(())
        }
        Some(opt) if opt.starts_with('-') => bail!("{opt} goes after the <dataset-dir>: {SEE_HELP}"),
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
                instincts: instinct_mode(args),
                sandbox_dir: flag(args, "--sandbox-dir").map(PathBuf::from),
                exec_e1: args.iter().any(|a| a == "--exec-e1"),
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
