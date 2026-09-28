//! `adage review allow-exec | revoke-exec` and `adage exec enable | disable | status` (Track C
//! §3.2, §4.4): a person's decisions, refused inside agent sessions; approvals written through
//! adage-exec's API; hook registrations swapped between `adage hook` and `adage-exec hook`.

use adage_core::spans::SpanKind;
use adage_core::Effect;
use adage_tracks::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["home", "adage", "acme-billing/.git", "acme-billing/scripts", "bin"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("acme-billing/scripts/wi-lookup"), "#!/usr/bin/env python3\nprint('{}')\n").unwrap();
        std::fs::write(root.join("bin/adage-exec"), "#!/bin/sh\n").unwrap();
        let t = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "acme-billing/work-items".into(),
                    version: "0.4.0".into(),
                    description: String::new(),
                    provenance: "desire-path".into(),
                },
                scope: Scope { repo: "acme-billing".into() },
                gauge: Gauge::default(),
            },
            routes: vec![Route {
                id: "lookup-work-item".into(),
                summary: String::new(),
                trigger: Trigger::Prompt { span: SpanKind::Ticket },
                step: Step::Run { command: "python3 scripts/wi-lookup {id} --json".into() },
                slots: vec![Slot {
                    name: "id".into(),
                    source: SlotSource::Prompt { span: SpanKind::Ticket },
                    contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
                }],
                effect: Effect::Observe,
                aspect: Aspect::Green,
                state: RouteState::Active,
                exit: vec![Exit::IntentChanges],
                evidence: Evidence { support: 9, opportunities: 10, days: 3, ..Default::default() },
                authored: false,
            }],
        };
        Index::compile(&[t]).write_atomic(&root.join("adage/index.json")).unwrap();
        World { _dir: dir, root }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        c.args(args)
            .current_dir(self.p("acme-billing"))
            .env("ADAGE_HOME", self.p("adage"))
            .env("ADAGE_POLICY", self.p("adage/managed.toml"))
            .env("HOME", self.p("home"))
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1");
        for v in ["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED", "ADAGE_ARM"] {
            c.env_remove(v);
        }
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
        "allow-exec",
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
    let (_, err, code) = w.run(&["review", "allow-exec", "acme-billing/work-items", "lookup-work-item"], &[]);
    assert_eq!(code, 1, "--level is required: {err}");

    let (out, err, code) = w.run(&allow(), &[]);
    assert_eq!(code, 0, "{err}");
    for want in [
        "argv:     python3 scripts/wi-lookup {id} --json",
        "input:    {id} from the prompt, must match ^W-\\d{4}$",
        "effect:   observe, approved (the classifier says environment)",
        "sandbox:  reads: repository, toolchain, cas",
        "script:   scripts/wi-lookup (L-ref) sha256 ",
        "exit:     when state is Closed",
        "evidence: 9 of 10 sessions over 3 days",
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

    let (out, _, code) = w.run(&["exec", "status", "--json"], &[]);
    assert_eq!(code, 0);
    let s: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(s["grants"]["acme-billing"][0]["route"], "acme-billing/work-items/lookup-work-item");
    assert_eq!(s["enabled"], false);

    let (_, _, code) =
        w.run(&["review", "revoke-exec", "acme-billing/work-items", "lookup-work-item"], &[("OPENCODE", "1")]);
    assert_eq!(code, 1, "revoking is a person's decision too");
    let (out, _, code) = w.run(&["review", "revoke-exec", "acme-billing/work-items", "lookup-work-item"], &[]);
    assert_eq!((code, out.contains("Revoked 1")), (0, true), "{out}");
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
    let (_, err, code) = w.run(&["exec", "enable", "--bin", bin], &[("CODEX_SANDBOX", "1")]);
    assert_eq!(code, 1, "refused inside an agent session: {err}");
    let (_, err, code) = w.run(&["exec", "enable", "--bin", "/nonexistent/adage-exec"], &[]);
    assert_eq!(code, 1, "{err}");

    let (out, err, code) = w.run(&["exec", "enable", "--bin", bin], &[]);
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
    let (out, _, _) = w.run(&["exec", "status", "--json"], &[]);
    let s: Value = serde_json::from_str(&out).unwrap();
    assert_eq!((s["enabled"].as_bool(), s["registrations"][0]["runs"].as_str()), (Some(true), Some("adage-exec")));

    let (_, err, code) = w.run(&["exec", "disable"], &[]);
    assert_eq!(code, 0, "{err}");
    let off = std::fs::read_to_string(&settings).unwrap();
    assert!(!off.contains("adage-exec"), "{off}");
    assert!(!w.p("adage/exec/enabled").exists());
}

