//! `adage eval bench report` and `adage eval bench bless`.

use super::{to_jsonl, Scorecard};
use adage_tracks::params::{canonical_json, EngineParams};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write;

/// One run of a results file.
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub run_id: String,
    pub label: String,
    pub preset: String,
    pub params: Value,
    pub scorecard: Scorecard,
}

/// The runs of a results file, in file order.
pub fn runs(records: &[Value]) -> Result<Vec<RunSummary>> {
    let mut order: Vec<String> = Vec::new();
    let mut by: BTreeMap<String, (Option<&Value>, Option<&Value>)> = BTreeMap::new();
    for r in records {
        let Some(id) = r["run_id"].as_str() else { continue };
        if !by.contains_key(id) {
            order.push(id.to_string());
        }
        let e = by.entry(id.to_string()).or_default();
        match r["record"].as_str() {
            Some("run") => e.0 = Some(r),
            Some("scorecard") => e.1 = Some(r),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for id in order {
        let (run, sc) = by[&id];
        let Some(sc) = sc else { continue };
        let scorecard: Scorecard = serde_json::from_value(sc.clone()).with_context(|| format!("scorecard of {id}"))?;
        out.push(RunSummary {
            run_id: id.clone(),
            label: scorecard.label.clone(),
            preset: run.and_then(|r| r["preset"].as_str()).unwrap_or("demo").to_string(),
            params: sc["params"].clone(),
            scorecard,
        });
    }
    Ok(out)
}

/// Dotted paths whose values differ between two parameter objects.
fn changed(a: &Value, b: &Value, prefix: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in x {
                let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                changed(v, y.get(k).unwrap_or(&Value::Null), &p, out);
            }
        }
        (x, y) if x != y => out.push(format!("{prefix}={}", canonical_json(x))),
        _ => {}
    }
}

/// The parameters of a run that differ from its preset, as `path=value`.
pub fn params_diff(r: &RunSummary) -> Vec<String> {
    let base = EngineParams::preset(&r.preset).map(|p| serde_json::to_value(p).unwrap_or(Value::Null));
    let mut out = Vec::new();
    if let Ok(b) = base {
        changed(&r.params, &b, "", &mut out);
    }
    out
}

fn term(s: &Scorecard, name: &str) -> f64 {
    if name == "objective" {
        return s.objective;
    }
    s.terms.get(name).and_then(Value::as_f64).unwrap_or(0.0)
}

/// Whether a Pareto axis is minimised: a `-` prefix, or a term that is a cost.
fn axis(name: &str) -> (String, bool) {
    if let Some(n) = name.strip_prefix('-') {
        return (n.to_string(), true);
    }
    if let Some(n) = name.strip_prefix('+') {
        return (n.to_string(), false);
    }
    let minimise = name.contains("false") || name.contains("ece");
    (name.to_string(), minimise)
}

/// The feasible runs no other feasible run dominates on `axes`.
pub fn pareto<'a>(runs: &'a [RunSummary], axes: &[String]) -> Vec<&'a RunSummary> {
    let axes: Vec<(String, bool)> = axes.iter().map(|a| axis(a)).collect();
    let val =
        |r: &RunSummary, (n, min): &(String, bool)| if *min { -term(&r.scorecard, n) } else { term(&r.scorecard, n) };
    let feasible: Vec<&RunSummary> = runs.iter().filter(|r| r.scorecard.feasible).collect();
    feasible
        .iter()
        .copied()
        .filter(|a| {
            !feasible
                .iter()
                .any(|b| axes.iter().all(|x| val(b, x) >= val(a, x)) && axes.iter().any(|x| val(b, x) > val(a, x)))
        })
        .collect()
}

