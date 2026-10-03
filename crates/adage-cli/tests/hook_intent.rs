//! The hook binary with intent routes: fires with the model installed, and fails open — exit 0,
//! no output, structured routes unaffected — when the model is missing or damaged.

use adage_core::Effect;
use adage_intent::model;
use adage_tracks::testkit::{self, kinds};
use adage_tracks::*;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

mod common;

const MODEL_ID: &str = "fixture@000000000000";

fn install_fixture_model(models: &Path) {
    let words = ["[UNK]", "fix", "tests", "failing", "docs", "write", "the"];
    let rows: Vec<Vec<f32>> = words
        .iter()
        .map(|w| match *w {
            "fix" | "tests" | "failing" => vec![1.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "docs" | "write" => vec![0.0, 0.0, 1.0, 0.1, 0.0, 0.0, 0.0, 0.0],
            _ => vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.05],
        })
        .collect();
    model::write_installed(
        &model::dir_in(models),
        MODEL_ID,
        8,
        0,
        100,
        &words.join("\n"),
        &model::safetensors_bytes(&rows),
    )
    .unwrap();
}

fn route(id: &str, trigger: Trigger, command: &str, effect: Effect) -> Route {
    testkit::route(id, trigger, Step::Run { command: command.into() })
        .effect(effect)
        .strength(Strength::Prime)
        .until_intent_changes()
        .support(5, 0)
        .build()
}

struct World {
    _w: common::TempWorld,
    home: std::path::PathBuf,
    repo: std::path::PathBuf,
}

fn world() -> World {
    let w = common::TempWorld::new(&[]);
    let (home, repo) = (w.home("home"), w.repo("acme-intent"));
    install_fixture_model(&home.paths.models());
    let m = model::StaticModel::open(&model::dir_in(&home.paths.models())).unwrap();
    let centroid = adage_intent::quantize(&m.embed("fix the failing tests").unwrap());
    let dead_end = kinds::dead_end("dead-end-x", "pytest", "make test", "ACME_ENV is not set")
        .until_intent_changes()
        .support(5, 0);
    let track = testkit::track("acme-intent/intents", "acme-intent")
        .route(route(
            "intent-tests",
            Trigger::Intent { family: "intent-tests".into(), model: MODEL_ID.into(), centroid, threshold: 0.8 },
            "make test",
            Effect::Verify,
        ))
        .route(dead_end)
        .validated();
    home.compile(&[track]);
    World { home: home.paths.root.clone(), repo, _w: w }
}

fn hook(w: &World, prompt: &str) -> (String, i32) {
    let mut c = common::scrub(&mut Command::new(env!("CARGO_BIN_EXE_adage")))
        .args(["hook", "claude", "UserPromptSubmit"])
        .env("ADAGE_HOME", &w.home)
        .env("ADAGE_AUTO_LEARN", "0")
        .env("ADAGE_ARM", "treated")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let payload = serde_json::json!({ "session_id": format!("s-{}", prompt.len()), "cwd": w.repo, "prompt": prompt });
    c.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let out = c.wait_with_output().unwrap();
    assert!(out.stderr.is_empty(), "the hook never writes to stderr");
    (String::from_utf8(out.stdout).unwrap(), out.status.code().unwrap_or(-1))
}

#[test]
fn hook_fires_intent_routes_with_the_model() {
    let w = world();
    let (out, code) = hook(&w, "the tests are failing, fix them");
    assert_eq!(code, 0);
    assert!(out.contains("route=\\\"intent-tests\\\"") && out.contains("make test"), "{out}");
    let (out, _) = hook(&w, "write the docs");
    assert!(!out.contains("intent-tests"), "{out}");
}

#[test]
fn hook_fails_open_without_the_model() {
    let w = world();
    std::fs::remove_dir_all(w.home.join("models")).unwrap();
    let (out, code) = hook(&w, "the tests are failing, fix them");
    assert_eq!(code, 0);
    assert!(!out.contains("intent-tests"), "{out}");
    assert!(out.contains("dead-end-x"), "structured routes still fire: {out}");
}

