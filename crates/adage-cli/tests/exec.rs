//! `adage review delegate | revoke` and `adage delegate on | off | status` (Track C
//! §3.2, §4.4): a person's decisions, refused inside agent sessions; approvals written through
//! adage-exec's API; hook registrations swapped between `adage hook` and `adage-exec hook`.

use adage_core::Effect;
use adage_tracks::testkit::{kinds, track};
use adage_tracks::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

struct World {
    _w: common::TempWorld,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        World::with(|_| {})
    }
    /// A world whose one route is changed by `f` before the index is compiled.
    fn with(f: impl Fn(&mut Route)) -> World {
        let w = common::TempWorld::new(&["home", "adage", "acme-billing/.git", "acme-billing/scripts", "bin"]);
        w.write("acme-billing/scripts/wi-lookup", "#!/usr/bin/env python3\nprint('{}')\n");
        w.write("acme-billing/tools/case", "#!/usr/bin/env python3\nprint('{}')\n");
        w.write("bin/adage-exec", "#!/bin/sh\n");
        let lookup = kinds::lookup("lookup-work-item", "python3 scripts/wi-lookup {id} --json")
            .until_intent_changes()
            .support(9, 10)
            .days(3);
        let mut t = track("acme-billing/work-items", "acme-billing").version("0.4.0").route(lookup).build();
        f(&mut t.routes[0]);
        w.home("adage").compile(&[t]);
        World { root: w.root.clone(), _w: w }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        common::scrub(&mut c);
        c.args(args)
            .current_dir(self.p("acme-billing"))
            .env("ADAGE_HOME", self.p("adage"))
            .env("ADAGE_POLICY", self.p("adage/managed.toml"))
            .env("HOME", self.p("home"))
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1");
        for (k, v) in env {
            c.env(k, v);
        }
        let o = c.output().unwrap();
        (
            String::from_utf8_lossy(&o.stdout).into(),
            String::from_utf8_lossy(&o.stderr).into(),
            o.status.code().unwrap_or(-1),
        )
    }
}

const FIELDS: &str = r#"{"files":{"type":"repo-path-list","json":"linked_files","max":5},"state":{"type":"enum","values":["Active","Closed"]}}"#;

fn allow<'a>() -> Vec<&'a str> {
    vec![
        "review",
        "delegate",
        "acme-billing/work-items",
        "lookup-work-item",
        "--level",
        "e1",
        "--fields",
        FIELDS,
        "--exit-when",
        "state=Closed",
    ]
}

#[test]
fn approving_execution_is_a_persons_decision_and_shows_what_would_run() {
    let w = World::new();
    let store = w.p("adage/approvals.json");
    let (_, err, code) = w.run(&allow(), &[("CLAUDECODE", "1")]);
    assert_eq!(code, 1);
    assert!(err.contains("will not run inside an agent session"), "{err}");
    assert!(!store.exists());
    let (_, err, code) = w.run(&["review", "delegate", "acme-billing/work-items", "lookup-work-item"], &[]);
    assert_eq!(code, 2, "--level is required (a usage error): {err}");

    let (out, err, code) = w.run(&allow(), &[]);
    assert_eq!(code, 0, "{err}");
    for want in [
        "argv        python3 scripts/wi-lookup {id} --json",
        "input       {id} from the prompt, must match ^W-\\d{4}$",
        "effect      observe, approved (the classifier says environment)",
        "sandbox     reads: repository, toolchain, cas",
        "script      scripts/wi-lookup (L-ref) sha256 ",
        "exit        when state is Closed",
        "evidence    9 of 10 traces over 3 days",
    ] {
        assert!(out.contains(want), "missing {want:?} in:\n{out}");
    }
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&store).unwrap()).unwrap();
    let a = &v["approvals"][0];
    assert_eq!(
        (a["route"].as_str(), a["key"]["level"].as_str()),
        (Some("acme-billing/work-items/lookup-work-item"), Some("e1"))
    );
    assert_eq!(a["key"]["scripts"][0][0], "scripts/wi-lookup");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&store).unwrap().permissions().mode() & 0o777, 0o600);
    }

    let (out, _, code) = w.run(&["delegate", "status", "--json"], &[]);
    assert_eq!(code, 0);
    let s: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(s["grants"]["acme-billing"][0]["route"], "acme-billing/work-items/lookup-work-item");
    assert_eq!(s["enabled"], false);

    let (_, _, code) =
        w.run(&["review", "revoke", "acme-billing/work-items", "lookup-work-item"], &[("OPENCODE", "1")]);
    assert_eq!(code, 1, "revoking is a person's decision too");
    let (out, _, code) = w.run(&["review", "revoke", "acme-billing/work-items", "lookup-work-item"], &[]);
    assert_eq!((code, out.contains("withdrawn   1 delegation")), (0, true), "{out}");
}

