//! How evidence is counted and judged (Design §11): learning thresholds ([`LearnConfig`]), the
//! strength rule (a Wilson lower bound for fire, a rate for prime, capped by the step's effect
//! class, with the outcome-lift path as a second gate), and the counting helpers every judgement
//! shares. Evidence is always counted in **distinct traces**.

use crate::import::{Session, StepObs};
use adage_core::{CanonicalAction, Effect};
use adage_tracks::lift::OutcomeLift;
use adage_tracks::params::LearnParams;
use adage_tracks::*;
use std::collections::{BTreeMap, BTreeSet};

/// One canonical record per repository/session id. Comparing complete records prevents an
/// arbitrary input order from choosing among conflicting outcomes, identities or tool histories.
pub(crate) fn unique_sessions(mut sessions: Vec<Session>) -> anyhow::Result<(Vec<Session>, usize)> {
    sessions.sort_by(|a, b| (&a.repo, &a.id).cmp(&(&b.repo, &b.id)));
    let mut unique: Vec<Session> = Vec::with_capacity(sessions.len());
    let mut duplicates = 0;
    for s in sessions {
        if let Some(previous) = unique.last().filter(|p| p.repo == s.repo && p.id == s.id) {
            anyhow::ensure!(
                previous == &s,
                "conflicting canonical records for repository {:?}, session {:?}",
                s.repo,
                s.id
            );
            duplicates += 1;
        } else {
            unique.push(s);
        }
    }
    Ok((unique, duplicates))
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    fn session(id: &str, repo: &str) -> Session {
        serde_json::from_value(serde_json::json!({
            "id":id, "root":"/fixture", "repo":repo, "model":null, "day":"2026-10-01",
            "prompt":"fix billing", "steps":[], "steered":false, "directed":[], "outcome":null,
            "harness":"codex"
        }))
        .unwrap()
    }

    #[test]
    fn duplicate_deliveries_do_not_multiply_evidence_and_repositories_stay_separate() {
        let (a, b) = (session("same-id", "a"), session("same-id", "b"));
        let (sessions, duplicates) = unique_sessions(vec![b.clone(), a.clone(), a.clone()]).unwrap();
        assert_eq!(sessions, vec![a.clone(), b.clone()]);
        assert_eq!(duplicates, 1);
        assert_eq!(unique_sessions(vec![a.clone(), b, a]).unwrap(), (sessions, duplicates));
    }

    #[test]
    fn conflicting_history_outcome_identity_or_harness_is_not_chosen_by_input_order() {
        let a = session("same-id", "repo");
        for field in ["prompt", "outcome", "identity", "harness"] {
            let mut b = a.clone();
            match field {
                "prompt" => b.prompt = "different work".into(),
                "outcome" => b.outcome = Some(false),
                "identity" => b.identity.task_id = Some("TASK-7".into()),
                _ => b.harness = "claude-code".into(),
            }
            for pair in [vec![a.clone(), b.clone()], vec![b, a.clone()]] {
                let err = unique_sessions(pair).unwrap_err().to_string();
                assert!(err.contains("conflicting canonical records"), "{field}: {err}");
            }
        }
    }
}

