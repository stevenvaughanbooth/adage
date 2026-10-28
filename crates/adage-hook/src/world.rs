//! The world at the hook (the situational awareness layer's floor, on every harness): at a prompt,
//! the resolver's hint rides after the reflexes' text; at the tool batches that follow, the hint
//! is labelled used or not; a prompt that names its own artifact labels the hint before it
//! overridden. Everything the resolver reads comes from the world's store
//! (`adage_store::Paths::world_*`), written by `adage world ingest` and `adage world follow`;
//! nothing here reaches the network, and nothing here changes a reflex decision.
//!
//! Off unless `world.enabled` is set, and silent while `world/paused` exists. In the control arm
//! the hint is withheld and recorded as `would_hint`, as every directive is.

use crate::ctx::HookCtx;
use crate::harness::Output;
use crate::record::{Record, RecordKind};
use crate::respond::Responded;
use adage_intent::model::{self, hashed_embed};
use adage_runtime::{Arm, Call, SessionState};
use adage_store::{state, Paths};
use adage_trace::{Injection, InjectionAt, PendingHint};
use adage_world::{ArtifactsState, CalendarState, Decision, Params, WorldCtx};
use serde_json::Value;

/// Tool batches a hint waits for its label.
const LABEL_BATCHES: u32 = 3;

/// An embedder: normalised text in, the model's unit vector out (`None` for a text with no words).
pub type Embedder = Box<dyn Fn(&str) -> Option<Vec<f32>>>;

/// The embedder every writer and reader of the world shares: the installed or bundled static
/// model when the binary has one, else the hashed fallback every binary has. It takes text
/// already normalised (`adage_intent::normalize::normalize`).
pub fn embedder(paths: &Paths) -> Embedder {
    match model::resolve_static(&paths.models(), false) {
        Some(m) => Box::new(move |t: &str| m.embed(t)),
        None => Box::new(|t: &str| hashed_embed(t)),
    }
}

/// Whether the world delivers anything now: on, and not paused.
pub(crate) fn live(ctx: &HookCtx<'_>) -> bool {
    ctx.config().world_enabled && !ctx.paths.world_paused().exists()
}

/// The person's local offset from UTC, when the payload says it (the mod does; a command hook's
/// payload does not, and times are then printed in UTC).
fn tz_offset_min(p: &Value) -> i32 {
    p.get("tz_offset_min")
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .filter(|v| v.abs() <= 14 * 60)
        .unwrap_or(0)
}

/// [`Responded`] with the world's hint for `prompt` added to its context, when there is one and
/// it fits beside the reflexes' text.
pub(crate) fn with_hint(ctx: &HookCtx<'_>, r: Responded, prompt: &str) -> Responded {
    let Some(hint) = at_prompt(ctx, prompt) else { return r };
    let max = ctx.adapter.max_context();
    let joined = |text: String| {
        if text.is_empty() {
            hint.clone()
        } else if text.len() + 2 + hint.len() <= max {
            format!("{text}\n\n{hint}")
        } else {
            text
        }
    };
    let out = match r.out {
        None => Some(Output::Context(hint)),
        Some(Output::Context(c)) => Some(Output::Context(joined(c))),
        Some(Output::SessionContext { context, notice }) => {
            Some(Output::SessionContext { context: joined(context), notice })
        }
        Some(Output::Notice(n)) => Some(Output::SessionContext { context: hint, notice: Some(n) }),
        other => other,
    };
    Responded { out, ..r }
}

