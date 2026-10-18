//! Conditioning in `adage ui`, as three cards in one flow:
//!
//! 1. **Confirm**: the history sources it will read and what they hold, before anything runs.
//! 2. **Running**: its stages as a checklist that ticks as `adage condition --progress` reports
//!    them, with the active one spinning. It runs in the background: `esc` hides the card.
//! 3. **Result**: what it did, from `adage condition --json`. One headline sentence, a funnel from
//!    traces to reflexes, what changed (each reflex by what it tells the agent), the reflex sets,
//!    overlaps settled and notes, each in plain words. Thresholds, ids and the notes as written
//!    are under `v`; `enter` takes the next step.
//!
//! Every line is laid out to the card's width and clipped with an ellipsis, never wrapped
//! mid-phrase. Only the details, long by nature, wrap: at words, under their own column.

use super::app::{Action, AppState, Cmd, Modal, Tab};
use super::jobs::Job;
use super::keys;
use super::kit::{self, Tone};
use super::palette::Run as Go;
use super::theme::Theme;
use super::tools::Tool;
use crate::history::Preview;
use crate::words::count;
use adage_learn::progress::{Event, Stage};
use adage_term::Glyph;
use adage_tracks::interlock::{Conflict, ConflictKind, Resolution};
use adage_tracks::Strength;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;
use serde_json::Value;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------------------------
// The result, as `adage condition --json` reports it

/// One conditioning run's result.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub read: usize,
    pub used: usize,
    pub steered: usize,
    pub elapsed_ms: Option<u64>,
    pub repos: Vec<Repo>,
    /// Overlaps the compiled index settled (`adage_tracks::interlock`).
    pub overlaps: Vec<Conflict>,
    /// What the run wrote to stderr, as written (cleaned of terminal escapes).
    pub notes: Vec<String>,
    pub thresholds: String,
    pub intent_model: Option<String>,
    pub generation: String,
    /// What `adage review` has after the run: `(needs you, to look over)`.
    pub waiting: (usize, usize),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Repo {
    pub key: String,
    pub traces: usize,
    /// Patterns mined, compiled, and the reflexes that passed judgement.
    pub patterns: usize,
    pub compiled: usize,
    pub kept: usize,
    pub contexts: usize,
    /// Review-only declarations, separate from verified checks and active reflexes.
    pub declared_checks: usize,
    /// A bounded preview; the full evidence remains in the conditioning report.
    pub declared_check_leads: Vec<DeclaredCheckLead>,
    pub kept_by: Vec<(String, usize)>,
    pub sets: Vec<ReflexSet>,
    pub changes: Vec<Change>,
    pub instincts: Option<Instincts>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredCheckLead {
    pub name: String,
    pub traces: usize,
    pub groups: usize,
}

/// A reflex set (a track) as the run left it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflexSet {
    pub name: String,
    pub version: String,
    /// `unchanged`, `created`, `evidence updated`, `reflex added`, `reflex changed or removed`.
    pub change: String,
    pub reflexes: usize,
}

impl ReflexSet {
    pub fn instincts(&self) -> bool {
        self.name.rsplit('/').next() == Some(adage_learn::instincts::TRACK)
    }
}

/// One change to one reflex (`adage_learn::diff`).
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub route: String,
    pub track: String,
    /// `new`, `nascent`, `strength`, `extinguished`, `dormant`, `proposal`, `evidence`, `candidate`.
    pub kind: String,
    pub from: Option<Strength>,
    pub to: Option<Strength>,
    /// What the reflex tells the agent (`Route::gist`); empty when unknown.
    pub gist: String,
    pub summary: String,
}

