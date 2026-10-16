//! Stateful verification evidence, mined alongside command sequences.
//!
//! A tool description can say *why* a check was attempted, but is not proof of its setup or
//! success. Keep that claim separate from the runner's report and from attributable execution.
//! These proposals never enter the runtime index. In particular, neither a worker's accepted
//! handback nor a successful mutant run is a positive task grade.

use adage_core::outcome::{self, VerificationOutcome, VerificationReport};
use adage_core::CanonicalAction;
use adage_trace::{Session, StepObs};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MAX_CHECKS: usize = 512;
const MAX_EXAMPLES: usize = 24;
const MAX_PROPOSALS: usize = 64;
const MAX_RECOVERY_CALLS: usize = 4096;

#[path = "procedures_persistence.rs"]
mod persistence;

#[path = "procedures_validators.rs"]
mod validators;
pub use validators::{DeclarationOrigin, DeclarationTiming, DeclaredValidator, ValidatorObservation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcedureKind {
    CleanState,
    NegativeControl,
    TestDiscovery,
    CheckScope,
    UnexplainedRecovery,
    PersistenceEquivalence,
}

impl ProcedureKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::CleanState => "verify from clean state",
            Self::NegativeControl => "challenge the intended assertion",
            Self::TestDiscovery => "confirm test discovery",
            Self::CheckScope => "preserve unresolved check scope",
            Self::UnexplainedRecovery => "investigate an unexplained passing retry",
            Self::PersistenceEquivalence => "investigate persistence equivalence checks",
        }
    }

    pub fn template(self) -> Option<&'static str> {
        match self {
            Self::CleanState => Some("verification-clean-state"),
            Self::NegativeControl => Some("verification-negative-control"),
            Self::TestDiscovery => Some("verification-discovery"),
            Self::CheckScope => None,
            Self::UnexplainedRecovery => Some("verification-unexplained-recovery"),
            Self::PersistenceEquivalence => None,
        }
    }
}

/// A bounded case-name line observed in a recognized runner's output. This is a method lead,
/// not proof that the case ran under this command or that its assertion establishes the invariant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedCaseObservation {
    pub name: String,
    pub reported_failed: bool,
}

/// Exact joins into the canonical corpus. Raw command/code/prompt text is deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub session: String,
    pub step: usize,
    pub turn: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    /// Semantic command key, not a runnable command or permission to execute it.
    pub key: String,
    pub outcome: VerificationOutcome,
    /// A runner summary in shared shell output is observable even when attribution is unknown.
    pub reported: VerificationReport,
    /// Labels inferred from the agent's own description of this invocation, not from stdout.
    /// They are claims to investigate, never certificates of a clean tree or applied mutation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_purpose: Option<ProcedureKind>,
    /// Named-case observations remain separate from agent-owned method descriptions and the
    /// attributable whole-command result. Shared/filtered output cannot establish ownership.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_case_observations: Vec<NamedCaseObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    pub kind: ProcedureKind,
    pub observation: String,
    pub checks: Vec<Check>,
    /// A receipt from a differently scoped command cannot discharge this check.
    pub closure: String,
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// A learned procedure proposal. Support is independent leakage groups, never worker count.
/// The schema is deliberately separate from runnable routes and their approval machinery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub kind: ProcedureKind,
    pub name: String,
    pub command_key: String,
    pub sessions: usize,
    pub evidence_groups: usize,
    pub episodes: Vec<Episode>,
    pub episodes_omitted: usize,
    pub status: String,
    pub requirements: Vec<String>,
    pub template: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Learning {
    pub checks: usize,
    pub unattributed_checks: usize,
    pub transport_disagreements: usize,
    pub delegated_sessions: usize,
    pub proposals: Vec<Proposal>,
    /// Declarative project checks joined to observed script requests. These are investigative
    /// leads, separate from runner reports, execution capabilities and active reflexes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declared_validators: Vec<DeclaredValidator>,
}

