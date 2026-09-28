//! The interactive `adage init`: a guided setup, inline in the terminal (no full screen), that
//! asks before each step and leaves its record in the scrollback.
//!
//! ```text
//! ┌  adage init · acme-billing
//! │
//! │  Adage ships with instincts and forms reflexes from your traces.
//! │  Nothing is written without asking. Fail-open: if Adage errors, your agent proceeds.
//! │
//! ◇  Harnesses · claude-code, codex                ← finished steps, in the scrollback
//! ◇  Hooks · claude-code · wrote ~/.claude/settings.json
//! ◆  Embedding model                               ← the active step, live
//! │  Optional: …
//! │  › Fetch it (31 MB)
//! │    Skip for now
//! └  ↑↓ choose · enter confirm · q stop
//! ```
//!
//! Steps: harnesses (read-only); hooks, per harness found (the exact file and diff first; this
//! repository or every repository); the embedding model (optional); a first `adage condition`;
//! auto-condition; then the doctor's summary and what to run next. `y` takes the first choice,
//! `n` skips, `q` (or esc) stops: nothing already done is undone, and nothing else is written.

use super::app::install_confirm;
use super::health::{self, Health};
use super::jobs::{self, Job};
use super::term::{self, Term};
use super::theme::Theme;
use super::view::Ctx;
use crate::detect::{self, Detected, Registration};
use crate::doctor::Status;
use crate::install::Plan;
use adage_store::config;
use adage_term::Glyph;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Frame;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Harnesses,
    Hooks(&'static str),
    Model,
    Learn,
    AutoLearn,
    Finish,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    Todo,
    Done(String),
    Skipped,
    Failed(String),
}

pub struct Step {
    pub kind: Kind,
    pub status: StepStatus,
}

impl Step {
    pub fn title(&self) -> String {
        match &self.kind {
            Kind::Harnesses => "Harnesses".into(),
            Kind::Hooks(h) => format!("Hooks · {h}"),
            Kind::Model => "Embedding model".into(),
            Kind::Learn => "Condition from past traces".into(),
            Kind::AutoLearn => "Auto-condition".into(),
            Kind::Finish => "Ready".into(),
        }
    }
}

/// What the runtime must do after a key.
pub enum Cmd {
    Quit,
    Install(Box<Plan>),
    FetchModel,
    Learn,
    AutoLearn,
    Replan,
    Finish,
}

pub struct Wizard {
    pub steps: Vec<Step>,
    pub cur: usize,
    pub detected: Vec<Detected>,
    pub registrations: Vec<Registration>,
    /// For the current hooks step: `project` or `user`, and the planned write.
    pub scope: &'static str,
    pub plan: Option<Result<Plan, String>>,
    pub model: Option<String>,
    /// The effective `learn.auto`, and where it came from.
    pub auto_learn: Option<(bool, String)>,
    pub config_path: String,
    pub progress: Option<(String, f64)>,
    pub output: Vec<String>,
    pub health: Option<Health>,
    pub scroll: u16,
    /// How far the body can scroll, as last drawn (the body's length is known only then).
    pub max_scroll: std::cell::Cell<u16>,
    pub has_repo: bool,
    /// The highlighted choice of the current step.
    pub choice: usize,
}

impl Wizard {
    pub fn new(detected: Vec<Detected>, registrations: Vec<Registration>, has_repo: bool) -> Wizard {
        let mut steps = vec![Step { kind: Kind::Harnesses, status: StepStatus::Todo }];
        for d in detected.iter().filter(|d| d.found()) {
            steps.push(Step { kind: Kind::Hooks(d.harness), status: StepStatus::Todo });
        }
        for k in [Kind::Model, Kind::Learn, Kind::AutoLearn, Kind::Finish] {
            steps.push(Step { kind: k, status: StepStatus::Todo });
        }
        Wizard {
            steps,
            cur: 0,
            detected,
            registrations,
            scope: if has_repo { "project" } else { "user" },
            plan: None,
            model: None,
            auto_learn: None,
            config_path: String::new(),
            progress: None,
            output: vec![],
            health: None,
            scroll: 0,
            max_scroll: Default::default(),
            has_repo,
            choice: 0,
        }
    }

