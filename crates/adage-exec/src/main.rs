//! `adage-exec`: runs approved routes in an OS sandbox (Track C §3.2).
//!
//! ```text
//! adage-exec hook    <claude|opencode|codex> <Event>   (a superset of `adage hook`: E1 prefetch, E2 actions)
//! adage-exec run     --repo <dir> --argv '<json array>' [request flags] [--dry-run] [--record]
//! adage-exec approve --repo <dir> --argv '<json array>' [request flags] [--by <name>]
//! adage-exec revoke  --repo <dir> [--track <t> --route <r>] [--mode execute|predict]
//! adage-exec status  --repo <dir>
//! adage-exec doctor  [--json]
//! adage-exec version
//! ```
//!
//! `version` prints `adage-exec <version>` on one line and exits 0: the Claude Code mod's
//! activation probe, so it reads nothing (no data directory, no policy) and cannot fail on a
//! machine whose setup is incomplete.
//!
//! Request flags: `--input k=v`, `--contract k=<regex>`, `--effect observe`, `--level e1|e2`,
//! `--output typed|raw`, `--fields '<json>'`, `--script <path>=<sha256>` (or `cas:<sha256>=<sha256>`
//! for a vendored script), `--track`, `--route`, `--step`, `--timeout-ms`, `--cwd <subdir>`,
//! `--canary <text>`; for E2, `--outputs <glob>` and `--cache <glob>` (the action is `verify` for
//! `--effect verify`, else `regenerate`). An E2 `run` stages the action, verifies its change set
//! and applies it (`--dry-run`: the gates only).
//!
//! Exit codes: 0 delivered (or, for `--dry-run`, would run); 2 usage; 3 refused or withheld.

