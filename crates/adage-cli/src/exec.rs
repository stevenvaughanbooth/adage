//! Delegation: letting approved reflexes act, from the person's side (Track C §3.2, §4.4). The
//! `adage` binary never runs a reflex; these commands only change which binary the hooks call and
//! what this machine approves.
//!
//! ```text
//! adage delegate on [--bin <adage-exec>] [--repo <dir>]   hooks call `adage-exec hook` (E1 prefetch,
//!                                                          E2 actions)
//! adage delegate off [--repo <dir>]                        hooks call `adage hook` again
//! adage delegate rewrites <on|off>                         rewrites to the project's environment
//!                                                          alone (no adage-exec needed)
//! adage delegate predict <on|off>                          calls made in the agent's place, on
//!                                                          this machine (off: none, whatever is
//!                                                          granted)
//! adage delegate [status] [--repo <dir>] [--json]          registrations, and grants per repository
//! adage delegate policy [--max-level e1|e2]                this machine's execution policy, and
//!                                                          its file; e2 opts in to actions
//! adage review delegate <track> <reflex> --level e1 [--raw] [--fields '<json>']
//!                         [--exit-when field=value]... [--timeout-ms n] [--repo <dir>] [--by <name>]
//! adage review delegate <track> <reflex> --level e2 [--outputs <glob>]... [--cache <glob>]...
//!                         [--effect verify|regenerate] [--timeout-ms n] [--repo <dir>] [--by <name>]
//! adage review delegate <track> <reflex> --predict [--repo <dir>] [--by <name>]
//! adage review delegate <track> <reflex> --level e1|e2 [terms…] --stage --json
//!                                                          what the grant would be and do (no record)
//! adage review delegate <track> <reflex> --level e1|e2 [terms…] --confirm
//!                                                          record it, if unchanged (stdin: { "expect" })
//! adage review revoke <track> <reflex> [--predict] [--repo <dir>]
//! ```
//!
//! **Predict** (plan §2.2 item 3): a separate grant, beside E1 and E2, that lets the `TurnStep`
//! hook make the reflex's call in the agent's place. The agent's harness runs that call under its
//! own permissions; adage-exec runs nothing, so nothing is staged or previewed. It is keyed on what
//! the call would run (command template, contracts, scripts, effect) and lapses when that changes.
//! It is refused for a step above regenerate, a denied or (without the team's opt-in) environment
//! effect, a reflex only adage-exec can carry out, and a reflex that already holds an E1 or E2
//! grant (two paths would make the same call). Policy must allow E2: making a call is acting
//! from a hook. `adage delegate predict off` stops every predict grant at once, from anywhere;
//! `on` is a person's decision.
//!
//! **E2** (Track C §3.6, §4): an action a hook runs when the reflex's stimulus fires. It is
//! approved explicitly, for one reflex in one repository, never automatically. Before recording
//! anything, `delegate --level e2` stages the action once in the sandbox (nothing is applied)
//! and shows the full diff it would make to its declared outputs now; a staged run that changes
//! anything else, or fails, approves nothing. Policy must allow E2 (`max_level = "E2"`, which a
//! person sets with `adage delegate policy --max-level e2`: actions are an opt-in beta).
//!
//! A reflex that runs a **vendored script** (L-vendor, §1.7) shows the script's full text, from
//! the read-only CAS, and needs policy's `allow_vendor_scripts`; it is never approved
//! automatically. A pack's **attestations** are listed, marked self-reported by its signer; they
//! change nothing.
//!
//! `delegate on` and `delegate off` rewrite the hook registrations `adage install` wrote (Claude Code
//! settings, Codex `hooks.json`, the OpenCode plugin), in this repository and in the user scope,
//! swapping only the binary: `adage-exec hook <harness> <event>` takes exactly the arguments of
//! `adage hook`. For the Claude Code plugin they write or remove `$ADAGE_HOME/exec/enabled`, the
//! marker its session-start shim reads: with it the plugin's hooks call `adage-exec`, without it
//! `adage` (the plugin ships both, and registers its hooks once). A registration
//! that runs `adage-exec` gives the events an action runs at (the edit and Stop events) a 30 s
//! timeout; an action's own wall limit is 20 s.
//!
//! Every command here that turns something on refuses inside an agent session and needs a
//! terminal, or a press from the Claude Code session (architecture §3.12: `delegate on`, `delegate
//! policy --max-level e2`, `delegate predict on`, `review delegate`, `review revoke`). Under a
//! press `delegate on` writes the plugin's marker only. `review delegate … --stage --json` answers
//! what a grant would be and do, recording nothing; `--confirm` stages again and records it only if
//! nothing but the repository moved (it then answers the fresh stage instead). On a machine still
//! at E1 the first confirmed action writes the E2 policy and the marker with its grant.

use crate::install;
use adage_core::{repo, CommandTable};
use adage_exec::approvals::{self, Approval, ApprovalKey, Store};
use adage_exec::request::{FieldSpec, Level};
use adage_exec::route::{self, Terms};
use adage_store::Paths;
use adage_term::{Line, Out, Role};
use adage_tracks::Index;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: adage delegate <on [--bin <adage-exec>] | off | rewrites <on|off> | predict <on|off> | status [--json] | policy [--max-level e1|e2]> [--repo <dir>]";
const REVIEW_USAGE: &str = "usage: adage review delegate <track> <reflex> --level e1 [--raw] [--fields '<json>'] [--exit-when field=value]... [--timeout-ms n] [--repo <dir>] [--by <name>]
       adage review delegate <track> <reflex> --level e2 [--outputs <glob>]... [--cache <glob>]... [--effect verify|regenerate] [--timeout-ms n] [--repo <dir>] [--by <name>]
       adage review delegate <track> <reflex> --predict [--repo <dir>] [--by <name>]
       adage review delegate <track> <reflex> --level e1|e2 [...] --stage --json | --confirm
       adage review revoke <track> <reflex> [--predict] [--repo <dir>]";

use adage_trace::Harness;

/// Harnesses whose registrations `delegate on` swaps.
const HARNESSES: [&str; 3] = [Harness::ClaudeCode.id(), Harness::Codex.id(), Harness::OpenCode.id()];

/// A key and its value, in a key column wide enough for every harness name (`claude-code`) and
/// `delegations`, with two spaces to spare; never narrower than the usual one.
fn kv<W: std::io::Write>(o: &mut Out<W>, key: &str, value: Line) -> std::io::Result<()> {
    let widest = HARNESSES.iter().chain(&["delegations"]).map(|k| k.chars().count()).max().unwrap_or(0);
    let column = (widest + 2).max(adage_term::out::KEY_WIDTH).max(key.chars().count() + 2);
    o.line(&Line::new().faint(key).pad_to(column).extend(value))
}

/// Text from a script or a command's output, with control characters shown as `?`.
fn clean(line: &str) -> String {
    line.chars().map(|c| if c.is_control() && c != '\t' { '?' } else { c }).collect()
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn flags(args: &[String], name: &str) -> Vec<String> {
    args.windows(2).filter(|w| w[0] == name).map(|w| w[1].clone()).collect()
}

/// The marker the Claude Code plugins' session-start shims read.
pub fn marker(paths: &Paths) -> PathBuf {
    paths.root.join("exec").join("enabled")
}

fn repo_root(args: &[String]) -> Result<Option<PathBuf>> {
    let start = match flag(args, "--repo") {
        Some(r) => PathBuf::from(r),
        None => std::env::current_dir()?,
    };
    let start = start.canonicalize().with_context(|| format!("{} does not exist", start.display()))?;
    Ok(repo::find_root(&start))
}

/// The registration file of `harness` in `scope`, and which binary it runs: `Some("adage")`,
/// `Some("adage-exec")`, or `None` when it holds no Adage registration.
fn registered(harness: &str, scope: &str, root: Option<&Path>) -> Option<(PathBuf, String)> {
    let p = install::plan(harness, scope, root, Some(PathBuf::from("/adage-probe/adage"))).ok()?;
    let before = p.before?;
    let ours = match harness {
        "opencode" => before.starts_with("// Adage for OpenCode"),
        _ => {
            let v: Value = serde_json::from_str(&before).ok()?;
            let h = if harness == "codex" { "codex" } else { "claude" };
            let found =
                v["hooks"].as_object()?.values().filter_map(|l| l.as_array()).flatten().any(|e| install::is_ours(e, h));
            found
        }
    };
    if !ours {
        return None;
    }
    let exec = before.contains("adage-exec");
    Some((p.target, if exec { "adage-exec" } else { "adage" }.to_string()))
}

fn scopes(root: Option<&Path>) -> Vec<&'static str> {
    if root.is_some() {
        vec!["project", "user"]
    } else {
        vec!["user"]
    }
}