    pub fn current(&self) -> &Kind {
        &self.steps[self.cur].kind
    }

    /// The current step's choices; the last one of a step that can be skipped is the skip.
    pub fn choices(&self) -> Vec<String> {
        if !self.output.is_empty() {
            return vec!["Continue".into()];
        }
        match self.current() {
            Kind::Harnesses => vec!["Continue".into()],
            Kind::Hooks(h) => {
                let mut v = Vec::new();
                if self.has_repo {
                    v.push("For this repository".to_string());
                }
                v.push(format!("For this user · every repository you open with {h}"));
                v.push(format!("Skip {h}"));
                v
            }
            Kind::Model if self.model.is_some() => vec!["Continue".into()],
            Kind::Model => vec!["Fetch it · 31 MB, SHA-256 verified".into(), "Skip for now".into()],
            Kind::Learn => vec!["Condition now".into(), "Skip · it runs by itself as traces end".into()],
            Kind::AutoLearn if self.auto_learn.as_ref().is_some_and(|(on, _)| *on) => vec!["Keep it on".into()],
            Kind::AutoLearn => vec!["Turn it on".into(), "Leave it off".into()],
            Kind::Finish => vec!["Done".into()],
        }
    }

    /// Record the current step's outcome and move on. Returns the command the next step needs to
    /// prepare (a plan, or the summary).
    pub fn advance(&mut self, status: StepStatus) -> Option<Cmd> {
        self.steps[self.cur].status = status;
        self.output.clear();
        self.scroll = 0;
        self.choice = 0;
        if self.cur + 1 < self.steps.len() {
            self.cur += 1;
        }
        match self.current() {
            Kind::Hooks(_) => {
                self.scope = if self.has_repo { "project" } else { "user" };
                Some(Cmd::Replan)
            }
            Kind::Finish => Some(Cmd::Finish),
            _ => None,
        }
    }