fn purpose(description: &str) -> Option<ProcedureKind> {
    let d = description.to_ascii_lowercase();
    let words: BTreeSet<_> = d.split(|c: char| !c.is_ascii_alphabetic()).filter(|s| !s.is_empty()).collect();
    if ["mutant", "mutants", "mutated"].iter().any(|w| words.contains(w))
        || ["mutation test", "mutation check", "negative control", "negative-control"].iter().any(|p| d.contains(p))
    {
        Some(ProcedureKind::NegativeControl)
    } else if persistence::purpose(description) {
        Some(ProcedureKind::PersistenceEquivalence)
    } else if ["clean build", "clean state", "clean export", "fresh export", "fresh checkout", "cold build"]
        .iter()
        .any(|s| d.contains(s))
    {
        Some(ProcedureKind::CleanState)
    } else {
        None
    }
}

fn describe(st: &StepObs) -> &str {
    st.call.input.get("description").and_then(|v| v.as_str()).unwrap_or("")
}

fn check(s: &Session, index: usize, st: &StepObs) -> Option<Check> {
    if st.call.by != adage_trace::By::Agent || st.call.refused() {
        return None;
    }
    let CanonicalAction::Exec { argv, .. } = &st.norm.action else { return None };
    let report = outcome::verification_report(argv, &st.call.output)?;
    let attributable = st.verification()?;
    let persistence_report = report.tests_run.unwrap_or(0) > 0
        && matches!(report.outcome, VerificationOutcome::Passed | VerificationOutcome::Failed);
    Some(Check {
        session: s.id.clone(),
        step: index,
        turn: st.turn,
        call_id: st.call.tool_use_id.clone(),
        key: st.key.clone(),
        outcome: attributable.outcome,
        reported: report,
        claimed_purpose: purpose(describe(st))
            .filter(|kind| *kind != ProcedureKind::PersistenceEquivalence || persistence_report),
        named_case_observations: if persistence_report { persistence::named_cases(s, st, argv) } else { Vec::new() },
    })
}

fn episode(s: &Session, kind: ProcedureKind, observation: &str, checks: Vec<Check>, closure: &str) -> Episode {
    Episode {
        kind,
        observation: observation.into(),
        checks,
        closure: closure.into(),
        group: procedure_group(s),
        parent: s.delegation.as_ref().map(|d| d.parent_id.clone()),
    }
}

fn procedure_group(s: &Session) -> Option<String> {
    // Production resolves complete native lineages before mining. Direct callers can still
    // supply sibling workers with different task prompts; those are not independent evidence.
    // Caller-assigned identities, including an attempt-only identity, remain authoritative.
    if s.identity.is_empty() {
        if let Some(parent) = s.delegation.as_ref().map(|d| d.parent_id.as_str()).filter(|p| !p.is_empty()) {
            let mut hash = Sha256::new();
            for part in ["adage-procedure-parent-v1", &s.repo, &s.harness, parent] {
                hash.update((part.len() as u64).to_le_bytes());
                hash.update(part.as_bytes());
            }
            return Some(format!("parent-v1:{:x}", hash.finalize()));
        }
    }
    s.evidence_group().map(|g| g.key)
}

fn requirements(kind: ProcedureKind) -> Vec<String> {
    let specific = match kind {
        ProcedureKind::CleanState => [
            "Confirm the source revision, generated inputs, clean-state materialization and required repository metadata.",
            "Recheck the repaired source from the same qualified clean state; a warm retry alone is insufficient.",
        ],
        ProcedureKind::NegativeControl => [
            "Confirm a passing baseline, an actually applied bounded mutation, and the specific requirement it violates.",
            "Require the relevant assertion to fail, then restore and recheck; setup errors and unrelated failures do not count.",
        ],
        ProcedureKind::TestDiscovery => [
            "Bind the project runner, configuration and intended targets using actual execution/discovery evidence.",
            "A process exit or an empty suite does not establish test coverage.",
        ],
        ProcedureKind::CheckScope => [
            "Compare source revision, configuration and targets before relating two check results.",
            "Retain each broader failure until a matching rerun resolves it.",
        ],
        ProcedureKind::PersistenceEquivalence => [
            "Review the declared persistence invariant and actual test assertions; method wording and named-case output are investigative leads only.",
            "Qualify save/reopen or equivalent ingestion paths on the same logical state, with attributable results and source identity before inferring a repair.",
        ],
        ProcedureKind::UnexplainedRecovery => [
            "Recorded calls contain no intervening invalidating action; unchanged source, inputs and environment are not established.",
            "Investigate the conflicting results and retain the earlier failure; a passing retry alone proves neither a repair nor flakiness.",
        ],
    };
    specific
        .into_iter()
        .map(str::to_string)
        .chain([
            "Review claimed setup and independent evidence before promotion; no task outcome is inferred.".into(),
            "Bind a precise live trigger before delivery; this proposal grants no execution authority.".into(),
        ])
        .collect()
}

