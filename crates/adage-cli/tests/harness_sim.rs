//! Harness simulator, end to end through the binary. For each harness: learn from that harness's
//! mock history (`sim/fixtures`, written by `adage-sim`), then replay a held-out session
//! (`adage_sim::scenario::replay`) through `adage hook <harness> <event>` in that harness's native
//! payloads — once in the treated arm, once in the control arm — and check:
//!
//! - the treated arm gets each directive at its moment, in the harness's output shape: the lookup
//!   and the known dead end at the prompt, the linked files after the lookup, the finish line
//!   after the schema edit, a request to continue at the first stop, and nothing at the second;
//! - the control arm gets nothing at all;
//! - `adage graph --json` then counts both sessions, split by arm.
//!
//! Also: learning from Codex history and delivering in OpenCode, learning from all three
//! histories at once, and the OpenCode plugin itself driven through a whole session in node.
//! Every run uses an isolated `ADAGE_HOME` and `HOME`.

use adage_sim::scenario::{self, Rng, Turn};
use adage_sim::{claude, codex, opencode};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Sandbox {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Sandbox {
    /// A checkout named `acme-billing` (so its repository key matches the mock history's) with the
    /// files the linked-files route may name.
    fn new() -> Sandbox {
        adage_sim::assume_no_checkout_at_cwd();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in [
            "home",
            "adage",
            "acme-billing/.git",
            "acme-billing/acme",
            "acme-billing/schema",
            "acme-billing/tests",
            "acme-billing/scripts",
        ] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // The tracker's lookup script: a `predict` grant on the lookup hashes it.
        std::fs::write(root.join("acme-billing/scripts/wi-lookup"), "#!/usr/bin/env python3\n").unwrap();
        for f in [
            "acme/billing.py",
            "acme/customers.py",
            "schema/invoice.json",
            "schema/customer.json",
            "README.md",
            "Makefile",
        ] {
            std::fs::write(root.join("acme-billing").join(f), scenario::body(f)).unwrap();
        }
        Sandbox { _dir: dir, root }
    }
    fn repo(&self) -> String {
        self.root.join("acme-billing").to_string_lossy().to_string()
    }
    fn command_of(&self, bin: &Path, args: &[&str], arm: &str) -> Command {
        let mut c = Command::new(bin);
        c.args(args)
            .current_dir(&self.root)
            .env("ADAGE_HOME", self.root.join("adage"))
            .env("ADAGE_AUTO_LEARN", "0")
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("home/.config"))
            .env("CODEX_HOME", self.root.join("home/.codex"))
            .env("ADAGE_ARM", arm)
            // The managed policy file, absent unless a test writes it.
            .env("ADAGE_POLICY", self.root.join("managed-policy.toml"))
            .env_remove("ADAGE_DISABLE")
            .env_remove("CLAUDECODE")
            .env_remove("OPENCODE")
            .env_remove("CODEX_SANDBOX")
            .env_remove("CODEX_SANDBOX_NETWORK_DISABLED");
        c
    }
    fn run(&self, args: &[&str], stdin: &[u8], arm: &str) -> (String, String, i32) {
        self.run_of(Path::new(env!("CARGO_BIN_EXE_adage")), args, stdin, arm)
    }
    fn run_of(&self, bin: &Path, args: &[&str], stdin: &[u8], arm: &str) -> (String, String, i32) {
        let mut child = self
            .command_of(bin, args, arm)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = child.stdin.take().unwrap().write_all(stdin);
        let o = child.wait_with_output().unwrap();
        (
            String::from_utf8_lossy(&o.stdout).to_string(),
            String::from_utf8_lossy(&o.stderr).to_string(),
            o.status.code().unwrap_or(-1),
        )
    }
    fn learn(&self, args: &[&str]) -> Value {
        let mut a = vec!["condition", "--demo", "--json"];
        a.extend_from_slice(args);
        let (out, err, code) = self.run(&a, b"", "treated");
        assert_eq!(code, 0, "learn failed: {err}");
        serde_json::from_str(&out).unwrap()
    }
    fn learn_files(&self, files: &[PathBuf]) -> Value {
        let paths: Vec<String> = files.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let mut a = vec!["--transcripts"];
        a.extend(paths.iter().map(String::as_str));
        self.learn(&a)
    }
    fn hook(&self, harness: &str, event: &str, payload: &Value, arm: &str) -> String {
        self.hook_of(Path::new(env!("CARGO_BIN_EXE_adage")), harness, event, payload, arm)
    }
    fn hook_of(&self, bin: &Path, harness: &str, event: &str, payload: &Value, arm: &str) -> String {
        let (out, err, code) = self.run_of(bin, &["hook", harness, event], payload.to_string().as_bytes(), arm);
        self.hooked(out, err, code)
    }
    /// A hook run in the session's checkout, as a harness runs it: adage-exec checks a `predict`
    /// grant against the checkout it is asked from.
    fn hook_in_repo(&self, bin: &Path, harness: &str, event: &str, payload: &Value, arm: &str) -> String {
        let mut child = self
            .command_of(bin, &["hook", harness, event], arm)
            .current_dir(self.repo())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = child.stdin.take().unwrap().write_all(payload.to_string().as_bytes());
        let o = child.wait_with_output().unwrap();
        self.hooked(
            String::from_utf8_lossy(&o.stdout).to_string(),
            String::from_utf8_lossy(&o.stderr).to_string(),
            o.status.code().unwrap_or(-1),
        )
    }
    fn hooked(&self, out: String, err: String, code: i32) -> String {
        assert_eq!(code, 0, "hooks always exit 0");
        assert!(err.is_empty(), "hooks never write to stderr: {err}");
        out
    }
    fn graph(&self) -> Value {
        let (out, err, code) = self.run(&["graph", "--json"], b"", "treated");
        assert_eq!(code, 0, "{err}");
        serde_json::from_str(&out).unwrap()
    }
}