impl Change {
    fn name(&self) -> String {
        if self.gist.is_empty() {
            self.route.clone()
        } else {
            super::words::list_label(&self.gist, " · ")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Instincts {
    pub bound: usize,
    pub advice: usize,
    pub nascent: usize,
    pub not_bound: Option<String>,
}

fn num(v: &Value, k: &str) -> usize {
    v[k].as_u64().unwrap_or(0) as usize
}

fn text(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

impl Card {
    /// The card for `adage condition --json`'s output and the notes it wrote; `None` when the
    /// output is not that report.
    pub fn parse(stdout: &str, notes: Vec<String>) -> Option<Card> {
        let v: Value = serde_json::from_str(stdout.trim()).ok()?;
        v.get("sessions_read")?.as_u64()?;
        let strength = |x: &Value| serde_json::from_value::<Strength>(x.clone()).ok();
        let repos = v["repos"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(key, r)| {
                let e = &r["engine"];
                let changes = v["changes"][key.as_str()]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| Change {
                        route: text(c, "route"),
                        track: text(c, "track"),
                        kind: text(c, "change"),
                        from: strength(&c["from"]),
                        to: strength(&c["to"]),
                        gist: text(c, "gist"),
                        summary: text(c, "summary"),
                    })
                    .collect();
                Repo {
                    key: key.clone(),
                    traces: num(r, "sessions"),
                    patterns: num(e, "reflexes"),
                    compiled: num(e, "compiled"),
                    kept: num(e, "routes"),
                    contexts: num(e, "contexts"),
                    declared_checks: e["procedures"]["declared_validators"].as_array().map_or(0, Vec::len),
                    declared_check_leads: e["procedures"]["declared_validators"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .take(6)
                        .map(|lead| DeclaredCheckLead {
                            name: crate::condition_out::declared_check_label(&text(lead, "name")),
                            traces: num(lead, "sessions"),
                            groups: num(lead, "evidence_groups"),
                        })
                        .collect(),
                    kept_by: e["kept_by"]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .map(|(k, n)| (k.clone(), n.as_u64().unwrap_or(0) as usize))
                        .collect(),
                    sets: r["tracks"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|t| ReflexSet {
                            name: text(t, "name"),
                            version: text(t, "version"),
                            change: text(t, "change"),
                            reflexes: t["reflexes"]
                                .as_u64()
                                .map_or_else(|| t["routes"].as_array().map_or(0, Vec::len), |n| n as usize),
                        })
                        .collect(),
                    changes,
                    instincts: r["instincts"].is_object().then(|| Instincts {
                        bound: num(&r["instincts"], "bound"),
                        advice: num(&r["instincts"], "advice"),
                        nascent: num(&r["instincts"], "nascent"),
                        not_bound: r["instincts"]["not_bound"].as_str().map(String::from),
                    }),
                }
            })
            .collect();
        Some(Card {
            read: num(&v, "sessions_read"),
            used: num(&v, "sessions_used"),
            steered: num(&v, "sessions_steered"),
            elapsed_ms: v["elapsed_ms"].as_u64(),
            repos,
            overlaps: serde_json::from_value(v["conflicts"].clone()).unwrap_or_default(),
            notes,
            thresholds: text(&v, "thresholds"),
            intent_model: v["intent_model"].as_str().map(String::from),
            generation: text(&v, "index_generation"),
            waiting: (num(&v["waiting"], "needs_you"), num(&v["waiting"], "to_look_over")),
        })
    }

    fn changes(&self) -> impl Iterator<Item = &Change> {
        self.repos.iter().flat_map(|r| r.changes.iter())
    }

    fn declared_checks(&self) -> usize {
        self.repos.iter().map(|r| r.declared_checks).sum()
    }

    fn sets(&self) -> impl Iterator<Item = (&Repo, &ReflexSet)> {
        self.repos.iter().flat_map(|r| r.sets.iter().map(move |s| (r, s)))
    }

    /// The reflexes this repository's sets hold now, learned and instincts.
    pub fn reflexes(&self) -> usize {
        self.sets().map(|(_, s)| s.reflexes).sum()
    }

    /// The headline, in parts: `Read 123 traces`, `kept 8 reflexes`, `2 new, 1 stronger`.
    pub fn headline(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if self.read == 0 {
            parts.push("No past traces here yet".to_string());
            if self.reflexes() > 0 {
                parts.push(format!("bound {} from its files", count(self.reflexes(), "instinct")));
            }
        } else {
            parts.push(format!("Read {}", count(self.read, "trace")));
            if self.repos.is_empty() || self.used == 0 {
                parts.push("none could be used".into());
            } else {
                parts.push(match self.reflexes() {
                    0 => "no reflexes yet".to_string(),
                    n => format!("kept {}", count(n, "reflex")),
                });
            }
        }
        if !self.repos.is_empty() {
            let changed = changed_words(self.changes().map(|c| (c.kind.as_str(), c.from, c.to)));
            parts.push(if self.declared_checks() > 0 && changed == "nothing changed" {
                "no active reflex changed".into()
            } else {
                changed
            });
        }
        if self.declared_checks() > 0 {
            parts.push(count(self.declared_checks(), "declared check lead"));
        }
        parts
    }

    /// The headline as one sentence.
    pub fn sentence(&self) -> String {
        self.headline().join(" · ")
    }

    /// The step `enter` takes: where to go, what it is called, and why.
    pub fn next(&self) -> (Go, &'static str, String) {
        let (needs, look) = self.waiting;
        // As `adage condition` ends: with nothing usable, what the sources hold comes first.
        if self.repos.is_empty() {
            (Go::Tool(Tool::HistorySources), "See the history sources", "which traces Adage can read".into())
        } else if let Some(why) = crate::review::waiting_words(needs, look) {
            (Go::Tab(Tab::Reflexes), "Review in Reflexes", why)
        } else {
            (Go::Tab(Tab::Reflexes), "Open Reflexes", "every reflex in effect".into())
        }
    }

    /// The notes, grouped and counted, without the one the headline already says.
    pub fn note_groups(&self) -> Vec<NoteGroup> {
        let mut groups = group_notes(&self.notes);
        if self.read == 0 {
            groups.retain(|g| !g.text.starts_with("no past traces found"));
        }
        groups
    }

    /// The card as plain lines (a job's text, where no card can be drawn).
    pub fn text_lines(&self) -> Vec<String> {
        let mut out = vec![self.sentence()];
        out.extend(
            self.changes().filter(|c| significant(&c.kind)).map(|c| format!("  {}: {}", badge_words(c), c.name())),
        );
        if self.declared_checks() > 0 {
            out.push("  Declared check results and applicability are unverified; not active reflexes.".into());
        }
        out.extend(self.note_groups().into_iter().map(|g| format!("  {}", g.text)));
        out
    }
}

/// Whether a change is one the card lists (evidence and candidates are counted instead).
fn significant(kind: &str) -> bool {
    !matches!(kind, "evidence" | "candidate")
}

/// prime 1, fire 2: an inhibit is a different polarity, not a step on this scale.
fn rank(a: Strength) -> Option<u8> {
    match a {
        Strength::Prime => Some(1),
        Strength::Fire => Some(2),
        Strength::Inhibit => None,
    }
}

/// What a run changed, in a phrase: `nothing changed`, `2 new, 1 stronger, 1 extinguished`.
/// Shared with the setup flow's conditioning step, which reads its changes from `learn.log`.
pub fn changed_words<'a>(changes: impl IntoIterator<Item = (&'a str, Option<Strength>, Option<Strength>)>) -> String {
    let mut n = [0usize; 9];
    for (kind, from, to) in changes {
        let i = match kind {
            "new" => 0,
            "nascent" | "draft" => 1,
            "strength" | "aspect" => match (from.and_then(rank), to.and_then(rank)) {
                (Some(a), Some(b)) if b > a => 2,
                (Some(a), Some(b)) if b < a => 3,
                _ => 4,
            },
            "extinguished" | "retired" => 5,
            "dormant" | "stale" => 6,
            "proposal" => 7,
            "evidence" => 8,
            _ => continue,
        };
        n[i] += 1;
    }
    let words = ["new", "to approve", "stronger", "weaker", "changed strength", "extinguished", "dormant"];
    let mut parts: Vec<String> = words.iter().zip(n).filter(|(_, k)| *k > 0).map(|(w, k)| format!("{k} {w}")).collect();
    if n[7] > 0 {
        parts.push(format!("{} proposed", count(n[7], "change")));
    }
    match (parts.is_empty(), n[8] > 0) {
        (false, _) => parts.join(", "),
        (true, true) => "same reflexes, fresher evidence".into(),
        (true, false) => "nothing changed".into(),
    }
}

// ---------------------------------------------------------------------------------------------
// Notes

/// Notes that say the same thing, counted: `1 reflex not learned: its trigger matches too many
/// different commands`.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteGroup {
    pub text: String,
    pub count: usize,
    /// A warning (something was left out or could not be done), not information.
    pub warn: bool,
}

/// What a reason says before its details: the text before ` (` or `: `.
fn gist_of(reason: &str) -> &str {
    let cut = [reason.find(" ("), reason.find(": ")].into_iter().flatten().min().unwrap_or(reason.len());
    &reason[..cut]
}

/// Group the notes a run wrote. The learner's `N reflexes not learned: <reason>` lines (and
/// `N instincts not bound: …`) sum by what their reason says before its details; any other line
/// counts the times it appears. `adage: ` and `warning` mark warnings.
pub fn group_notes(lines: &[String]) -> Vec<NoteGroup> {
    use std::sync::OnceLock;
    static LEFT_OUT: OnceLock<regex::Regex> = OnceLock::new();
    let left_out = LEFT_OUT.get_or_init(|| {
        regex::Regex::new(r"^(\d+) (reflex|reflexes|instinct|instincts) (not learned|not bound): (.*)$").expect("regex")
    });
    let mut groups: Vec<NoteGroup> = Vec::new();
    let mut add = |text: String, n: usize, warn: bool| match groups.iter_mut().find(|g| g.text == text) {
        Some(g) => g.count += n,
        None => groups.push(NoteGroup { text, count: n, warn }),
    };
    for line in lines {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let (l, mut warn) = match l.strip_prefix("adage: ") {
            Some(rest) => (rest, true),
            None => (l, false),
        };
        let l = match l.strip_prefix("warning") {
            Some(rest) if rest.starts_with(char::is_whitespace) => {
                warn = true;
                rest.trim_start()
            }
            _ => l,
        };
        match left_out.captures(l) {
            Some(c) => {
                let n: usize = c[1].parse().unwrap_or(1);
                let what = if c[2].starts_with("reflex") { "reflex" } else { "instinct" };
                add(format!("\u{0}{what}\u{0}{}\u{0}{}", &c[3], gist_of(&c[4])), n, true)
            }
            None => add(l.to_string(), 1, warn),
        }
    }
    // The counted kind reads with its total: `3 reflexes not learned: …`.
    for g in &mut groups {
        if let Some(rest) = g.text.strip_prefix('\u{0}') {
            let mut it = rest.splitn(3, '\u{0}');
            let (what, how, why) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
            g.text = format!("{} {how}: {why}", count(g.count, what));
        } else if g.count > 1 {
            g.text = format!("{} · {} times", g.text, g.count);
        }
    }
    groups
}

// ---------------------------------------------------------------------------------------------
// Progress

/// How far a run has got: when each stage started, and the last report.
#[derive(Debug, Clone, PartialEq)]
pub struct Stages {
    pub started: Instant,
    /// When each stage started, in [`Stage::ALL`]'s order.
    pub at: [Option<Instant>; 5],
    /// How far each stage that counts got: `(done, total)`.
    pub counts: [Option<(usize, usize)>; 5],
    pub last: Option<Event>,
}

fn index(s: Stage) -> usize {
    Stage::ALL.iter().position(|x| *x == s).unwrap_or(0)
}

impl Stages {
    pub fn new(started: Instant) -> Stages {
        Stages { started, at: [Some(started), None, None, None, None], counts: [None; 5], last: None }
    }

    /// A report at `now`. Stages never go back (a run over several repositories mines each), and
    /// a stage passed over between two frames counts as started when the later one was seen.
    pub fn record(&mut self, e: Event, now: Instant) {
        let i = index(e.progress);
        if self.last.is_some_and(|l| index(l.progress) > i) {
            return;
        }
        for at in self.at.iter_mut().take(i + 1) {
            at.get_or_insert(now);
        }
        if e.total > 0 {
            self.counts[i] = Some((e.done, e.total));
        }
        self.last = Some(e);
    }

    /// The stage running now.
    pub fn current(&self) -> usize {
        self.last.map_or(0, |e| index(e.progress))
    }
}

// ---------------------------------------------------------------------------------------------
// The flow

pub enum Flow {
    /// What it will read, before it runs.
    Confirm { sources: Box<Preview>, repo: String, scroll: u16 },
    /// Its stages as they run.
    Running { repo: String, stages: Box<Stages> },
    /// What it did. `revealed` is when the card first showed, while its sections still appear
    /// one after another (`None`: all of them at once).
    Result { card: Box<Card>, details: bool, scroll: u16, revealed: Option<Instant> },
}

/// How long the result's sections take to appear, one after another.
const REVEAL: Duration = Duration::from_millis(300);

impl Flow {
    pub fn confirm(sources: Preview, repo: String) -> Flow {
        Flow::Confirm { sources: Box::new(sources), repo, scroll: 0 }
    }

    /// The running card for `job`, from what it has reported so far.
    pub fn running(job: &Job, repo: String) -> Flow {
        Flow::Running { repo, stages: Box::new(job.stages.clone().unwrap_or_else(|| Stages::new(Instant::now()))) }
    }

    /// The result, revealed section by section unless `reveal` is false.
    pub fn result(card: Card, reveal: bool) -> Flow {
        Flow::Result { card: Box::new(card), details: false, scroll: 0, revealed: reveal.then(Instant::now) }
    }

    /// Follow `job` while it runs.
    pub fn follow(&mut self, job: &Job) {
        if let (Flow::Running { stages, .. }, Some(s)) = (self, &job.stages) {
            **stages = s.clone();
        }
    }

    /// How soon the next frame is wanted: sooner while sections are appearing.
    pub fn frame(&self, now: Instant) -> Option<Duration> {
        match self {
            Flow::Result { revealed: Some(at), .. } if now.duration_since(*at) < REVEAL => {
                Some(Duration::from_millis(40))
            }
            Flow::Running { .. } => Some(Duration::from_millis(100)),
            _ => None,
        }
    }
}

fn nudge(scroll: &mut u16, code: KeyCode) {
    match code {
        KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
        KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
        KeyCode::PageDown => *scroll = scroll.saturating_add(10),
        _ => {}
    }
}

/// A key while a conditioning card is open.
pub fn key(s: &mut AppState, mut flow: Box<Flow>, code: KeyCode) -> Option<Cmd> {
    match &mut *flow {
        Flow::Confirm { scroll, .. } => match code {
            KeyCode::Enter | KeyCode::Char('y') => return Some(Cmd::Do(Action::LearnNow)),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => return None,
            _ => nudge(scroll, code),
        },
        Flow::Running { .. } => {
            if matches!(code, KeyCode::Esc | KeyCode::Char('q')) {
                s.toast("Conditioning goes on in the background; its result opens when it is done", true);
                return None;
            }
        }
        Flow::Result { card, details, scroll, revealed } => match code {
            KeyCode::Esc | KeyCode::Char('q') => return None,
            KeyCode::Enter => return s.run_entry(card.next().0),
            KeyCode::Char('v') => {
                *details = !*details;
                *revealed = None;
                // Opened: start at the details; closed: back to the top.
                *scroll = if *details { main_lines(card, 80, &Theme::with_color(false)).len() as u16 } else { 0 };
            }
            _ => nudge(scroll, code),
        },
    }
    s.modal = Some(Modal::Conditioning(flow));
    None
}

// ---------------------------------------------------------------------------------------------
// Drawing: shared pieces

fn sp(s: impl Into<String>, st: Style) -> Span<'static> {
    Span::styled(s.into(), st)
}