/// Pure, bounded, deterministic extraction. Descriptions propose method labels; only runner
/// observations supply results. It never reads a checkout, executes a command or calls a model.
pub fn learn(sessions: &[&Session]) -> Learning {
    let mut out = Learning { declared_validators: validators::learn(sessions), ..Learning::default() };
    let mut found: BTreeMap<(ProcedureKind, String), Vec<Episode>> = BTreeMap::new();
    let mut ordered = sessions.to_vec();
    ordered.sort_by(|a, b| (&a.repo, &a.id).cmp(&(&b.repo, &b.id)));
    ordered.dedup_by(|a, b| a.repo == b.repo && a.id == b.id);
    for s in ordered {
        for e in unexplained_recoveries(s) {
            found.entry((e.kind, e.checks[1].key.clone())).or_default().push(e);
        }
        out.delegated_sessions += usize::from(s.delegation.is_some());
        let mut seen = BTreeSet::new();
        let checks: Vec<_> = s
            .steps
            .iter()
            .enumerate()
            .filter_map(|(i, st)| {
                let c = check(s, i, st)?;
                // A shell call can expand into many canonical steps. Never count a repeated copy.
                let call = c.call_id.clone().unwrap_or_else(|| {
                    format!("{}:{}", c.turn, serde_json::to_string(&st.call.input).unwrap_or_default())
                });
                if !seen.insert((call, c.key.clone())) {
                    return None;
                }
                Some(c)
            })
            .take(MAX_CHECKS)
            .collect();
        out.checks += checks.len();
        for (at, c) in checks.iter().enumerate() {
            out.unattributed_checks += usize::from(c.outcome == VerificationOutcome::Unknown);
            out.transport_disagreements += usize::from(
                matches!(c.outcome, VerificationOutcome::Failed | VerificationOutcome::SetupFailure)
                    && !s.steps[c.step].call.failed,
            );
            // A directed check or an Adage instruction is not organic learning evidence.
            if s.influence
                .iter()
                .any(|d| d.at_step as usize <= c.step && (d.all || !d.commands.is_empty() || !d.avoided.is_empty()))
            {
                continue;
            }
            let mut add = |kind, observation: &str, evidence: Vec<Check>, closure: &str| {
                found.entry((kind, c.key.clone())).or_default().push(episode(s, kind, observation, evidence, closure));
            };
            if let Some(kind) = c
                .claimed_purpose
                .or_else(|| (!c.named_case_observations.is_empty()).then_some(ProcedureKind::PersistenceEquivalence))
                .filter(|kind| {
                    *kind != ProcedureKind::CleanState
                        || matches!(c.reported.outcome, VerificationOutcome::Failed | VerificationOutcome::SetupFailure)
                        || checks[..at].iter().rev().take(8).any(|p| {
                            p.key == c.key
                                && matches!(
                                    p.reported.outcome,
                                    VerificationOutcome::Failed | VerificationOutcome::SetupFailure
                                )
                        })
                })
            {
                let mut evidence = Vec::new();
                // Preserve a nearby normal baseline with the same exact runner, cwd and prompt.
                if let Some(baseline) = checks[..at].iter().rev().take(8).find(|p| {
                    p.claimed_purpose.is_none() && p.outcome == VerificationOutcome::Passed && same_scope(s, p, c)
                }) {
                    evidence.push(baseline.clone());
                }
                evidence.push(c.clone());
                if let Some(next) = checks[at + 1..].iter().take(8).find(|p| same_scope(s, c, p)) {
                    evidence.push(next.clone());
                }
                let observation = match kind {
                    ProcedureKind::PersistenceEquivalence if c.claimed_purpose.is_none() => {
                        "runner output contains a persistence-equivalence case name consistent with the recorded task; invocation ownership and the invariant remain unverified"
                    }
                    ProcedureKind::PersistenceEquivalence => {
                        "agent described a persistence-equivalence check with a runner report; setup, assertions and any repair remain unverified"
                    }
                    _ if c.outcome == VerificationOutcome::Unknown => {
                        "shared or filtered output has a runner report; the claimed check's result is unverified"
                    }
                    ProcedureKind::NegativeControl
                        if c.reported.tests_run.unwrap_or(0) > 0
                            && c.reported.outcome == VerificationOutcome::Passed =>
                    {
                        "claimed mutation survived the reported tests"
                    }
                    ProcedureKind::NegativeControl
                        if c.reported.outcome == VerificationOutcome::Failed
                            && c.reported.tests_run.unwrap_or(0) > 0 =>
                    {
                        "claimed mutation produced test failures; target assertion remains unverified"
                    }
                    ProcedureKind::NegativeControl => "claimed mutation check has incomplete setup or result evidence",
                    ProcedureKind::CleanState
                        if matches!(
                            c.reported.outcome,
                            VerificationOutcome::Failed | VerificationOutcome::SetupFailure
                        ) =>
                    {
                        "check in claimed clean state exposed a reported failure"
                    }
                    _ => "check in claimed clean state needs source and artifact provenance",
                };
                add(kind, observation, evidence, "requires setup and matching recheck evidence");
            }
            let deliberate_discovery = match &s.steps[c.step].norm.action {
                CanonicalAction::Exec { argv, .. } => outcome::verification_is_discovery(argv),
                _ => false,
            };
            if c.outcome == VerificationOutcome::NoTests && !deliberate_discovery {
                add(
                    ProcedureKind::TestDiscovery,
                    "runner reported no executed tests",
                    vec![c.clone()],
                    "verification not established",
                );
            }
            // A narrower/different invocation after a failure is evidence of a scoped recheck,
            // never proof that the earlier failed scope was fixed. No textual scope guessing.
            if c.outcome == VerificationOutcome::Failed {
                if let Some(next) = checks[at + 1..]
                    .iter()
                    .take(8)
                    .find(|n| n.outcome == VerificationOutcome::Passed && same_task(s, c, n))
                {
                    if !same_scope(s, c, next) && same_runner(s, c, next) {
                        add(
                            ProcedureKind::CheckScope,
                            "failed check followed by a passing differently scoped check",
                            vec![c.clone(), next.clone()],
                            "earlier scope is not resolved by this differently scoped check",
                        );
                    }
                }
            }
        }
    }
    for ((kind, key), mut episodes) in found {
        episodes
            .sort_by(|a, b| (&a.checks[0].session, a.checks[0].step).cmp(&(&b.checks[0].session, b.checks[0].step)));
        episodes.dedup();
        let groups: BTreeSet<_> = episodes.iter().filter_map(|e| e.group.as_ref()).collect();
        let ss: BTreeSet<_> = episodes.iter().flat_map(|e| e.checks.iter().map(|c| &c.session)).collect();
        let (evidence_groups, sessions) = (groups.len(), ss.len());
        let omitted = episodes.len().saturating_sub(MAX_EXAMPLES);
        episodes.truncate(MAX_EXAMPLES);
        // Stable over input order and more observations; this is not a command-derived route id.
        let id = format!("{:x}", Sha256::digest(format!("verification-procedure-v1:{kind:?}:{key}").as_bytes()));
        out.proposals.push(Proposal {
            id: format!("procedure-{}", &id[..16]),
            kind,
            name: kind.name().into(),
            command_key: key,
            sessions,
            evidence_groups,
            episodes,
            episodes_omitted: omitted,
            status: "needs_evidence_review".into(),
            requirements: requirements(kind),
            template: kind.template().map(str::to_string),
        });
    }
    out.proposals.truncate(MAX_PROPOSALS);
    out
}

