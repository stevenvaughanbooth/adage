//! Hook client, harness-agnostic. Reads the harness payload on stdin, translates it into a
//! normalised event through the harness adapter, decides, and prints the adapter's rendering.
//! Returns `Ok(None)` when there is nothing to say, and `Err` when something went wrong: the caller
//! prints nothing in either case (fail open), and records the error in `fail-open.log`.

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

pub fn run(harness: &str, event: &str) -> Result<Option<String>, Failure> {
    let Some(adapter) = harness::adapter(harness) else { return Err(Failure::new("config", "unknown harness")) };
    if std::env::var("ADAGE_DISABLE").is_ok_and(|v| v == "1") {
        return Ok(None);
    }
    let mut buf = String::new();
    std::io::stdin().take(MAX_STDIN).read_to_string(&mut buf).map_err(|e| Failure::io("io", "reading stdin", &e))?;
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
    let index = Index::load(&paths.index());
    let keys: std::collections::BTreeSet<&str> = index.routes.iter().map(|r| r.repo.as_str()).collect();
    let Some(repo_key) = repo::lookup_key(&root, Some(&paths.repo_keys()), &keys) else {
        return Ok(None); // nothing installed for this repository: the common, cheapest path
    };
    // An event this adapter does not act on.
    let Some(ev) = adapter.translate(event, &p, &cwd.to_string_lossy()) else { return Ok(None) };
    // An event that cannot change any decision stops here, before the command table, the state
    // lock and the spool (Design §13.2; `adage_runtime::fast`).
    let state_path = paths.state(&session);
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
    let ctx = Ctx { index: &index, repo: repo_key.clone(), root: &root, table: &table, embed: Some(&embed) };
    let arm_env = std::env::var("ADAGE_ARM").ok();
    let share: f64 = std::env::var("ADAGE_HOLDOUT").ok().and_then(|v| v.parse().ok()).unwrap_or(0.1);
    let spool_path = paths.spool(&session);
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
                _ => assign_arm(&session, share),
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
    if let Err(e) = spool::append(&paths.spool(&session), &rec) {
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
    let max = adapter.max_context();
    if text.len() > max {
        text.truncate(text.char_indices().take_while(|(i, _)| *i < max).last().map(|(i, _)| i).unwrap_or(0));
    }
    Ok(adapter.render(event, Output::Context(text)))
}
