//! Hook latency for repository keys and events that match nothing (no model needed).
//!
//! ```text
//! cargo build --release -p adage-cli --examples
//! target/release/examples/hook_bench <adage binary> [runs] [commits]
//! ```
//! Builds, with git, a repository without `origin` holding `commits` commits (default 5,000;
//! packed by `git fast-import`), a small one with loose objects, and one with `origin`. Writes an
//! index for each whose routes are keyed both by the bare directory name (the key before
//! repository keys carried the root commit) and by the current key, so that binaries from before
//! and after that change do the same work. Then spawns `adage hook claude <event>` `runs` times
//! per scenario (after 20 warm-up runs), exec form, payload on stdin, timing spawn to exit, and
//! prints JSON: p50, p95, p99, max and mean in milliseconds, and the first run's time with an
//! empty repository-key cache.

use adage_core::spans::SpanKind;
use adage_core::Effect;
use adage_tracks::*;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

fn route(id: &str, trigger: Trigger, step: Step, slots: Vec<Slot>, effect: Effect, aspect: Aspect) -> Route {
    Route {
        id: id.into(),
        summary: String::new(),
        trigger,
        step,
        slots,
        effect,
        aspect,
        state: RouteState::Active,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { support: 5, ..Default::default() },
        authored: false,
    }
}

/// The demo's four route kinds, for each of `repos`.
fn index(home: &Path, repos: &[String]) {
    let routes = vec![
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
            Aspect::Green,
        ),
        route(
            "read-linked-files",
            Trigger::AfterCommand { command: "exec python3 scripts/wi-lookup {id}".into() },
            Step::ReadBatch { slot: "files".into(), max: 5 },
            vec![Slot {
                name: "files".into(),
                source: SlotSource::ResultField { field: "linked_files".into() },
                contract: Contract::RepoPath,
            }],
            Effect::Observe,
            Aspect::Yellow,
        ),
        route(
            "finish-line-1",
            Trigger::AfterEdit { paths: "schema/*.json".into() },
            Step::Run { command: "make gen".into() },
            vec![],
            Effect::Regenerate,
            Aspect::Green,
        ),
        route(
            "dead-end-1",
            Trigger::SessionStart,
            Step::Avoid {
                avoid: "python3 -m pytest".into(),
                instead: "make test".into(),
                signature: "No module named pytest".into(),
            },
            vec![],
            Effect::Verify,
            Aspect::Red,
        ),
    ];
    let tracks: Vec<Track> = repos
        .iter()
        .map(|repo| Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: format!("{}/bench", adage_learn::slug(repo)),
                    version: "0.1.0".into(),
                    description: String::new(),
                    provenance: "desire-path".into(),
                },
                scope: Scope { repo: repo.clone() },
                gauge: Gauge::default(),
            },
            routes: routes.clone(),
        })
        .collect();
    for t in &tracks {
        io::validate(t).unwrap();
    }
    Index::compile(&tracks).write_atomic(&home.join("index.json")).unwrap();
}

fn git(dir: &Path, args: &[&str], stdin: Option<&[u8]>) {
    let mut c = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.com")
        .env("GIT_COMMITTER_NAME", "bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.com")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("git");
    if let Some(s) = stdin {
        c.stdin.take().unwrap().write_all(s).unwrap();
    }
    drop(c.stdin.take());
    assert!(c.wait().unwrap().success(), "git {args:?}");
}

/// A repository with `n` commits: packed (`git fast-import`), or loose (`git commit`).
fn repo(dir: &Path, n: usize, packed: bool) {
    std::fs::create_dir_all(dir.join("schema")).unwrap();
    git(dir, &["init", "-q", "-b", "main"], None);
    if packed {
        let mut s = String::new();
        for i in 0..n {
            let msg = format!("commit {i}");
            let data = format!("line {i}\n");
            s.push_str(&format!(
                "commit refs/heads/main\nmark :{m}\ncommitter bench <bench@example.com> {t} +0000\ndata {}\n{msg}\n",
                msg.len(),
                m = i + 1,
                t = 1_700_000_000 + i
            ));
            if i > 0 {
                s.push_str(&format!("from :{i}\n"));
            }
            s.push_str(&format!("M 644 inline f.txt\ndata {}\n{data}\n", data.len()));
        }
        git(dir, &["fast-import", "--quiet"], Some(s.as_bytes()));
        git(dir, &["checkout", "-q", "main"], None);
    } else {
        for i in 0..n {
            git(dir, &["commit", "-q", "--allow-empty", "-m", &format!("commit {i}")], None);
        }
    }
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[(((v.len() - 1) as f64) * p).round() as usize]
}