use adage_core::{CommandTable, Effect};
use adage_exec::approvals::{self, Approval, ApprovalKey, Store};
use adage_exec::request::{
    ActionKind, ActionSpec, ExecRequest, FieldSpec, GrantMode, Labels, Level, Origin, OutputMode, OutputSpec, ScriptRef,
};
use adage_exec::run::{self, Context};
use adage_exec::sandbox::{self, Profile};
use adage_exec::{person, policy};
use anyhow::{bail, Context as _, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

const USAGE: &str = "usage:
  adage-exec hook    <claude|opencode|codex> <Event>
  adage-exec run     --repo <dir> --argv '<json array>' [--input k=v]... [--contract k=<regex>]...
                     [--effect observe] [--level e1|e2] [--output typed|raw] [--fields '<json>']
                     [--outputs <glob>]... [--cache <glob>]...
                     [--script <path>=<sha256>]... [--track t --route r --step s]
                     [--timeout-ms n] [--cwd <subdir>] [--canary <text>]... [--dry-run] [--record]
  adage-exec approve <the same request flags> [--by <name>]
  adage-exec revoke  --repo <dir> [--track <t> --route <r>] [--mode execute|predict]
  adage-exec status  --repo <dir>
  adage-exec journal [--repo <dir>]
  adage-exec recover --id <operation-id> --acknowledge
  adage-exec doctor  [--json]
  adage-exec version";

struct Args {
    list: Vec<String>,
}

impl Args {
    fn one(&self, name: &str) -> Option<String> {
        self.list.iter().position(|a| a == name).and_then(|i| self.list.get(i + 1).cloned())
    }
    fn all(&self, name: &str) -> Vec<String> {
        self.list.windows(2).filter(|w| w[0] == name).map(|w| w[1].clone()).collect()
    }
    fn has(&self, name: &str) -> bool {
        self.list.iter().any(|a| a == name)
    }
    /// Refuse flags this command does not know (a typo must not silently drop a restriction).
    fn only(&self, valued: &[&str], switches: &[&str]) -> Result<()> {
        let mut i = 0;
        while i < self.list.len() {
            let a = &self.list[i];
            if valued.contains(&a.as_str()) {
                if i + 1 >= self.list.len() {
                    bail!("{a} needs a value");
                }
                i += 2;
            } else if switches.contains(&a.as_str()) {
                i += 1;
            } else {
                bail!("unknown argument {a:?}");
            }
        }
        Ok(())
    }
}

const REQUEST_FLAGS: &[&str] = &[
    "--repo",
    "--argv",
    "--input",
    "--contract",
    "--effect",
    "--level",
    "--output",
    "--fields",
    "--script",
    "--track",
    "--route",
    "--step",
    "--timeout-ms",
    "--cwd",
    "--canary",
    "--outputs",
    "--cache",
];

fn kv(s: &str, what: &str) -> Result<(String, String)> {
    let (k, v) = s.split_once('=').with_context(|| format!("{what} {s:?} is not name=value"))?;
    Ok((k.to_string(), v.to_string()))
}

fn request(a: &Args) -> Result<ExecRequest> {
    let repo = PathBuf::from(a.one("--repo").context("--repo is required")?);
    let argv: Vec<String> = serde_json::from_str(&a.one("--argv").context("--argv is required")?)
        .context("--argv must be a JSON array of strings")?;
    let level = match a.one("--level") {
        Some(l) => Level::parse(&l).with_context(|| format!("--level {l:?} is not e1 or e2"))?,
        None => Level::E1,
    };
    let effect = match a.one("--effect") {
        Some(e) => Effect::parse(&e).with_context(|| format!("--effect {e:?} is not an effect"))?,
        None => Effect::Observe,
    };
    let mode = match a.one("--output") {
        Some(m) => OutputMode::parse(&m).with_context(|| format!("--output {m:?} is not typed or raw"))?,
        None => OutputMode::Typed,
    };
    let fields: BTreeMap<String, FieldSpec> = match a.one("--fields") {
        Some(f) => serde_json::from_str(&f).context("--fields must be a JSON object of field specs")?,
        None => BTreeMap::new(),
    };
    let mut profile = Profile::for_level(level);
    if let Some(t) = a.one("--timeout-ms") {
        let ms: u64 = t.parse().context("--timeout-ms must be a number")?;
        profile.limits = profile.limits.with_timeout(level, ms);
    }
    profile.cwd = a.one("--cwd");
    let mut labels = Labels::default();
    if let Some(t) = a.one("--track") {
        labels.track = t;
    }
    if let Some(r) = a.one("--route") {
        labels.route = r;
    }
    if let Some(s) = a.one("--step") {
        labels.step = s;
    }
    Ok(ExecRequest {
        repo_root: repo,
        project_python: None,
        argv,
        inputs: a.all("--input").iter().map(|s| kv(s, "--input")).collect::<Result<_>>()?,
        contracts: a.all("--contract").iter().map(|s| kv(s, "--contract")).collect::<Result<_>>()?,
        effect,
        level,
        output: OutputSpec { mode, fields },
        scripts: a
            .all("--script")
            .iter()
            .map(|s| kv(s, "--script").map(|(path, sha256)| ScriptRef { path, sha256 }))
            .collect::<Result<_>>()?,
        profile,
        labels,
        origin: Origin::Local,
        route_exec_digest: None,
        action: (level == Level::E2).then(|| ActionSpec {
            kind: if effect == Effect::Verify { ActionKind::Verify } else { ActionKind::Regenerate },
            outputs: a.all("--outputs"),
            cache: a.all("--cache"),
        }),
        mode: Default::default(),
    })
}

fn adage_home() -> PathBuf {
    adage_store::Paths::from_env().root
}

fn repo_key(req: &ExecRequest) -> Result<String> {
    let root = req.repo_root.canonicalize().with_context(|| format!("repository {}", req.repo_root.display()))?;
    Ok(adage_core::repo::repo_key(&root))
}

fn cmd_run(a: &Args) -> Result<i32> {
    a.only(REQUEST_FLAGS, &["--dry-run", "--record"])?;
    let req = request(a)?;
    let home = adage_home();
    let pol = policy::load(&home)?;
    let store = Store::load(&approvals::path(&home))?;
    let cap = sandbox::detect();
    let table = CommandTable::load();
    let ctx = Context {
        inherited_env: std::env::vars().collect(),
        policy: &pol,
        store: &store,
        capability: &cap,
        table: &table,
        repo_key: repo_key(&req)?,
        cas: Some(home.join("cas")).filter(|p| p.is_dir()),
        home: adage_store::home_dir(),
        canaries: a.all("--canary"),
    };
    if a.has("--dry-run") {
        let (ok, value) = match run::check(&req, &ctx) {
            Ok(c) => (true, serde_json::json!({ "would_run": true, "plan": c })),
            Err((r, c)) => (false, serde_json::json!({ "would_run": false, "refusal": r, "plan": c })),
        };
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(if ok { 0 } else { 3 });
    }
    if req.level == Level::E2 {
        if let Some(receipt) = adage_exec::journal::pending(&home, &req.repo_root)? {
            println!("{}", serde_json::to_string_pretty(&receipt)?);
            return Ok(3);
        }
        let staged = match adage_exec::stage::stage(&req, &ctx) {
            Ok(s) => s,
            Err(r) => {
                eprintln!("adage-exec: not staged: {r}");
                return Ok(3);
            }
        };
        let approved_on = store.find(&ctx_key(&req, &ctx)).map(|a| a.approved_on.clone()).unwrap_or_default();
        // A person's explicit CLI invocation is a new occurrence, not a hook retry.
        let occurrence = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()
        );
        let completed = adage_exec::journal::finish(
            &staged,
            &adage_exec::journal::Commit {
                home: &home,
                repo: &ctx.repo_key,
                session: "adage-exec run",
                occurrence: &occurrence,
                approved_on: &approved_on,
                request: &req,
                origin: adage_exec::journal::RunOrigin::Manual,
            },
        );
        let outcome = completed.outcome;
        let revoked = completed.revoked;
        let kind = req.action.as_ref().map(|a| a.kind).unwrap_or(ActionKind::Regenerate);
        let mut v = adage_exec::stage::result_json(&req.labels.route, kind, Some(&staged), &outcome, revoked);
        v["operation_id"] = serde_json::json!(completed.id);
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(if outcome.name() == "applied" { 0 } else { 3 });
    }
    match run::execute(&req, &ctx) {
        Ok(d) => {
            println!("{}", d.block);
            if a.has("--record") {
                eprintln!("{}", serde_json::to_string(&serde_json::json!({ "exec": d.record }))?);
            }
            Ok(0)
        }
        Err(r) => {
            eprintln!("adage-exec: not delivered: {r}");
            Ok(3)
        }
    }
}

