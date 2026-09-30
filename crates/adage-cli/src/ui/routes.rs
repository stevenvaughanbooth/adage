//! The Routes tab's data: every installed route, grouped by track, with its evidence and live
//! counts (from `adage graph`'s build) and the review decisions that apply to it (from `adage
//! review`).

use super::theme::parse_strength;
use adage_store::Paths;
use adage_tracks::Strength;
use anyhow::Result;
use serde_json::Value;
use std::path::Path;

/// Execution authority is independent of learned confidence and evidence review.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionView {
    pub mode: String,
    pub reason: String,
    pub last_result: Option<String>,
}

impl Default for ExecutionView {
    fn default() -> Self {
        Self { mode: "Suggest".into(), reason: "No execution grant".into(), last_result: None }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiveCounts {
    pub emitted: u64,
    pub followed: u64,
    pub ignored: u64,
    pub met: u64,
    pub would_have: u64,
    pub unaided: u64,
    pub not_reached: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteRow {
    /// The track's directory under the tracks directory: what `adage review` takes.
    pub reference: String,
    pub track: String,
    pub version: String,
    pub repo: String,
    pub imported: bool,
    pub in_index: bool,
    pub id: String,
    pub strength: Strength,
    pub ceiling: String,
    pub effect: String,
    pub state: String,
    pub authored: bool,
    pub trigger: String,
    pub step: String,
    pub executable: bool,
    pub support: u64,
    pub opportunities: u64,
    pub rate: f64,
    pub lower_bound: f64,
    pub live: LiveCounts,
    pub directive: String,
    /// Decisions that apply (`adage review <verb>`).
    pub verbs: Vec<&'static str>,
    /// Why it awaits review; empty if it does not.
    pub reasons: Vec<(String, String)>,
    pub execution: ExecutionView,
}

#[derive(Debug, Clone, Default)]
pub struct Routes {
    pub rows: Vec<RouteRow>,
}

fn step_label(s: &Value) -> String {
    let label = s["label"].as_str().unwrap_or("");
    match s["kind"].as_str() {
        Some("run") => format!("run `{label}`"),
        Some("avoid") => format!("avoid `{}`, use `{label}`", s["avoid"].as_str().unwrap_or("?")),
        _ => label.to_string(),
    }
}

pub fn from_graph(
    g: &Value,
    standings: &std::collections::BTreeMap<(String, String), crate::review::Standing>,
) -> Routes {
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    let rows = g["routes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let reference = r["reference"].as_str().unwrap_or("").to_string();
            let id = r["id"].as_str().unwrap_or("").to_string();
            let st = standings.get(&(reference.clone(), id.clone()));
            let (l, e) = (&r["live"], &r["evidence"]);
            RouteRow {
                track: r["track"].as_str().unwrap_or("").to_string(),
                version: r["version"].as_str().unwrap_or("").to_string(),
                repo: r["repo"].as_str().unwrap_or("").to_string(),
                imported: r["imported"].as_bool().unwrap_or(false),
                in_index: r["in_index"].as_bool().unwrap_or(false),
                strength: r["strength"].as_str().and_then(parse_strength).unwrap_or(Strength::Prime),
                ceiling: r["ceiling"].as_str().unwrap_or("").to_string(),
                effect: r["effect"].as_str().unwrap_or("").to_string(),
                state: r["state"].as_str().unwrap_or("").to_string(),
                authored: r["authored"].as_bool().unwrap_or(false),
                trigger: r["trigger"]["label"].as_str().unwrap_or("").to_string(),
                step: step_label(&r["step"]),
                executable: r["step"]["kind"].as_str() == Some("run"),
                support: n(&e["support"]),
                opportunities: n(&e["opportunities"]),
                rate: e["rate"].as_f64().unwrap_or(0.0),
                lower_bound: e["lower_bound"].as_f64().unwrap_or(0.0),
                live: LiveCounts {
                    emitted: n(&l["emitted"]),
                    followed: n(&l["followed"]),
                    ignored: n(&l["ignored"]),
                    met: n(&l["obligation_met"]),
                    would_have: n(&l["would_have"]),
                    unaided: n(&l["control"]["unaided"]),
                    not_reached: n(&l["control"]["not_reached"]),
                },
                directive: r["directive"].as_str().unwrap_or("").to_string(),
                verbs: st.map(|s| s.verbs.clone()).unwrap_or_default(),
                reasons: st
                    .map(|s| s.reasons.iter().map(|(k, d)| (k.to_string(), d.clone())).collect())
                    .unwrap_or_default(),
                execution: ExecutionView::default(),
                reference,
                id,
            }
        })
        .collect();
    Routes { rows }
}

pub fn load(paths: &Paths, root: Option<&Path>) -> Result<Routes> {
    let g = crate::graph::build(paths, 0)?;
    let standings = crate::review::standings(paths)?;
    let mut rows = from_graph(&g, &standings);
    let store = adage_exec::approvals::Store::load(&adage_exec::approvals::path(&paths.root));
    let index = adage_tracks::Index::load(&paths.index());
    let table = adage_core::CommandTable::load();
    let policy = adage_exec::policy::load(&paths.root);
    let enabled = crate::exec::marker(paths).is_file()
        || crate::detect::registrations(root)
            .iter()
            .any(|r| r.binary.as_ref().and_then(|p| p.file_stem()).is_some_and(|name| name == "adage-exec"));
    let observe_only = adage_store::config::load(paths).config.runtime_control >= 1.0;
    let current_repo = root.map(adage_core::repo::repo_key);
    for row in &mut rows.rows {
        let label = format!("{}/{}", row.reference, row.id);
        let track_label = format!("{}/{}", row.track, row.id);
        let view = &mut row.execution;
        if std::env::var("ADAGE_DISABLE").is_ok_and(|v| v == "1") {
            view.mode = "Disabled".into();
            view.reason = "ADAGE_DISABLE is set".into();
            continue;
        }
        if root
            .is_some_and(|p| current_repo.as_deref() == Some(row.repo.as_str()) && p.join(".adage/disabled").exists())
        {
            view.mode = "Disabled".into();
            view.reason = "Repository kill switch is set".into();
            continue;
        }
        if !row.in_index || matches!(row.state.as_str(), "extinguished" | "nascent") {
            view.mode = "Inactive".into();
            view.reason = "Not active in the runtime index".into();
            continue;
        }
        if !row.executable {
            view.reason = "Learned guidance; the agent handles this step".into();
            continue;
        }
        let store = match &store {
            Ok(s) => s,
            Err(_) => {
                view.mode = "Unavailable".into();
                view.reason = "Execution grants could not be read".into();
                continue;
            }
        };
        let grants: Vec<_> = store
            .approvals
            .iter()
            .filter(|a| a.key.repo == row.repo && (a.route == label || a.route == track_label))
            .collect();
        if grants.is_empty() {
            if let Some(r) =
                store.revoked.iter().rev().find(|r| r.repo == row.repo && (r.route == label || r.route == track_label))
            {
                view.reason = format!("Execution revoked: {}", r.cause);
            } else if let (Some(root), Ok(policy), Some(ir)) = (
                root.filter(|_| current_repo.as_deref() == Some(row.repo.as_str())),
                &policy,
                index.routes.iter().find(|ir| ir.repo == row.repo && ir.route.id == row.id && ir.track == row.track),
            ) {
                if policy.auto_approve {
                    if let Ok(plan) =
                        adage_exec::route::plan(ir, &index, root, &paths.tracks(), adage_exec::Level::E1, None, &table)
                    {
                        let hazards = adage_exec::git::hazards(root);
                        let context = adage_exec::approvals::AutoContext {
                            table: &table,
                            git_hazards: &hazards,
                            sandbox_enforced: adage_exec::sandbox::detect().available(),
                            policy_auto: true,
                        };
                        if adage_exec::approvals::auto_eligible(&plan.request, &context).is_ok()
                            && policy
                                .permits(&plan.request, adage_exec::classify::effect(&plan.request.argv, &table))
                                .is_ok()
                        {
                            view.mode = if enabled { "Automatic" } else { "Suggest" }.into();
                            view.reason = if enabled {
                                "Inert read-only command permitted by local policy"
                            } else {
                                "Local policy permits this command; automatic execution is off"
                            }
                            .into();
                        }
                    }
                }
            }
            continue;
        }
        let Some(root) = root.filter(|_| current_repo.as_deref() == Some(row.repo.as_str())) else {
            view.mode = "Grant saved".into();
            view.reason = "Open this repository to validate its scripts and scope".into();
            continue;
        };
        let Some(ir) =
            index.routes.iter().find(|ir| ir.repo == row.repo && ir.route.id == row.id && ir.track == row.track)
        else {
            continue;
        };
        let mut checked = false;
        let mut reason = "Executable changed; review the grant again".to_string();
        for grant in grants {
            match adage_exec::route::plan(
                ir,
                &index,
                root,
                &paths.tracks(),
                grant.key.level,
                grant.terms.as_ref(),
                &table,
            ) {
                Ok(plan) => {
                    let key = adage_exec::approvals::ApprovalKey::for_request(&plan.request, &ir.repo);
                    if key != grant.key {
                        continue;
                    }
                    let classified = adage_exec::classify::effect(&plan.request.argv, &table);
                    match &policy {
                        Ok(p) => match p.permits(&plan.request, classified) {
                            Ok(()) => {
                                checked = true;
                                reason = match &plan.request.action {
                                    Some(action) if action.kind == adage_exec::request::ActionKind::Regenerate => {
                                        format!("Grant matches · may change {}", action.outputs.join(", "))
                                    }
                                    Some(_) => "Grant matches · verification cannot change sources".into(),
                                    None => "Grant matches · read-only command".into(),
                                };
                                break;
                            }
                            Err(e) => reason = e,
                        },
                        Err(_) => reason = "Execution policy could not be read".into(),
                    }
                }
                Err(e) => reason = e,
            }
        }
        view.mode = if !checked {
            "Needs review"
        } else if enabled {
            "Automatic"
        } else {
            "Suggest"
        }
        .into();
        view.reason = if checked && !enabled { "Grant valid; automatic execution is off".into() } else { reason };
        if checked && enabled && !adage_exec::sandbox::detect().available() {
            view.mode = "Unavailable".into();
            view.reason = "No supported execution sandbox".into();
        }
    }
    // Read once for the whole view. A damaged journal must never look like an empty history.
    let receipts = adage_exec::journal::list(&paths.root);
    let worktree = root.and_then(|r| r.canonicalize().ok());
    for row in &mut rows.rows {
        if observe_only && row.execution.mode == "Automatic" {
            row.execution.mode = "Observe".into();
            row.execution.reason = "New sessions default to quiet controls".into();
        }
        let has_e2 = store.as_ref().is_ok_and(|s| {
            s.approvals.iter().any(|a| {
                a.key.repo == row.repo
                    && a.key.level == adage_exec::Level::E2
                    && (a.route == format!("{}/{}", row.reference, row.id)
                        || a.route == format!("{}/{}", row.track, row.id))
            })
        });
        match &receipts {
            Ok(receipts) => {
                if let Some(receipt) = receipts.iter().rev().find(|r| {
                    r.repo == row.repo
                        && (r.route == format!("{}/{}", row.reference, row.id)
                            || r.route == format!("{}/{}", row.track, row.id))
                }) {
                    row.execution.last_result = Some(receipt_label(receipt));
                }
                if has_e2 && row.execution.mode == "Automatic" {
                    if let Some(pending) = receipts
                        .iter()
                        .find(|r| r.needs_recovery() && worktree.as_deref() == Some(Path::new(&r.worktree)))
                    {
                        row.execution.mode = "Needs recovery".into();
                        row.execution.reason = format!(
                            "E2 paused · inspect operation {}",
                            pending.id.chars().take(12).collect::<String>()
                        );
                    }
                }
            }
            Err(_) => {
                row.execution.last_result = Some("History unavailable · execution journal could not be read".into());
                if has_e2 && row.execution.mode == "Automatic" {
                    row.execution.mode = "Unavailable".into();
                    row.execution.reason = "E2 paused · execution journal could not be read".into();
                }
            }
        }
    }
    Ok(rows)
}

fn receipt_label(receipt: &adage_exec::journal::Receipt) -> String {
    use adage_exec::{journal::Phase, stage::Outcome};
    let result = if receipt.needs_recovery() {
        "Unknown · inspect the worktree before acknowledging".to_string()
    } else if receipt.phase == Phase::Acknowledged {
        "Unknown · acknowledged after inspection; success was not confirmed".to_string()
    } else {
        match &receipt.outcome {
            Outcome::Applied { changed, .. } => format!("Completed · {} file(s) changed", changed.len()),
            Outcome::Failed { exit, signal } => format!("Failed · exit {exit:?}, signal {signal:?}"),
            Outcome::Refused { reason, .. } => format!("Refused · {}", adage_exec::reason_text(reason)),
            Outcome::Unknown { .. } => "Unknown".into(),
        }
    };
    format!("{} · {result}", receipt.ts)
}

/// A review verb and its key in the Reflexes tab.
pub const ACTIONS: [(char, &str); 6] =
    [('a', "approve"), ('e', "extinguish"), ('w', "weaken"), ('s', "strengthen"), ('n', "pin"), ('x', "dismiss")];

#[cfg(test)]
mod tests {
    use super::*;
    use adage_exec::approvals::{self, Approval, ApprovalKey, Store};
    use adage_tracks::*;

    #[test]
    fn interrupted_and_acknowledged_receipts_never_display_success() {
        use adage_exec::{
            journal::{Phase, Receipt},
            stage::Outcome,
        };
        let mut receipt = Receipt {
            id: "operation".into(),
            ts: "2026-09-29T00:00:00Z".into(),
            session: "session".into(),
            repo: "repo".into(),
            worktree: "/project".into(),
            route: "track/route".into(),
            approved_on: "2026-09-29T00:00:00Z".into(),
            level: "e2".into(),
            phase: Phase::Prepared,
            outcome: Outcome::Unknown { reason: "completion lost".into() },
            input_digest: String::new(),
            outputs: Default::default(),
        };
        assert!(receipt_label(&receipt).contains("Unknown · inspect"));
        receipt.phase = Phase::Acknowledged;
        assert!(receipt_label(&receipt).contains("success was not confirmed"));
        receipt.phase = Phase::Completed;
        receipt.outcome = Outcome::Applied { changed: vec![], discarded: 0 };
        assert!(receipt_label(&receipt).contains("Completed · 0 file(s) changed"));
    }

    #[test]
    fn execution_view_rechecks_grants_and_does_not_confuse_evidence_with_authority() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        let script = root.join("scripts/lookup");
        std::fs::write(&script, "printf '{\"status\":\"ready\"}'\n").unwrap();
        let repo = adage_core::repo::repo_key(&root);
        let paths = Paths::at(temp.path().join("home"));
        let track = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "project/workflow".into(),
                    version: "0.1.0".into(),
                    description: String::new(),
                    provenance: "desire-path".into(),
                },
                scope: Scope { repo: repo.clone() },
                gauge: Gauge::default(),
            },
            routes: vec![Route {
                id: "lookup".into(),
                summary: String::new(),
                trigger: Trigger::Prompt { span: adage_core::spans::SpanKind::Ticket },
                step: Step::Run { command: "sh scripts/lookup".into() },
                slots: vec![],
                effect: adage_core::Effect::Observe,
                strength: Strength::Fire,
                state: RouteState::Firm,
                exit: vec![],
                evidence: Evidence { support: 9, opportunities: 10, days: 3, ..Default::default() },
                authored: false,
            }],
        };
        adage_tracks::io::write(&paths.tracks().join("project/workflow"), &track).unwrap();
        let index = Index::compile(&[track]);
        index.write_atomic(&paths.index()).unwrap();
        let view = || load(&paths, Some(&root)).unwrap().rows.remove(0).execution;
        assert_eq!(view().mode, "Suggest", "firm evidence does not grant execution");
        let plan = adage_exec::route::plan(
            &index.routes[0],
            &index,
            &root,
            &paths.tracks(),
            adage_exec::request::Level::E1,
            None,
            &adage_core::CommandTable::load(),
        )
        .unwrap();
        let mut store = Store::default();
        store.approve(Approval {
            key: ApprovalKey::for_request(&plan.request, &repo),
            route: "project/workflow/lookup".into(),
            approved_on: "2026-09-29T00:00:00Z".into(),
            by: None,
            basis: Value::Null,
            auto: false,
            terms: None,
        });
        store.save(&approvals::path(&paths.root)).unwrap();
        assert_eq!(view().mode, "Suggest", "grant does not turn the executing hook on");
        std::fs::create_dir_all(paths.root.join("exec")).unwrap();
        std::fs::write(crate::exec::marker(&paths), "").unwrap();
        if adage_exec::sandbox::detect().available() {
            assert_eq!(view().mode, "Automatic");
            adage_store::config::set(&paths, "runtime.control", "1").unwrap();
            assert_eq!(view().mode, "Observe");
            adage_store::config::set(&paths, "runtime.control", "0").unwrap();
        }
        assert_eq!(load(&paths, None).unwrap().rows[0].execution.mode, "Grant saved");
        std::fs::write(&script, "printf changed\n").unwrap();
        assert_eq!(view().mode, "Needs review", "script changes invalidate the displayed grant");
        std::fs::create_dir_all(root.join(".adage")).unwrap();
        std::fs::write(root.join(".adage/disabled"), "").unwrap();
        assert_eq!(view().mode, "Disabled");
    }
}