fn spawn(bin: &str, home: &Path, event: &str, payload: &Value) -> (f64, String) {
    let t = Instant::now();
    let mut c = Command::new(bin)
        .args(["hook", "claude", event])
        .env("ADAGE_HOME", home)
        .env("ADAGE_ARM", "treated")
        .env_remove("ADAGE_DISABLE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let out = c.wait_with_output().unwrap();
    (t.elapsed().as_secs_f64() * 1e3, String::from_utf8_lossy(&out.stdout).to_string())
}

/// Time one scenario: `payload(i)` for run `i` (`usize::MAX` for the first, cold-cache run). A
/// payload with a fixed session id keeps its state across runs, as within a real session.
fn time(bin: &str, home: &Path, event: &str, payload: &dyn Fn(usize) -> Value, runs: usize) -> (Value, String) {
    let _ = std::fs::remove_dir_all(home.join("repo-keys"));
    let (first, first_out) = spawn(bin, home, event, &payload(usize::MAX));
    let mut ms = Vec::with_capacity(runs);
    let mut last = String::new();
    for i in 0..runs + 20 {
        let (e, out) = spawn(bin, home, event, &payload(i));
        if i >= 20 {
            ms.push(e);
        }
        last = out;
    }
    let _ = std::fs::remove_dir_all(home.join("sessions"));
    let mean = ms.iter().sum::<f64>() / ms.len() as f64;
    (
        json!({ "runs": runs, "first_run_ms": first, "first_run_output_bytes": first_out.len(), "p50_ms": pct(&mut ms, 0.5), "p95_ms": pct(&mut ms, 0.95), "p99_ms": pct(&mut ms, 0.99), "max_ms": pct(&mut ms, 1.0), "mean_ms": mean }),
        last,
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bin = &args[1];
    let runs: usize = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(300);
    let commits: usize = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(5000);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let home = root.join("home");
    let (big, small, origin) = (root.join("big/acme-bench"), root.join("small/acme-bench"), root.join("acme-origin"));
    for d in [&big, &small, &origin] {
        std::fs::create_dir_all(d).unwrap();
    }
    repo(&big, commits, true);
    repo(&small, 50, false);
    repo(&origin, 1, false);
    git(&origin, &["remote", "add", "origin", "https://example.com/acme/origin.git"], None);
    let key = |p: &Path| adage_core::repo::repo_key(p);
    let prompt = |cwd: &Path| {
        let cwd = cwd.to_path_buf();
        move |i: usize| json!({ "session_id": format!("bench-{i}"), "cwd": cwd, "prompt": "Pick up W-2042 please" })
    };
    // One turn of calls that no route matches: a read, a listing, an edit outside `schema/`.
    let batch = |cwd: &Path, session: Option<&'static str>| {
        let cwd = cwd.to_path_buf();
        move |i: usize| {
            let sid = session.map(str::to_string).unwrap_or_else(|| format!("bench-{i}"));
            json!({ "session_id": sid, "cwd": cwd, "tool_calls": [
                { "tool_name": "Read", "tool_input": { "file_path": cwd.join("README.md") }, "tool_response": "text" },
                { "tool_name": "Bash", "tool_input": { "command": "ls -la" }, "tool_response": "README.md" },
                { "tool_name": "Edit", "tool_input": { "file_path": cwd.join("acme/billing.py"), "old_string": "a", "new_string": "b" }, "tool_response": "ok" },
            ] })
        }
    };

    let mut out = serde_json::Map::new();
    out.insert("commits".into(), commits.into());
    let t = Instant::now();
    let _ = key(&big);
    out.insert("in_process_walk_packed_ms".into(), (t.elapsed().as_secs_f64() * 1e3).into());
    out.insert("keys".into(), json!({ "packed": key(&big), "loose": key(&small), "origin": key(&origin) }));

    index(&home, &["someone-else".into()]);
    out.insert("no_routes_for_repo".into(), time(bin, &home, "PostToolBatch", &batch(&big, None), runs).0);

    index(&home, &[key(&origin)]);
    let (r, last) = time(bin, &home, "UserPromptSubmit", &prompt(&origin), runs);
    assert!(last.contains("wi-lookup W-2042"), "origin: {last}");
    out.insert("origin_prompt".into(), r);

    index(&home, &["acme-bench".into(), key(&small)]);
    let (r, last) = time(bin, &home, "UserPromptSubmit", &prompt(&small), runs);
    assert!(last.contains("wi-lookup W-2042"), "small: {last}");
    out.insert("no_origin_loose_50_prompt".into(), r);

    index(&home, &["acme-bench".into(), key(&big)]);
    let (r, last) = time(bin, &home, "UserPromptSubmit", &prompt(&big), runs);
    assert!(last.contains("wi-lookup W-2042"), "big: {last}");
    out.insert("no_origin_packed_prompt".into(), r);

    let (r, last) = time(bin, &home, "PostToolBatch", &batch(&big, None), runs);
    assert!(last.is_empty(), "no route matches: {last}");
    out.insert("no_match_batch_fresh_session".into(), r);
    // Mid-session: the first prompt has been decided (the state exists, the dead end announced).
    let first = json!({ "session_id": "bench-one", "cwd": big, "prompt": "Pick up W-2042 please" });
    assert!(spawn(bin, &home, "UserPromptSubmit", &first).1.contains("dead-end-1"));
    let (r, last) = time(bin, &home, "PostToolBatch", &batch(&big, Some("bench-one")), runs);
    assert!(last.is_empty(), "no route matches: {last}");
    out.insert("no_match_batch_mid_session".into(), r);

    println!("{}", serde_json::to_string_pretty(&Value::Object(out)).unwrap());
}