#[test]
fn hook_fails_open_with_a_damaged_model() {
    let w = world();
    let table = model::dir_in(&w.home.join("models")).join("model.safetensors");
    let bytes = std::fs::read(&table).unwrap();
    std::fs::write(&table, &bytes[..bytes.len() / 2]).unwrap();
    let (out, code) = hook(&w, "the tests are failing, fix them");
    assert_eq!(code, 0);
    assert!(!out.contains("intent-tests") && out.contains("dead-end-x"), "{out}");
    std::fs::write(model::dir_in(&w.home.join("models")).join("vocab.txt"), "\u{0}garbage").unwrap();
    assert_eq!(hook(&w, "fix the failing tests").1, 0);
}

/// Any hook event, as the installed hook receives it.
fn hook_event(w: &World, event: &str, payload: serde_json::Value) -> String {
    let mut c = common::scrub(&mut Command::new(env!("CARGO_BIN_EXE_adage")))
        .args(["hook", "claude", event])
        .env("ADAGE_HOME", &w.home)
        .env("ADAGE_AUTO_LEARN", "0")
        .env("ADAGE_ARM", "treated")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let out = c.wait_with_output().unwrap();
    assert!(out.status.success() && out.stderr.is_empty(), "{out:?}");
    String::from_utf8(out.stdout).unwrap()
}

/// A prompt just below a procedure card's threshold: the card is said at the session's first edit
/// of one of its files, through the installed hook (whose no-match fast path must not skip that
/// edit), and not at an edit of another file.
#[test]
fn a_card_waiting_for_an_edit_is_said_at_that_edit_through_the_hook() {
    use adage_tracks::card::{Card, CardFile};
    let w = world();
    let m = model::StaticModel::open(&model::dir_in(&w.home.join("models"))).unwrap();
    let library = m.embed("fix the failing tests").unwrap();
    let grey = "write the fix";
    let score = adage_intent::cosine(&m.embed(grey).unwrap(), &library);
    let threshold = score + 0.05;
    assert!(threshold < 0.99 && score >= threshold - adage_tracks::card::GREY_MARGIN, "{score}");
    let q = adage_intent::quantize(&library);
    let card = Card {
        sessions: 2,
        passed: 2,
        files: vec![CardFile { path: "flag_<x>.go".into(), sessions: 2 }],
        exemplars: vec![],
        commands: vec![],
        avoid: vec![],
        finish: None,
        prompts: vec![q.clone()],
    };
    let mut r = route(
        "card-grey",
        Trigger::Intent { family: "card-grey".into(), model: MODEL_ID.into(), centroid: q, threshold },
        "make test",
        Effect::Observe,
    );
    r.step = Step::Card { card };
    let track = testkit::track("acme-intent/learned", "acme-intent").route(r).validated();
    common::LearnedHome::at(&w.home).compile(&[track]);

    let sid = "s-card-grey";
    let edit = |file: &str| {
        serde_json::json!({
            "session_id": sid, "cwd": w.repo, "hook_event_name": "PostToolUse", "tool_name": "Edit",
            "tool_use_id": format!("t-{file}"),
            "tool_input": {"file_path": w.repo.join(file), "old_string": "a", "new_string": "b"},
            "tool_response": {"filePath": w.repo.join(file)}
        })
    };
    let prompt =
        serde_json::json!({ "session_id": sid, "cwd": w.repo, "hook_event_name": "UserPromptSubmit", "prompt": grey });
    let said = |out: &str| out.contains("route=\\\"card-grey\\\"");
    assert!(!said(&hook_event(&w, "UserPromptSubmit", prompt)), "below the threshold: not at the prompt");
    assert!(!said(&hook_event(&w, "PostToolUse", edit("main.go"))), "not one of the card's files");
    let out = hook_event(&w, "PostToolUse", edit("flag_port.go"));
    assert!(said(&out), "at the first edit of a card file: {out}");
    assert!(!said(&hook_event(&w, "PostToolUse", edit("flag_url.go"))), "once");
}
