//! Execution, from the person's side (Track C §3.2, §4.4). The `adage` binary never runs a
//! route; these commands only change which binary the hooks call and what this machine approves.
//!
//! ```text
//! adage delegate [--bin <adage-exec>] [--repo <dir>]   hooks call `adage-exec hook` (E1 prefetch)
//! adage delegate --off [--repo <dir>]                        hooks call `adage hook` again
//! adage delegate --status [--repo <dir>] [--json]                registrations, and grants per repository
//! adage review delegate <track> <route> --level e1 [--raw] [--fields '<json>']
//!                         [--exit-when field=value]... [--timeout-ms n] [--repo <dir>] [--by <name>]
//! adage review undelegate <track> <route> [--repo <dir>]
//! ```
//!
//! `enable` and `disable` rewrite the hook registrations `adage install` wrote (Claude Code
//! settings, Codex `hooks.json`, the OpenCode plugin), in this repository and in the user scope,
//! swapping only the binary: `adage-exec hook <harness> <event>` takes exactly the arguments of
//! `adage hook`. For the Claude Code plugin they write or remove `$ADAGE_HOME/exec/enabled`, the
//! marker both plugins' session-start shims read: with it the Adage plugin goes quiet and the
//! `adage-exec` plugin serves; without it the reverse, so hooks never fire twice.
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

const USAGE: &str = "usage: adage delegate [--bin <adage-exec>] | --off | --status [--json]  [--repo <dir>]";
const REVIEW_USAGE: &str = "usage: adage review delegate <track> <reflex> --level e1 [--raw] [--fields '<json>'] [--exit-when field=value]... [--timeout-ms n] [--repo <dir>] [--by <name>]
       adage review undelegate <track> <reflex> [--repo <dir>]";

/// Harnesses whose registrations `enable` swaps.
const HARNESSES: [&str; 3] = ["claude-code", "codex", "opencode"];

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

pub fn exec(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("enable") => switch(true, &args[1..], paths),
        Some("disable") => switch(false, &args[1..], paths),
        Some("status") => status(&args[1..], paths),
        _ => bail!("{USAGE}"),
    }
}