fn learned_routes(report: &Value) -> Vec<String> {
    let mut v: Vec<String> = report["repos"]["acme-billing"]["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|t| t["routes"].as_array().cloned().unwrap_or_default())
        // Execution candidates are additional learned knowledge, silent without a local grant.
        // These replay assertions cover advisory delivery; the replay below still checks that
        // no extra directive reaches the agent.
        .filter(|r| !r.as_str().is_some_and(|s| s.contains("[execution only]")))
        .filter_map(|r| r.as_str().map(|s| s.split(' ').next().unwrap().to_string()))
        // Procedure cards (families of similar tasks, with the bundled model) are their own kind;
        // `adage-learn/tests/cards.rs` covers them.
        .filter(|id| !id.starts_with("card-"))
        .collect();
    v.sort();
    v
}

/// The directive text in a native hook response, whatever the harness.
fn context_of(out: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(out) else { return String::new() };
    v["hookSpecificOutput"]["additionalContext"].as_str().or_else(|| v["context"].as_str()).unwrap_or("").to_string()
}

/// Route ids in a response, in order.
fn routes_in(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"<adage-reflex [^>]*route="([^"]+)""#).unwrap();
    re.captures_iter(text).map(|c| c[1].to_string()).collect()
}

/// The response carries exactly the expected routes (by id prefix), and nothing else.
fn expect_routes(out: &str, want: &[&str], what: &str) {
    let all = routes_in(&context_of(out));
    // A procedure card may come with a task prompt (these histories are all one kind of task,
    // so they form a family); it is the only thing besides the expected routes that may.
    let got: Vec<&String> = all.iter().filter(|g| !g.starts_with("card-")).collect();
    assert_eq!(got.len(), want.len(), "{what}: got {got:?}, want {want:?}\n{out}");
    for w in want {
        assert!(got.iter().any(|g| g.starts_with(w)), "{what}: no {w} in {got:?}");
    }
    if want.is_empty() && all.is_empty() {
        assert_eq!(out, "", "{what}: silence");
    }
}

/// [`expect_routes`] for a turn's hook, with a diagnosis on failure: what the runtime observed in
/// this session (from `adage graph`), the edit-triggered routes it had, and the payload sent. A
/// failure on a platform we cannot run locally (Windows in CI) then says why, not only what.
fn expect_turn(sb: &Sandbox, payload: &Value, out: &str, want: &[&str], what: &str) {
    let got = routes_in(&context_of(out));
    let ok = got.len() == want.len() && want.iter().all(|w| got.iter().any(|g| g.starts_with(w)));
    if !ok {
        let g = sb.graph();
        let recent: Vec<&Value> = g["events"].as_array().map(|e| e.iter().rev().take(4).collect()).unwrap_or_default();
        let edits: Vec<&Value> = g["routes"]
            .as_array()
            .map(|r| r.iter().filter(|r| r["trigger"]["kind"] == "after_edit").map(|r| &r["trigger"]).collect())
            .unwrap_or_default();
        panic!(
            "{what}: got {got:?}, want {want:?}\nresponse: {out}\npayload: {payload}\nedit routes: {edits:?}\nrecent events (newest first): {recent:#?}\nsandbox: {}",
            sb.repo()
        );
    }
    expect_routes(out, want, what);
}

fn silent_or(treated: bool, v: &[&'static str]) -> Vec<&'static str> {
    if treated {
        v.to_vec()
    } else {
        vec![]
    }
}

// ---------------------------------------------------------------------------------------------
// Replays, one per harness, in native payloads

/// The hook binary a Claude replay runs, and the routes (by id prefix) holding a `predict` grant
/// there. Only adage-exec reads grants, so `adage hook` never predicts.
#[derive(Clone, Copy)]
enum Hook<'a> {
    Adage,
    Exec { bin: &'a Path, granted: &'a [&'a str] },
}

impl Hook<'_> {
    fn granted(&self, route: &str) -> bool {
        matches!(self, Hook::Exec { granted, .. } if granted.iter().any(|g| route.starts_with(g)))
    }
}

/// Claude Code: `PostToolBatch` per turn, `PostToolUseFailure` per failed call, Stop `block`.
fn replay_claude(sb: &Sandbox, sid: &str, arm: &str) {
    replay_claude_with(sb, sid, arm, Hook::Adage);
}

/// The JSON calls a prediction yields for these acts, as the binary renders them.
fn predicted_calls(acts: &[scenario::Act], cwd: &str) -> Value {
    acts.iter()
        .map(|a| match a {
            scenario::Act::Shell { cmd, .. } => json!({ "tool": "Bash", "input": { "command": cmd } }),
            scenario::Act::Read { path, .. } => {
                json!({ "tool": "Read", "input": { "file_path": format!("{cwd}/{path}") } })
            }
            scenario::Act::Edit { .. } => panic!("an edit is never predicted"),
        })
        .collect()
}

/// [`replay_claude`] with the mod loaded: its `Press` after the session starts, its `TurnStep`
/// at every step boundary of the turn (0 after the prompt, `k` after the k-th tool batch, and
/// `ending` after the response that ends the turn), and its two-phase `PreToolUse` before every
/// Bash call. Under `hook` with grants, the treated arm gets each granted step of
/// [`scenario::Replay::predict`] in the model's place: the mod's calls come back with `adage_`
/// ids, the agent's own turn of them is not replayed, and with the finish line granted no Stop
/// request follows. The control arm prints nothing and spools `would_predict`. Without a grant, or
/// under `adage`, every step is silent and spools nothing. Both arms notice the `make lint`
/// candidate once ([`scenario::Replay::noticed`]).
fn replay_claude_with(sb: &Sandbox, sid: &str, arm: &str, hook: Hook) {
    let r = replay_for(sb);
    let cwd = sb.repo();
    let treated = arm == "treated";
    let predictions: Vec<&scenario::Predict> = r.predict.iter().filter(|p| hook.granted(p.route)).collect();
    let mut rng = Rng::new(7);
    let turn_id = scenario::uuid4(&mut Rng::new(13));
    let send = |ev: &str, p: &Value| match hook {
        Hook::Adage => sb.hook("claude", ev, p, arm),
        Hook::Exec { bin, .. } => sb.hook_in_repo(bin, "claude", ev, p, arm),
    };
    let h = |ev: &str, extra: Value| send(ev, &claude::hook(sid, &cwd, ev, extra));
    assert_eq!(h("SessionStart", json!({ "source": "startup" })), "");
    // The mod asks for the session's press token before the agent acts: one, in either arm.
    let press: Value = serde_json::from_str(&send("Press", &claude::press_hook(sid, &cwd, "startup"))).unwrap();
    assert!(press["press"].as_str().is_some_and(|t| t.len() == 64), "{press}");
    let out = h("UserPromptSubmit", json!({ "prompt": r.prompt }));
    assert_eq!(send("Press", &claude::press_hook(sid, &cwd, "startup")), "", "the agent acted: no token now");
    expect_routes(&out, &silent_or(treated, &r.prompt_expect), "claude prompt");
    if treated {
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
        assert!(
            context_of(&out).contains(&format!("python3 scripts/wi-lookup {}", r.item.id)),
            "the prompt's work item is bound into the command"
        );
    }
    // One step boundary: what the binary answers, checked against the expectations. Returns the
    // prediction the treated arm made, with the route it names.
    let step = |index: u32, ending: bool| -> Option<(&scenario::Predict, String)> {
        let out = send("TurnStep", &claude::turn_step_hook(sid, &cwd, &turn_id, index, ending));
        let want = predictions.iter().copied().find(|p| p.index == index && p.ending == ending);
        let what = format!("claude TurnStep {index}{}", if ending { " ending" } else { "" });
        match want.filter(|_| treated) {
            None => {
                assert_eq!(out, "", "{what}: nothing in the model's place");
                None
            }
            Some(p) => {
                let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{what}: {e}: {out:?}"));
                let pr = &v["predict"];
                assert_eq!(pr["kind"], p.kind, "{what}: {out}");
                assert!(pr["route"].as_str().unwrap().starts_with(p.route), "{what}: {out}");
                assert_eq!(pr["track"].as_str().map(|t| t.is_empty()), Some(false), "{what}: {out}");
                assert_eq!(pr["calls"], predicted_calls(&p.acts, &cwd), "{what}");
                Some((p, pr["route"].as_str().unwrap().to_string()))
            }
        }
    };
    // The mod asks before a Bash call runs, whoever made it: the plan, then the decision.
    let substitutes = scenario::substitutions(&r);
    let mut pretools = r.pretool.iter();
    let pretool = |pretools: &mut std::slice::Iter<scenario::Pretool>, act: &scenario::Act, id: &str| {
        let scenario::Act::Shell { cmd, .. } = act else { return };
        let want = pretools.next().unwrap_or_else(|| panic!("no pretool expectation for {cmd}"));
        assert_eq!(&want.command, cmd, "pretool expectations follow the calls");
        let plan = send("PreToolUse", &claude::pretool_hook(sid, &cwd, cmd, id, true));
        let decision = send("PreToolUse", &claude::pretool_hook(sid, &cwd, cmd, id, false));
        match substitutes.iter().find(|s| &s.command == cmd) {
            // C6, T-C6-03/04: the dead end's plan names the learned command, and the mod's verdict
            // (the same permission decision for both) makes it in the treated arm only.
            Some(s) => check_substitute(sb, sid, arm, s, &plan, |route| {
                send(
                    "PreToolUse",
                    &claude::verdict_hook(sid, &cwd, cmd, id, json!({ "engine": "same", "substitute": route })),
                )
            }),
            None => assert_eq!(plan, decision, "PreToolUse {cmd}: the dry run answers what the call will"),
        }
        assert_eq!(decision, want.answer.unwrap_or(""), "PreToolUse {cmd}");
    };
    // The agent's own turn: its Bash calls checked before they run, then its batch.
    let agent_turn = |t: &Turn, rng: &mut Rng, pretools: &mut std::slice::Iter<scenario::Pretool>| {
        // The ids the batch below carries.
        let mut ids = rng.clone();
        for c in &t.calls {
            pretool(pretools, &c.act, &claude::tool_use_id(&mut ids));
        }
        // The treated arm ran the learned command in the dead end's place: no failure.
        let ran = substituted(t, &substitutes, treated);
        for (ev, p, want) in claude::turn_hooks(&ran, sid, &cwd, rng) {
            let out = send(ev, &p);
            expect_turn(sb, &p, &out, &silent_or(treated, &want), &format!("claude {ev}"));
            if !out.is_empty() {
                assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["hookSpecificOutput"]["hookEventName"], ev);
            }
        }
    };
    // Adage's step: the mod yields the calls in the model's place, the harness runs them (each
    // Bash call through the same checks) and reports them with the mod's ids, drawing what the
    // agent's own turn would have drawn. Returns whether it was the finish line.
    let adage_step =
        |p: &scenario::Predict, route: &str, index: u32, pretools: &mut std::slice::Iter<scenario::Pretool>| {
            let ids: Vec<(String, scenario::Act)> = p
                .acts
                .iter()
                .enumerate()
                .map(|(k, a)| (claude::predicted_tool_use_id(route, &turn_id, index, k), a.clone()))
                .collect();
            for (id, act) in &ids {
                pretool(pretools, act, id);
            }
            let batch = claude::predicted_batch(sid, &cwd, &ids);
            let replaced = r.work.iter().find(|t| t.calls.iter().map(|c| &c.act).eq(p.acts.iter()));
            let want: Vec<&str> =
                replaced.map(|t| t.calls.iter().flat_map(|c| c.expect.clone()).collect()).unwrap_or_default();
            expect_turn(sb, &batch, &send("PostToolBatch", &batch), &want, "claude predicted batch");
        };
    let mut index = 0u32;
    assert!(step(index, false).is_none(), "never at the prompt's own request");
    let mut work: std::collections::VecDeque<Turn> = r.work.iter().cloned().collect();
    while let Some(t) = work.pop_front() {
        agent_turn(&t, &mut rng, &mut pretools);
        index += 1;
        // Steps Adage makes before the model's next request: each replaces the agent's next turn.
        while let Some((p, route)) = step(index, false) {
            let next = work.pop_front().expect("a predicted step stands for an agent's turn");
            assert!(next.calls.iter().map(|c| &c.act).eq(p.acts.iter()), "Adage makes the step the agent made");
            adage_step(p, &route, index, &mut pretools);
            index += 1;
        }
    }
    // The model's response to request `index` ends the turn.
    let finish: Vec<Turn> = match step(index, true) {
        Some((p, route)) => {
            // The mod yields the call in the model's place: Adage's call meets the obligation, so
            // the Stop gate has nothing to ask, and the agent does not run it again.
            adage_step(p, &route, index, &mut pretools);
            index += 1;
            assert!(step(index, false).is_none(), "claude: not twice in a row");
            r.finish
                .iter()
                .map(|t| Turn {
                    say: t.say.clone(),
                    calls: t.calls.iter().filter(|c| !p.acts.contains(&c.act)).cloned().collect(),
                })
                .filter(|t| !t.calls.is_empty())
                .collect()
        }
        None => {
            let stop = h("Stop", json!({ "stop_hook_active": false }));
            if treated {
                let v: Value = serde_json::from_str(&stop).unwrap();
                assert_eq!(v["decision"], "block");
                assert!(
                    v["reason"].as_str().unwrap().starts_with("Adage finish-line check:")
                        && v["reason"].as_str().unwrap().contains("make gen")
                );
            } else {
                assert_eq!(stop, "", "control: no stop request");
            }
            // The request the Stop request made.
            index += 1;
            assert!(step(index, false).is_none());
            r.finish.clone()
        }
    };
    for t in &finish {
        agent_turn(t, &mut rng, &mut pretools);
        index += 1;
        assert!(step(index, false).is_none(), "nothing left to predict");
    }
    assert_eq!(pretools.next(), None, "every Bash call was checked before it ran");
    assert!(step(index, true).is_none(), "obligation met: nothing at the turn's end");
    let finish_predicted = treated && predictions.iter().any(|p| p.ending);
    assert_eq!(
        h("Stop", json!({ "stop_hook_active": treated && !finish_predicted })),
        "",
        "obligation met: nothing more"
    );
    assert_eq!(h("SessionEnd", json!({ "reason": "other" })), "");
    check_turn_steps(sb, sid, arm, &predictions, &turn_id);
    check_noticed(sb, sid, arm, &r);
}

/// The dead end's two phases (C6): the dry run plans the learned command with its one line of
/// context, in either arm and recording nothing; the verdict substitutes in the treated arm,
/// judged `substituted`, and in the control arm prints nothing and spools `would_substitute`, as
/// `demo/adage_metrics.py` reads them.
fn check_substitute(
    sb: &Sandbox,
    sid: &str,
    arm: &str,
    s: &scenario::Substitute,
    plan: &str,
    verdict: impl Fn(&str) -> String,
) {
    let what = format!("PreToolUse {} (dead end)", s.command);
    let v: Value = serde_json::from_str(plan).unwrap_or_else(|e| panic!("{what}: {e}: {plan:?}"));
    let sub = &v["plan"]["substitute"];
    let scenario::Act::Shell { cmd, .. } = &s.instead else { panic!("a Bash substitute") };
    assert_eq!(sub["command"].as_str(), Some(cmd.as_str()), "{what}: {plan}");
    let route = sub["route"].as_str().unwrap_or_default().to_string();
    assert!(route.starts_with(s.route), "{what}: {plan}");
    assert!(sub["context"].as_str().is_some_and(|c| c.starts_with(&s.context) && !c.contains('\n')), "{what}: {plan}");
    let spool = adage_store::Paths::at(sb.root.join("adage")).spool(sid);
    let before = adage_store::spool::read_all::<Value>(&spool).unwrap_or_default().len();
    let out = verdict(&route);
    let records: Vec<Value> = adage_store::spool::read_all(&spool).unwrap_or_default();
    assert_eq!(records.len(), before + 1, "{what}: one record, at the verdict");
    let rec = &records[before];
    assert_eq!(rec["kind"], "pretool", "{what}: {rec}");
    if arm == "treated" {
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["substitute"], *sub, "{what}");
        assert_eq!(rec["substituted"], true, "{what}: {rec}");
        assert_eq!(rec["decision"]["judgements"], json!([{ "kind": "substituted", "route": route }]), "{what}");
    } else {
        assert_eq!(out, "", "{what}: control: never substituted");
        assert_eq!((rec["would_substitute"].clone(), rec["withheld"].clone()), (json!(true), json!(true)), "{what}");
        assert_eq!(rec["decision"]["judgements"], json!([]), "{what}");
    }
}

/// `t` as it ran: in the treated arm, each call the mod substituted is the learned command.
fn substituted(t: &Turn, subs: &[scenario::Substitute], treated: bool) -> Turn {
    let mut t = t.clone();
    for c in t.calls.iter_mut().filter(|_| treated) {
        if let Some(s) = subs.iter().find(|s| matches!(&c.act, scenario::Act::Shell { cmd, .. } if cmd == &s.command)) {
            c.act = s.instead.clone();
        }
    }
    t
}

/// The session's `turn_step` spool records, as `demo/adage_metrics.py` reads them (`kind:
/// "turn_step"`, `decision.predicted`): one per step Adage made or would have made, none for an
/// empty step, in both arms at the same steps.
fn check_turn_steps(sb: &Sandbox, sid: &str, arm: &str, predictions: &[&scenario::Predict], turn_id: &str) {
    let spool = adage_store::Paths::at(sb.root.join("adage")).spool(sid);
    let records: Vec<Value> = adage_store::spool::read_all(&spool).unwrap_or_default();
    let steps: Vec<&Value> = records.iter().filter(|r| r["kind"] == "turn_step").collect();
    assert_eq!(steps.len(), predictions.len(), "{arm}: one turn_step record per prediction: {steps:#?}");
    for (rec, p) in steps.iter().zip(predictions) {
        let d = &rec["decision"];
        assert_eq!(rec["event"], "TurnStep");
        assert_eq!(d["would_predict"]["kind"], p.kind, "{arm}: {rec}");
        assert_eq!(d["predicted_at"], json!([turn_id, p.index]), "{arm}: {rec}");
        if arm == "treated" {
            assert_eq!(d["predicted"], d["would_predict"], "{arm}: {rec}");
        } else {
            assert!(d.get("predicted").is_none(), "control: never predicted: {rec}");
        }
    }
    // Each call the mod made is Adage's step, judged `predicted` once per response (never a
    // follow); the finish line is met once, whoever ran `make gen`.
    let judged = |kind: &str, route: &str| {
        records
            .iter()
            .filter(|r| r["kind"] != "turn_step")
            .flat_map(|r| r["decision"]["judgements"].as_array().cloned().unwrap_or_default())
            .filter(|j| j["kind"] == kind && j["route"].as_str().is_some_and(|x| x.starts_with(route)))
            .count()
    };
    for p in predictions {
        let want = usize::from(arm == "treated");
        assert_eq!(judged("predicted", p.route), want, "{arm}: Adage's {} judged predicted", p.route);
    }
    assert_eq!(judged("obligation_met", "finish-line-"), 1, "{arm}: make gen ran once, whoever ran it");
}

/// T-D2-05: the session notices the `make lint` candidate once, in both arms alike, at the batch
/// of the first `make test` after the code edit (a turn earlier in the treated arm, whose dead end
/// ran `make test` in its place); nothing about it reaches the agent.
fn check_noticed(sb: &Sandbox, sid: &str, arm: &str, r: &scenario::Replay) {
    let records: Vec<Value> =
        adage_store::spool::read_all(&adage_store::Paths::at(sb.root.join("adage")).spool(sid)).unwrap_or_default();
    let noticing: Vec<&Value> = records.iter().filter(|x| x["decision"]["noticed"].is_array()).collect();
    assert_eq!(noticing.len(), r.noticed.len(), "{arm}: {noticing:#?}");
    let candidates = sim_candidates(sb);
    for (rec, want) in noticing.iter().zip(&r.noticed) {
        let n = &rec["decision"]["noticed"];
        assert_eq!(n.as_array().unwrap().len(), 1, "{arm}: {rec}");
        let c = candidates.iter().find(|c| c["id"] == n[0]["candidate"]).unwrap_or_else(|| panic!("{arm}: {rec}"));
        assert_eq!(c["step"]["command"], want.step, "{arm}: {c}");
        assert_eq!(
            (n[0]["support"].as_u64(), n[0]["s_min"].as_u64(), n[0]["review"].as_bool()),
            (Some(2), Some(3), Some(false))
        );
        assert_eq!(rec["kind"], "batch", "{arm}: noticed at a tool batch");
        let at = format!("exec {}", want.at);
        assert!(rec["decision"]["observed"].as_array().unwrap().iter().any(|k| *k == at), "{arm}: {rec}");
        // The first such batch.
        let first = records
            .iter()
            .position(|x| x["decision"]["observed"].as_array().is_some_and(|o| o.iter().any(|k| *k == at)));
        assert_eq!(first, records.iter().position(|x| std::ptr::eq(x, *rec)), "{arm}: at the first {}", want.at);
        assert!(rec["decision"]["would_have"].as_array().unwrap().iter().all(|w| w["route"] != n[0]["candidate"]));
    }
}

/// The candidates learning counted for the sandbox's repository (`candidates.json`).
fn sim_candidates(sb: &Sandbox) -> Vec<Value> {
    let index: Value = serde_json::from_slice(&std::fs::read(sb.root.join("adage/index.json")).unwrap()).unwrap();
    index["candidates"]["acme-billing"].as_array().cloned().unwrap_or_default()
}

/// OpenCode: the payloads `plugin/opencode/adage.js` builds, one `tool.execute.after` per call;
/// the finish line is a `continue` at `session.idle`, whose prompt comes back via `chat.message`.
fn replay_opencode(sb: &Sandbox, sid: &str, arm: &str) {
    let r = replay_for(sb);
    let cwd = sb.repo();
    let treated = arm == "treated";
    let h = |ev: &str, extra: Value| sb.hook("opencode", ev, &opencode::payload(sid, &cwd, extra), arm);
    assert_eq!(h("session.created", json!({})), "");
    let out = h("chat.message", json!({ "prompt": r.prompt }));
    expect_routes(&out, &silent_or(treated, &r.prompt_expect), "opencode prompt");
    let turns = |turns: &[Turn]| {
        for t in turns {
            for (ev, p, want) in opencode::turn_hooks(t, sid, &cwd) {
                let out = sb.hook("opencode", ev, &p, arm);
                expect_turn(sb, &p, &out, &silent_or(treated, &want), &format!("opencode {ev} {}", p["tool"]));
                if !out.is_empty() {
                    assert!(serde_json::from_str::<Value>(&out).unwrap()["context"].is_string());
                }
            }
        }
    };
    turns(&r.work);
    let idle = h("session.idle", json!({ "stop_hook_active": false }));
    if treated {
        let reason = serde_json::from_str::<Value>(&idle).unwrap()["continue"].as_str().unwrap().to_string();
        assert!(reason.starts_with("Adage finish-line check:"));
        assert_eq!(h("chat.message", json!({ "prompt": reason })), "", "Adage's own continuation is not a prompt");
    } else {
        assert_eq!(idle, "");
    }
    turns(&r.finish);
    assert_eq!(h("session.idle", json!({ "stop_hook_active": treated })), "");
    assert_eq!(h("session.deleted", json!({})), "");
}

/// Codex: `PostToolUse` per call with no exit code; Stop `block` becomes a new `UserPromptSubmit`.
fn replay_codex(sb: &Sandbox, sid: &str, arm: &str) {
    let r = replay_for(sb);
    let cwd = sb.repo();
    let treated = arm == "treated";
    let mut rng = Rng::new(9);
    let h = |ev: &str, turn: &str, extra: Value| sb.hook("codex", ev, &codex::hook(sid, &cwd, ev, turn, extra), arm);
    assert_eq!(h("SessionStart", "", json!({ "source": "startup" })), "");
    let out = h("UserPromptSubmit", "1", json!({ "prompt": r.prompt }));
    expect_routes(&out, &silent_or(treated, &r.prompt_expect), "codex prompt");
    let turns = |turns: &[Turn], turn_id: &str, rng: &mut Rng| {
        for t in turns {
            for (ev, p, want) in codex::turn_hooks(t, sid, &cwd, turn_id, rng) {
                let out = sb.hook("codex", ev, &p, arm);
                expect_turn(sb, &p, &out, &silent_or(treated, &want), &format!("codex {ev} {}", p["tool_input"]));
                if !out.is_empty() {
                    assert_eq!(
                        serde_json::from_str::<Value>(&out).unwrap()["hookSpecificOutput"]["hookEventName"],
                        "PostToolUse"
                    );
                }
            }
        }
    };
    turns(&r.work, "1", &mut rng);
    let stop = h("Stop", "1", json!({ "stop_hook_active": false, "last_assistant_message": r.answer }));
    if treated {
        let v: Value = serde_json::from_str(&stop).unwrap();
        assert_eq!(v["decision"], "block");
        // Codex turns the reason into a new user prompt: Adage must not match prompt routes on it.
        assert_eq!(h("UserPromptSubmit", "2", json!({ "prompt": v["reason"] })), "");
    } else {
        assert_eq!(stop, "");
    }
    turns(&r.finish, "2", &mut rng);
    assert_eq!(h("Stop", "2", json!({ "stop_hook_active": treated, "last_assistant_message": r.answer })), "");
    assert_eq!(h("SessionEnd", "", json!({ "reason": "other" })), "");
}

// ---------------------------------------------------------------------------------------------
// Graph

/// The learned dead ends: one per failure signature before `make test`.
fn dead_ends(g: &Value) -> Vec<&Value> {
    g["routes"].as_array().unwrap().iter().filter(|r| r["id"].as_str().unwrap().starts_with("dead-end-")).collect()
}

/// The dead end that names this runner.
fn dead_end<'a>(g: &'a Value, avoid: &str) -> &'a Value {
    dead_ends(g).into_iter().find(|r| r["step"]["avoid"] == avoid).unwrap_or_else(|| panic!("no dead end for {avoid}"))
}

/// The held-out replay, expecting at the prompt the lookup and every dead end the sandbox learned
/// (each announced once, at the first prompt).
fn replay_for(sb: &Sandbox) -> scenario::Replay {
    let mut r = scenario::replay();
    let n = dead_ends(&sb.graph()).len();
    assert!(n >= 1, "no dead end learned");
    r.prompt_expect = std::iter::once("lookup-work-item").chain(std::iter::repeat_n("dead-end-", n)).collect();
    // A habit is noticed only while it is a candidate: learned from several histories at once,
    // `make lint` after `make test` has the support of a reflex.
    let candidates = sim_candidates(sb);
    r.noticed.retain(|n| candidates.iter().any(|c| c["step"]["command"] == n.step));
    r
}

fn route<'a>(g: &'a Value, prefix: &str) -> &'a Value {
    g["routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_str().unwrap().starts_with(prefix))
        .unwrap_or_else(|| panic!("no route {prefix}"))
}