fn ctx_key(req: &ExecRequest, ctx: &Context) -> ApprovalKey {
    ApprovalKey::for_request(req, &ctx.repo_key)
}

fn cmd_approve(a: &Args) -> Result<i32> {
    a.only(&[REQUEST_FLAGS, &["--by"]].concat(), &[])?;
    person::require_person("approve")?;
    let req = request(a)?;
    let bound = req.bind()?;
    req.output.validate()?;
    let table = CommandTable::load();
    let classified = adage_exec::classify::effect(&bound, &table);
    if classified > req.effect && req.scripts.is_empty() {
        bail!("declared effect {} is lower than the classified {}", req.effect.as_str(), classified.as_str());
    }
    let home = adage_home();
    let pol = policy::load(&home)?;
    pol.permits(&req, classified).map_err(anyhow::Error::msg)?;
    let key = ApprovalKey::for_request(&req, &repo_key(&req)?);
    let path = approvals::path(&home);
    let mut store = Store::load(&path)?;
    println!("Approving on this machine, for repository {}:", key.repo);
    println!("  route:    {}", approvals::route_label(&req));
    println!("  argv:     {}", req.argv.join(" "));
    for (k, v) in &req.contracts {
        println!("  {{{k}}}:  must match {v}");
    }
    println!("  effect:   {} (classified {})", req.effect.as_str(), classified.as_str());
    println!("  level:    {}   output: {}", req.level.as_str().to_uppercase(), req.output.mode.as_str());
    println!("  sandbox:  {}", req.profile.summary());
    for s in &req.scripts {
        println!("  script:   {} sha256 {}", s.path, s.sha256);
    }
    if let Some(x) = &req.action {
        println!("  action:   {} · outputs {} · cache {}", x.kind.as_str(), x.outputs.join(", "), x.cache.join(", "));
    }
    store.approve(Approval {
        key,
        route: approvals::route_label(&req),
        approved_on: adage_exec::time::now(),
        by: a.one("--by"),
        basis: serde_json::Value::Null,
        auto: false,
        terms: None,
        // Only a person at a terminal gets here: adage-exec's verbs never take a press.
        via: Some(adage_tracks::review::Via::Terminal),
    });
    store.save(&path)?;
    println!("Recorded in {}.", path.display());
    Ok(0)
}

