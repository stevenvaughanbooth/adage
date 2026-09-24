//! Hook client, harness-agnostic. Reads the harness payload on stdin, translates it into a
//! normalised event through the harness adapter, decides, and prints the adapter's rendering.
//! Returns `Ok(None)` when there is nothing to say, and `Err` when something went wrong: the caller
//! prints nothing in either case (fail open), and records the error in `fail-open.log`.
//!
//! When an event ends a turn or a session, the hook then asks for the repository to be learned
//! (`crate::autolearn`): that never changes what it prints, and a failure there is recorded in
//! `fail-open.log` and ignored.

use crate::autolearn;
use crate::failopen::{self, Failure};
use crate::harness::{self, Output};
use adage_core::{repo, CommandTable};
use adage_intent::model::{self, StaticModel};
use adage_intent::normalize::normalize;
use adage_runtime::{assign_arm, decide, fast, rebuild, render, Arm, Ctx, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::Index;
use serde_json::{json, Value};
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_STDIN: u64 = 8 << 20;

/// The payload, at most [`MAX_STDIN`] bytes. A larger one is refused, but read to the end first
/// and discarded (Design §13.3: the hook always drains its input), so the harness writing it never
/// sees a broken pipe.
fn read_payload(input: &mut impl Read) -> Result<String, Failure> {
    let mut bytes = Vec::new();
    (&mut *input).take(MAX_STDIN + 1).read_to_end(&mut bytes).map_err(|e| Failure::io("io", "reading stdin", &e))?;
    if bytes.len() as u64 > MAX_STDIN {
        let _ = std::io::copy(input, &mut std::io::sink());
        return Err(Failure::new("payload", "stdin exceeds 8 MiB"));
    }
    String::from_utf8(bytes).map_err(|_| Failure::new("payload", "stdin is not UTF-8"))
}

pub fn run(harness: &str, event: &str) -> Result<Option<String>, Failure> {
    // Read (or drain) stdin first, whatever happens next.
    let payload = read_payload(&mut std::io::stdin().lock());
    let Some(adapter) = harness::adapter(harness) else { return Err(Failure::new("config", "unknown harness")) };
    if std::env::var("ADAGE_DISABLE").is_ok_and(|v| v == "1") {
        return Ok(None);
    }
    let buf = payload?;
    #[cfg(debug_assertions)]
    if std::env::var("ADAGE_TEST_FAULT").is_ok_and(|v| v == "panic") {
        // Fault injection for tests (debug builds only). The message quotes the payload, as a real
        // panic message can: it must never reach the log.
        panic!("injected fault while handling {buf}");
    }
    let p: Value = serde_json::from_str(&buf)
        .map_err(|e| Failure::new("payload", format!("stdin is not a JSON payload ({:?})", e.classify())))?;
    let Some(harness::Envelope { session, cwd }) = adapter.envelope(&p) else {
        return Err(Failure::new("payload", "no session id or working directory"));
    };
    let root = repo::find_root(&cwd).unwrap_or_else(|| cwd.clone());
    if root.join(".adage/disabled").exists() {
        return Ok(None);
    }
    let paths = Paths::from_env();
    let out = respond(adapter, harness, event, &p, &session, &cwd, &root, &paths);
    if adapter.is_learn_point(event) {
        let transcript = p.get("transcript_path").and_then(|v| v.as_str());
        if let Err(f) = autolearn::on_turn_end(&paths, &root, adapter.name(), transcript) {
            failopen::record(&paths.root, harness, event, &f);
        }
    }
    out
}

/// Decide and render: everything after the kill switches.
#[allow(clippy::too_many_arguments)]
fn respond(
    adapter: &dyn harness::Adapter,
    harness: &str,
    event: &str,
    p: &Value,
    session: &str,
    cwd: &std::path::Path,
    root: &std::path::Path,
    paths: &Paths,
) -> Result<Option<String>, Failure> {
    let index = Index::load(&paths.index());
    let keys: std::collections::BTreeSet<&str> = index.routes.iter().map(|r| r.repo.as_str()).collect();
    let Some(repo_key) = repo::lookup_key(root, Some(&paths.repo_keys()), &keys) else {
        return Ok(None); // nothing installed for this repository: the common, cheapest path
    };
    // An event this adapter does not act on.
    let Some(ev) = adapter.translate(event, p, &cwd.to_string_lossy()) else { return Ok(None) };
    // An event that cannot change any decision stops here, before the command table, the state
    // lock and the spool (Design §13.2; `adage_runtime::fast`).
    let state_path = paths.state(session);
    let read_state = || state::try_read::<SessionState>(&state_path).ok().flatten();
    if fast::skip(&index.triggers(&repo_key), &ev, &cwd.to_string_lossy(), read_state) {
        return Ok(None);
    }
    let table = CommandTable::load();
    // The embedding model is opened only if the runtime asks, which it does only at a prompt in a
    // repository with intent routes. Missing or damaged model: no vector, no intent directive.
    let models = paths.models();
    let embed = move |text: &str| -> Option<(String, Vec<f32>)> {
        let m = StaticModel::open(&model::dir_in(&models)).ok()?;
        let v = m.embed(&normalize(text))?;
        Some((m.id, v))
    };
    let ctx = Ctx { index: &index, repo: repo_key.clone(), root, table: &table, embed: Some(&embed) };
    let arm_env = std::env::var("ADAGE_ARM").ok();
    // ADAGE_HOLDOUT, else `runtime.holdout` in config.toml, else 0.1.
    let share: f64 = adage_store::config::load(paths).config.runtime_holdout;
    let spool_path = paths.spool(session);
    // A state that can no longer be read is rebuilt from the session's spool, if it is small
    // enough to replay quickly (it is read under the state lock).
    let rebuild = || {
        let small = std::fs::metadata(&spool_path).is_ok_and(|m| m.len() <= rebuild::MAX_SPOOL_BYTES);
        let records: Vec<Value> = if small { spool::read_all(&spool_path).ok()? } else { return None };
        rebuild::rebuild(&records, &index, &repo_key, &table)
    };
    let (decision, arm) = state::update_or_rebuild::<SessionState, _>(&state_path, rebuild, |st| {
        if st.arm.is_none() {
            st.arm = Some(match arm_env.as_deref() {
                Some("holdout") => Arm::Holdout,
                Some("treated") => Arm::Treated,
                _ => assign_arm(session, share),
            });
            st.repo = Some(repo_key.clone());
        }
        (decide(&ctx, st, &ev), st.arm)
    })
    .map_err(|e| Failure::io("state", "updating the session state", &e))?;

    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let mut rec = json!({ "ts": ts, "event": event, "kind": ev.name(), "arm": arm, "repo": repo_key, "generation": index.generation, "decision": decision });
    if adapter.name() != "claude" {
        rec["harness"] = json!(adapter.name());
    }
    if let Err(e) = spool::append(&paths.spool(session), &rec) {
        // Measurement is lost, but the decision stands: record it and carry on.
        failopen::record(&paths.root, harness, event, &Failure::io("spool", "appending to the session spool", &e));
    }

    if let Some(reason) = decision.continue_reason {
        return Ok(adapter.render(event, Output::Continue(reason)));
    }
    if decision.directives.is_empty() {
        return Ok(None);
    }
    let mut text = decision
        .directives
        .iter()
        .map(|d| render::wrap(&d.track, &d.version, &d.route, d.aspect, &d.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    // Design W8 (spill overflow to a file) cannot arise: the longest decision validation allows
    // fits every harness's cap (`the_largest_decision_fits_every_harness_cap`). This is a backstop.
    let max = adapter.max_context();
    if text.len() > max {
        text.truncate(text.char_indices().take_while(|(i, _)| *i < max).last().map(|(i, _)| i).unwrap_or(0));
    }
    Ok(adapter.render(event, Output::Context(text)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::spans::SpanKind;
    use adage_core::Effect;
    use adage_runtime::MAX_DIRECTIVES_PER_DECISION;
    use adage_tracks::*;

    fn route(trigger: Trigger, step: Step, slots: Vec<Slot>, effect: Effect, aspect: Aspect) -> Route {
        Route {
            id: "r".repeat(64),
            summary: String::new(),
            trigger,
            step,
            slots,
            effect,
            aspect,
            state: RouteState::Active,
            exit: vec![Exit::IntentChanges, Exit::ExploratoryChange],
            evidence: Evidence::default(),
            authored: false,
        }
    }

    fn track(routes: Vec<Route>) -> Track {
        Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: format!("{}/{}", "a".repeat(63), "b".repeat(64)),
                    version: format!("{}.0.0", "1".repeat(196)),
                    description: String::new(),
                    provenance: "desire-path".into(),
                },
                scope: Scope { repo: "r".into() },
                gauge: Gauge::default(),
            },
            routes,
        }
    }

    /// Design W8 says overflow is spilled to a file. It cannot happen: with the longest fields
    /// validation accepts, three directives (the most one decision gives) fit every harness's cap,
    /// so the truncation in `run` is a backstop that never fires. If a limit is raised, this fails
    /// and the spill must be built.
    #[test]
    fn the_largest_decision_fits_every_harness_cap() {
        let cmd = |n: usize| format!("m{}", "x".repeat(n - 1));
        // 160 bytes, safe_text's limit, in two-byte characters.
        let signature = "é".repeat(80);
        let slot = |name: &str, source: SlotSource, contract: Contract| Slot { name: name.into(), source, contract };
        let item = slot(
            "item",
            SlotSource::Prompt { span: SpanKind::Ticket },
            Contract::Pattern { regex: r"^[a-z0-9]{1,200}$".into() },
        );
        let files = slot("files", SlotSource::ResultField { field: "f".into() }, Contract::RepoPath);
        let routes = vec![
            route(
                Trigger::Prompt { span: SpanKind::Ticket },
                Step::Run { command: format!("{} {{item}}", cmd(193)) },
                vec![item],
                Effect::Environment,
                Aspect::Yellow,
            ),
            route(
                Trigger::AfterEdit { paths: format!("{}/*.json", "s".repeat(113)) },
                Step::Run { command: cmd(200) },
                vec![],
                Effect::Environment,
                Aspect::Yellow,
            ),
            route(
                Trigger::AfterCommand { command: "exec x".into() },
                Step::ReadBatch { slot: "files".into(), max: 10 },
                vec![files],
                Effect::Observe,
                Aspect::Yellow,
            ),
            route(
                Trigger::SessionStart,
                Step::Avoid { avoid: cmd(200), instead: format!("pytest t{}", "x".repeat(192)), signature },
                vec![],
                Effect::Verify,
                Aspect::Red,
            ),
        ];
        for r in &routes {
            io::validate(&track(vec![r.clone()])).unwrap_or_else(|e| panic!("the longest fields must be valid: {e:#}"));
        }
        // One more of anything is refused.
        let refused = |mutate: &dyn Fn(&mut Track)| {
            let mut t = track(vec![routes[3].clone()]);
            mutate(&mut t);
            assert!(io::validate(&t).is_err());
        };
        refused(&|t| t.manifest.track.name.push('b'));
        refused(&|t| t.routes[0].id.push('r'));
        refused(&|t| t.routes[0].step = Step::Avoid { avoid: cmd(201), instead: "m".into(), signature: "s".into() });
        refused(&|t| {
            t.routes[0].step = Step::Avoid { avoid: "m".into(), instead: "m".into(), signature: "é".repeat(81) }
        });
        let mut edit = track(vec![routes[1].clone()]);
        edit.routes[0].trigger = Trigger::AfterEdit { paths: format!("{}/*.json", "s".repeat(114)) };
        assert!(io::validate(&edit).is_err());
        let mut batch = track(vec![routes[2].clone()]);
        batch.routes[0].step = Step::ReadBatch { slot: "files".into(), max: 11 };
        assert!(io::validate(&batch).is_err());

        // The longest text each route kind renders, slots at their longest: a bound value is a
        // plain token of at most 200 bytes, a listed file shorter than 200 bytes.
        let t = track(vec![]);
        let longest = routes
            .iter()
            .map(|r| {
                let bound: Vec<(String, String)> = match &r.step {
                    Step::ReadBatch { max, .. } => vec![("files".into(), vec!["f".repeat(199); *max].join(", "))],
                    _ => vec![("item".into(), "i".repeat(200))],
                };
                let body = render::route_text(r, &bound).expect("renders");
                render::wrap(&t.manifest.track.name, &t.manifest.track.version, &r.id, r.aspect, &body).len()
            })
            .max()
            .unwrap();
        let decision = MAX_DIRECTIVES_PER_DECISION * longest + (MAX_DIRECTIVES_PER_DECISION - 1) * 2;
        eprintln!("longest directive {longest} bytes; largest decision {decision} bytes");
        for h in ["claude", "opencode", "codex"] {
            let cap = harness::adapter(h).unwrap().max_context();
            assert!(decision <= cap, "{h}: {decision} bytes of directives can exceed the {cap}-byte cap");
        }
    }
}