/// One treated and one control replay of the same session: every route applied once per arm,
/// delivered only in the treated arm.
fn check_graph(sb: &Sandbox, harness: &str) {
    check_graph_predicting(sb, harness, &[]);
}

/// [`check_graph`] where routes held `predict` grants (`predicted`, by id prefix): the treated
/// arm's step of each was Adage's (judged `predicted`, never followed), the control arm's would
/// have been, and with the finish line among them no stop request was made.
fn check_graph_predicting(sb: &Sandbox, harness: &str, predicted: &[&str]) {
    let g = sb.graph();
    assert_eq!(
        g["sessions"],
        json!({ "total": 2, "treated": 1, "control": 1 }),
        "{harness}: both sessions, split by arm"
    );
    let mut all: Vec<(String, &Value)> = ["lookup-work-item", "read-linked-files", "finish-line-"]
        .iter()
        .map(|id| (id.to_string(), route(&g, id)))
        .collect();
    all.extend(dead_ends(&g).into_iter().map(|r| (r["id"].as_str().unwrap().to_string(), r)));
    for (id, r) in all {
        for arm in ["treated", "control"] {
            assert_eq!(r["arms"][arm]["sessions"], 1, "{harness} {id} {arm}");
            assert_eq!(r["arms"][arm]["applied"], 1, "{harness} {id} {arm}: applied (delivered or withheld) once");
        }
        assert_eq!(r["live"]["emitted"], 1, "{harness} {id}: delivered once, in the treated arm only");
        assert_eq!(r["live"]["ignored"], 0, "{harness} {id}");
    }
    let made = |prefix: &str| predicted.contains(&prefix);
    let lookup = route(&g, "lookup-work-item");
    let followed = u8::from(!made("lookup-work-item"));
    assert_eq!(lookup["arms"]["treated"]["followed"], followed, "{harness}: Adage's lookup is not a follow");
    assert_eq!(lookup["arms"]["control"]["followed"], 1, "{harness}: the control session looked it up unaided");
    assert_eq!(lookup["live"]["followed"], followed);
    assert_eq!(lookup["live"]["control"]["unaided"], 1);
    let fin = route(&g, "finish-line-");
    assert_eq!(
        (fin["arms"]["treated"]["met"].clone(), fin["arms"]["control"]["met"].clone()),
        (json!(1), json!(1)),
        "{harness}: make gen ran in both"
    );
    let dead = dead_end(&g, "python3 -m unittest");
    // Claude Code's replay has the mod's two-phase PreToolUse: the treated arm's dead end was
    // replaced before it ran (C6, T-C6-04), avoided and never a follow; the control arm ran it
    // and would have replaced it.
    let replaced = harness.starts_with("claude");
    assert_eq!(
        (dead["arms"]["treated"]["hit_avoided"].clone(), dead["arms"]["control"]["hit_avoided"].clone()),
        (json!(u8::from(!replaced)), json!(1)),
        "{harness}: unittest ran in the control replay, and in the treated one unless replaced"
    );
    let n = |arm: &str| dead["arms"][arm]["substituted"].as_u64().unwrap_or(0);
    assert_eq!((n("treated"), n("control")), (u64::from(replaced), u64::from(replaced)), "{harness}: substituted");
    assert_eq!(dead["arms"]["treated"]["followed"], 0, "{harness}: a substitution is not a follow");
    let events = g["events"].as_array().unwrap();
    let stops = events.iter().filter(|e| e["continue"] == true).count();
    let count = |key: &str, arm: &str| events.iter().filter(|e| e[key].is_string() && e["arm"] == arm).count();
    let n = predicted.len();
    assert_eq!(
        stops,
        1 - usize::from(made("finish-line-")),
        "{harness}: one stop request in the treated arm, unless Adage made the step"
    );
    assert_eq!(
        (count("predicted", "treated"), count("predicted", "control")),
        (n, 0),
        "{harness}: steps made in the model's place"
    );
    assert_eq!(
        (count("would_predict", "treated"), count("would_predict", "control")),
        (n, n),
        "{harness}: steps Adage would make, in both arms"
    );
    let judged_predicted =
        events.iter().filter(|e| e["judged"].as_array().is_some_and(|j| j.iter().any(|x| x[0] == "predicted"))).count();
    assert_eq!(judged_predicted, n, "{harness}: Adage's calls judged predicted, never followed");
    // T-D2-05: the `make lint` candidate is noticed once in each arm. Codex's own history learns
    // it after the failed test run (its candidate starts with that failure), and Codex's hooks
    // report no failure (no exit code), so a Codex replay never completes it.
    let noticed = |arm: &str| events.iter().filter(|e| e["noticed"].is_array() && e["arm"] == arm).count();
    let want = usize::from(harness != "codex");
    assert_eq!((noticed("treated"), noticed("control")), (want, want), "{harness}: noticed once per session");
}