/// Review S5 and fresh-install B6: rewrites to the project's environment have their own switch
/// (`runtime.rewrite`), apart from execution: a plugin-only install (no adage-exec) turns them
/// off and on again, from a script or from `!` in an agent session; `delegate off` needs no
/// terminal, and neither `delegate on` nor `off` touches rewrites.
#[test]
fn rewrites_switch_on_and_off_without_adage_exec() {
    let w = World::new();
    let rewrite = || {
        let (out, _, _) = w.run(&["config", "get", "runtime.rewrite"], &[]);
        out.trim().to_string()
    };
    let scripted = [("ADAGE_ALLOW_NONINTERACTIVE", "0")];
    assert_eq!(rewrite(), "true", "on by default");
    let (out, err, code) = w.run(&["delegate", "rewrites", "off"], &scripted);
    assert_eq!(code, 0, "no terminal needed to turn it off: {err}");
    assert!(out.contains("off, from the next command"), "{out}");
    assert_eq!(rewrite(), "false");
    let (_, err, code) = w.run(&["delegate", "rewrites", "on"], &[("CLAUDECODE", "1")]);
    assert_eq!(code, 0, "`!adage delegate rewrites on` inside a session: {err}");
    assert_eq!(rewrite(), "true");
    // The older marker is cleared by `rewrites on`.
    std::fs::create_dir_all(w.p("adage/exec")).unwrap();
    std::fs::write(w.p("adage/exec/rewrite-off"), "").unwrap();
    w.run(&["delegate", "rewrites", "on"], &[]);
    assert!(!w.p("adage/exec/rewrite-off").exists());
    let (_, _, code) = w.run(&["delegate", "rewrites", "maybe"], &[]);
    assert_eq!(code, 2, "on|off only: a usage error");

    // Execution off: no terminal needed, and rewrites are left as they are.
    let (out, err, code) = w.run(&["delegate", "off"], &scripted);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("adage delegate rewrites on|off"), "{out}");
    assert_eq!(rewrite(), "true");
    let (_, _, code) = w.run(&["delegate", "on", "--bin", "/nonexistent/adage-exec"], &[]);
    assert_eq!(code, 1);
    let (_, _, code) = w.run(&["delegate", "on"], &scripted);
    assert_eq!(code, 1, "turning execution on needs a person");
}

