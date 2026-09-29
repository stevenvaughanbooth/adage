//! `adage watch --demo` / `adage ui --demo`: a small scripted scenario replayed into a temporary
//! `ADAGE_HOME`, so the screens can be seen (and screenshotted, and tested) without a live agent.
//!
//! The scenario is modelled on the harbor-dispatch demo (`demo/results/…harbor-dispatch…`): a
//! TypeScript service whose incidents resolve only through `bin/fetch-incident`, whose `npm test`
//! fails where `npm run check` works, and whose `registry/*.csv` edits need `npm run codegen`.
//! Three agent sessions run side by side in each round — one follows every directive, one
//! ignores and trips the finish line, one is a control — and each hook call is a real `adage
//! hook claude <event>` run of this binary, so the spools hold real decisions. After each round a
//! learning run is simulated: the track is updated, the index recompiled, and a `learn.log` line
//! written in the live-learning format.
//!
//! Everything lives under a temporary directory, removed on exit; nothing outside it is touched.

use super::view::Ctx;
use adage_core::spans::SpanKind;
use adage_core::{CommandTable, Effect};
use adage_store::Paths;
use adage_tracks::*;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const REPO: &str = "harbor-dispatch";
pub const TRACK: &str = "harbor-dispatch/incidents";
const ROUNDS: usize = 3;

/// A temporary data directory and repository, removed when dropped.
pub struct DemoHome {
    pub dir: PathBuf,
    pub paths: Paths,
    pub repo: PathBuf,
}

impl Drop for DemoHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn evidence(support: u32, opportunities: u32) -> Evidence {
    // Four decimals, as a person would write them: serde_json's default float parser does not
    // round-trip every f64 (18/19 = 0.9473684210526315 comes back one ulp off), and the doctor
    // re-hashes the index's routes after parsing it.
    let r4 = |x: f64| (x * 1e4).round() / 1e4;
    Evidence {
        support,
        opportunities,
        days: 3,
        rate: r4(support as f64 / opportunities.max(1) as f64),
        lower_bound: r4(wilson_lower(support, opportunities)),
        provenance: "desire-path".into(),
        ..Default::default()
    }
}

fn route(
    id: &str,
    summary: &str,
    trigger: Trigger,
    step: Step,
    effect: Effect,
    strength: Strength,
    ev: Evidence,
) -> Route {
    Route {
        id: id.into(),
        summary: summary.into(),
        trigger,
        step,
        slots: vec![],
        effect,
        strength,
        state: RouteState::Firm,
        exit: vec![Exit::IntentChanges],
        evidence: ev,
        authored: false,
    }
}

/// The demo track; `round` > 0 is after that many learning runs.
pub fn track(round: usize) -> Track {
    let table = CommandTable::load();
    let lookup_key = adage_runtime::command_key("bin/fetch-incident INC-1", &table);
    let npm_test = adage_runtime::command_key("npm test", &table);
    let mut lookup = route(
        "lookup-incident",
        "incidents resolve through bin/fetch-incident",
        Trigger::Prompt { span: SpanKind::Ticket },
        Step::Run { command: "bin/fetch-incident {item}".into() },
        Effect::Observe,
        Strength::Fire,
        evidence(24 + round as u32, 25 + round as u32),
    );
    lookup.slots = vec![Slot {
        name: "item".into(),
        source: SlotSource::Prompt { span: SpanKind::Ticket },
        contract: Contract::Pattern { regex: r"^INC-\d{1,5}$".into() },
    }];
    let linked_strength = if round >= 1 { Strength::Fire } else { Strength::Prime };
    let mut linked = route(
        "read-linked-files",
        "the files an incident lists are read together, in one turn",
        Trigger::AfterCommand { command: lookup_key },
        Step::ReadBatch { slot: "files".into(), max: 5 },
        Effect::Observe,
        linked_strength,
        evidence(9 + 4 * round as u32, 14 + 3 * round as u32),
    );
    linked.slots = vec![Slot {
        name: "files".into(),
        source: SlotSource::ResultField { field: "linked_files".into() },
        contract: Contract::RepoPath,
    }];
    let codegen = route(
        "codegen-after-registry",
        "registry edits need npm run codegen",
        Trigger::AfterEdit { paths: "registry/*.csv".into() },
        Step::Run { command: "npm run codegen".into() },
        Effect::Regenerate,
        Strength::Fire,
        evidence(18 + round as u32, 19 + round as u32),
    );
    let sig = "HARBOR_ENV is not set";
    let dead = route(
        "dead-end-npm-test",
        "npm test fails here; npm run check works",
        Trigger::AfterFailure { command: npm_test, signature: sig.into() },
        Step::Avoid { avoid: "npm test".into(), instead: "npm run check".into(), signature: sig.into() },
        Effect::Verify,
        Strength::Inhibit,
        evidence(6 + round as u32, 7 + round as u32),
    );
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: TRACK.into(),
                version: format!("0.{}.0", 3 + round),
                description: "harbor-dispatch incident work (demo)".into(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: REPO.into() },
            gauge: Gauge::default(),
        },
        routes: vec![lookup, linked, codegen, dead],
    }
}

