//! Bare `adage`: the status card (docs/zero-config.md §3.4). The same few lines whether stdout is
//! a terminal, a pipe, or `!adage` inside Claude Code: what Adage is doing for this repository,
//! whether it is healthy, what it has measured, and the one thing that waits for a person.
//!
//! ```text
//! adage 0.1.0 · github.com/acme/billing                         ● healthy
//!   14 reflexes active: 4 instincts, 9 learned, 1 from your team
//!   learning after each session · last: 2m ago, 1 change
//!   last 30 days: 61 sessions · 188 directives · 152 followed · 7 dead ends headed off
//!   savings: ~$0.034 and 3.1 turns per session, API-list equivalent (95% CI $0.011–$0.058)
//!            ≈ $1.87 over 55 sessions, vs 6 where Adage stayed quiet (priced as of 2026-06-24)
//!   1 install step waits for you → adage review
//!   embeddings: potion-base-8m-i8 (bundled) · adage ui · adage map · adage doctor
//! ```
//!
//! Outside a repository it lists every repository with reflexes, one line each. It only reads:
//! nothing is written, nothing runs, no network.

use crate::autolearn::{read_json, Queue, RunState};
use crate::doctor::{self, Status};
use crate::savings;
use adage_store::Paths;
use adage_tracks::Index;
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;

/// Sessions of its own a repository needs before learned reflexes can form (the default
/// `min_support`): until then instincts carry it.
const SESSIONS_NEEDED: usize = 5;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Reflexes {
    pub instincts: usize,
    pub learned: usize,
    pub team: usize,
}

impl Reflexes {
    pub fn total(&self) -> usize {
        self.instincts + self.learned + self.team
    }
}

pub fn reflexes(index: &Index, repo: &str) -> Reflexes {
    let mut r = Reflexes::default();
    for ir in index.for_repo(repo) {
        if ir.route.is_instinct() {
            r.instincts += 1;
        } else if ir.track.ends_with(&format!("/{}", adage_learn::TRACK)) {
            r.learned += 1;
        } else {
            r.team += 1;
        }
    }
    r
}

fn ago(now: u64, ts: u64) -> String {
    let s = now.saturating_sub(ts) / 1000;
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=172_799 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86_400),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The health dot: the worst doctor status, and the single fix line for it.
fn health(paths: &Paths, root: Option<&Path>) -> (String, Option<String>) {
    let checks = doctor::checks(paths, root);
    let worst =
        checks.iter().find(|c| c.status == Status::Fail).or_else(|| checks.iter().find(|c| c.status == Status::Warn));
    match worst {
        None => ("● healthy".into(), None),
        Some(c) => {
            let word = if c.status == Status::Fail { "problem" } else { "check" };
            let fix = c.fix.clone().map(|f| format!("{}: {} → {f}", c.check, c.detail));
            (format!("● {word}: {}", c.check), fix)
        }
    }
}

/// Is Adage connected to any agent here: hooks registered (settings, plugin files, the Claude
/// Code plugin's staged binary), or any hook has ever run?
fn connected(paths: &Paths, root: Option<&Path>) -> bool {
    !crate::detect::registrations(root).is_empty()
        || std::fs::read_dir(paths.sessions()).into_iter().flatten().flatten().next().is_some()
        || std::fs::read_dir(paths.learn_queue()).into_iter().flatten().flatten().next().is_some()
}

/// The lines for one repository (after the header).
fn repo_lines(paths: &Paths, repo: &str, index: &Index, now: u64) -> Vec<String> {
    let mut out = Vec::new();
    let r = reflexes(index, repo);
    let mut parts = Vec::new();
    if r.instincts > 0 {
        parts.push(plural(r.instincts, "instinct", "instincts"));
    }
    if r.learned > 0 {
        parts.push(format!("{} learned", r.learned));
    }
    if r.team > 0 {
        parts.push(format!("{} from your team", r.team));
    }
    out.push(if r.total() == 0 {
        "no reflexes active yet".to_string()
    } else {
        format!("{} active: {}", plural(r.total(), "reflex", "reflexes"), parts.join(", "))
    });

    // Learning.
    let cfg = adage_store::config::load(paths);
    let q = Queue::new(paths, repo);
    let run: Option<RunState> = read_json(&q.state());
    let ledger = adage_learn::ledger::Ledger::new(&paths.ledger(), repo).read();
    let own = ledger.len();
    if !cfg.config.learn_auto {
        let (_, source) = &cfg.values["learn.auto"];
        out.push(format!("learning is off ({source}) → adage config set learn.auto true"));
    } else if r.learned == 0 && own < SESSIONS_NEEDED {
        out.push(format!(
            "learning: {own} of ~{SESSIONS_NEEDED} sessions needed before Adage forms its own reflexes{}",
            if r.instincts > 0 {
                format!("; {} active meanwhile", plural(r.instincts, "instinct", "instincts"))
            } else {
                String::new()
            }
        ));
    } else {
        let last = crate::learnlog::read(paths).into_iter().rev().find(|l| l["repo"].as_str() == Some(repo));
        let when = match (&last, &run) {
            (Some(l), _) if l["kind"] == "condition_error" => {
                format!(
                    "last: {}, failed ({})",
                    ago(now, l["ts"].as_u64().unwrap_or(0)),
                    l["class"].as_str().unwrap_or("error")
                )
            }
            (Some(l), _) => {
                let n = l["changes"].as_array().map(|c| c.len()).unwrap_or(0);
                format!("last: {}, {}", ago(now, l["ts"].as_u64().unwrap_or(0)), plural(n, "change", "changes"))
            }
            (None, Some(s)) if s.finished_at.is_some() => format!("last: {}", ago(now, s.finished_at.unwrap_or(0))),
            _ => "not yet run".into(),
        };
        out.push(format!("learning after each session · {when}"));
    }

    // Counts and savings.
    let summary = savings::read(paths, repo);
    if let Some(s) = &summary {
        let c = &s.counts;
        if c.sessions > 0 {
            out.push(format!(
                "last {} days: {} · {} · {} followed · {} headed off",
                c.days,
                plural(c.sessions, "session", "sessions"),
                plural(c.directives, "directive", "directives"),
                c.followed,
                plural(c.dead_ends_headed_off, "dead end", "dead ends"),
            ));
        }
        // Adage acting, not advising: always visible (docs/execution.md, rewrites).
        if c.rewritten > 0 {
            out.push(format!(
                "rewrote {} to use the project's environment (off: adage delegate off)",
                plural(c.rewritten, "command", "commands")
            ));
        }
        out.extend(savings::lines(s));
        if let Some(g) = savings::guardrail(s) {
            out.push(g);
        }
    }

    // What waits for a person.
    let dir = paths.tracks().join(adage_learn::slug(repo));
    let nascent = adage_tracks::io::read_all(&dir)
        .unwrap_or_default()
        .iter()
        .flat_map(|t| t.routes.iter())
        .filter(|r| r.state == adage_tracks::RouteState::Nascent)
        .count();
    if nascent > 0 {
        out.push(format!(
            "{} for you → adage review",
            if nascent == 1 { "1 install step waits".to_string() } else { format!("{nascent} install steps wait") }
        ));
    }
    out
}