/// `adage delegate <on|off|status>`; bare `adage delegate` (or only flags) is `status`.
pub fn delegate(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("on") => switch(true, &args[1..], paths),
        Some("off") => switch(false, &args[1..], paths),
        // Like `adage config set runtime.rewrite`: it only turns an instinct's rewrite on or off
        // (the default is on), so it runs from a script, and from `!` inside an agent session.
        Some("rewrites") => match args.get(1).map(String::as_str) {
            Some(v @ ("on" | "off")) => rewrites(v == "on", paths),
            _ => bail!("{USAGE}"),
        },
        // Off only turns something down, so it runs from a script, and from `!` inside an agent
        // session; on is a person's decision.
        Some("predict") => match args.get(1).map(String::as_str) {
            Some(v @ ("on" | "off")) => predict(v == "on", paths),
            _ => bail!("{USAGE}"),
        },
        Some("status") => status(&args[1..], paths),
        Some("policy") => policy(&args[1..], paths),
        None => status(args, paths),
        Some(a) if a.starts_with("--") => status(args, paths),
        _ => bail!("{USAGE}"),
    }
}

/// Rewrites to the project's environment (docs/execution.md), on their own switch
/// (`runtime.rewrite`), apart from execution: they need no adage-exec, so a plugin-only install
/// turns them off and on again (review S5, fresh-install B6). The hook reads the switch at every
/// call.
fn rewrites(on: bool, paths: &Paths) -> Result<()> {
    adage_store::config::set(paths, "runtime.rewrite", if on { "true" } else { "false" })
        .map_err(|e| anyhow::anyhow!(e))?;
    // The marker `delegate off` wrote before the switch existed.
    let old = adage_hook::rewrite_off(paths);
    if on {
        match std::fs::remove_file(&old) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    let mut o = Out::stdout();
    o.header("delegate rewrites", &[if on { "on" } else { "off" }, "rewrites to the project's environment"])?;
    o.blank()?;
    if on {
        o.kv("rewrites", Line::new().text("on, from the next command"))?;
    } else {
        o.kv(
            "rewrites",
            Line::new()
                .text("off, from the next command")
                .sep()
                .quiet("Python test runs keep the interpreter the agent chose"),
        )?;
    }
    o.blank()?;
    if on {
        o.next("adage delegate rewrites off", "turn them off again")?;
    } else {
        o.next("adage delegate rewrites on", "turn them back on")?;
    }
    Ok(())
}

/// `adage delegate predict on|off`: the machine's switch for calls made in the agent's place. Off
/// writes `exec/predict-off`, which adage-exec reads at every `TurnStep`; grants are kept.
fn predict(on: bool, paths: &Paths) -> Result<()> {
    let marker = adage_exec::hook::predict_off(paths);
    if on {
        crate::review::person_or_press("delegate predict on", paths)?;
        match std::fs::remove_file(&marker) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    } else {
        std::fs::create_dir_all(marker.parent().context("no directory")?)?;
        std::fs::write(&marker, format!("{}\n", adage_exec::time::now()))?;
    }
    let mut o = Out::stdout();
    o.header("delegate predict", &[if on { "on" } else { "off" }, "calls made in the agent's place"])?;
    o.blank()?;
    if on {
        o.kv("predict", Line::new().text("on, from the next step").sep().quiet("only reflexes granted --predict"))?;
    } else {
        o.kv("predict", Line::new().text("off, from the next step").sep().quiet("grants are kept"))?;
    }
    o.blank()?;
    if on {
        o.next("adage delegate predict off", "stop every call made in the agent's place")?;
    } else {
        o.next("adage delegate predict on", "turn them back on (your decision)")?;
    }
    Ok(())
}

fn switch(on: bool, args: &[String], paths: &Paths) -> Result<()> {
    // Turning execution on is a person's decision; turning it off only turns something down, so
    // it also runs from a script or from `!` in an agent session (fresh-install B6).
    let pressed = if on {
        crate::review::person_or_press("delegate on", paths)?.0.via() == adage_tracks::review::Via::Press
    } else {
        false
    };
    if pressed {
        return switch_pressed(args, paths);
    }
    let here = std::env::current_exe().context("cannot locate the adage binary")?;
    let bin = if on {
        let b = match flag(args, "--bin") {
            Some(b) => PathBuf::from(b),
            None => {
                here.parent().context("no directory")?.join(if cfg!(windows) { "adage-exec.exe" } else { "adage-exec" })
            }
        };
        if !b.is_file() {
            return Err(crate::hint(
                format!("no adage-exec binary at {}", b.display()),
                "install it beside adage, or pass --bin <path to adage-exec>",
            ));
        }
        b
    } else {
        here
    };
    let root = repo_root(args)?;
    let mut rows: Vec<(String, String)> = Vec::new();
    for h in HARNESSES {
        for scope in scopes(root.as_deref()) {
            let Some((target, _)) = registered(h, scope, root.as_deref()) else { continue };
            let p = install::plan(h, scope, root.as_deref(), Some(bin.clone()))?;
            if p.before.as_deref() != Some(p.after.as_str()) {
                install::apply(&p)?;
                rows.push((h.to_string(), format!("{scope}: {} now runs {}", target.display(), bin.display())));
            }
        }
    }
    let m = marker(paths);
    let mut o = Out::stdout();
    if on {
        std::fs::create_dir_all(m.parent().context("no directory")?)?;
        let body = json!({ "enabled_at": adage_exec::time::now(), "bin": bin.to_string_lossy() });
        std::fs::write(&m, serde_json::to_string_pretty(&body)? + "\n")?;
        o.header("delegate on", &["hooks run delegated reflexes (E1 prefetch, E2 actions)"])?;
    } else {
        let _ = std::fs::remove_file(&m);
        o.header("delegate off", &["hooks call adage again"])?;
    }
    o.blank()?;
    for (h, what) in &rows {
        kv(&mut o, h, Line::new().text(what))?;
    }
    if rows.is_empty() {
        kv(&mut o, "hooks", Line::new().quiet("no registration written by `adage install` needed changing"))?;
    }
    if on {
        kv(
            &mut o,
            "plugin",
            Line::new().quiet(format!(
                "Claude Code plugin: no second install; with {} present its hooks call adage-exec",
                m.display()
            )),
        )?;
    } else {
        kv(&mut o, "delegations", Line::new().quiet("kept; `adage review revoke <track> <reflex>` withdraws one"))?;
    }
    kv(&mut o, "rewrites", Line::new().quiet("unchanged; they have their own switch: adage delegate rewrites on|off"))?;
    o.blank()?;
    if on {
        o.note(&format!(
            "Recommended: deny your agent edits to {} in the harness's permissions.",
            paths.root.display()
        ))?;
    }
    o.note("This takes effect in new agent sessions.")?;
    o.blank()?;
    if on {
        o.next("adage review delegate <track> <reflex> --level e1|e2", "nothing runs until you delegate a reflex")?;
    } else {
        o.next("adage delegate status", "registrations and delegated reflexes")?;
    }
    Ok(())
}

/// `adage delegate on` with a press (architecture §3.12): the marker only, which the Claude Code
/// plugin's shim reads at the next session start. Rewriting a settings file, Codex's `hooks.json`
/// or the OpenCode plugin is `install`'s work and stays a terminal's: each one found is named.
fn switch_pressed(args: &[String], paths: &Paths) -> Result<()> {
    let root = repo_root(args)?;
    let mut pending: Vec<(String, String)> = Vec::new();
    for h in HARNESSES {
        for scope in scopes(root.as_deref()) {
            if let Some((target, bin)) = registered(h, scope, root.as_deref()) {
                if bin != "adage-exec" {
                    pending.push((h.to_string(), format!("{scope}: {}", target.display())));
                }
            }
        }
    }
    let m = marker(paths);
    std::fs::create_dir_all(m.parent().context("no directory")?)?;
    let body = json!({ "enabled_at": adage_exec::time::now(), "bin": "plugin", "via": "press" });
    std::fs::write(&m, serde_json::to_string_pretty(&body)? + "\n")?;
    let mut o = Out::stdout();
    o.header("delegate on", &["the Claude Code plugin's hooks call adage-exec"])?;
    o.blank()?;
    kv(&mut o, "plugin", Line::new().text(format!("{} written", m.display())))?;
    for (h, what) in &pending {
        kv(
            &mut o,
            h,
            Line::new().text(what).sep().quiet("still runs adage; `adage delegate on` at a terminal switches it"),
        )?;
    }
    o.blank()?;
    o.note("This takes effect in new agent sessions.")?;
    Ok(())
}

/// `adage delegate policy [--max-level e1|e2]`: this machine's execution policy and the file it is
/// kept in. `--max-level e2` (a person's decision, refused in an agent session) opts in to E2
/// actions: it writes `max_level = "E2"` under `[exec]` in `$ADAGE_HOME/policy.toml`, with
/// automatic approval off and the sandbox required; each action still needs its own approval.
/// `--max-level e1` only turns execution down, so it runs from a script too.
fn policy(args: &[String], paths: &Paths) -> Result<()> {
    let file = adage_exec::policy::personal_path(&paths.root);
    let set = match flag(args, "--max-level") {
        Some(v) => Some(Level::parse(&v).with_context(|| format!("--max-level {v:?} is not e1 or e2"))?),
        None => None,
    };
    if set == Some(Level::E2) {
        crate::review::person_or_press("delegate policy --max-level e2", paths)?;
    }
    if let Some(level) = set {
        adage_exec::policy::set_max_level(&paths.root, level)?;
    }
    let pol = adage_exec::policy::load(&paths.root)?;
    let max = pol.max_level.as_str().to_uppercase();
    let mut o = Out::stdout();
    o.header("delegate policy", &[&format!("max level {max}"), if set.is_some() { "updated" } else { "unchanged" }])?;
    o.blank()?;
    kv(
        &mut o,
        "max level",
        Line::new().text(max.clone()).sep().quiet(if pol.max_level == Level::E2 {
            "approved actions may run from hooks, each after its own approval"
        } else {
            "read-only lookups at most; no actions"
        }),
    )?;
    kv(&mut o, "automatic", Line::new().text(if pol.auto_approve { "inert lookups" } else { "off" }))?;
    kv(&mut o, "sandbox", Line::new().text(if pol.require_sandbox { "required" } else { "not required" }))?;
    kv(&mut o, "file", Line::new().text(file.display().to_string()))?;
    if let Some(managed) = pol.sources.iter().find(|s| s.ends_with("(managed)")) {
        kv(&mut o, "managed", Line::new().text(managed.clone()).sep().quiet("its limits win"))?;
    }
    o.blank()?;
    if set.is_some_and(|l| l > pol.max_level) {
        o.note(&format!("A managed policy caps execution at {max} on this machine; your file asks for more."))?;
        o.blank()?;
    }
    if pol.max_level == Level::E2 {
        o.next(
            "adage review delegate <track> <reflex> --level e2 --effect verify",
            "approve one reflex's action (it is staged once first, and nothing runs until you approve)",
        )?;
    } else {
        o.next("adage delegate policy --max-level e2", "opt in to E2 actions on this machine (your decision)")?;
    }
    Ok(())
}

fn status(args: &[String], paths: &Paths) -> Result<()> {
    let root = repo_root(args)?;
    let mut regs = Vec::new();
    for h in HARNESSES {
        for scope in scopes(root.as_deref()) {
            if let Some((target, bin)) = registered(h, scope, root.as_deref()) {
                regs.push(json!({ "harness": h, "scope": scope, "file": target, "runs": bin }));
            }
        }
    }
    let store = Store::load(&approvals::path(&paths.root))?;
    let mut grants: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for a in &store.approvals {
        grants.entry(a.key.repo.clone()).or_default().push(json!({
            "route": a.route,
            "mode": a.key.mode.as_str(),
            "level": a.key.level.as_str(),
            "output": a.key.output_mode.as_str(),
            "argv": a.key.argv_templates.first().map(|v| v.join(" ")),
            "scripts": a.key.scripts,
            "approved_on": a.approved_on,
            "auto": a.auto,
        }));
    }
    let enabled = marker(paths).is_file();
    let lapsed = lapsed_here(paths, root.as_deref(), &store);
    // A harness registered twice runs every hook twice (`adage doctor` says the same).
    let dups = install::duplicates(&crate::detect::registrations(root.as_deref()), root.as_deref());
    if args.iter().any(|a| a == "--json") {
        let duplicates: Vec<Value> =
            dups.iter().map(|d| json!({ "harness": d.harness, "places": d.places, "fix": d.fix })).collect();
        let predict = if adage_exec::hook::predict_off(paths).exists() { "off" } else { "on" };
        let v = json!({ "enabled": enabled, "predict": predict, "registrations": regs, "grants": grants, "lapsed": lapsed, "duplicates": duplicates });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    let mut o = Out::stdout();
    let n: usize = grants.values().map(Vec::len).sum();
    o.header("delegate status", &[if enabled { "on" } else { "off" }, &crate::words::count(n, "delegated reflex")])?;
    o.blank()?;
    for r in &regs {
        kv(
            &mut o,
            r["harness"].as_str().unwrap_or(""),
            Line::new().text(format!(
                "{}: {} runs {}",
                r["scope"].as_str().unwrap_or(""),
                r["file"].as_str().unwrap_or(""),
                r["runs"].as_str().unwrap_or("")
            )),
        )?;
    }
    if regs.is_empty() {
        kv(&mut o, "hooks", Line::new().quiet("none registered by `adage install` here"))?;
    }
    if enabled {
        kv(&mut o, "plugin", Line::new().quiet("Claude Code: the adage-exec plugin serves"))?;
    }
    for d in &dups {
        o.blank()?;
        o.warn(&format!("{} is registered twice ({}): every hook runs twice", d.harness, d.places.join(" and ")))?;
        o.line(&Line::new().text("  ").faint("fix ").text(d.fix.replace('`', "")))?;
    }
    for (repo, gs) in &grants {
        o.blank()?;
        o.line(&Line::new().bold(Role::Ink, repo.clone()))?;
        for g in gs {
            let l = Line::new().text("  ").text(g["route"].as_str().unwrap_or("")).sep();
            let mut l = match g["mode"].as_str() {
                Some("predict") => l.text("predict").sep().quiet("in the agent's place"),
                _ => l
                    .text(g["level"].as_str().unwrap_or("").to_uppercase())
                    .sep()
                    .quiet(g["output"].as_str().unwrap_or("")),
            }
            .sep()
            .cmd(g["argv"].as_str().unwrap_or(""));
            if g["auto"] == true {
                l = l.sep().faint("automatic");
            }
            o.line(&l)?;
        }
    }
    print_lapsed(&mut o, &lapsed)?;
    o.blank()?;
    if let Some(cmd) = dups.first().and_then(|d| d.fix.split('`').nth(1)) {
        o.next(cmd, "leaves one registration")?;
    } else if grants.is_empty() {
        o.next("adage review delegate <track> <reflex> --level e1|e2", "no reflex is delegated on this machine")?;
    } else if !enabled {
        o.next("adage delegate on", "hooks run nothing delegated until it is on")?;
    } else {
        o.next("adage-exec doctor", "the sandbox this machine can enforce")?;
    }
    Ok(())
}

/// The grants for the repository at `root` (if any) that no longer match what would run there:
/// their routes do nothing until a person approves them again.
pub(crate) fn lapsed_here(paths: &Paths, root: Option<&Path>, store: &Store) -> Vec<route::Lapse> {
    let Some(root) = root else { return Vec::new() };
    let index = Index::load(&paths.index());
    let keys: std::collections::BTreeSet<&str> = index.routes.iter().map(|r| r.repo.as_str()).collect();
    let Some(key) = repo::lookup_key(root, Some(&paths.repo_keys()), &keys) else { return Vec::new() };
    let planned = route::plan_grants(&key, root, &index, &paths.tracks(), store, &CommandTable::load());
    route::lapses(&planned, store)
}

/// The lapsed grants, each with what changed and the command that approves it again.
pub(crate) fn print_lapsed<W: std::io::Write>(o: &mut Out<W>, lapsed: &[route::Lapse]) -> Result<()> {
    if lapsed.is_empty() {
        return Ok(());
    }
    o.blank()?;
    o.warn(&format!(
        "{} no longer {} what would run here, so {} nothing until approved again",
        crate::words::count(lapsed.len(), "delegation"),
        if lapsed.len() == 1 { "matches" } else { "match" },
        if lapsed.len() == 1 { "it runs" } else { "they run" }
    ))?;
    for l in lapsed {
        o.blank()?;
        o.line(&Line::new().bold(Role::Ink, l.route.clone()).sep().text(l.grant()))?;
        kv(o, "changed", Line::new().text(l.what()))?;
        kv(o, "approve", Line::new().cmd(l.reapprove.clone()).faint(" (after checking the change)"))?;
    }
    Ok(())
}

/// The index entry for `<track> <route>` in the repository at `root`.
fn indexed<'a>(
    index: &'a Index,
    root: &Path,
    paths: &Paths,
    track: &str,
    route_id: &str,
) -> Result<&'a adage_tracks::index::IndexedRoute> {
    let keys: std::collections::BTreeSet<&str> = index.routes.iter().map(|r| r.repo.as_str()).collect();
    let key = repo::lookup_key(root, Some(&paths.repo_keys()), &keys)
        .with_context(|| format!("no reflexes are installed for {} (see `adage status`)", root.display()))?;
    route::find(index, &key, track, route_id)
        .with_context(|| format!("no reflex {route_id} in track {track} for {key} (see `adage status`)"))
}

