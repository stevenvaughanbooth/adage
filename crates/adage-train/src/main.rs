//! adage-train: grow reflexes from a skill (docs/training.md). People reach it as `adage train`,
//! which executes this binary with the same arguments (the `adage` binary never starts agent
//! sessions itself).
//!
//! adage train plan --from <SKILL.md|CONTRIBUTING.md|Makefile|package.json|workflow.yml|runbook.md>
//!                  [--repo <path>] [--seed N] [--sessions N] [--model ID] [--json] [--out plan.json]
//! adage train --plan plan.json [--harness claude-code|fake] [--fake-world world.json]
//!                 [--model ID] [--budget USD] [--max-turns N] [--concurrency N] [--validate N]
//!                 [--oracle trajectory] [--container none] [--run-id ID] [--adage PATH]
//!                 [--thresholds default|demo] [--timeout-secs N] [--keep] [--json] [--execute]
//! adage train report <run id|run dir> [--json]

use adage_train::fake::{Fake, FakeWorld};
use adage_train::harness::{ClaudeCode, Harness};
use adage_train::{isolate, plan, report, run};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const USAGE: &str = "usage:
  adage train plan --from <file> [--repo <path>] [--seed N] [--sessions N] [--model ID] [--json] [--out plan.json]
  adage train --plan plan.json [--harness claude-code|fake] [--model ID] [--budget USD] [--max-turns N]
                  [--concurrency N] [--validate N] [--oracle trajectory] [--container none] [--run-id ID]
                  [--adage PATH] [--thresholds default|demo] [--timeout-secs N] [--keep] [--json] [--execute]
                  [--forward-env NAME,NAME..] [--host-auth]
  adage train report <run id | run dir> [--json]

`plan` is free: it reads files and executes nothing. `--plan` without --execute is a dry run: it prints
every session, the exact commands and the isolation, and exits. With --execute it starts real agent
sessions that spend money, and refuses unless a person is at a terminal outside any agent session.
`--forward-env` passes the named variables of your environment to every isolated session (for a
login the host provides: Bedrock, Vertex); `--host-auth` adds the variables of a host-managed
Claude Code login (session-ingress token file, organisation, account, base URL, proxies, CA
bundles). An agent session's own markers (CLAUDECODE, a session id) are never forwarded.
See docs/training.md.";

struct Args {
    rest: Vec<String>,
}

impl Args {
    fn flag(&mut self, name: &str) -> bool {
        match self.rest.iter().position(|a| a == name) {
            Some(i) => {
                self.rest.remove(i);
                true
            }
            None => false,
        }
    }
    fn value(&mut self, name: &str) -> Result<Option<String>> {
        match self.rest.iter().position(|a| a == name) {
            Some(i) if i + 1 < self.rest.len() => {
                let v = self.rest.remove(i + 1);
                self.rest.remove(i);
                Ok(Some(v))
            }
            Some(_) => bail!("{name} needs a value"),
            None => Ok(None),
        }
    }
    fn parsed<T: std::str::FromStr>(&mut self, name: &str) -> Result<Option<T>> {
        match self.value(name)? {
            Some(v) => Ok(Some(v.parse::<T>().map_err(|_| anyhow::anyhow!("{name}: not a valid value: {v}"))?)),
            None => Ok(None),
        }
    }
    fn done(self) -> Result<Vec<String>> {
        if let Some(a) = self.rest.iter().find(|a| a.starts_with("--")) {
            bail!("unknown option {a}\n\n{USAGE}");
        }
        Ok(self.rest)
    }
}

fn adage_home() -> PathBuf {
    adage_store::Paths::from_env().root
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = real_main(argv) {
        eprintln!("adage-train: {e:#}");
        std::process::exit(1);
    }
}

fn real_main(argv: Vec<String>) -> Result<()> {
    let Some(cmd) = argv.first().cloned() else {
        println!("{USAGE}");
        return Ok(());
    };
    // `adage train --plan plan.json …` runs a plan: the options come first, with no subcommand.
    if cmd.starts_with("--") && cmd != "--help" {
        return cmd_run(Args { rest: argv });
    }
    let mut a = Args { rest: argv[1..].to_vec() };
    match cmd.as_str() {
        "plan" => cmd_plan(&mut a).and_then(|_| a.done().map(|_| ())),
        "report" => cmd_report(a),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => bail!("unknown command {other:?}\n\n{USAGE}"),
    }
}

fn cmd_plan(a: &mut Args) -> Result<()> {
    let from = a.value("--from")?.context("plan needs --from <file>")?;
    let o = plan::PlanOptions {
        from: PathBuf::from(from),
        repo: a.value("--repo")?.map(PathBuf::from),
        seed: a.parsed("--seed")?.unwrap_or(1),
        sessions: a.parsed("--sessions")?.unwrap_or(24),
        model: a.value("--model")?.unwrap_or_else(|| plan::DEFAULT_MODEL.into()),
    };
    if o.sessions == 0 || o.sessions > 1000 {
        bail!("--sessions must be between 1 and 1000");
    }
    let json = a.flag("--json");
    let out = a.value("--out")?;
    let p = plan::build(&o)?;
    let text = serde_json::to_string_pretty(&p)?;
    if let Some(out) = &out {
        std::fs::write(out, &text).with_context(|| format!("writing {out}"))?;
    }
    if json {
        println!("{text}");
    } else {
        print!("{}", plan::render(&p));
        if let Some(out) = out {
            println!("\nplan written to {out}; next: adage train --plan {out}   (a dry run)");
        } else {
            println!("\n(pass --out plan.json to save it for `adage train --plan`)");
        }
    }
    Ok(())
}

