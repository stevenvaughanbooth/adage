//! `adage-train report <run>`: what became of each claim, what the run cost, and (with
//! validation) whether each compiled reflex does as well as the skill it came from.

use crate::claims::{argv_of, Claim, ClaimKind};
use crate::harness::Arm;
use crate::oracle::{self, glob_matches, same_command};
use crate::plan::Plan;
use crate::run::{Job, RunState};
use adage_core::CommandTable;
use adage_learn::import::Session;
use adage_tracks::{io, Route, Step, Trigger};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

/// Acceptance bars (per claim, validation phase).
pub const FOLLOW_MIN: f64 = 0.70;
/// The reflex arm may follow at most this much less often than the skill arm.
pub const NON_INFERIORITY: f64 = 0.10;
/// Mean turns and cost of the reflex arm may exceed the skill arm's by at most this share.
pub const COST_TOLERANCE: f64 = 0.10;
/// A command "failed consistently" when it failed in at least this share of the sessions that ran it.
pub const CONSISTENT: (usize, usize) = (9, 10);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    /// A staged reflex does what the claim says.
    Compiled {
        route: String,
    },
    /// The skill says X; X failed consistently and a dead end X → Y was learned.
    Contradicted {
        route: String,
        instead: String,
        failed_sessions: usize,
        ran_sessions: usize,
    },
    /// No session gave the claim a chance.
    NeverExercised {
        reason: String,
    },
    /// Exercised, but no reflex came of it.
    NotCompiled {
        reason: String,
    },
    Unsafe {
        reason: String,
    },
}