/// When an action runs, in words: its reflex's stimulus.
fn stimulus_line(r: &adage_tracks::Route) -> String {
    match &r.trigger {
        adage_tracks::Trigger::AfterEdit { paths } => {
            format!("an edit matches {paths} (and at Stop, if its finish line is still unmet)")
        }
        _ => "its stimulus fires".to_string(),
    }
}

/// A vendored script's full text, from the read-only CAS (Track C §1.7: shown in full at
/// approval).
fn print_vendored<W: std::io::Write>(o: &mut Out<W>, paths: &Paths, hex: &str) -> Result<()> {
    let p = crate::packs::cas_dir(paths).join(hex);
    let bytes =
        std::fs::read(&p).with_context(|| format!("vendored script {} is not in {}", &hex[..12], p.display()))?;
    if adage_pack::digest::sha256_hex(&bytes) != hex {
        return Err(crate::hint(
            format!("vendored script {} in the CAS does not match its hash", &hex[..12]),
            "run `adage add` again",
        ));
    }
    o.kv(
        "script",
        Line::new().text(format!("vendored (L-vendor) sha256 {hex}, {} bytes", bytes.len())).sep().quiet(format!(
            "run from {} by the interpreter in argv (its shebang is ignored); never approved automatically. Its full text:",
            p.display()
        )),
    )?;
    for line in String::from_utf8_lossy(&bytes).lines() {
        o.line(&Line::new().faint("    | ").text(clean(line)))?;
    }
    Ok(())
}