#[test]
fn allow_exec_at_e2_approves_an_mcp_tool() {
    let w = World::new();
    let mut args = allow();
    args[5] = "e2";
    args.truncate(8); // no --exit-when: E2 results are returned whole
    let (out, err, code) = w.run(&args, &[]);
    assert_eq!(code, 0, "{err}");
    for want in [
        "level:    E2   output: typed",
        "input:    {id} a tool argument, must match ^W-\\d{4}$ (and be a plain token)",
        "tool:     adage_lookup_work_item (adage-exec mcp;",
        "automatic: no (",
        "Execution is off: `adage exec enable` turns on the adage-exec MCP server",
    ] {
        assert!(out.contains(want), "missing {want:?} in:\n{out}");
    }
    // Automatic approval stays off by default (§4.2, until gate G3).
    assert!(out.contains("automatic approval is off"), "{out}");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(w.p("adage/approvals.json")).unwrap()).unwrap();
    assert_eq!(v["approvals"][0]["key"]["level"], "e2");
    // E1 and E2 grants are separate: approving E1 keeps the E2 one.
    let (_, err, code) = w.run(&allow(), &[]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(w.p("adage/approvals.json")).unwrap()).unwrap();
    assert_eq!(v["approvals"].as_array().unwrap().len(), 2);
    let mut e3 = allow();
    e3[5] = "e3";
    let (_, err, code) = w.run(&e3, &[]);
    assert_eq!(code, 1);
    assert!(err.contains("E3"), "{err}");
}

#[test]
fn exec_enable_registers_the_mcp_server_for_codex_and_opencode_and_disable_removes_it() {
    let w = World::new();
    let env = [
        ("CODEX_HOME", w.p("home/.codex").to_string_lossy().to_string()),
        ("XDG_CONFIG_HOME", w.p("home/.config").to_string_lossy().to_string()),
    ];
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let codex = w.p("home/.codex/config.toml");
    let opencode = w.p("home/.config/opencode/opencode.json");
    std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
    let original = "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"other\"\n";
    std::fs::write(&codex, original).unwrap();
    for h in ["codex", "opencode"] {
        let (_, err, code) = w.run(&["install", h, "--write"], &env);
        assert_eq!(code, 0, "{err}");
    }
    let bin = w.p("bin/adage-exec");
    let bin = bin.to_str().unwrap();
    let (out, err, code) = w.run(&["exec", "enable", "--bin", bin], &env);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("codex: registered the MCP server `adage`"), "{out}");
    assert!(out.contains("+ [mcp_servers.adage]"), "the diff is shown: {out}");
    assert!(out.contains("opencode: registered the MCP server `adage`"), "{out}");
    let toml = std::fs::read_to_string(&codex).unwrap();
    assert!(toml.starts_with(original.trim_end()), "the rest of the file is kept:\n{toml}");
    assert!(toml.contains(&format!("[mcp_servers.adage]\ncommand = \"{bin}\"\nargs = [\"mcp\"]\n")), "{toml}");
    assert!(toml.contains("env = { ADAGE_HOME = "), "Codex passes a minimal environment: {toml}");
    assert_eq!(std::fs::read_to_string(w.p("home/.codex/config.toml.adage-backup")).unwrap(), original);
    let oc: Value = serde_json::from_str(&std::fs::read_to_string(&opencode).unwrap()).unwrap();
    assert_eq!(oc["mcp"]["adage"]["command"], serde_json::json!([bin, "mcp"]));
    assert_eq!(oc["mcp"]["adage"]["type"], "local");

    // Idempotent.
    let (out, _, code) = w.run(&["exec", "enable", "--bin", bin], &env);
    assert_eq!(code, 0);
    assert!(!out.contains("registered the MCP server"), "{out}");
    assert_eq!(std::fs::read_to_string(&codex).unwrap(), toml);
    let (out, _, _) = w.run(&["exec", "status", "--json"], &env);
    let s: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(s["mcp"].as_array().map(Vec::len), Some(2), "{s}");

    let (out, err, code) = w.run(&["exec", "disable"], &env);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("codex: removed the MCP server `adage`"), "{out}");
    assert_eq!(std::fs::read_to_string(&codex).unwrap(), original);
    let oc: Value = serde_json::from_str(&std::fs::read_to_string(&opencode).unwrap()).unwrap();
    assert!(oc.get("mcp").is_none(), "{oc}");
}
