//! Hook latency with and without intent routes (the hot-path budget, spike S1 and Design §13.2).
//!
//! ```text
//! cargo build --release -p adage-cli --examples
//! target/release/examples/hook_latency <adage binary> <dir with the pinned model files> [runs]
//! ```
//! Installs the real model from the directory into a scratch `ADAGE_HOME` (SHA-256 verified),
//! writes an index with a structured route and an intent route whose centroid is the model's
//! embedding of a phrase, then spawns `adage hook claude UserPromptSubmit` `runs` times per
//! scenario, exec form, payload on stdin, timing spawn to exit. Prints JSON.

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
        aspect: Aspect::Yellow,
        state: RouteState::Active,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { support: 5, ..Default::default() },
        authored: false,
    }
}

fn index(home: &Path, repo: &str, with_intent: bool, centroid: Vec<i16>, model_id: &str) {
    let mut routes = vec![route(
        "lookup-work-item",
        Trigger::Prompt { span: SpanKind::Ticket },
        Step::Run { command: "python3 scripts/wi-lookup {item}".into() },
        vec![Slot {
            name: "item".into(),
            source: SlotSource::Prompt { span: SpanKind::Ticket },
            contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
        }],
        Effect::Observe,
    )];
    if with_intent {
        routes.push(route(
            "intent-tests",
            Trigger::Intent { family: "intent-tests".into(), model: model_id.into(), centroid, threshold: 0.7 },
            Step::Run { command: "make test".into() },
            vec![],
            Effect::Verify,
        ));
    }
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

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[(((v.len() - 1) as f64) * p).round() as usize]
}

fn time(bin: &str, home: &Path, repo: &Path, prompt: &str, runs: usize) -> (serde_json::Value, String) {
    let mut ms = Vec::with_capacity(runs);
    let mut last = String::new();
    for i in 0..runs + 20 {
        let payload =
            serde_json::json!({ "session_id": format!("bench-{i}"), "cwd": repo, "prompt": prompt }).to_string();
        let t = Instant::now();
        let mut c = Command::new(bin)
            .args(["hook", "claude", "UserPromptSubmit"])
            .env("ADAGE_HOME", home)
            .env("ADAGE_ARM", "treated")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
        let out = c.wait_with_output().unwrap();
        let e = t.elapsed().as_secs_f64() * 1e3;
        if i >= 20 {
            ms.push(e); // the first 20 warm the page cache
        }
        last = String::from_utf8_lossy(&out.stdout).to_string();
        let _ = std::fs::remove_dir_all(home.join("sessions"));
    }
    let mean = ms.iter().sum::<f64>() / ms.len() as f64;
    (
        serde_json::json!({ "runs": runs, "p50_ms": pct(&mut ms, 0.5), "p95_ms": pct(&mut ms, 0.95), "p99_ms": pct(&mut ms, 0.99), "max_ms": pct(&mut ms, 1.0), "mean_ms": mean }),
        last,
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (bin, src) = (&args[1], Path::new(&args[2]));
    let runs: usize = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(300);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let (home, bare, repo) = (root.join("home"), root.join("home-no-model"), root.join("acme-bench"));
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(&bare).unwrap();
    model::install(src, &home.join("models")).expect("install the pinned model");

    let t = Instant::now();
    let m = model::StaticModel::open(&model::dir_in(&home.join("models"))).unwrap();
    let open_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let v = m.embed(&adage_intent::normalize::normalize("the tests are failing, can you fix them")).unwrap();
    let embed_ms = t.elapsed().as_secs_f64() * 1e3;
    let centroid = adage_intent::quantize(&v);

    let prompt = "fix the failing tests please";
    let mut out = serde_json::Map::new();
    out.insert("in_process_open_ms".into(), open_ms.into());
    out.insert("in_process_embed_ms".into(), embed_ms.into());

    index(&home, "someone-else", false, centroid.clone(), &m.id);
    out.insert("no_routes_for_repo".into(), time(bin, &home, &repo, prompt, runs).0);

    index(&home, "acme-bench", false, centroid.clone(), &m.id);
    out.insert("structured_routes_only".into(), time(bin, &home, &repo, prompt, runs).0);

    index(&home, "acme-bench", true, centroid.clone(), &m.id);
    let (r, last) = time(bin, &home, &repo, prompt, runs);
    assert!(last.contains("make test"), "the intent route must fire: {last}");
    out.insert("intent_routes_model_installed".into(), r);
    let (r, last) = time(bin, &home, &repo, "W-2042 please", runs);
    assert!(last.contains("wi-lookup W-2042"), "{last}");
    out.insert("intent_routes_model_installed_ticket_prompt".into(), r);

    index(&bare, "acme-bench", true, centroid, &m.id);
    let (r, last) = time(bin, &bare, &repo, prompt, runs);
    assert!(!last.contains("make test"), "no model, no intent directive");
    out.insert("intent_routes_model_missing".into(), r);

    println!("{}", serde_json::to_string_pretty(&serde_json::Value::Object(out)).unwrap());
}