/// All four route kinds, with `dead` dead ends: one per failure signature with enough support.
/// Each history's agents run `python3 -m unittest` (`ACME_ENV is not set`) or `python3 -m pytest`
/// (`No module named pytest`) before `make test`, picked at random, so a history names the pytest
/// dead end only where at least three sessions ran it.
fn assert_all_four(report: &Value, harness: &str, dead: usize) {
    assert_kinds(report, harness, dead);
    assert_eq!(report["sessions_used"], 12, "{harness}");
}

fn assert_kinds(report: &Value, harness: &str, dead: usize) {
    let got = learned_routes(report);
    let want: Vec<&str> = std::iter::repeat_n("dead-end-", dead)
        .chain(["finish-line-", "lookup-work-item", "read-linked-files"])
        .collect();
    assert_eq!(got.len(), want.len(), "{harness}: {got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(g.starts_with(w), "{harness}: {got:?}");
    }
}

/// Two sessions' ids as the harness would mint them, two seconds apart (two agents started together).
fn ids(harness: &str) -> (String, String) {
    let t = scenario::EPOCH_MS + 21 * 86_400_000 + 9 * 3_600_000;
    let mut rng = Rng::new(42);
    match harness {
        "claude" => (scenario::uuid4(&mut rng), scenario::uuid4(&mut rng)),
        "opencode" => {
            (opencode::ident("ses", t, 1, true, &mut rng), opencode::ident("ses", t + 2_000, 1, true, &mut rng))
        }
        _ => (scenario::uuid7(&mut rng, t), scenario::uuid7(&mut rng, t + 2_000)),
    }
}