fn switch(on: bool, args: &[String], paths: &Paths) -> Result<()> {
    let verb = if on { "delegate" } else { "delegate --off" };
    require_person(verb)?;
    let here = std::env::current_exe().context("cannot locate the adage binary")?;
    let bin = if on {
        let b = match flag(args, "--bin") {
            Some(b) => PathBuf::from(b),
            None => {
                here.parent().context("no directory")?.join(if cfg!(windows) { "adage-exec.exe" } else { "adage-exec" })
            }
        };
        if !b.is_file() {
            bail!("no adage-exec binary at {}; install it beside adage, or pass --bin", b.display());
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
        o.header("delegate", &["on", "hooks run delegated reflexes (E1 prefetch)"])?;
    } else {
        let _ = std::fs::remove_file(&m);
        o.header("delegate --off", &["off", "hooks call adage again"])?;
    }
    o.blank()?;
    for (h, what) in &rows {
        o.kv(h, Line::new().text(what))?;
    }
    if rows.is_empty() {
        o.kv("hooks", Line::new().quiet("no registration written by `adage install` needed changing"))?;
    }
    if on {
        o.kv(
            "plugin",
            Line::new().quiet(format!(
                "Claude Code plugin users: /plugin install adage-exec@adage; with {} present the Adage plugin goes quiet",
                m.display()
            )),
        )?;
        o.blank()?;
        o.note(&format!(
            "Recommended: deny your agent edits to {} in the harness's permissions.",
            paths.root.display()
        ))?;
    } else {
        o.kv("delegations", Line::new().quiet("kept; `adage review undelegate <track> <reflex>` withdraws one"))?;
        o.blank()?;
    }
    o.note("This takes effect in new agent sessions.")?;
    o.blank()?;
    if on {
        o.next("adage review delegate <track> <reflex> --level e1", "nothing runs until you delegate a reflex")?;
    } else {
        o.next("adage delegate --status", "registrations and delegated reflexes")?;
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
    o.header("delegate --status", &[if enabled { "on" } else { "off" }, &crate::words::count(n, "delegated reflex")])?;
    o.blank()?;
    for r in &regs {
        o.kv(
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
        o.kv("hooks", Line::new().quiet("none registered by `adage install` here"))?;
    }
    if enabled {
        o.kv("plugin", Line::new().quiet("Claude Code: the adage-exec plugin serves"))?;
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
        o.next("adage review delegate <track> <reflex> --level e1", "no reflex is delegated on this machine")?;
    } else if !enabled {
        o.next("adage delegate", "hooks run nothing delegated until it is on")?;
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

/// `adage review delegate` and `revoke-exec`.
pub fn review(verb: &str, args: &[String], paths: &Paths) -> Result<()> {
    require_person(if verb == "revoke-exec" { "review undelegate" } else { "review delegate" })?;
    // Positional arguments, wherever the flags are (`--level e1 <track> <reflex>` works too).
    let mut pos: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--level" | "--fields" | "--exit-when" | "--timeout-ms" | "--repo" | "--by" => i += 2,
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
    if verb == "revoke-exec" {
        let n = store.revoke(&ir.repo, Some(&label));
        store.save(&store_path)?;
        let mut o = Out::stdout();
        o.header("review undelegate", &[&label, &ir.repo])?;
        o.blank()?;
        o.kv("withdrawn", Line::new().text(crate::words::count(n, "delegation")))?;
        o.blank()?;
        return Ok(o.next("adage delegate --status", "what stays delegated")?);
    }
    let level = match flag(args, "--level").as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("e1") => Level::E1,
        Some("e2" | "e3") => bail!("E2 and E3 arrive with adage-exec mcp (Track C C4, C5); approve at e1"),
        _ => bail!("--level e1 is required\n{REVIEW_USAGE}"),
    };
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
    let terms = Terms { fields, exit_when, timeout_ms, raw: args.iter().any(|a| a == "--raw") };
    let table = CommandTable::load();
    let plan =
        route::plan(ir, &index, &root, &paths.tracks(), level, Some(&terms), &table).map_err(anyhow::Error::msg)?;
    let req = plan.request;
    req.output.validate()?;
    for k in plan.exit_when.keys() {
        if !req.output.fields.contains_key(k) {
            bail!("--exit-when names {k}, which is not an output field");
        }
    }
    let classified = adage_exec::classify::effect(&req.argv, &table);
    let pol = adage_exec::policy::load(&paths.root)?;
    pol.permits(&req, classified).map_err(anyhow::Error::msg)?;
    let key = ApprovalKey::for_request(&req, &ir.repo);
    let e = &ir.route.evidence;

    let mut rows: Vec<(&str, String)> = vec![
        ("reflex", format!("{label} (track version {})", ir.version)),
        ("level", format!("{}, output {}", level.as_str().to_uppercase(), req.output.mode.as_str())),
        ("argv", req.argv.join(" ")),
    ];
    for (k, v) in &req.contracts {
        rows.push(("input", format!("{{{k}}} from the prompt, must match {v}")));
    }
    rows.push(("effect", format!("observe, approved (the classifier says {})", classified.as_str())));
    rows.push(("sandbox", req.profile.summary()));
    if req.scripts.is_empty() {
        rows.push(("scripts", "none (L-none)".into()));
    }
    for s in &req.scripts {
        rows.push(("script", format!("{} (L-ref) sha256 {}; re-read and checked before every run", s.path, s.sha256)));
    }
    for (name, f) in &req.output.fields {
        rows.push(("output", format!("{name}: {}", serde_json::to_string(f)?)));
    }
    for (f, v) in &plan.exit_when {
        rows.push(("exit", format!("when {f} is {v}, nothing further is directed")));
    }
    rows.push((
        "evidence",
        format!(
            "{} of {} traces over {} days{}",
            e.support,
            e.opportunities,
            e.days,
            if e.by_origin.is_empty() { String::new() } else { format!(", by origin {:?}", e.by_origin) }
        ),
    ));
    let repo_key = key.repo.clone();
    store.approve(Approval {
        key,
        route: label.clone(),
        approved_on: adage_exec::time::now(),
        by: flag(args, "--by"),
        basis: json!({ "support": e.support, "opportunities": e.opportunities, "days": e.days, "by_origin": e.by_origin }),
        auto: false,
        terms: Some(terms),
    });
    store.save(&store_path)?;
    let mut o = Out::stdout();
    o.header("review delegate", &[&label, &format!("for {repo_key} on this machine")])?;
    o.blank()?;
    for (k, v) in &rows {
        o.kv(k, Line::new().text(v.clone()))?;
    }
    o.kv("recorded", Line::new().text(store_path.display().to_string()))?;
    o.blank()?;
    if !req.scripts.is_empty() {
        o.note("A repository script is code: Adage will run the pinned bytes, read-only, with no network.")?;
    }
    o.note("The delegation lapses if the reflex, its script or its sandbox profile changes.")?;
    o.blank()?;
    if !marker(paths).is_file() {
        o.next("adage delegate", "execution is off; this makes hooks run delegated reflexes")?;
    } else {
        o.next("adage delegate --status", "every delegated reflex")?;
    }
    Ok(())
}