/// Learning thresholds. Build one from [`LearnParams`] with
/// [`LearnConfig::from_params`]; [`LearnConfig::default`] and [`LearnConfig::demo`] are the two
/// presets (`adage_tracks::params`).
#[derive(Debug, Clone)]
pub struct LearnConfig {
    /// Reuse pure mining checkpoints when the store supports them. Never skips current judgment.
    pub computation_cache: bool,
    /// Minimum distinct supporting sessions for any route.
    pub min_support: u32,
    /// Wilson lower bound required for fire.
    pub fire_lower_bound: f64,
    /// Minimum support rate for prime.
    pub prime_rate: f64,
    /// Lower bar that keeps an *existing* route (hysteresis): routes do not flap when their rate
    /// dips just below the bar that created them. Weakening on harm is separate.
    pub retain_rate: f64,
    /// Ids of routes that already exist; they are judged against `retain_rate`.
    pub existing: BTreeSet<String>,
    pub provenance: String,
    /// An avoid names a runner only if it failed in at least `num/den` of the traces that ran it.
    pub dead_end_consistency: (usize, usize),
    /// At most this many avoids (failure signatures) per working command.
    pub dead_end_signatures: usize,
    /// A step after an edit: its rate after the edit must exceed its rate without it by this much.
    pub finish_line_contrast: f64,
    /// Prompt-intent prototypes: the share of traces above which a command is ubiquitous.
    pub intent_ubiquitous: f64,
    /// Prompt-intent prototypes: the in-family minus out-of-family rate a command needs.
    pub intent_contrast: f64,
    /// Review: the lower-bound movement that counts as a material change.
    pub material_lower_bound: f64,
    /// Dormant routes: the exposed share that explains a route's absence ([`crate::dormant`]).
    pub dormant_exposure: f64,
    /// The outcome-lift path (`adage_tracks::lift`): the lower bound on the success difference a
    /// step needs, and the traces that must have taken it.
    pub outcome_lift_lower_bound: f64,
    pub outcome_min_support: u32,
    /// Bind instincts per repository after learning ([`crate::instincts`]); `None` binds none
    /// (the offline eval's default, so its numbers are those of reflexes alone).
    pub instincts: Option<crate::instincts::Config>,
    /// Cross-repository priors of the directive-value model ([`crate::value::Priors`]).
    pub priors: crate::value::Priors,
    /// Uplift gating's bar (`learn.uplift_min`, [`crate::value::assess`]); 0 turns it off.
    pub uplift_min: f64,
    /// Repository key → its root on disk, for the static scan the value model reads
    /// ([`crate::value::Statics`]); a repository not named here is read at its sessions' working
    /// directory (the instinct configuration's roots take precedence).
    pub roots: BTreeMap<String, std::path::PathBuf>,
    /// Learn procedure cards for families of similar tasks ([`crate::cards`]); `None` learns none
    /// (the offline eval's default, so its numbers are those of reflexes alone).
    pub cards: Option<crate::cards::CardParams>,
    /// Mine and keep candidates (plan D1, `adage_tracks::candidates`): sequences seen in fewer
    /// traces than a reflex needs, in a pass of their own that never changes the reflexes. On in
    /// both presets; off only to measure the reflexes' pass alone.
    pub candidates: bool,
    /// Where the run reports how far it has got ([`crate::progress`]); nowhere by default. It
    /// never changes what is learned.
    pub progress: crate::progress::Progress,
}

impl Default for LearnConfig {
    fn default() -> Self {
        LearnConfig::from_params(&LearnParams::default())
    }
}

impl LearnConfig {
    /// The bars a home conditions with (`learn.thresholds` in its config): the one mapping the
    /// learner, `adage sense` and the map all use.
    pub fn for_thresholds(t: adage_store::config::Thresholds) -> LearnConfig {
        match t {
            adage_store::config::Thresholds::Default => LearnConfig::default(),
            adage_store::config::Thresholds::Demo => LearnConfig::demo(),
        }
    }

    /// Uplift gating's bar for this run: `uplift_min`, except for traces made by agents that were
    /// given the steps (`adage train`'s exploration sessions, provenance `synthetic:…`), which say
    /// nothing of what agents do unaided (0: estimated and shown, never gated).
    pub fn uplift_bar(&self) -> f64 {
        if self.provenance.starts_with("synthetic") {
            0.0
        } else {
            self.uplift_min
        }
    }

    /// The rate bar for a route id: `retain_rate` if it already exists, else `prime_rate`.
    pub fn bar(&self, id: &str) -> f64 {
        if self.existing.contains(id) {
            self.retain_rate
        } else {
            self.prime_rate
        }
    }

    /// Thresholds for small demonstrations; the report always states which were used.
    pub fn demo() -> Self {
        LearnConfig::from_params(&LearnParams::demo())
    }

    /// The configuration for a set of learn parameters.
    pub fn from_params(p: &LearnParams) -> Self {
        LearnConfig {
            computation_cache: true,
            min_support: p.min_support,
            fire_lower_bound: p.fire_lower_bound,
            prime_rate: p.prime_rate,
            retain_rate: p.retain_rate,
            existing: BTreeSet::new(),
            provenance: "desire-path".into(),
            dead_end_consistency: (p.dead_end_consistency[0] as usize, p.dead_end_consistency[1] as usize),
            dead_end_signatures: p.dead_end_signatures,
            finish_line_contrast: p.finish_line_contrast,
            intent_ubiquitous: p.intent_ubiquitous,
            intent_contrast: p.intent_contrast,
            material_lower_bound: p.material_lower_bound,
            dormant_exposure: p.dormant_exposure,
            outcome_lift_lower_bound: p.outcome_lift_lower_bound,
            outcome_min_support: p.outcome_min_support.unwrap_or(p.min_support),
            instincts: None,
            priors: p.outcome_priors.as_ref().and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
            roots: BTreeMap::new(),
            uplift_min: p.uplift_min,
            cards: None,
            candidates: true,
            progress: Default::default(),
        }
    }
}

pub(crate) fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

pub(crate) fn strength_for(
    effect: Effect,
    by_edit: bool,
    lb: f64,
    rate: f64,
    cfg: &LearnConfig,
    id: &str,
) -> Option<Strength> {
    let max = ceiling(effect, by_edit)?;
    if lb >= cfg.fire_lower_bound && max == Strength::Fire {
        Some(Strength::Fire)
    } else if rate >= cfg.bar(id) {
        Some(Strength::Prime)
    } else {
        None
    }
}