/// A pack's attestations, marked self-reported by its signer (Track C §1.8). They grant nothing.
fn print_attestations<W: std::io::Write>(
    o: &mut Out<W>,
    paths: &Paths,
    ir: &adage_tracks::index::IndexedRoute,
) -> Result<()> {
    if ir.exec.is_none() {
        return Ok(());
    }
    for line in crate::packs::attestation_lines(paths, &ir.track) {
        o.kv("attested", Line::new().quiet(line))?;
    }
    Ok(())
}

/// Stage `req` once under a trial store that approves it (and nothing else), applying nothing.
fn trial_stage(
    paths: &Paths,
    ir: &adage_tracks::index::IndexedRoute,
    req: &adage_exec::ExecRequest,
    label: &str,
    store: &Store,
    pol: &adage_exec::policy::Policy,
    table: &CommandTable,
) -> std::result::Result<adage_exec::stage::Staged, adage_exec::run::Refusal> {
    let mut trial = store.clone();
    trial.approve(Approval {
        key: ApprovalKey::for_request(req, &ir.repo),
        route: label.to_string(),
        approved_on: adage_exec::time::now(),
        by: None,
        basis: Value::Null,
        auto: false,
        terms: None,
        via: None,
    });
    let cap = adage_exec::sandbox::detect();
    let ctx = adage_exec::run::Context {
        inherited_env: std::env::vars().collect(),
        policy: pol,
        store: &trial,
        capability: &cap,
        table,
        repo_key: ir.repo.clone(),
        cas: Some(crate::packs::cas_dir(paths)).filter(|p| p.is_dir()),
        home: adage_store::home_dir(),
        canaries: vec![],
    };
    adage_exec::stage::stage(req, &ctx)
}