fn width(spans: &[Span]) -> usize {
    kit::spans_width(spans)
}

/// `line` in at most `w` columns, cut with the theme's ellipsis (`kit::clip_spans`). Text is made
/// ASCII first where the terminal needs it, so what is measured is what is drawn.
fn clip(line: Line<'static>, w: usize, t: &Theme) -> Line<'static> {
    let spans: Vec<Span<'static>> = line.spans.into_iter().map(|s| Span::styled(t.text(&s.content), s.style)).collect();
    Line::from(kit::clip_spans(spans, w, t))
}

/// The first of `options` that fits in `w` columns, else the last, clipped.
fn first_fit(options: &[String], w: usize, t: &Theme) -> String {
    options.iter().find(|o| t.text(o).chars().count() <= w).cloned().unwrap_or_else(|| {
        let last = options.last().cloned().unwrap_or_default();
        t.fit(&t.text(&last), w)
    })
}

/// `● prime → ● fire`.
fn shift(a: Strength, b: Strength, t: &Theme) -> Vec<Span<'static>> {
    let mut v = kit::strength(a, t);
    v.push(sp(format!(" {} ", t.glyph(Glyph::Arrow)), t.dim()));
    v.extend(kit::strength(b, t));
    v
}

/// The card's main action as a button: its key in an accent chip, then what it does.
fn button(key: &str, label: &str, t: &Theme) -> Vec<Span<'static>> {
    vec![kit::chip(key, Tone::Accent, t), sp(format!(" {label}"), t.bold())]
}

/// The other keys, as keycaps (`kit::hints`).
fn hint(pairs: &[(&str, &str)], t: &Theme) -> Vec<Span<'static>> {
    kit::hints(pairs, t, usize::MAX)
}

fn rule(w: usize, t: &Theme) -> Line<'static> {
    Line::from(sp(t.ch("─", "-").repeat(w), t.rule()))
}

const LABEL: usize = 14;

/// A details row: `label` in its column, `value` wrapped at words under the value column.
fn detail(label: &str, value: &str, w: usize, t: &Theme) -> Vec<Line<'static>> {
    let indent = 2 + LABEL;
    kit::wrap(&t.text(value), w.saturating_sub(indent).max(8))
        .into_iter()
        .enumerate()
        .map(|(i, row)| {
            let head = if i == 0 { format!("  {}", t.pad(label, LABEL)) } else { " ".repeat(indent) };
            Line::from(vec![sp(head, t.dim()), sp(row, t.ink2())])
        })
        .collect()
}

/// A card's frame: its title, how far its body is scrolled, how wide it is.
struct Panel<'a> {
    title: &'a str,
    scroll: u16,
    width: u16,
}

/// A bordered card over `area` (`kit::Card`'s frame, in the accent as popups are): `body`
/// scrolls, `footer` stays at the bottom.
fn panel(f: &mut Frame, area: Rect, p: Panel, body: Vec<Line<'static>>, footer: Vec<Line<'static>>, t: &Theme) {
    let (w, scroll) = (p.width, p.scroll);
    let need = (body.len() + footer.len()) as u16 + 2;
    let h = need.min(area.height.saturating_sub(2).max(3));
    let r = super::view::centered(area, w, h);
    f.render_widget(Clear, r);
    let inner_h = h.saturating_sub(2);
    let foot = (footer.len() as u16).min(inner_h.saturating_sub(1));
    let room = inner_h.saturating_sub(foot);
    let hidden = (body.len() as u16).saturating_sub(room);
    let scroll = scroll.min(hidden);
    let mut block = kit::Card::new(p.title).tone(Tone::Accent).block(t);
    if hidden > 0 {
        let more = match (scroll > 0, scroll < hidden) {
            (true, true) => format!(" {} more{}j/k scroll ", t.arrows(), t.sep()),
            (false, _) => format!(" {} more{}j/k scroll ", t.ch("↓", "v"), t.sep()),
            (true, false) => format!(" {} more{}j/k scroll ", t.ch("↑", "^"), t.sep()),
        };
        block = block.title_bottom(Line::from(sp(more, t.dim())).right_aligned());
    }
    let inner = block.inner(r).inner(Margin { horizontal: 1, vertical: 0 });
    f.render_widget(block, r);
    let body_area = Rect { height: room, ..inner };
    let foot_area = Rect { y: inner.y + room, height: foot, ..inner };
    f.render_widget(Paragraph::new(body).scroll((scroll, 0)), body_area);
    f.render_widget(Paragraph::new(footer), foot_area);
}

/// A card's width in `area`: `cap` columns at most, and a margin each side.
fn card_width(area: Rect, cap: u16) -> u16 {
    area.width.saturating_sub(4).clamp(30.min(area.width), cap)
}

pub fn draw(f: &mut Frame, area: Rect, flow: &Flow, t: &Theme) {
    draw_at(f, area, flow, t, Instant::now());
}