fn cmd_revoke(a: &Args) -> Result<i32> {
    a.only(&["--repo", "--track", "--route", "--mode"], &[])?;
    // One mode at a time (execution by default): a prediction grant is its own decision, revoked
    // only when named, as `adage review revoke --predict` does.
    let mode = match a.one("--mode").as_deref() {
        None | Some("execute") => GrantMode::Execute,
        Some("predict") => GrantMode::Predict,
        Some(m) => bail!("--mode is execute or predict, not {m:?}"),
    };
    person::require_person("revoke")?;
    let repo = PathBuf::from(a.one("--repo").context("--repo is required")?);
    let key = adage_core::repo::repo_key(&repo.canonicalize()?);
    let route = match (a.one("--track"), a.one("--route")) {
        (Some(t), Some(r)) => Some(format!("{t}/{r}")),
        (None, None) => None,
        _ => bail!("--track and --route go together"),
    };
    let path = approvals::path(&adage_home());
    let mut store = Store::load(&path)?;
    let n = store.revoke(&key, route.as_deref(), Some(mode));
    store.save(&path)?;
    match mode {
        GrantMode::Execute => println!("Revoked {n} approval(s) for {key}."),
        GrantMode::Predict => println!("Revoked {n} prediction grant(s) for {key}."),
    }
    Ok(0)
}

fn cmd_status(a: &Args) -> Result<i32> {
    a.only(&["--repo"], &[])?;
    let repo = PathBuf::from(a.one("--repo").context("--repo is required")?);
    let key = adage_core::repo::repo_key(&repo.canonicalize()?);
    let store = Store::load(&approvals::path(&adage_home()))?;
    let line = |x: &Approval| {
        println!(
            "  {}  {} {}  {}  (approved {}{})",
            x.route,
            x.key.level.as_str().to_uppercase(),
            x.key.output_mode.as_str(),
            x.key.argv_templates.iter().map(|v| v.join(" ")).collect::<Vec<_>>().join(" ; "),
            x.approved_on,
            if x.auto { ", automatic" } else { "" }
        )
    };
    let key = key.as_str();
    let of = |m: GrantMode| store.approvals.iter().filter(move |x| x.key.repo == key && x.key.mode == m);
    println!("Execution grants for {key}: {}", of(GrantMode::Execute).count());
    of(GrantMode::Execute).for_each(line);
    // A prediction grant runs nothing itself: it lets the step be taken in the model's place.
    let predict = of(GrantMode::Predict).count();
    if predict > 0 {
        println!("Prediction grants for {key}: {predict}");
        of(GrantMode::Predict).for_each(line);
    }
    Ok(0)
}