/// Stage the action once, apply nothing, and show the full diff it would make now. Refuses (and
/// approves nothing) when the staged run fails or changes anything the approval would not cover.
#[allow(clippy::too_many_arguments)]
fn preview<W: std::io::Write>(
    o: &mut Out<W>,
    paths: &Paths,
    root: &Path,
    ir: &adage_tracks::index::IndexedRoute,
    req: &adage_exec::ExecRequest,
    label: &str,
    store: &Store,
    pol: &adage_exec::policy::Policy,
    table: &CommandTable,
) -> Result<()> {
    o.blank()?;
    o.section("Staged once, to show what it would change now (nothing is applied)")?;
    o.flush()?;
    let staged = trial_stage(paths, ir, req, label, store, pol, table).map_err(|r| {
        let help = if r.stage == "policy" {
            "E2 actions are opt-in: run `adage delegate policy --max-level e2` yourself, then approve again"
        } else {
            "fix what it needs, then delegate again"
        };
        crate::hint(format!("the staged run was refused ({r}); nothing was approved"), help)
    })?;
    o.kv(
        "staged",
        Line::new()
            .text(staged.backend.line())
            .sep()
            .quiet(format!("exit {}", staged.exit.map(|e| e.to_string()).unwrap_or_else(|| "none".into()))),
    )?;
    if staged.exit != Some(0) {
        let err = String::from_utf8_lossy(&staged.stderr);
        for line in err.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev() {
            let clean: String = line.chars().filter(|c| !c.is_control() || *c == '\t').collect();
            o.line(&Line::new().faint("    ! ").quiet(clean))?;
        }
        return Err(crate::hint(
            "the staged run did not succeed; nothing was approved",
            "fix what it needs, then approve again",
        ));
    }
    if !staged.plan.refused.is_empty() {
        for (p, why) in &staged.plan.refused {
            o.kv("refused", Line::new().text(p.to_string()).sep().quiet(why.to_string()))?;
        }
        return Err(crate::hint(
            "the staged run changed paths the approval would not cover; nothing was approved",
            "declare its outputs (--outputs), or leave it at E0",
        ));
    }
    if staged.plan.apply.is_empty() {
        o.kv("diff", Line::new().quiet("none: the declared outputs are up to date now"))?;
    }
    for c in &staged.plan.apply {
        let before = std::fs::read(root.join(&c.path)).unwrap_or_default();
        let after = match &c.after {
            adage_exec::stage::After::File { bytes, .. } => bytes.clone(),
            _ => Vec::new(),
        };
        o.kv("diff", Line::new().text(format!("{} ({:?})", c.path, c.kind)))?;
        match (std::str::from_utf8(&before), std::str::from_utf8(&after)) {
            (Ok(a), Ok(b)) => {
                for line in crate::install::line_diff(a, b).lines() {
                    o.line(&Line::new().text(format!("    {}", clean(line))))?;
                }
            }
            _ => o.line(&Line::new().quiet(format!(
                "    (binary: {} bytes before, {} after)",
                before.len(),
                after.len()
            )))?,
        }
    }
    if !staged.plan.discarded.is_empty() {
        o.kv("discarded", Line::new().quiet(crate::words::count(staged.plan.discarded.len(), "cache path")))?;
    }
    Ok(())
}