    /// The scope the hooks step's highlighted choice stands for.
    fn scope_of_choice(&self) -> &'static str {
        if self.has_repo && self.choice == 0 {
            "project"
        } else {
            "user"
        }
    }

    pub fn key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<Cmd> {
        if code == KeyCode::Char('c') && mods.contains(KeyModifiers::CONTROL)
            || matches!(code, KeyCode::Char('q') | KeyCode::Esc)
        {
            return Some(Cmd::Quit);
        }
        if self.progress.is_some() {
            return None; // a job is running
        }
        let n = self.choices().len();
        let hooks = matches!(self.current(), Kind::Hooks(_)) && self.output.is_empty();
        match code {
            KeyCode::Up | KeyCode::Char('k') if self.choice > 0 => {
                self.choice -= 1;
                if hooks && self.choice < n - 1 && self.scope_of_choice() != self.scope {
                    self.scope = self.scope_of_choice();
                    return Some(Cmd::Replan);
                }
                return None;
            }
            KeyCode::Down | KeyCode::Char('j') if self.choice + 1 < n => {
                self.choice += 1;
                if hooks && self.choice < n - 1 && self.scope_of_choice() != self.scope {
                    self.scope = self.scope_of_choice();
                    return Some(Cmd::Replan);
                }
                return None;
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(5);
                return None;
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(5).min(self.max_scroll.get());
                return None;
            }
            _ => {}
        }
        // `y` is the first choice, `n` the skip, Enter the highlighted one.
        let skip = n > 1
            && (code == KeyCode::Char('n')
                || code == KeyCode::Char('s')
                || (code == KeyCode::Enter && self.choice == n - 1));
        let yes = !skip && (code == KeyCode::Char('y') || code == KeyCode::Enter);
        // A step that has run shows its result; any answer moves on.
        if !self.output.is_empty() && (yes || skip || code == KeyCode::Char('n')) && *self.current() != Kind::Finish {
            let s = self.steps[self.cur].status.clone();
            return self.advance(if s == StepStatus::Todo { StepStatus::Done(String::new()) } else { s });
        }
        match self.current().clone() {
            Kind::Harnesses if yes => self.advance(StepStatus::Done(self.found_names())),
            Kind::Hooks(_) if code == KeyCode::Tab && self.has_repo => {
                self.scope = if self.scope == "project" { "user" } else { "project" };
                self.choice = if self.scope == "project" { 0 } else { 1 };
                Some(Cmd::Replan)
            }
            Kind::Hooks(_) if skip => self.advance(StepStatus::Skipped),
            Kind::Hooks(_) if yes => match &self.plan {
                Some(Ok(p)) => Some(Cmd::Install(Box::new(p.clone()))),
                _ => None,
            },
            Kind::Model if yes && self.model.is_some() => {
                let m = self.model.clone().unwrap_or_default();
                self.advance(StepStatus::Done(format!("{m} already installed")))
            }
            Kind::Model if yes => Some(Cmd::FetchModel),
            Kind::Learn if yes => Some(Cmd::Learn),
            // Already on (by default, in the file, or ADAGE_AUTO_LEARN): nothing to write.
            Kind::AutoLearn if yes && self.auto_learn.as_ref().is_some_and(|(on, _)| *on) => {
                let src = self.auto_learn.as_ref().map(|(_, s)| s.clone()).unwrap_or_default();
                self.advance(StepStatus::Done(format!("on ({src})")))
            }
            Kind::AutoLearn if yes => Some(Cmd::AutoLearn),
            Kind::Model | Kind::Learn | Kind::AutoLearn if skip => self.advance(StepStatus::Skipped),
            Kind::Finish if yes => Some(Cmd::Quit),
            _ => None,
        }
    }

    fn found_names(&self) -> String {
        let found: Vec<&str> = self.detected.iter().filter(|d| d.found()).map(|d| d.harness).collect();
        if found.is_empty() {
            "none found".into()
        } else {
            found.join(", ")
        }
    }

    /// What was done, as plain lines (as it reads in the scrollback).
    #[cfg(test)]
    pub fn report(&self) -> Vec<String> {
        let t = Theme::with_color(false);
        let mut out: Vec<String> = Vec::new();
        for s in &self.steps {
            if s.kind == Kind::Finish {
                continue;
            }
            out.extend(record(s, &t).iter().map(line_text));
        }
        out.extend(finish_card(self, &t, true, 100).iter().map(line_text));
        out
    }
}

#[cfg(test)]
fn line_text(l: &Line) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect::<String>().trim_end().to_string()
}

// ---------------------------------------------------------------------------------------------
// The rail

fn rail<'a>(t: &Theme, active: bool) -> Span<'a> {
    Span::styled(format!("{}  ", t.glyph(Glyph::Rail)), if active { t.accent() } else { t.dim() })
}

/// Wrap `text` to `width` columns (by words; long words are left whole).
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for w in text.split(' ') {
        if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > width {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

/// The opening lines, printed once into the scrollback.
pub fn header(ctx: &Ctx) -> Vec<Line<'static>> {
    let t = &ctx.theme;
    vec![
        Line::from(vec![
            Span::styled(t.ch("┌  ", "+  "), t.dim()),
            Span::styled("adage init", t.title()),
            Span::styled(format!(" · {}", ctx.label), t.dim()),
        ]),
        Line::from(rail(t, false)),
        Line::from(vec![rail(t, false), Span::raw("Adage ships with instincts and forms reflexes from your traces.")]),
        Line::from(vec![
            rail(t, false),
            Span::styled(
                "Nothing is written without asking. Fail-open: if Adage errors, your agent proceeds normally.",
                t.dim(),
            ),
        ]),
        Line::from(rail(t, false)),
    ]
}

/// A finished step as it stays in the scrollback: `◇  Title · what happened`.
pub fn record(s: &Step, t: &Theme) -> Vec<Line<'static>> {
    let (mark, style, note) = match &s.status {
        StepStatus::Done(n) => (t.glyph(Glyph::Done), t.green(), n.clone()),
        StepStatus::Skipped => (t.glyph(Glyph::Done), t.dim(), "skipped".to_string()),
        StepStatus::Failed(e) => (t.glyph(Glyph::Fail), t.red(), e.clone()),
        StepStatus::Todo => (t.glyph(Glyph::Sep), t.dim(), "not reached".to_string()),
    };
    let mut first = vec![Span::styled(format!("{mark}  "), style), Span::styled(s.title(), t.bold())];
    if !note.is_empty() {
        first.push(Span::styled(format!(" · {note}"), t.dim()));
    }
    vec![Line::from(first)]
}