/// [`draw`] at `now` (the spinner, the elapsed time, how far the reveal has got).
pub fn draw_at(f: &mut Frame, area: Rect, flow: &Flow, t: &Theme, now: Instant) {
    match flow {
        Flow::Confirm { sources, repo, scroll } => {
            let w = card_width(area, 84);
            let inner = w.saturating_sub(4) as usize;
            let (body, footer) = confirm_lines(sources, repo, inner, t);
            panel(f, area, Panel { title: "Condition this repository", scroll: *scroll, width: w }, body, footer, t);
        }
        Flow::Running { repo, stages } => {
            let w = card_width(area, 76);
            let inner = w.saturating_sub(4) as usize;
            let (body, footer) = running_lines(stages, inner, t, now);
            let title = if repo.is_empty() {
                "Conditioning".to_string()
            } else {
                format!("Conditioning {}", super::scope::short(repo))
            };
            panel(f, area, Panel { title: &title, scroll: 0, width: w }, body, footer, t);
        }
        Flow::Result { card, details, scroll, revealed } => {
            let w = card_width(area, 100);
            let inner = w.saturating_sub(4) as usize;
            let shown = revealed.map(|at| {
                let ms = now.saturating_duration_since(at).as_millis() as usize;
                1 + ms * SECTIONS / REVEAL.as_millis() as usize
            });
            let mut body = sections(card, inner, t, shown);
            if *details {
                body.push(Line::raw(""));
                body.extend(details_lines(card, inner, t));
            }
            let footer = result_footer(card, inner, t);
            let title = match card.repos.as_slice() {
                [] => "Conditioning done".to_string(),
                [r] => format!("Conditioned {}", super::scope::short(&r.key)),
                rs => format!("Conditioned {} repositories", rs.len()),
            };
            panel(f, area, Panel { title: &title, scroll: *scroll, width: w }, body, footer, t);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The confirm card

fn confirm_lines(p: &Preview, repo: &str, w: usize, t: &Theme) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let name = if repo.is_empty() { "this repository".to_string() } else { super::scope::short(repo) };
    let mut v = vec![
        Line::from(sp(
            first_fit(
                &[
                    format!("Adage reads your agents' past work in {name} and learns reflexes from it."),
                    "Adage reads your agents' past work here and learns reflexes from it.".into(),
                    "Learn reflexes from your agents' past work.".into(),
                ],
                w,
                t,
            ),
            t.ink2(),
        )),
        Line::raw(""),
        Line::from(sp("Sources", t.bold())),
    ];
    let nw = p.sources.iter().map(|s| s.name.len()).max().unwrap_or(8) + 2;
    for s in &p.sources {
        let (mark, style) = if s.found { (t.glyph(Glyph::Ok), t.green()) } else { (t.glyph(Glyph::Off), t.dim()) };
        let where_ = match &s.path {
            Some(path) if s.found => path.clone(),
            Some(path) => format!("{path} (folder not found)"),
            None => s.hint.to_string(),
        };
        let ink = if s.found { t.ink2() } else { t.dim() };
        v.push(Line::from(vec![
            sp(format!("  {mark} "), style),
            sp(t.pad(s.name, nw), if s.found { t.bold() } else { t.dim() }),
            sp(where_, ink),
        ]));
    }
    v.push(Line::raw(""));
    v.push(Line::from(vec![
        sp("Found ", t.bold()),
        sp(count(p.files, "trace file"), t.bold()),
        sp(if p.instincts_only { " · no non-empty history" } else { "" }, t.yellow()),
    ]));
    let parts = [
        (p.usable, "unsteered", t.green(), t.ch("█", "#")),
        (p.steered, "steered", t.yellow(), t.ch("▒", "+")),
        (p.unreadable, "empty or unreadable", t.dim(), t.ch("░", ".")),
    ];
    let total: usize = parts.iter().map(|x| x.0).sum();
    if total > 0 {
        let bw = w.saturating_sub(2).min(56);
        let mut cells: Vec<usize> =
            parts.iter().map(|x| if x.0 == 0 { 0 } else { (x.0 * bw / total).max(1) }).collect();
        // Rounding lands on the largest part, so the bar is always exactly as wide.
        let largest = (0..3).max_by_key(|&i| parts[i].0).unwrap_or(0);
        let sum: usize = cells.iter().sum();
        cells[largest] = (cells[largest] + bw).saturating_sub(sum);
        let mut bar = vec![sp("  ", Style::default())];
        for (c, x) in cells.iter().zip(&parts) {
            bar.push(sp(x.3.repeat(*c), x.2));
        }
        v.push(Line::from(bar));
        let mut legend = vec![sp("  ", Style::default())];
        for x in parts.iter().filter(|x| x.0 > 0) {
            if legend.len() > 1 {
                legend.push(sp("   ", Style::default()));
            }
            legend.push(sp(format!("{} ", x.3), x.2));
            legend.push(sp(format!("{} ", x.0), t.bold()));
            legend.push(sp(x.1, t.ink2()));
        }
        v.push(Line::from(legend));
    }
    for note in [crate::history::COVERAGE_NOTE, crate::history::INVENTORY_NOTE, crate::history::ELIGIBILITY_NOTE] {
        v.push(Line::from(sp(note, t.ink2())));
    }
    if p.steered > 0 {
        v.push(Line::from(sp(crate::history::STEERED_NOTE, t.ink2())));
    }
    if p.instincts_only {
        v.push(Line::from(sp("  Conditioning binds instincts from this repository's files instead.", t.ink2())));
    }
    v.push(Line::raw(""));
    v.push(Line::from(sp(
        first_fit(
            &[
                "Reads locally · never runs history commands · only this repository's traces".into(),
                "Reads locally · never runs history commands".into(),
            ],
            w,
            t,
        ),
        t.dim(),
    )));
    let body = v.into_iter().map(|l| clip(l, w, t)).collect();
    let mut keys = button("enter", if p.instincts_only { "Bind instincts" } else { "Condition" }, t);
    keys.push(sp("    ", Style::default()));
    keys.extend(hint(&[("esc", "Cancel")], t));
    let footer = vec![rule(w, t), clip(Line::from(keys), w, t)];
    (body, footer)
}

// ---------------------------------------------------------------------------------------------
// The running card

fn seconds(d: Duration) -> String {
    crate::ui::learnlog::duration(d.as_millis() as u64)
}

fn running_lines(st: &Stages, w: usize, t: &Theme, now: Instant) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let elapsed = now.saturating_duration_since(st.started);
    let head = first_fit(
        &[
            "Reading your agents' past work here and learning reflexes from it.".into(),
            "Learning reflexes from your agents' past work.".into(),
        ],
        w.saturating_sub(8),
        t,
    );
    let secs = format!("{} s", elapsed.as_secs());
    let gap = w.saturating_sub(head.chars().count() + secs.len()).max(1);
    let mut v = vec![
        Line::from(vec![sp(head, t.ink2()), sp(" ".repeat(gap), Style::default()), sp(secs, t.dim())]),
        Line::raw(""),
    ];
    let cur = st.current();
    let lw = Stage::ALL.iter().map(|s| s.label().len()).max().unwrap_or(18) + 2;
    let spin = kit::spinner(elapsed.as_millis(), t);
    for (i, stage) in Stage::ALL.iter().enumerate() {
        let mut l = vec![sp("  ", Style::default())];
        if i < cur {
            l.push(sp(format!("{} ", t.glyph(Glyph::Ok)), t.green()));
            l.push(sp(t.pad(stage.label(), lw), t.ink2()));
            if let Some((_, total)) = st.counts[i].filter(|_| *stage == Stage::Read) {
                l.push(sp(format!("{}{}", count(total, "trace"), t.sep()), t.ink2()));
            }
            if let (Some(a), Some(b)) = (st.at[i], st.at.get(i + 1).copied().flatten()) {
                l.push(sp(seconds(b.saturating_duration_since(a)), t.dim()));
            }
        } else if i == cur {
            l.push(sp(format!("{spin} "), t.accent().patch(t.bold())));
            l.push(sp(t.pad(stage.label(), lw), t.bold()));
            let since = st.at[i].map(|a| now.saturating_duration_since(a)).unwrap_or_default();
            match st.last.filter(|e| e.progress == *stage && e.total > 0) {
                Some(e) => {
                    let numbers = format!(" {} of {}", e.done, e.total);
                    let bw = w.saturating_sub(2 + 2 + lw + numbers.len()).min(28);
                    l.extend(kit::meter(e.done as f64 / e.total.max(1) as f64, bw, Tone::Accent, t));
                    l.push(sp(numbers, t.ink2()));
                }
                None => l.push(sp(seconds(since), t.dim())),
            }
        } else {
            l.push(sp(format!("{} ", t.glyph(Glyph::Off)), t.dim()));
            l.push(sp(stage.label(), t.dim()));
        }
        v.push(clip(Line::from(l), w, t));
    }
    let calm = first_fit(
        &[
            "You can keep using adage: this runs in the background, and the result opens when it is done.".into(),
            "You can keep using adage; this runs in the background.".into(),
        ],
        w,
        t,
    );
    v.push(Line::raw(""));
    v.push(Line::from(sp(calm, t.dim())));
    let footer = vec![rule(w, t), clip(Line::from(hint(&[("esc", "Hide")], t)), w, t)];
    (v, footer)
}

// ---------------------------------------------------------------------------------------------
// The result card

/// The sections a result card reveals in turn: headline, funnel, changes, reflex sets, overlaps,
/// notes.
const SECTIONS: usize = 6;

/// The card's lines before its details: every line one row, whatever the width.
fn main_lines(card: &Card, w: usize, t: &Theme) -> Vec<Line<'static>> {
    sections(card, w, t, None)
}

fn sections(card: &Card, w: usize, t: &Theme, shown: Option<usize>) -> Vec<Line<'static>> {
    let all: [Vec<Line<'static>>; SECTIONS] = [
        headline_lines(card, w, t),
        funnel_lines(card, w, t),
        change_lines(card, t),
        set_lines(card, w, t),
        overlap_lines(card, w, t),
        note_lines(card, t),
    ];
    let mut out: Vec<Line<'static>> = Vec::new();
    for (i, s) in all.into_iter().enumerate() {
        if s.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(Line::raw(""));
        }
        // Not yet revealed: the rows are kept (the card does not jump), blank.
        if shown.is_some_and(|n| i >= n) {
            out.extend(s.iter().map(|_| Line::raw("")));
        } else {
            out.extend(s.into_iter().map(|l| clip(l, w, t)));
        }
    }
    out
}

fn headline_lines(card: &Card, w: usize, t: &Theme) -> Vec<Line<'static>> {
    let mut parts = card.headline();
    let time = card.elapsed_ms.map(crate::ui::learnlog::duration).unwrap_or_default();
    // Too long for the card: what changed as a count (the rows below say which).
    let changes = card.changes().filter(|c| significant(&c.kind)).count();
    if t.text(&parts.join(" · ")).chars().count() > w && changes > 0 {
        if let Some(last) = parts.last_mut() {
            *last = count(changes, "change");
        }
    }
    let mut spans = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            spans.push(sp(t.sep(), t.dim()));
        }
        // The first part says what was read; the last, what changed: plain ink, the rest bold.
        let style = if i + 1 == parts.len() && parts.len() > 1 { t.ink2() } else { t.bold() };
        spans.push(sp(p.clone(), style));
    }
    let used = width(&spans) + t.text(&time).chars().count();
    if !time.is_empty() && used < w {
        spans.push(sp(" ".repeat(w - used), Style::default()));
        spans.push(sp(time, t.dim()));
    }
    vec![Line::from(spans)]
}

/// Traces read › used › patterns found › compiled › reflexes learned, as centred bars: a funnel.
fn funnel_lines(card: &Card, w: usize, t: &Theme) -> Vec<Line<'static>> {
    if card.read == 0 {
        return vec![Line::from(sp(
            "Nothing to learn from yet: reflexes form as your agents work here, and instincts bind from its files.",
            t.ink2(),
        ))];
    }
    if card.repos.is_empty() || card.used == 0 {
        return vec![Line::from(sp(
            "None of these traces could be used: they hold no steps Adage can learn from.",
            t.ink2(),
        ))];
    }
    let sum = |f: fn(&Repo) -> usize| card.repos.iter().map(f).sum::<usize>();
    let learned: usize = card.sets().filter(|(_, s)| !s.instincts()).map(|(_, s)| s.reflexes).sum();
    let stages = [
        (card.read, "traces read"),
        (card.used, "used"),
        (sum(|r| r.patterns), "patterns found"),
        (sum(|r| r.compiled), "compiled"),
        (learned, if learned == 1 { "reflex learned" } else { "reflexes learned" }),
    ];
    let max = stages.iter().map(|s| s.0).max().unwrap_or(1).max(1);
    let cw = stages.iter().map(|s| s.0.to_string().len()).max().unwrap_or(1);
    let lw = stages.iter().map(|s| s.1.len()).max().unwrap_or(8);
    let bw = w.saturating_sub(2 + 2 + cw + 1 + lw).clamp(4, 44);
    let block = t.ch("█", "#");
    stages
        .iter()
        .enumerate()
        .map(|(i, (n, label))| {
            let cells = if *n == 0 { 0 } else { ((*n * bw + max / 2) / max).clamp(1, bw) };
            let left = (bw - cells) / 2;
            let last = i + 1 == stages.len();
            Line::from(vec![
                sp(" ".repeat(2 + left), Style::default()),
                sp(block.repeat(cells), if last { t.green() } else { t.accent() }),
                sp(" ".repeat(bw - cells - left + 2), Style::default()),
                sp(format!("{n:>cw$}"), t.bold()),
                sp(format!(" {label}"), if last { t.bold() } else { t.ink2() }),
            ])
        })
        .collect()
}

