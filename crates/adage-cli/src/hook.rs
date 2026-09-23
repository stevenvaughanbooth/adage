//! Hook client, harness-agnostic. Reads the harness payload on stdin, translates it into a
//! normalised event through the harness adapter, decides, and prints the adapter's rendering.
//! Returns `None` for "no output" — including on every error path (fail open).

use crate::harness::{self, Output};
use adage_core::{repo, CommandTable};
use adage_intent::model::{self, StaticModel};
use adage_intent::normalize::normalize;
use adage_runtime::{assign_arm, decide, render, Arm, Ctx, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::Index;
use serde_json::{json, Value};
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_STDIN: u64 = 8 << 20;

pub fn run(harness: &str, event: &str) -> Option<String> {
    let adapter = harness::adapter(harness)?;
    if std::env::var("ADAGE_DISABLE").is_ok_and(|v| v == "1") {
        return None;
    }
    let mut buf = String::new();
    std::io::stdin().take(MAX_STDIN).read_to_string(&mut buf).ok()?;
    let p: Value = serde_json::from_str(&buf).ok()?;
    let harness::Envelope { session, cwd } = adapter.envelope(&p)?;
    let root = repo::find_root(&cwd).unwrap_or_else(|| cwd.clone());
    if root.join(".adage/disabled").exists() {
        return None;
    }
    let paths = Paths::from_env();
    let index = Index::load(&paths.index());
    let repo_key = repo::repo_key(&root);
    index.for_repo(&repo_key).next()?; // nothing installed for this repository: the common, cheapest path
    let ev = adapter.translate(event, &p, &cwd.to_string_lossy())?;
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
    let (decision, arm) = state::update::<SessionState, _>(&paths.state(&session), |st| {
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
    .ok()?;

    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let mut rec = json!({ "ts": ts, "event": event, "arm": arm, "generation": index.generation, "decision": decision });
    if adapter.name() != "claude" {
        rec["harness"] = json!(adapter.name());
    }
    let _ = spool::append(&paths.spool(&session), &rec);

    if let Some(reason) = decision.continue_reason {
        return adapter.render(event, Output::Continue(reason));
    }
    if decision.directives.is_empty() {
        return None;
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
    adapter.render(event, Output::Context(text))
}
