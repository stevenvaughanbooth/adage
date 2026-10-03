//! `adage sense` (Design J1): "what does my agent actually do?" — a read-only report over
//! existing session history, per repository.
//!
//! ```text
//! adage sense [--all | --repo <key>] [--since <days>] [--json] [--demo]
//!              [--claude-projects <dir>] [--codex-sessions <dir>] [--transcripts <file>...]
//! ```
//!
//! Sessions are found as `adage condition` finds them: by default the configured history
//! recorded in the repository this runs in (picked before anything is parsed, as the learner
//! picks them: [`adage_learn::scope`]); `--all` reads every repository's; explicit sources,
//! including `--transcripts` files, replace the configured ones. `--repo` keeps one repository
//! key; `--since` keeps sessions from the last N days. For each repository
//! the report shows sessions and prompts, the most common command templates, turns before the
//! first edit, repeated failing commands (dead-end candidates), edits followed by regeneration
//! (finish-line candidates), what prompts name (work items, files, commits, links), and the routes
//! `adage condition` would build from the same sessions — computed by learn's own dry run
//! ([`adage_learn::plan`]) and marked as candidates. Nothing is written.
//!
//! Large histories: files are streamed one at a time and folded into bounded per-repository
//! counters; only the most recent [`MINED_PER_REPO`] sessions per repository are kept (with tool
//! output trimmed) for the candidate routes, and the report says when it sampled. Prompts are
//! never printed beyond short snippets, with home paths and secret-looking strings redacted
//! ([`crate::redact::scrub`]).

use crate::redact::scrub;
use adage_core::spans::{self, SpanKind};
use adage_core::{keys, CanonicalAction, CommandTable, Effect};
use adage_learn::import::Session;
use adage_learn::LearnConfig;
use adage_store::Paths;
use adage_tracks::review::strength_str;
use adage_tracks::Route;
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Sessions per repository kept in memory for the candidate routes (the most recent).
pub const MINED_PER_REPO: usize = 1000;
/// Tool output kept per call for mining: the head (a lookup's listing) and the tail (the error).
const OUTPUT_HEAD: usize = 6 << 10;
const OUTPUT_TAIL: usize = 2 << 10;
/// Distinct command templates tracked per repository before rare ones are pruned.
const MAX_KEYS: usize = 20_000;
const TOP: usize = 10;
const SNIPPET: usize = 72;
const SAMPLES: usize = 3;

#[derive(Debug, Default)]
pub struct Options {
    pub repo: Option<String>,
    /// Only traces recorded in this repository's checkouts; `None` reads every repository's.
    pub root: Option<PathBuf>,
    pub since_days: Option<u32>,
    pub demo: bool,
    pub claude_projects: Option<PathBuf>,
    /// Explicit Codex rollouts directory, replacing default source selection.
    pub codex_sessions: Option<PathBuf>,
    pub transcripts: Vec<PathBuf>,
    /// Keep an explicitly empty `--transcripts` from silently selecting other history.
    pub explicit_sources: bool,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Options> {
        let mut o = Options {
            repo: crate::flag(args, "--repo"),
            demo: args.iter().any(|a| a == "--demo"),
            explicit_sources: args
                .iter()
                .any(|a| matches!(a.as_str(), "--transcripts" | "--claude-projects" | "--codex-sessions")),
            ..Default::default()
        };
        if let Some(d) = crate::flag(args, "--since") {
            match d.parse::<u32>() {
                Ok(n) if n > 0 => o.since_days = Some(n),
                _ => bail!("--since takes a number of days, e.g. --since 30"),
            }
        }
        o.claude_projects = crate::flag(args, "--claude-projects").map(PathBuf::from);
        o.codex_sessions = crate::flag(args, "--codex-sessions").map(PathBuf::from);
        // The repository this runs in, unless a source, a key or every repository is named.
        if !o.explicit_sources && o.repo.is_none() && !args.iter().any(|a| a == "--all") {
            o.root = Some(crate::here_or_all("sense")?);
        }
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--transcripts" {
                i += 1;
                while i < args.len() && !args[i].starts_with("--") {
                    o.transcripts.push(PathBuf::from(&args[i]));
                    i += 1;
                }
                continue;
            }
            i += 1;
        }
        Ok(o)
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Sense {
    /// Where sessions came from (home directory redacted).
    pub source: String,
    pub files: usize,
    pub sessions: usize,
    /// Only sessions on or after this date, with `--since`.
    pub since: Option<String>,
    pub thresholds: String,
    pub intent_model: Option<String>,
    pub repos: Vec<RepoSense>,
}

