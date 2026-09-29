//! The no-match fast path (Design §13.2): events that cannot change any decision, told apart
//! before the hook loads the command table, parses a command, takes the state lock or writes the
//! spool.
//!
//! [`skip`] says an event can be skipped only when running [`crate::decide`] on it would give no
//! directive, no judgement and no stop request, and would change nothing in the session state that
//! a later decision reads (`emitted`, `pending`, `obligations`, `stop_requests`, `prompts`
//! while it is 0, and the context state `ctx`). What a skipped event leaves out is measurement
//! only: the `batches` and `history` counters, and its spool record, so `adage graph` lists
//! commands only from recorded
//! events. Runs of a dead end's avoided commands are always recorded (they are words in the
//! triggers), as are all events of a session with a pending judgement or an obligation.
//!
//! By event:
//!
//! | Event | Skipped when |
//! | --- | --- |
//! | session start, session end | always: nothing decides on them |
//! | prompt | no intent routes; no prompt route's span kind in the text; no prompt token a prompt route's learned value type accepts; not the session's first prompt (`prompts` > 0) |
//! | batch | no pending judgement, no obligation and no procedure card waiting for an edit (`card_waiting`) in the state; no call might match ([`Triggers::call_might_match`]) |
//! | failure | no call might match, and no pending judgement (a failure of a directed command judges it `misfired`) |
//! | stop | no obligation both unmet and not yet requested |
//!
//! **Context triggers** (Track G §7.3) match a subsequence over *alphabet* events only: an event
//! that satisfies no context item pushes nothing and changes no match. Every item's words, globs,
//! value types and intents are in the triggers ([`Triggers::of`]), so an event that might be an
//! alphabet event is never skipped, and skipping the rest is exactly equivalent.
//!
//! The eval replay checks this on every event of every dataset: a skipped event's full decision
//! must be empty, and deciding with the fast path must give the same decisions as without it
//! (`adage_eval::replay`, the `fast_path_changes_no_decision` gate).

use crate::{Event, SessionState};
use adage_core::spans;
use adage_tracks::triggers::Triggers;

/// Whether this event can be skipped. `cwd` is the event's working directory; `state` reads the
/// session's state, and is called only when the answer depends on it (a `None` from it, for a
/// state that exists but cannot be read, means "do not skip").
pub fn skip(t: &Triggers, ev: &Event, cwd: &str, state: impl FnOnce() -> Option<SessionState>) -> bool {
    match ev {
        Event::SessionStart | Event::SessionEnd => true,
        Event::Prompt { text } => {
            if t.intent {
                return false;
            }
            if !t.prompt_spans.is_empty() && spans::extract(text).iter().any(|s| t.prompt_spans.contains(&s.kind)) {
                return false;
            }
            if t.prompt_might_bind(text) {
                return false;
            }
            // The first prompt of a session is always decided in full, even with no dead end to
            // announce: it moves `prompts` off 0, which a later decision reads if a dead end is
            // learned mid-session. One full decision per session costs little.
            state().is_some_and(|st| st.prompts > 0)
        }
        Event::Batch { calls } => {
            if t.calls_matter() && calls.iter().any(|c| t.call_might_match(&haystack(cwd, &c.tool, &c.input))) {
                return false;
            }
            // A procedure card waiting for the session's first edit of one of its files (a prompt
            // just below its threshold) is said at an edit no trigger word names.
            state().is_some_and(|st| st.pending.is_empty() && st.obligations.is_empty() && st.card_waiting.is_none())
        }
        // A failure can also judge a pending directive (followed, and it failed).
        Event::Failure { call } => {
            (!t.calls_matter() || !t.call_might_match(&haystack(cwd, &call.tool, &call.input)))
                && state().is_some_and(|st| st.pending.is_empty())
        }
        Event::Stop { .. } => state().is_some_and(|st| !st.obligations.iter().any(|o| !o.met && !o.stop_requested)),
    }
}

