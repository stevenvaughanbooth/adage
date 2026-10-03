//! Paired, held-out outcome and spend evidence. Replay opportunities are not accepted as runs.
//! Every independent task contributes one pair; retries and train/test overlap fail qualification.

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudyKind {
    SameModel,
    ModelTransfer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub min_groups: usize,
    pub max_quality_loss: f64,
    pub min_savings: f64,
    pub min_baseline_success: f64,
}

impl Default for Policy {
    fn default() -> Self {
        Self { min_groups: 30, max_quality_loss: 0.02, min_savings: 0.20, min_baseline_success: 0.80 }
    }
}

/// Input includes cached input; cached_input is a subset, never added a second time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    pub model_calls: u64,
    /// Recorded billed cost, millionths of a US dollar. Missing cost is never inferred from tokens.
    pub cost_microusd: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub model: String,
    pub task_revision: String,
    /// Checkout, harness, tools, input context and budget pinned by the experiment runner.
    pub environment_revision: String,
    pub evaluator_revision: String,
    /// Independently graded task completion, including failures/timeouts; never a trace proxy.
    pub passed: Option<bool>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pair {
    pub repo: String,
    pub identity: adage_learn::EvidenceIdentity,
    pub baseline: Run,
    pub intervention: Run,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Study {
    pub schema: u32,
    pub experiment: String,
    pub kind: StudyKind,
    pub training_generation: String,
    /// Canonical `Session::evidence_group().key` values from the training snapshot.
    pub training_groups: BTreeSet<String>,
    /// From `corpus inspect --evidence`. Inferred/unknown training identities cannot establish
    /// disjoint groups against an independently declared evaluation task set.
    pub training_inferred_identity: usize,
    pub training_unknown_identity: usize,
    #[serde(default)]
    pub policy: Policy,
    pub pairs: Vec<Pair>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Reduction {
    pub baseline: u128,
    pub intervention: u128,
    pub observed_fraction: Option<f64>,
    /// Deterministic task-pair bootstrap, 2,000 resamples, one-sided 95% percentile bound.
    pub lower_95: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub schema: u32,
    pub experiment: String,
    pub kind: StudyKind,
    pub training_generation: String,
    pub policy: Policy,
    pub pairs: usize,
    pub independent_groups: usize,
    pub baseline_passed: usize,
    pub intervention_passed: usize,
    pub improvements: usize,
    pub regressions: usize,
    pub unknown_outcomes: usize,
    pub missing_usage: usize,
    pub missing_cost: usize,
    pub quality_delta: Option<f64>,
    /// Simultaneous 95% bound: Clopper–Pearson lower(improvements) - upper(regressions).
    pub quality_delta_lower_95: Option<f64>,
    pub tokens: Option<Reduction>,
    pub cost_microusd: Option<Reduction>,
    pub model_calls: Option<Reduction>,
    pub quality_accepted: bool,
    pub token_target_accepted: bool,
    pub cost_target_accepted: bool,
    pub reasons: BTreeSet<String>,
}

fn group(pair: &Pair) -> Option<String> {
    pair.identity.group(&pair.repo, "").map(|g| g.key)
}

/// Exactly one independently evaluated task pair per evidence group. Multiple model cohorts
/// must be reported separately; a model-transfer study deliberately compares different models.
pub fn evaluate(study: &Study) -> Result<Report> {
    ensure!(study.schema == 1, "unsupported paired-study schema");
    let policy = &study.policy;
    ensure!(
        policy.min_groups >= 2
            && policy.max_quality_loss.is_finite()
            && (0.0..=1.0).contains(&policy.max_quality_loss)
            && policy.min_savings.is_finite()
            && (0.0..=1.0).contains(&policy.min_savings)
            && policy.min_baseline_success.is_finite()
            && (0.0..=1.0).contains(&policy.min_baseline_success),
        "invalid evaluation policy"
    );
    let mut pairs: Vec<_> = study.pairs.iter().collect();
    pairs.sort_by_key(|p| (group(p), &p.baseline.id, &p.intervention.id));
    let mut reasons = BTreeSet::new();
    if study.experiment.trim().is_empty() || study.training_generation.trim().is_empty() {
        reasons.insert("missing_experiment_provenance".into());
    }
    if study.training_inferred_identity > 0 || study.training_unknown_identity > 0 {
        reasons.insert("training_identity_incomplete".into());
    }
    let mut groups = BTreeSet::new();
    let mut runs = BTreeSet::new();
    let mut cohorts = BTreeSet::new();
    let (mut baseline_passed, mut intervention_passed, mut improvements, mut regressions) = (0, 0, 0, 0);
    let (mut unknown_outcomes, mut missing_usage, mut missing_cost) = (0, 0, 0);
    let (mut tokens, mut costs, mut calls) = (Vec::new(), Vec::new(), Vec::new());
    for p in &pairs {
        let (b, t) = (&p.baseline, &p.intervention);
        match group(p) {
            Some(g) => {
                if study.training_groups.contains(&g) {
                    reasons.insert("training_overlap".into());
                }
                if !groups.insert(g) {
                    reasons.insert("repeated_task_group".into());
                }
            }
            None => {
                reasons.insert("unknown_task_identity".into());
            }
        }
        if p.repo.trim().is_empty() {
            reasons.insert("missing_repository".into());
        }
        for run in [b, t] {
            if run.id.trim().is_empty() || !runs.insert(&run.id) {
                reasons.insert("missing_or_reused_run_id".into());
            }
            if run.model.trim().is_empty() {
                reasons.insert("missing_model".into());
            }
        }
        cohorts.insert((&b.model, &t.model));
        if study.kind == StudyKind::SameModel && b.model != t.model {
            reasons.insert("model_mismatch".into());
        }
        if study.kind == StudyKind::ModelTransfer && b.model == t.model {
            reasons.insert("no_model_transfer".into());
        }
        for (left, right) in [
            (&b.task_revision, &t.task_revision),
            (&b.environment_revision, &t.environment_revision),
            (&b.evaluator_revision, &t.evaluator_revision),
        ] {
            if left.trim().is_empty() || left != right {
                reasons.insert("unmatched_conditions".into());
            }
        }
        baseline_passed += usize::from(b.passed == Some(true));
        intervention_passed += usize::from(t.passed == Some(true));
        match (b.passed, t.passed) {
            (Some(false), Some(true)) => improvements += 1,
            (Some(true), Some(false)) => regressions += 1,
            (Some(_), Some(_)) => {}
            _ => unknown_outcomes += 1,
        }
        match (&b.usage, &t.usage) {
            (Some(b), Some(t)) => {
                if b.cached_input_tokens > b.input_tokens || t.cached_input_tokens > t.input_tokens {
                    reasons.insert("invalid_cached_token_count".into());
                }
                tokens.push((
                    u128::from(b.input_tokens) + u128::from(b.output_tokens),
                    u128::from(t.input_tokens) + u128::from(t.output_tokens),
                ));
                calls.push((u128::from(b.model_calls), u128::from(t.model_calls)));
                if let (Some(b), Some(t)) = (b.cost_microusd, t.cost_microusd) {
                    costs.push((u128::from(b), u128::from(t)));
                } else {
                    missing_cost += 1;
                }
            }
            _ => {
                missing_usage += 1;
                missing_cost += 1;
            }
        }
    }
    if cohorts.len() > 1 {
        reasons.insert("mixed_model_cohorts".into());
    }
    let structurally_valid = reasons.is_empty();
    let n = pairs.len();
    let quality_delta =
        (n > 0 && unknown_outcomes == 0).then(|| (intervention_passed as f64 - baseline_passed as f64) / n as f64);
    let quality_lower = (structurally_valid && n > 0 && unknown_outcomes == 0).then(|| {
        use adage_graph::stats::beta_quantile;
        let wins = if improvements == 0 {
            0.0
        } else {
            beta_quantile(0.025, improvements as f64, (n - improvements + 1) as f64)
        };
        let losses = if regressions == n {
            1.0
        } else {
            beta_quantile(0.975, (regressions + 1) as f64, (n - regressions) as f64)
        };
        wins - losses
    });
    if n < policy.min_groups {
        reasons.insert("insufficient_independent_tasks".into());
    }
    if unknown_outcomes > 0 {
        reasons.insert("unknown_task_outcomes".into());
    }
    let baseline_qualified = n > 0 && baseline_passed as f64 / n as f64 >= policy.min_baseline_success;
    if !baseline_qualified {
        reasons.insert("baseline_quality_too_low".into());
    }
    let quality_accepted = structurally_valid
        && baseline_qualified
        && n >= policy.min_groups
        && quality_lower.is_some_and(|q| q >= -policy.max_quality_loss);
    if quality_lower.is_some_and(|q| q < -policy.max_quality_loss) {
        reasons.insert("quality_noninferiority_not_established".into());
    }
    let tokens = (missing_usage == 0 && n > 0).then(|| reduction(&tokens, structurally_valid));
    let model_calls = (missing_usage == 0 && n > 0).then(|| reduction(&calls, structurally_valid));
    let costs = (missing_cost == 0 && n > 0).then(|| reduction(&costs, structurally_valid));
    if missing_usage > 0 {
        reasons.insert("missing_token_usage".into());
    }
    if missing_cost > 0 {
        reasons.insert("missing_billed_cost".into());
    }
    let token_target_accepted =
        quality_accepted && tokens.as_ref().is_some_and(|r| r.lower_95.is_some_and(|x| x >= policy.min_savings));
    let cost_target_accepted =
        quality_accepted && costs.as_ref().is_some_and(|r| r.lower_95.is_some_and(|x| x >= policy.min_savings));
    if tokens.as_ref().is_some_and(|r| r.lower_95.is_none_or(|x| x < policy.min_savings)) {
        reasons.insert("token_target_not_established".into());
    }
    if costs.as_ref().is_some_and(|r| r.lower_95.is_none_or(|x| x < policy.min_savings)) {
        reasons.insert("cost_target_not_established".into());
    }
    Ok(Report {
        schema: 1,
        experiment: study.experiment.clone(),
        kind: study.kind,
        training_generation: study.training_generation.clone(),
        policy: policy.clone(),
        pairs: n,
        independent_groups: groups.len(),
        baseline_passed,
        intervention_passed,
        improvements,
        regressions,
        unknown_outcomes,
        missing_usage,
        missing_cost,
        quality_delta,
        quality_delta_lower_95: quality_lower,
        tokens,
        cost_microusd: costs,
        model_calls,
        quality_accepted,
        token_target_accepted,
        cost_target_accepted,
        reasons,
    })
}

fn reduction(pairs: &[(u128, u128)], independent: bool) -> Reduction {
    let (baseline, intervention) = pairs.iter().fold((0, 0), |(a, b), (x, y)| (a + x, b + y));
    let fraction = |b: u128, t: u128| {
        if b >= t {
            (b - t) as f64 / b as f64
        } else {
            -((t - b) as f64 / b as f64)
        }
    };
    let observed_fraction = (baseline > 0).then(|| fraction(baseline, intervention));
    let lower_95 = if independent && pairs.len() >= 2 && pairs.iter().all(|p| p.0 > 0) {
        let mut state = 0x00ad_a9e0_2026_u64;
        let mut samples = Vec::with_capacity(2000);
        for _ in 0..2000 {
            let (mut b, mut t) = (0u128, 0u128);
            for _ in pairs {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let (x, y) = pairs[(state % pairs.len() as u64) as usize];
                b += x;
                t += y;
            }
            samples.push(fraction(b, t));
        }
        samples.sort_by(f64::total_cmp);
        Some(samples[99])
    } else {
        None
    };
    Reduction { baseline, intervention, observed_fraction, lower_95 }
}

pub fn cli(args: &[String]) -> Result<()> {
    let required = args.get(1).map(String::as_str);
    ensure!(
        args.len() == 1
            || (args.len() == 2 && matches!(required, Some("--require-token-target" | "--require-cost-target"))),
        "usage: adage eval paired <study.json> [--require-token-target | --require-cost-target]"
    );
    use std::io::Read;
    let file = std::fs::File::open(&args[0])?;
    let mut body = Vec::new();
    file.take(64 * 1024 * 1024 + 1).read_to_end(&mut body)?;
    ensure!(body.len() <= 64 * 1024 * 1024, "paired study exceeds 64 MiB");
    let study: Study = serde_json::from_slice(&body).context("invalid paired study")?;
    let report = evaluate(&study)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    match required {
        Some("--require-token-target") => {
            ensure!(report.token_target_accepted, "token target is not qualified by this evidence")
        }
        Some("--require-cost-target") => {
            ensure!(report.cost_target_accepted, "cost target is not qualified by this evidence")
        }
        _ => {}
    }
    Ok(())
}
