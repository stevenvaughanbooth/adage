//! Decide and deliver: every event that is not a press, a skill prompt, a `PreToolUse`, a
//! `Standing` or a session start (a tool batch, a failure, a prompt, a `Stop`, a `TurnStep`). The
//! no-routes path (a session seen in a repository with no reflexes), the fast path, the decision
//! under the session's state lock (a repository with only candidates still notices them), the
//! extension's delivery, the record, and the rendering's text. Entry: [`respond`], from `handle`.

use crate::extension::guarded;
use crate::failopen::{self, Failure};
use crate::harness::{self, Output};
use crate::session_start::record_seen;
use crate::{record, site, skill, stimuli, Delivery, Options, Turn};
use adage_core::{repo, CommandTable};
use adage_intent::model;
use adage_intent::normalize::normalize;
use adage_runtime::{decide, fast, rebuild, render, Arm, Ctx, Event, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::Index;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

/// Whether this session has not yet had a lapsed-grant notice; marks it as having had one.
pub(crate) fn first_lapse_notice(paths: &Paths, session: &str) -> bool {
    state::update::<SessionState, _>(&paths.state(session), |st| !std::mem::replace(&mut st.lapse_noticed, true))
        .unwrap_or(false)
}

/// What [`respond`] decided for one event, before rendering.
#[derive(Debug, Default)]
pub(crate) struct Responded {
    pub(crate) out: Option<Output>,
    /// [`Delivery::at_stop_action`].
    pub(crate) at_stop_action: bool,
}

const RECOVERY_NOTICE: &str = "Adage cannot confirm an execution outcome. Do not retry the action automatically. Inspect the worktree and execution journal, then ask the person to resolve recovery.";
const APPLIED_NOTICE: &str = "Adage applied an approved action. Its detailed result could not fit here; inspect the execution journal before rerunning the action.";

fn bounded_notice(text: String, fallback: &str, max: usize) -> Option<String> {
    if text.len() <= max {
        Some(text)
    } else if fallback.len() <= max {
        Some(fallback.into())
    } else {
        None
    }
}

fn extension_delivery(f: impl FnOnce() -> Delivery) -> Delivery {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Delivery { recovery: Some(RECOVERY_NOTICE.into()), ..Delivery::default() })
}

fn fitted_context(all: String, e0: impl FnOnce() -> String, applied: bool, max: usize) -> String {
    if all.len() <= max {
        all
    } else if applied {
        bounded_notice(APPLIED_NOTICE.into(), APPLIED_NOTICE, max).unwrap_or_default()
    } else {
        e0()
    }
}

/// Where an event happens: its session, directories, data directory, and when the hook began.
pub(crate) struct Here<'a> {
    pub(crate) session: &'a str,
    pub(crate) cwd: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) paths: &'a Paths,
    pub(crate) started: Instant,
}

/// Whether the mod's step payload says the person set a routine model (`routine_model`: a
/// non-empty model name, or `true`): only then may a step be routed ([`adage_runtime::Policy::routine`]).
fn routine_asked(p: &Value) -> bool {
    match p.get("routine_model") {
        Some(Value::String(m)) => !m.trim().is_empty(),
        Some(Value::Bool(b)) => *b,
        _ => false,
    }
}