#[test]
fn exec_enable_swaps_hook_registrations_and_disable_swaps_them_back() {
    let w = World::new();
    let settings = w.p("acme-billing/.claude/settings.local.json");
    let (_, err, code) = w.run(&["install", "claude-code", "--write"], &[]);
    assert_eq!(code, 0, "{err}");
    let before = std::fs::read_to_string(&settings).unwrap();
    assert!(!before.contains("adage-exec"));

    let bin = w.p("bin/adage-exec");
    let bin = bin.to_str().unwrap();
    let (_, err, code) = w.run(&["delegate", "on", "--bin", bin], &[("CODEX_SANDBOX", "1")]);
    assert_eq!(code, 1, "refused inside an agent session: {err}");
    let (_, err, code) = w.run(&["delegate", "on", "--bin", "/nonexistent/adage-exec"], &[]);
    assert_eq!(code, 1, "{err}");

    let (out, err, code) = w.run(&["delegate", "on", "--bin", bin], &[]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("now runs"), "{out}");
    let on: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    let h = &on["hooks"]["UserPromptSubmit"][0]["hooks"][0];
    assert_eq!(h["command"], bin);
    assert_eq!(
        h["args"],
        serde_json::json!(["hook", "claude", "UserPromptSubmit"]),
        "the same arguments as adage hook"
    );
    assert!(Path::new(&w.p("adage/exec/enabled")).is_file(), "the plugins' marker");
    // An action runs at the edit and Stop events: they get room for it; the others keep 5 s.
    assert_eq!(on["hooks"]["PostToolBatch"][0]["hooks"][0]["timeout"], 30);
    assert_eq!(on["hooks"]["Stop"][0]["hooks"][0]["timeout"], 30);
    assert_eq!(h["timeout"], 5);
    let (out, _, _) = w.run(&["delegate", "status", "--json"], &[]);
    let s: Value = serde_json::from_str(&out).unwrap();
    assert_eq!((s["enabled"].as_bool(), s["registrations"][0]["runs"].as_str()), (Some(true), Some("adage-exec")));

    let (_, err, code) = w.run(&["delegate", "off"], &[]);
    assert_eq!(code, 0, "{err}");
    let off = std::fs::read_to_string(&settings).unwrap();
    assert!(!off.contains("adage-exec"), "{off}");
    assert!(!off.contains("\"timeout\": 30"), "adage hook keeps 5 s everywhere: {off}");
    assert!(!w.p("adage/exec/enabled").exists());
}

#[test]
fn allow_exec_at_e2_is_for_actions_only() {
    let w = World::new();
    let mut args = allow();
    args[5] = "e2";
    args.truncate(6);
    std::fs::write(w.p("adage/policy.toml"), "[exec]\nmax_level = \"E2\"\n").unwrap();
    // A lookup is read-only: an E1 prefetch, never an E2 action.
    let (_, err, code) = w.run(&args, &[]);
    assert_eq!(code, 1);
    assert!(err.contains("is not an E2 action"), "{err}");
    assert!(!w.p("adage/approvals.json").exists());
    let mut e3 = allow();
    e3[5] = "e3";
    let (_, err, code) = w.run(&e3, &[]);
    assert_eq!(code, 1);
    assert!(err.contains("--level e1 or e2 is required"), "{err}");
}

/// Launch review: opting in to E2 is a person's command, not a hand-edited file. It names the
/// file it writes, keeps automatic approval off and the sandbox required, and is refused inside
/// an agent session; turning it back down needs no terminal. Hints say how, in plain words.
#[test]
fn delegate_policy_opts_in_to_e2_with_a_command() {
    let w = World::with(|r| {
        r.trigger = Trigger::AfterEdit { paths: "src/*.py".into() };
        r.step = Step::Run { command: "make test".into() };
        r.slots = vec![];
        r.effect = Effect::Verify;
    });
    std::fs::write(w.p("acme-billing/Makefile"), "test:\n\ttrue\n").unwrap();
    let file = w.p("adage/policy.toml");
    let e2 =
        ["review", "delegate", "acme-billing/work-items", "lookup-work-item", "--level", "e2", "--effect", "verify"];
    let (_, err, code) = w.run(&e2, &[]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("run `adage delegate policy --max-level e2` yourself"), "{err}");
    assert!(err.contains(file.to_str().unwrap()) && !err.contains("G4"), "the file, in plain words: {err}");

    let (out, err, code) = w.run(&["delegate", "policy"], &[]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("E1") && out.contains(file.to_str().unwrap()), "{out}");
    assert!(out.contains("adage delegate policy --max-level e2"), "{out}");
    assert!(!file.exists(), "showing it changes nothing");
    let (_, err, code) = w.run(&["delegate", "policy", "--max-level", "e2"], &[("CLAUDECODE", "1")]);
    assert_eq!(code, 1);
    assert!(err.contains("will not run inside an agent session"), "{err}");
    assert!(!file.exists());
    let (out, err, code) = w.run(&["delegate", "policy", "--max-level", "e2"], &[]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(file.to_str().unwrap()) && out.contains("adage review delegate"), "{out}");
    let text = std::fs::read_to_string(&file).unwrap();
    for want in ["max_level = \"E2\"", "auto_approve = \"off\"", "require_sandbox = true"] {
        assert!(text.contains(want), "{want}: {text}");
    }
    let (_, err, code) = w.run(&["delegate", "policy", "--max-level", "e1"], &[("ADAGE_ALLOW_NONINTERACTIVE", "0")]);
    assert_eq!(code, 0, "turning it down needs no terminal: {err}");
    assert!(std::fs::read_to_string(&file).unwrap().contains("max_level = \"E1\""));
}