/// Unlike the report-only method leads, recovery must see every intervening call. Looking only
/// at the list of checks would silently bridge an edit, opaque script or changed task scope.
fn unexplained_recoveries(s: &Session) -> Vec<Episode> {
    use adage_tracks::recovery::RecoveryState;

    // Older corpora can retain only the session-level exposure flag, without a located span.
    if s.influence.is_empty() && (s.steered || !s.directed.is_empty()) {
        return Vec::new();
    }
    let mut calls = BTreeMap::new();
    let mut conflicting = BTreeSet::new();
    let mut turns: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    for (i, st) in s.steps.iter().enumerate() {
        let identity = st.call.tool_use_id.clone().unwrap_or_else(|| format!("missing:{i}"));
        turns.entry(st.turn).or_default().insert(identity);
        if let Some(id) = st.call.tool_use_id.as_deref() {
            if let Some(&first) = calls.get(id) {
                let earlier: &StepObs = &s.steps[first];
                if earlier.call != st.call || earlier.turn != st.turn || earlier.prompt != st.prompt {
                    conflicting.insert(id);
                }
            } else {
                calls.insert(id, i);
            }
        }
    }
    let mut state = RecoveryState::default();
    let mut previous_prompt = None;
    let mut observed = 0;
    let mut episodes = Vec::new();
    for (i, st) in s.steps.iter().enumerate() {
        let Some(id) = st.call.tool_use_id.as_deref() else {
            state.clear();
            continue;
        };
        if calls.get(id) != Some(&i) {
            continue;
        }
        observed += 1;
        if observed > MAX_RECOVERY_CALLS {
            break;
        }
        if previous_prompt.is_some_and(|p| p != st.prompt) {
            state.clear();
        }
        previous_prompt = Some(st.prompt);
        // Several calls issued in one model turn may execute concurrently. Their serialized
        // order is not a failure-before-pass relation, even when each has a separate result.
        let ambiguous_turn = turns.get(&st.turn).is_some_and(|ids| ids.len() != 1);
        let directed = s
            .influence
            .iter()
            .any(|d| d.at_step as usize <= i && (d.all || !d.commands.is_empty() || !d.avoided.is_empty()));
        if conflicting.contains(id) || ambiguous_turn || directed {
            state.clear();
            continue;
        }
        let Some(recovery) = state.observe(
            &st.call,
            &s.root,
            adage_core::CommandTable::load(),
            adage_tracks::params::RuntimeParams::default().history,
        ) else {
            continue;
        };
        let Some(&failed_at) = calls.get(recovery.failed_call.as_str()) else { continue };
        let Some(&passed_at) = calls.get(recovery.passed_call.as_str()) else { continue };
        let (Some(failed), Some(passed)) =
            (check(s, failed_at, &s.steps[failed_at]), check(s, passed_at, &s.steps[passed_at]))
        else {
            continue;
        };
        // The shared recognizer and canonical projection must agree; no independently parsed
        // raw report may be borrowed by a different normalized step.
        if failed.outcome != VerificationOutcome::Failed
            || passed.outcome != VerificationOutcome::Passed
            || !same_scope(s, &failed, &passed)
        {
            continue;
        }
        episodes.push(episode(
            s,
            ProcedureKind::UnexplainedRecovery,
            "same recorded test invocation failed, then passed with no observed intervening invalidating action",
            vec![failed, passed],
            "source and environment equality are unverified; the passing retry does not establish a repair or task success",
        ));
    }
    episodes
}

