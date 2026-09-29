//! `adage-exec hook` against `adage hook` (Track C §3.2, §6 L0): on events that match nothing,
//! `adage-exec hook` must stay within 1 ms at p95 of `adage hook`; and what an E1 prefetch adds.
//!
//! ```text
//! cargo build --release -p adage-cli -p adage-exec --examples
//! target/release/examples/exec_latency target/release/adage target/release/adage-exec [runs]
//! ```
//! Scenarios, each timed spawn to exit, exec form, payload on stdin, the two binaries interleaved:
//! - `no-repo`: a repository with nothing installed (the cheapest path);
//! - `no-match`: a prompt with no work item in a repository whose index has routes;
//! - `e1` (adage-exec only, when a sandbox is available): the lookup route approved at E1, a
//!   prompt naming W-2042, the lookup (`sh scripts/lookup {id}`) run in the sandbox.
//!
//! Prints JSON: p50 and p95 per binary and scenario, and the p95 difference.

use adage_core::spans::SpanKind;
use adage_core::{CommandTable, Effect};
use adage_exec::approvals::{self, Approval, ApprovalKey, Store};
use adage_exec::request::{FieldSpec, Level};
use adage_exec::route::{self, Terms};
use adage_tracks::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

fn setup(root: &Path) {
    let repo = root.join("acme");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("scripts")).unwrap();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::create_dir_all(root.join("plain/.git")).unwrap();
    std::fs::create_dir_all(root.join("home")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "fn main() {}\n").unwrap();
    std::fs::write(repo.join("scripts/lookup"), "echo '{\"files\":[\"src/a.rs\"],\"state\":\"Active\"}'\n").unwrap();
    #[cfg(unix)]
    for p in [root.to_path_buf(), repo.clone(), repo.join("scripts"), repo.join("src")] {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
    }
    let t = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "acme/work".into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes: vec![Route {
            id: "lookup-work-item".into(),
            summary: String::new(),
            trigger: Trigger::Prompt { span: SpanKind::Ticket },
            step: Step::Run { command: "sh scripts/lookup {id}".into() },
            slots: vec![Slot {
                name: "id".into(),
                source: SlotSource::Prompt { span: SpanKind::Ticket },
                contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
            }],
            effect: Effect::Observe,
            strength: Strength::Fire,
            state: RouteState::Firm,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence { support: 5, ..Default::default() },
            authored: false,
        }],
    };
    let home = root.join("adage");
    std::fs::create_dir_all(&home).unwrap();
    let index = Index::compile(&[t]);
    index.write_atomic(&home.join("index.json")).unwrap();
    let ir = route::find(&index, "acme", "acme/work", "lookup-work-item").unwrap();
    let fields: BTreeMap<String, FieldSpec> = serde_json::from_value(json!({
        "files": { "type": "repo-path-list", "max": 5 },
        "state": { "type": "enum", "values": ["Active", "Closed"] }
    }))
    .unwrap();
    let terms = Terms { fields, ..Default::default() };
    let plan =
        route::plan(ir, &index, &repo, &home.join("tracks"), Level::E1, Some(&terms), &CommandTable::load()).unwrap();
    let mut store = Store::default();
    store.approve(Approval {
        key: ApprovalKey::for_request(&plan.request, "acme"),
        route: approvals::route_label(&plan.request),
        approved_on: "bench".into(),
        by: None,
        basis: Value::Null,
        auto: false,
        terms: Some(terms),
    });
    store.save(&approvals::path(&home)).unwrap();
}

fn once(bin: &str, root: &Path, cwd: &str, prompt: &str, session: &str) -> (f64, String) {
    let payload = json!({ "session_id": session, "cwd": cwd, "prompt": prompt, "hook_event_name": "UserPromptSubmit" });
    let t = Instant::now();
    let mut c = Command::new(bin)
        .args(["hook", "claude", "UserPromptSubmit"])
        .env("ADAGE_HOME", root.join("adage"))
        .env("ADAGE_POLICY", root.join("adage/managed.toml"))
        .env("ADAGE_ARM", "treated")
        .env("ADAGE_AUTO_LEARN", "0")
        .env("HOME", root.join("home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    (t.elapsed().as_secs_f64() * 1000.0, String::from_utf8_lossy(&o.stdout).into_owned())
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[(((v.len() - 1) as f64) * p).round() as usize]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (adage, exec) = (args[0].clone(), args[1].clone());
    let runs: usize = args.get(2).and_then(|r| r.parse().ok()).unwrap_or(200);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    setup(&root);
    let repo = root.join("acme").to_string_lossy().into_owned();
    let plain = root.join("plain").to_string_lossy().into_owned();
    let mut out = serde_json::Map::new();
    for (name, cwd, prompt) in [("no-repo", &plain, "Pick up W-2042"), ("no-match", &repo, "Tidy up the README please")]
    {
        let (mut a, mut e) = (Vec::new(), Vec::new());
        for i in 0..runs {
            let (ta, oa) = once(&adage, &root, cwd, prompt, &format!("{name}-a-{i}"));
            let (te, oe) = once(&exec, &root, cwd, prompt, &format!("{name}-e-{i}"));
            assert_eq!(oa, oe, "{name}: the two binaries printed different bytes");
            a.push(ta);
            e.push(te);
        }
        let (a50, a95, e50, e95) = (pct(&mut a, 0.5), pct(&mut a, 0.95), pct(&mut e, 0.5), pct(&mut e, 0.95));
        out.insert(
            name.into(),
            json!({ "adage_p50_ms": a50, "adage_p95_ms": a95, "exec_p50_ms": e50, "exec_p95_ms": e95, "p95_delta_ms": e95 - a95, "within_1ms": e95 - a95 <= 1.0 }),
        );
    }
    let (mut a, mut e) = (Vec::new(), Vec::new());
    let mut delivered = 0;
    for i in 0..runs.min(50) {
        a.push(once(&adage, &root, &repo, "Pick up W-2042", &format!("e0-{i}")).0);
        let (t, o) = once(&exec, &root, &repo, "Pick up W-2042", &format!("e1-{i}"));
        delivered += o.contains("<adage-data") as usize;
        e.push(t);
    }
    out.insert(
        "e1".into(),
        json!({ "delivered": delivered, "of": e.len(), "adage_e0_p50_ms": pct(&mut a, 0.5), "adage_e0_p95_ms": pct(&mut a, 0.95), "exec_e1_p50_ms": pct(&mut e, 0.5), "exec_e1_p95_ms": pct(&mut e, 0.95) }),
    );
    println!("{}", serde_json::to_string_pretty(&Value::Object(out)).unwrap());
}