/// The hint for a prompt, recorded; `None` when the world is off, paused, empty, or has nothing
/// to say, and in the control arm (recorded as withheld).
pub(crate) fn at_prompt(ctx: &HookCtx<'_>, prompt: &str) -> Option<String> {
    if !live(ctx) {
        return None;
    }
    let HookCtx { adapter, event, p, session, paths, started, .. } = *ctx;
    let state_path = paths.state(session);
    // A prompt that names its own artifact says the hint before it (if any) was not what the
    // person meant.
    if adage_world::gate::names_its_own(prompt) {
        label(ctx, |pending| Some((pending.id.clone(), None, true)));
    }
    let artifacts = state::try_read::<ArtifactsState>(&paths.world_artifacts()).ok().flatten()?;
    if artifacts.artifacts.is_empty() {
        return None;
    }
    let calendar = state::try_read::<CalendarState>(&paths.world_calendar()).ok().flatten().unwrap_or_default();
    let all = artifacts.all();
    let embed = embedder(paths);
    let params = Params::default();
    let now = adage_core::time::now_ms();
    let wctx = WorldCtx {
        now,
        tz_offset_min: tz_offset_min(p),
        artifacts: &all,
        events: &[],
        blocks: &calendar.blocks,
        embed: &embed,
        params: &params,
    };
    let decision = adage_world::resolve(&wctx, prompt);
    let hint = decision.hint()?;
    let arms = ctx.arms();
    let key = ctx.repo_key().to_string();
    let locators: Vec<(String, String)> = hint
        .artifacts
        .iter()
        .filter_map(|id| all.iter().find(|a| &a.id == id).map(|a| (a.id.clone(), a.locator.clone())))
        .collect();
    let (arm, id) = state::update::<SessionState, _>(&state_path, |st| {
        arms.ensure(st, session, &key);
        st.world.hints += 1;
        let id = format!("w{now}-{}", st.world.hints);
        if st.arm != Some(Arm::Control) {
            st.world.pending =
                Some(PendingHint { id: id.clone(), artifacts: locators.clone(), batches_left: LABEL_BATCHES });
        }
        (st.arm, id)
    })
    .ok()?;
    let withheld = arm == Some(Arm::Control);
    let injection = Injection {
        id,
        session: session.to_string(),
        at: InjectionAt::Prompt,
        cls: hint.cls,
        artifacts: hint.artifacts.clone(),
        tokens: adage_world::text::tokens(&hint.text),
        confidence: Some(hint.confidence),
        used: None,
        overridden: None,
    };
    let top: Vec<&adage_world::Scored> = hint.scored.iter().take(3).collect();
    let mut rec = Record::new(RecordKind::Injection, event)
        .with("arm", arm)
        .with("repo", &key)
        .with("injection", &injection)
        .with("scored", top)
        .harness(adapter);
    if withheld {
        rec.set("withheld", true);
        rec.set("would_hint", true);
    }
    rec.timed(started).append_or_log(paths, session, adapter.name(), event);
    if let Decision::None { .. } = decision {
        return None;
    }
    (!withheld).then(|| hint.text.clone())
}

/// A tool batch: the hint waiting for its label is used when a call opened one of its artifacts;
/// after [`LABEL_BATCHES`] batches without one, it was not.
pub(crate) fn at_batch(ctx: &HookCtx<'_>, calls: &[Call]) {
    if !ctx.config().world_enabled {
        return;
    }
    label(ctx, |pending| {
        let used = calls.iter().any(|c| opens(c, &pending.artifacts));
        if used {
            return Some((pending.id.clone(), Some(true), false));
        }
        pending.batches_left = pending.batches_left.saturating_sub(1);
        (pending.batches_left == 0).then(|| (pending.id.clone(), Some(false), false))
    });
}

/// Whether a call opens one of the hinted artifacts: a path argument that ends with the
/// locator, or a shell command or path that names it, its key or its number.
fn opens(call: &Call, artifacts: &[(String, String)]) -> bool {
    let text = match &call.input {
        Value::Object(m) => m.values().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
        Value::String(s) => s.clone(),
        _ => String::new(),
    };
    if text.is_empty() {
        return false;
    }
    let expand = |s: &str| s.replace("~/", "/");
    artifacts.iter().any(|(id, locator)| {
        let loc = expand(locator);
        let loc = loc.trim_start_matches('/');
        (!loc.is_empty() && loc.len() >= 3 && expand(&text).contains(loc))
            || adage_world::text::words(id)
                .iter()
                .chain(adage_world::text::words(locator).iter())
                .any(|w| adage_world::gate::is_key(w) && adage_world::text::words(&text).contains(w))
    })
}