/// [`strength_for`], with the outcome-lift path (`adage_tracks::lift`) as a second gate: a step
/// that fails the rate bar (or has too little support for it) still gets prime (prime) when the
/// traces that took it succeeded significantly more often than those that did not. Never fire:
/// fire reads the rate's lower bound alone.
#[allow(clippy::too_many_arguments)]
pub(crate) fn strength_with_lift(
    effect: Effect,
    by_edit: bool,
    support: u32,
    lb: f64,
    rate: f64,
    cfg: &LearnConfig,
    id: &str,
    lift: Option<&OutcomeLift>,
) -> Option<Strength> {
    if support >= cfg.min_support {
        if let Some(a) = strength_for(effect, by_edit, lb, rate, cfg, id) {
            return Some(a);
        }
    }
    lift_strength(effect, by_edit, cfg, lift)
}

/// The outcome-lift path alone: prime (prime) when the lift qualifies and the effect may be
/// directed.
pub(crate) fn lift_strength(
    effect: Effect,
    by_edit: bool,
    cfg: &LearnConfig,
    lift: Option<&OutcomeLift>,
) -> Option<Strength> {
    lift.filter(|l| l.qualifies(cfg.outcome_min_support, cfg.outcome_lift_lower_bound))
        .and_then(|_| adage_tracks::lift::strength(effect, by_edit))
}

/// The outcomes lift is judged on: the recorded ones (`--outcomes`, a dataset's `passed`) when
/// any of these sessions has one, else the transcript proxies ([`Session::proxy_outcome`]).
pub(crate) fn known_outcomes<'a>(sessions: &[&'a Session]) -> (BTreeMap<&'a str, bool>, &'static str) {
    let recorded: BTreeMap<&str, bool> =
        sessions.iter().filter_map(|s| s.outcome.map(|o| (s.id.as_str(), o))).collect();
    if !recorded.is_empty() {
        return (recorded, "outcomes");
    }
    (sessions.iter().filter_map(|s| s.proxy_outcome().map(|o| (s.id.as_str(), o))).collect(), "proxies")
}

/// The outcome lift of a step: among the sessions where it was open (`opportunity`), those that
/// took it (`took`) against those that did not, on known outcomes only.
pub(crate) fn outcome_lift(
    sessions: &[&Session],
    opportunity: &BTreeSet<String>,
    took: &BTreeSet<String>,
) -> Option<OutcomeLift> {
    let (known, source) = known_outcomes(sessions);
    let (mut with, mut without) = (Vec::new(), Vec::new());
    for id in opportunity {
        let Some(o) = known.get(id.as_str()) else { continue };
        if took.contains(id) {
            with.push(*o);
        } else {
            without.push(*o);
        }
    }
    OutcomeLift::of(&with, &without, source)
}

pub(crate) fn models(sessions: &[&Session]) -> Vec<String> {
    let s: BTreeSet<String> = sessions.iter().filter_map(|x| x.model.clone()).collect();
    s.into_iter().collect()
}

pub(crate) fn days(sessions: &[&Session], ids: &BTreeSet<String>) -> u32 {
    sessions.iter().filter(|s| ids.contains(&s.id)).map(|s| s.day.clone()).collect::<BTreeSet<_>>().len() as u32
}

pub(crate) fn argv(step: &StepObs) -> Option<&Vec<String>> {
    match &step.norm.action {
        CanonicalAction::Exec { argv, .. } => Some(argv),
        _ => None,
    }
}

pub use adage_graph::compile::stable::{runner, shell_join, stable};

/// The path pattern an edit to `rel` stands for: `schema/invoice.json` → `schema/*.json`.
pub fn glob_for(rel: &str) -> Option<String> {
    // A path outside the repository (a scratch file, say) is no pattern of the repository's.
    if !adage_core::repo::is_safe_relative(rel) {
        return None;
    }
    let (dir, file) = rel.rsplit_once('/').unwrap_or(("", rel));
    let ext = file.rsplit_once('.').map(|(_, e)| e)?;
    Some(if dir.is_empty() { format!("*.{ext}") } else { format!("{dir}/*.{ext}") })
}

/// Pseudo-field meaning "repository paths mentioned in the command's text output".
pub const PATHS_FIELD: &str = "*paths";

pub(crate) trait MaxRank {
    fn max_by_rank(self, other: Effect) -> Effect;
}

impl MaxRank for Effect {
    fn max_by_rank(self, other: Effect) -> Effect {
        let r = |e: Effect| match e {
            Effect::Observe => 0,
            Effect::Verify => 1,
            Effect::Regenerate => 2,
            Effect::Environment => 3,
            Effect::Author => 4,
            Effect::Publish => 5,
        };
        if r(other) > r(self) {
            other
        } else {
            self
        }
    }
}
