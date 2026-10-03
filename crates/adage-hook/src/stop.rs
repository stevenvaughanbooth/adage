//! The finish-line gate at a Claude Code `Stop` (adage-exec's [`Extension::stop_gate`]): its
//! deadline within the Stop hook's timeout, its `stop_gate` record, and how its result joins the
//! decision's output for the same Stop. Entry: [`gated`], from `handle`.

use crate::ctx::HookCtx;
use crate::extension::guarded;
use crate::failopen::{self, Failure};
use crate::harness::{self, Output};
use crate::record::{Record, RecordKind};
use crate::respond::Responded;
use crate::{site, Extension, StopCall, StopGate};
use adage_runtime::render;
use std::time::{Duration, Instant};

/// Kept back from the Stop hook's registered timeout for what follows the gate (the spool
/// record, the learner request, the output).
const STOP_RESERVE: Duration = Duration::from_secs(2);

/// When the Stop gate must be done, applying included: the registered timeout of a Stop hook
/// that runs actions ([`harness::ACTION_HOOK_TIMEOUT_S`]) from the hook's start, less
/// [`STOP_RESERVE`]. Whatever the event already spent is deducted.
fn stop_deadline(started: Instant) -> Instant {
    started + Duration::from_secs(u64::from(harness::ACTION_HOOK_TIMEOUT_S)) - STOP_RESERVE
}

/// A gate result alone.
fn gate_output(g: StopGate) -> Output {
    match g {
        StopGate::Block(reason) => Output::Continue(reason),
        StopGate::Notice(n) => Output::Notice(n),
    }
}

/// The gate's result with the decision's output for the same Stop. A failing gate's
/// continuation and a learned obligation's request (or an action's) both reach the agent, in one
/// continuation; a passing gate's line for the person yields to anything for the agent.
fn with_gate(g: StopGate, decided: Option<Output>) -> Output {
    match (g, decided) {
        (StopGate::Block(reason), Some(Output::Continue(other))) => {
            Output::Continue(render::join_finish_lines(&reason, &other))
        }
        // A Stop carries no context: anything else decided here has no channel at a Stop.
        (StopGate::Block(reason), _) => Output::Continue(reason),
        (StopGate::Notice(_), Some(o)) => o,
        (StopGate::Notice(n), None) => Output::Notice(n),
    }
}