#[derive(Debug, Default, Serialize)]
pub struct RepoSense {
    pub repo: String,
    pub sessions: u32,
    pub prompts: u32,
    pub harnesses: Vec<String>,
    pub first_day: String,
    pub last_day: String,
    /// Sessions that already received Adage directives.
    pub steered: u32,
    pub turns_before_first_edit: Turns,
    pub commands: Vec<CommandCount>,
    pub repeated_failures: Vec<Failure>,
    pub regeneration_after_edits: Vec<Regeneration>,
    pub prompt_spans: Vec<SpanCount>,
    pub sample_prompts: Vec<String>,
    /// Sessions the candidate routes were computed from.
    pub mined_sessions: u32,
    /// True when the history was larger than [`MINED_PER_REPO`] and only the most recent
    /// sessions were mined for candidates.
    pub sampled: bool,
    pub candidates: Vec<Candidate>,
    pub intent_families: usize,
    /// Families of similar tasks, and the procedure cards (reflexes) they would form.
    pub card_families: usize,
    pub cards: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct Turns {
    pub median: Option<u32>,
    pub p75: Option<u32>,
    pub max: Option<u32>,
    pub sessions_without_edit: u32,
}

#[derive(Debug, Serialize)]
pub struct CommandCount {
    pub template: String,
    pub sessions: u32,
    pub runs: u32,
}

#[derive(Debug, Serialize)]
pub struct Failure {
    pub template: String,
    pub sessions: u32,
    pub runs: u32,
    /// What the same sessions ran successfully next, instead (most common first).
    pub then_worked: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Regeneration {
    pub edits: String,
    pub then_ran: String,
    pub sessions: u32,
    pub editing_sessions: u32,
}

#[derive(Debug, Serialize)]
pub struct SpanCount {
    pub kind: String,
    pub sessions: u32,
    /// Work-item id shapes (`W-####`), never values.
    pub shapes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Candidate {
    pub track: String,
    pub route: String,
    /// `new`, `installed` (identical to what is installed), `update`, or `proposed` (a pinned
    /// route whose learned version differs).
    pub status: String,
    pub strength: String,
    pub effect: String,
    pub trigger: String,
    pub step: String,
    pub support: u32,
    pub opportunities: u32,
    pub lower_bound: f64,
    /// What delivering it changes (`adage_learn::value::assess`): why a quiet reflex is quiet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uplift: Option<adage_tracks::Uplift>,
}

/// A failing command: (sessions, runs, what worked next → sessions).
type FailCount = (u32, u32, BTreeMap<String, u32>);

/// Per-repository counters, folded one session at a time.
#[derive(Default)]
struct Acc {
    sessions: u32,
    prompts: u32,
    harnesses: BTreeSet<String>,
    days: (String, String),
    steered: u32,
    turns: Vec<u32>,
    no_edit: u32,
    commands: HashMap<String, (u32, u32)>,
    failures: HashMap<String, FailCount>,
    editors: HashMap<String, u32>,
    regen: HashMap<(String, String), u32>,
    spans: BTreeMap<&'static str, (u32, BTreeMap<String, u32>)>,
    samples: Vec<(String, String, String)>,
    mined: Vec<Session>,
    sampled: bool,
}

fn home() -> Option<String> {
    adage_store::paths::home_dir().map(|h| h.to_string_lossy().into_owned()).filter(|h| !h.is_empty())
}

fn redacted(s: &str) -> String {
    scrub(s, home().as_deref())
}

/// A history directory as a source line names it: whole, with the home directory as `~`. It is a
/// place the person configured, not trace content, so it is not scrubbed for secrets (which cut
/// a long directory name in half).
fn place(d: &std::path::Path) -> String {
    crate::detect::tilde(d)
}

/// `W-1042` → `W-####`: the shape of an id, never the id.
fn shape(v: &str) -> String {
    v.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect()
}

/// A short, single-line, redacted snippet of a prompt. Redacted before it is cut, so a cut never
/// leaves part of a path or secret behind.
fn snippet(p: &str) -> String {
    let head: String = p.chars().take(SNIPPET * 8).collect();
    let flat = redacted(&head.split_whitespace().collect::<Vec<_>>().join(" "));
    let mut s: String = flat.chars().take(SNIPPET).collect();
    if flat.chars().count() > SNIPPET || p.chars().count() > SNIPPET * 8 {
        s.push('…');
    }
    s
}

fn trim_output(s: &mut String) {
    if s.len() <= OUTPUT_HEAD + OUTPUT_TAIL {
        return;
    }
    let head = (0..=OUTPUT_HEAD).rev().find(|i| s.is_char_boundary(*i)).unwrap_or(0);
    let tail = (s.len() - OUTPUT_TAIL..s.len()).find(|i| s.is_char_boundary(*i)).unwrap_or(s.len());
    *s = format!("{}\n…\n{}", &s[..head], &s[tail..]);
}

/// Drop what candidate mining never reads, so a retained session costs little.
fn compact(s: &mut Session) {
    let keep: usize = s.prompt.char_indices().nth(4000).map(|(i, _)| i).unwrap_or(s.prompt.len());
    s.prompt.truncate(keep);
    // Candidate mining reads the first prompt only.
    s.prompts.clear();
    for st in &mut s.steps {
        // A shell command's first line stays: a procedure card lists the commands of its family.
        let cmd = st.call.input.get("command").and_then(|c| c.as_str()).and_then(|c| c.lines().next());
        let cmd: Option<String> = cmd.map(|c| c.chars().take(1000).collect());
        st.call.input = match cmd {
            Some(c) => serde_json::json!({ "command": c }),
            None => serde_json::Value::Null,
        };
        trim_output(&mut st.call.output);
    }
}

fn is_exec(a: &CanonicalAction) -> bool {
    matches!(a, CanonicalAction::Exec { .. })
}

impl Acc {
    fn add(&mut self, mut s: Session) {
        self.sessions += 1;
        self.prompts += s.prompt_count();
        self.harnesses.insert(s.harness.clone());
        self.steered += u32::from(s.steered);
        if !s.day.is_empty() {
            if self.days.0.is_empty() || s.day < self.days.0 {
                self.days.0 = s.day.clone();
            }
            if s.day > self.days.1 {
                self.days.1 = s.day.clone();
            }
        }
        match s.first_edit() {
            Some(i) => self.turns.push(s.steps[i].turn),
            None => self.no_edit += 1,
        }
        // Commands, in distinct sessions and runs.
        let mut seen = BTreeSet::new();
        for st in s.steps.iter().filter(|st| is_exec(&st.norm.action)) {
            let e = self.commands.entry(st.key.clone()).or_default();
            e.1 += 1;
            if seen.insert(st.key.as_str()) {
                e.0 += 1;
            }
        }
        // Failing commands, and what the session ran successfully next instead (the dead-end
        // learner's pairing: a different command of the same effect within five steps). One-off
        // inline code and housekeeping (`echo`, `cat`, `mkdir`) are no conventions: neither a dead
        // end nor what works instead.
        let noise = |k: &str| {
            k.contains("{code}") || adage_graph::compile::shapes::housekeeping(k.strip_prefix("exec ").unwrap_or(k))
        };
        let mut failed_here = BTreeSet::new();
        for (i, a) in s.steps.iter().enumerate() {
            if !a.call.failed || !is_exec(&a.norm.action) || noise(&a.key) {
                continue;
            }
            let e = self.failures.entry(a.key.clone()).or_default();
            e.1 += 1;
            if failed_here.insert(a.key.clone()) {
                e.0 += 1;
                if let Some(b) = s.steps.iter().skip(i + 1).take(5).find(|b| {
                    is_exec(&b.norm.action)
                        && !b.call.failed
                        && b.norm.effect == a.norm.effect
                        && b.key != a.key
                        && !noise(&b.key)
                }) {
                    *e.2.entry(b.key.clone()).or_default() += 1;
                }
            }
        }
        // Edits to a path pattern followed by a successful regeneration step.
        let mut last_edit: BTreeMap<String, usize> = BTreeMap::new();
        for (i, st) in s.steps.iter().enumerate() {
            if keys::is_author(&st.norm.action) {
                if let Some(g) = st.rel_path.as_deref().and_then(adage_learn::evidence::glob_for) {
                    last_edit.insert(g, i);
                }
            }
        }
        for (g, &li) in &last_edit {
            *self.editors.entry(g.clone()).or_default() += 1;
            let ran: BTreeSet<&str> = s.steps[li + 1..]
                .iter()
                .filter(|st| st.norm.effect == Effect::Regenerate && !st.call.failed && is_exec(&st.norm.action))
                .map(|st| st.key.as_str())
                .collect();
            for c in ran {
                *self.regen.entry((g.clone(), c.to_string())).or_default() += 1;
            }
        }
        // What prompts name.
        let found = spans::extract(&s.prompt);
        let kinds: BTreeSet<SpanKind> = found.iter().map(|x| x.kind).collect();
        for k in kinds {
            let e = self.spans.entry(adage_runtime::span_kind_label(k)).or_default();
            e.0 += 1;
            if k == SpanKind::Ticket {
                for v in found.iter().filter(|x| x.kind == SpanKind::Ticket) {
                    *e.1.entry(shape(&v.value)).or_default() += 1;
                }
            }
        }
        self.samples.push((s.day.clone(), s.id.clone(), snippet(&s.prompt)));
        if self.samples.len() > SAMPLES * 4 {
            self.samples.sort_by(|a, b| (&b.0, &b.1).cmp(&(&a.0, &a.1)));
            self.samples.truncate(SAMPLES);
        }
        if self.commands.len() > MAX_KEYS {
            self.commands.retain(|_, v| v.0 > 1);
        }
        if self.failures.len() > MAX_KEYS {
            self.failures.retain(|_, v| v.0 > 1);
        }
        // Keep a bounded, most-recent sample for the candidate routes.
        compact(&mut s);
        self.mined.push(s);
        if self.mined.len() >= MINED_PER_REPO * 2 {
            self.keep_recent();
        }
    }