/// `adage review delegate` and `adage review revoke`.
pub fn review(verb: &str, args: &[String], paths: &Paths) -> Result<()> {
    let (person, stdin) = crate::review::person_or_press(&format!("review {verb}"), paths)?;
    // Positional arguments, wherever the flags are (`--level e1 <track> <reflex>` works too).
    let mut pos: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--level" | "--fields" | "--exit-when" | "--timeout-ms" | "--repo" | "--by" | "--outputs" | "--cache"
            | "--effect" => i += 2,
            a if a.starts_with("--") => i += 1,
            _ => {
                pos.push(&args[i]);
                i += 1;
            }
        }
    }
    let (Some(track), Some(route_id), None) = (pos.first(), pos.get(1), pos.get(2)) else { bail!("{REVIEW_USAGE}") };
    let root = repo_root(args)?.context("not inside a repository; pass --repo")?;
    let index = Index::load(&paths.index());
    let ir = indexed(&index, &root, paths, track, route_id)?;
    let store_path = approvals::path(&paths.root);
    let mut store = Store::load(&store_path)?;
    let label = format!("{track}/{route_id}");
    let predict = args.iter().any(|a| a == "--predict");
    if verb == "revoke" {
        // Each grant is withdrawn on its own: revoking one never takes the other with it.
        let mode = if predict { approvals::GrantMode::Predict } else { approvals::GrantMode::Execute };
        let n = store.revoke(&ir.repo, Some(&label), Some(mode));
        store.save(&store_path)?;
        let mut o = Out::stdout();
        o.header("review revoke", &[&label, &ir.repo])?;
        o.blank()?;
        o.kv("withdrawn", Line::new().text(crate::words::count(n, "delegation")))?;
        o.blank()?;
        return Ok(o.next("adage delegate status", "what stays delegated")?);
    }
    if predict {
        return delegate_predict(args, paths, &root, ir, &label, store, &store_path, person.via());
    }
    let level = match flag(args, "--level").as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("e1") => Level::E1,
        Some("e2") => Level::E2,
        _ => bail!("--level e1 or e2 is required\n{REVIEW_USAGE}"),
    };
    let (outputs, cache) = (flags(args, "--outputs"), flags(args, "--cache"));
    let effect = match flag(args, "--effect") {
        Some(e) => Some(adage_core::Effect::parse(&e).with_context(|| format!("--effect {e:?} is not an effect"))?),
        None => None,
    };
    if level != Level::E2 && (!outputs.is_empty() || !cache.is_empty()) {
        bail!("--outputs and --cache declare an E2 action's change set; they need --level e2");
    }
    if ir.exec.is_some() && (!outputs.is_empty() || !cache.is_empty()) {
        bail!("a pack reflex's outputs and cache globs come from its pack (exec outputs, cache); they are not declared here");
    }
    let fields: BTreeMap<String, FieldSpec> = match flag(args, "--fields") {
        Some(f) => serde_json::from_str(&f).context("--fields must be a JSON object of field specs")?,
        None => BTreeMap::new(),
    };
    let mut exit_when = BTreeMap::new();
    for e in flags(args, "--exit-when") {
        let (k, v) = e.split_once('=').with_context(|| format!("--exit-when {e:?} is not field=value"))?;
        exit_when.insert(k.to_string(), v.to_string());
    }
    let timeout_ms = flag(args, "--timeout-ms").map(|t| t.parse::<u64>()).transpose().context("--timeout-ms")?;
    let terms = Terms {
        fields,
        exit_when,
        timeout_ms,
        raw: args.iter().any(|a| a == "--raw"),
        outputs,
        cache,
        effect: if level == Level::E2 { effect } else { None },
    };
    let confirm = args.iter().any(|a| a == "--confirm");
    if confirm || args.iter().any(|a| a == "--stage") {
        return staged(Staging {
            args,
            paths,
            root: &root,
            index: &index,
            ir,
            label: &label,
            level,
            terms,
            store,
            store_path: &store_path,
            via: person.via(),
            expect: stdin.expect,
            confirm,
        });
    }
    let table = CommandTable::load();
    let plan =
        route::plan(ir, &index, &root, &paths.tracks(), level, Some(&terms), &table).map_err(anyhow::Error::msg)?;
    let req = plan.request;
    if level != Level::E2 || !req.output.fields.is_empty() {
        req.output.validate()?;
    }
    for k in plan.exit_when.keys() {
        if !req.output.fields.contains_key(k) {
            bail!("--exit-when names {k}, which is not an output field");
        }
    }
    let classified = route::predict_classified(&req, &table);
    let pol = adage_exec::policy::load(&paths.root)?;
    pol.permits(&req, classified).map_err(|e| {
        if level == Level::E2 && e.contains("caps execution") {
            crate::hint(
                e,
                format!(
                    "E2 actions are opt-in: run `adage delegate policy --max-level e2` yourself (it sets max_level = \"E2\" under [exec] in {})",
                    adage_exec::policy::personal_path(&paths.root).display()
                ),
            )
        } else if e.contains("allow_vendor_scripts") {
            crate::hint(
                e,
                format!(
                    "set allow_vendor_scripts = true under [exec] in {} to allow vendored scripts on this machine",
                    adage_exec::policy::personal_path(&paths.root).display()
                ),
            )
        } else {
            anyhow::anyhow!(e)
        }
    })?;
    let key = ApprovalKey::for_request(&req, &ir.repo);
    let e = &ir.route.evidence;

    let mut o = Out::stdout();
    o.header("review delegate", &[&label, &format!("for {} on this machine", key.repo)])?;
    o.blank()?;
    o.kv("reflex", Line::new().text(label.clone()).sep().quiet(format!("track version {}", ir.version)))?;
    o.kv(
        "level",
        Line::new().text(level.as_str().to_uppercase()).sep().quiet(format!("output {}", req.output.mode.as_str())),
    )?;
    o.kv("argv", Line::new().cmd(req.argv.join(" ")))?;
    if let Some(p) = &req.project_python {
        o.kv("Python", Line::new().text(format!("{} (Python {})", p.program, p.version)))?;
        o.kv("runtime", Line::new().text(p.runtime_root.display().to_string()))?;
        o.kv("dependencies", Line::new().text(format!("sha256 {}", p.environment_sha256)))?;
        o.kv("runtime hash", Line::new().text(format!("sha256 {}", p.runtime_sha256)))?;
        o.note("This exact interpreter and dependency tree are approved and run from verified private snapshots. Changes require a new grant; editable project paths are rebased into the staged checkout.")?;
        o.note("Python uses -B and a fresh scratch bytecode-cache prefix, so cached bytecode cannot replace the approved source dependencies.")?;
    }
    for (k, v) in &req.contracts {
        let what = if level == Level::E1 {
            format!("{{{k}}} from the prompt, must match {v}")
        } else if plan.repo_paths.contains(k) {
            format!("{{{k}}} bound from the edited path: a repository path that exists, checked at every run")
        } else {
            format!("{{{k}}} bound from the edited path, must match {v} (and be a plain token)")
        };
        o.kv("input", Line::new().text(what))?;
    }
    let effect_word = if level == Level::E2 { req.effect.as_str() } else { "observe" };
    o.kv("effect", Line::new().text(format!("{effect_word}, approved (the classifier says {})", classified.as_str())))?;
    // A script the command table does not know, learned observe from what the traces show it
    // doing: the person approving sees why (it is never approved automatically: it runs a script).
    if let Some(note) = ir.route.evidence.effect_evidence.as_deref().filter(|n| !n.trim().is_empty()) {
        o.kv("inferred", Line::new().text(note.to_string()))?;
    }
    o.kv("sandbox", Line::new().text(req.profile.summary()))?;
    // A timeout outside the level's bounds is clamped (`Limits::with_timeout`): say so, and to what.
    if let Some(asked) = timeout_ms.filter(|ms| *ms != req.profile.limits.wall_ms) {
        let max =
            if level == Level::E2 { adage_exec::limits::E2_MAX_WALL_MS } else { adage_exec::limits::E1_MAX_WALL_MS };
        o.kv(
            "timeout",
            Line::new().text(format!("{} ms", req.profile.limits.wall_ms)).sep().quiet(format!(
                "--timeout-ms {asked} was clamped: {} allows at most {max} ms",
                level.as_str().to_uppercase()
            )),
        )?;
    }
    if let (Level::E2, Some(a)) = (level, &req.action) {
        o.kv(
            "runs",
            Line::new().text(format!(
                "from the hook, when {}; at most one action per hook event, within {} s",
                stimulus_line(&ir.route),
                adage_exec::limits::E2_MAX_WALL_MS / 1000
            )),
        )?;
        o.kv(
            "action",
            Line::new().text(format!(
                "{}; staged in a writable overlay, verified, then applied file by file",
                a.kind.as_str()
            )),
        )?;
        match a.kind {
            adage_exec::request::ActionKind::Regenerate => o.kv(
                "outputs",
                Line::new()
                    .text(format!("{} (only these may change", a.outputs.join(", ")))
                    .quiet("; anything else refuses the application)"),
            )?,
            adage_exec::request::ActionKind::Verify => {
                o.kv("outputs", Line::new().text("none").quiet(" (a verify action may not change sources)"))?
            }
        }
        if !a.cache.is_empty() {
            o.kv("cache", Line::new().text(a.cache.join(", ")))?;
        }
        o.kv(
            "revoked",
            Line::new().quiet(format!("automatically after {} refused change sets", adage_exec::stage::REVOKE_AFTER)),
        )?;
    }
    let hazards = adage_exec::git::hazards(&root);
    let actx = adage_exec::approvals::AutoContext {
        table: &table,
        git_hazards: &hazards,
        sandbox_enforced: adage_exec::sandbox::detect().available(),
        policy_auto: pol.auto_approve,
    };
    match adage_exec::approvals::auto_eligible(&req, &actx) {
        Ok(()) => o.kv(
            "automatic",
            Line::new().text("yes").quiet(": in the inert-observe class, and policy approves that class automatically"),
        )?,
        Err(why) => o.kv("automatic", Line::new().text("no").quiet(format!(" ({})", why.join("; "))))?,
    }
    if req.scripts.is_empty() {
        o.kv("scripts", Line::new().quiet("none (L-none)"))?;
    }
    for s in &req.scripts {
        match s.path.strip_prefix("cas:") {
            Some(hex) => print_vendored(&mut o, paths, hex)?,
            None => o.kv(
                "script",
                Line::new()
                    .text(format!("{} (L-ref) sha256 {}", s.path, s.sha256))
                    .quiet("; re-read and checked before every run"),
            )?,
        }
    }
    for (name, f) in &req.output.fields {
        o.kv("output", Line::new().text(format!("{name}: {}", serde_json::to_string(f)?)))?;
    }
    for (f, v) in &plan.exit_when {
        o.kv("exit", Line::new().text(format!("when {f} is {v}, nothing further is directed")))?;
    }
    o.kv(
        "evidence",
        Line::new().text(format!(
            "{} of {} traces over {} days{}",
            e.support,
            e.opportunities,
            e.days,
            if e.by_origin.is_empty() { String::new() } else { format!(", by origin {:?}", e.by_origin) }
        )),
    )?;
    print_attestations(&mut o, paths, ir)?;
    if level == Level::E2 {
        preview(&mut o, paths, &root, ir, &req, &label, &store, &pol, &table)?;
    }
    store.approve(Approval {
        key,
        route: label.clone(),
        approved_on: adage_exec::time::now(),
        by: flag(args, "--by"),
        basis: json!({
            "support": e.support,
            "opportunities": e.opportunities,
            "days": e.days,
            "by_origin": e.by_origin,
            "effect_evidence": e.effect_evidence,
        }),
        auto: false,
        terms: Some(terms),
        via: Some(person.via()),
    });
    store.save(&store_path)?;
    o.blank()?;
    o.kv("recorded", Line::new().text(store_path.display().to_string()))?;
    o.blank()?;
    if req.scripts.iter().any(|s| s.path.starts_with("cas:")) {
        o.note("A vendored script is code from the pack's signer: Adage will run exactly these bytes, read-only, with no network.")?;
    } else if !req.scripts.is_empty() {
        o.note("A repository script is code: Adage will run the pinned bytes, read-only, with no network.")?;
    }
    o.note("The delegation lapses if the reflex, its script or its sandbox profile changes.")?;
    o.blank()?;
    if !marker(paths).is_file() {
        o.next("adage delegate on", "execution is off; this makes hooks run delegated reflexes")?;
    } else {
        o.next("adage delegate status", "every delegated reflex")?;
    }
    Ok(())
}