/// A change's badge, as words (for text).
fn badge_words(c: &Change) -> String {
    let w = |a: Option<Strength>| a.map(Theme::strength_word).unwrap_or("?");
    match c.kind.as_str() {
        "new" => format!("new {}", w(c.to)),
        "nascent" | "draft" => "to approve".into(),
        "strength" | "aspect" => format!("{} → {}", w(c.from), w(c.to)),
        "extinguished" | "retired" => "extinguished".into(),
        "dormant" | "stale" => "dormant".into(),
        "proposal" => "change proposed".into(),
        other => other.to_string(),
    }
}

/// A change's badge, drawn: the strength in its colour.
fn badge(c: &Change, t: &Theme) -> Vec<Span<'static>> {
    match (c.kind.as_str(), c.from, c.to) {
        ("new", _, Some(b)) => {
            let mut v = vec![sp("new ", t.accent().patch(t.bold()))];
            v.extend(kit::strength(b, t));
            v
        }
        ("strength" | "aspect", Some(a), Some(b)) => shift(a, b, t),
        ("nascent" | "draft", _, _) => {
            vec![sp(format!("{} ", t.glyph(Glyph::Off)), t.yellow()), sp("to approve", t.yellow())]
        }
        ("extinguished" | "retired", _, _) => {
            vec![sp(format!("{} ", t.glyph(Glyph::Fail)), t.red()), sp("extinguished", t.dim())]
        }
        ("dormant" | "stale", _, _) => vec![sp(format!("{} ", t.glyph(Glyph::Quiet)), t.dim()), sp("dormant", t.dim())],
        ("proposal", _, _) => {
            vec![sp(format!("{} ", t.glyph(Glyph::Notice)), t.accent()), sp("change proposed", t.accent())]
        }
        _ => vec![sp(badge_words(c), t.ink2())],
    }
}

/// The order changes are listed in: what arrived, what needs a person, what moved, what left.
fn order(kind: &str) -> usize {
    ["new", "nascent", "strength", "proposal", "dormant", "extinguished"].iter().position(|k| *k == kind).unwrap_or(6)
}

/// At most this many changes, overlaps and notes before `v` shows the rest.
const ROWS: usize = 6;

fn more_line(n: usize, what: &str, t: &Theme) -> Line<'static> {
    Line::from(vec![sp(format!("  and {n} more {what} · "), t.dim()), sp("v", t.key()), sp(" shows them", t.dim())])
}

fn change_lines(card: &Card, t: &Theme) -> Vec<Line<'static>> {
    if card.repos.is_empty() {
        return vec![];
    }
    let mut listed: Vec<&Change> = card.changes().filter(|c| significant(&c.kind)).collect();
    listed.sort_by_key(|c| order(&c.kind));
    let evidence = card.changes().filter(|c| c.kind == "evidence").count();
    let candidates = card.changes().filter(|c| c.kind == "candidate").count();
    let mut v = Vec::new();
    if listed.is_empty() {
        v.push(Line::from(sp(
            if evidence > 0 {
                format!("No reflex changed strength; {} saw new traces.", count(evidence, "reflex"))
            } else if card.declared_checks() > 0 {
                "No active reflex changed.".to_string()
            } else {
                "Nothing changed: every reflex kept its strength.".to_string()
            },
            t.ink2(),
        )));
    } else {
        v.push(Line::from(sp("What changed", t.bold())));
        let badges: Vec<Vec<Span<'static>>> = listed.iter().map(|c| badge(c, t)).collect();
        let bw = badges.iter().map(|b| width(b)).max().unwrap_or(0).min(20);
        for (c, b) in listed.iter().zip(badges).take(ROWS) {
            let pad = bw.saturating_sub(width(&b)) + 2;
            let mut l = vec![sp("  ", Style::default())];
            l.extend(b);
            l.push(sp(" ".repeat(pad), Style::default()));
            l.push(sp(c.name(), Style::default()));
            v.push(Line::from(l));
        }
        if listed.len() > ROWS {
            v.push(more_line(listed.len() - ROWS, "changes", t));
        }
        if evidence > 0 {
            let what = if evidence == 1 { "reflex" } else { "reflexes" };
            v.push(Line::from(sp(format!("  {evidence} more {what} saw new traces"), t.dim())));
        }
    }
    if candidates > 0 {
        v.push(Line::from(sp(
            format!("  {} forming: seen, but not yet in enough traces", count(candidates, "candidate")),
            t.dim(),
        )));
    }
    if card.declared_checks() > 0 {
        v.push(Line::from(sp(
            format!("  {} · not active reflexes", count(card.declared_checks(), "declared check lead")),
            t.ink2(),
        )));
        v.push(Line::from(sp("  Results and applicability unverified · v shows the leads", t.dim())));
    }
    v
}

/// A reflex set in plain words: `Learned here`, `Instincts`, a pack's name.
fn set_title(r: &Repo, s: &ReflexSet, many: bool) -> String {
    if s.instincts() {
        return if many { format!("Instincts · {}", super::scope::short(&r.key)) } else { "Instincts".into() };
    }
    super::scope::collection(&s.name, &r.key, false, !many)
}

fn set_lines(card: &Card, w: usize, t: &Theme) -> Vec<Line<'static>> {
    let many = card.repos.len() > 1;
    let rows: Vec<(String, &Repo, &ReflexSet)> =
        card.sets().map(|(r, s)| (format!("{} · v{}", set_title(r, s, many), s.version), r, s)).collect();
    let nw = rows.iter().map(|x| t.text(&x.0).chars().count()).max().unwrap_or(10).min(w / 2) + 3;
    // The glyphs' column is as wide as the wider one (`~` and `(o)` in ASCII).
    let gw = [Glyph::Reflex, Glyph::Instinct].iter().map(|g| t.glyph(*g).chars().count()).max().unwrap_or(1) + 1;
    rows.into_iter()
        .map(|(title, r, s)| {
            let glyph = if s.instincts() { Glyph::Instinct } else { Glyph::Reflex };
            let (state, style) = match s.change.as_str() {
                "unchanged" => ("unchanged", t.dim()),
                "created" => ("new", t.green()),
                "evidence updated" => ("fresher evidence", t.ink2()),
                "reflex added" => ("reflexes added", t.accent()),
                _ => ("reflexes changed", t.accent()),
            };
            let mut l = vec![
                sp(t.pad(t.glyph(glyph), gw), t.accent()),
                sp(t.pad(&title, nw), t.bold()),
                sp(t.pad(&count(s.reflexes, "reflex"), 13), Style::default()),
                sp(state, style),
            ];
            if let Some(i) = r.instincts.as_ref().filter(|_| s.instincts()) {
                // The longer words where they fit, else the count.
                let room = w.saturating_sub(width(&l) + t.sep().chars().count());
                let words = instincts_words(i);
                l.push(sp(t.sep(), t.dim()));
                l.push(sp(first_fit(&words, room, t), t.ink2()));
            }
            Line::from(l)
        })
        .collect()
}

/// What binding instincts left in use, longest first: `2 in use as advice`, `1 step to run,
/// 2 advice in use`, then `3 in use`.
fn instincts_words(i: &Instincts) -> Vec<String> {
    if let Some(why) = &i.not_bound {
        return vec![format!("none bound: {}", crate::words::prose(why)), "none bound".into()];
    }
    let needs = match i.nascent {
        0 => String::new(),
        1 => " · 1 needs you".into(),
        n => format!(" · {n} need you"),
    };
    let long = match (i.bound, i.advice) {
        (0, 0) => "none in use".to_string(),
        (0, a) => format!("{a} in use as advice"),
        (b, 0) => format!("{} in use", count(b, "step to run")),
        (b, a) => format!("{}, {a} advice in use", count(b, "step to run")),
    };
    vec![format!("{long}{needs}"), format!("{} in use{needs}", i.bound + i.advice)]
}

/// What an overlap did to the reflex that gave way, as a badge and the words after its command.
fn overlap_badge(c: &Conflict, t: &Theme) -> (Vec<Span<'static>>, &'static str) {
    match (c.kind, c.resolution) {
        (ConflictKind::TwoFires, Resolution::Weakened) => (shift(Strength::Fire, Strength::Prime, t), ""),
        (ConflictKind::Polarity, Resolution::Suppressed) => {
            (vec![sp(format!("{} left out", t.glyph(Glyph::Fail)), t.red())], "a warning rules it out")
        }
        (ConflictKind::Polarity, Resolution::RuntimeFilter) => (
            vec![sp(format!("{} held back", t.glyph(Glyph::Off)), t.yellow())],
            "never right after a warning against it",
        ),
        (ConflictKind::Instinct, _) => {
            (vec![sp(format!("{} set aside", t.glyph(Glyph::Off)), t.dim())], "a reflex learned here comes first")
        }
        _ => (vec![sp(format!("{} gave way", t.glyph(Glyph::Off)), t.dim())], "another reflex comes first"),
    }
}