fn cmd_doctor(a: &Args) -> Result<i32> {
    a.only(&[], &["--json"])?;
    let cap = sandbox::detect();
    let home = adage_home();
    let pol = policy::load(&home);
    if a.has("--json") {
        let v = serde_json::json!({
            "sandbox": cap,
            "summary": cap.line(),
            "stage": adage_exec::stage::backend(&cap),
            "policy": pol.as_ref().ok(),
            "policy_error": pol.as_ref().err().map(|e| e.to_string()),
            "last_withheld": adage_exec::last_withheld(&home),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(0);
    }
    println!("Sandbox: {}", cap.line());
    println!("Staging (E2): {}", adage_exec::stage::backend(&cap).line());
    println!(
        "  Landlock:           {}",
        cap.landlock_abi.map(|a| format!("ABI {a}")).unwrap_or_else(|| "not available".into())
    );
    println!("  user namespaces:    {}", if cap.user_namespaces { "available" } else { "not available" });
    println!(
        "  bubblewrap:         {}",
        cap.bwrap.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "not installed".into())
    );
    println!(
        "  sandbox-exec:       {}",
        cap.sandbox_exec.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "not present".into())
    );
    if cap.running_as_root {
        println!(
            "  running as root:    sandboxed commands run as nobody (65534); a repository nobody can reach is \
             bind-mounted read-only where it can (E1)"
        );
    }
    match pol {
        Ok(p) => println!(
            "Policy: max level {}, automatic approval {}, raw output {}, sources: {}",
            p.max_level.as_str().to_uppercase(),
            if p.auto_approve { "inert-observe" } else { "off" },
            if p.allow_raw_output { "allowed" } else { "not allowed" },
            if p.sources.is_empty() { "defaults".to_string() } else { p.sources.join(", ") }
        ),
        Err(e) => println!("Policy: unreadable ({e:#}); execution refuses until it is fixed"),
    }
    match adage_exec::last_withheld(&home) {
        Some(w) => println!("Last withheld: {}", w.line()),
        None => println!("Last withheld: none recorded"),
    }
    Ok(0)
}

fn cmd_journal(a: &Args) -> Result<i32> {
    a.only(&["--repo"], &[])?;
    let repo = a.one("--repo").map(std::fs::canonicalize).transpose()?;
    let repo = repo.as_ref().map(|p| adage_core::repo::repo_key(p));
    let rows: Vec<_> = adage_exec::journal::list(&adage_home())?
        .into_iter()
        .filter(|r| repo.as_ref().is_none_or(|k| &r.repo == k))
        .collect();
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(0)
}

fn cmd_recover(a: &Args) -> Result<i32> {
    a.only(&["--id"], &["--acknowledge"])?;
    person::require_person("recover")?;
    if !a.has("--acknowledge") {
        bail!("inspect the worktree first, then use --acknowledge; recovery does not retry or roll back the action");
    }
    let id = a.one("--id").context("--id is required")?;
    let receipt = adage_exec::journal::acknowledge(&adage_home(), &id)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(0)
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("hook") {
        // Before anything else: the no-match path must cost what `adage hook`'s does.
        adage_exec::hook::main(args.get(1).cloned().unwrap_or_default(), args.get(2).cloned().unwrap_or_default());
    }
    if args.first().map(String::as_str) == Some("version") {
        println!("adage-exec {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        std::process::exit(if args.is_empty() { 2 } else { 0 });
    }
    let cmd = args.remove(0);
    let a = Args { list: args };
    let r = match cmd.as_str() {
        "run" => cmd_run(&a),
        "approve" => cmd_approve(&a),
        "revoke" => cmd_revoke(&a),
        "status" => cmd_status(&a),
        "doctor" => cmd_doctor(&a),
        "journal" => cmd_journal(&a),
        "recover" => cmd_recover(&a),
        other => {
            eprintln!("adage-exec: unknown command {other:?}\n{USAGE}");
            std::process::exit(2);
        }
    };
    match r {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("adage-exec: {e:#}");
            std::process::exit(2);
        }
    }
}
