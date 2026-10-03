//! Track G G4, end to end through the real hook: learn on an `evals/gen`
//! newkinds bundle's rehearsal traces, then replay held-out traces event by event through
//! `adage hook claude <event>` (a fresh process per event, payload on stdin, the treated arm) and
//! read what it delivers.
//!
//! Held out, per bundle: both traces of the last scoped, deps and control task, and the second
//! trace of the last registry task (the registry has only two tasks, and the sequence needs
//! both). The environment step is approved in review first (`adage review approve`), as a person
//! would: it is nascent until then. A context route (`make check` after a registry edit then
//! `make gen`) is added to the learned tracks, since the rehearsal's reflexes compile to one-event
//! triggers.
//!
//! - **scope**: its reflex fires at the batch of the first edit under `packages/<name>/`, once,
//!   with `{part}` bound to that `<name>` (`EditSegment`), and no Stop request follows (the trace
//!   runs the check);
//! - **env_sync**: `make deps then make check` at the batch of the first `requirements.txt` edit;
//! - **sequence**: `make gen then make check` at the first registry edit, and the context route at
//!   the batch that runs `make gen` after it;
//! - **controls**: nothing but the dead ends announced at the first prompt.
//!
//! Skipped without python3; `ADAGE_GEN_NEWKINDS=<dir>` reuses a rehearsed suite.
#![cfg(unix)]

use adage_core::Effect;
use adage_runtime::Event;
use adage_tracks::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod common;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn adage(home: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
    common::scrub(&mut c);
    c.env("ADAGE_HOME", home).env("HOME", home).env("ADAGE_AUTO_LEARN", "0");
    c
}

fn suite(tmp: &Path) -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("ADAGE_GEN_NEWKINDS") {
        return Some(PathBuf::from(d));
    }
    if !Command::new("python3").arg("--version").output().is_ok_and(|o| o.status.success()) {
        eprintln!("python3 missing: skipped");
        return None;
    }
    let out = tmp.join("newkinds");
    let run = |args: &[&str]| {
        Command::new("python3")
            .args(args)
            .arg(&out)
            .args(if args[0].ends_with("rehearse.py") { &["--no-learn"][..] } else { &[] })
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let gen = root().join("evals/gen/gen.py").to_string_lossy().to_string();
    let reh = root().join("evals/gen/rehearse.py").to_string_lossy().to_string();
    if !run(&[&gen, "--suite", "newkinds", "--out"]) || !run(&[&reh]) {
        eprintln!("gen.py or rehearse.py failed: skipped");
        return None;
    }
    Some(out)
}

fn context_track(repo: &str) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: format!("{repo}/contexts"),
                version: "0.1.0".into(),
                description: "a context route for the G4 end-to-end test".into(),
                provenance: "authored".into(),
            },
            scope: Scope { repo: repo.into() },
            gauge: Gauge::default(),
        },
        routes: vec![Route {
            id: "context-check-after-gen".into(),
            summary: "make check after a registry edit and make gen".into(),
            trigger: Trigger::Context {
                items: vec![
                    CtxItem::Edit { glob: "registry/*.csv".into() },
                    CtxItem::Act { keys: vec!["exec make gen".into()] },
                ],
                within: 8,
            },
            step: Step::Run { command: "make check".into() },
            slots: vec![],
            effect: Effect::Verify,
            strength: Strength::Prime,
            state: RouteState::Firm,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence { support: 4, ..Default::default() },
            authored: true,
        }],
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (p, q) = (e.path(), to.join(e.file_name()));
        let ty = e.file_type().unwrap();
        if ty.is_dir() {
            copy_dir(&p, &q);
        } else if ty.is_file() {
            std::fs::copy(&p, &q).unwrap();
        }
    }
}

/// One hook event as Claude Code sends it, and what the hook printed: the route ids and texts of
/// the directives, or the Stop request.
struct Heard {
    routes: Vec<String>,
    text: String,
    stop: bool,
}