/// A `Stop` under an extension, in a repository, once the decision is made (`responded`): the
/// finish-line gate ([`Extension::stop_gate`]) runs unless an approved action already ran for this
/// Stop's obligation; its run is spooled as a `stop_gate` record, and its result joins the
/// decision's output ([`with_gate`]). Decide first, always: the session's bookkeeping must not
/// depend on what the gate finds (review F1).
pub(crate) fn gated(
    ctx: &HookCtx<'_>,
    x: &dyn Extension,
    stop_hook_active: bool,
    responded: Result<Responded, Failure>,
) -> Result<Option<String>, Failure> {
    let HookCtx { adapter, harness, event, session, root, paths, started, .. } = *ctx;
    let index = ctx.index();
    // One hook never stages two actions: when an approved action already ran for this
    // Stop's obligation, the gate waits for the next Stop (review F4).
    let acted = responded.as_ref().is_ok_and(|r| r.at_stop_action);
    let gate = if acted {
        None
    } else {
        site::routed_key(index, root, paths, false).and_then(|repo_key| {
            let call = StopCall {
                harness: adapter.harness(),
                repo_key: &repo_key,
                root,
                session,
                paths,
                index,
                stop_hook_active,
                deadline: stop_deadline(started),
            };
            guarded(|| x.stop_gate(&call))
        })
    };
    if let Some(g) = &gate {
        Record::new(RecordKind::StopGate, event)
            .with("blocked", matches!(g, StopGate::Block(_)))
            .timed(started)
            .append_or_log(paths, session, harness, event);
    }
    match (gate, responded) {
        (None, r) => r.map(|r| r.out),
        (Some(g), Err(f)) => {
            // The decision failed (and ran nothing); the gate's result still stands.
            failopen::record(&paths.root, harness, event, &f);
            Ok(Some(gate_output(g)))
        }
        (Some(g), Ok(r)) => Ok(Some(with_gate(g, r.out))),
    }
    .map(|o| o.and_then(|o| adapter.render(event, o)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use crate::{Delivery, Turn};
    use adage_runtime::Directive;
    /// F1: a Stop-gate block no longer hides a learned finish-line obligation. The gate fails at
    /// the first Stop and the obligation is due there too: the agent gets both in one
    /// continuation. At the next Stop the gate passes, and the obligation has been requested.
    #[test]
    fn a_failing_stop_gate_keeps_the_learned_obligation_request() {
        let w = ObligationWorld::new();
        let failed = render::stop_gate_failed(".venv/bin/python -m pytest tests -q", Some(1), None);
        let ext = scripted(vec![
            StopGate::Block(failed.clone()),
            StopGate::Notice(
                "adage: ran `.venv/bin/python -m pytest tests -q` at finish: passed (exit 0, 1.0 s).".into(),
            ),
        ]);
        let directive = w.edit(ext).expect("the edit arms the obligation");
        assert!(directive["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("make gen"));
        let first = w.stop(ext, false).expect("the gate failed: the agent continues");
        assert_eq!(first["decision"], "block");
        let reason = first["reason"].as_str().unwrap();
        assert!(reason.contains("it failed (exit 1)"), "the gate's failure: {reason}");
        assert!(reason.contains("running `make gen`"), "the obligation's request is not lost: {reason}");
        assert!(harness::is_own_text(reason), "{reason}");
        let st = w.state();
        assert!(st.obligations[0].stop_requested && st.stop_requests == 1, "{:?}", st.obligations);
        // The agent fixes the failure and finishes again: the gate passes; nothing is lost.
        let second = w.stop(ext, true).expect("the person's line");
        assert!(second["systemMessage"].as_str().unwrap().contains("passed"), "{second}");
        assert_eq!(ext.asked.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(w.state().stop_requests, 1, "requested once");
    }

    /// F4: when an approved action already ran for this Stop's obligation request, the gate does
    /// not also run in the same hook (two staged runs could outlast the Stop hook's timeout); it
    /// runs at the next Stop. The gate's deadline is the registered timeout from the hook's start,
    /// less a reserve, so what the event already spent is deducted.
    #[test]
    fn an_action_at_stop_leaves_the_gate_for_the_next_stop() {
        struct ActsAtStop {
            asked: std::sync::atomic::AtomicUsize,
            deadlines: std::sync::Mutex<Vec<Instant>>,
        }
        impl Extension for ActsAtStop {
            fn deliver(&self, turn: &Turn<'_>, _: &[Directive]) -> Delivery {
                let at_stop: Vec<String> = turn.stimuli.iter().filter(|s| s.at_stop).map(|s| s.route.clone()).collect();
                // As adage-exec does for an applied at-Stop action: met, and the request withdrawn.
                Delivery {
                    at_stop_action: !at_stop.is_empty(),
                    stop: (!at_stop.is_empty()).then_some(None),
                    met: at_stop,
                    ..Delivery::default()
                }
            }
            fn stop_gate(&self, call: &StopCall<'_>) -> Option<StopGate> {
                self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.deadlines.lock().unwrap().push(call.deadline);
                Some(StopGate::Notice("adage: ran `make test` at finish: passed (exit 0, 0.1 s).".into()))
            }
        }
        let ext: &'static ActsAtStop =
            Box::leak(Box::new(ActsAtStop { asked: Default::default(), deadlines: Default::default() }));
        let w = ObligationWorld::new();
        w.edit(ext).expect("the obligation is announced");
        assert!(w.stop(ext, false).is_none(), "the action met the obligation: the turn ends");
        assert_eq!(ext.asked.load(std::sync::atomic::Ordering::SeqCst), 0, "no gate beside an action");
        assert!(w.state().obligations[0].met);
        let before = Instant::now();
        let next = w.stop(ext, false).expect("the gate runs at the next Stop");
        assert!(next["systemMessage"].as_str().unwrap().contains("passed"), "{next}");
        assert_eq!(ext.asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        let deadline = ext.deadlines.lock().unwrap()[0];
        let timeout = Duration::from_secs(u64::from(harness::ACTION_HOOK_TIMEOUT_S));
        assert!(deadline <= before + timeout - STOP_RESERVE + Duration::from_secs(1), "within the timeout");
        // Time the event already spent is deducted.
        let started = Instant::now() - Duration::from_secs(10);
        assert_eq!(stop_deadline(started), started + timeout - STOP_RESERVE);
    }
}
