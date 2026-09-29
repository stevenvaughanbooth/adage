//! Fit the directive-value model's cross-repository priors (`adage_learn::value::Priors`) from
//! other repositories' traces: every situation's calls to a test run that ran tests, pooled per
//! cell of the hierarchy, with each cell's κ fitted across the repositories by the method of
//! moments (`adage_graph::stats::fit_kappa`).
//!
//! ```text
//! cargo run --release -p adage-learn --example outcome_priors -- \
//!     --repo <key> [--root <dir>] <transcript>... [--repo <key> [--root <dir>] <transcript>...]... > priors.json
//! ```
//!
//! The output is the `learn.outcome_priors` object of an engine-parameters file
//! (`adage eval --params`). Only sessions no directive steered are read.

use adage_core::CommandTable;
use adage_learn::value::{self, Cell, Priors};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The largest κ a cell gets (full pooling).
const MAX_KAPPA: f64 = 20.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // (repo, root, files) groups.
    let mut groups: Vec<(String, Option<PathBuf>, Vec<PathBuf>)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--repo" => {
                groups.push((args[i + 1].clone(), None, Vec::new()));
                i += 2;
            }
            "--root" => {
                groups.last_mut().expect("--repo first").1 = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            f => {
                groups.last_mut().expect("--repo first").2.push(PathBuf::from(f));
                i += 1;
            }
        }
    }
    let table = CommandTable::load();
    let mut per_repo: Vec<Priors> = Vec::new();
    for (repo, root, files) in &groups {
        let statics = value::Statics::of(root.as_deref());
        let mut p = Priors::default();
        for f in files {
            let Some(s) = adage_learn::read_session(f, &table, Some(repo)) else { continue };
            if s.steered {
                continue;
            }
            p.add(&value::rows(&s, &statics.env_kind));
        }
        per_repo.push(p);
    }
    let mut cells: BTreeMap<String, Vec<(f64, f64, f64)>> = BTreeMap::new();
    for p in &per_repo {
        for (k, c) in &p.cells {
            cells.entry(k.clone()).or_default().push((c.sum, c.n, c.misfire));
        }
    }
    let mut out = Priors::default();
    for (k, units) in cells {
        let n: f64 = units.iter().map(|u| u.1).sum();
        let sum: f64 = units.iter().map(|u| u.0).sum();
        let misfire: f64 = units.iter().map(|u| u.2).sum();
        let pairs: Vec<(f64, f64)> = units.iter().map(|u| (u.0, u.1)).collect();
        let kappa = adage_graph::stats::fit_kappa(&pairs, MAX_KAPPA);
        out.cells.insert(k, Cell { n, sum, misfire, kappa });
    }
    println!("{}", serde_json::to_string_pretty(&out).expect("json"));
}
