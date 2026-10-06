//! Synthetic arithmetic and qualification contracts, not evidence of production savings.
use adage_eval::paired::{evaluate, Study, StudyKind};

fn study(n: usize) -> Study {
    let mut study: Study = serde_json::from_str(include_str!("fixtures/paired-study.json")).unwrap();
    let template = study.pairs[0].clone();
    study.pairs = (0..n)
        .map(|i| {
            let mut p = template.clone();
            p.identity.task_id = Some(format!("TASK-{i}"));
            p.baseline.id = format!("baseline-{i}");
            p.intervention.id = format!("intervention-{i}");
            p
        })
        .collect();
    study
}

#[test]
fn cached_input_is_a_subset_and_savings_require_quality_evidence() {
    let r = evaluate(&study(200)).unwrap();
    assert_eq!((r.independent_groups, r.baseline_passed, r.intervention_passed), (200, 200, 200));
    let tokens = r.tokens.unwrap();
    assert_eq!((tokens.baseline, tokens.intervention), (200_000, 150_000));
    assert_eq!((tokens.observed_fraction, tokens.lower_95), (Some(0.25), Some(0.25)));
    let expected_quality_lower = -(1.0 - 0.025_f64.powf(1.0 / 200.0));
    assert!((r.quality_delta_lower_95.unwrap() - expected_quality_lower).abs() < 1e-10);
    assert!(r.quality_accepted && r.token_target_accepted && r.cost_target_accepted);
    let thin = evaluate(&study(30)).unwrap();
    assert!(!thin.quality_accepted && !thin.token_target_accepted);
    assert!(thin.reasons.contains("quality_noninferiority_not_established"));
}

#[test]
fn fewer_tokens_cannot_mask_a_quality_regression() {
    let mut s = study(200);
    for p in s.pairs.iter_mut().step_by(20) {
        p.intervention.passed = Some(false);
    }
    let r = evaluate(&s).unwrap();
    assert_eq!(r.regressions, 10);
    assert_eq!(r.tokens.unwrap().observed_fraction, Some(0.25));
    assert!(!r.quality_accepted && !r.token_target_accepted && !r.cost_target_accepted);
}

#[test]
fn exactly_twenty_percent_clears_a_twenty_percent_target() {
    let mut s = study(200);
    for p in &mut s.pairs {
        let usage = p.intervention.usage.as_mut().unwrap();
        usage.output_tokens = 200;
        usage.cost_microusd = Some(1600);
    }
    let r = evaluate(&s).unwrap();
    assert_eq!(r.tokens.unwrap().lower_95, Some(0.20));
    assert!(r.token_target_accepted && r.cost_target_accepted);
}

#[test]
fn missing_usage_or_cost_is_never_filled_with_zero() {
    let mut s = study(200);
    s.pairs[0].intervention.usage = None;
    let r = evaluate(&s).unwrap();
    assert_eq!((r.missing_usage, r.missing_cost), (1, 1));
    assert!(r.tokens.is_none() && r.cost_microusd.is_none());
    assert!(!r.token_target_accepted && !r.cost_target_accepted);
    let mut s = study(200);
    s.pairs[0].intervention.usage.as_mut().unwrap().cost_microusd = None;
    let r = evaluate(&s).unwrap();
    assert!(r.token_target_accepted && !r.cost_target_accepted);
    assert!(r.cost_microusd.is_none());
}

#[test]
fn doing_nothing_in_both_arms_is_not_a_savings_success() {
    let mut s = study(200);
    for p in &mut s.pairs {
        p.baseline.passed = Some(false);
        p.intervention.passed = Some(false);
    }
    let r = evaluate(&s).unwrap();
    assert!(r.reasons.contains("baseline_quality_too_low"));
    assert!(!r.token_target_accepted && !r.cost_target_accepted);
}

#[test]
fn invalid_comparisons_never_qualify_as_savings_evidence() {
    for problem in [
        "training_overlap",
        "repeated_task_group",
        "unknown_task_identity",
        "missing_or_reused_run_id",
        "unmatched_conditions",
        "unknown_task_outcomes",
        "model_mismatch",
        "invalid_cached_token_count",
        "training_identity_incomplete",
    ] {
        let mut s = study(200);
        match problem {
            "training_overlap" => {
                s.training_groups.insert(s.pairs[0].identity.group(&s.pairs[0].repo, "").unwrap().key);
            }
            "repeated_task_group" => s.pairs[1].identity = s.pairs[0].identity.clone(),
            "unknown_task_identity" => s.pairs[0].identity = Default::default(),
            "missing_or_reused_run_id" => s.pairs[1].baseline.id = s.pairs[0].baseline.id.clone(),
            "unmatched_conditions" => s.pairs[0].intervention.evaluator_revision = "different-checks".into(),
            "unknown_task_outcomes" => s.pairs[0].intervention.passed = None,
            "model_mismatch" => s.pairs[0].intervention.model = "other-model".into(),
            "invalid_cached_token_count" => {
                s.pairs[0].intervention.usage.as_mut().unwrap().cached_input_tokens = 10_000
            }
            _ => s.training_inferred_identity = 1,
        }
        let r = evaluate(&s).unwrap();
        assert!(r.reasons.contains(problem), "{problem}: {:?}", r.reasons);
        assert!(!r.quality_accepted && !r.token_target_accepted && !r.cost_target_accepted, "{problem}");
    }
}

#[test]
fn tokens_billed_cost_and_model_transfer_are_separate_findings() {
    let mut s = study(200);
    for p in &mut s.pairs {
        p.intervention.usage.as_mut().unwrap().cost_microusd = Some(4000);
    }
    let r = evaluate(&s).unwrap();
    assert!(r.token_target_accepted && !r.cost_target_accepted);
    assert_eq!(r.cost_microusd.unwrap().observed_fraction, Some(-1.0));
    let mut s = study(200);
    s.kind = StudyKind::ModelTransfer;
    for p in &mut s.pairs {
        p.intervention.model = "cheaper-fixture-model".into();
    }
    let r = evaluate(&s).unwrap();
    assert!(r.quality_accepted && r.cost_target_accepted);
}

#[test]
fn paired_bootstrap_and_report_are_input_order_independent() {
    let mut s = study(200);
    for (i, p) in s.pairs.iter_mut().enumerate() {
        p.intervention.usage.as_mut().unwrap().output_tokens += i as u64;
    }
    let first = evaluate(&s).unwrap();
    s.pairs.reverse();
    assert_eq!(serde_json::to_vec(&first).unwrap(), serde_json::to_vec(&evaluate(&s).unwrap()).unwrap());
    assert!(first.tokens.unwrap().lower_95.unwrap() < 0.20);
    assert!(!first.token_target_accepted);
}