/// The body of the active step, before its choices.
fn body(w: &Wizard, t: &Theme, width: usize) -> Vec<Line<'static>> {
    let text = |s: &str, style: Style| -> Vec<Line<'static>> {
        wrap(s, width).into_iter().map(|l| Line::from(Span::styled(l, style))).collect()
    };
    let plain = Style::default();
    let mut out: Vec<Line> = Vec::new();
    match w.current() {
        Kind::Harnesses => {
            for d in &w.detected {
                if d.found() {
                    out.push(Line::from(vec![
                        Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.green()),
                        Span::styled(format!("{:<13}", d.harness), t.bold()),
                        Span::styled(d.evidence.join("; "), t.dim()),
                    ]));
                } else {
                    out.push(Line::from(vec![
                        Span::styled(format!("{} ", t.glyph(Glyph::Off)), t.dim()),
                        Span::styled(format!("{:<13}", d.harness), t.dim()),
                        Span::styled("not found", t.dim()),
                    ]));
                }
            }
            if !w.detected.iter().any(|d| d.found()) {
                out.push(Line::raw(""));
                out.extend(text("No harness found, so there are no hooks to install. Install Claude Code, Codex or OpenCode, then run `adage init` again; the other steps still help.", t.dim()));
            }
        }
        Kind::Hooks(h) => {
            for r in w.registrations.iter().filter(|r| r.harness == *h) {
                out.extend(text(&format!("Already registered in {}.", detect::tilde(&r.location)), t.green()));
            }
            match &w.plan {
                None => out.push(Line::from(Span::styled("planning…", t.dim()))),
                Some(Err(e)) => out.extend(text(e, t.red())),
                Some(Ok(p)) => out.extend(
                    install_confirm(p, t)
                        .into_iter()
                        .filter(|l| !l.spans.iter().any(|s| s.content.starts_with("y write"))),
                ),
            }
        }
        Kind::Model => match &w.model {
            Some(m) => out.extend(text(&format!("Installed: {m}."), plain)),
            None => out.extend(text(
                "Optional. With the embedding model, prompts that mean the same thing (\"run the tests\", \"check my change\") share a reflex. Everything else works without it.",
                plain,
            )),
        },
        Kind::Learn => out.extend(text(
            "Read your agents' past traces (Claude Code history under ~/.claude/projects) and form reflexes from what they repeat. It only reads them; it never runs anything in them.",
            plain,
        )),
        Kind::AutoLearn => {
            out.extend(text(
                "Condition in the background whenever an agent's trace ends, so reflexes keep up with how your agents work.",
                plain,
            ));
            let now = match &w.auto_learn {
                Some((true, src)) => format!("Now: on ({src})."),
                Some((false, src)) => format!("Now: off ({src}). Turning it on sets learn.auto = true in {}.", w.config_path),
                None => String::new(),
            };
            if !now.is_empty() {
                out.extend(text(&now, t.dim()));
            }
        }
        Kind::Finish => match &w.health {
            None => out.push(Line::from(Span::styled("running `adage doctor`…", t.dim()))),
            Some(h) => {
                let (p, wn, f) = (h.count(Status::Pass), h.count(Status::Warn), h.count(Status::Fail));
                out.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", t.glyph(if f > 0 { Glyph::Fail } else { Glyph::Ok })),
                        if f > 0 { t.red() } else { t.green() },
                    ),
                    Span::styled(format!("adage doctor · {p} passed · {wn} to look at · {f} failing"), t.bold()),
                ]));
                // Each finding and its fix, wrapped under their own text rather than cut off.
                let mut found = Vec::new();
                for c in h.checks.iter().filter(|c| c.status != Status::Pass) {
                    let style = if c.status == Status::Fail { t.red() } else { t.yellow() };
                    found.push(Line::from(vec![
                        Span::styled("! ", style),
                        Span::styled(format!("{}  ", c.label), t.bold()),
                        Span::styled(c.detail.clone(), t.dim()),
                    ]));
                    if let Some(fx) = &c.fix {
                        found.push(Line::from(vec![Span::styled("  fix ", t.dim()), Span::styled(fx.clone(), t.dim())]));
                    }
                }
                out.extend(super::view::wrap_hanging(t.lines(found), width as u16, true));
            }
        },
    }
    if !w.output.is_empty() {
        out.push(Line::raw(""));
        for l in &w.output {
            out.extend(text(l, t.dim()));
        }
    }
    out
}