/// `adage review delegate <track> <reflex> --predict`: the person's grant that lets the `TurnStep`
/// hook make this reflex's call in the agent's place (plan §2.2 item 3). Nothing runs here, so
/// there is no preview; what is shown is what the call would run and the evidence for it.
#[allow(clippy::too_many_arguments)]
fn delegate_predict(
    args: &[String],
    paths: &Paths,
    root: &Path,
    ir: &adage_tracks::index::IndexedRoute,
    label: &str,
    mut store: Store,
    store_path: &Path,
    via: adage_tracks::review::Via,
) -> Result<()> {
    if ["--level", "--effect", "--outputs", "--cache", "--fields", "--exit-when", "--timeout-ms", "--raw"]
        .iter()
        .any(|f| args.iter().any(|a| a == f))
    {
        bail!("--predict grants a call in the agent's place; it takes no level or execution terms\n{REVIEW_USAGE}");
    }
    // Two paths would make the same call: adage-exec's run (or prefetch) and the agent's.
    if let Some(a) =
        store.approvals.iter().find(|a| a.key.repo == ir.repo && a.route == label && a.key.mode.is_execute())
    {
        return Err(crate::hint(
            format!(
                "{label} already holds an {} grant here, so adage-exec runs it; predicting the same call too would run it twice",
                a.key.level.as_str().to_uppercase()
            ),
            format!("withdraw that grant first (`adage review revoke {} {}`), or keep it and leave this reflex unpredicted", ir.track, ir.route.id),
        ));
    }
    let plan = route::plan_predict(ir, root, &paths.tracks()).map_err(anyhow::Error::msg)?;
    let req = plan.request;
    let table = CommandTable::load();
    let classified = route::predict_classified(&req, &table);
    let pol = adage_exec::policy::load(&paths.root)?;
    pol.permits(&req, classified).map_err(|e| {
        if e.contains("caps execution") {
            crate::hint(
                e,
                format!(
                    "a call made in the agent's place is acting from a hook: run `adage delegate policy --max-level e2` yourself (it sets max_level = \"E2\" under [exec] in {})",
                    adage_exec::policy::personal_path(&paths.root).display()
                ),
            )
        } else {
            anyhow::anyhow!(e)
        }
    })?;
    let key = ApprovalKey::for_request(&req, &ir.repo);
    let e = &ir.route.evidence;
    let mut o = Out::stdout();
    o.header("review delegate", &[label, &format!("predict, for {} on this machine", key.repo)])?;
    o.blank()?;
    o.kv("reflex", Line::new().text(label.to_string()).sep().quiet(format!("track version {}", ir.version)))?;
    o.kv(
        "grant",
        Line::new().text("predict").sep().quiet("Adage makes this call in the agent's place, when the reflex is due"),
    )?;
    o.kv("argv", Line::new().cmd(req.argv.join(" ")))?;
    for (k, v) in &req.contracts {
        o.kv("input", Line::new().text(format!("{{{k}}} bound by Adage before the call, must match {v}")))?;
    }
    o.kv("effect", Line::new().text(format!("{} (the classifier says {})", req.effect.as_str(), classified.as_str())))?;
    o.kv("runs", Line::new().text("in the agent's harness, under its own permissions; adage-exec runs nothing"))?;
    if req.scripts.is_empty() {
        o.kv("scripts", Line::new().quiet("none (L-none)"))?;
    }
    for s in &req.scripts {
        o.kv(
            "script",
            Line::new().text(format!("{} sha256 {}", s.path, s.sha256)).quiet("; a change lapses the grant"),
        )?;
    }
    o.kv(
        "evidence",
        Line::new().text(format!(
            "{} of {} traces over {} days{}",
            e.support,
            e.opportunities,
            e.days,
            if e.by_origin.is_empty() { String::new() } else { format!(", by origin {:?}", e.by_origin) }
        )),
    )?;
    print_attestations(&mut o, paths, ir)?;
    store.approve(Approval {
        key,
        route: label.to_string(),
        approved_on: adage_exec::time::now(),
        by: flag(args, "--by"),
        basis: json!({
            "support": e.support,
            "opportunities": e.opportunities,
            "days": e.days,
            "by_origin": e.by_origin,
            "effect_evidence": e.effect_evidence,
        }),
        auto: false,
        terms: None,
        via: Some(via),
    });
    store.save(store_path)?;
    o.blank()?;
    o.kv("recorded", Line::new().text(store_path.display().to_string()))?;
    o.blank()?;
    o.note("The grant lapses if the reflex's command, its contracts or a script it runs changes.")?;
    if adage_exec::hook::predict_off(paths).exists() {
        o.note("Predictions are off on this machine (`adage delegate predict off`); nothing is predicted until they are on.")?;
    }
    o.blank()?;
    if !marker(paths).is_file() {
        o.next("adage delegate on", "only adage-exec predicts; this makes hooks call it")?;
    } else {
        o.next("adage delegate status", "every delegated reflex")?;
    }
    Ok(())
}

/// What `review delegate --stage` and `--confirm` need, as [`review`] parsed it.
struct Staging<'a> {
    args: &'a [String],
    paths: &'a Paths,
    root: &'a Path,
    index: &'a Index,
    ir: &'a adage_tracks::index::IndexedRoute,
    label: &'a str,
    level: Level,
    terms: Terms,
    store: Store,
    store_path: &'a Path,
    via: adage_tracks::review::Via,
    expect: Option<String>,
    confirm: bool,
}

/// The grant asked for, planned against the repository as it is now: its key, terms
/// (as declared, or as the binary proposes them) and the facts a person decides on.
struct Staged {
    key: ApprovalKey,
    terms: Terms,
    proposed: bool,
    facts: Value,
    snapshot: Value,
    /// Why the staged run cannot be granted as it is (it failed, or changed something the grant
    /// would not cover); `None` when it can.
    refused: Option<String>,
}

/// `review delegate … --stage --json` (the first act: answer what the grant would be and do, record
/// nothing) and `--confirm` (the second: plan and stage again, and record the grant only if the
/// key and terms are what the first act answered). Architecture §3.12, "Stage, then confirm":
/// nothing is kept between the two acts but the digest the caller hands back on stdin.
fn staged(s: Staging<'_>) -> Result<()> {
    let table = CommandTable::load();
    let current = adage_exec::policy::load(&s.paths.root)?;
    // A machine still at E1 stages its first action under the policy a confirm would write, for
    // the trial store only; the confirm then writes it with the grant.
    let raise = s.level == Level::E2 && current.max_level < Level::E2;
    let pol = if raise {
        adage_exec::policy::trial_e2(&s.paths.root)?.ok_or_else(|| {
            crate::hint(
                "a managed policy caps execution at E1 on this machine, so no action can be granted here",
                format!("the cap is in {}", adage_exec::policy::managed_path().display()),
            )
        })?
    } else {
        current
    };
    let plan = plan_grant(&s, &pol, &table)?;
    let digest = route::grant_digest(&plan.key, &plan.terms, &plan.snapshot);
    let answer = |recorded: bool, extra: Value| -> Value {
        let mut v = json!({
            "route": s.label,
            "repo": plan.key.repo,
            "level": s.level.as_str(),
            "terms": plan.terms,
            "proposed": plan.proposed,
            "facts": plan.facts,
            "first": raise || !marker(s.paths).is_file(),
            "ok": plan.refused.is_none(),
            "digest": digest,
            "recorded": recorded,
        });
        if let Some(why) = &plan.refused {
            v["why"] = json!(why);
        }
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        v
    };
    if !s.confirm {
        println!("{}", answer(false, json!({})));
        return Ok(());
    }
    let expect = s.expect.as_deref().ok_or_else(|| {
        crate::hint(
            "--confirm records the grant a stage answered, and reads that stage's digest on stdin",
            "pipe { \"expect\": <digest> } from `adage review delegate … --stage --json`",
        )
    })?;
    match route::digest_change(expect, &digest) {
        route::DigestChange::Same => {}
        route::DigestChange::Snapshot => {
            // The agent kept working: show the fresh stage, record nothing until it is confirmed.
            println!("{}", answer(false, json!({ "changed": "snapshot" })));
            return Ok(());
        }
        route::DigestChange::Terms(fields) => {
            return Err(crate::hint(
                format!(
                    "{} changed since it was staged ({}); nothing was recorded",
                    s.label,
                    fields.iter().map(|f| f.replace('_', " ")).collect::<Vec<_>>().join(", ")
                ),
                "stage it again, and decide on what it is now",
            ));
        }
        route::DigestChange::Malformed => {
            bail!("--confirm: {expect:?} is not a digest `--stage --json` answered; nothing was recorded")
        }
    }
    if let Some(why) = &plan.refused {
        return Err(crate::hint(format!("{why}; nothing was recorded"), "fix what it needs, then stage it again"));
    }
    // The first execution grant on this machine turns execution on with it: the policy, the
    // marker and the grant are one decision (architecture §3.12).
    let mut wrote = Vec::new();
    if raise {
        adage_exec::policy::set_max_level(&s.paths.root, Level::E2)?;
        wrote.push("policy");
    }
    let m = marker(s.paths);
    if !m.is_file() {
        std::fs::create_dir_all(m.parent().context("no directory")?)?;
        let body = json!({ "enabled_at": adage_exec::time::now(), "bin": "plugin", "via": s.via.as_str() });
        std::fs::write(&m, serde_json::to_string_pretty(&body)? + "\n")?;
        wrote.push("marker");
    }
    let e = &s.ir.route.evidence;
    let mut store = s.store;
    store.approve(Approval {
        key: plan.key.clone(),
        route: s.label.to_string(),
        approved_on: adage_exec::time::now(),
        by: flag(s.args, "--by"),
        basis: json!({
            "support": e.support,
            "opportunities": e.opportunities,
            "days": e.days,
            "by_origin": e.by_origin,
            "effect_evidence": e.effect_evidence,
        }),
        auto: false,
        terms: Some(plan.terms.clone()),
        via: Some(s.via),
    });
    store.save(s.store_path)?;
    wrote.push("grant");
    println!("{}", answer(true, json!({ "wrote": wrote, "takes_effect": "in new sessions" })));
    Ok(())
}

