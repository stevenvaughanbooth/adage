//! `adage eval bench sweep`: run the suite for many settings sampled from a parameter space.
//!
//! Configuration 0 is always the suite's preset with no override (label `default`); the rest are
//! the sampled ones (`<strategy>-<i>`). Configurations run in parallel (`--jobs`, plain threads);
//! the output is in configuration order, so it does not depend on scheduling. Each configuration
//! is scored exactly as `bench run` scores it; the route-precision guardrail's reference is the
//! baseline file's when given, else configuration 0's.

use super::space::{overrides, Space};
use super::{measure, params_for, records, Env, Measured, RunOutput};
use adage_tracks::params::EngineParams;
use anyhow::Result;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// One configuration of a sweep.
#[derive(Debug, Clone)]
pub struct Config {
    pub label: String,
    /// The override object laid over the suite's preset.
    pub overrides: Value,
    pub params: EngineParams,
}

/// The configurations a sweep runs: the preset, then the samples.
pub fn configs(env: &Env, space: &Space, strategy: &str, seed: u64) -> Result<Vec<Config>> {
    let mut out = vec![Config {
        label: "default".into(),
        overrides: Value::Object(Default::default()),
        params: params_for(&env.suite, None)?,
    }];
    let kind = strategy.split(':').next().unwrap_or(strategy);
    for (i, pairs) in space.sample(strategy, seed)?.into_iter().enumerate() {
        let refs: Vec<(&str, Value)> = pairs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        let o = overrides(&refs);
        let params = params_for(&env.suite, Some(&o))?;
        out.push(Config { label: format!("{kind}-{:04}", i + 1), overrides: o, params });
    }
    Ok(out)
}

/// Measure every configuration, `jobs` at a time. Results are in configuration order.
pub fn measure_all(env: &Env, configs: &[Config], jobs: usize) -> Result<Vec<Measured>> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Result<Measured>>>> = Mutex::new((0..configs.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..jobs.max(1).min(configs.len().max(1)) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= configs.len() {
                    break;
                }
                let r = measure(env, &configs[i].params);
                results.lock().unwrap()[i] = Some(r);
            });
        }
    });
    results.into_inner().unwrap().into_iter().map(|r| r.expect("every configuration ran")).collect()
}

/// Run a sweep: measure every configuration and score it. `baseline_rp`: the route-precision
/// reference from a baseline file, if any (else configuration 0's).
pub fn run(
    env: &Env,
    configs: &[Config],
    jobs: usize,
    baseline_rp: Option<f64>,
    detail: bool,
) -> Result<Vec<RunOutput>> {
    let measured = measure_all(env, configs, jobs)?;
    let reference = match baseline_rp {
        Some(x) => Some(x),
        None => measured.first().map(|m| {
            records(env, m, "default", None, false).scorecard.terms["route_precision"].as_f64().unwrap_or(0.0)
        }),
    };
    Ok(measured.iter().zip(configs).map(|(m, c)| records(env, m, &c.label, reference, detail)).collect())
}
