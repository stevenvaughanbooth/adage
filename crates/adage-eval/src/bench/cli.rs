//! `adage eval bench`: the command line.
//!
//! ```text
//! adage eval bench run [--suite core|<file.toml>] [--params <file.toml|json>] [--out <f.jsonl>]
//!                      [--datasets <dir>] [--baseline <f.jsonl>] [--label <name>]
//!                      [--git-sha <sha>] [--deterministic] [--summary-only]
//! adage eval bench sweep --space <file.toml> [--strategy grid|random:N|sobol:N|halton:N]
//!                        [--seed <n>] [--jobs <n>] [--out <f.jsonl>] [--suite …] [--datasets …]
//!                        [--baseline <f.jsonl>] [--detail] [--git-sha <sha>] [--deterministic]
//! adage eval bench report <f.jsonl> [--top <k>] [--pareto a,b,c] [--vs-baseline <f.jsonl>]
//! adage eval bench bless <f.jsonl> --reason "<why>" [--label <name>] [--out <f.jsonl>]
//! ```
//!
//! `--baseline` defaults to `evals/baselines/l0-<suite>.jsonl` when that file exists: its route
//! precision is the reference for the route-precision guardrail.

use super::report;
use super::space::Space;
use super::{measure, params_for, read_jsonl, records, render_scorecard, sweep, to_jsonl, Env, Suite};
use adage_tracks::params::EngineParams;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub const USAGE: &str = "usage: adage eval bench run [--suite core|<file>] [--params <file>] [--out <f.jsonl>] [--datasets <dir>] [--baseline <f.jsonl>] [--label <name>] [--git-sha <sha>] [--deterministic]\n       adage eval bench sweep --space <file> [--strategy grid|random:N|sobol:N|halton:N] [--seed <n>] [--jobs <n>] [--out <f.jsonl>] [--detail] [--suite …] [--datasets …] [--baseline …]\n       adage eval bench report <f.jsonl> [--top <k>] [--pareto a,b,c] [--vs-baseline <f.jsonl>]\n       adage eval bench bless <f.jsonl> --reason \"<why>\" [--label <name>] [--out <f.jsonl>]";

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn env_from(args: &[String], suite_name: Option<String>) -> Result<Env> {
    let suite = Suite::load(suite_name.as_deref().unwrap_or("core"))?;
    let datasets = PathBuf::from(flag(args, "--datasets").unwrap_or_else(|| "evals/datasets".into()));
    let git_sha = flag(args, "--git-sha").or_else(|| std::env::var("ADAGE_GIT_SHA").ok().filter(|s| !s.is_empty()));
    Env::new(suite, &datasets, git_sha, has(args, "--deterministic"))
}

/// The route-precision reference: `--baseline`, else `evals/baselines/l0-<suite>.jsonl` if it
/// exists.
fn baseline_rp(args: &[String], suite: &str) -> Result<Option<f64>> {
    let path = match flag(args, "--baseline") {
        Some(p) => PathBuf::from(p),
        None => {
            let p = PathBuf::from(format!("evals/baselines/l0-{suite}.jsonl"));
            if !p.is_file() {
                return Ok(None);
            }
            p
        }
    };
    let recs = read_jsonl(&path)?;
    Ok(Some(super::baseline_route_precision(&recs).with_context(|| format!("{}: no scorecard", path.display()))?))
}

fn write_out(args: &[String], text: &str) -> Result<Option<PathBuf>> {
    match flag(args, "--out") {
        Some(p) => {
            let p = PathBuf::from(p);
            if let Some(d) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(&p, text).with_context(|| p.display().to_string())?;
            Ok(Some(p))
        }
        None => Ok(None),
    }
}

pub fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("run") => {
            let env = env_from(args, flag(args, "--suite"))?;
            let overrides = flag(args, "--params").map(|p| EngineParams::read_overrides(Path::new(&p))).transpose()?;
            let params = params_for(&env.suite, overrides.as_ref())?;
            let m = measure(&env, &params)?;
            let rp = baseline_rp(args, &env.suite.name)?;
            let label = flag(args, "--label").unwrap_or_else(|| {
                if overrides.is_some() {
                    "params".into()
                } else {
                    "default".into()
                }
            });
            let out = records(&env, &m, &label, rp, true);
            let text = to_jsonl(&out.records);
            match write_out(args, &text)? {
                Some(p) => {
                    print!("{}", render_scorecard(&out.scorecard, &out.run_id));
                    println!("wrote {} records to {}", out.records.len(), p.display());
                }
                None => print!("{text}"),
            }
            Ok(())
        }
        Some("sweep") => {
            let space_path = flag(args, "--space").context(USAGE)?;
            let space = Space::load(Path::new(&space_path))?;
            let env = env_from(args, flag(args, "--suite").or(space.suite.clone()))?;
            let strategy = flag(args, "--strategy").unwrap_or_else(|| "sobol:16".into());
            let seed: u64 =
                flag(args, "--seed").map(|s| s.parse()).transpose().context("--seed takes a number")?.unwrap_or(0);
            let jobs: usize = match flag(args, "--jobs") {
                Some(j) => j.parse().context("--jobs takes a number")?,
                None => std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            };
            let configs = sweep::configs(&env, &space, &strategy, seed)?;
            let rp = baseline_rp(args, &env.suite.name)?;
            let outs = sweep::run(&env, &configs, jobs, rp, has(args, "--detail"))?;
            let all: Vec<serde_json::Value> = outs.iter().flat_map(|o| o.records.iter().cloned()).collect();
            let text = to_jsonl(&all);
            match write_out(args, &text)? {
                Some(p) => {
                    let axes = default_axes();
                    print!("{}", report::render(&all, 10, &axes, None)?);
                    println!("\nwrote {} configurations ({} records) to {}", outs.len(), all.len(), p.display());
                }
                None => print!("{text}"),
            }
            Ok(())
        }
        Some("report") => {
            let file = args.get(1).filter(|a| !a.starts_with("--")).context(USAGE)?;
            let recs = read_jsonl(Path::new(file))?;
            let k: usize =
                flag(args, "--top").map(|s| s.parse()).transpose().context("--top takes a number")?.unwrap_or(10);
            let axes: Vec<String> = match flag(args, "--pareto") {
                Some(p) => p.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                None => default_axes(),
            };
            let base = flag(args, "--vs-baseline").map(|p| read_jsonl(Path::new(&p))).transpose()?;
            print!("{}", report::render(&recs, k, &axes, base.as_deref())?);
            Ok(())
        }
        Some("bless") => {
            let file = args.get(1).filter(|a| !a.starts_with("--")).context(USAGE)?;
            let reason = flag(args, "--reason").context("bless needs --reason \"<why>\"")?;
            let recs = read_jsonl(Path::new(file))?;
            let text = report::bless(&recs, flag(args, "--label").as_deref(), &reason)?;
            let suite = recs.iter().find_map(|r| r["suite"].as_str()).unwrap_or("core").to_string();
            let out = PathBuf::from(flag(args, "--out").unwrap_or_else(|| format!("evals/baselines/l0-{suite}.jsonl")));
            if let Some(d) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(&out, &text).with_context(|| out.display().to_string())?;
            println!("blessed {} records into {}", text.lines().count(), out.display());
            Ok(())
        }
        Some(other) => bail!("unknown bench command {other:?}\n{USAGE}"),
    }
}

/// The default Pareto axes: the objective's three terms.
pub fn default_axes() -> Vec<String> {
    vec!["turns_saved_at_window_per_session".into(), "-false_directives_per_session".into(), "route_f1".into()]
}
