//! Delegation: letting approved reflexes act, from the person's side (Track C §3.2, §4.4). The
//! `adage` binary never runs a reflex; these commands only change which binary the hooks call and
//! what this machine approves.
//!
//! ```text
//! adage delegate on [--bin <adage-exec>] [--repo <dir>]   hooks call `adage-exec hook` (E1 prefetch,
//!                                                          E2 actions)
//! adage delegate off [--repo <dir>]                        hooks call `adage hook` again
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
use adage_tracks::Index;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: adage delegate <on [--bin <adage-exec>] | off | status [--json]> [--repo <dir>]";
const REVIEW_USAGE: &str = "usage: adage review delegate <track> <reflex> --level e1 [--raw] [--fields '<json>'] [--exit-when field=value]... [--timeout-ms n] [--repo <dir>] [--by <name>]
       adage review delegate <track> <reflex> --level e2 [--outputs <glob>]... [--cache <glob>]... [--effect verify|regenerate] [--timeout-ms n] [--repo <dir>] [--by <name>]
       adage review revoke <track> <reflex> [--repo <dir>]";

/// Harnesses whose registrations `delegate on` swaps.
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

/// `adage delegate <on|off|status>`; bare `adage delegate` (or only flags) is `status`.
pub fn delegate(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("on") => switch(true, &args[1..], paths),
        Some("off") => switch(false, &args[1..], paths),
        Some("status") => status(&args[1..], paths),
        None => status(args, paths),
        Some(a) if a.starts_with("--") => status(args, paths),
        _ => bail!("{USAGE}"),
    }
}