/// A grant whose route no longer matches what would run (here its script changed) is listed by
/// `adage delegate status` and `adage review`, with what changed and the command that approves
/// it again, instead of being withheld silently.
#[test]
fn lapsed_grants_are_listed_with_what_changed_and_how_to_approve_again() {
    let w = World::new();
    let (_, err, code) = w.run(&allow(), &[]);
    assert_eq!(code, 0, "{err}");
    let (out, _, _) = w.run(&["delegate", "status", "--json"], &[]);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["lapsed"], serde_json::json!([]), "current: nothing lapsed");
    std::fs::write(w.p("acme-billing/scripts/wi-lookup"), "#!/usr/bin/env python3\nprint('{\"x\": 1}')\n").unwrap();
    let (out, _, _) = w.run(&["delegate", "status", "--json"], &[]);
    let v: Value = serde_json::from_str(&out).unwrap();
    let l = &v["lapsed"][0];
    assert_eq!(l["route"], "acme-billing/work-items/lookup-work-item", "{v}");
    assert!(l["changed"].as_array().unwrap().iter().any(|c| c == "scripts"), "{l}");
    assert!(
        l["reapprove"]
            .as_str()
            .unwrap()
            .starts_with("adage review delegate acme-billing/work-items lookup-work-item --level e1"),
        "{l}"
    );
    let (out, _, code) = w.run(&["delegate", "status"], &[]);
    assert_eq!(code, 0);
    assert!(out.contains("1 delegation no longer matches") && out.contains("a script it runs"), "{out}");
    let (out, _, code) = w.run(&["review", "--json"], &[]);
    assert_eq!(code, 0, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["exec_lapsed"][0]["route"], "acme-billing/work-items/lookup-work-item", "{v}");
}

/// Launch review: a `--timeout-ms` above the level's limit is clamped, and the approval says so.
#[test]
fn a_timeout_above_the_levels_limit_is_clamped_and_said() {
    let w = World::new();
    let mut args = allow();
    args.extend(["--timeout-ms", "9000"]);
    let (out, err, code) = w.run(&args, &[]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("3000 ms") && out.contains("--timeout-ms 9000 was clamped: E1 allows at most 3000 ms"),
        "{out}"
    );
    let mut within = allow();
    within.extend(["--timeout-ms", "2000"]);
    let (out, err, code) = w.run(&within, &[]);
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains("clamped"), "{out}");
}

