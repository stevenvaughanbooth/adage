//! The hook binary with intent routes: fires with the model installed, and fails open — exit 0,
//! no output, structured routes unaffected — when the model is missing or damaged.

use adage_core::Effect;
use adage_intent::model;
use adage_tracks::*;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

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
    Route {
        id: id.into(),
        summary: String::new(),
        trigger,
        step: Step::Run { command: command.into() },
        slots: vec![],
        effect,
        strength: Strength::Prime,
        state: RouteState::Firm,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { support: 5, ..Default::default() },
        authored: false,
    }
}

struct World {
    _d: tempfile::TempDir,
    home: std::path::PathBuf,
    repo: std::path::PathBuf,
}

fn world() -> World {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let (home, repo) = (root.join("home"), root.join("acme-intent"));
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    install_fixture_model(&home.join("models"));
    let m = model::StaticModel::open(&model::dir_in(&home.join("models"))).unwrap();
    let centroid = adage_intent::quantize(&m.embed("fix the failing tests").unwrap());
    let track = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "acme-intent/intents".into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: "acme-intent".into() },
            gauge: Gauge::default(),
        },
        routes: vec![
            route(
                "intent-tests",
                Trigger::Intent { family: "intent-tests".into(), model: MODEL_ID.into(), centroid, threshold: 0.8 },
                "make test",
                Effect::Verify,
            ),
            route("dead-end-x", Trigger::SessionStart, "make test", Effect::Verify),
        ],
    };
    let mut track = track;
    track.routes[1].step =
        Step::Avoid { avoid: "pytest".into(), instead: "make test".into(), signature: "ACME_ENV is not set".into() };
    track.routes[1].strength = Strength::Inhibit;
    io::validate(&track).unwrap();
    Index::compile(&[track]).write_atomic(&home.join("index.json")).unwrap();
    World { _d: d, home, repo }
}

fn hook(w: &World, prompt: &str) -> (String, i32) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_adage"))
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