fn hint(w: &Wizard) -> &'static str {
    if w.progress.is_some() {
        return "working… q stops";
    }
    if w.choices().len() > 1 {
        if matches!(w.current(), Kind::Hooks(_)) && w.output.is_empty() {
            "↑↓ choose · enter confirm · PgUp/PgDn diff · q stop"
        } else {
            "↑↓ choose · enter confirm · q stop"
        }
    } else if *w.current() == Kind::Finish {
        "enter finishes"
    } else {
        "enter continues · q stop"
    }
}

/// The active step, drawn in the inline viewport.
pub fn draw(f: &mut Frame, w: &Wizard, ctx: &Ctx) {
    let area = f.area();
    render(area, f.buffer_mut(), w, ctx);
}

fn render(area: Rect, buf: &mut ratatui::buffer::Buffer, w: &Wizard, ctx: &Ctx) {
    let t = &ctx.theme;
    let width = (area.width as usize).saturating_sub(4).max(20);
    let mut lines: Vec<Line> = vec![Line::from(vec![
        Span::styled(format!("{}  ", t.glyph(Glyph::Active)), t.accent()),
        Span::styled(w.steps[w.cur].title(), t.bold()),
    ])];
    // The body scrolls (PgUp/PgDn) inside what is left once the choices and hint are placed.
    let choices = w.choices();
    let fixed = 1 + 1 + if choices.len() > 1 || w.progress.is_some() { choices.len() + 1 } else { 0 } + 2;
    let room = (area.height as usize).saturating_sub(fixed);
    let b = body(w, t, width);
    w.max_scroll.set(b.len().saturating_sub(room) as u16);
    let start = (w.scroll as usize).min(b.len().saturating_sub(room));
    for l in b.into_iter().skip(start).take(room) {
        let mut spans = vec![rail(t, true)];
        spans.extend(l.spans);
        lines.push(Line::from(spans));
    }
    if let Some((status, ratio)) = &w.progress {
        let bar_w = width.clamp(10, 40);
        let done = ((*ratio).clamp(0.0, 1.0) * bar_w as f64).round() as usize;
        lines.push(Line::from(rail(t, true)));
        lines.push(Line::from(vec![
            rail(t, true),
            Span::styled(t.ch("━", "=").repeat(done), t.accent()),
            Span::styled(t.ch("─", "-").repeat(bar_w - done), t.dim()),
            Span::styled(format!("  {status}"), t.dim()),
        ]));
    } else if choices.len() > 1 {
        lines.push(Line::from(rail(t, true)));
        for (i, c) in choices.iter().enumerate() {
            let sel = i == w.choice;
            lines.push(Line::from(vec![
                rail(t, true),
                Span::styled(if sel { format!("{} ", t.glyph(Glyph::Pointer)) } else { "  ".into() }, t.accent()),
                Span::styled(c.clone(), if sel { t.bold() } else { t.dim() }),
            ]));
        }
    }
    lines.push(Line::from(rail(t, true)));
    lines.push(Line::from(vec![Span::styled(t.ch("└  ", "+  "), t.accent()), Span::styled(hint(w), t.dim())]));
    Paragraph::new(t.lines(lines)).render(area, buf);
}