fn overlap_lines(card: &Card, w: usize, t: &Theme) -> Vec<Line<'static>> {
    let n = card.overlaps.len();
    if n == 0 {
        return vec![];
    }
    let head = count(n, "overlap");
    let two_fires =
        card.overlaps.iter().all(|c| c.kind == ConflictKind::TwoFires && c.resolution == Resolution::Weakened);
    let these = if n == 1 { "it now suggests" } else { "these now suggest" };
    let options: Vec<String> = if two_fires {
        vec![
            format!("{head} settled: {these} instead of firing, so one reflex fires at a time"),
            format!("{head} settled: {these} instead of firing"),
            format!("{head} settled"),
        ]
    } else {
        vec![format!("{head} settled, so reflexes never contradict each other"), format!("{head} settled")]
    };
    let mut v = vec![Line::from(sp(first_fit(&options, w, t), t.bold()))];
    let badges: Vec<(Vec<Span<'static>>, &str)> = card.overlaps.iter().map(|c| overlap_badge(c, t)).collect();
    let bw = badges.iter().map(|b| width(&b.0)).max().unwrap_or(0);
    for (c, (b, why)) in card.overlaps.iter().zip(badges).take(ROWS) {
        let pad = bw.saturating_sub(width(&b)) + 2;
        let mut l = vec![sp("  ", Style::default())];
        l.extend(b);
        l.push(sp(" ".repeat(pad), Style::default()));
        l.push(sp(c.command.clone(), Style::default()));
        if !why.is_empty() {
            l.push(sp(format!("{}{why}", t.sep()), t.dim()));
        }
        v.push(Line::from(l));
    }
    if n > ROWS {
        v.push(more_line(n - ROWS, "overlaps", t));
    }
    v
}

fn note_lines(card: &Card, t: &Theme) -> Vec<Line<'static>> {
    let groups = card.note_groups();
    let mut v: Vec<Line<'static>> = groups
        .iter()
        .take(3)
        .map(|g| {
            let (mark, style) = if g.warn { ("!", t.yellow().patch(t.bold())) } else { (t.glyph(Glyph::Sep), t.dim()) };
            Line::from(vec![
                sp(format!("{mark} "), style),
                sp(g.text.clone(), if g.warn { Style::default() } else { t.ink2() }),
            ])
        })
        .collect();
    if groups.len() > 3 {
        v.push(more_line(groups.len() - 3, "notes", t));
    }
    v
}

fn result_footer(card: &Card, w: usize, t: &Theme) -> Vec<Line<'static>> {
    let (_, label, why) = card.next();
    let mut next = button("enter", label, t);
    next.push(sp(format!("{}{why}", t.sep()), t.dim()));
    // The other keys, from the keymap (enter is the button above).
    let pairs: Vec<(&str, &str)> = keys::CONDITIONED.iter().filter(|k| k.bar).map(|k| (k.keys, k.label)).collect();
    vec![rule(w, t), clip(Line::from(next), w, t), Line::from(kit::hints(&pairs, t, w))]
}

fn details_lines(card: &Card, w: usize, t: &Theme) -> Vec<Line<'static>> {
    let mut v = vec![Line::from(sp("Details", t.bold()))];
    if !card.thresholds.is_empty() {
        v.extend(detail("thresholds", &crate::condition_out::thresholds(&card.thresholds), w, t));
    }
    v.extend(detail(
        "intent model",
        card.intent_model.as_deref().unwrap_or("none: similar prompts cannot share a reflex (adage models fetch)"),
        w,
        t,
    ));
    if !card.generation.is_empty() {
        v.extend(detail("index", &format!("generation {}", &card.generation[..card.generation.len().min(8)]), w, t));
    }
    if card.steered > 0 {
        v.extend(detail(
            "steered",
            &format!(
                "{} carried Adage's directives: what those named is not their evidence",
                count(card.steered, "trace")
            ),
            w,
            t,
        ));
    }
    for r in &card.repos {
        let mut learning = format!(
            "{} · {} → {} compiled · {} kept",
            count(r.traces, "trace"),
            count(r.patterns, "pattern"),
            r.compiled,
            count(r.kept, "reflex")
        );
        let from: Vec<String> = r.kept_by.iter().map(|(k, n)| crate::condition_out::kept_from(k, *n)).collect();
        if !from.is_empty() && r.kept_by.iter().map(|x| x.1).sum::<usize>() == r.kept {
            learning.push_str(&format!(": {}", from.join(", ")));
        }
        if r.contexts > 0 {
            learning.push_str(&format!(" · {} with a context trigger", r.contexts));
        }
        let label = if card.repos.len() > 1 { super::scope::short(&r.key) } else { "learning".into() };
        v.extend(detail(&label, &learning, w, t));
        if r.declared_checks > 0 {
            v.extend(detail(
                "declared checks",
                &format!(
                    "{}; results and applicability unverified, not active reflexes",
                    count(r.declared_checks, "declared check lead")
                ),
                w,
                t,
            ));
            for lead in &r.declared_check_leads {
                v.extend(detail(
                    "",
                    &format!(
                        "{} · {}, {}",
                        lead.name,
                        count(lead.traces, "trace"),
                        count(lead.groups, "evidence group")
                    ),
                    w,
                    t,
                ));
            }
            if r.declared_checks > r.declared_check_leads.len() {
                v.extend(detail(
                    "",
                    &format!(
                        "{} in the JSON report",
                        count(r.declared_checks - r.declared_check_leads.len(), "additional lead")
                    ),
                    w,
                    t,
                ));
            }
        }
    }
    for (i, (_, s)) in card.sets().enumerate() {
        let value = format!("{}@{} · {}", s.name, s.version, crate::words::prose(&s.change));
        v.extend(detail(if i == 0 { "reflex sets" } else { "" }, &value, w, t));
    }
    for (i, c) in card.changes().enumerate() {
        let value = format!("{} in {}: {}", c.route, c.track, crate::words::prose(&c.summary));
        v.extend(detail(if i == 0 { "changes" } else { "" }, &value, w, t));
    }
    for (i, c) in card.overlaps.iter().enumerate() {
        let said = c.describe();
        // One repository: it need not be named on every line.
        let said = match card.repos.as_slice() {
            [r] => said.strip_prefix(&format!("{}: ", r.key)).unwrap_or(&said).to_string(),
            _ => said,
        };
        v.extend(detail(if i == 0 { "overlaps" } else { "" }, &crate::words::prose(&said), w, t));
    }
    for (i, n) in card.notes.iter().filter(|n| !n.trim().is_empty()).enumerate() {
        v.extend(detail(if i == 0 { "notes" } else { "" }, n, w, t));
    }
    v
}