fn switch(on: bool, args: &[String], paths: &Paths) -> Result<()> {
    let verb = if on { "delegate on" } else { "delegate off" };
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
    let mut changed = 0;
    for h in HARNESSES {
        for scope in scopes(root.as_deref()) {
            let Some((target, _)) = registered(h, scope, root.as_deref()) else { continue };
            let p = install::plan(h, scope, root.as_deref(), Some(bin.clone()))?;
            if p.before.as_deref() != Some(p.after.as_str()) {
                install::apply(&p)?;
                changed += 1;
                println!("{h} ({scope}): {} now runs {}", target.display(), bin.display());
            }
        }
    }
    let m = marker(paths);
    if on {
        std::fs::create_dir_all(m.parent().context("no directory")?)?;
        let body = json!({ "enabled_at": adage_exec::time::now(), "bin": bin.to_string_lossy() });
        std::fs::write(&m, serde_json::to_string_pretty(&body)? + "\n")?;
        println!("Execution: on (E1 prefetch and E2 actions, for approved reflexes). Claude Code plugin users: enable");
        println!(
            "the adage-exec plugin (/plugin install adage-exec@adage); with {} present the Adage plugin goes quiet.",
            m.display()
        );
        println!("Nothing runs until you approve a reflex: adage review delegate <track> <reflex> --level e1|e2");
        println!("Recommended: deny your agent edits to {} in the harness's permissions.", paths.root.display());
    } else {
        let _ = std::fs::remove_file(&m);
        println!("Execution: off. Hooks call adage again (approvals are kept; revoke them with review revoke).");
    }
    // Rewrites to the project's environment (docs/execution.md): on by default, off with
    // `delegate off`, back on with `delegate on`.
    let off = adage_hook::rewrite_off(paths);
    if on {
        let _ = std::fs::remove_file(&off);
    } else {
        std::fs::create_dir_all(off.parent().context("no directory")?)?;
        std::fs::write(&off, "")?;
        println!("Rewrites to the project's environment: off (Python test runs keep the interpreter the agent chose).");
    }
    if changed == 0 {
        println!("No hook registration written by `adage install` needed changing.");
    }
    println!("This takes effect in new agent sessions.");
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
    println!("Execution: {}", if enabled { "on (Claude Code plugin: adage-exec serves)" } else { "off" });
    for r in &regs {
        println!(
            "  {} ({}): {} runs {}",
            r["harness"].as_str().unwrap_or(""),
            r["scope"].as_str().unwrap_or(""),
            r["file"].as_str().unwrap_or(""),
            r["runs"].as_str().unwrap_or("")
        );
    }
    if grants.is_empty() {
        println!("No execution grants on this machine.");
    }
    for (repo, gs) in &grants {
        println!("Grants for {repo}:");
        for g in gs {
            println!(
                "  {}  {} {}  {}{}",
                g["route"].as_str().unwrap_or(""),
                g["level"].as_str().unwrap_or("").to_uppercase(),
                g["output"].as_str().unwrap_or(""),
                g["argv"].as_str().unwrap_or(""),
                if g["auto"] == true { "  (automatic)" } else { "" }
            );
        }
    }
    println!("The sandbox this machine can enforce: adage-exec doctor");
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
        .with_context(|| format!("no routes are installed for {}", root.display()))?;
    route::find(index, &key, track, route_id).with_context(|| format!("no route {route_id} in track {track} for {key}"))
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
fn print_vendored(paths: &Paths, hex: &str) -> Result<()> {
    let p = crate::packs::cas_dir(paths).join(hex);
    let bytes =
        std::fs::read(&p).with_context(|| format!("vendored script {} is not in {}", &hex[..12], p.display()))?;
    if adage_pack::digest::sha256_hex(&bytes) != hex {
        bail!("vendored script {} in the CAS does not match its hash; run `adage add` again", &hex[..12]);
    }
    let text = String::from_utf8_lossy(&bytes);
    println!(
        "  script:   vendored (L-vendor) sha256 {hex}, {} bytes, run from {} by the interpreter in argv (its shebang is ignored); never approved automatically. Its full text:",
        bytes.len(),
        p.display()
    );
    for line in text.lines() {
        let clean: String = line.chars().map(|c| if c.is_control() && c != '\t' { '?' } else { c }).collect();
        println!("    | {clean}");
    }
    Ok(())
}

/// A pack's attestations, marked self-reported by its signer (Track C §1.8). They grant nothing.
pub(crate) fn print_attestations(paths: &Paths, ir: &adage_tracks::index::IndexedRoute) {
    if ir.exec.is_none() {
        return;
    }
    for line in crate::packs::attestation_lines(paths, &ir.track) {
        println!("  attested: {line}");
    }
}

/// Stage the action once, apply nothing, and show the full diff it would make now. Refuses (and
/// approves nothing) when the staged run fails or changes anything the approval would not cover.
#[allow(clippy::too_many_arguments)]
fn preview(
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
    println!("Staging the action once to show what it would change now (nothing is applied)…");
    let staged = adage_exec::stage::stage(req, &ctx).map_err(|r| {
        let hint =
            if r.stage == "policy" { " (E2 is a beta: set max_level = \"E2\" in policy.toml to opt in)" } else { "" };
        anyhow::anyhow!("the staged run was refused ({r}){hint}; nothing was approved")
    })?;
    println!(
        "  staged:   {} · exit {}",
        staged.backend.line(),
        staged.exit.map(|e| e.to_string()).unwrap_or_else(|| "none".into())
    );
    if staged.exit != Some(0) {
        let err = String::from_utf8_lossy(&staged.stderr);
        for line in err.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev() {
            let clean: String = line.chars().filter(|c| !c.is_control() || *c == '\t').collect();
            println!("    ! {clean}");
        }
        bail!("the staged run did not succeed; nothing was approved. Fix what it needs, then approve again");
    }
    if !staged.plan.refused.is_empty() {
        for (p, why) in &staged.plan.refused {
            println!("  refused:  {p} ({why})");
        }
        bail!("the staged run changed paths the approval would not cover; nothing was approved. Declare its outputs (--outputs), or leave it at E0");
    }
    if staged.plan.apply.is_empty() {
        println!("  diff:     none: the declared outputs are up to date now");
    }
    for c in &staged.plan.apply {
        let before = std::fs::read(root.join(&c.path)).unwrap_or_default();
        let after = match &c.after {
            adage_exec::stage::After::File { bytes, .. } => bytes.clone(),
            _ => Vec::new(),
        };
        println!("  diff:     {} ({:?})", c.path, c.kind);
        match (std::str::from_utf8(&before), std::str::from_utf8(&after)) {
            (Ok(a), Ok(b)) => {
                for line in crate::install::line_diff(a, b).lines() {
                    let clean: String =
                        line.chars().map(|c| if c.is_control() && c != '\t' { '?' } else { c }).collect();
                    println!("    {clean}");
                }
            }
            _ => println!("    (binary: {} bytes before, {} after)", before.len(), after.len()),
        }
    }
    if !staged.plan.discarded.is_empty() {
        println!("  discarded: {} cache path(s)", staged.plan.discarded.len());
    }
    Ok(())
}

/// `adage review delegate` and `revoke`.
pub fn review(verb: &str, args: &[String], paths: &Paths) -> Result<()> {
    require_person(&format!("review {verb}"))?;
    let (Some(track), Some(route_id)) = (args.first(), args.get(1)) else { bail!("{REVIEW_USAGE}") };
    if track.starts_with("--") || route_id.starts_with("--") {
        bail!("{REVIEW_USAGE}");
    }
    let root = repo_root(args)?.context("not inside a repository; pass --repo")?;
    let index = Index::load(&paths.index());
    let ir = indexed(&index, &root, paths, track, route_id)?;
    let store_path = approvals::path(&paths.root);
    let mut store = Store::load(&store_path)?;
    let label = format!("{track}/{route_id}");
    if verb == "revoke" {
        let n = store.revoke(&ir.repo, Some(&label));
        store.save(&store_path)?;
        println!("Revoked {n} execution grant(s) for {label} in {}.", ir.repo);
        return Ok(());
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
        let hint = if level == Level::E2 && e.contains("caps execution") {
            " (E2 actions are a beta behind gate G4: set max_level = \"E2\" under [exec] in policy.toml to opt in)"
        } else if e.contains("allow_vendor_scripts") {
            " (set allow_vendor_scripts = true under [exec] in policy.toml to allow vendored scripts on this machine)"
        } else {
            ""
        };
        anyhow::anyhow!("{e}{hint}")
    })?;
    let key = ApprovalKey::for_request(&req, &ir.repo);
    let e = &ir.route.evidence;

    println!("Approving on this machine, for repository {}:", key.repo);
    println!("  route:    {label} (track version {})", ir.version);
    println!("  level:    {}   output: {}", level.as_str().to_uppercase(), req.output.mode.as_str());
    println!("  argv:     {}", req.argv.join(" "));
    for (k, v) in &req.contracts {
        if level == Level::E1 {
            println!("  input:    {{{k}}} from the prompt, must match {v}");
        } else if plan.repo_paths.contains(k) {
            println!(
                "  input:    {{{k}}} bound from the edited path: a repository path that exists, checked at every run"
            );
        } else {
            println!("  input:    {{{k}}} bound from the edited path, must match {v} (and be a plain token)");
        }
    }
    if level == Level::E2 {
        println!("  effect:   {}, approved (the classifier says {})", req.effect.as_str(), classified.as_str());
    } else {
        println!("  effect:   observe, approved (the classifier says {})", classified.as_str());
    }
    // A script the command table does not know, learned observe from what the traces show it
    // doing: the person approving sees why (it is never approved automatically: it runs a script).
    if let Some(note) = ir.route.evidence.effect_evidence.as_deref().filter(|n| !n.trim().is_empty()) {
        println!("  inferred: {note}");
    }
    println!("  sandbox:  {}", req.profile.summary());
    if let (Level::E2, Some(a)) = (level, &req.action) {
        println!(
            "  runs:     from the hook, when {}; at most one action per hook event, within {} s",
            stimulus_line(&ir.route),
            adage_exec::limits::E2_MAX_WALL_MS / 1000
        );
        println!("  action:   {}; staged in a writable overlay, verified, then applied file by file", a.kind.as_str());
        match a.kind {
            adage_exec::request::ActionKind::Regenerate => {
                println!(
                    "  outputs:  {} (only these may change; anything else refuses the application)",
                    a.outputs.join(", ")
                )
            }
            adage_exec::request::ActionKind::Verify => {
                println!("  outputs:  none (a verify action may not change sources)")
            }
        }
        if !a.cache.is_empty() {
            println!("  cache:    {}", a.cache.join(", "));
        }
        println!("  revoked:  automatically after {} refused change sets", adage_exec::stage::REVOKE_AFTER);
    }
    let hazards = adage_exec::git::hazards(&root);
    let actx = adage_exec::approvals::AutoContext {
        table: &table,
        git_hazards: &hazards,
        sandbox_enforced: adage_exec::sandbox::detect().available(),
        policy_auto: pol.auto_approve,
    };
    match adage_exec::approvals::auto_eligible(&req, &actx) {
        Ok(()) => println!("  automatic: in the inert-observe class, and policy approves that class automatically"),
        Err(why) => println!("  automatic: no ({})", why.join("; ")),
    }
    if req.scripts.is_empty() {
        println!("  scripts:  none (L-none)");
    }
    for s in &req.scripts {
        match s.path.strip_prefix("cas:") {
            Some(hex) => print_vendored(paths, hex)?,
            None => {
                println!("  script:   {} (L-ref) sha256 {}; re-read and checked before every run", s.path, s.sha256)
            }
        }
    }
    for (name, f) in &req.output.fields {
        println!("  output:   {name}: {}", serde_json::to_string(f)?);
    }
    for (f, v) in &plan.exit_when {
        println!("  exit:     when {f} is {v}, nothing further is directed");
    }
    println!(
        "  evidence: {} of {} traces over {} days{}",
        e.support,
        e.opportunities,
        e.days,
        if e.by_origin.is_empty() { String::new() } else { format!(", by origin {:?}", e.by_origin) }
    );
    print_attestations(paths, ir);
    if req.scripts.iter().any(|s| s.path.starts_with("cas:")) {
        println!("A vendored script is code from the pack's signer: Adage will run exactly these bytes, read-only, with no network.");
    } else if !req.scripts.is_empty() {
        println!("A repository script is code: Adage will run the pinned bytes, read-only, with no network.");
    }
    if level == Level::E2 {
        preview(paths, &root, ir, &req, &label, &store, &pol, &table)?;
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
    println!(
        "Recorded in {}. It lapses if the route, its script or its sandbox profile changes.",
        store_path.display()
    );
    if !marker(paths).is_file() {
        println!("Execution is off: `adage delegate on` makes hooks run it.");
    }
    Ok(())
}