/// Plan the grant `s` asks for, stage it once when it is an action, and gather the facts.
fn plan_grant(s: &Staging<'_>, pol: &adage_exec::policy::Policy, table: &CommandTable) -> Result<Staged> {
    let tracks = s.paths.tracks();
    let mut terms = s.terms.clone();
    // An action with nothing declared: the binary proposes what a trial stage shows it writing.
    let proposed = s.level == Level::E2 && s.ir.exec.is_none() && terms.outputs.is_empty() && terms.cache.is_empty();
    if proposed {
        let changed = probe_writes(s, pol, table)?;
        let (outputs, cache) = route::propose_outputs(&changed);
        if s.ir.route.effect != adage_core::Effect::Verify {
            terms.outputs = outputs;
        }
        terms.cache = cache;
    }
    let plan = route::plan(s.ir, s.index, s.root, &tracks, s.level, Some(&terms), table).map_err(anyhow::Error::msg)?;
    let req = plan.request;
    if s.level != Level::E2 || !req.output.fields.is_empty() {
        req.output.validate()?;
    }
    let classified = adage_exec::classify::effect(&req.argv, table);
    pol.permits(&req, classified).map_err(anyhow::Error::msg)?;
    let key = ApprovalKey::for_request(&req, &s.ir.repo);
    let mut scripts = Vec::new();
    for sc in &req.scripts {
        let text = match sc.path.strip_prefix("cas:") {
            Some(hex) => std::fs::read(crate::packs::cas_dir(s.paths).join(hex)).ok(),
            None => std::fs::read(s.root.join(&sc.path)).ok(),
        };
        scripts.push(json!({
            "path": sc.path,
            "sha256": sc.sha256,
            "text": text.map(|b| String::from_utf8_lossy(&b).into_owned()),
        }));
    }
    let inputs: Vec<Value> = req
        .contracts
        .iter()
        .map(|(k, v)| {
            let from = if s.level == Level::E1 {
                "the prompt"
            } else if plan.repo_paths.contains(k) {
                "the edited path, a repository path"
            } else {
                "the edited path"
            };
            json!({ "slot": k, "contract": v, "from": from })
        })
        .collect();
    let e = &s.ir.route.evidence;
    let mut facts = json!({
        "argv": req.argv,
        "inputs": inputs,
        "effect": { "approved": req.effect.as_str(), "classified": classified.as_str() },
        "output": { "mode": req.output.mode.as_str(), "fields": req.output.fields },
        "exit_when": plan.exit_when,
        "scripts": scripts,
        "sandbox": req.profile.summary(),
        "profile_id": key.profile_id,
        "route_exec_digest": key.route_exec_digest,
        "evidence": { "support": e.support, "opportunities": e.opportunities, "days": e.days },
        "attestations": if s.ir.exec.is_some() { crate::packs::attestation_lines(s.paths, &s.ir.track) } else { Vec::new() },
    });
    if let Some(p) = &req.project_python {
        facts["python"] = json!({
            "program": p.program,
            "version": p.version,
            "runtime": p.runtime_root,
            "runtime_sha256": p.runtime_sha256,
            "environment_sha256": p.environment_sha256,
        });
    }
    let mut snapshot = Value::Null;
    let mut refused = None;
    if s.level == Level::E2 {
        let a = req.action.as_ref().context("an E2 grant plans an action")?;
        facts["action"] = json!({ "kind": a.kind.as_str(), "outputs": a.outputs, "cache": a.cache });
        match trial_stage(s.paths, s.ir, &req, s.label, &s.store, pol, table) {
            Err(r) => refused = Some(format!("the staged run was refused ({r})")),
            Ok(st) => {
                let mut diff = Vec::new();
                for c in &st.plan.apply {
                    let before = std::fs::read(s.root.join(&c.path)).ok();
                    let after = match &c.after {
                        adage_exec::stage::After::File { bytes, .. } => Some(bytes.clone()),
                        _ => None,
                    };
                    let text =
                        match (before.as_deref().map(std::str::from_utf8), after.as_deref().map(std::str::from_utf8)) {
                            (Some(Err(_)), _) | (_, Some(Err(_))) => None,
                            (b, a) => Some(crate::install::line_diff(
                                b.and_then(std::result::Result::ok).unwrap_or(""),
                                a.and_then(std::result::Result::ok).unwrap_or(""),
                            )),
                        };
                    diff.push(json!({
                        "path": c.path,
                        "change": format!("{:?}", c.kind).to_lowercase(),
                        "before_sha256": before.as_deref().map(adage_exec::sha256_hex),
                        "after_sha256": after.as_deref().map(adage_exec::sha256_hex),
                        "diff": text,
                    }));
                }
                let refusals: Vec<Value> =
                    st.plan.refused.iter().map(|(p, why)| json!({ "path": p, "why": why })).collect();
                facts["staged"] = json!({
                    "backend": st.backend.line(),
                    "exit": st.exit,
                    "diff": diff,
                    "refused": refusals,
                    "discarded": st.plan.discarded.len(),
                });
                if st.exit != Some(0) {
                    let err = String::from_utf8_lossy(&st.stderr);
                    facts["staged"]["stderr"] =
                        json!(err.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>());
                    refused = Some("the staged run did not succeed".to_string());
                } else if !st.plan.refused.is_empty() {
                    refused = Some("the staged run changed paths the grant would not cover".to_string());
                }
                snapshot = json!({ "inputs": st.inputs, "exit": st.exit, "changes": facts["staged"]["diff"].clone(), "refused": refusals });
            }
        }
    }
    Ok(Staged { key, terms, proposed, facts, snapshot, refused })
}

/// What the route's action writes now: staged once with an output glob that matches nothing, so
/// every change it makes is refused, and listed. These are the paths a proposal is made from.
fn probe_writes(s: &Staging<'_>, pol: &adage_exec::policy::Policy, table: &CommandTable) -> Result<Vec<String>> {
    let probe = Terms { outputs: vec![PROBE_OUTPUT.into()], cache: Vec::new(), ..s.terms.clone() };
    let plan = route::plan(s.ir, s.index, s.root, &s.paths.tracks(), Level::E2, Some(&probe), table)
        .map_err(anyhow::Error::msg)?;
    let staged = trial_stage(s.paths, s.ir, &plan.request, s.label, &s.store, pol, table)
        .map_err(|r| anyhow::anyhow!("the staged run was refused ({r}); nothing can be proposed"))?;
    Ok(staged.plan.refused.into_iter().map(|(p, _)| p).filter(|p| p != "*").collect())
}

/// A repository directory no checkout has: the probe's only declared output.
const PROBE_OUTPUT: &str = "adage-probe-declares-nothing/**";
