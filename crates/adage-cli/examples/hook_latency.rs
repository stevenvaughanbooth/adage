//! Hook latency with and without intent routes (the hot-path budget, spike S1 and Design §13.2),
//! and, since Track G G4, tool batches in a repository with context triggers.
//!
//! ```text
//! cargo build --release -p adage-cli --examples
//! target/release/examples/hook_latency <adage binary> <dir with the pinned model files | -> [runs]
//! ```
//! Installs the real model from the directory into a scratch `ADAGE_HOME` (SHA-256 verified),
//! writes an index with a structured route and an intent route whose centroid is the model's
//! embedding of a phrase, then spawns `adage hook claude <event>` `runs` times per scenario, exec
//! form, payload on stdin, timing spawn to exit. Prints JSON. With `-` for the model directory,
//! the scenarios that need the model are left out. `<binary a>,<binary b>` compares two builds:
//! each run spawns both, in alternating order, and each scenario reports both.
//!
//! | Scenario | Index | Event |
//! | --- | --- | --- |
//! | `no_routes_for_repo` | routes for another repository | a prompt |
//! | `structured_routes_only` | a lookup | a prompt it does not match (the fast path) |
//! | `structured_routes_ticket_prompt` | a lookup | a prompt naming a work item (it fires) |
//! | `intent_routes_model_installed` (+ `_ticket_prompt`) | a lookup and an intent route | a prompt (the model embeds it) |
//! | `intent_routes_model_missing` | the same, nothing installed (the bundled model embeds; its id differs) | a prompt |
//! | `intent_routes_bundled_model` | an intent route learned with the bundled model, nothing installed | a prompt (it fires) |
//! | `intent_routes_no_model` | the same, the hashed fallback selected | a prompt |
//! | `batch_no_match` | a lookup, a finish line and three context routes | a batch of `ls` (the fast path) |
//! | `batch_context_fires` | the same | a batch editing `registry/a.csv` then running `make gen`: a context route fires |
//! | `turn_step_nothing_pending` | the same | a `TurnStep` (the Claude Code mod's step boundary) in a fresh session: nothing directed, so the fast path; compare with `batch_no_match` |
//!
//! `turn_step_predicts` (a pending directive under a seeded `predict` grant, run against
//! `adage-exec`) waits for the grant itself (plan chunk C2): `adage hook` has no extension, so it
//! never predicts, and this example cannot inject one into a spawned binary.

use adage_core::spans::SpanKind;
use adage_core::Effect;
use adage_intent::model;
use adage_tracks::*;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

fn route(id: &str, trigger: Trigger, step: Step, slots: Vec<Slot>, effect: Effect) -> Route {
    Route {
        id: id.into(),
        summary: String::new(),
        trigger,
        step,
        slots,
        effect,
        strength: Strength::Prime,
        state: RouteState::Firm,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { support: 5, ..Default::default() },
        authored: false,
    }
}

fn lookup() -> Route {
    route(
        "lookup-work-item",
        Trigger::Prompt { span: SpanKind::Ticket },
        Step::Run { command: "python3 scripts/wi-lookup {item}".into() },
        vec![Slot {
            name: "item".into(),
            source: SlotSource::Prompt { span: SpanKind::Ticket },
            contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
        }],
        Effect::Observe,
    )
}

/// A finish line and three context routes (Track G G4).
fn with_contexts() -> Vec<Route> {
    let act = |k: &str| CtxItem::Act { keys: vec![k.into()] };
    let ctx = |id: &str, items: Vec<CtxItem>, cmd: &str| {
        route(id, Trigger::Context { items, within: 8 }, Step::Run { command: cmd.into() }, vec![], Effect::Verify)
    };
    vec![
        lookup(),
        route(
            "finish-line-gen",
            Trigger::AfterEdit { paths: "schema/*.json".into() },
            Step::Run { command: "make gen".into() },
            vec![],
            Effect::Regenerate,
        ),
        ctx("context-check", vec![CtxItem::Edit { glob: "registry/*.csv".into() }, act("exec make gen")], "make check"),
        ctx("context-lint", vec![act("exec make fmt"), act("exec make lint")], "make check"),
        ctx(
            "context-three",
            vec![act("exec make deps"), CtxItem::Edit { glob: "requirements.txt".into() }, act("exec make lock")],
            "make check",
        ),
    ]
}

