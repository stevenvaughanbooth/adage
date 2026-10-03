//! The extension contract (Track C §3.2): what an [`Extension`] (adage-exec) is shown at each
//! event and what it may add. Every type here is re-exported at the crate root, where adage-exec
//! names it.

use crate::harness::Harness;
use adage_runtime::{Arm, Directive, Event, Obligation};
use adage_store::Paths;
use adage_tracks::Index;
use serde_json::Value;
use std::path::Path;
use std::time::Instant;

/// What one event is about, for an [`Extension`].
pub struct Turn<'a> {
    pub harness: Harness,
    /// The native event name.
    pub event: &'a str,
    /// Opaque native tool-call identity. None means retries cannot be identified reliably.
    pub event_identity: Option<String>,
    pub ev: &'a Event,
    pub session: &'a str,
    pub index: &'a Index,
    pub repo_key: &'a str,
    pub root: &'a Path,
    pub cwd: &'a Path,
    pub paths: &'a Paths,
    /// The harness's cap on directive text ([`crate::harness::Adapter::max_context`]).
    pub max_context: usize,
    /// The session's arm: nothing is ever run in the control (control) arm.
    pub arm: Option<Arm>,
    /// Finish-line obligations an approved E2 action could carry out at this event.
    pub stimuli: &'a [Stimulus],
}

/// A finish-line obligation this event armed (an edit matched its paths) or is enforcing (a Stop
/// request), for an E2 action (Track C §3.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stimulus {
    pub route: String,
    /// The command the obligation expects, bound.
    pub command: String,
    /// The paths glob that armed it.
    pub paths: String,
    /// The repository-relative path whose edit armed it (empty at Stop).
    pub edited: String,
    /// This event is the Stop request for it.
    pub at_stop: bool,
}

/// The obligations `decide()` armed or began enforcing at this event: `before` is each
/// obligation's `(route, stop_requested)` before it ran.
pub fn stimuli(before: &[(String, bool)], after: &[Obligation], is_batch: bool) -> Vec<Stimulus> {
    after
        .iter()
        .filter(|o| !o.met)
        .filter_map(|o| {
            let at_stop = o.stop_requested && !before.iter().any(|(r, s)| r == &o.route && *s);
            let armed = is_batch && o.fresh;
            (armed || at_stop).then(|| Stimulus {
                route: o.route.clone(),
                command: o.command.clone(),
                paths: o.paths.clone(),
                edited: if at_stop { String::new() } else { o.edited.clone() },
                at_stop,
            })
        })
        .collect()
}

/// What an extension adds to a decision's delivery.
#[derive(Debug, Clone, Default)]
pub struct Delivery {
    /// The text for each directive, in order, replacing [`adage_runtime::render::wrap_directive`]'s.
    /// `None`, or a list of the wrong length, delivers exactly the E0 text.
    pub texts: Option<Vec<String>>,
    /// Recorded under `exec` in this event's spool record.
    pub spool: Option<Value>,
    /// Blocks delivered after the directives (an action's result when no directive was due).
    pub extra: Vec<String>,
    /// Obligations an applied action met: marked met in the session state.
    pub met: Vec<String>,
    /// Contexts handled as CPU predictions, including abstentions: the agent was not instructed
    /// to perform them, so retire advisory follow judgements without marking obligations met.
    pub cpu_predictions: Vec<String>,
    /// The Stop request: `None` as decided; `Some(None)` withdrawn (an action met it);
    /// `Some(Some(reason))` with this reason instead.
    pub stop: Option<Option<String>>,
    /// Execution may have effects that require inspection. This warning takes precedence over
    /// ordinary directives and must survive context overflow; it never implies a safe retry.
    pub recovery: Option<String>,
    /// An approved action was selected for this Stop's obligation request (whatever its outcome):
    /// the Stop gate does not also run at this event, so one hook never stages two actions within
    /// its timeout.
    pub at_stop_action: bool,
    /// One line for the person: a grant no longer matches what would run, so its action was
    /// withheld (which field changed, and how to approve it again). Shown at most once per
    /// session; the agent never sees it.
    pub notice: Option<String>,
}