/// Settle the pending hint under the state lock: `f` says whether it is done, as `(id, used,
/// overridden)`; a settled hint writes its label record and is cleared.
fn label(ctx: &HookCtx<'_>, f: impl FnOnce(&mut PendingHint) -> Option<(String, Option<bool>, bool)>) {
    let HookCtx { adapter, event, session, paths, started, .. } = *ctx;
    let state_path = paths.state(session);
    if !state::try_read::<SessionState>(&state_path).ok().flatten().is_some_and(|st| st.world.pending.is_some()) {
        return;
    }
    let settled = state::update::<SessionState, _>(&state_path, |st| {
        let pending = st.world.pending.as_mut()?;
        let done = f(pending);
        if done.is_some() {
            st.world.pending = None;
        }
        done
    })
    .ok()
    .flatten();
    if let Some((id, used, overridden)) = settled {
        let mut rec = Record::new(RecordKind::InjectionLabel, event).with("injection_id", id).harness(adapter);
        if let Some(u) = used {
            rec.set("used", u);
        }
        if overridden {
            rec.set("overridden", true);
        }
        rec.timed(started).append_or_log(paths, session, adapter.name(), event);
    }
}

#[cfg(test)]
mod tests {
    use crate::testkit::*;
    use adage_intent::normalize::normalize;
    use adage_runtime::Arm;
    use adage_store::state;
    use adage_trace::{Artifact, ArtifactKind};
    use adage_world::ArtifactsState;
    use serde_json::{json, Value};

    fn pr(id: &str, title: &str, locator: &str, paths: &adage_store::Paths) -> Artifact {
        let now = adage_core::time::now_ms();
        let embed = super::embedder(paths);
        let mut a = Artifact {
            id: id.into(),
            kind: ArtifactKind::Pr,
            title: title.into(),
            locator: locator.into(),
            summary: None,
            embedding: None,
            first_seen: now - 20 * 60_000,
            last_touched: now - 20 * 60_000,
            related: vec![],
            state: Some("ci:green".into()),
            people: vec![],
        };
        a.embedding = embed(&normalize(&a.text())).map(|v| adage_intent::quantize(&v));
        a
    }

    fn seeded(arm: Arm, enabled: bool) -> ObligationWorld {
        let w = dead_end_world(arm);
        if enabled {
            std::fs::write(w.paths.config(), "[learn]\nauto = false\n\n[world]\nenabled = true\n").unwrap();
        }
        state::update::<ArtifactsState, _>(&w.paths.world_artifacts(), |s| {
            s.upsert(pr("github:pr:412", "Schema evolution for invoices", "#412", &w.paths));
        })
        .unwrap();
        w
    }

    fn records_of(w: &ObligationWorld, kind: &str) -> Vec<Value> {
        spooled(w).into_iter().filter(|r| r["kind"] == kind).collect()
    }