// ---------------------------------------------------------------------------------------------

#[test]
fn claude_code_learn_then_replay() {
    let sb = Sandbox::new();
    let projects = adage_sim::fixtures_dir().join("claude/projects");
    let rep = sb.learn(&["--claude-projects", projects.to_str().unwrap()]);
    assert_all_four(&rep, "claude", 2);
    let (a, b) = ids("claude");
    replay_claude(&sb, &a, "treated");
    replay_claude(&sb, &b, "control");
    check_graph(&sb, "claude");
    // T-D2-05: `adage watch --once` shows the noticed candidate's row.
    let repo = sb.repo();
    let mut c = sb.command_of(
        Path::new(env!("CARGO_BIN_EXE_adage")),
        &["watch", "--once", "--size", "160x60", "--repo", &repo],
        "treated",
    );
    let o = c.env("NO_COLOR", "1").stdin(Stdio::null()).output().unwrap();
    let frame = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(frame.contains("noticed") && frame.contains("seen in 2 earlier sessions"), "{frame}");
}

/// `adage-exec`, built beside `adage` by a workspace build (`cargo build -p adage-exec`): a test
/// of one package does not build another's binaries.
fn adage_exec() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_BIN_EXE_adage")).with_file_name("adage-exec");
    p.is_file().then_some(p)
}

