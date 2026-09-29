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
//! adage delegate [status] [--repo <dir>] [--json]          registrations, and grants per repository
//! adage review delegate <track> <reflex> --level e1 [--raw] [--fields '<json>']
//!                         [--exit-when field=value]... [--timeout-ms n] [--repo <dir>] [--by <name>]
//! adage review delegate <track> <reflex> --level e2 [--outputs <glob>]... [--cache <glob>]...
//!                         [--effect verify|regenerate] [--timeout-ms n] [--repo <dir>] [--by <name>]
//! adage review revoke <track> <reflex> [--repo <dir>]
//! ```
//!
//! **E2** (Track C §3.6, §4): an action a hook runs when the reflex's stimulus fires. It is
//! approved explicitly, for one reflex in one repository, never automatically. Before recording
//! anything, `delegate --level e2` stages the action once in the sandbox (nothing is applied)
//! and shows the full diff it would make to its declared outputs now; a staged run that changes
//! anything else, or fails, approves nothing. Policy must allow E2 (`max_level = "E2"`: actions
//! are a beta behind gate G4).
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
//! marker both plugins' session-start shims read: with it the Adage plugin goes quiet and the
//! `adage-exec` plugin serves; without it the reverse, so hooks never fire twice. A registration
//! that runs `adage-exec` gives the events an action runs at (the edit and Stop events) a 30 s
//! timeout; an action's own wall limit is 20 s.
//!
//! Every command here that changes anything refuses inside an agent session and needs a terminal.

use crate::install;
use crate::share::require_person;
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

const USAGE: &str =
    "usage: adage delegate <on [--bin <adage-exec>] | off | rewrites <on|off> | status [--json]> [--repo <dir>]";
const REVIEW_USAGE: &str = "usage: adage review delegate <track> <reflex> --level e1 [--raw] [--fields '<json>'] [--exit-when field=value]... [--timeout-ms n] [--repo <dir>] [--by <name>]
       adage review delegate <track> <reflex> --level e2 [--outputs <glob>]... [--cache <glob>]... [--effect verify|regenerate] [--timeout-ms n] [--repo <dir>] [--by <name>]
       adage review revoke <track> <reflex> [--repo <dir>]";

/// Harnesses whose registrations `delegate on` swaps.
const HARNESSES: [&str; 3] = ["claude-code", "codex", "opencode"];

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
        Some("status") => status(&args[1..], paths),
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

fn switch(on: bool, args: &[String], paths: &Paths) -> Result<()> {
    // Turning execution on is a person's decision; turning it off only turns something down, so
    // it also runs from a script or from `!` in an agent session (fresh-install B6).
    if on {
        require_person("delegate on")?;
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
                "Claude Code plugin users: /plugin install adage-exec@adage; with {} present the Adage plugin goes quiet",
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
            "level": a.key.level.as_str(),
            "output": a.key.output_mode.as_str(),
            "argv": a.key.argv_templates.first().map(|v| v.join(" ")),
            "scripts": a.key.scripts,
            "approved_on": a.approved_on,
            "auto": a.auto,
        }));
    }
    let enabled = marker(paths).is_file();
    if args.iter().any(|a| a == "--json") {
        let v = json!({ "enabled": enabled, "registrations": regs, "grants": grants });
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
    for (repo, gs) in &grants {
        o.blank()?;
        o.line(&Line::new().bold(Role::Ink, repo.clone()))?;
        for g in gs {
            let mut l = Line::new()
                .text("  ")
                .text(g["route"].as_str().unwrap_or(""))
                .sep()
                .text(g["level"].as_str().unwrap_or("").to_uppercase())
                .sep()
                .quiet(g["output"].as_str().unwrap_or(""))
                .sep()
                .cmd(g["argv"].as_str().unwrap_or(""));
            if g["auto"] == true {
                l = l.sep().faint("automatic");
            }
            o.line(&l)?;
        }
    }
    o.blank()?;
    if grants.is_empty() {
        o.next("adage review delegate <track> <reflex> --level e1|e2", "no reflex is delegated on this machine")?;
    } else if !enabled {
        o.next("adage delegate on", "hooks run nothing delegated until it is on")?;
    } else {
        o.next("adage-exec doctor", "the sandbox this machine can enforce")?;
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
    let mut trial = store.clone();
    trial.approve(Approval {
        key: ApprovalKey::for_request(req, &ir.repo),
        route: label.to_string(),
        approved_on: adage_exec::time::now(),
        by: None,
        basis: Value::Null,
        auto: false,
        terms: None,
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
    o.blank()?;
    o.section("Staged once, to show what it would change now (nothing is applied)")?;
    o.flush()?;
    let staged = adage_exec::stage::stage(req, &ctx).map_err(|r| {
        let help = if r.stage == "policy" {
            "E2 is a beta: set max_level = \"E2\" under [exec] in policy.toml to opt in"
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
    require_person(&format!("review {verb}"))?;
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
    if verb == "revoke" {
        let n = store.revoke(&ir.repo, Some(&label));
        store.save(&store_path)?;
        let mut o = Out::stdout();
        o.header("review revoke", &[&label, &ir.repo])?;
        o.blank()?;
        o.kv("withdrawn", Line::new().text(crate::words::count(n, "delegation")))?;
        o.blank()?;
        return Ok(o.next("adage delegate status", "what stays delegated")?);
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
    let classified = adage_exec::classify::effect(&req.argv, &table);
    let pol = adage_exec::policy::load(&paths.root)?;
    pol.permits(&req, classified).map_err(|e| {
        if level == Level::E2 && e.contains("caps execution") {
            crate::hint(
                e,
                "E2 actions are a beta behind gate G4: set max_level = \"E2\" under [exec] in policy.toml to opt in",
            )
        } else if e.contains("allow_vendor_scripts") {
            crate::hint(
                e,
                "set allow_vendor_scripts = true under [exec] in policy.toml to allow vendored scripts on this machine",
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