    /// T-W2-01: with the world on, a referential prompt gets the hint after the reflexes' text and
    /// an `injection` record; off, the same prompt's output is exactly what it was.
    #[test]
    fn a_prompt_gets_a_hint_only_while_the_world_is_on() {
        let off = seeded(Arm::Treated, false);
        let before = off.event(
            &NO_EXTENSION,
            "UserPromptSubmit",
            json!({ "prompt": "review the PR and fix the invoice rounding" }),
        );
        assert!(!before.unwrap().to_string().contains("adage-world"));
        assert!(records_of(&off, "injection").is_empty());

        let on = seeded(Arm::Treated, true);
        let out = on
            .event(&NO_EXTENSION, "UserPromptSubmit", json!({ "prompt": "review the PR and fix the invoice rounding" }))
            .unwrap();
        let text = out["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
        assert!(text.contains("<adage-reflex"), "the reflex comes first: {text}");
        assert!(text.contains("<adage-world kind=\"hint\">") && text.contains("#412"), "{text}");
        assert!(text.find("adage-reflex").unwrap() < text.find("adage-world").unwrap());
        let rec = &records_of(&on, "injection")[0];
        assert_eq!(rec["injection"]["cls"], "hint");
        assert_eq!(rec["injection"]["artifacts"], json!(["github:pr:412"]));
        assert_eq!(rec["arm"], "treated");
        assert!(rec.get("withheld").is_none());
        assert!(on.state().world.pending.is_some(), "the hint waits for its label");

        // A prompt with no referential signal: no hint, no record.
        let n = records_of(&on, "injection").len();
        on.event(&NO_EXTENSION, "UserPromptSubmit", json!({ "prompt": "add a currency field" }));
        assert_eq!(records_of(&on, "injection").len(), n);
    }

    /// T-W2-01 (control): the hint is withheld and recorded as `would_hint`; paused, nothing is
    /// recorded at all.
    #[test]
    fn the_control_arm_withholds_and_a_pause_silences() {
        let c = seeded(Arm::Control, true);
        let out = c.event(&NO_EXTENSION, "UserPromptSubmit", json!({ "prompt": "review the PR" }));
        assert!(!out.map(|o| o.to_string()).unwrap_or_default().contains("adage-world"));
        let rec = &records_of(&c, "injection")[0];
        assert_eq!((&rec["withheld"], &rec["would_hint"]), (&json!(true), &json!(true)));
        assert!(c.state().world.pending.is_none());

        let p = seeded(Arm::Treated, true);
        std::fs::write(p.paths.world_paused(), "").unwrap();
        let out = p.event(&NO_EXTENSION, "UserPromptSubmit", json!({ "prompt": "review the PR" }));
        assert!(!out.map(|o| o.to_string()).unwrap_or_default().contains("adage-world"));
        assert!(records_of(&p, "injection").is_empty());
    }

    /// T-W2-07: the label. A call that names the hinted pull request within three batches labels
    /// it used; three batches without one label it unused; a prompt naming its own file labels
    /// the hint before it overridden.
    #[test]
    fn a_hint_is_labelled_by_the_batches_that_follow() {
        let w = seeded(Arm::Treated, true);
        w.event(&NO_EXTENSION, "UserPromptSubmit", json!({ "prompt": "review the PR" }));
        let bash = |cmd: &str| json!({ "tool_calls": [{ "tool_name": "Bash", "tool_use_id": "t", "tool_input": { "command": cmd }, "tool_response": "" }] });
        w.event(&NO_EXTENSION, "PostToolBatch", bash("gh pr view 412"));
        assert!(records_of(&w, "injection_label").is_empty(), "412 alone is not #412");
        w.event(&NO_EXTENSION, "PostToolBatch", bash("gh pr view #412 --json files"));
        let label = &records_of(&w, "injection_label")[0];
        assert_eq!(label["used"], json!(true));
        assert!(w.state().world.pending.is_none());

        let u = seeded(Arm::Treated, true);
        u.event(&NO_EXTENSION, "UserPromptSubmit", json!({ "prompt": "review the PR" }));
        for _ in 0..3 {
            u.event(&NO_EXTENSION, "PostToolBatch", bash("ls"));
        }
        assert_eq!(records_of(&u, "injection_label")[0]["used"], json!(false));

        let o = seeded(Arm::Treated, true);
        o.event(&NO_EXTENSION, "UserPromptSubmit", json!({ "prompt": "review the PR" }));
        o.event(&NO_EXTENSION, "UserPromptSubmit", json!({ "prompt": "no, read ~/notes/other.md instead" }));
        assert_eq!(records_of(&o, "injection_label")[0]["overridden"], json!(true));
    }
}