/// What hooks and the learner embed with, in a few words: `potion-base-8m (bundled)`, an
/// installed model's key, or `hashed fallback`; and whether that is a fallback nobody chose
/// (no model installed or bundled), which the card's header then says in grey.
pub fn embeddings(paths: &Paths) -> (String, bool) {
    let r = adage_intent::model::resolve(&paths.models(), false);
    let id = r.embedder.id();
    let key = id.split('@').next().unwrap_or(&id).to_string();
    match r.source {
        adage_intent::model::Source::Installed { .. } => (key, false),
        adage_intent::model::Source::Bundled => (format!("{key} (bundled)"), false),
        adage_intent::model::Source::Hashed { .. } => ("hashed fallback".into(), r.selected != "hashed"),
    }
}

/// Bare `adage`.
pub fn run(paths: &Paths) -> Result<()> {
    print!("{}", card(paths, std::env::current_dir().ok().as_deref()));
    Ok(())
}

pub fn card(paths: &Paths, cwd: Option<&Path>) -> String {
    let now = crate::learnlog::now_ms();
    let version = env!("CARGO_PKG_VERSION");
    let root = cwd.and_then(adage_core::repo::find_root);
    let index = Index::load(&paths.index());
    let mut s = String::new();
    if !connected(paths, root.as_deref()) {
        s.push_str(&format!("adage {version}\n"));
        s.push_str("  Not connected to any agent yet.\n");
        let found: Vec<&str> =
            crate::detect::harnesses().into_iter().filter(|h| h.found()).map(|h| h.harness).collect();
        if found.contains(&"claude-code") {
            s.push_str("  Claude Code found → in Claude Code: /plugin marketplace add stevenvaughanbooth/adage, then /plugin install adage@adage\n");
            s.push_str("                       or, from here: adage install claude-code --write --scope user\n");
        }
        for h in found.iter().filter(|h| **h != "claude-code") {
            s.push_str(&format!("  {h} found → adage install {h} --write --scope user\n"));
        }
        if found.is_empty() {
            s.push_str("  No agent found: install Claude Code, Codex or OpenCode, then run adage again.\n");
        }
        s.push_str("  adage --help · adage doctor\n");
        return s;
    }
    let (dot, fix) = health(paths, root.as_deref());
    let (embed, fallback) = embeddings(paths);
    let grey = if fallback { "  (no embedding model: hashed fallback)" } else { "" };
    match &root {
        Some(root) => {
            let repo = crate::ui::health::repo_key(paths, root, &index);
            let head = format!("adage {version} · {repo}");
            s.push_str(&format!("{head:<62} {dot}{grey}\n"));
            for l in repo_lines(paths, &repo, &index, now) {
                s.push_str(&format!("  {l}\n"));
            }
        }
        None => {
            s.push_str(&format!("{:<62} {dot}{grey}\n", format!("adage {version}")));
            let mut repos: BTreeMap<&str, ()> = BTreeMap::new();
            for r in &index.routes {
                repos.insert(r.repo.as_str(), ());
            }
            if repos.is_empty() {
                s.push_str("  no reflexes yet: start an agent session in a repository\n");
            }
            for repo in repos.keys() {
                let r = reflexes(&index, repo);
                let line = savings::read(paths, repo).map(|x| savings::lines(&x).remove(0)).unwrap_or_default();
                s.push_str(&format!("  {repo:<40} {:>3} reflexes  {line}\n", r.total()));
            }
        }
    }
    if let Some(f) = fix {
        s.push_str(&format!("  {f}\n"));
    }
    s.push_str(&format!("  embeddings: {embed} · adage ui · adage map · adage doctor\n"));
    s
}