/// What a call's words are looked for in: the working directory (a relative path in the call
/// resolves against it), the tool's name, and its input as JSON.
fn haystack(cwd: &str, tool: &str, input: &serde_json::Value) -> String {
    format!("{cwd}\n{tool}\n{input}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Call, Obligation, Pending};
    use adage_core::spans::SpanKind;
    use serde_json::json;

    fn ls() -> Event {
        Event::Batch {
            calls: vec![Call {
                tool: "Bash".into(),
                input: json!({"command": "ls"}),
                output: String::new(),
                failed: false,
            }],
        }
    }

    #[test]
    fn what_the_state_decides() {
        let t = Triggers { first_prompt: true, words: vec!["wi-lookup".into()], ..Default::default() };
        let fresh = || Some(SessionState::default());
        let prompted = || Some(SessionState { prompts: 1, ..Default::default() });
        let prompt = Event::Prompt { text: "hello".into() };
        assert!(!skip(&t, &prompt, "/r", fresh), "the dead end is still to announce");
        assert!(skip(&t, &prompt, "/r", prompted));
        assert!(skip(&t, &ls(), "/r", fresh));
        let pending = || Some(SessionState { pending: vec![Pending::default()], ..Default::default() });
        assert!(!skip(&t, &ls(), "/r", pending), "a follow judgement may be due");
        let met = || {
            Some(SessionState {
                obligations: vec![Obligation { met: true, ..Default::default() }],
                ..Default::default()
            })
        };
        assert!(!skip(&t, &ls(), "/r", met), "an edit may re-arm it");
        let waiting = || Some(SessionState { card_waiting: Some("card-1".into()), ..Default::default() });
        assert!(!skip(&t, &ls(), "/r", waiting), "a card may be due at this edit");
        assert!(skip(&t, &Event::Stop { stop_hook_active: false }, "/r", met));
        let unmet = || Some(SessionState { obligations: vec![Obligation::default()], ..Default::default() });
        assert!(!skip(&t, &Event::Stop { stop_hook_active: false }, "/r", unmet));
        // A state that exists but cannot be read is never skipped past: it will be rebuilt.
        assert!(!skip(&t, &ls(), "/r", || None));
        assert!(!skip(&t, &prompt, "/r", || None));
        assert!(skip(&t, &Event::SessionStart, "/r", || None));
    }

    #[test]
    fn what_the_event_decides() {
        let never = || -> Option<SessionState> { panic!("the state is not needed") };
        let t = Triggers { prompt_spans: vec![SpanKind::Ticket], ..Default::default() };
        assert!(!skip(&t, &Event::Prompt { text: "Pick up W-2042".into() }, "/r", never));
        let prompted = || Some(SessionState { prompts: 1, ..Default::default() });
        assert!(skip(&t, &Event::Prompt { text: "Tidy the README".into() }, "/r", prompted));
        // The first prompt is always decided in full, dead end or not: it moves `prompts` off 0.
        let fresh = || Some(SessionState::default());
        assert!(!skip(&t, &Event::Prompt { text: "Tidy the README".into() }, "/r", fresh));
        let t = Triggers { intent: true, ..Default::default() };
        assert!(!skip(&t, &Event::Prompt { text: "anything".into() }, "/r", never));
        let t = Triggers { words: vec!["pytest".into()], ..Default::default() };
        let failed = |cmd: &str| Event::Failure {
            call: Call { tool: "Bash".into(), input: json!({ "command": cmd }), output: String::new(), failed: true },
        };
        assert!(!skip(&t, &failed("python -m pytest -q"), "/r", never));
        let fresh = || Some(SessionState::default());
        assert!(skip(&t, &failed("cargo build"), "/r", fresh));
        let pending = || Some(SessionState { pending: vec![Pending::default()], ..Default::default() });
        assert!(!skip(&t, &failed("cargo build"), "/r", pending), "a directed command may have failed");
        let calls_that_might = Event::Batch {
            calls: vec![Call {
                tool: "Bash".into(),
                input: json!({"command": "pytest"}),
                output: String::new(),
                failed: false,
            }],
        };
        assert!(!skip(&t, &calls_that_might, "/r", never));
    }
}