/// A step after `decide()` that may change what directives are delivered as (Track C §3.2).
pub trait Extension: Sync {
    fn deliver(&self, turn: &Turn<'_>, directives: &[Directive]) -> Delivery;
    /// Optional responsibility contract. No action runs here; the caller gates treatment.
    fn session_context(&self, _repo_key: &str, _root: &Path, _index: &Index, _paths: &Paths) -> Option<String> {
        None
    }
    /// The session start's texts: the contract ([`Extension::session_context`]) and one line for
    /// the person (a grant that no longer matches what would run). An extension that can find
    /// both in one pass overrides this.
    fn session_texts(&self, repo_key: &str, root: &Path, index: &Index, paths: &Paths) -> SessionTexts {
        SessionTexts { context: self.session_context(repo_key, root, index, paths), notice: None }
    }
    /// The exec-only reflexes (`(track, route id)`, vendored scripts) this extension would carry
    /// out in this repository, so `decide()` may direct them. Asked only when the index holds one.
    fn exec_only(&self, _repo_key: &str, _index: &Index, _paths: &Paths) -> Vec<(String, String)> {
        Vec::new()
    }
    /// The reflexes (`(track, route id)`) a person granted `predict` on this machine in this
    /// repository: what a `TurnStep` may make in the agent's place. Asked only at a `TurnStep`,
    /// with the checkout the step was asked in (`root`: a grant's scripts are hashed there).
    fn predict_grants(&self, _repo_key: &str, _root: &Path, _index: &Index, _paths: &Paths) -> Vec<(String, String)> {
        Vec::new()
    }
    /// A fact for a tool call about to run (`PreToolUse`): completed work it would repeat, checked
    /// now ([`PreNote`]). Context only, never a refusal: a command hook delivers its text beside
    /// any interpreter advice only when it is fresh; the mod may answer the call with its result.
    fn pretool_context(&self, _call: &PreCall<'_>) -> Option<PreNote> {
        None
    }
    /// The finish-line verification (Claude Code `Stop`): run an approved learned check whose
    /// result on the current inputs is not yet known, and say what happened.
    fn stop_gate(&self, _call: &StopCall<'_>) -> Option<StopGate> {
        None
    }
}

/// One extension call that is not an action's delivery (a receipt check, a Stop gate, the session
/// texts, the grants): a panic there loses only what it would have answered (`T::default()`), and
/// the hook carries on. A delivery is wrapped by `respond::extension_delivery` instead, since a
/// panic there cannot prove no action ran.
pub(crate) fn guarded<T: Default>(f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_default()
}

/// What [`Extension::pretool_context`] found for a Bash call about to run: a receipt of a run this
/// session made that covers it (plan §2.2 item 5; the Claude Code mod's architecture §3.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreNote {
    /// The receipt block for the model (`<adage-receipt …>`): delivered beside the call, and only
    /// when the receipt is `fresh`.
    pub text: String,
    /// A completed, applied run on inputs byte-identical to the current ones, which ran every test
    /// the call selects, checked for a call made at the repository root: the call's result is
    /// already known. A receipt that covers the call but is not fresh (a file changed since, or
    /// the call is made from elsewhere) is still reported, for the mod to say so beside the run.
    pub fresh: bool,
    /// The receipt as the tool's output would read (which run, when, and the tests it ran that the
    /// call selects): what the mod answers the call with. Only when `fresh`.
    pub result: Option<String>,
    /// The command that ran: a covering run can be broader than the call.
    pub covering: String,
    /// What the mod's receipt card shows for a call this receipt answers. Only when `fresh`.
    pub card: Option<ReceiptCard>,
    /// The run's operation id (sanitised, at most 12 characters), fresh or not: the mod names the
    /// receipt by it beside a run that happened anyway. Empty when the run has none.
    pub operation: String,
}

/// The receipt card's data for a call answered by a receipt (the Claude Code mod's `answered`
/// mark: the run that answered it, when, and the tests it ran that the call selects).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ReceiptCard {
    /// The run's operation id, as the receipt block names it (its first 12 characters).
    pub operation: String,
    /// When the covering run was recorded (RFC 3339, UTC).
    pub ran_at: String,
    /// How long before the answer that was (`4 min ago`).
    pub ago: String,
    /// One line: what the run found.
    pub summary: String,
    /// The tests it ran that the call selects, the session's own first (at most 12).
    pub tests: Vec<String>,
    /// How many more it ran than `tests` lists.
    pub more: usize,
}

/// What [`Extension::session_texts`] says at a session start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionTexts {
    /// For the model.
    pub context: Option<String>,
    /// For the person only (Claude Code's `systemMessage`), at most once per session.
    pub notice: Option<String>,
}

/// A `Stop` event, as [`Extension::stop_gate`] sees it.
pub struct StopCall<'a> {
    pub harness: Harness,
    pub repo_key: &'a str,
    pub root: &'a Path,
    pub session: &'a str,
    pub paths: &'a Paths,
    pub index: &'a Index,
    pub stop_hook_active: bool,
    /// When the gate must be finished, applying included: the Stop hook's registered timeout,
    /// less what this event already spent and a reserve for what follows. A gate that cannot
    /// finish by then does not start.
    pub deadline: Instant,
}

/// What a Stop gate decided: continue the agent with this reason (a failure it must see), or let
/// the turn end with a line for the person. A `Block` reason is a Stop continuation like any
/// other Adage writes ([`adage_runtime::render::finish_line`]): the hook and the learners
/// recognise it as Adage's own text.
#[derive(Debug, Clone, PartialEq)]
pub enum StopGate {
    Block(String),
    Notice(String),
}

/// A Bash call about to run, as [`Extension::pretool_context`] sees it.
pub struct PreCall<'a> {
    pub repo_key: &'a str,
    pub root: &'a Path,
    /// The session's working directory at this call (the payload's `cwd`): a relative command
    /// runs from here, which need not be the repository root.
    pub cwd: &'a Path,
    pub session: &'a str,
    pub paths: &'a Paths,
    pub index: &'a Index,
    pub command: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ob(route: &str, met: bool, stop_requested: bool, fresh: bool) -> Obligation {
        Obligation {
            route: route.into(),
            command: "make gen".into(),
            paths: "schema/*.json".into(),
            edited: "schema/users.json".into(),
            met,
            stop_requested,
            fresh,
            ..Default::default()
        }
    }

    #[test]
    fn actions_follow_fresh_edits_and_new_stop_requests_only() {
        let after = [ob("fresh", false, false, true), ob("stale", false, false, false), ob("met", true, false, true)];
        let s = stimuli(&[], &after, true);
        assert_eq!(s.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["fresh"]);
        assert_eq!(s[0].edited, "schema/users.json");
        assert!(stimuli(&[], &after, false).is_empty(), "only a tool batch arms an action");
        // At Stop: the obligation whose request this event makes, not one requested before.
        let after = [ob("now", false, true, false), ob("earlier", false, true, false)];
        let before = [("now".to_string(), false), ("earlier".to_string(), true)];
        let s = stimuli(&before, &after, false);
        assert_eq!(s.len(), 1);
        assert!(s[0].at_stop && s[0].route == "now" && s[0].edited.is_empty());
    }
}