/// The person's `predict` grant on the learned route whose id begins `prefix`, as `adage review
/// delegate --predict` writes it (the key's digests are the route's own, so the file is not
/// written by hand), under the policy opt-in a call made from a hook needs.
fn grant_predict(sb: &Sandbox, prefix: &str) {
    std::fs::write(sb.root.join("adage/policy.toml"), "[exec]\nmax_level = \"E2\"\n").unwrap();
    let index: Value = serde_json::from_slice(&std::fs::read(sb.root.join("adage/index.json")).unwrap()).unwrap();
    let ir = index["routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["route"]["id"].as_str().is_some_and(|id| id.starts_with(prefix)))
        .unwrap_or_else(|| panic!("no learned {prefix}"));
    let track = ir["track"].as_str().unwrap().split('@').next().unwrap();
    let id = ir["route"]["id"].as_str().unwrap();
    let repo = sb.repo();
    let mut c = sb.command_of(
        Path::new(env!("CARGO_BIN_EXE_adage")),
        &["review", "delegate", track, id, "--predict", "--repo", &repo],
        "treated",
    );
    let o = c.env("ADAGE_ALLOW_NONINTERACTIVE", "1").stdin(Stdio::null()).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    let store: Value = serde_json::from_slice(&std::fs::read(sb.root.join("adage/approvals.json")).unwrap()).unwrap();
    let mine =
        store["approvals"].as_array().unwrap().iter().find(|x| x["route"].as_str().is_some_and(|r| r.ends_with(id)));
    assert_eq!(mine.map(|x| x["key"]["mode"].clone()), Some(json!("predict")), "{store}");
}

/// T-C4-04, T-C7-04, T-C8-04 (the sim's half): the mod's `turn.step` through `adage-exec hook`.
/// With a `predict` grant on the finish line, the treated arm's `make gen` is Adage's step at the
/// work turn's end (no Stop request); with grants on the lookup and the reads it names too, Adage
/// looks the item up after the look around and reads its linked files at the very next step, one
/// response of two calls (two model requests not made), then a model step. The control arm prints
/// nothing and spools `would_predict` at the same steps; without a grant, both arms are exactly
/// the `adage` replay with every step silent. Under `adage hook` the same steps are silent too
/// ([`claude_code_learn_then_replay`]).
#[test]
fn claude_code_predicts_under_adage_exec() {
    let Some(exec) = adage_exec() else {
        eprintln!("adage-exec is not built beside adage: skipping (cargo build -p adage-exec)");
        return;
    };
    let projects = adage_sim::fixtures_dir().join("claude/projects");
    let (a, b) = ids("claude");
    let sets: [&[&str]; 3] = [&[], &["finish-line-"], &["lookup-work-item", "read-linked-files", "finish-line-"]];
    for granted in sets {
        let sb = Sandbox::new();
        assert_all_four(&sb.learn(&["--claude-projects", projects.to_str().unwrap()]), "claude", 2);
        for prefix in granted {
            grant_predict(&sb, prefix);
        }
        let hook = Hook::Exec { bin: &exec, granted };
        replay_claude_with(&sb, &a, "treated", hook);
        replay_claude_with(&sb, &b, "control", hook);
        check_graph_predicting(&sb, &format!("claude under adage-exec, granted {granted:?}"), granted);
    }
}

/// The mod's attachment payloads through the binary (C5, C9, T-C9-04's binary half), both arms:
/// `Standing` after the start answers the standing dead ends for the cached section (so the first
/// prompt says only the lookup), a subagent spawned for the work item is told the lookup its own
/// prompt binds and the dead ends before it starts, and its tool batch (with its `agent_id`) is
/// decided like any other; `SkillPrompt` for a skill no person accepted answers nothing. The
/// control arm is told nothing anywhere. That a told subagent then makes no lookup of its own is
/// the live half.
#[test]
fn claude_code_standing_and_a_subagent_told_at_spawn() {
    let sb = Sandbox::new();
    let projects = adage_sim::fixtures_dir().join("claude/projects");
    assert_all_four(&sb.learn(&["--claude-projects", projects.to_str().unwrap()]), "claude", 2);
    let r = replay_for(&sb);
    let cwd = sb.repo();
    let (a, b) = ids("claude");
    for (sid, arm) in [(a, "treated"), (b, "control")] {
        let treated = arm == "treated";
        let send = |ev: &str, p: &Value| sb.hook("claude", ev, p, arm);
        let h = |ev: &str, extra: Value| send(ev, &claude::hook(&sid, &cwd, ev, extra));
        assert_eq!(h("SessionStart", json!({ "source": "startup" })), "");
        let standing: Value =
            serde_json::from_str(&send("Standing", &claude::standing_hook(&sid, &cwd, None))).unwrap();
        let text = standing["standing"].as_str().unwrap();
        assert_eq!(
            routes_in(text).iter().filter(|r| r.starts_with("dead-end-")).count(),
            if treated { 2 } else { 0 },
            "{arm}: {text}"
        );
        assert_eq!(standing["environment"], "", "{arm}: no environment is bound here");
        assert!(standing["compact"].as_str().is_some_and(|c| !c.is_empty()), "{arm}: compaction is told in both arms");
        assert!(standing.get("subagent").is_none());
        // The dead ends sit in the cached section: the first prompt says only the lookup.
        let out = h("UserPromptSubmit", json!({ "prompt": r.prompt }));
        expect_routes(&out, &silent_or(treated, &["lookup-work-item"]), &format!("{arm}: prompt after Standing"));
        let agent = json!({ "type": "Explore", "description": "Look up the work item",
                            "prompt": format!("Find what {} asks for and list the files it names.", r.item.id) });
        let spawn: Value =
            serde_json::from_str(&send("Standing", &claude::standing_hook(&sid, &cwd, Some(agent)))).unwrap();
        let told = spawn["subagent"].as_str().unwrap();
        if treated {
            assert!(told.contains(&format!("python3 scripts/wi-lookup {}", r.item.id)), "{told}");
            assert_eq!(routes_in(told).iter().filter(|r| r.starts_with("dead-end-")).count(), 2, "{told}");
        } else {
            assert_eq!(told, "", "control: the subagent is told nothing");
        }
        assert_eq!(spawn["standing"], standing["standing"], "{arm}: the session's own answer is unchanged");
        // The subagent reads what the item links: decided as any batch, and it says nothing.
        let reads: Vec<(String, scenario::Act)> =
            r.item.linked.iter().enumerate().map(|(k, p)| (format!("toolu_sub{k}"), scenario::Act::read(p))).collect();
        assert_eq!(send("PostToolBatch", &claude::subagent_batch(&sid, &cwd, "explore-1", &reads)), "");
        let skill = claude::skill_prompt_hook(&sid, &cwd, "release-checklist", "1. Bump the version.");
        assert_eq!(send("SkillPrompt", &skill), "", "{arm}: no accepted pack here");
    }
}

#[test]
fn opencode_learn_then_replay() {
    let sb = Sandbox::new();
    let rep = sb.learn_files(&adage_sim::fixture_files("opencode"));
    assert_all_four(&rep, "opencode", 2);
    let (a, b) = ids("opencode");
    replay_opencode(&sb, &a, "treated");
    replay_opencode(&sb, &b, "control");
    check_graph(&sb, "opencode");
}

#[test]
fn codex_learn_then_replay() {
    let sb = Sandbox::new();
    let sessions = adage_sim::fixtures_dir().join("codex/sessions");
    let rep = sb.learn(&["--codex-sessions", sessions.to_str().unwrap()]);
    assert_all_four(&rep, "codex", 1);
    let (a, b) = ids("codex");
    replay_codex(&sb, &a, "treated");
    replay_codex(&sb, &b, "control");
    check_graph(&sb, "codex");
}

/// Dead ends learned from Codex's rollouts (which record exit codes) are delivered in another
/// harness — and in Codex itself, at the first prompt (above), without any failure signal.
#[test]
fn learn_from_codex_history_deliver_in_opencode() {
    let sb = Sandbox::new();
    let rep = sb.learn_files(&adage_sim::fixture_files("codex"));
    assert_all_four(&rep, "codex", 1);
    let (a, b) = ids("opencode");
    replay_opencode(&sb, &a, "treated");
    replay_opencode(&sb, &b, "control");
    check_graph(&sb, "codex → opencode");
}

/// Each `adage condition` appends one summary line to `$ADAGE_HOME/learn.log`.
#[test]
fn learn_writes_one_summary_line_per_run() {
    let sb = Sandbox::new();
    let files = adage_sim::fixture_files("claude");
    sb.learn_files(&files);
    let rep = sb.learn_files(&files);
    let text = std::fs::read_to_string(sb.root.join("adage/learn.log")).unwrap();
    let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 2, "{text}");
    let last = &lines[1];
    assert_eq!((last["sessions_read"].clone(), last["sessions_used"].clone()), (json!(12), json!(12)));
    assert_eq!(last["index_generation"], rep["index_generation"]);
    let tracks = last["repos"]["acme-billing"]["tracks"].as_array().unwrap();
    assert!(tracks.iter().all(|t| t["change"].is_string() && t["routes"].as_u64().unwrap() > 0), "{last}");
    assert!(!text.contains(&*sb.root.to_string_lossy()), "no paths in the log");
}