/// A repository script the traces showed only reading (`tools/case`, whose name says nothing) is
/// learned observe with its effect evidence. It can be delegated at E1 like a script named for
/// reading, the evidence is shown at approval, and it is never approved automatically.
#[test]
fn a_script_inferred_observe_from_traces_can_be_delegated_at_e1_with_its_evidence_shown() {
    const NOTE: &str = "observe from 4 traces: never followed by changes; its output was read next in 4 of 4";
    let w = World::with(|r| {
        r.step = Step::Run { command: "python3 tools/case {id} --json".into() };
        r.evidence.effect_evidence = Some(NOTE.into());
    });
    std::fs::write(w.p("adage/policy.toml"), "[exec]\nauto_approve = \"inert-observe\"\n").unwrap();
    let (out, err, code) = w.run(&allow(), &[]);
    assert_eq!(code, 0, "{err}");
    for want in [
        "argv        python3 tools/case {id} --json",
        "effect      observe, approved (the classifier says environment)",
        &format!("inferred    {NOTE}"),
        "script      tools/case (L-ref) sha256 ",
        "automatic   no (",
    ] {
        assert!(out.contains(want), "missing {want:?} in:\n{out}");
    }
}

fn predict<'a>() -> Vec<&'a str> {
    vec!["review", "delegate", "acme-billing/work-items", "lookup-work-item", "--predict"]
}

/// Calls made in the agent's place are acting from a hook: the policy opt-in E2 actions need.
fn e2(w: &World) {
    std::fs::write(w.p("adage/policy.toml"), "[exec]\nmax_level = \"E2\"\n").unwrap();
}

/// T-C2-04: `--predict` is a person's decision, refused under every harness's agent marker and
/// writing nothing; at a terminal it records a predict key (no sandbox profile, no terms) with the
/// evidence it was granted on, and stages nothing.
#[test]
fn a_predict_grant_is_a_persons_decision() {
    let w = World::new();
    e2(&w);
    let store = w.p("adage/approvals.json");
    for var in adage_app::person::AGENT_ENV {
        let (_, err, code) = w.run(&predict(), &[(var, "1")]);
        assert_eq!(code, 1, "{var}: {err}");
        assert!(err.contains(var), "{var}: {err}");
        assert!(!store.exists(), "{var}");
    }
    let (out, err, code) = w.run(&predict(), &[]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("predict") && out.contains("in the agent's harness, under its own permissions"), "{out}");
    assert!(!out.contains("Staged once"), "nothing runs, so nothing is previewed: {out}");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&store).unwrap()).unwrap();
    let a = &v["approvals"][0];
    assert_eq!((a["key"]["mode"].as_str(), a["key"]["profile_id"].as_str()), (Some("predict"), Some("harness")));
    assert_eq!(a["key"]["level"], "e2");
    assert_eq!(a["key"]["scripts"][0][0], "scripts/wi-lookup");
    assert_eq!(a["basis"]["support"], 9);
    assert!(a.get("terms").is_none(), "{a}");
}

/// T-C2-05: the grant round-trips through `delegate status`, a lapse notice whose reapproval
/// ends in `--predict`, and `revoke --predict`, which withdraws it alone.
#[test]
fn a_predict_grant_round_trips_through_status_lapse_and_revoke() {
    let w = World::new();
    e2(&w);
    let (_, err, code) = w.run(&predict(), &[]);
    assert_eq!(code, 0, "{err}");
    let (out, _, _) = w.run(&["delegate", "status", "--json"], &[]);
    let v: Value = serde_json::from_str(&out).unwrap();
    let g = &v["grants"]["acme-billing"][0];
    assert_eq!(
        (g["route"].as_str(), g["mode"].as_str()),
        (Some("acme-billing/work-items/lookup-work-item"), Some("predict"))
    );
    assert_eq!((v["predict"].as_str(), v["lapsed"].clone()), (Some("on"), serde_json::json!([])));
    let (out, _, _) = w.run(&["delegate", "status"], &[]);
    assert!(out.contains("predict · in the agent's place"), "{out}");
    std::fs::write(w.p("acme-billing/scripts/wi-lookup"), "#!/usr/bin/env python3\nprint('{\"x\": 1}')\n").unwrap();
    let (out, _, _) = w.run(&["delegate", "status", "--json"], &[]);
    let v: Value = serde_json::from_str(&out).unwrap();
    let l = &v["lapsed"][0];
    assert_eq!(l["mode"], "predict", "{v}");
    assert!(l["changed"].as_array().unwrap().iter().any(|c| c == "scripts"), "{l}");
    assert_eq!(l["reapprove"], "adage review delegate acme-billing/work-items lookup-work-item --predict");
    let (out, _, _) = w.run(&["delegate", "status"], &[]);
    assert!(out.contains("1 delegation no longer matches") && out.contains("a script it runs"), "{out}");
    let revoke = ["review", "revoke", "acme-billing/work-items", "lookup-work-item"];
    let (out, _, code) = w.run(&revoke, &[]);
    assert_eq!((code, out.contains("withdrawn   0 delegations")), (0, true), "the execute grants only: {out}");
    let (out, _, code) = w.run(&[&revoke[..], &["--predict"]].concat(), &[]);
    assert_eq!((code, out.contains("withdrawn   1 delegation")), (0, true), "{out}");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(w.p("adage/approvals.json")).unwrap()).unwrap();
    assert_eq!(v["approvals"], serde_json::json!([]));
}

