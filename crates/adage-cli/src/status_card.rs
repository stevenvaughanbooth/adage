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
//!   » today: adage: 2 new reflexes active.
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
    /// Compiled from a skill (`adage train`) and accepted here.
    pub skills: usize,
    /// Of the learned ones: procedure cards for families of similar tasks.
    pub cards: usize,
    /// Indexed routes that stay silent until separately authorized for execution.
    pub execution_only: usize,
}

impl Reflexes {
    pub fn total(&self) -> usize {
        self.instincts + self.learned + self.team + self.skills
    }
}

pub fn reflexes(index: &Index, repo: &str) -> Reflexes {
    let mut r = Reflexes::default();
    for ir in index.for_repo(repo) {
        r.execution_only += usize::from(ir.exec_only());
        if ir.route.is_instinct() {
            r.instincts += 1;
        } else if ir.track.ends_with(&format!("/{}", adage_learn::TRACK)) {
            r.learned += 1;
            if matches!(ir.route.step, adage_tracks::Step::Card { .. }) {
                r.cards += 1;
            }
        } else if ir.route.evidence.origin == Some(adage_tracks::Origin::Authored) {
            r.skills += 1;
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

/// The repository's traces so far: those the learner has read (its ledger), or, before it has
/// run, the Claude Code traces recorded in its checkouts (counted by name: nothing is opened).
fn traces_so_far(paths: &Paths, repo: &str, root: &Path) -> usize {
    let own = adage_learn::ledger::Ledger::new(&paths.ledger(), repo).read().len();
    let cfg = adage_store::config::load(paths).config;
    let projects = cfg
        .learn_claude_projects
        .clone()
        .or_else(|| adage_store::home_dir().map(|h| h.join(".claude").join("projects")));
    let found = projects
        .map(|p| adage_learn::scope::count_claude_sessions(&p, &adage_learn::scope::checkouts(root)))
        .unwrap_or(0);
    own.max(found)
}

/// The lines for one repository (after the header).
fn repo_lines(paths: &Paths, repo: &str, root: &Path, index: &Index, now: u64) -> Vec<String> {
    let mut out = Vec::new();
    let r = reflexes(index, repo);
    let mut parts = Vec::new();
    if r.instincts > 0 {
        parts.push(plural(r.instincts, "instinct", "instincts"));
    }
    if r.learned > 0 && r.cards > 0 {
        parts.push(format!("{} learned ({})", r.learned, plural(r.cards, "procedure card", "procedure cards")));
    } else if r.learned > 0 {
        parts.push(format!("{} learned", r.learned));
    }
    if r.skills > 0 {
        parts.push(format!("{} from skills", r.skills));
    }
    if r.team > 0 {
        parts.push(format!("{} from your team", r.team));
    }
    out.push(if r.total() == 0 {
        "no reflexes active yet".to_string()
    } else {
        let label = if r.execution_only > 0 { "indexed" } else { "active" };
        format!("{} {label}: {}", plural(r.total(), "reflex", "reflexes"), parts.join(", "))
    });
    if r.execution_only > 0 {
        out.push(format!("{} execution-only: silent until delegated → adage delegate status", r.execution_only));
    }

    // Learning.
    let cfg = adage_store::config::load(paths);
    let q = Queue::new(paths, repo);
    let run: Option<RunState> = read_json(&q.state());
    let own = traces_so_far(paths, repo, root);
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
            _ => "first run at your next agent session here".into(),
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
            // Said as it stands now: the switch that turns rewrites off, or that they are off.
            let now_on = cfg.config.runtime_rewrite && !adage_hook::rewrite_off(paths).exists();
            out.push(format!(
                "rewrote {} to use the project's environment ({})",
                plural(c.rewritten, "command", "commands"),
                if now_on { "turn off: adage delegate rewrites off" } else { "now off: adage delegate rewrites on" }
            ));
        }
        out.extend(savings::lines(s));
        if let Some(g) = savings::guardrail(s) {
            out.push(g);
        }
    }

    // What waits for a person: install steps, and quiet reflexes (their uplift is below the bar,
    // so they wait for a person too, but they are not install steps).
    let (steps, quiet) = waiting(paths, repo);
    if steps > 0 {
        out.push(format!(
            "{} for you → adage review",
            if steps == 1 { "1 install step waits".to_string() } else { format!("{steps} install steps wait") }
        ));
    }
    if quiet > 0 {
        out.push(format!(
            "{} quiet: agents already do {}, so {} for you to approve → adage review",
            plural(quiet, "reflex", "reflexes"),
            if quiet == 1 { "it" } else { "them" },
            if quiet == 1 { "it waits" } else { "they wait" }
        ));
    }
    if let Some(n) = notice_today(paths, repo, now) {
        out.push(format!("» today: {n}"));
    }
    out
}

/// Nascent reflexes in the repository's tracks: install steps, and quiet ones (kept quiet by
/// their uplift: `crate::evidence::quiet_line`).
pub fn waiting(paths: &Paths, repo: &str) -> (usize, usize) {
    let dir = paths.tracks().join(adage_learn::slug(repo));
    let mut out = (0, 0);
    for t in adage_tracks::io::read_all(&dir).unwrap_or_default() {
        for r in t.routes.iter().filter(|r| r.state == adage_tracks::RouteState::Nascent) {
            if crate::evidence::quiet_line(r).is_some() {
                out.1 += 1;
            } else {
                out.0 += 1;
            }
        }
    }
    out
}

/// The notice shown at session start for `repo` in the last day (`ui.notices`), if one was:
/// the text of `<slug>.notice.json` when `<slug>.notice-shown.json` records it shown.
pub fn notice_today(paths: &Paths, repo: &str, now: u64) -> Option<String> {
    if !adage_store::config::load(paths).config.ui_notices {
        return None;
    }
    let q = Queue::new(paths, repo);
    let shown: adage_hook::autolearn::Shown = read_json(&q.shown())?;
    let n: adage_hook::autolearn::Notice = read_json(&q.notice())?;
    (n.id == shown.id && now.saturating_sub(shown.at) < 86_400_000 && !n.text.is_empty()).then_some(n.text)
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
            for l in repo_lines(paths, &repo, root, &index, now) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use adage_hook::autolearn::{Notice, Shown};

    #[test]
    fn quiet_reflexes_are_not_install_steps_and_todays_notice_shows() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let repo = "harbor-dispatch";
        let mut t = crate::ui::demo::track(0);
        t.routes[0].state = adage_tracks::RouteState::Nascent;
        t.routes[0].evidence.uplift = Some(adage_tracks::Uplift { mean: 0.04, gated: true, ..Default::default() });
        t.routes[1].state = adage_tracks::RouteState::Nascent;
        adage_tracks::io::write(&paths.tracks().join(adage_learn::slug(repo)).join("learned"), &t).unwrap();
        assert_eq!(waiting(&paths, repo), (1, 1), "one install step, one quiet reflex");

        let now = 1_790_165_040_000;
        let q = Queue::new(&paths, repo);
        std::fs::create_dir_all(paths.learn_queue()).unwrap();
        let n = Notice {
            id: "n1".into(),
            text: "adage: 2 new reflexes active.".into(),
            kind: "reflexes".into(),
            at: now - 60_000,
            basis: serde_json::Value::Null,
            baseline: serde_json::Value::Null,
        };
        crate::autolearn::write_json(&q.notice(), &n).unwrap();
        assert_eq!(notice_today(&paths, repo, now), None, "composed, not shown yet");
        crate::autolearn::write_json(&q.shown(), &Shown { id: "n1".into(), at: now - 30_000 }).unwrap();
        assert_eq!(notice_today(&paths, repo, now).as_deref(), Some("adage: 2 new reflexes active."));
        assert_eq!(notice_today(&paths, repo, now + 2 * 86_400_000), None, "yesterday's is not today's");
        std::fs::write(paths.config(), "[ui]\nnotices = \"off\"\n").unwrap();
        assert_eq!(notice_today(&paths, repo, now), None, "ui.notices = off");
    }
}