/// The text report: top `k` by objective, the Pareto front, deltas against a baseline, and the
/// infeasible configurations with the guardrails they fail.
pub fn render(records: &[Value], k: usize, axes: &[String], baseline: Option<&[Value]>) -> Result<String> {
    let all = runs(records)?;
    if all.is_empty() {
        bail!("no scorecards in the results");
    }
    let base = match baseline {
        Some(b) => runs(b)?.into_iter().next(),
        None => None,
    };
    let mut s = String::new();
    let feasible = all.iter().filter(|r| r.scorecard.feasible).count();
    let _ = writeln!(s, "bench report: {} configurations, {} feasible", all.len(), feasible);
    let _ = writeln!(
        s,
        "J = turns_saved_at_window/session - λfp·false_directives/session + λgt·route_F1, mean over the task-grouped CV folds.\nThe temporal final check (final-J) is shown for reference and never used for ranking."
    );
    if let Some(b) = &base {
        let _ = writeln!(s, "baseline: {} ({}), J {:.4}", b.label, b.run_id, b.scorecard.objective);
    }
    let mut ranked: Vec<&RunSummary> = all.iter().filter(|r| r.scorecard.feasible).collect();
    ranked.sort_by(|a, b| b.scorecard.objective.total_cmp(&a.scorecard.objective).then_with(|| a.label.cmp(&b.label)));
    let _ = writeln!(s, "\ntop {} by objective", k.min(ranked.len()));
    let _ = writeln!(
        s,
        "  {:>4}  {:<12} {:>8} {:>8} {:>8} {:>8} {:>7} {:>7} {:>8}  params (vs preset)",
        "rank", "label", "J", "ΔJ", "tsw/s", "fd/s", "F1", "RP", "final-J"
    );
    for (i, r) in ranked.iter().take(k).enumerate() {
        let sc = &r.scorecard;
        let delta = base
            .as_ref()
            .map(|b| format!("{:+.4}", sc.objective - b.scorecard.objective))
            .unwrap_or_else(|| "-".into());
        let fin = sc
            .final_check
            .get("objective")
            .and_then(Value::as_f64)
            .map(|x| format!("{x:.4}"))
            .unwrap_or_else(|| "-".into());
        let diff = params_diff(r);
        let _ = writeln!(
            s,
            "  {:>4}  {:<12} {:>8.4} {:>8} {:>8.4} {:>8.4} {:>7.4} {:>7.4} {:>8}  {}",
            i + 1,
            r.label,
            sc.objective,
            delta,
            term(sc, "turns_saved_at_window_per_session"),
            term(sc, "false_directives_per_session"),
            term(sc, "route_f1"),
            term(sc, "route_precision"),
            fin,
            if diff.is_empty() { "(preset)".to_string() } else { diff.join(" ") }
        );
    }
    if let Some(b) = &base {
        if let Some(best) = ranked.first() {
            let _ = writeln!(s, "\nbest vs baseline ({} vs {})", best.label, b.label);
            let mut names: Vec<&String> = best.scorecard.terms.keys().filter(|k| *k != "objective_per_unit").collect();
            names.sort();
            let _ = writeln!(s, "  {:<36} {:>9} {:>9} {:>9}", "term", "best", "baseline", "delta");
            let _ = writeln!(
                s,
                "  {:<36} {:>9.4} {:>9.4} {:>+9.4}",
                "objective",
                best.scorecard.objective,
                b.scorecard.objective,
                best.scorecard.objective - b.scorecard.objective
            );
            for n in names {
                let (x, y) = (term(&best.scorecard, n), term(&b.scorecard, n));
                let _ = writeln!(s, "  {:<36} {:>9.4} {:>9.4} {:>+9.4}", n, x, y, x - y);
            }
        }
    }
    let front = pareto(&all, axes);
    let _ = writeln!(s, "\nPareto front over ({}): {} configurations", axes.join(", "), front.len());
    let mut front: Vec<&&RunSummary> = front.iter().collect();
    front.sort_by(|a, b| b.scorecard.objective.total_cmp(&a.scorecard.objective).then_with(|| a.label.cmp(&b.label)));
    for r in front {
        let vals: Vec<String> =
            axes.iter().map(|a| format!("{}={:.4}", axis(a).0, term(&r.scorecard, &axis(a).0))).collect();
        let _ = writeln!(s, "  {:<12} J {:>8.4}  {}", r.label, r.scorecard.objective, vals.join(" "));
    }
    let infeasible: Vec<&RunSummary> = all.iter().filter(|r| !r.scorecard.feasible).collect();
    let _ = writeln!(s, "\ninfeasible: {}", infeasible.len());
    for r in infeasible {
        let why: Vec<String> = r
            .scorecard
            .guardrails
            .iter()
            .filter(|g| !g.pass)
            .map(|g| format!("{} {} (limit {} {})", g.name, g.value, g.op, g.limit))
            .collect();
        let _ = writeln!(
            s,
            "  {:<12} J {:>8.4}  fails {}  [{}]",
            r.label,
            r.scorecard.objective,
            why.join("; "),
            params_diff(r).join(" ")
        );
    }
    Ok(s)
}

/// The records a baseline keeps for run `run_id`: its aggregates and its scorecard, without
/// `git_sha`.
fn baseline_records(records: &[Value], run_id: &str) -> Vec<Value> {
    records
        .iter()
        .filter(|r| r["run_id"] == run_id && matches!(r["record"].as_str(), Some("aggregate" | "scorecard")))
        .map(|r| {
            let mut r = r.clone();
            if let Some(o) = r.as_object_mut() {
                o.remove("git_sha");
            }
            r
        })
        .collect()
}

/// `bless`: the baseline file's contents for one run of `records` (the only run, or the run
/// labelled `label`), with `reason` recorded in its scorecard.
pub fn bless(records: &[Value], label: Option<&str>, reason: &str) -> Result<String> {
    if reason.trim().is_empty() {
        bail!("bless needs --reason");
    }
    let all = runs(records)?;
    let pick: Vec<&RunSummary> = match label {
        Some(l) => all.iter().filter(|r| r.label == l).collect(),
        None => all.iter().collect(),
    };
    let r = match pick.as_slice() {
        [r] => *r,
        [] => bail!("no run {} in the results", label.unwrap_or("")),
        _ => bail!("the results hold {} runs; choose one with --label", pick.len()),
    };
    let mut out = baseline_records(records, &r.run_id);
    for x in out.iter_mut().filter(|x| x["record"] == "scorecard") {
        x["reason"] = Value::String(reason.to_string());
    }
    Ok(to_jsonl(&out))
}

/// Compare a fresh run with a committed baseline: the same aggregates and scorecard, byte for
/// byte, ignoring `adage_version`, `git_sha` and the blessing's `reason`. `Err` lists the first
/// differences.
pub fn compare_to_baseline(fresh: &[Value], baseline: &[Value]) -> std::result::Result<(), String> {
    let norm = |v: &[Value]| -> Vec<String> {
        v.iter()
            .filter(|r| matches!(r["record"].as_str(), Some("aggregate" | "scorecard")))
            .map(|r| {
                let mut r = r.clone();
                if let Some(o) = r.as_object_mut() {
                    o.remove("adage_version");
                    o.remove("git_sha");
                    o.remove("reason");
                }
                canonical_json(&r)
            })
            .collect()
    };
    let (a, b) = (norm(fresh), norm(baseline));
    if a == b {
        return Ok(());
    }
    let mut msg = format!("{} fresh records, {} in the baseline\n", a.len(), b.len());
    for (i, (x, y)) in a.iter().zip(&b).enumerate().filter(|(_, (x, y))| x != y).take(5) {
        let _ = writeln!(msg, "record {i}:\n  fresh    {x}\n  baseline {y}");
    }
    Err(msg)
}
