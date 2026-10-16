//! The L0 bench (docs/evals.md, "Bench"): the committed baseline is reproduced exactly with the
//! default parameters, sweeps are deterministic, and the engine parameters really reach the
//! learner and the runtime.
//!
//! Replay needs POSIX paths (see gates.rs), so these run on Unix only.
#![cfg(unix)]

use adage_eval::bench::report::{self, compare_to_baseline};
use adage_eval::bench::space::Space;
use adage_eval::bench::{
    self, baseline_route_precision, measure, params_for, read_jsonl, records, sweep, to_jsonl, Env, Suite,
};
use adage_eval::{run, Options};
use serde_json::json;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn env() -> Env {
    Env::new(Suite::load("core").unwrap(), &root().join("evals/datasets"), Some("abc123".into()), true).unwrap()
}

/// With the default parameters, `bench run --suite core` gives exactly the committed baseline's
/// aggregates and scorecard. After a deliberate change of behaviour, re-measure and re-bless:
///
/// ```text
/// adage eval bench run --suite core --deterministic --out /tmp/l0.jsonl
/// adage eval bench bless /tmp/l0.jsonl --reason "<why the numbers moved>"
/// ```
#[test]
fn bench_defaults_reproduce_the_committed_baseline() {
    let env = env();
    let baseline = read_jsonl(&root().join("evals/baselines/l0-core.jsonl")).unwrap();
    let params = params_for(&env.suite, None).unwrap();
    let m = measure(&env, &params).unwrap();
    let out = records(&env, &m, "default", baseline_route_precision(&baseline), false);
    if let Err(e) = compare_to_baseline(&out.records, &baseline) {
        panic!("the default parameters no longer reproduce evals/baselines/l0-core.jsonl:\n{e}");
    }
    assert!(out.scorecard.feasible, "{:#?}", out.scorecard.guardrails);
    assert!(baseline.iter().any(|r| r["record"] == "scorecard" && r["reason"].is_string()), "the baseline says why");
    // Deterministic mode records no git sha, and nothing machine-specific.
    let text = to_jsonl(&out.records);
    assert!(!text.contains("abc123"));
    assert!(!text.contains(&*std::env::temp_dir().to_string_lossy().trim_end_matches('/').to_string()));
    assert!(!text.contains(&*root().to_string_lossy()));
}

#[test]
fn a_bench_run_is_byte_deterministic_and_carries_the_envelope() {
    let env = env();
    let params = params_for(&env.suite, None).unwrap();
    let a = records(&env, &measure(&env, &params).unwrap(), "default", None, true);
    let b = records(&env, &measure(&env, &params).unwrap(), "default", None, true);
    assert_eq!(to_jsonl(&a.records), to_jsonl(&b.records));
    let kinds: std::collections::BTreeSet<&str> = a.records.iter().filter_map(|r| r["record"].as_str()).collect();
    assert_eq!(kinds.into_iter().collect::<Vec<_>>(), ["aggregate", "directive", "run", "scorecard", "trial"]);
    for r in &a.records {
        assert_eq!(r["schema_version"], 1);
        assert_eq!(r["tier"], "l0");
        assert_eq!(r["suite"], "core");
        assert_eq!(r["run_id"], a.run_id.as_str());
        assert_eq!(r["params_hash"], params.params_hash().as_str());
        assert!(r["split"]["kind"].is_string() && r["params"].is_object() && r["adage_version"].is_string());
        if matches!(r["record"].as_str(), Some("trial" | "directive")) {
            assert_eq!(r["dataset"]["content_sha256"].as_str().map(str::len), Some(64));
        }
    }
    // One trial per scored test session.
    let trials = a.records.iter().filter(|r| r["record"] == "trial").count();
    let scored: usize = measure(&env, &params).unwrap().units.iter().map(|u| u.report.scored_sessions).sum();
    assert_eq!(trials, scored);
    // Without --deterministic, a given git sha is recorded.
    let loose = Env { deterministic: false, ..env.clone() };
    let c = records(&loose, &measure(&loose, &params).unwrap(), "default", None, false);
    assert!(c.records.iter().all(|r| r["git_sha"] == "abc123"));
}

#[test]
fn a_small_sweep_is_deterministic_and_every_configuration_is_safe() {
    let env = env();
    let space = Space::load(&root().join("evals/bench/spaces/learner.toml")).unwrap();
    let configs = sweep::configs(&env, &space, "random:4", 7).unwrap();
    assert_eq!(configs.len(), 5, "the preset, then four samples");
    assert_eq!(configs[0].label, "default");
    let a = sweep::run(&env, &configs, 1, None, false).unwrap();
    let b = sweep::run(&env, &configs, 4, None, false).unwrap();
    let text = |o: &[bench::RunOutput]| o.iter().map(|x| to_jsonl(&x.records)).collect::<String>();
    assert_eq!(text(&a), text(&b), "the same sweep gives the same bytes, however many jobs");
    for o in &a {
        let unsafe_ = o.scorecard.guardrails.iter().find(|g| g.name == "unsafe_directives").unwrap();
        assert!(unsafe_.pass, "{}: {} unsafe directives", o.scorecard.label, unsafe_.value);
    }
    let ids: std::collections::BTreeSet<&str> = a.iter().map(|o| o.run_id.as_str()).collect();
    assert_eq!(ids.len(), 5, "each setting has its own run id");
    // The report ranks them, and a blessing keeps one run's aggregates and scorecard only.
    let all: Vec<serde_json::Value> = a.iter().flat_map(|o| o.records.iter().cloned()).collect();
    let r = report::render(&all, 3, &bench::cli::default_axes(), None).unwrap();
    assert!(r.contains("bench report: 5 configurations"), "{r}");
    assert!(report::bless(&all, None, "x").is_err(), "several runs: choose one");
    let blessed = report::bless(&all, Some("default"), "initial").unwrap();
    assert!(blessed.lines().all(|l| l.contains("\"record\":\"aggregate\"") || l.contains("\"record\":\"scorecard\"")));
    assert!(blessed.contains("\"reason\":\"initial\""));
}

#[test]
fn engine_parameters_reach_the_learner_and_the_runtime() {
    let ds = root().join("evals/datasets/synthetic");
    let with = |p: serde_json::Value| run(&ds, &Options { params: Some(p), ..Default::default() }).unwrap();
    let base = with(json!({}));
    let plain = run(&ds, &Options::default()).unwrap();
    assert_eq!(serde_json::to_vec(&base).unwrap(), serde_json::to_vec(&plain).unwrap(), "{{}} is the preset");
    // Runtime: at most one directive per session.
    let one = with(json!({"runtime": {"max_directives_per_session": 1}}));
    assert!(one.overall.directives <= one.scored_sessions as u32 && one.overall.directives > 0);
    assert!(one.overall.directives < base.overall.directives);
    assert_ne!(one.params_hash, base.params_hash);
    // Learner: nothing clears a support bar higher than the train split.
    let none = with(json!({"learn": {"min_support": 1000}}));
    assert!(none.routes.is_empty() && none.overall.directives == 0);
    // Unknown parameters are refused.
    assert!(run(&ds, &Options { params: Some(json!({"learn": {"min_suport": 3}})), ..Default::default() }).is_err());
}