/// All three histories at once: one repository, the same four routes, evidence from every harness.
#[test]
fn learn_from_all_three_histories_together() {
    let sb = Sandbox::new();
    let mut files = adage_sim::fixture_files("claude");
    files.extend(adage_sim::fixture_files("opencode"));
    files.extend(adage_sim::fixture_files("codex"));
    let rep = sb.learn_files(&files);
    assert_eq!((rep["sessions_read"].clone(), rep["sessions_used"].clone()), (json!(36), json!(36)));
    assert_eq!(rep["repos"].as_object().unwrap().len(), 1);
    assert_kinds(&rep, "all three", 2);
    let g = sb.graph();
    let lookup = route(&g, "lookup-work-item");
    // Nine sessions per harness look the work item up; ten per harness name one.
    assert_eq!(
        (lookup["evidence"]["support"].clone(), lookup["evidence"]["opportunities"].clone()),
        (json!(27), json!(30))
    );
    // Six sessions per harness hit a dead end before `make test`. Dead ends are keyed by failure
    // signature: the unittest runs (`ACME_ENV is not set`, 11 sessions) are one route's evidence,
    // and the pytest runs (`No module named pytest`, 7 sessions) another's.
    let dead = dead_end(&g, "python3 -m unittest");
    assert_eq!(dead["evidence"]["support"], 11);
    let pytest = dead_end(&g, "python3 -m pytest");
    assert_eq!(pytest["step"]["signature"], "No module named pytest");
    assert_eq!(pytest["evidence"]["support"], 7);
    assert_ne!(dead["id"], pytest["id"]);
    // Replaying in each harness against the combined index draws the same directives.
    let (a, _) = ids("claude");
    replay_claude(&sb, &a, "treated");
    let (a, _) = ids("opencode");
    replay_opencode(&sb, &a, "treated");
    let (a, _) = ids("codex");
    replay_codex(&sb, &a, "treated");
    assert_eq!(sb.graph()["sessions"], json!({ "total": 3, "treated": 3, "control": 0 }));
}