/// The closing lines, printed into the scrollback when the flow ends.
pub fn finish_card(w: &Wizard, t: &Theme, completed: bool, cols: usize) -> Vec<Line<'static>> {
    let mut out = vec![Line::from(Span::styled(t.glyph(Glyph::Rail), t.dim()))];
    if !completed {
        out.push(Line::from(vec![
            Span::styled(t.ch("└  ", "+  "), t.dim()),
            Span::styled("Stopped.", t.bold()),
            Span::styled(
                " Steps already done stay done; nothing else was changed. `adage init` picks up again any time.",
                t.dim(),
            ),
        ]));
        return out;
    }
    let failing = w.health.as_ref().map_or(0, |h| h.count(Status::Fail));
    // Without hooks, agents never hear from Adage: "ready" would be a promise it cannot keep.
    let unhooked: Vec<String> = w
        .health
        .as_ref()
        .map(|h| {
            h.checks
                .iter()
                .filter(|c| c.check.starts_with("hooks: ") && c.status != Status::Pass)
                .map(|c| c.check.trim_start_matches("hooks: ").to_string())
                .collect()
        })
        .unwrap_or_default();
    let (mark, mark_style, title, why) = if failing > 0 {
        (t.glyph(Glyph::Fail), t.red(), "Set up, with problems to fix.", " Each one and its fix is below.".to_string())
    } else if !unhooked.is_empty() {
        (
            "!",
            t.yellow(),
            "Almost ready.",
            format!(
                " Hooks are not registered with {}, so agents there don't hear from Adage yet.",
                unhooked.join(", ")
            ),
        )
    } else {
        (
            t.glyph(Glyph::Ok),
            t.green(),
            "Ready.",
            " Your agents' next traces get Adage's instincts, and reflexes as they form.".to_string(),
        )
    };
    out.push(Line::from(vec![
        Span::styled(t.ch("└  ", "+  "), t.dim()),
        Span::styled(format!("{mark} "), mark_style),
        Span::styled(title, t.bold()),
        Span::raw(why),
    ]));
    // The doctor's findings stay in the scrollback, each with its fix.
    if let Some(h) = &w.health {
        for c in h.checks.iter().filter(|c| c.status != Status::Pass) {
            let style = if c.status == Status::Fail { t.red() } else { t.yellow() };
            out.push(Line::from(vec![
                Span::styled("   ! ", style),
                Span::styled(format!("{}  ", c.label), t.bold()),
                Span::styled(c.detail.clone(), t.dim()),
            ]));
            if let Some(fx) = &c.fix {
                out.push(Line::from(vec![Span::styled("       fix ", t.dim()), Span::styled(fx.clone(), t.dim())]));
            }
        }
    }
    out.push(Line::raw(""));
    let mut next: Vec<(String, &str)> = Vec::new();
    if failing > 0 {
        next.push(("adage doctor".into(), "what needs fixing, and how"));
    }
    if let Some(h) = unhooked.first() {
        next.push((format!("adage install {h} --write"), "registers the hooks (or run `adage init` again)"));
    }
    next.push(("adage watch".into(), "live pane for a split terminal"));
    next.push(("adage map".into(), "explore reflexes in the browser"));
    next.push(("adage ui".into(), "review, health and settings in one place"));
    out.extend(next_lines(&next, t, cols));
    out
}

