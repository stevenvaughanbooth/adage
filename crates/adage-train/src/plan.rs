//! `adage train plan`: claims, tasks and a budget estimate from one source file. Free: it reads
//! files and executes nothing.

use crate::claims::{self, Claim, SourceKind};
use crate::tasks::{self, Task};
use adage_core::CommandTable;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const FORMAT: u32 = 1;
/// The model the estimate assumes when none is given.
pub const DEFAULT_MODEL: &str = "claude-haiku-4-5";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub path: PathBuf,
    pub kind: SourceKind,
    /// The skill's name (front matter `name:`, else the file stem).
    pub name: String,
    pub sha256: String,
    /// The source text, as planned: the run appends exactly this to the skill arm's system prompt.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub root: PathBuf,
    /// The repository key staged reflexes are learned under (`adage_core::repo::repo_key`).
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Estimate {
    pub model: String,
    pub sessions: usize,
    pub usd_per_session: f64,
    pub usd: f64,
    /// Each held-out task of `run --validate N` adds one session per arm (three arms).
    pub validate_usd_per_task: f64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub format: u32,
    pub source: Source,
    pub repo: Repo,
    pub seed: u64,
    pub claims: Vec<Claim>,
    /// Why each claim without tasks has none.
    pub not_exercised: Vec<(String, String)>,
    pub tasks: Vec<Task>,
    pub estimate: Estimate,
}

/// A rough cost per headless session by model family, in USD. An estimate from the demo runs
/// (README: Haiku 4.5 $0.05–0.14, Sonnet 5 $0.07–0.09 per run at up to 30 turns), rounded up.
pub fn usd_per_session(model: &str) -> f64 {
    let m = model.to_lowercase();
    if m.contains("haiku") {
        0.15
    } else if m.contains("sonnet") {
        0.25
    } else if m.contains("opus") {
        0.75
    } else if m == "fake" {
        0.0
    } else {
        0.50
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The repository containing `start` (its git root), else `start` itself.
pub fn repo_root(start: &Path) -> PathBuf {
    let abs = std::fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    let dir = if abs.is_file() { abs.parent().map(Path::to_path_buf).unwrap_or(abs) } else { abs };
    adage_core::repo::find_root(&dir).unwrap_or(dir)
}

pub struct PlanOptions {
    pub from: PathBuf,
    pub repo: Option<PathBuf>,
    pub seed: u64,
    pub sessions: usize,
    pub model: String,
}

pub fn build(o: &PlanOptions) -> Result<Plan> {
    let bytes = std::fs::read(&o.from).with_context(|| format!("reading {}", o.from.display()))?;
    if bytes.len() > 1 << 20 {
        bail!("{} is larger than 1 MiB", o.from.display());
    }
    let text = String::from_utf8(bytes.clone()).with_context(|| format!("{} is not UTF-8", o.from.display()))?;
    let root = repo_root(o.repo.as_deref().unwrap_or(&o.from));
    if !root.is_dir() {
        bail!("{} is not a directory", root.display());
    }
    let from = std::fs::canonicalize(&o.from).unwrap_or_else(|_| o.from.clone());
    let table = CommandTable::load();
    let claims = claims::extract(&from, &text, &root, table);
    let files = adage_instincts::files::RepoFiles::open(&root).map(|f| f.walk()).unwrap_or_default();
    let name = claims::skill_name(&text, &from);
    let not_exercised: Vec<(String, String)> =
        claims.iter().filter_map(|c| tasks::exercisable(c, &files).err().map(|e| (c.id.clone(), e))).collect();
    let tasks = tasks::generate(&claims, &files, &name, o.seed, o.sessions, &BTreeSet::new(), "t");
    let per = usd_per_session(&o.model);
    let estimate = Estimate {
        model: o.model.clone(),
        sessions: tasks.len(),
        usd_per_session: per,
        usd: per * tasks.len() as f64,
        validate_usd_per_task: per * 3.0,
        note: "ESTIMATE, not a quote: a rounded cost per session for the model family; real cost depends on \
               the repository, the prompts and --max-turns. `run` stops starting sessions at --budget."
            .into(),
    };
    Ok(Plan {
        format: FORMAT,
        source: Source { path: from.clone(), kind: SourceKind::of(&from), name, sha256: sha256_hex(&bytes), text },
        repo: Repo { key: adage_core::repo::repo_key(&root), root },
        seed: o.seed,
        claims,
        not_exercised,
        tasks,
        estimate,
    })
}

pub fn load(path: &Path) -> Result<Plan> {
    let p: Plan =
        serde_json::from_str(&std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?)
            .with_context(|| format!("{} is not an adage train plan", path.display()))?;
    if p.format != FORMAT {
        bail!("{}: plan format {} (this adage-train reads {FORMAT})", path.display(), p.format);
    }
    if sha256_hex(p.source.text.as_bytes()) != p.source.sha256 {
        bail!("{}: the source text does not match its sha256; re-plan", path.display());
    }
    Ok(p)
}

pub fn render(p: &Plan) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "adage train plan (free: nothing was executed)");
    let _ = writeln!(s, "source  {} ({:?}, skill `{}`)", p.source.path.display(), p.source.kind, p.source.name);
    let _ = writeln!(s, "        sha256 {}", p.source.sha256);
    let _ = writeln!(s, "repo    {} (key {})", p.repo.root.display(), p.repo.key);
    let _ = writeln!(s, "seed    {}", p.seed);
    let _ = writeln!(s, "\nclaims ({}):", p.claims.len());
    for c in &p.claims {
        let tag = if c.is_unsafe() { "UNSAFE" } else { c.effect.as_str() };
        let _ = writeln!(s, "  {:<4} {:<11} {}   [{}]", c.id, tag, c.kind.describe(), c.at);
        if let Some(u) = &c.r#unsafe {
            let _ = writeln!(s, "       {u}");
        }
    }
    if !p.not_exercised.is_empty() {
        let _ = writeln!(s, "\nnot exercised:");
        for (id, why) in &p.not_exercised {
            let _ = writeln!(s, "  {id:<4} {why}");
        }
    }
    let controls = p.tasks.iter().filter(|t| t.control).count();
    let explicit = p.tasks.iter().filter(|t| t.explicit).count();
    let _ = writeln!(
        s,
        "\ntasks ({}: {} trigger, of which {} explicit; {} control):",
        p.tasks.len(),
        p.tasks.len() - controls,
        explicit,
        controls
    );
    for t in &p.tasks {
        let what = match (&t.claim, t.explicit) {
            (Some(c), true) => format!("{c} explicit"),
            (Some(c), false) => format!("{c} implicit"),
            (None, _) => "control".into(),
        };
        let _ = writeln!(s, "  {} {:<13} {}", t.id, what, t.prompt);
    }
    let e = &p.estimate;
    let _ = writeln!(
        s,
        "\nbudget estimate: {} sessions × ${:.2}/session ({}) = ${:.2}",
        e.sessions, e.usd_per_session, e.model, e.usd
    );
    let _ = writeln!(
        s,
        "                 + per held-out task with --validate: 3 arms × ${:.2} = ${:.2}",
        e.usd_per_session, e.validate_usd_per_task
    );
    let _ = writeln!(s, "                 {}", e.note);
    s
}