    fn keep_recent(&mut self) {
        if self.mined.len() > MINED_PER_REPO {
            self.mined.sort_by(|a, b| (&b.day, &b.id).cmp(&(&a.day, &a.id)));
            self.mined.truncate(MINED_PER_REPO);
            self.sampled = true;
        }
    }
}

fn percentile(sorted: &[u32], p: f64) -> Option<u32> {
    (!sorted.is_empty()).then(|| sorted[((sorted.len() - 1) as f64 * p).round() as usize])
}

/// Session files, as `adage condition` finds them, and a description of where they came from.
fn discover(o: &Options, paths: &Paths) -> (Vec<PathBuf>, String) {
    let mut files = o.transcripts.clone();
    let cfg = adage_store::config::load(paths).config;
    let explicit =
        o.explicit_sources || !o.transcripts.is_empty() || o.codex_sessions.is_some() || o.claude_projects.is_some();
    let dir =
        o.claude_projects.clone().or_else(|| (!explicit).then(|| crate::history::claude_projects(&cfg)).flatten());
    let codex = o
        .codex_sessions
        .clone()
        .or_else(|| (!explicit).then(|| crate::history::codex_sessions(&cfg, o.root.as_deref())).flatten());
    // One repository's traces, picked by where they were recorded before any is parsed.
    let roots = o.root.as_deref().filter(|_| !explicit).map(adage_learn::scope::checkouts);
    let mine = |found: Vec<PathBuf>| match &roots {
        Some(r) => adage_learn::scope::recorded_inside(found, r),
        None => found,
    };
    let mut source: Vec<String> = Vec::new();
    if let Some(d) = &codex {
        let found = mine(adage_learn::codex::discover(d).0);
        source.push(format!("{} Codex rollout(s) in {}", found.len(), place(d)));
        files.extend(found);
    }
    if !o.transcripts.is_empty() {
        source.push(format!("{} file(s) given with --transcripts", o.transcripts.len()));
    }
    if let Some(d) = dir {
        source.push(format!("Claude Code history in {}", place(&d)));
        files.extend(match &roots {
            Some(r) => adage_learn::scope::claude_transcripts(&d, r),
            None => adage_learn::import::discover(&d),
        });
    }
    if !explicit {
        if let Some(d) = &cfg.learn_opencode_exports {
            source.push(format!("OpenCode exports in {}", place(d)));
            files.extend(mine(crate::history::opencode_exports(d)));
        }
    }
    files.sort();
    files.dedup();
    let mut source = source.join(" and ");
    if let Some(root) = o.root.as_deref().filter(|_| !explicit && !source.is_empty()) {
        source.push_str(&format!(", recorded in {} (--all reads every repository's)", place(root)));
    }
    (files, source)
}

/// Days since the Unix epoch, now.
fn today_days() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0) as i64
}

fn modified_before(p: &Path, cutoff_days: i64) -> bool {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .is_some_and(|d| ((d.as_secs() / 86_400) as i64) < cutoff_days)
}

/// Run `adage sense`. Reads history and installed tracks; writes nothing.
pub fn run(o: &Options, paths: &Paths) -> Result<Sense> {
    let (files, source) = discover(o, paths);
    let table = CommandTable::load();
    let cutoff = o.since_days.map(|d| today_days() - i64::from(d));
    let since = cutoff.map(adage_tracks::review::civil_date);
    let mut accs: BTreeMap<String, Acc> = BTreeMap::new();
    let mut sense = Sense { source, files: files.len(), since: since.clone(), ..Default::default() };
    for p in &files {
        // A file last written before the window cannot hold a session inside it.
        if cutoff.is_some_and(|c| modified_before(p, c)) {
            continue;
        }
        let Some(s) = adage_learn::read_session(p, table, None) else { continue };
        if s.steps.is_empty() && s.prompts.is_empty() {
            continue;
        }
        if o.repo.as_ref().is_some_and(|r| *r != s.repo) {
            continue;
        }
        if since.as_ref().is_some_and(|d| s.day.is_empty() || s.day < *d) {
            continue;
        }
        sense.sessions += 1;
        accs.entry(s.repo.clone()).or_default().add(s);
    }

    // Candidate routes: learn's own dry run over the retained sessions.
    let mut mined = Vec::new();
    for a in accs.values_mut() {
        a.keep_recent();
        mined.append(&mut a.mined);
    }
    let settings = adage_store::config::load(paths).config;
    let mut cfg = if o.demo { LearnConfig::demo() } else { LearnConfig::for_thresholds(settings.learn_thresholds) };
    cfg.cards = crate::autolearn::card_params(&settings, paths);
    let store = adage_learn::store::Files::new(&paths.tracks(), &paths.index());
    let (report, planned) = adage_learn::plan(mined, Some(&paths.models()), &store, &cfg)?;
    sense.thresholds = report.thresholds.clone();
    sense.intent_model = report.intent_model.clone();

    for (repo, a) in accs {
        let mut turns = a.turns.clone();
        turns.sort_unstable();
        let top = |m: &HashMap<String, (u32, u32)>| -> Vec<CommandCount> {
            let mut v: Vec<(&String, &(u32, u32))> = m.iter().collect();
            v.sort_by(|x, y| y.1.cmp(x.1).then(x.0.cmp(y.0)));
            v.into_iter()
                .take(TOP)
                .map(|(k, (s, r))| CommandCount { template: redacted(k), sessions: *s, runs: *r })
                .collect()
        };
        let mut failures: Vec<(&String, &FailCount)> =
            a.failures.iter().filter(|(_, (s, r, _))| *s >= 2 || *r > *s).collect();
        failures.sort_by(|x, y| (y.1 .0, y.1 .1).cmp(&(x.1 .0, x.1 .1)).then(x.0.cmp(y.0)));
        let repeated_failures = failures
            .into_iter()
            .take(TOP)
            .map(|(k, (s, r, next))| Failure {
                template: redacted(k),
                sessions: *s,
                runs: *r,
                then_worked: next.iter().max_by(|x, y| x.1.cmp(y.1).then(y.0.cmp(x.0))).map(|(k, _)| redacted(k)),
            })
            .collect();
        let mut regen: Vec<(&(String, String), &u32)> = a.regen.iter().filter(|(_, n)| **n >= 2).collect();
        regen.sort_by(|x, y| y.1.cmp(x.1).then(x.0.cmp(y.0)));
        let regeneration_after_edits = regen
            .into_iter()
            .take(TOP)
            .map(|((g, c), n)| Regeneration {
                edits: redacted(g),
                then_ran: redacted(c),
                sessions: *n,
                editing_sessions: a.editors.get(g).copied().unwrap_or(0),
            })
            .collect();
        let prompt_spans = a
            .spans
            .iter()
            .map(|(k, (n, shapes))| {
                let mut sh: Vec<(&String, &u32)> = shapes.iter().collect();
                sh.sort_by(|x, y| y.1.cmp(x.1).then(x.0.cmp(y.0)));
                SpanCount {
                    kind: k.to_string(),
                    sessions: *n,
                    shapes: sh.into_iter().take(3).map(|(s, _)| redacted(s)).collect(),
                }
            })
            .collect();
        let mut samples = a.samples.clone();
        samples.sort_by(|x, y| (&y.0, &y.1).cmp(&(&x.0, &x.1)));
        let candidates = planned
            .iter()
            .filter(|p| p.track.manifest.scope.repo == repo)
            .flat_map(|p| candidates(&store, p))
            .collect();
        sense.repos.push(RepoSense {
            repo: redacted(&repo),
            sessions: a.sessions,
            prompts: a.prompts,
            harnesses: a.harnesses.into_iter().collect(),
            first_day: a.days.0,
            last_day: a.days.1,
            steered: a.steered,
            turns_before_first_edit: Turns {
                median: percentile(&turns, 0.5),
                p75: percentile(&turns, 0.75),
                max: turns.last().copied(),
                sessions_without_edit: a.no_edit,
            },
            commands: top(&a.commands),
            repeated_failures,
            regeneration_after_edits,
            prompt_spans,
            sample_prompts: samples.into_iter().take(SAMPLES).map(|s| s.2).collect(),
            mined_sessions: report.repos.get(&repo).map_or(0, |r| r.sessions as u32),
            sampled: a.sampled,
            candidates,
            intent_families: report.repos.get(&repo).map_or(0, |r| r.intent_families),
            card_families: report.repos.get(&repo).map_or(0, |r| r.engine.card_families),
            cards: report.repos.get(&repo).map_or(0, |r| r.engine.cards),
        });
    }
    Ok(sense)
}

fn candidates(store: &dyn adage_learn::store::Store, p: &adage_learn::Planned) -> Vec<Candidate> {
    let installed = store.track(&p.name);
    let suffix = p.track.manifest.track.name.rsplit('/').next().unwrap_or_default().to_string();
    let row = |r: &Route, status: &str| Candidate {
        track: suffix.clone(),
        route: r.id.clone(),
        status: status.into(),
        strength: strength_str(r.strength).into(),
        effect: r.effect.as_str().into(),
        trigger: redacted(adage_app::graph::trigger(&r.trigger)["label"].as_str().unwrap_or_default()),
        step: redacted(&crate::share::describe_step(&r.step)),
        support: r.evidence.support,
        opportunities: r.evidence.opportunities,
        lower_bound: (r.evidence.lower_bound * 100.0).round() / 100.0,
        uplift: r.evidence.uplift.clone(),
    };
    let mut out = Vec::new();
    for r in p.track.routes.iter().filter(|r| {
        matches!(
            r.state,
            adage_tracks::RouteState::Firm | adage_tracks::RouteState::Forming | adage_tracks::RouteState::Nascent
        )
    }) {
        if let Some(prop) = p.proposals.routes.get(&r.id) {
            out.push(row(&prop.route, "proposed"));
            continue;
        }
        // Kept quiet by its uplift: agents already take its step unaided.
        if r.state == adage_tracks::RouteState::Nascent && r.evidence.uplift.as_ref().is_some_and(|u| u.gated) {
            out.push(row(r, "quiet"));
            continue;
        }
        // An environment step learn would build as nascent: never delivered until approved (O11).
        if r.state == adage_tracks::RouteState::Nascent {
            out.push(row(r, "needs approval"));
            continue;
        }
        let status = match installed.as_ref().and_then(|t| t.routes.iter().find(|x| x.id == r.id)) {
            None => "new",
            Some(x) if x == r => "installed",
            Some(_) => "update",
        };
        out.push(row(r, status));
    }
    out
}

pub fn sense(args: &[String], paths: &Paths) -> Result<()> {
    let o = Options::parse(args)?;
    if let Some(missing) = o.transcripts.iter().find(|p| !p.is_file()) {
        return Err(crate::hint(
            format!("{} is not a transcript file", missing.display()),
            "pass files that exist, or a directory with --claude-projects <dir> or --codex-sessions <dir>",
        ));
    }
    crate::check_dirs(&[
        ("--claude-projects", o.claude_projects.as_deref()),
        ("--codex-sessions", o.codex_sessions.as_deref()),
    ])?;
    let s = run(&o, paths)?;
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&s)?);
    } else {
        print_text(&s)?;
    }
    Ok(())
}