/// The `next` rows: a command column, then each description wrapped to `cols` under its own
/// first line. The column is as wide as the longest command when that leaves the descriptions
/// room; otherwise a longer command takes a line of its own, its description on the next line
/// in the same column as the others. Narrower still, descriptions go under their commands.
fn next_lines(rows: &[(String, &str)], t: &Theme, cols: usize) -> Vec<Line<'static>> {
    const LEAD: usize = 9; // `   next  `
    let longest = rows.iter().map(|(c, _)| c.chars().count()).max().unwrap_or(0).max(14);
    let cmd_w = if cols.saturating_sub(LEAD + longest + 2) >= 36 { longest } else { 14 };
    let (indent, room) = match cols.saturating_sub(LEAD + cmd_w + 2) {
        room if room >= 20 => (LEAD + cmd_w + 2, room),
        _ => (LEAD + 2, cols.saturating_sub(LEAD + 2).max(20)),
    };
    let mut out = Vec::new();
    for (k, (cmd, why)) in rows.iter().enumerate() {
        let lead = Span::styled(if k == 0 { "   next  " } else { "         " }, t.accent().patch(t.bold()));
        let mut parts = wrap(why, room).into_iter();
        let beside = indent > LEAD + 2 && cmd.chars().count() <= cmd_w;
        if beside {
            let first = parts.next().unwrap_or_default();
            out.push(Line::from(vec![
                lead,
                Span::styled(format!("{cmd:<cmd_w$}"), t.command()),
                Span::styled(format!("  {first}"), t.dim()),
            ]));
        } else {
            out.push(Line::from(vec![lead, Span::styled(cmd.clone(), t.command())]));
        }
        for p in parts {
            out.push(Line::from(vec![Span::raw(" ".repeat(indent)), Span::styled(p, t.dim())]));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Runtime

/// Height of the inline viewport: enough for a hooks diff and its choices.
const VIEW_HEIGHT: u16 = 18;

/// The terminal's width, for text printed into the scrollback.
fn cols(t: &Term) -> usize {
    t.terminal.size().map(|s| s.width).unwrap_or(80).max(20) as usize
}

fn insert(t: &mut Term, lines: Vec<Line<'static>>, theme: &Theme) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let cols = cols(t);
    let lines: Vec<Line<'static>> = theme.lines(lines).into_iter().flat_map(|l| fold(l, cols)).collect();
    let h = lines.len() as u16;
    t.terminal.insert_before(h, |buf| {
        Paragraph::new(lines).render(buf.area, buf);
    })?;
    Ok(())
}

/// Fold a line wider than `cols` at word boundaries; continuation lines keep the line's indent
/// (its leading rail or mark column), so wrapped text stays aligned in the scrollback.
fn fold(line: Line<'static>, cols: usize) -> Vec<Line<'static>> {
    if line.width() <= cols {
        return vec![line];
    }
    // The first span is the rail or mark (`│  `, `└  `, `   ! `): continuation lines indent past it.
    let indent = line.spans.first().map_or(0, |s| s.content.chars().count()).min(cols / 2);
    let pad = if line.spans.first().is_some_and(|s| s.content.starts_with(['│', '|'])) {
        line.spans[0].clone()
    } else {
        Span::raw(" ".repeat(indent))
    };
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut cur: Vec<Span<'static>> = Vec::new();
    let mut w = 0usize;
    for span in line.spans {
        let style = span.style;
        for word in span.content.split_inclusive(' ') {
            let ww = word.chars().count();
            if w + word.trim_end().chars().count() > cols && w > indent {
                out.push(Line::from(std::mem::take(&mut cur)));
                cur.push(pad.clone());
                w = indent;
            }
            cur.push(Span::styled(word.to_string(), style));
            w += ww;
        }
    }
    if !cur.is_empty() {
        out.push(Line::from(cur));
    }
    out
}

pub fn run(ctx: Ctx) -> Result<()> {
    let mut w = Wizard::new(detect::harnesses(), detect::registrations(ctx.root.as_deref()), ctx.root.is_some());
    w.model = adage_intent::model::status(&ctx.paths.models()).ok().map(|m| m.id);
    let loaded = config::load(&ctx.paths);
    w.auto_learn = loaded.get("learn.auto").map(|(_, src)| (loaded.config.learn_auto, src.to_string()));
    w.config_path = detect::tilde(&ctx.paths.config());
    let mut job: Option<(Job, Kind)> = None;
    let rows = ratatui::crossterm::terminal::size().map(|(_, h)| h).unwrap_or(24);
    let mut t = Term::inline(VIEW_HEIGHT.min(rows.saturating_sub(2)).max(8)).map_err(|e| {
        anyhow::anyhow!(
            "this terminal could not run the interactive setup ({e:#}); `adage init --print` shows every step as text"
        )
    })?;
    insert(&mut t, header(&ctx), &ctx.theme)?;
    let replan = |w: &mut Wizard| {
        if let Kind::Hooks(h) = w.current().clone() {
            // A new plan starts at its top: the file it writes, then the diff.
            w.scroll = 0;
            w.plan = Some(crate::install::plan(h, w.scope, ctx.root.as_deref(), None).map_err(|e| format!("{e:#}")));
        }
    };
    // Every step that finishes goes into the scrollback, above the viewport.
    let mut recorded = 0usize;
    let completed;
    loop {
        while recorded < w.cur && recorded < w.steps.len() {
            if w.steps[recorded].status != StepStatus::Todo {
                insert(&mut t, record(&w.steps[recorded], &ctx.theme), &ctx.theme)?;
            }
            recorded += 1;
        }
        if let Some((j, kind)) = job.as_mut() {
            match j.poll() {
                Some(done) => {
                    let kind = kind.clone();
                    job = None;
                    w.progress = None;
                    match (kind, done) {
                        (Kind::Model, Ok(lines)) => {
                            w.model = adage_intent::model::status(&ctx.paths.models()).ok().map(|m| m.id);
                            let note = lines.first().cloned().unwrap_or_default();
                            w.advance(StepStatus::Done(note));
                        }
                        (Kind::Learn, Ok(lines)) => {
                            let summary = lines.first().cloned().unwrap_or_default();
                            w.steps[w.cur].status = StepStatus::Done(summary);
                            let n = lines.len();
                            w.output = lines.into_iter().skip(n.saturating_sub(12)).collect();
                        }
                        (_, Err(e)) => {
                            w.steps[w.cur].status = StepStatus::Failed(e.clone());
                            w.output = vec![format!("failed: {e}"), "Everything else works without this step.".into()];
                        }
                        _ => {}
                    }
                }
                None => w.progress = Some((j.status.clone(), j.ratio)),
            }
        }
        t.terminal.draw(|f| draw(f, &w, &ctx))?;
        let Some(k) = term::next_key()? else { continue };
        let Some(cmd) = w.key(k.code, k.modifiers) else { continue };
        let next = match cmd {
            Cmd::Quit => {
                completed = *w.current() == Kind::Finish;
                break;
            }
            Cmd::Replan => {
                replan(&mut w);
                None
            }
            Cmd::Finish => {
                w.health = Some(health::gather(&ctx.paths, ctx.root.as_deref()));
                None
            }
            Cmd::Install(p) => match crate::install::apply(&p) {
                Ok(()) => {
                    w.registrations = detect::registrations(ctx.root.as_deref());
                    w.advance(StepStatus::Done(format!("wrote {}", detect::tilde(&p.target))))
                }
                Err(e) => {
                    w.plan = Some(Err(format!("not written: {e:#}")));
                    None
                }
            },
            Cmd::FetchModel => {
                job = Some((jobs::fetch_model(ctx.paths.clone()), Kind::Model));
                w.progress = Some(("starting…".into(), 0.0));
                None
            }
            Cmd::Learn => {
                job = Some((jobs::learn_now(ctx.paths.clone(), ctx.root.clone()), Kind::Learn));
                w.progress = Some(("reading traces…".into(), 0.1));
                None
            }
            // `adage config set`'s own guard and validation.
            Cmd::AutoLearn => match crate::share::require_person("config set")
                .and_then(|()| config::set(&ctx.paths, "learn.auto", "true").map_err(|e| anyhow::anyhow!(e)))
            {
                Ok(_) => {
                    w.auto_learn = Some((true, "file".into()));
                    w.advance(StepStatus::Done(format!("on · learn.auto = true in {}", w.config_path)))
                }
                Err(e) => w.advance(StepStatus::Failed(format!("{e:#}"))),
            },
        };
        match next {
            Some(Cmd::Replan) => replan(&mut w),
            Some(Cmd::Finish) => w.health = Some(health::gather(&ctx.paths, ctx.root.as_deref())),
            _ => {}
        }
    }
    // The steps not yet in the scrollback (the last ones), then the closing card.
    while recorded < w.steps.len() {
        if w.steps[recorded].kind != Kind::Finish && w.steps[recorded].status != StepStatus::Todo {
            insert(&mut t, record(&w.steps[recorded], &ctx.theme), &ctx.theme)?;
        }
        recorded += 1;
    }
    let card = finish_card(&w, &ctx.theme, completed, cols(&t));
    insert(&mut t, card, &ctx.theme)?;
    t.close_inline()?;
    drop(t);
    for n in super::take_notices() {
        eprintln!("{n}");
    }
    Ok(())
}