/// Decide: everything after the kill switches, up to rendering.
pub(crate) fn respond(
    adapter: &dyn harness::Adapter,
    event: &str,
    p: &Value,
    at: &Here<'_>,
    opts: Options,
    index: &Index,
) -> Result<Responded, Failure> {
    let Here { session, cwd, root, paths, started } = *at;
    // A repository with candidates and no reflexes yet still notices (plan D2): it is not the
    // no-routes path.
    let Some(repo_key) = site::routed_key(index, root, paths, true) else {
        // Nothing installed or counted for this repository: the common, cheapest path. In a
        // repository the session is still seen (outside one there is nothing to learn).
        if root.join(".git").exists() {
            // A step boundary is not a session event the person sees: it is never recorded here.
            let ev =
                adapter.translate(event, p, &cwd.to_string_lossy()).filter(|e| !matches!(e, Event::TurnStep { .. }));
            if let Some(ev) = ev {
                let key = repo::repo_key_cached(root, &paths.repo_keys());
                record_seen(paths, session, adapter, event, &ev, &key, started);
            }
        }
        return Ok(Responded::default());
    };
    // An event this adapter does not act on.
    let Some(ev) = adapter.translate(event, p, &cwd.to_string_lossy()) else { return Ok(Responded::default()) };
    // An event that cannot change any decision stops here, before the command table, the state
    // lock and the spool (Design §13.2; `adage_runtime::fast`).
    let state_path = paths.state(session);
    let read_state = || state::try_read::<SessionState>(&state_path).ok().flatten();
    if fast::skip(&index.triggers(&repo_key), &ev, &cwd.to_string_lossy(), read_state) {
        return Ok(Responded::default());
    }
    // Predict grants matter only at a step boundary. Without one there is nothing to predict and,
    // unless the person set a routine model (a step may then be routed), nothing to record in
    // either arm: the step stops here, before the state lock.
    let step = matches!(ev, Event::TurnStep { .. });
    // A step boundary is acted on only where the harness has one (the mod's `turn.step`).
    if step && !adapter.capabilities(event, p).predict_step {
        return Ok(Responded::default());
    }
    let routine = step && routine_asked(p);
    let predict_grants: Vec<(String, String)> = match opts.extension {
        Some(x) if step => guarded(|| x.predict_grants(&repo_key, root, index, paths)),
        _ => Vec::new(),
    };
    if step && predict_grants.is_empty() && !routine {
        return Ok(Responded::default());
    }
    let table = CommandTable::load();
    // The embedding model is opened only if the runtime asks, which it does only at a prompt in a
    // repository with intent routes: the installed model, else the one bundled in the binary
    // (`model::resolve`, as the learner chose). No model: no vector, no intent directive.
    let models = paths.models();
    let embed = move |text: &str| -> Option<(String, Vec<f32>)> {
        let m = model::resolve_static(&models, false)?;
        let v = m.embed(&normalize(text))?;
        Some((m.id, v))
    };
    // ADAGE_CONTROL, else `runtime.control` in config.toml, else 0.1; `runtime.predict_cap`, else 6.
    let cfg = adage_store::config::load(paths).config;
    let arms = site::ArmInputs::with_share(cfg.runtime_control);
    let spool_path = paths.spool(session);
    // Exec-only reflexes come into play only under an extension that carries them out.
    let exec_only: Vec<(String, String)> = match opts.extension {
        Some(x) if index.for_repo(&repo_key).any(|r| r.exec_only()) => guarded(|| x.exec_only(&repo_key, index, paths)),
        _ => Vec::new(),
    };
    // The learned follow judge's weights for this repository (fitted at learn time), if any.
    let weights = adage_tracks::layout::read_follow_weights(&paths.tracks(), &repo_key);
    let policy = adage_runtime::Policy {
        // ADAGE_CARD_CHECKS=0 (an evaluation knob, like ADAGE_ARM) leaves the card's test-command
        // line out.
        card_checks: std::env::var("ADAGE_CARD_CHECKS").map_or(true, |v| v != "0"),
        // ADAGE_CARD_ORIENTATION=0 (likewise) gives the card its earlier form: the exemplar of the
        // last edit instead of where its files stand in the checkout now.
        card_orientation: std::env::var("ADAGE_CARD_ORIENTATION").map_or(true, |v| v != "0"),
        follow_weights: weights,
        exec_only,
        exact_keys: false,
        invoke_skills: if predict_grants.is_empty() { Vec::new() } else { skill::invocable(paths, root) },
        predict_grants,
        predict_cap: cfg.runtime_predict_cap,
        observe: site::observed(p),
        routine,
    };
    let ctx = Ctx {
        checkout: &adage_runtime::checkout::Disk,
        index,
        repo: repo_key.clone(),
        root,
        table: &table,
        embed: Some(&embed),
        params: Default::default(),
        policy,
    };
    // A state that can no longer be read is rebuilt from the session's spool, if it is small
    // enough to replay quickly (it is read under the state lock).
    let rebuild = || {
        let small = std::fs::metadata(&spool_path).is_ok_and(|m| m.len() <= rebuild::MAX_SPOOL_BYTES);
        let records: Vec<Value> = if small { spool::read_all(&spool_path).ok()? } else { return None };
        rebuild::rebuild(&records, index, &repo_key, &table)
    };
    let (mut decision, arm, propensity, stims) =
        state::update_or_rebuild::<SessionState, _>(&state_path, rebuild, |st| {
            arms.ensure(st, session, &repo_key);
            // A state from before propensities were recorded: the arm was drawn with (as far as can
            // be known) today's share, unless it is forced now.
            let propensity = st.propensity.unwrap_or_else(|| match (st.arm, arms.forced.as_deref()) {
                (_, Some("control" | "treated")) => 1.0,
                (Some(Arm::Control), _) => arms.share.clamp(0.0, 1.0),
                _ => 1.0 - arms.share.clamp(0.0, 1.0),
            });
            let before: Vec<(String, bool)> =
                st.obligations.iter().map(|o| (o.route.clone(), o.stop_requested)).collect();
            let d = decide(&ctx, st, &ev);
            let stims = stimuli(&before, &st.obligations, matches!(ev, Event::Batch { .. }));
            (d, st.arm, propensity, stims)
        })
        .map_err(|e| Failure::io("state", "updating the session state", &e))?;

    // A step boundary where nothing would be predicted or routed leaves no record: only a step
    // that counted against the bounds, or would have, or would be routed, is replayed or shown.
    if step && decision.would_predict.is_none() && decision.would_route.is_none() {
        return Ok(Responded::default());
    }
    // An extension (adage-exec: E1 prefetch, E2 actions) sees the directives about to be
    // delivered and the obligations armed or enforced now. A panic cannot prove no action ran.
    let mut delivery = Delivery::default();
    if let Some(x) = opts.extension {
        let directives_due = decision.continue_reason.is_none() && !decision.directives.is_empty();
        if directives_due || !stims.is_empty() {
            let turn = Turn {
                harness: adapter.harness(),
                event,
                event_identity: adapter.event_identity(event, p),
                ev: &ev,
                session,
                index,
                repo_key: &repo_key,
                root,
                cwd,
                paths,
                max_context: adapter.max_context(),
                arm,
                stimuli: &stims,
            };
            delivery = extension_delivery(|| x.deliver(&turn, &decision.directives));
        }
    }

    // Obligations an action met: no Stop request follows for them.
    if !delivery.met.is_empty() || !delivery.cpu_predictions.is_empty() {
        let met = delivery.met.clone();
        let withdrawn = matches!(delivery.stop, Some(None));
        let marked = state::update::<SessionState, _>(&state_path, |st| {
            st.pending.retain(|p| !delivery.cpu_predictions.contains(&p.route));
            // Carried out (or abstained from) in the sandbox already: never also predicted.
            for r in &delivery.cpu_predictions {
                if !st.predicted_routes.contains(r) {
                    st.predicted_routes.push(r.clone());
                }
            }
            for o in st.obligations.iter_mut().filter(|o| met.contains(&o.route)) {
                o.met = true;
                if withdrawn && o.stop_requested {
                    o.stop_requested = false;
                    st.stop_requests = st.stop_requests.saturating_sub(1);
                }
            }
        });
        if let Err(e) = marked {
            failopen::record(&paths.root, adapter.name(), event, &Failure::io("state", "marking obligations met", &e));
        }
    }
    match delivery.stop.take() {
        Some(None) => decision.continue_reason = None,
        Some(Some(r)) if decision.continue_reason.is_some() => decision.continue_reason = Some(r),
        _ => {}
    }

    let ts = record::now_ms();
    // `propensity`: the probability of `arm` at assignment (control share, or one minus it; 1 when
    // `ADAGE_ARM` forced it), for inverse-propensity weighting (Design §11, §14.1).
    let mut rec = json!({ "ts": ts, "event": event, "kind": ev.name(), "arm": arm, "propensity": propensity, "repo": repo_key, "generation": index.generation, "decision": decision });
    record::name_harness(&mut rec, adapter);
    // The harness's ids for the calls judged here (in order), where its payload carries them: a
    // judgement is then drawn under its call, not merely near it.
    let ids: Vec<&str> = match &ev {
        Event::Batch { calls } => calls.iter().filter_map(|c| c.tool_use_id.as_deref()).collect(),
        Event::Failure { call } => call.tool_use_id.as_deref().into_iter().collect(),
        _ => Vec::new(),
    };
    if !ids.is_empty() {
        rec["tool_use_ids"] = json!(ids);
    }
    if let Some(x) = delivery.spool.take() {
        rec["exec"] = x;
    }
    if delivery.recovery.is_some() {
        rec["recovery_required"] = json!(true);
    }
    rec["duration_us"] = json!(record::duration_us(started));
    // Measurement may be lost, but the decision stands: a failure is recorded and the event goes on.
    record::append_or_log(paths, session, adapter.name(), event, &rec);

    let at_stop_action = delivery.at_stop_action;
    let done = |out: Option<Output>| Ok(Responded { out, at_stop_action });
    if let Some(warning) = delivery.recovery.take() {
        return done(bounded_notice(warning, RECOVERY_NOTICE, adapter.max_context()).map(|text| {
            // At a Stop the warning is a continuation, marked as Adage's own text like every other.
            if matches!(ev, Event::Stop { .. }) {
                Output::Continue(render::finish_line(&text))
            } else {
                Output::Context(text)
            }
        }));
    }
    if let Some(reason) = decision.continue_reason {
        return done(Some(Output::Continue(reason)));
    }
    // A step boundary carries no context: a predicted step, a routed one, or nothing.
    if step {
        let out = match (decision.predicted.take(), decision.routine.take()) {
            (Some(p), _) => Some(Output::Predict(p)),
            (None, Some(_)) => Some(Output::Routine),
            (None, None) => None,
        };
        return done(out);
    }
    // A grant that stopped matching: one line for the person, once per session, beside whatever
    // the agent receives (never at a Stop, which has no such channel).
    let lapse = delivery.notice.take().filter(|_| {
        !matches!(ev, Event::Stop { .. })
            && adage_store::config::load(paths).config.ui_notices
            && first_lapse_notice(paths, session)
    });
    let with_lapse = |text: String| match &lapse {
        Some(n) if !text.is_empty() => Output::SessionContext { context: text, notice: Some(n.clone()) },
        Some(n) => Output::Notice(n.clone()),
        None => Output::Context(text),
    };
    if decision.directives.is_empty() && delivery.extra.is_empty() {
        return done(lapse.map(Output::Notice));
    }
    let max = adapter.max_context();
    let e0 = || decision.directives.iter().map(render::wrap_directive).collect::<Vec<_>>().join("\n\n");
    let texts = match delivery.texts {
        Some(t) if t.len() == decision.directives.len() => t,
        _ if !delivery.met.is_empty() => vec![APPLIED_NOTICE.into()],
        _ => decision.directives.iter().map(render::wrap_directive).collect(),
    };
    let all: Vec<String> = texts.into_iter().chain(delivery.extra).collect();
    // An applied action cannot fall back to its original "run this" directive on overflow.
    let mut text = fitted_context(all.join("\n\n"), e0, !delivery.met.is_empty(), max);
    if text.is_empty() {
        return done(lapse.map(Output::Notice));
    }
    // Design W8 (spill overflow to a file) cannot arise: the longest decision validation allows
    // fits every harness's cap (`the_largest_decision_fits_every_harness_cap`). This is a backstop.
    if text.len() > max {
        text.truncate(text.char_indices().take_while(|(i, _)| *i < max).last().map(|(i, _)| i).unwrap_or(0));
    }
    done(Some(with_lapse(text)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use crate::{handle, Extension, SessionTexts};
    use adage_runtime::Directive;

    #[test]
    fn execution_fallback_never_reintroduces_an_action_after_an_uncertain_result() {
        let panic = extension_delivery(|| panic!("after a possible side effect"));
        assert!(panic.recovery.unwrap().contains("Do not retry"));
        let overlong = format!("{}{}", RECOVERY_NOTICE, "x".repeat(10_000));
        let warning = bounded_notice(overlong, RECOVERY_NOTICE, 512).unwrap();
        assert_eq!(warning, RECOVERY_NOTICE);
        assert!(bounded_notice("x".repeat(1000), RECOVERY_NOTICE, 10).is_none());
        let applied = fitted_context("x".repeat(10_000), || panic!("must not restore run instruction"), true, 512);
        assert_eq!(applied, APPLIED_NOTICE);
        let readonly = fitted_context("x".repeat(10_000), || "read-only advice".into(), false, 512);
        assert_eq!(readonly, "read-only advice");
    }

    /// A grant that stopped matching is told to the person (Claude Code's `systemMessage`, never
    /// the model), at most once per session: at the session start, or beside the first event
    /// whose action it withheld.
    #[test]
    fn a_lapsed_grant_notice_is_shown_once_per_session() {
        const LINE: &str = "adage: the E2 approval for acme/finish-lines/x no longer matches (…), so it does not run.";
        struct Lapsed;
        impl Extension for Lapsed {
            fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
                Delivery { notice: Some(LINE.into()), ..Delivery::default() }
            }
            fn session_texts(&self, _: &str, _: &Path, _: &Index, _: &Paths) -> SessionTexts {
                SessionTexts { context: Some("contract".into()), notice: Some(LINE.into()) }
            }
        }
        static LAPSED: Lapsed = Lapsed;
        let w = ObligationWorld::new();
        let start = |session: &str| {
            let p = json!({ "session_id": session, "cwd": w.root, "source": "startup" });
            let opts = Options { learner: None, extension: Some(&LAPSED) };
            handle("claude", "SessionStart", &p.to_string(), opts, &w.paths, Instant::now())
                .unwrap()
                .map(|o| serde_json::from_str::<Value>(&o).unwrap())
        };
        let first = start("s").expect("contract and notice");
        assert_eq!(first["systemMessage"], LINE, "{first}");
        assert_eq!(first["hookSpecificOutput"]["additionalContext"], "contract");
        let again = start("s").expect("the contract again");
        assert!(again.get("systemMessage").is_none(), "once per session: {again}");
        let edit = w.edit(&LAPSED).expect("the obligation's directive");
        assert!(edit.get("systemMessage").is_none(), "already told this session: {edit}");
        // Another session: told once, beside the directive the event delivers.
        state::update::<SessionState, _>(&w.paths.state("s2"), |st| st.arm = Some(Arm::Treated)).unwrap();
        let event = |session: &str| {
            let call = json!({ "tool_name": "Edit", "tool_use_id": "t2", "tool_response": "ok",
                               "tool_input": { "file_path": w.root.join("registry/currencies.csv") } });
            let p = json!({ "session_id": session, "cwd": w.root, "tool_calls": [call] });
            let opts = Options { learner: None, extension: Some(&LAPSED) };
            handle("claude", "PostToolBatch", &p.to_string(), opts, &w.paths, Instant::now())
                .unwrap()
                .map(|o| serde_json::from_str::<Value>(&o).unwrap())
        };
        let told = event("s2").expect("directive and notice");
        assert_eq!(told["systemMessage"], LINE, "{told}");
        assert!(told["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("make gen"), "{told}");
        assert!(!told["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("no longer matches"));
        let quiet = event("s2");
        assert!(quiet.as_ref().is_none_or(|q| q.get("systemMessage").is_none()), "{quiet:?}");
    }

    /// A stand-in for adage-exec with a person's `predict` grant on the finish line.
    struct Predicts(Vec<(String, String)>);

    impl Extension for Predicts {
        fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
            Delivery::default()
        }
        fn predict_grants(&self, repo_key: &str, root: &Path, _: &Index, _: &Paths) -> Vec<(String, String)> {
            assert_eq!(repo_key, "acme");
            assert!(root.ends_with("acme"), "the checkout the step was asked in: {}", root.display());
            self.0.clone()
        }
    }

    fn turn_step(w: &ObligationWorld, ext: &'static dyn Extension, index: u32, ending: bool) -> Option<Value> {
        w.event(
            ext,
            "TurnStep",
            json!({ "hook_event_name": "TurnStep", "turn_id": "t1", "index": index, "model": "claude-x", "ending": ending }),
        )
    }

    /// T-C1-10, in process: with a grant, the step that ends the turn yields the armed finish
    /// line's call, spooled as `turn_step`; Adage's call then meets it and no Stop request follows.
    /// Without a grant nothing is printed and nothing is recorded.
    #[test]
    fn turn_step_predicts_the_granted_finish_line() {
        let w = ObligationWorld::new();
        let granted: &'static Predicts =
            Box::leak(Box::new(Predicts(vec![("acme/finish-lines".into(), "regenerate-registry".into())])));
        let none: &'static Predicts = Box::leak(Box::new(Predicts(vec![])));
        assert!(turn_step(&w, granted, 1, true).is_none(), "nothing directed yet: the fast path");
        w.edit(granted).expect("the edit arms the obligation");
        let records = spooled(&w).len();
        assert!(turn_step(&w, none, 1, true).is_none(), "no grant, no prediction");
        assert_eq!(spooled(&w).len(), records, "and no record");
        assert!(turn_step(&w, granted, 1, false).is_none(), "not the turn's last step");
        let out = turn_step(&w, granted, 2, true).expect("the finish line, in the model's place");
        assert_eq!(
            out,
            json!({ "predict": { "kind": "obligation", "route": "regenerate-registry", "track": "acme/finish-lines",
                                  "calls": [{ "tool": "Bash", "input": { "command": "make gen" } }] } })
        );
        let rec = spooled(&w).pop().unwrap();
        assert_eq!((rec["kind"].as_str(), rec["event"].as_str()), (Some("turn_step"), Some("TurnStep")));
        assert_eq!(rec["decision"]["predicted"]["route"], "regenerate-registry");
        assert_eq!(rec["decision"]["predicted_at"], json!(["t1", 2]));
        assert!(turn_step(&w, granted, 3, true).is_none(), "a route is predicted once per session");
        let call = json!({ "tool_name": "Bash", "tool_use_id": "adage_regenerate-registry_t1_2",
                           "tool_input": { "command": "make gen" }, "tool_response": "ok" });
        w.event(granted, "PostToolBatch", json!({ "tool_calls": [call] }));
        assert!(w.state().obligations[0].met, "Adage's call met the finish line");
        assert!(w.stop(none, false).is_none(), "no Stop request follows");
    }

    /// T-C8-03 in process: the finish line Adage's call failed stays unmet; the next turn's end
    /// does not predict it again for that arming, and the Stop request asks for it as without the
    /// mod. Met by the agent then, a later edit arms it again, and it is predicted again.
    #[test]
    fn a_finish_line_whose_predicted_call_failed_falls_back_to_the_stop_request() {
        let w = ObligationWorld::new();
        let granted: &'static Predicts =
            Box::leak(Box::new(Predicts(vec![("acme/finish-lines".into(), "regenerate-registry".into())])));
        w.edit(granted).expect("the edit arms the obligation");
        assert!(turn_step(&w, granted, 2, true).is_some(), "the finish line, in the model's place");
        let failed = json!({ "tool_name": "Bash", "tool_use_id": "adage_regenerate-registry_t1_2_0",
                             "tool_input": { "command": "make gen" }, "error": "make: *** [gen] Error 2" });
        w.event(granted, "PostToolUseFailure", failed);
        assert!(!w.state().obligations[0].met);
        assert!(turn_step(&w, granted, 4, true).is_none(), "once per arming");
        let stop = w.stop(granted, false).expect("the Stop request, as without the mod");
        assert_eq!(stop["decision"], "block");
        assert!(stop["reason"].as_str().unwrap().contains("make gen"), "{stop}");
        let mine = json!({ "tool_name": "Bash", "tool_use_id": "toolu_gen", "tool_input": { "command": "make gen" },
                           "tool_response": "ok" });
        w.event(granted, "PostToolBatch", json!({ "tool_calls": [mine] }));
        assert!(w.state().obligations[0].met, "the agent's own run met it");
        w.edit(granted);
        let again =
            json!({ "hook_event_name": "TurnStep", "turn_id": "t2", "index": 3, "model": "claude-x", "ending": true });
        assert!(w.event(granted, "TurnStep", again).is_some(), "armed again: predicted again");
    }

    /// The control arm: nothing printed, `would_predict` spooled.
    #[test]
    fn turn_step_in_the_control_arm_prints_nothing() {
        let w = ObligationWorld::new();
        let granted: &'static Predicts =
            Box::leak(Box::new(Predicts(vec![("acme/finish-lines".into(), "regenerate-registry".into())])));
        state::update::<SessionState, _>(&w.paths.state("s"), |st| st.arm = Some(Arm::Control)).unwrap();
        w.edit(granted);
        assert!(turn_step(&w, granted, 1, true).is_none());
        let rec = spooled(&w).pop().unwrap();
        assert_eq!(rec["kind"], "turn_step");
        assert!(rec["decision"].get("predicted").is_none());
        assert_eq!(rec["decision"]["would_predict"]["kind"], "obligation");
    }

    /// The adapters: Claude Code translates the mod's payload and renders a prediction; a call
    /// whose id Adage minted is Adage's; Codex and OpenCode have no such event and render nothing.
    #[test]
    fn turn_step_on_each_adapter() {
        use crate::harness::Adapter;
        let claude = harness::claude::Claude;
        let p = json!({ "session_id": "s", "cwd": "/r", "hook_event_name": "TurnStep", "turn_id": "t", "index": 3,
                         "model": "m", "agent_id": "a", "ending": true });
        match claude.translate("TurnStep", &p, "/r") {
            Some(Event::TurnStep { turn_id, index, model, agent, ending }) => {
                assert_eq!(
                    (turn_id.as_str(), index, model.as_deref(), agent.as_deref(), ending),
                    ("t", 3, Some("m"), Some("a"), true)
                );
            }
            other => panic!("{other:?}"),
        }
        let bare = json!({ "turn_id": "t", "index": 1 });
        assert!(matches!(
            claude.translate("TurnStep", &bare, "/r"),
            Some(Event::TurnStep { ending: false, model: None, .. })
        ));
        assert!(claude.translate("TurnStep", &json!({ "turn_id": "t" }), "/r").is_none(), "no index");
        assert!(claude.translate("TurnStep", &json!({ "turn_id": "t", "index": -1 }), "/r").is_none());
        let batch = json!({ "tool_calls": [
            { "tool_name": "Bash", "tool_use_id": "adage_gen_t_1", "tool_input": { "command": "make gen" } },
            { "tool_name": "Bash", "tool_use_id": "toolu_1", "tool_input": { "command": "ls" } },
            { "tool_name": "Bash", "tool_input": { "command": "ls" } } ] });
        let Some(Event::Batch { calls }) = claude.translate("PostToolBatch", &batch, "/r") else { panic!() };
        let by: Vec<(adage_runtime::By, Option<&str>)> =
            calls.iter().map(|c| (c.by, c.tool_use_id.as_deref())).collect();
        use adage_runtime::By;
        assert_eq!(by, vec![(By::Adage, Some("adage_gen_t_1")), (By::Agent, Some("toolu_1")), (By::Agent, None)]);
        let failure = json!({ "tool_name": "Bash", "tool_use_id": "adage_gen_t_1", "tool_input": {}, "error": "x" });
        assert!(
            matches!(claude.translate("PostToolUseFailure", &failure, "/r"), Some(Event::Failure { call }) if call.by == By::Adage)
        );
        let pred = adage_runtime::Prediction {
            kind: adage_runtime::PredictionKind::Skill,
            route: "r".into(),
            track: "acme/t".into(),
            calls: vec![adage_runtime::PredictedCall { tool: "Skill".into(), input: json!({ "skill": "release" }) }],
        };
        let out: Value =
            serde_json::from_str(&claude.render("TurnStep", Output::Predict(pred.clone())).unwrap()).unwrap();
        assert_eq!(
            out,
            json!({ "predict": { "kind": "skill", "route": "r", "track": "acme/t", "calls": [{ "tool": "Skill", "input": { "skill": "release" } }] } })
        );
        for name in ["codex", "opencode"] {
            let a = harness::adapter(name).unwrap();
            assert!(a.translate("TurnStep", &p, "/r").is_none(), "{name}");
            assert!(a.render("TurnStep", Output::Predict(pred.clone())).is_none(), "{name}");
        }
    }

    /// Review L1: `runtime.predict_cap` in config.toml bounds the predictions a session gets; at
    /// 0 a granted step predicts nothing.
    #[test]
    fn the_predict_cap_is_read_from_the_config() {
        let w = ObligationWorld::new();
        let granted: &'static Predicts =
            Box::leak(Box::new(Predicts(vec![("acme/finish-lines".into(), "regenerate-registry".into())])));
        w.edit(granted).expect("the edit arms the obligation");
        adage_store::config::set(&w.paths, "runtime.predict_cap", "0").unwrap();
        assert!(turn_step(&w, granted, 2, true).is_none(), "the cap is reached before the first");
        adage_store::config::set(&w.paths, "runtime.predict_cap", "6").unwrap();
        assert!(turn_step(&w, granted, 3, true).is_some(), "the default cap allows it");
    }

    /// Observe mode (`"observe": true`, the mod's `ADAGE_MOD_OBSERVE`): a treated session's step
    /// prints nothing and spools `would_predict`, as the control arm does; the arm is unchanged.
    #[test]
    fn turn_step_in_observe_mode_prints_nothing() {
        let w = ObligationWorld::new();
        let granted: &'static Predicts =
            Box::leak(Box::new(Predicts(vec![("acme/finish-lines".into(), "regenerate-registry".into())])));
        w.edit(granted);
        let step = json!({ "hook_event_name": "TurnStep", "turn_id": "t1", "index": 2, "model": "m", "ending": true,
                           "observe": true });
        assert!(w.event(granted, "TurnStep", step).is_none());
        let rec = spooled(&w).pop().unwrap();
        assert!(rec["decision"].get("predicted").is_none());
        assert_eq!(rec["decision"]["would_predict"]["route"], "regenerate-registry");
        assert_eq!((rec["arm"].as_str(), w.state().arm), (Some("treated"), Some(Arm::Treated)));
    }

    /// T-C5-04 (the binary, end to end): with the mod's `routine_model` set, a step whose pending
    /// `fire` directive holds no grant answers `{"routine": true}` and is recorded; without it,
    /// nothing and no record; in control, nothing and `would_route` recorded.
    #[test]
    fn a_routine_step_is_routed_only_when_the_person_set_a_model() {
        for arm in [Arm::Treated, Arm::Control] {
            let w = ObligationWorld::new();
            let table = CommandTable::load();
            state::update::<SessionState, _>(&w.paths.state("s"), |st| {
                st.arm = Some(arm);
                st.pending = vec![adage_runtime::Pending {
                    route: "regenerate-registry".into(),
                    expect_key: adage_runtime::command_key("make gen", &table),
                    expect_head: String::new(),
                    batches_left: 3,
                    command: "make gen".into(),
                }];
            })
            .unwrap();
            let step = |routine: Value| {
                let mut p = json!({ "hook_event_name": "TurnStep", "turn_id": "t1", "index": 1 });
                if !routine.is_null() {
                    p["routine_model"] = routine;
                }
                w.event(&NO_EXTENSION, "TurnStep", p)
            };
            assert!(step(Value::Null).is_none() && step(json!("")).is_none());
            assert!(!w.paths.spool("s").exists(), "no routine model: nothing recorded");
            let out = step(json!("claude-small"));
            let rec = spooled(&w).pop().unwrap();
            assert_eq!(rec["kind"], "turn_step");
            assert_eq!(rec["decision"]["would_route"]["route"], "regenerate-registry");
            assert!(rec["duration_us"].is_u64());
            match arm {
                Arm::Treated => {
                    assert_eq!(out, Some(json!({ "routine": true })));
                    assert_eq!(rec["decision"]["routine"]["route"], "regenerate-registry");
                }
                Arm::Control => {
                    assert!(out.is_none());
                    assert!(rec["decision"].get("routine").is_none());
                }
            }
        }
    }

    // ------------------------------------------------------------ noticed (D2)

    /// T-D2-04 at the hook: a repository with candidates and no reflexes yet takes the match path,
    /// not the no-routes one: the batch that completes a candidate's context is decided and
    /// spooled with `noticed` (in both arms), and nothing is printed; a session's other events
    /// stay on the fast path.
    #[test]
    fn a_repository_with_candidates_and_no_routes_notices_them() {
        use adage_tracks::{CtxItem, Step, Trigger};
        let w = ObligationWorld::new();
        let mut index = Index { generation: "candidates-only".into(), ..Default::default() };
        index.attach_candidates(
            [(
                "acme".to_string(),
                vec![adage_tracks::candidates::Candidate {
                    id: "run-lint".into(),
                    trigger: Trigger::Context {
                        items: vec![
                            CtxItem::Edit { glob: "acme/*.py".into() },
                            CtxItem::Act { keys: vec!["exec make test".into()] },
                        ],
                        within: 3,
                    },
                    step: Step::Run { command: "make lint".into() },
                    support: 2,
                    s_min: 3,
                    days: 2,
                    last_seen: None,
                    review: false,
                }],
            )]
            .into_iter()
            .collect(),
        );
        index.write_atomic(&w.paths.index()).unwrap();
        std::fs::create_dir_all(w.root.join("acme")).unwrap();
        for (session, arm) in [("s", "treated"), ("c", "control")] {
            state::update::<SessionState, _>(&w.paths.state(session), |st| {
                st.arm = Some(if arm == "control" { Arm::Control } else { Arm::Treated })
            })
            .unwrap();
            let batch = |call: Value| {
                let p = json!({ "session_id": session, "cwd": w.root, "tool_calls": [call] });
                let opts = Options { learner: None, extension: Some(&NO_EXTENSION) };
                handle("claude", "PostToolBatch", &p.to_string(), opts, &w.paths, Instant::now()).unwrap()
            };
            assert_eq!(
                batch(
                    json!({ "tool_name": "Bash", "tool_use_id": "a", "tool_input": { "command": "ls" }, "tool_response": "" })
                ),
                None
            );
            assert_eq!(
                batch(
                    json!({ "tool_name": "Edit", "tool_use_id": "b", "tool_input": { "file_path": w.root.join("acme/a.py") }, "tool_response": "ok" })
                ),
                None
            );
            assert_eq!(
                batch(
                    json!({ "tool_name": "Bash", "tool_use_id": "c", "tool_input": { "command": "make test" }, "tool_response": "OK" })
                ),
                None
            );
            let recs = spool::read_all::<Value>(&w.paths.spool(session)).unwrap();
            assert!(recs.iter().all(|r| r.get("unrouted").is_none()), "{arm}: not the no-routes path: {recs:?}");
            assert_eq!(recs.len(), 2, "{arm}: `ls` is skipped by the fast path: {recs:?}");
            assert_eq!(
                recs[1]["decision"]["noticed"],
                json!([{ "candidate": "run-lint", "support": 2, "s_min": 3, "review": false }]),
                "{arm}"
            );
            assert!(recs[1]["decision"]["would_have"].as_array().unwrap().is_empty());
        }
    }
}