/// The report as text on standard output (also the last step of `adage init --print`).
pub fn print_text(s: &Sense) -> std::io::Result<()> {
    render(&mut adage_term::Out::stdout(), s)
}

/// `adage sense`'s report, in the output grammar (docs/design/style.md).
pub fn render<W: std::io::Write>(o: &mut adage_term::Out<W>, s: &Sense) -> std::io::Result<()> {
    use crate::words::count;
    use adage_term::{Glyph, Line, Role, Strength};
    let traces = count(s.sessions, "trace");
    let repos = count(s.repos.len(), "repository");
    o.header("sense", &[&traces, &repos, "read-only"])?;
    o.kv("source", Line::new().text(if s.source.is_empty() { "nowhere".to_string() } else { s.source.clone() }))?;
    if let Some(d) = &s.since {
        o.kv("window", Line::new().text(format!("since {d}")))?;
    }
    if s.repos.is_empty() {
        o.blank()?;
        o.note("No traces to sense in that source.")?;
        o.blank()?;
        return o.next("adage sense --transcripts <file>…", "or --claude-projects <dir>, --codex-sessions <dir>");
    }
    // Rows under a label: counts right-aligned in one column, then the text.
    let counted = |a: u32, b: u32, text: &str| {
        Line::new().pad_to(14).text(format!("{a:>4}")).faint(format!(" · {b:<5}")).text(text.to_string())
    };
    let mut any_candidates = false;
    for r in &s.repos {
        o.blank()?;
        let mut head = Line::new()
            .bold(Role::Ink, r.repo.clone())
            .sep()
            .quiet(count(r.sessions as usize, "trace"))
            .sep()
            .quiet(count(r.prompts as usize, "prompt"));
        if !r.first_day.is_empty() {
            head = head.sep().quiet(format!("{} … {}", r.first_day, r.last_day));
        }
        if !r.harnesses.is_empty() {
            head = head.sep().quiet(r.harnesses.join(", "));
        }
        o.line(&head)?;
        let kv = |label: &str| Line::new().text("  ").faint(label.to_string()).pad_to(14);
        if r.steered > 0 {
            o.line(&kv("steered").text(format!("by Adage in {} of them", r.steered)))?;
        }
        let t = &r.turns_before_first_edit;
        let mut l = kv("first edit");
        l = match (t.median, t.p75, t.max) {
            (Some(m), Some(p), Some(x)) => {
                l.text(format!("median {m} turns")).sep().quiet(format!("p75 {p}")).sep().quiet(format!("max {x}"))
            }
            _ => l.quiet("no trace edited anything"),
        };
        if t.sessions_without_edit > 0 {
            l = l.sep().quiet(format!("{} made no edit", count(t.sessions_without_edit as usize, "trace")));
        }
        o.line(&l)?;
        if !r.commands.is_empty() {
            o.line(&kv("commands").faint("traces · runs"))?;
            for c in &r.commands {
                o.line(&counted(c.sessions, c.runs, &c.template))?;
            }
        }
        if !r.prompt_spans.is_empty() {
            let mut l = kv("prompts name");
            for (i, p) in r.prompt_spans.iter().enumerate() {
                if i > 0 {
                    l = l.sep();
                }
                l = l.text(format!("a {} in {}", p.kind, count(p.sessions as usize, "trace")));
                if !p.shapes.is_empty() {
                    l = l.faint(format!(" ({})", p.shapes.join(", ")));
                }
            }
            o.line(&l)?;
        }
        if !r.repeated_failures.is_empty() {
            o.line(&kv("dead ends").faint("candidates · traces · runs"))?;
            for f in &r.repeated_failures {
                let mut l = counted(f.sessions, f.runs, &f.template);
                if let Some(n) = &f.then_worked {
                    l = l.faint("   then worked: ").push(Role::Command, n.clone());
                }
                o.line(&l)?;
            }
        }
        if !r.regeneration_after_edits.is_empty() {
            o.line(&kv("finish lines").faint("candidates"))?;
            for g in &r.regeneration_after_edits {
                o.line(&Line::new().pad_to(14).text(format!(
                    "{} of {} traces editing {} then ran {}",
                    g.sessions, g.editing_sessions, g.edits, g.then_ran
                )))?;
            }
        }
        if !r.sample_prompts.is_empty() {
            let quoted: Vec<String> = r.sample_prompts.iter().map(|p| format!("\"{p}\"")).collect();
            o.line(&kv("recent").quiet(quoted.join("  ")))?;
        }
        let sampled =
            if r.sampled { format!(" · from the {} most recent traces", r.mined_sessions) } else { String::new() };
        if r.candidates.is_empty() {
            o.line(
                &kv("would form")
                    .quiet(format!("nothing yet: no behaviour clears the bars ({}){sampled}", s.thresholds)),
            )?;
        } else {
            any_candidates = true;
            o.line(&kv("would form").quiet(format!("candidates only, nothing is written{sampled}")))?;
            let id_w = r
                .candidates
                .iter()
                .map(|c| c.track.chars().count() + c.route.chars().count() + 1)
                .max()
                .unwrap_or(20)
                .clamp(20, 44);
            for c in &r.candidates {
                let strength = Strength::parse(&c.strength).unwrap_or(Strength::Prime);
                o.line(
                    &Line::new()
                        .text("    ")
                        .strength(strength)
                        .pad_to(15)
                        .push(if c.status == "new" { Role::Accent } else { Role::Ink3 }, c.status.clone())
                        .pad_to(25)
                        .glyph(Role::Accent, Glyph::Reflex)
                        .text(" ")
                        .text(format!("{}/{}", c.track, c.route))
                        .pad_to(28 + id_w)
                        .text(format!("{}/{}", c.support, c.opportunities))
                        .faint(format!(" · lb {} · {}", crate::evidence::num(c.lower_bound), c.effect))
                        .faint(c.uplift.as_ref().map(|u| format!(" · uplift {:+.2}", u.mean)).unwrap_or_default()),
                )?;
                o.line(
                    &Line::new()
                        .pad_to(27)
                        .faint(format!("{} ", crate::words::prose(&c.trigger)))
                        .glyph(Role::Ink3, Glyph::Arrow)
                        .quiet(format!(" {}", crate::words::prose(&c.step))),
                )?;
                // Why a quiet (uplift-gated) candidate stays quiet.
                if let Some(u) = c.uplift.as_ref().filter(|u| u.gated) {
                    o.line(&Line::new().pad_to(27).quiet(u.line()))?;
                }
            }
        }
        match &s.intent_model {
            None => o.line(&kv("intent").push(Role::Prime, "skipped").sep().quiet("no embedding model"))?,
            Some(m) => o.line(
                &kv("intent").text(format!("{} usable", count(r.intent_families, "family"))).faint(format!(" ({m})")),
            )?,
        }
        if s.intent_model.is_some() && r.card_families > 0 {
            o.line(
                &kv("recurring")
                    .text(count(r.card_families, "task family"))
                    .sep()
                    .text(format!("{} with a procedure card", r.cards))
                    .faint(" (a reflex said once at a matching prompt)"),
            )?;
        }
    }
    o.blank()?;
    if any_candidates {
        o.next("adage condition", "form these reflexes")?;
        o.also("adage review", "then decide on each one")?;
    } else {
        o.next("adage condition", "nothing clears the bars yet; conditioning runs by itself as sessions end")?;
        o.also("adage condition --instincts-only", "bind Adage's instincts from your repository's files today")?;
    }
    if s.intent_model.is_none() {
        o.also("adage models fetch", "similar prompts can then share a reflex")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_snippets_and_trimming() {
        assert_eq!(shape("PROJ-1042"), "PROJ-####");
        let long = "word ".repeat(40);
        let s = snippet(&long);
        assert!(s.ends_with('…') && s.chars().count() == SNIPPET + 1);
        let mut out = format!("{}{}", "a".repeat(OUTPUT_HEAD + 10), "Error: tail");
        trim_output(&mut out);
        assert!(out.len() < OUTPUT_HEAD + OUTPUT_TAIL + 10 && out.ends_with("Error: tail"));
        assert_eq!(percentile(&[1, 2, 3, 4], 0.5), Some(3));
        assert_eq!(percentile(&[], 0.5), None);
    }

    #[test]
    fn memory_stays_bounded_and_keeps_the_most_recent_sessions() {
        let mut a = Acc::default();
        let n = MINED_PER_REPO * 2 + 5;
        for i in 0..n {
            a.add(Session {
                id: format!("s{i:05}"),
                root: PathBuf::from("/r"),
                repo: "r".into(),
                model: None,
                day: format!("2026-01-{:02}", 1 + i * 28 / n),
                prompt: "p".repeat(10_000),
                steps: vec![],
                steered: false,
                directed: BTreeSet::new(),
                influence: vec![],
                outcome: None,
                harness: "claude-code".into(),
                prompts: vec![],
                ending: Default::default(),
            });
            assert!(a.mined.len() < MINED_PER_REPO * 2 && a.samples.len() <= SAMPLES * 4);
        }
        a.keep_recent();
        assert_eq!((a.sessions as usize, a.mined.len(), a.sampled), (n, MINED_PER_REPO, true));
        assert!(a.mined.iter().all(|s| s.id.as_str() >= "s01005" && s.prompt.len() <= 4000));
    }
}