fn install_track(paths: &Paths, t: &Track) -> Result<String> {
    io::validate(t).context("the demo track is invalid")?;
    io::write(&paths.tracks().join(TRACK), t)?;
    let index = Index::compile(&io::read_all(&paths.tracks())?);
    index.write_atomic(&paths.index())?;
    Ok(index.generation)
}

pub fn setup() -> Result<DemoHome> {
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("adage-demo-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let dir = dir.canonicalize()?;
    let home = DemoHome { paths: Paths::at(dir.join("adage")), repo: dir.join(REPO), dir };
    for d in [".git", "registry", "bin", ".claude", "src/dispatch"] {
        std::fs::create_dir_all(home.repo.join(d))?;
    }
    std::fs::create_dir_all(home.dir.join("home"))?;
    std::fs::write(home.repo.join("registry/carriers.csv"), "id,name\n")?;
    std::fs::write(home.repo.join("src/dispatch/queue.ts"), "export {};\n")?;
    let exe = std::env::current_exe()?.to_string_lossy().to_string();
    std::fs::write(
        home.repo.join(".claude/settings.local.json"),
        serde_json::to_string_pretty(&crate::harness::claude::settings(&exe))?,
    )?;
    std::fs::create_dir_all(&home.paths.root)?;
    std::fs::write(home.paths.config(), "[learn]\nauto = true\n")?;
    install_track(&home.paths, &track(0))?;
    Ok(home)
}

/// One step of the scenario.
enum Beat {
    Hook { arm: &'static str, event: &'static str, payload: Value },
    Learn { round: usize },
}

fn sessions(repo: &Path, round: usize) -> Vec<Vec<Beat>> {
    let cwd = repo.to_string_lossy().to_string();
    let hook = |session: &str, arm: &'static str, event: &'static str, extra: Value| {
        let mut p =
            json!({ "session_id": session, "transcript_path": "/dev/null", "cwd": cwd, "hook_event_name": event });
        for (k, v) in extra.as_object().into_iter().flatten() {
            p[k] = v.clone();
        }
        Beat::Hook { arm, event, payload: p }
    };
    let bash_out = |cmd: &str, stdout: &str| json!({ "tool_calls": [{ "tool_name": "Bash", "tool_input": { "command": cmd }, "tool_response": { "stdout": stdout, "stderr": "" } }] });
    let bash = |cmd: &str| bash_out(cmd, "ok");
    let edit = |file: &str| json!({ "tool_calls": [{ "tool_name": "Edit", "tool_input": { "file_path": format!("{cwd}/{file}"), "old_string": "a", "new_string": "b" }, "tool_response": "ok" }] });
    let read = |files: &[&str]| {
        let calls: Vec<Value> = files
            .iter()
            .map(|f| json!({ "tool_name": "Read", "tool_input": { "file_path": format!("{cwd}/{f}") }, "tool_response": "…" }))
            .collect();
        json!({ "tool_calls": calls })
    };
    let n = |k: usize| 17 + round * 31 + k * 7;
    let inc = |k: usize| format!("INC-{}", n(k));
    let lookup = |k: usize| {
        let out = json!({ "incident": inc(k), "linked_files": ["src/dispatch/queue.ts", "registry/carriers.csv"] });
        bash_out(&format!("bin/fetch-incident {}", inc(k)), &out.to_string())
    };
    // Session ids shaped like Claude Code's (UUIDs), distinct per round.
    let id = |lane: u64| {
        let mut x = (round as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ lane.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x ^= x >> 29;
        format!(
            "{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}",
            x >> 32,
            x & 0xffff,
            (x >> 20) & 0xfff,
            (x >> 8) & 0xfff,
            x & 0xffff_ffff_ffff
        )
    };
    let (a, b, c) = (id(1), id(2), id(3));
    vec![
        // A: follows every directive, meets the finish line.
        vec![
            hook(&a, "treated", "SessionStart", json!({ "source": "startup" })),
            hook(
                &a,
                "treated",
                "UserPromptSubmit",
                json!({ "prompt": format!("Pick up {}: carrier retries double-book slots", inc(0)) }),
            ),
            hook(&a, "treated", "PostToolBatch", lookup(0)),
            hook(&a, "treated", "PostToolBatch", read(&["src/dispatch/queue.ts", "registry/carriers.csv"])),
            hook(&a, "treated", "PostToolBatch", edit("registry/carriers.csv")),
            hook(&a, "treated", "PostToolBatch", bash("npm run codegen")),
            hook(&a, "treated", "Stop", json!({ "stop_hook_active": false })),
        ],
        // B: explores instead, trips the dead end, forgets codegen.
        vec![
            hook(
                &b,
                "treated",
                "UserPromptSubmit",
                json!({ "prompt": format!("{} — dispatch times out under load", inc(1)) }),
            ),
            hook(&b, "treated", "PostToolBatch", bash(&format!("grep -rn {} src", inc(1)))),
            hook(&b, "treated", "PostToolBatch", bash("ls src/dispatch")),
            hook(&b, "treated", "PostToolBatch", bash("cat src/dispatch/queue.ts")),
            hook(
                &b,
                "treated",
                "PostToolUseFailure",
                json!({ "tool_name": "Bash", "tool_input": { "command": "npm test" }, "error": "Error: HARBOR_ENV is not set" }),
            ),
            hook(&b, "treated", "PostToolBatch", bash("npm run check")),
            hook(&b, "treated", "PostToolBatch", edit("registry/carriers.csv")),
            hook(&b, "treated", "Stop", json!({ "stop_hook_active": false })),
            hook(&b, "treated", "PostToolBatch", bash("npm run codegen")),
            hook(&b, "treated", "Stop", json!({ "stop_hook_active": true })),
        ],
        // C: the control — Adage decides, logs, and says nothing.
        vec![
            hook(&c, "control", "UserPromptSubmit", json!({ "prompt": format!("Please look at {}", inc(2)) })),
            hook(&c, "control", "PostToolBatch", lookup(2)),
            hook(&c, "control", "PostToolBatch", edit("registry/carriers.csv")),
            hook(&c, "control", "Stop", json!({ "stop_hook_active": false })),
        ],
    ]
}

/// The scenario for `round`, the three sessions interleaved, then a learning run.
fn round_steps(repo: &Path, round: usize) -> Vec<Beat> {
    let mut lanes: Vec<std::collections::VecDeque<Beat>> = sessions(repo, round).into_iter().map(Into::into).collect();
    let mut out = Vec::new();
    while lanes.iter().any(|l| !l.is_empty()) {
        for l in lanes.iter_mut() {
            if let Some(s) = l.pop_front() {
                out.push(s);
            }
        }
    }
    out.push(Beat::Learn { round: round + 1 });
    out
}

fn run_hook(exe: &Path, home: &DemoHome, arm: &str, event: &str, payload: &Value) {
    let child = Command::new(exe)
        .args(["hook", "claude", event])
        .current_dir(&home.repo)
        .env("ADAGE_HOME", &home.paths.root)
        .env("HOME", home.dir.join("home"))
        .env("ADAGE_ARM", arm)
        // The scenario simulates learning itself; a real background learner here would find no
        // history (HOME is the demo's) and has nothing to add.
        .env("ADAGE_AUTO_LEARN", "0")
        .env_remove("ADAGE_DISABLE")
        .env_remove("ADAGE_CONTROL")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        if let Some(mut sin) = child.stdin.take() {
            use std::io::Write;
            let _ = sin.write_all(payload.to_string().as_bytes());
        }
        let _ = child.wait();
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// A simulated learning run: the track re-learned with this round's evidence, the index
/// recompiled, and one `learn.log` line in the live-learning format.
fn learn(home: &DemoHome, round: usize) {
    let t = track(round);
    let Ok(generation) = install_track(&home.paths, &t) else { return };
    let (read, used) = (25 + 3 * round as u64, 24 + 3 * round as u64);
    let lookup = &t.routes[0].evidence;
    let linked = &t.routes[1].evidence;
    // Summaries in `adage_learn::diff`'s fixed templates.
    let mut changes = vec![json!({
        "route": "lookup-incident", "track": TRACK, "change": "evidence", "from": null, "to": null,
        "summary": format!(
            "fire reflex evidence {}/{} → {}/{} traces",
            lookup.support - 1, lookup.opportunities - 1, lookup.support, lookup.opportunities
        ),
    })];
    if round == 1 {
        changes.insert(
            0,
            json!({
                "route": "read-linked-files", "track": TRACK, "change": "strength", "from": "prime", "to": "fire",
                "summary": format!(
                    "reflex prime → fire ({} of {} traces, lower bound {:.2})",
                    linked.support, linked.opportunities, linked.lower_bound
                ),
            }),
        );
    }
    let line = json!({
        "ts": now_ms(), "kind": "condition", "trigger": "auto", "repo": REPO,
        "sessions_read": read, "sessions_used": used, "duration_ms": 640 + 90 * round as u64,
        "index_generation": generation, "changes": changes,
    });
    let _ = crate::learnlog::append(&home.paths, &line);
}

/// Play `rounds` rounds, `pace` apart, until done or `stop` is set.
pub fn play(home: &DemoHome, exe: &Path, rounds: usize, pace: Duration, stop: &AtomicBool) {
    for round in 0..rounds {
        for step in round_steps(&home.repo, round) {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            match step {
                Beat::Hook { arm, event, payload } => run_hook(exe, home, arm, event, &payload),
                Beat::Learn { round } => {
                    std::thread::sleep(pace * 2);
                    learn(home, round);
                }
            }
            std::thread::sleep(pace);
        }
    }
}

/// `--demo`: set up, then play in the background under the live screen (or, with `once`, play
/// everything at once and print one frame).
pub fn run(once: bool, size: Option<(u16, u16)>, full_app: bool) -> Result<()> {
    let home = Arc::new(setup()?);
    let exe = std::env::current_exe()?;
    let mut ctx = Ctx::at(home.paths.clone(), Some(home.repo.clone()));
    ctx.demo = true;
    if once {
        play(&home, &exe, 1, Duration::ZERO, &AtomicBool::new(false));
        print!("{}", super::watch::snapshot(&ctx, size));
        return Ok(());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let player = {
        let (home, stop) = (home.clone(), stop.clone());
        std::thread::spawn(move || play(&home, &exe, ROUNDS, Duration::from_millis(650), &stop))
    };
    let result = if full_app { super::app::run(ctx, None) } else { super::watch::run(ctx, None) };
    stop.store(true, Ordering::SeqCst);
    let _ = player.join();
    drop(home);
    result
}