/// A preview of what conditioning reads, for tests of the cards that start from it.
#[cfg(test)]
pub(super) fn sample_sources(usable: usize) -> Preview {
    use crate::history::Source;
    Preview {
        sources: vec![
            Source { name: "Claude Code", path: Some("~/.claude/projects".into()), found: true, hint: "" },
            Source { name: "Codex", path: None, found: false, hint: "connect hooks or choose a history folder" },
            Source {
                name: "OpenCode",
                path: None,
                found: false,
                hint: "import local history or choose an exports folder",
            },
        ],
        files: usable + 145,
        usable,
        steered: 6,
        unreadable: 139,
        instincts_only: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::watch::buffer_text;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use serde_json::json;

    const ADAGE: &str = "github.com/stevenvaughanbooth/adage";
    const NOTE: &str = "adage: 1 reflex not learned: its trigger matches too many different commands (20; the limit is \
        16): claude plugin test {path}, claude plugin validate --strict {path}, claude -p --help and 17 more (run-ce9cdb)";

    /// The run the screenshot showed: nothing changed, three overlaps settled, one note.
    fn unchanged() -> Card {
        let track = |name: &str, version: &str, n: usize| json!({ "name": format!("github-com-stevenvaughanbooth-adage/{name}"), "version": version, "change": "unchanged", "routes": [], "reflexes": n });
        let fires = |winner: &str, loser: &str, command: &str| json!({ "repo": ADAGE, "kind": "two-fires", "winner": winner, "loser": loser, "command": command, "resolution": "weakened" });
        let report = json!({
            "sessions_read": 122, "sessions_used": 103, "sessions_duplicates": 0, "sessions_steered": 6,
            "repos": { ADAGE: {
                "sessions": 103, "intent_families": 1,
                "tracks": [track("learned", "17.0.12", 5), track("instincts", "3.0.0", 2)],
                "instincts": { "bound": 0, "advice": 2, "nascent": 0, "skipped": [], "extinguished": [], "confirmed": [] },
                "engine": { "sessions": 103, "reflexes": 39, "compiled": 9, "routes": 6, "kept_by": { "mined pattern": 6 }, "contexts": 2 }
            }},
            "index_generation": "b44620fe0a1c55d2", "thresholds": "min_support=5 fire_lower_bound=0.8 prime_rate=0.5",
            "intent_model": "potion-base-8m@f65d0f325faa",
            "conflicts": [
                fires("run-7c62ab", "finish-line-0ba99a", "cargo fmt --all"),
                fires("run-7c62ab", "run-137805", "cargo build -p adage-cli"),
                fires("run-7c62ab", "run-4c6db8", "cargo test -p {part}"),
            ],
            "elapsed_ms": 9440, "changes": { ADAGE: [] }, "waiting": { "needs_you": 0, "to_look_over": 7 }
        });
        Card::parse(&report.to_string(), vec![NOTE.into()]).expect("a report")
    }

    /// A run that changed things: one new, one stronger, one to approve, one extinguished, and
    /// reflexes and candidates that only saw new traces.
    fn changed() -> Card {
        let learned = "github-com-stevenvaughanbooth-adage/learned";
        let change = |route: &str, kind: &str, from: Value, to: Value, gist: &str, summary: &str| json!({ "route": route, "track": learned, "change": kind, "from": from, "to": to, "summary": summary, "gist": gist });
        let report = json!({
            "sessions_read": 125, "sessions_used": 106, "sessions_duplicates": 0, "sessions_steered": 6,
            "repos": { ADAGE: {
                "sessions": 106, "intent_families": 1,
                "tracks": [
                    { "name": learned, "version": "18.0.0", "change": "reflex changed or removed", "routes": [], "reflexes": 6 },
                    { "name": "github-com-stevenvaughanbooth-adage/instincts", "version": "3.0.0", "change": "unchanged", "routes": [], "reflexes": 2 }
                ],
                "instincts": { "bound": 1, "advice": 1, "nascent": 0, "skipped": [], "extinguished": [], "confirmed": [] },
                "engine": { "sessions": 106, "reflexes": 37, "compiled": 10, "routes": 6, "kept_by": { "mined pattern": 5, "prompt intent": 1 }, "contexts": 1 }
            }},
            "index_generation": "514b9f0bac3afbc6", "thresholds": "min_support=5 fire_lower_bound=0.8 prime_rate=0.5",
            "intent_model": "potion-base-8m-i8@b9c0e329a2ce",
            "elapsed_ms": 12_310,
            "changes": { ADAGE: [
                change("run-1c7d66", "extinguished", json!("prime"), Value::Null,
                    "after editing crates/**/*.rs, then cargo build -p adage-cli, run cargo build -p adage-cli", "prime reflex extinguished"),
                change("run-0f3a91", "new", Value::Null, json!("fire"), "after editing crates/**/*.rs, run cargo fmt --all",
                    "new fire reflex (24 of 26 sessions, lower bound 0.81)"),
                change("run-7c62ab", "strength", json!("prime"), json!("fire"),
                    "after editing crates/**/*.rs, run cargo clippy --workspace --all-targets", "prime → fire (31 of 33 sessions, lower bound 0.82)"),
                change("intent-a897d5", "nascent", Value::Null, json!("prime"), "on requests like these, run cargo test -p adage-cli",
                    "nascent prime reflex: waits for review"),
                change("run-137805", "evidence", Value::Null, Value::Null, "after editing crates/adage-cli/src/*.rs, run cargo build -p adage-cli",
                    "prime reflex evidence 22/35 → 24/37 traces"),
                change("run-4c6db8", "evidence", Value::Null, Value::Null, "after editing crates/**/*.rs, run cargo test -p {part}",
                    "prime reflex evidence 23/39 → 24/41 traces"),
                change("cand-77a0b1", "candidate", Value::Null, Value::Null, "", "candidate seen in 3 of 5 traces"),
            ]},
            "waiting": { "needs_you": 1, "to_look_over": 3 }
        });
        Card::parse(&report.to_string(), vec![]).expect("a report")
    }

    fn result(card: Card) -> Flow {
        Flow::result(card, false)
    }

    /// The card as `adage ui` draws it over a `w`×`h` screen (its body: the screen less the tab
    /// bar, the status bar and their rules).
    fn frame(flow: &Flow, w: u16, h: u16, t: &Theme, now: Instant) -> (String, ratatui::buffer::Buffer) {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| {
            let body = Rect { x: 0, y: 2, width: w, height: h - 4 };
            draw_at(f, body, flow, t, now);
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        (buffer_text(&buf), buf)
    }

    fn plain() -> Theme {
        Theme::with_color(false)
    }

    #[test]
    fn declaration_only_learning_is_visible_without_active_reflex_or_approval_counts() {
        let leads: Vec<Value> = (0..7)
            .map(|i| {
                json!({
                    "name": format!("check-{i}\u{1b}[2J"), "sessions": 8, "evidence_groups": 1
                })
            })
            .collect();
        let mut report = json!({
            "sessions_read": 8, "sessions_used": 8,
            "repos": { "acme": { "sessions": 8, "tracks": [], "engine": {
                "routes": 0, "procedures": { "declared_validators": leads }
            }}},
            "changes": { "acme": [] }, "waiting": { "needs_you": 0, "to_look_over": 0 }
        });
        let card = Card::parse(&report.to_string(), vec![]).unwrap();
        assert_eq!(card.declared_checks(), 7);
        assert_eq!(card.repos[0].declared_check_leads.len(), 6);
        assert_eq!((card.reflexes(), card.waiting), (0, (0, 0)));
        assert_eq!(card.next().1, "Open Reflexes");
        let text = card.text_lines().join("\n");
        assert!(text.contains("7 declared check leads") && text.contains("no active reflex changed"), "{text}");
        assert!(text.contains("results and applicability are unverified; not active reflexes"), "{text}");
        assert!(!text.contains("nothing changed") && !text.contains("approve"), "{text}");
        let lines = |lines: Vec<Line<'static>>| lines.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
        let main = lines(change_lines(&card, &plain()));
        assert!(
            main.contains("7 declared check leads") && main.contains("Results and applicability unverified"),
            "{main}"
        );
        let details = lines(details_lines(&card, 160, &plain()));
        assert!(details.contains("check-5") && !details.contains("check-6"), "{details}");
        assert!(details.contains("8 traces, 1 evidence group") && !details.contains('\u{1b}'), "{details}");
        report["repos"]["acme"]["engine"]["procedures"].as_object_mut().unwrap().remove("declared_validators");
        let old = Card::parse(&report.to_string(), vec![]).unwrap();
        assert_eq!(old.declared_checks(), 0, "older reports remain readable");
        assert!(old.sentence().contains("nothing changed"));
    }

    #[test]
    fn the_result_card_reads_in_plain_words_at_every_width() {
        let now = Instant::now();
        for (name, card) in [("unchanged", unchanged()), ("changes", changed())] {
            for (w, h) in [(80, 30), (120, 40)] {
                let (text, _) = frame(&result(card.clone()), w, h, &plain(), now);
                insta::assert_snapshot!(format!("conditioned-{name}-{w}x{h}"), text);
                // No ids or slugs on the first read, and nothing the old wall of text said.
                for hidden in ["run-7c62ab", "finish-line-0ba99a", "github-com-", "gen ", "thresholds", "run-ce9cdb"] {
                    assert!(!text.contains(hidden), "{hidden} belongs under details:\n{text}");
                }
            }
        }
        let (text, _) = frame(&result(unchanged()), 100, 30, &plain(), now);
        for want in [
            "Read 122 traces · kept 7 reflexes · nothing changed",
            "Learned here · v17.0.12",
            "Instincts · v3.0.0",
            "2 in use as advice",
            "3 overlaps settled: these now suggest instead of firing",
            "1 reflex not learned: its trigger matches too many different commands",
            "Review in Reflexes · 7 to look over",
        ] {
            assert!(text.contains(want), "{want}:\n{text}");
        }
    }

    #[test]
    fn every_line_is_laid_out_to_the_width_never_wrapped() {
        let t = plain();
        for card in [unchanged(), changed()] {
            for w in [26, 40, 60, 72, 96] {
                let lines = main_lines(&card, w, &t);
                for l in &lines {
                    assert!(l.width() <= w, "{w}: {l:?}");
                }
                assert_eq!(lines.len(), main_lines(&card, 96, &t).len(), "the same rows at every width");
            }
        }
    }

    #[test]
    fn colour_ascii_and_details() {
        let now = Instant::now();
        let (_, buf) = frame(&result(changed()), 100, 30, &Theme::with_color(true), now);
        insta::assert_snapshot!("conditioned-changes-colour-100x30", crate::ui::tests::styled(&buf));
        let (text, _) = frame(&result(unchanged()), 80, 30, &plain().ascii(), now);
        assert!(text.is_ascii(), "ADAGE_ASCII draws nothing else:\n{text}");
        insta::assert_snapshot!("conditioned-unchanged-ascii-80x30", text);
        // `v`: the details, ids and the note as written, wrapped at words under their column.
        let mut s = AppState::new("config.toml".into());
        s.modal = Some(Modal::Conditioning(Box::new(result(unchanged()))));
        let m = s.modal.take().unwrap();
        let Modal::Conditioning(flow) = m else { unreachable!() };
        assert!(key(&mut s, flow, KeyCode::Char('v')).is_none());
        let Some(Modal::Conditioning(flow)) = &s.modal else { panic!("the card stays open") };
        let (text, _) = frame(flow, 120, 40, &plain(), now);
        insta::assert_snapshot!("conditioned-details-120x40", text);
        for want in ["run-7c62ab stays the one fire beside", "gen", "support ≥ 5", "(run-ce9cdb)"] {
            assert!(text.contains(want) || want == "gen", "{want}:\n{text}");
        }
    }

    #[test]
    fn enter_takes_the_next_step_and_esc_closes() {
        let open = |card: Card| {
            let mut s = AppState::new("config.toml".into());
            s.tab = Tab::Health;
            (s, Box::new(result(card)))
        };
        let (mut s, flow) = open(unchanged());
        assert!(matches!(key(&mut s, flow, KeyCode::Enter), Some(Cmd::Want(_))));
        assert_eq!(s.tab, Tab::Reflexes);
        assert!(s.modal.is_none());
        let (mut s, flow) = open(unchanged());
        assert!(key(&mut s, flow, KeyCode::Esc).is_none());
        assert!(s.modal.is_none() && s.tab == Tab::Health);
        // Nothing usable: the history sources, as the CLI says `adage sense`.
        let mut none = unchanged();
        none.repos.clear();
        none.used = 0;
        assert!(matches!(none.next().0, Go::Tool(Tool::HistorySources)));
        assert_eq!(none.sentence(), "Read 122 traces · none could be used");
    }

    #[test]
    fn headlines_say_what_happened() {
        assert_eq!(unchanged().sentence(), "Read 122 traces · kept 7 reflexes · nothing changed");
        assert_eq!(
            changed().sentence(),
            "Read 125 traces · kept 8 reflexes · 1 new, 1 to approve, 1 stronger, 1 extinguished"
        );
        let mut day0 = unchanged();
        day0.read = 0;
        day0.used = 0;
        day0.repos[0].sets.retain(ReflexSet::instincts);
        assert_eq!(day0.sentence(), "No past traces here yet · bound 2 instincts from its files · nothing changed");
        let p = |k: &'static str, a: Option<Strength>, b: Option<Strength>| (k, a, b);
        assert_eq!(changed_words([]), "nothing changed");
        assert_eq!(changed_words([p("evidence", None, None)]), "same reflexes, fresher evidence");
        assert_eq!(
            changed_words([
                p("new", None, Some(Strength::Fire)),
                p("new", None, Some(Strength::Prime)),
                p("strength", Some(Strength::Fire), Some(Strength::Prime)),
                p("strength", Some(Strength::Prime), Some(Strength::Inhibit)),
                p("proposal", None, None),
                p("dormant", None, None),
                p("candidate", None, None),
            ]),
            "2 new, 1 weaker, 1 changed strength, 1 dormant, 1 change proposed"
        );
        // Older learn.log words read the same.
        assert_eq!(
            changed_words([p("retired", None, None), p("aspect", Some(Strength::Prime), Some(Strength::Fire))]),
            "1 stronger, 1 extinguished"
        );
    }

    #[test]
    fn notes_are_grouped_and_counted() {
        let lines: Vec<String> = [
            NOTE,
            "adage: 2 reflexes not learned: its trigger matches too many different commands (run-a, run-b)",
            "adage: 1 instinct not bound: its trigger names none of the commands it follows (u3)",
            "warning  skipped 3 compressed Codex rollouts (.jsonl.zst); decompress them with `zstd -d`",
            "warning  skipped 3 compressed Codex rollouts (.jsonl.zst); decompress them with `zstd -d`",
            "no past traces found; binding this repository's instincts from its files",
            "",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let g = group_notes(&lines);
        let texts: Vec<(&str, usize, bool)> = g.iter().map(|g| (g.text.as_str(), g.count, g.warn)).collect();
        assert_eq!(
            texts,
            vec![
                ("3 reflexes not learned: its trigger matches too many different commands", 3, true),
                ("1 instinct not bound: its trigger names none of the commands it follows", 1, true),
                ("skipped 3 compressed Codex rollouts (.jsonl.zst); decompress them with `zstd -d` · 2 times", 2, true),
                ("no past traces found; binding this repository's instincts from its files", 1, false),
            ]
        );
        // On a day-0 run the headline already says it.
        let mut day0 = unchanged();
        day0.read = 0;
        day0.notes = lines;
        assert!(day0.note_groups().iter().all(|g| !g.text.starts_with("no past traces")));
    }

    #[test]
    fn the_report_reads_back_with_its_added_fields() {
        let c = changed();
        assert_eq!((c.read, c.used, c.steered, c.elapsed_ms, c.waiting), (125, 106, 6, Some(12_310), (1, 3)));
        assert_eq!(c.reflexes(), 8);
        let r = &c.repos[0];
        assert_eq!((r.patterns, r.compiled, r.kept, r.contexts), (37, 10, 6, 1));
        assert_eq!(r.changes[1].to, Some(Strength::Fire));
        assert_eq!(r.changes[1].name(), "cargo fmt --all · after editing crates/**/*.rs");
        assert_eq!(unchanged().overlaps[0].kind, ConflictKind::TwoFires);
        assert!(Card::parse("adage condition · 3 traces read", vec![]).is_none(), "text output is not a report");
        assert!(Card::parse("{}", vec![]).is_none());
        // What a job's text says when no card is drawn.
        assert_eq!(
            changed().text_lines()[..3],
            [
                "Read 125 traces · kept 8 reflexes · 1 new, 1 to approve, 1 stronger, 1 extinguished".to_string(),
                "  extinguished: cargo build -p adage-cli · after editing crates/**/*.rs, then cargo build -p adage-cli"
                    .into(),
                "  new fire: cargo fmt --all · after editing crates/**/*.rs".into(),
            ]
        );
    }

    #[test]
    fn the_sources_card_and_the_running_card() {
        let now = Instant::now();
        let flow = Flow::confirm(sample_sources(117), ADAGE.into());
        let (text, _) = frame(&flow, 80, 30, &plain(), now);
        insta::assert_snapshot!("conditioned-confirm-80x30", text);
        for want in
            ["✓ Claude Code", "117 unsteered", "6 steered", "not sessions used for learning", "enter  Condition"]
        {
            assert!(text.contains(want), "{want}:\n{text}");
        }
        let mut s = AppState::new("config.toml".into());
        assert!(matches!(
            key(&mut s, Box::new(Flow::confirm(sample_sources(1), ADAGE.into())), KeyCode::Char('y')),
            Some(Cmd::Do(Action::LearnNow))
        ));
        assert!(key(&mut s, Box::new(Flow::confirm(sample_sources(1), ADAGE.into())), KeyCode::Esc).is_none());
        assert!(s.modal.is_none(), "esc: nothing runs");
        let (text, _) = frame(&Flow::confirm(sample_sources(0), ADAGE.into()), 80, 30, &plain(), now);
        assert!(text.contains("6 steered") && text.contains("enter  Condition"), "{text}");
        assert!(!text.contains("Bind instincts"), "steered files can still contain useful work: {text}");
        let mut empty = sample_sources(0);
        empty.files -= empty.steered;
        empty.steered = 0;
        empty.instincts_only = true;
        let (text, _) = frame(&Flow::confirm(empty, ADAGE.into()), 80, 30, &plain(), now);
        assert!(text.contains("no non-empty history") && text.contains("Bind instincts"), "{text}");

        // Running: reading is done, patterns are being found.
        let t0 = now;
        let mut st = Stages::new(t0);
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        st.record(Event { progress: Stage::Read, done: 120, total: 264 }, at(2_000));
        st.record(Event { progress: Stage::Read, done: 264, total: 264 }, at(4_000));
        st.record(Event { progress: Stage::Mine, done: 0, total: 0 }, at(4_100));
        let flow = Flow::Running { repo: ADAGE.into(), stages: Box::new(st.clone()) };
        let (text, _) = frame(&flow, 80, 30, &plain(), at(16_400));
        insta::assert_snapshot!("conditioned-running-80x30", text);
        assert!(text.contains("✓ Reading traces") && text.contains("○ Compiling reflexes"), "{text}");
        // Stages never go back; one passed over between frames counts as started.
        st.record(Event { progress: Stage::Index, done: 0, total: 0 }, at(20_000));
        st.record(Event { progress: Stage::Compile, done: 0, total: 0 }, at(21_000));
        assert_eq!(st.current(), 4);
        assert_eq!(st.at[2], Some(at(20_000)));
        // Esc hides it; the run goes on.
        let mut s = AppState::new("config.toml".into());
        assert!(key(&mut s, Box::new(flow), KeyCode::Esc).is_none());
        assert!(s.modal.is_none() && s.toasts[0].text.contains("background"));
        // While reading, how far it has got.
        let mut st = Stages::new(t0);
        st.record(Event { progress: Stage::Read, done: 120, total: 264 }, at(2_000));
        let (text, _) = frame(&Flow::Running { repo: ADAGE.into(), stages: Box::new(st) }, 80, 30, &plain(), at(3_000));
        assert!(text.contains("Reading traces") && text.contains("120 of 264") && text.contains("━"), "{text}");
    }

    #[test]
    fn the_app_routes_conditioning_through_the_cards() {
        let mut s = AppState::new("config.toml".into());
        // The palette's Condition starts from the sources card, as Health's `c` does.
        assert!(matches!(s.run_entry(Go::Tool(Tool::Condition)), Some(Cmd::PreviewLearning)));
        // Done with no card in the way: the result opens; with another open, a toast says it.
        s.complete_job(
            "Condition this repository",
            Ok(super::super::jobs::Outcome::Conditioned(Box::new(unchanged()))),
        );
        assert!(matches!(&s.modal, Some(Modal::Conditioning(f)) if matches!(**f, Flow::Result { .. })));
        s.modal = Some(Modal::Help { scroll: 0 });
        s.complete_job("Condition this repository", Ok(super::super::jobs::Outcome::Conditioned(Box::new(changed()))));
        assert!(matches!(s.modal, Some(Modal::Help { .. })), "the open card stays");
        assert!(s.toasts.last().is_some_and(|t| t.text.starts_with("Conditioning done: Read 125 traces")));
    }
}