fn same_task(s: &Session, a: &Check, b: &Check) -> bool {
    s.steps[a.step].prompt == s.steps[b.step].prompt && b.turn > a.turn
}

fn same_scope(s: &Session, a: &Check, b: &Check) -> bool {
    same_task(s, a, b) && s.steps[a.step].norm.action == s.steps[b.step].norm.action
}

fn same_runner(s: &Session, a: &Check, b: &Check) -> bool {
    let family = |st: &StepObs| match &st.norm.action {
        CanonicalAction::Exec { argv, .. } => {
            let r = adage_core::action::classify_argv(argv).runner;
            (!r.is_empty()).then_some(r)
        }
        _ => None,
    };
    family(&s.steps[a.step]).is_some_and(|r| Some(r) == family(&s.steps[b.step]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_ingest::format::Decode;
    use serde_json::json;

    fn worker(id: &str, calls: &[(&str, &str, &str)]) -> Session {
        let mut records = vec![json!({"type":"user","sessionId":"root","agentId":id,
            "isSidechain":true,"cwd":"/repo","message":{"content":"Audit the parser"}})];
        for (i, (command, description, output)) in calls.iter().enumerate() {
            records.push(json!({"type":"assistant","message":{"id":format!("m{i}"),"content":[
                {"type":"tool_use","id":format!("c{i}"),"name":"Bash","input":{"command":command,"description":description}}]}}));
            records.push(json!({"type":"user","message":{"content":[
                {"type":"tool_result","tool_use_id":format!("c{i}"),"content":output,"is_error":false}]}}));
        }
        let text = records.iter().map(serde_json::Value::to_string).collect::<Vec<_>>().join("\n");
        let cx = Decode {
            table: adage_core::CommandTable::load(),
            locator: &adage_ingest::locate::AsRecorded,
            repo_override: Some("r"),
            subagent: true,
        };
        let mut s = adage_ingest::claude::decode(text.as_bytes(), &cx).unwrap();
        s.identity.group_id = Some("parent-family".into());
        s
    }

    #[test]
    fn a_surviving_claimed_mutation_is_a_proposal_not_positive_task_evidence() {
        let s = worker(
            "a",
            &[
                ("pytest tests/test_parser.py", "Baseline", "27 passed in 0.2s"),
                ("pytest tests/test_parser.py", "Run mutated implementation", "27 passed in 0.2s"),
            ],
        );
        let learned = learn(&[&s]);
        let p = learned.proposals.iter().find(|p| p.kind == ProcedureKind::NegativeControl).unwrap();
        assert_eq!(p.status, "needs_evidence_review");
        assert_eq!(p.evidence_groups, 1);
        assert_eq!(p.episodes[0].checks.len(), 2, "same-scope baseline retained");
        assert_eq!(p.episodes[0].checks[1].reported.tests_run, Some(27));
        assert_eq!(p.episodes[0].checks[1].call_id.as_deref(), Some("c1"));
        assert!(p.episodes[0].observation.contains("claimed mutation survived"));
        assert_eq!(s.outcome, None);
    }

    #[test]
    fn clean_state_failure_keeps_the_warm_retry_but_not_a_claim_of_repair() {
        let s = worker(
            "a",
            &[
                ("cargo check", "Check clean build state", "error[E0432]: unresolved import dependency"),
                ("cargo check", "Retry after generated output", "Finished dev profile"),
            ],
        );
        let learned = learn(&[&s]);
        let p = learned.proposals.iter().find(|p| p.kind == ProcedureKind::CleanState).unwrap();
        assert_eq!(p.episodes[0].checks.len(), 2);
        assert!(p.episodes[0].closure.contains("requires"));
        assert!(learned.transport_disagreements > 0);
    }

    #[test]
    fn descriptions_and_printed_fixture_text_cannot_be_runner_evidence() {
        let s = worker(
            "a",
            &[
                ("cat test.log", "Run mutated implementation", "Tests 27 passed (27)"),
                ("printf '27 passed in 0.2s'", "Check clean build state", "27 passed in 0.2s"),
            ],
        );
        assert!(learn(&[&s]).proposals.is_empty());
        let ordinary = worker("a", &[("pytest", "Run tests", "27 passed in 0.2s")]);
        assert!(learn(&[&ordinary]).proposals.is_empty(), "no generic passing-test reminder");
    }

    #[test]
    fn setup_failure_is_not_a_caught_mutation_and_subagents_are_not_independent() {
        let a = worker("a", &[("pytest", "Run mutation check", "ImportError while loading conftest.py")]);
        let b = worker("b", &[("pytest", "Run mutation check", "ImportError while loading conftest.py")]);
        let forward = learn(&[&a, &b]);
        let backward = learn(&[&b, &a]);
        assert_eq!(forward, backward);
        assert_eq!(forward.proposals[0].sessions, 2);
        assert_eq!(forward.proposals[0].evidence_groups, 1);
        assert!(!forward.proposals[0].episodes[0].observation.contains("produced test failures"));
    }

    #[test]
    fn a_narrow_pass_does_not_resolve_an_earlier_failed_scope() {
        let s = worker(
            "a",
            &[
                ("pytest", "Full suite", "1 failed, 20 passed in 0.3s"),
                ("pytest tests/test_parser.py", "Focused recheck", "3 passed in 0.1s"),
            ],
        );
        let p = learn(&[&s]).proposals;
        assert!(p.iter().any(|p| p.kind == ProcedureKind::CheckScope
            && p.episodes[0].closure == "earlier scope is not resolved by this differently scoped check"));
    }

    #[test]
    fn direct_sibling_tasks_share_support_without_overriding_declared_identity() {
        let calls = [("pytest", "Run mutation check", "ImportError while loading conftest.py")];
        let mut a = worker("a", &calls);
        let mut b = worker("b", &calls);
        a.identity = Default::default();
        b.identity = Default::default();
        a.prompt = "Audit parser syntax".into();
        b.prompt = "Check formatter mutation".into();
        assert_eq!(a.delegation.as_ref().unwrap().parent_id, b.delegation.as_ref().unwrap().parent_id);
        let learned = learn(&[&a, &b]);
        assert_eq!(learned.proposals[0].sessions, 2);
        assert_eq!(learned.proposals[0].evidence_groups, 1);
        assert_eq!(learned, learn(&[&b, &a]));

        let parent_group = procedure_group(&a);
        b.repo = "another-repository".into();
        assert_ne!(parent_group, procedure_group(&b));
        b.repo = a.repo.clone();
        b.harness = "codex".into();
        assert_ne!(parent_group, procedure_group(&b));
        b.harness = a.harness.clone();
        b.identity.task_id = Some("declared-task".into());
        assert_eq!(procedure_group(&b), b.evidence_group().map(|g| g.key));
        assert_ne!(parent_group, procedure_group(&b));
        b.identity.group_id = Some("declared-group".into());
        assert_eq!(procedure_group(&b), b.evidence_group().map(|g| g.key));
    }

    #[test]
    fn a_directed_method_does_not_become_organic_learning_evidence() {
        let mut s = worker("a", &[("pytest", "Run mutation check", "27 passed in 0.2s")]);
        s.influence.push(adage_trace::Directed { at_step: 0, all: true, ..Default::default() });
        assert!(learn(&[&s]).proposals.is_empty());
    }

    #[test]
    fn quoted_methods_discovery_and_filtered_passes_do_not_overclaim() {
        let ordinary = worker(
            "a",
            &[
                ("pytest", "Test GraphQL mutation", "27 passed in 0.2s"),
                ("pytest --collect-only", "Find tests", "no tests collected in 0.1s"),
            ],
        );
        assert!(learn(&[&ordinary]).proposals.is_empty());
        let filtered = worker("b", &[("pytest 2>&1 | tail -n 10", "Run mutated implementation", "27 passed in 0.2s")]);
        let p = learn(&[&filtered]).proposals;
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].episodes[0].checks[0].outcome, VerificationOutcome::Unknown);
        assert!(p[0].episodes[0].observation.contains("unverified"));
        assert!(!p[0].episodes[0].observation.contains("survived"));
    }
}

#[cfg(test)]
#[path = "procedures_recovery_tests.rs"]
mod recovery_tests;

#[cfg(test)]
#[path = "procedures_persistence_tests.rs"]
mod persistence_tests;