impl Verdict {
    pub fn line(&self) -> String {
        match self {
            Verdict::Compiled { route } => format!("compiled → route {route}"),
            Verdict::Contradicted { route, instead, failed_sessions, ran_sessions } => format!(
                "contradicted: failed in {failed_sessions}/{ran_sessions} sessions; learned dead end {route} → `{instead}` (your skill is stale)"
            ),
            Verdict::NeverExercised { reason } => format!("never exercised ({reason})"),
            Verdict::NotCompiled { reason } => format!("not compiled ({reason})"),
            Verdict::Unsafe { reason } => format!("unsafe ({reason})"),
        }
    }
    fn route(&self) -> Option<&str> {
        match self {
            Verdict::Compiled { route } | Verdict::Contradicted { route, .. } => Some(route),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimReport {
    pub id: String,
    pub claim: String,
    pub at: String,
    #[serde(flatten)]
    pub verdict: Verdict,
    /// Exploration sessions that gave it a chance, and those that did what it says.
    pub exercised: usize,
    pub followed: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArmStats {
    pub sessions: usize,
    pub applicable: usize,
    pub followed: usize,
    pub rate: Option<f64>,
    pub mean_turns: Option<f64>,
    pub mean_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Acceptance {
    pub claim: String,
    pub route: Option<String>,
    pub arms: BTreeMap<String, ArmStats>,
    /// `accepted`, `rejected`, or `insufficient` (too few applicable sessions to judge).
    pub result: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub run: String,
    pub source: String,
    pub source_sha256: String,
    pub provenance: String,
    pub harness: String,
    pub model: String,
    pub repo_key: String,
    pub sessions: BTreeMap<String, usize>,
    pub skipped: usize,
    pub failed: usize,
    pub cost_usd: f64,
    pub budget_usd: f64,
    pub claims: Vec<ClaimReport>,
    pub staged_routes: Vec<String>,
    pub acceptance: Vec<Acceptance>,
    pub staged: String,
}

fn staged_routes(dir: &Path) -> Vec<Route> {
    io::read_all(&dir.join("staged/tracks")).unwrap_or_default().into_iter().flat_map(|t| t.routes).collect()
}

fn argv(cmd: &str, table: &CommandTable) -> Vec<String> {
    argv_of(cmd, table).unwrap_or_default()
}

/// Runners a dead end avoids (`python3 -m unittest / pytest`).
fn avoids(r: &Route, cmd: &[String], table: &CommandTable) -> bool {
    match &r.step {
        Step::Avoid { avoid, .. } => avoid.split(" / ").any(|x| {
            let a = argv(x.trim(), table);
            !a.is_empty() && cmd.starts_with(&a)
        }),
        _ => false,
    }
}

/// The staged route that does what the claim says, if any.
fn compiled(c: &Claim, routes: &[Route], table: &CommandTable) -> Option<String> {
    let directs = |r: &Route, want: &[String]| match &r.step {
        Step::Run { command } => same_command(&argv(&command.replace("{item}", "X-1"), table), want),
        Step::Avoid { instead, .. } => same_command(&argv(instead, table), want),
        _ => false,
    };
    routes
        .iter()
        .find(|r| match &c.kind {
            ClaimKind::RegenerateAfter { glob: Some(g), command } => {
                let glob_ok = matches!(&r.trigger, Trigger::AfterEdit { paths } if paths == g || glob_matches(paths, g) || glob_matches(g, paths));
                glob_ok && matches!(&r.step, Step::Run { .. }) && directs(r, &argv(command, table))
            }
            ClaimKind::Verify { command } => directs(r, &argv(command, table)),
            ClaimKind::Avoid { avoid, instead } => {
                avoids(r, &argv(avoid, table), table) && directs(r, &argv(instead, table))
            }
            ClaimKind::Lookup { tool, .. } => match (&r.trigger, &r.step) {
                (Trigger::Prompt { .. }, Step::Run { command }) => {
                    let a = argv(&command.replace("{item}", "X-1"), table);
                    let t = argv(tool, table);
                    !t.is_empty() && a.starts_with(&t)
                }
                _ => false,
            },
            _ => false,
        })
        .map(|r| r.id.clone())
}

fn consistent(failed: usize, ran: usize) -> bool {
    ran > 0 && failed * CONSISTENT.1 >= ran * CONSISTENT.0
}

/// Per claim, the working command a learned dead end gives instead of what it says, if it says
/// something that failed consistently: (route id, instead, failed sessions, ran sessions).
fn contradiction(
    c: &Claim,
    routes: &[Route],
    sessions: &[Session],
    table: &CommandTable,
) -> Option<(String, String, usize, usize)> {
    let said = match &c.kind {
        ClaimKind::Verify { command } => command,
        ClaimKind::Avoid { instead, .. } => instead,
        _ => return None,
    };
    let want = argv(said, table);
    let (mut ran, mut failed) = (0, 0);
    for s in sessions {
        let (n, f) = oracle::runs(s, &want);
        if n > 0 {
            ran += 1;
            failed += usize::from(f == n);
        }
    }
    if !consistent(failed, ran) {
        return None;
    }
    let r = routes.iter().find(|r| avoids(r, &want, table))?;
    let Step::Avoid { instead, .. } = &r.step else { return None };
    Some((r.id.clone(), instead.clone(), failed, ran))
}

fn sessions_of<'a>(
    dir: &Path,
    jobs: impl Iterator<Item = &'a Job>,
    key: &str,
    table: &CommandTable,
) -> Vec<(&'a Job, Session)> {
    jobs.filter_map(|j| {
        let t = j.transcript.as_ref()?;
        Some((j, oracle::read(&dir.join(t), key, table)?))
    })
    .collect()
}

fn mean(xs: &[f64]) -> Option<f64> {
    (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64)
}

pub fn build(dir: &Path, plan: &Plan, state: &RunState) -> Result<Report> {
    let table = CommandTable::load();
    let routes = staged_routes(dir);
    let key = &plan.repo.key;
    let explore = sessions_of(dir, state.jobs.iter().filter(|j| j.phase == "explore"), key, &table);
    let explore_sessions: Vec<Session> = explore.iter().map(|(_, s)| s.clone()).collect();
    let mut claims = Vec::new();
    for c in &plan.claims {
        let exercised = explore_sessions.iter().filter(|s| oracle::exercised(c, s, &table)).count();
        let followed = explore_sessions.iter().filter(|s| oracle::followed(c, None, s, &table) == Some(true)).count();
        let verdict = if let Some(u) = &c.r#unsafe {
            Verdict::Unsafe { reason: u.clone() }
        } else if let Some((route, instead, failed_sessions, ran_sessions)) =
            contradiction(c, &routes, &explore_sessions, &table)
        {
            Verdict::Contradicted { route, instead, failed_sessions, ran_sessions }
        } else if let Some(route) = compiled(c, &routes, &table) {
            Verdict::Compiled { route }
        } else if let Some((_, why)) = plan.not_exercised.iter().find(|(id, _)| id == &c.id) {
            Verdict::NeverExercised { reason: why.clone() }
        } else if exercised == 0 {
            Verdict::NeverExercised { reason: "no exploration session gave it a chance".into() }
        } else {
            let reason = match &c.kind {
                ClaimKind::Verify { command } if followed > 0 => format!(
                    "`{command}` was followed in {followed}/{exercised} sessions, but not after the last edit in \
                     enough of them, or as often without an edit (a claimed check is compiled when sessions run it \
                     after their edits; thresholds {})",
                    state.thresholds
                ),
                _ => format!(
                    "followed in {followed}/{exercised} exploration sessions; below the learning thresholds ({})",
                    state.thresholds
                ),
            };
            Verdict::NotCompiled { reason }
        };
        claims.push(ClaimReport {
            id: c.id.clone(),
            claim: c.kind.describe(),
            at: c.at.clone(),
            verdict,
            exercised,
            followed,
        });
    }

    // Validation: per claim and arm, on the held-out tasks that exercise the claim.
    let validate = sessions_of(dir, state.jobs.iter().filter(|j| j.phase == "validate"), key, &table);
    let mut acceptance = Vec::new();
    if state.jobs.iter().any(|j| j.phase == "validate") {
        for (c, cr) in plan.claims.iter().zip(&claims) {
            if c.is_unsafe() {
                continue;
            }
            let corrected = match &cr.verdict {
                Verdict::Contradicted { instead, .. } => Some(instead.as_str()),
                _ => None,
            };
            let mut arms = BTreeMap::new();
            for arm in [Arm::Baseline, Arm::Skill, Arm::Reflex] {
                let mut st = ArmStats::default();
                let (mut turns, mut cost) = (Vec::new(), Vec::new());
                for (j, s) in validate.iter().filter(|(j, _)| j.arm == arm && j.task.claim.as_deref() == Some(&c.id)) {
                    st.sessions += 1;
                    turns.push(j.outcome.turns as f64);
                    cost.push(j.outcome.cost_usd);
                    if let Some(f) = oracle::followed(c, corrected, s, &table) {
                        st.applicable += 1;
                        st.followed += usize::from(f);
                    }
                }
                st.rate = (st.applicable > 0).then(|| st.followed as f64 / st.applicable as f64);
                st.mean_turns = mean(&turns);
                st.mean_cost_usd = mean(&cost);
                arms.insert(arm.as_str().to_string(), st);
            }
            if arms.values().all(|a| a.sessions == 0) {
                continue; // no held-out task exercised it
            }
            let (skill, reflex) = (&arms["skill"], &arms["reflex"]);
            let mut reasons = Vec::new();
            let result = match (cr.verdict.route(), reflex.rate, skill.rate) {
                (None, _, _) => {
                    reasons.push("no staged reflex for this claim".into());
                    "rejected"
                }
                (Some(_), None, _) | (Some(_), _, None) => {
                    reasons.push("no applicable held-out session in the skill or reflex arm".into());
                    "insufficient"
                }
                (Some(_), Some(r), Some(sk)) => {
                    if r < FOLLOW_MIN {
                        reasons.push(format!("reflex followed {:.0}% < {:.0}%", r * 100.0, FOLLOW_MIN * 100.0));
                    }
                    if r + NON_INFERIORITY < sk {
                        reasons.push(format!(
                            "reflex {:.0}% is more than {:.0} points below the skill arm's {:.0}%",
                            r * 100.0,
                            NON_INFERIORITY * 100.0,
                            sk * 100.0
                        ));
                    }
                    for (what, a, b) in [
                        ("turns", reflex.mean_turns, skill.mean_turns),
                        ("cost", reflex.mean_cost_usd, skill.mean_cost_usd),
                    ] {
                        if let (Some(a), Some(b)) = (a, b) {
                            if a > b * (1.0 + COST_TOLERANCE) + 1e-9 {
                                reasons.push(format!("reflex mean {what} {a:.3} is worse than the skill arm's {b:.3}"));
                            }
                        }
                    }
                    if reasons.is_empty() {
                        "accepted"
                    } else {
                        "rejected"
                    }
                }
            };
            acceptance.push(Acceptance {
                claim: c.id.clone(),
                route: cr.verdict.route().map(str::to_string),
                arms,
                result: result.into(),
                reasons,
            });
        }
    }

    let mut sessions: BTreeMap<String, usize> = BTreeMap::new();
    for j in state.jobs.iter().filter(|j| j.status == "done") {
        *sessions.entry(j.arm.as_str().to_string()).or_default() += 1;
    }
    Ok(Report {
        run: state.run.clone(),
        source: plan.source.path.display().to_string(),
        source_sha256: plan.source.sha256.clone(),
        provenance: state.provenance.clone(),
        harness: state.harness.clone(),
        model: state.model.clone(),
        repo_key: state.repo_key.clone(),
        sessions,
        skipped: state.jobs.iter().filter(|j| j.status == "skipped").count(),
        failed: state.jobs.iter().filter(|j| j.status == "failed").count(),
        cost_usd: state.spent_usd,
        budget_usd: state.budget_usd,
        claims,
        staged_routes: routes.iter().map(|r| r.id.clone()).collect(),
        acceptance,
        staged: dir.join("staged").display().to_string(),
    })
}

fn pct(x: Option<f64>) -> String {
    x.map(|v| format!("{:.0}%", v * 100.0)).unwrap_or_else(|| "-".into())
}

fn num(x: Option<f64>, d: usize) -> String {
    x.map(|v| format!("{v:.d$}")).unwrap_or_else(|| "-".into())
}

pub fn render(r: &Report) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "adage-train report {}", r.run);
    let _ = writeln!(s, "source     {} (sha256 {})", r.source, &r.source_sha256[..12.min(r.source_sha256.len())]);
    let _ = writeln!(s, "harness    {} · model {} · repo {}", r.harness, r.model, r.repo_key);
    let _ = writeln!(
        s,
        "sessions   {} done {:?}, {} skipped (budget), {} failed",
        r.sessions.values().sum::<usize>(),
        r.sessions,
        r.skipped,
        r.failed
    );
    let _ = writeln!(s, "cost       ${:.2} of a ${:.2} cap (as reported by the harness)", r.cost_usd, r.budget_usd);
    let _ = writeln!(
        s,
        "staged     {} ({} routes; provenance {}); nothing is active until reviewed",
        r.staged,
        r.staged_routes.len(),
        r.provenance
    );
    let _ = writeln!(s, "\nclaims:");
    for c in &r.claims {
        let _ = writeln!(s, "  {:<4} {}  [{}]", c.id, c.claim, c.at);
        let _ = writeln!(
            s,
            "       {}   (exploration: exercised {}, followed {})",
            c.verdict.line(),
            c.exercised,
            c.followed
        );
    }
    if !r.acceptance.is_empty() {
        let _ = writeln!(
            s,
            "\nacceptance (held-out; followed ≥ {:.0}%, reflex within {:.0} points of the skill arm, turns/cost no more than {:.0}% worse):",
            FOLLOW_MIN * 100.0,
            NON_INFERIORITY * 100.0,
            COST_TOLERANCE * 100.0
        );
        let _ = writeln!(s, "  claim  arm       n  applicable  followed  turns   cost     result");
        for a in &r.acceptance {
            for (i, arm) in ["baseline", "skill", "reflex"].iter().enumerate() {
                let st = &a.arms[*arm];
                let _ = writeln!(
                    s,
                    "  {:<6} {:<8} {:>2}  {:>10}  {:>8}  {:>5}  {:>7}  {}",
                    if i == 0 { a.claim.as_str() } else { "" },
                    arm,
                    st.sessions,
                    st.applicable,
                    pct(st.rate),
                    num(st.mean_turns, 1),
                    num(st.mean_cost_usd, 3),
                    if i == 2 { a.result.as_str() } else { "" }
                );
            }
            for why in &a.reasons {
                let _ = writeln!(s, "         {why}");
            }
        }
    }
    s
}