fn index(home: &Path, repo: &str, routes: Vec<Route>) {
    let t = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: format!("{repo}/bench"),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: repo.into() },
            gauge: Gauge::default(),
        },
        routes,
    };
    io::validate(&t).unwrap();
    Index::compile(&[t]).write_atomic(&home.join("index.json")).unwrap();
}

fn intent(centroid: Vec<i16>, model_id: &str) -> Route {
    route(
        "intent-tests",
        Trigger::Intent { family: "intent-tests".into(), model: model_id.into(), centroid, threshold: 0.7 },
        Step::Run { command: "make test".into() },
        vec![],
        Effect::Verify,
    )
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[(((v.len() - 1) as f64) * p).round() as usize]
}

/// Time `runs` hook events per binary. With several binaries (to compare two builds on a busy
/// machine), each run spawns every binary in turn, in an order that alternates, so contention
/// falls on all of them alike. Returns each binary's statistics and its last output.
fn time(
    bins: &[&str],
    home: &Path,
    repo: &Path,
    event: &str,
    extra: serde_json::Value,
    runs: usize,
) -> (serde_json::Value, String) {
    let mut ms = vec![Vec::with_capacity(runs); bins.len()];
    let mut last = vec![String::new(); bins.len()];
    for i in 0..runs + 20 {
        let mut payload = serde_json::json!({ "session_id": format!("bench-{i}"), "cwd": repo });
        payload.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let payload = payload.to_string();
        for k in 0..bins.len() {
            let b = if i % 2 == 0 { k } else { bins.len() - 1 - k };
            let t = Instant::now();
            let mut c = Command::new(bins[b])
                .args(["hook", "claude", event])
                .env("ADAGE_HOME", home)
                .env("ADAGE_ARM", "treated")
                .env("ADAGE_AUTO_LEARN", "0")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            c.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
            let out = c.wait_with_output().unwrap();
            let e = t.elapsed().as_secs_f64() * 1e3;
            if i >= 20 {
                ms[b].push(e); // the first 20 warm the page cache
            }
            last[b] = String::from_utf8_lossy(&out.stdout).to_string();
            let _ = std::fs::remove_dir_all(home.join("sessions"));
        }
    }
    let stats = |ms: &mut Vec<f64>| {
        let mean = ms.iter().sum::<f64>() / ms.len() as f64;
        serde_json::json!({ "runs": runs, "p50_ms": pct(ms, 0.5), "p95_ms": pct(ms, 0.95), "p99_ms": pct(ms, 0.99), "max_ms": pct(ms, 1.0), "mean_ms": mean })
    };
    if bins.len() == 1 {
        return (stats(&mut ms[0]), last.remove(0));
    }
    let all: serde_json::Map<String, serde_json::Value> =
        bins.iter().zip(ms.iter_mut()).map(|(b, m)| (b.to_string(), stats(m))).collect();
    (serde_json::Value::Object(all), last.pop().unwrap_or_default())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // `a,b` compares two binaries, run by turns (the last one's output is checked).
    let bins: Vec<&str> = args[1].split(',').collect();
    let (bin, src) = (bins.as_slice(), args[2].as_str());
    let runs: usize = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(300);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let (home, bare, repo) = (root.join("home"), root.join("home-no-model"), root.join("acme-bench"));
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bare).unwrap();
    let prompt = serde_json::json!({ "prompt": "fix the failing tests please" });
    let ticket = serde_json::json!({ "prompt": "W-2042 please" });
    let mut out = serde_json::Map::new();

    // With the model: an intent centroid from it. Without: a fixed one (no model, no match).
    let (centroid, model_id) = if src == "-" {
        (vec![625i16; 256], model::pinned_id()) // a unit vector, in units of 1e-4
    } else {
        model::install(Path::new(src), &home.join("models")).expect("install the pinned model");
        let t = Instant::now();
        let m = model::StaticModel::open(&model::dir_in(&home.join("models"))).unwrap();
        out.insert("in_process_open_ms".into(), (t.elapsed().as_secs_f64() * 1e3).into());
        let t = Instant::now();
        let v = m.embed(&adage_intent::normalize::normalize("the tests are failing, can you fix them")).unwrap();
        out.insert("in_process_embed_ms".into(), (t.elapsed().as_secs_f64() * 1e3).into());
        (adage_intent::quantize(&v), m.id.clone())
    };

    index(&home, "someone-else", vec![lookup()]);
    out.insert("no_routes_for_repo".into(), time(bin, &home, &repo, "UserPromptSubmit", prompt.clone(), runs).0);

    index(&home, "acme-bench", vec![lookup()]);
    out.insert("structured_routes_only".into(), time(bin, &home, &repo, "UserPromptSubmit", prompt.clone(), runs).0);
    let (r, last) = time(bin, &home, &repo, "UserPromptSubmit", ticket.clone(), runs);
    assert!(last.contains("wi-lookup W-2042"), "{last}");
    out.insert("structured_routes_ticket_prompt".into(), r);

    if src != "-" {
        index(&home, "acme-bench", vec![lookup(), intent(centroid.clone(), &model_id)]);
        let (r, last) = time(bin, &home, &repo, "UserPromptSubmit", prompt.clone(), runs);
        assert!(last.contains("make test"), "the intent route must fire: {last}");
        out.insert("intent_routes_model_installed".into(), r);
        let (r, last) = time(bin, &home, &repo, "UserPromptSubmit", ticket, runs);
        assert!(last.contains("wi-lookup W-2042"), "{last}");
        out.insert("intent_routes_model_installed_ticket_prompt".into(), r);
    }

    // Nothing installed: the model bundled in the binary embeds (a different id from the
    // installed f32 model's, so an index learned with that one does not match).
    index(&bare, "acme-bench", vec![lookup(), intent(centroid, &model_id)]);
    let (r, last) = time(bin, &bare, &repo, "UserPromptSubmit", prompt.clone(), runs);
    assert!(!last.contains("make test"), "another model's intent route does not fire");
    out.insert("intent_routes_model_missing".into(), r);
    // An index learned with the bundled model: it fires with nothing installed.
    if let Some(b) = model::bundled() {
        let v = b.embed(&adage_intent::normalize::normalize("the tests are failing, can you fix them")).unwrap();
        index(&bare, "acme-bench", vec![lookup(), intent(adage_intent::quantize(&v), &b.id)]);
        let (r, last) = time(bin, &bare, &repo, "UserPromptSubmit", prompt.clone(), runs);
        out.insert("intent_routes_bundled_model".into(), r);
        out.insert("intent_routes_bundled_model_fired".into(), last.contains("make test").into());
    }
    // No model at all (the hashed fallback selected): intent routes cannot fire.
    std::fs::create_dir_all(bare.join("models")).unwrap();
    std::fs::write(bare.join("models").join(model::ACTIVE_FILE), "hashed").unwrap();
    let (r, last) = time(bin, &bare, &repo, "UserPromptSubmit", prompt, runs);
    assert!(!last.contains("make test"), "no model, no intent directive");
    out.insert("intent_routes_no_model".into(), r);

    // Tool batches with context routes in the index.
    index(&home, "acme-bench", with_contexts());
    let call = |tool: &str, input: serde_json::Value| serde_json::json!({ "tool_name": tool, "tool_input": input, "tool_response": "ok" });
    let ls = serde_json::json!({ "tool_calls": [call("Bash", serde_json::json!({ "command": "ls -la" }))] });
    let (r, last) = time(bin, &home, &repo, "PostToolBatch", ls, runs);
    assert!(last.trim().is_empty(), "{last}");
    out.insert("batch_no_match".into(), r);
    let step = serde_json::json!({ "hook_event_name": "TurnStep", "turn_id": "bench", "index": 1, "model": "claude-bench", "ending": true });
    let (r, last) = time(bin, &home, &repo, "TurnStep", step, runs);
    assert!(last.trim().is_empty(), "nothing is directed, so nothing is predicted: {last}");
    out.insert("turn_step_nothing_pending".into(), r);
    let file = repo.join("registry/a.csv");
    let fires = serde_json::json!({ "tool_calls": [
        call("Edit", serde_json::json!({ "file_path": file, "old_string": "a", "new_string": "b" })),
        call("Bash", serde_json::json!({ "command": "make gen" })),
    ] });
    let (r, last) = time(bin, &home, &repo, "PostToolBatch", fires, runs);
    // A binary from before G4 decides the batch in full, but has no context trigger to fire.
    out.insert("batch_context_fires".into(), r);
    out.insert("batch_context_fired".into(), last.contains("context-check").into());

    println!("{}", serde_json::to_string_pretty(&serde_json::Value::Object(out)).unwrap());
}