/// The OpenCode plugin (plugin/opencode/adage.js) driven through the whole held-out session by
/// plugin/opencode/test/session.test.mjs with a fake OpenCode client, against the real binary,
/// in both arms. Skipped without node.
#[test]
fn opencode_plugin_full_session_in_node() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found: skipping the OpenCode plugin session test");
        return;
    }
    let sb = Sandbox::new();
    assert_all_four(&sb.learn_files(&adage_sim::fixture_files("opencode")), "opencode", 2);
    let (src, _, code) =
        sb.run(&["install", "opencode", "--print", "--bin", env!("CARGO_BIN_EXE_adage")], b"", "treated");
    assert_eq!(code, 0);
    let plugin = sb.root.join("adage-plugin.mjs");
    std::fs::write(&plugin, src).unwrap();
    let r = replay_for(&sb);
    let (a, b) = ids("opencode");
    for (sid, arm) in [(a, "treated"), (b, "control")] {
        let script = sb.root.join(format!("script-{arm}.json"));
        std::fs::write(&script, opencode::plugin_script(&r, &sid, arm == "treated").to_string()).unwrap();
        let driver =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin/opencode/test/session.test.mjs");
        let o = Command::new("node")
            .arg(driver)
            .env("PLUGIN", &plugin)
            .env("SCRIPT", &script)
            .env("REPO", sb.repo())
            .env("ADAGE_HOME", sb.root.join("adage"))
            .env("ADAGE_AUTO_LEARN", "0")
            .env("HOME", sb.root.join("home"))
            .env("ADAGE_ARM", arm)
            .env_remove("ADAGE_DISABLE")
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert!(o.status.success() && text.contains("session ok"), "{arm}: {text}");
    }
    check_graph(&sb, "opencode plugin");
}

/// OpenCode session ids begin with 12 hex digits of the (inverted) creation time and Codex thread
/// ids are UUIDv7s, so sessions started close together share a long prefix. `adage graph` must
/// still count them as different sessions.
#[test]
fn graph_counts_sessions_whose_ids_share_a_prefix() {
    let sb = Sandbox::new();
    sb.learn_files(&adage_sim::fixture_files("opencode"));
    let (a, b) = ids("opencode");
    assert_eq!(a[..8], b[..8], "the ids share a prefix: {a} {b}");
    replay_opencode(&sb, &a, "treated");
    replay_opencode(&sb, &b, "treated");
    let (c, d) = ids("codex");
    assert_eq!(c[..8], d[..8], "{c} {d}");
    replay_codex(&sb, &c, "treated");
    replay_codex(&sb, &d, "treated");
    let g = sb.graph();
    assert_eq!(g["sessions"], json!({ "total": 4, "treated": 4, "control": 0 }));
    let lookup = route(&g, "lookup-work-item");
    assert_eq!(lookup["arms"]["treated"]["sessions"], 4);
    assert_eq!(lookup["arms"]["treated"]["applied"], 4);
    assert_eq!(lookup["arms"]["treated"]["followed"], 4);
    assert_eq!(lookup["live"]["sessions"], 4);
    let sessions: std::collections::BTreeSet<&str> =
        g["events"].as_array().unwrap().iter().filter_map(|e| e["session"].as_str()).collect();
    assert_eq!(sessions.len(), 4, "events name four sessions: {sessions:?}");
}