/// T-C2-06: `--predict` excludes `--effect` and `--level` (usage errors), and is refused, writing
/// nothing, below E2, for a reflex that already holds an execute grant (two paths would run it),
/// for one only adage-exec can carry out, and for an effect above regenerate.
#[test]
fn predict_is_exclusive_and_refused_where_it_would_not_hold() {
    let w = World::new();
    for extra in [["--effect", "verify"], ["--level", "e2"]] {
        let (_, err, code) = w.run(&[&predict()[..], &extra].concat(), &[]);
        assert_eq!(code, 2, "{extra:?} is a usage error: {err}");
    }
    let (_, err, code) = w.run(&predict(), &[]);
    assert_eq!(code, 1);
    assert!(err.contains("caps execution at E1") && err.contains("delegate policy --max-level e2"), "{err}");
    e2(&w);
    let (_, err, code) = w.run(&allow(), &[]);
    assert_eq!(code, 0, "{err}");
    let (_, err, code) = w.run(&predict(), &[]);
    assert_eq!(code, 1);
    assert!(err.contains("already holds an E1 grant") && err.contains("run it twice"), "{err}");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(w.p("adage/approvals.json")).unwrap()).unwrap();
    assert_eq!(v["approvals"].as_array().unwrap().len(), 1, "nothing written");
    let exec_only = World::with(|r| {
        r.evidence.execution = Some(ExecutionEvidence { opportunities: 5, successful: 5, sessions: 5 })
    });
    e2(&exec_only);
    let (_, err, code) = exec_only.run(&predict(), &[]);
    assert_eq!(code, 1);
    assert!(err.contains("never predicted"), "{err}");
    let environment = World::with(|r| r.effect = Effect::Environment);
    e2(&environment);
    let (_, err, code) = environment.run(&predict(), &[]);
    assert_eq!(code, 1);
    assert!(err.contains("at most regenerate"), "{err}");
    assert!(!environment.p("adage/approvals.json").exists());
}

/// T-C2-08: `adage delegate predict off` works from anywhere (it only turns something down) and
/// is what adage-exec reads at every step; `on` is a person's decision.
#[test]
fn the_predict_switch_is_off_from_anywhere_and_on_by_a_person() {
    let w = World::new();
    let marker = w.p("adage/exec/predict-off");
    let (out, err, code) = w.run(&["delegate", "predict", "off"], &[("CLAUDECODE", "1")]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("off, from the next step") && marker.is_file(), "{out}");
    let (out, _, _) = w.run(&["delegate", "status", "--json"], &[]);
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["predict"], "off");
    let (_, err, code) = w.run(&["delegate", "predict", "on"], &[("CLAUDECODE", "1")]);
    assert_eq!(code, 1, "{err}");
    assert!(marker.is_file());
    let (_, err, code) = w.run(&["delegate", "predict", "on"], &[]);
    assert_eq!(code, 0, "{err}");
    assert!(!marker.exists());
}