fn harness_for(name: &str, world: Option<&str>, forward: &[String], host_auth: bool) -> Result<Box<dyn Harness>> {
    match name {
        "claude-code" => Ok(Box::new(ClaudeCode::with_forward(forward, host_auth)?)),
        "fake" => {
            let path = world.context("--harness fake needs --fake-world <world.json> (see docs/training.md)")?;
            let w: FakeWorld =
                serde_json::from_str(&std::fs::read_to_string(path)?).context("reading the fake world")?;
            Ok(Box::new(Fake { world: w }))
        }
        other => bail!("--harness {other}: only claude-code (and fake, for trying the pipeline) are implemented"),
    }
}

fn cmd_run(mut a: Args) -> Result<()> {
    let plan_path = a
        .value("--plan")?
        .context("adage train needs --plan plan.json (make one with `adage train plan --from <file>`)")?;
    let p = plan::load(Path::new(&plan_path))?;
    let mut o = run::RunOptions::defaults(adage_home());
    o.model = p.estimate.model.clone();
    let harness = a.value("--harness")?.unwrap_or_else(|| "claude-code".into());
    let world = a.value("--fake-world")?;
    let forward: Vec<String> = a
        .value("--forward-env")?
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let host_auth = a.flag("--host-auth");
    if let Some(m) = a.value("--model")? {
        o.model = m;
    }
    let budget: Option<f64> = a.parsed("--budget")?;
    if let Some(b) = budget {
        o.budget_usd = b;
    }
    o.max_turns = a.parsed("--max-turns")?.unwrap_or(o.max_turns);
    o.concurrency = a.parsed("--concurrency")?.unwrap_or(o.concurrency);
    o.validate = a.parsed("--validate")?.unwrap_or(0);
    o.oracle = a.value("--oracle")?.unwrap_or(o.oracle);
    o.container = a.value("--container")?.unwrap_or(o.container);
    o.run_id = a.value("--run-id")?;
    o.adage_bin = a.value("--adage")?.map(PathBuf::from);
    o.thresholds = a.value("--thresholds")?.unwrap_or(o.thresholds);
    if let Some(t) = a.parsed::<u64>("--timeout-secs")? {
        o.timeout = std::time::Duration::from_secs(t);
    }
    o.keep = a.flag("--keep");
    o.execute = a.flag("--execute");
    let json = a.flag("--json");
    a.done()?;
    if !matches!(o.thresholds.as_str(), "default" | "demo") {
        bail!("--thresholds must be default or demo");
    }
    if o.max_turns == 0 || o.concurrency == 0 {
        bail!("--max-turns and --concurrency must be at least 1");
    }
    let h = harness_for(&harness, world.as_deref(), &forward, host_auth)?;
    let base = isolate::process_env();
    if !o.execute {
        let d = run::describe(&p, &o, h.as_ref(), &base);
        if json {
            println!("{}", serde_json::to_string_pretty(&d)?);
        } else {
            print!("{}", run::render_describe(&d));
        }
        return Ok(());
    }
    if h.spawns_agents() {
        isolate::require_person(&base, isolate::stdin_is_tty())?;
        if budget.is_none() {
            bail!("--execute spends money: give an explicit --budget <usd>");
        }
    }
    let progress = |j: &run::Job| {
        eprintln!(
            "  {:<26} {:<8} {:<7} turns {:>3}  ${:.3}{}",
            j.job,
            j.arm.as_str(),
            j.status,
            j.outcome.turns,
            j.outcome.cost_usd,
            j.outcome.error.as_deref().map(|e| format!("  ({e})")).unwrap_or_default()
        )
    };
    let (dir, state) = run::execute(&p, &o, h.as_ref(), &base, &progress)?;
    let r = report::build(&dir, &p, &state)?;
    std::fs::write(dir.join("report.json"), serde_json::to_string_pretty(&r)?)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&r)?);
    } else {
        print!("{}", report::render(&r));
        println!("\nrun directory: {}", dir.display());
    }
    Ok(())
}

fn cmd_report(mut a: Args) -> Result<()> {
    let json = a.flag("--json");
    let rest = a.done()?;
    let [target] = rest.as_slice() else { bail!("report needs one run id or run directory\n\n{USAGE}") };
    let dir = if Path::new(target).join("state.json").is_file() {
        PathBuf::from(target)
    } else if run::safe_run_id(target) {
        run::run_dir(&adage_home(), target)
    } else {
        bail!("{target}: not a run directory, nor a run id");
    };
    let state = run::load_state(&dir)?;
    let p = plan::load(&dir.join("plan.json"))?;
    let r = report::build(&dir, &p, &state)?;
    std::fs::write(dir.join("report.json"), serde_json::to_string_pretty(&r)?)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&r)?);
    } else {
        print!("{}", report::render(&r));
    }
    Ok(())
}