fn hook(home: &Path, cwd: &Path, session: &str, ev: &Event) -> Option<Heard> {
    let call =
        |c: &adage_runtime::Call| json!({ "tool_name": c.tool, "tool_input": c.input, "tool_response": c.output });
    let (name, extra) = match ev {
        Event::SessionStart => ("SessionStart", json!({})),
        Event::Prompt { text } => ("UserPromptSubmit", json!({ "prompt": text })),
        Event::Batch { calls } => {
            ("PostToolBatch", json!({ "tool_calls": calls.iter().map(call).collect::<Vec<_>>() }))
        }
        Event::Failure { call: c } => {
            ("PostToolUseFailure", json!({ "tool_name": c.tool, "tool_input": c.input, "error": c.output }))
        }
        Event::Stop { .. } => ("Stop", json!({ "stop_hook_active": false })),
        Event::SessionEnd => ("SessionEnd", json!({})),
        Event::TurnStep { turn_id, index, model, agent, ending } => (
            "TurnStep",
            json!({ "turn_id": turn_id, "index": index, "model": model, "agent_id": agent, "ending": ending }),
        ),
    };
    let mut payload = json!({ "session_id": session, "cwd": cwd });
    payload.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let mut c = adage(home)
        .args(["hook", "claude", name])
        .env("ADAGE_ARM", "treated")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success() && o.stderr.is_empty(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8(o.stdout).unwrap();
    if out.trim().is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    if v["decision"] == "block" {
        return Some(Heard { routes: vec![], text: v["reason"].as_str().unwrap_or_default().into(), stop: true });
    }
    let text = v["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_default().to_string();
    let re = regex::Regex::new(r#"route="([^"]+)""#).unwrap();
    Some(Heard { routes: re.captures_iter(&text).map(|c| c[1].to_string()).collect(), text, stop: false })
}

/// The repository-relative paths a batch edits.
fn edits(ev: &Event, root: &str) -> Vec<String> {
    let Event::Batch { calls } = ev else { return vec![] };
    calls
        .iter()
        .filter(|c| matches!(c.tool.as_str(), "Edit" | "Write" | "MultiEdit"))
        .filter_map(|c| c.input.get("file_path")?.as_str()?.strip_prefix(root)?.strip_prefix('/').map(str::to_string))
        .collect()
}

fn runs(ev: &Event, cmd: &str) -> bool {
    let Event::Batch { calls } = ev else { return false };
    calls.iter().any(|c| c.input.get("command").and_then(Value::as_str).is_some_and(|x| x.trim() == cmd))
}

#[test]
fn new_kind_reflexes_and_a_context_route_fire_through_the_hook_on_held_out_traces() {
    let d = tempfile::tempdir().unwrap();
    let tmp = d.path().canonicalize().unwrap();
    let Some(s) = suite(&tmp) else { return };
    let mut bundles: Vec<PathBuf> = std::fs::read_dir(&s)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("rehearsal/report.json").is_file())
        .collect();
    bundles.sort();
    assert!(!bundles.is_empty());
    let mut summary = Vec::new();
    for b in &bundles {
        let name = b.file_name().unwrap().to_string_lossy().to_string();
        let tasks: Value = serde_json::from_slice(&std::fs::read(b.join("tasks.json")).unwrap()).unwrap();
        let repo = tasks["repo"].as_str().unwrap().to_string();
        let report: Value = serde_json::from_slice(&std::fs::read(b.join("rehearsal/report.json")).unwrap()).unwrap();
        let sessions = report["sessions"].as_array().unwrap();
        let s = |x: &Value, k: &str| x[k].as_str().unwrap().to_string();
        // The last task of each kind (by id).
        let mut last: BTreeMap<String, String> = BTreeMap::new();
        for x in sessions {
            let e = last.entry(s(x, "kind")).or_default();
            if s(x, "task") > *e {
                *e = s(x, "task");
            }
        }
        let held = |x: &Value| {
            let (kind, task) = (s(x, "kind"), s(x, "task"));
            match kind.as_str() {
                "scoped" | "deps" | "control" => last[&kind] == task,
                "registry" => last[&kind] == task && s(x, "run").ends_with("-r2"),
                _ => false,
            }
        };
        let tx_dir = std::fs::read_dir(b.join("rehearsal/transcripts")).unwrap().flatten().next().unwrap().path();
        let path = |x: &Value| tx_dir.join(format!("{}.jsonl", s(x, "session")));
        let train: Vec<PathBuf> = sessions.iter().filter(|x| !held(x)).map(path).collect();
        let test: Vec<&Value> = sessions.iter().filter(|x| held(x)).collect();
        assert_eq!(test.len(), 7, "{name}");

        // Learn on the rest.
        let home = tmp.join(format!("home-{name}"));
        let o = adage(&home)
            .args(["condition", "--demo", "--json", "--no-instincts", "--repo", &repo])
            .arg("--outcomes")
            .arg(b.join("rehearsal/outcomes.json"))
            .arg("--transcripts")
            .args(&train)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let learned: Vec<Track> = io::read_all(&home.join("tracks")).unwrap();
        let find = |f: &dyn Fn(&Route) -> bool| {
            learned.iter().flat_map(|t| t.routes.iter().map(move |r| (t, r))).find(|(_, r)| f(r))
        };
        let (_, scope) = find(&|r| r.slots.iter().any(|s| matches!(s.source, SlotSource::EditSegment { .. })))
            .unwrap_or_else(|| panic!("{name}: no scope reflex"));
        let (env_track, env) =
            find(&|r| r.trigger == Trigger::AfterEdit { paths: "requirements.txt".into() }).expect("env_sync");
        let (_, seq) = find(&|r| {
            r.trigger == Trigger::AfterEdit { paths: "registry/*.csv".into() }
                && matches!(r.step, Step::Sequence { .. })
        })
        .expect("sequence");
        assert_eq!(env.state, RouteState::Nascent, "an environment step waits for review");
        // A person approves the environment step.
        // (As the review tests do: the command refuses to run from an agent's shell.)
        let o = adage(&home)
            .args(["review", "approve", &env_track.manifest.track.name, &env.id, "--by", "e2e"])
            .env_remove("CLAUDECODE")
            .env_remove("OPENCODE")
            .env_remove("CODEX_SANDBOX")
            .env_remove("CODEX_SANDBOX_NETWORK_DISABLED")
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        // The context route, compiled into the index with the learned tracks.
        let mut tracks = io::read_all(&home.join("tracks")).unwrap();
        tracks.push(context_track(&repo));
        let (index, _) = Index::compile_root(&home.join("tracks"), &tracks);
        assert!(index.for_repo(&repo).any(|r| r.route.id == env.id), "approved");
        index.write_atomic(&home.join("index.json")).unwrap();

        // The repository, under its key's name.
        let work = tmp.join(format!("work-{name}")).join(&repo);
        copy_dir(&b.join("repo"), &work);
        std::fs::create_dir_all(work.join(".git")).unwrap();
        let root_s = work.to_string_lossy().to_string();

        for x in test {
            let (kind, sid) = (s(x, "kind"), s(x, "session"));
            let t = adage_eval::transcript::read(&path(x)).unwrap();
            let events: Vec<Event> =
                adage_eval::transcript::events(&t, &root_s, true).into_iter().map(|s| s.event).collect();
            let heard: Vec<(usize, Heard)> = events
                .iter()
                .enumerate()
                .filter_map(|(i, ev)| hook(&home, &work, &format!("e2e-{sid}"), ev).map(|h| (i, h)))
                .collect();
            let at = |id: &str| -> Vec<(usize, &Heard)> {
                heard.iter().filter(|(_, h)| h.routes.iter().any(|r| r == id)).map(|(i, h)| (*i, h)).collect()
            };
            let first_edit = |dir: &str, ext: &str| {
                events.iter().enumerate().find_map(|(i, ev)| {
                    edits(ev, &root_s).into_iter().find(|p| p.starts_with(dir) && p.ends_with(ext)).map(|p| (i, p))
                })
            };
            let new_kinds = [scope.id.as_str(), env.id.as_str(), seq.id.as_str(), "context-check-after-gen"];
            let fired: Vec<&str> = new_kinds.iter().copied().filter(|id| !at(id).is_empty()).collect();
            match kind.as_str() {
                "scoped" => {
                    let (i, p) = first_edit("packages/", ".py").expect("a package edit");
                    let part = p.split('/').nth(1).unwrap();
                    let hits = at(&scope.id);
                    assert_eq!(hits.len(), 1, "{name} {sid}: said once");
                    assert_eq!(hits[0].0, i, "{name} {sid}: at the edit's batch");
                    let want = format!("\n  make -C packages/{part} check\n");
                    assert!(hits[0].1.text.contains(&want), "{name} {sid}: {want:?} in {}", hits[0].1.text);
                    assert_eq!(fired, [scope.id.as_str()], "{name} {sid}");
                    assert!(heard.iter().all(|(_, h)| !h.stop), "{name} {sid}: the check ran, no Stop request");
                    summary.push(format!("{name} {sid} scope: event {i}, bound {part:?}"));
                }
                "deps" => {
                    let (i, _) = first_edit("requirements.txt", "").expect("a requirements edit");
                    let hits = at(&env.id);
                    assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), [i], "{name} {sid}");
                    assert!(hits[0].1.text.contains("  1. make deps\n  2. make check"), "{}", hits[0].1.text);
                    assert_eq!(fired, [env.id.as_str()], "{name} {sid}");
                    summary.push(format!("{name} {sid} env_sync: event {i}"));
                }
                "registry" => {
                    let (i, _) = first_edit("registry/", ".csv").expect("a registry edit");
                    let hits = at(&seq.id);
                    assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), [i], "{name} {sid}");
                    assert!(hits[0].1.text.contains("  1. make gen\n  2. make check"), "{}", hits[0].1.text);
                    let gen = (i..events.len()).find(|&j| runs(&events[j], "make gen")).expect("make gen");
                    let ctx_hits = at("context-check-after-gen");
                    assert_eq!(ctx_hits.iter().map(|h| h.0).collect::<Vec<_>>(), [gen], "{name} {sid}");
                    assert!(ctx_hits[0].1.text.contains("A known next step here is:\n  make check"));
                    summary.push(format!("{name} {sid} sequence: event {i}; context: event {gen}"));
                }
                "control" => {
                    assert!(fired.is_empty(), "{name} {sid}: {fired:?}");
                    // Nothing else either, but the dead ends announced at the first prompt.
                    for (i, h) in &heard {
                        assert!(
                            matches!(events[*i], Event::Prompt { .. })
                                && h.routes.iter().all(|r| r.starts_with("dead-end-")),
                            "{name} {sid}: event {i}: {:?}",
                            h.routes
                        );
                    }
                    summary.push(format!(
                        "{name} {sid} control: {} directives, all dead ends at the first prompt",
                        heard.len()
                    ));
                }
                other => panic!("{other}"),
            }
        }
    }
    eprintln!("{}", summary.join("\n"));
}
