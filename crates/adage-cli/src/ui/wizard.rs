//! The interactive `adage init`: a guided setup that asks before each step.
//!
//! 1. Harnesses: what is installed here (read-only).
//! 2. Hooks, per harness found: the exact file and diff for this repository (Tab: user scope);
//!    written only on `y`, through `adage install --write`'s own code.
//! 3. The embedding model (optional): fetched with progress.
//! 4. A first `adage condition`.
//! 5. Auto-learn: `learn.auto = true` in `config.toml` (`adage config set`), if it is off.
//! 6. The doctor's summary, and where to go next (`adage watch` in a split pane).
//!
//! `n` skips a step, `q` stops; what was done is printed when the wizard exits.

use super::app::install_confirm;
use super::health::{self, Health};
use super::jobs::{self, Job};
use super::term::{self, Term};
use super::theme::{Lamp, Theme};
use super::view::Ctx;
use crate::detect::{self, Detected, Registration};
use crate::doctor::Status;
use crate::install::Plan;
use adage_store::config;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Wrap};
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
    fn title(&self) -> String {
        match &self.kind {
            Kind::Harnesses => "Harnesses".into(),
            Kind::Hooks(h) => format!("Hooks: {h}"),
            Kind::Model => "Embedding model".into(),
            Kind::Learn => "First conditioning".into(),
            Kind::AutoLearn => "Auto-learn".into(),
            Kind::Finish => "Summary".into(),
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
    pub has_repo: bool,
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
            has_repo,
        }
    }

    pub fn current(&self) -> &Kind {
        &self.steps[self.cur].kind
    }

    /// Record the current step's outcome and move on. Returns the command the next step needs to
    /// prepare (a plan, or the summary).
    pub fn advance(&mut self, status: StepStatus) -> Option<Cmd> {
        self.steps[self.cur].status = status;
        self.output.clear();
        self.scroll = 0;
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

    pub fn key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<Cmd> {
        if code == KeyCode::Char('c') && mods.contains(KeyModifiers::CONTROL) || code == KeyCode::Char('q') {
            return Some(Cmd::Quit);
        }
        if self.progress.is_some() {
            return None; // a job is running
        }
        let yes = matches!(code, KeyCode::Char('y') | KeyCode::Enter);
        let no = matches!(code, KeyCode::Char('n') | KeyCode::Char('s'));
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            _ => {}
        }
        // A step that has run shows its result; any answer moves on.
        let ran = !self.output.is_empty();
        if ran && (yes || no) && *self.current() != Kind::Finish {
            let s = self.steps[self.cur].status.clone();
            return self.advance(if s == StepStatus::Todo { StepStatus::Done(String::new()) } else { s });
        }
        match self.current().clone() {
            Kind::Harnesses if yes => self.advance(StepStatus::Done(String::new())),
            Kind::Hooks(_) if code == KeyCode::Tab => {
                self.scope = if self.scope == "project" { "user" } else { "project" };
                Some(Cmd::Replan)
            }
            Kind::Hooks(_) if yes => match &self.plan {
                Some(Ok(p)) => Some(Cmd::Install(Box::new(p.clone()))),
                _ => None,
            },
            Kind::Hooks(_) if no => self.advance(StepStatus::Skipped),
            Kind::Model if yes && self.model.is_some() => {
                let m = self.model.clone().unwrap_or_default();
                self.advance(StepStatus::Done(format!("{m} ready")))
            }
            Kind::Model if yes => Some(Cmd::FetchModel),
            Kind::Learn if yes => Some(Cmd::Learn),
            // Already on (by default, in the file, or ADAGE_AUTO_LEARN): nothing to write.
            Kind::AutoLearn if yes && self.auto_learn.as_ref().is_some_and(|(on, _)| *on) => {
                let src = self.auto_learn.as_ref().map(|(_, s)| s.clone()).unwrap_or_default();
                self.advance(StepStatus::Done(format!("already on ({src})")))
            }
            Kind::AutoLearn if yes => Some(Cmd::AutoLearn),
            Kind::Model | Kind::Learn | Kind::AutoLearn if no => self.advance(StepStatus::Skipped),
            Kind::Finish if yes || code == KeyCode::Esc => Some(Cmd::Quit),
            _ => None,
        }
    }

    /// What was done, for the terminal after the wizard exits.
    pub fn report(&self) -> Vec<String> {
        let mut out = vec!["adage init:".to_string()];
        for s in &self.steps {
            if matches!(s.kind, Kind::Harnesses | Kind::Finish) {
                continue;
            }
            let (mark, note) = match &s.status {
                StepStatus::Todo => ("·", "not reached".to_string()),
                StepStatus::Done(n) => ("✓", n.clone()),
                StepStatus::Skipped => ("–", "skipped".to_string()),
                StepStatus::Failed(e) => ("✗", e.clone()),
            };
            let note = if note.is_empty() { String::new() } else { format!(": {note}") };
            out.push(format!("  {mark} {}{note}", s.title()));
        }
        if let Some(h) = &self.health {
            out.push(format!(
                "adage doctor: {} passed, {} warning(s), {} failure(s)",
                h.count(Status::Pass),
                h.count(Status::Warn),
                h.count(Status::Fail)
            ));
        }
        out.push("Run `adage watch` in a split pane to see Adage work live; `adage ui` for everything else.".into());
        out
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

fn detail(w: &Wizard, t: &Theme) -> Vec<Line<'static>> {
    let dim = |s: &str| Line::from(Span::styled(s.to_string(), t.dim()));
    let mut out: Vec<Line> = Vec::new();
    match w.current() {
        Kind::Harnesses => {
            out.push(Line::raw("Adage directs your coding agents through their harness's hooks. Found here:"));
            out.push(Line::raw(""));
            for d in &w.detected {
                if d.found() {
                    out.push(Line::from(vec![
                        Span::styled("● ", t.green()),
                        Span::styled(format!("{:<12}", d.harness), t.bold()),
                        Span::raw(d.evidence.join("; ")),
                    ]));
                } else {
                    out.push(Line::from(vec![
                        Span::styled("○ ", t.dim()),
                        Span::styled(format!("{:<12}", d.harness), t.dim()),
                        Span::styled("not found", t.dim()),
                    ]));
                }
            }
            out.push(Line::raw(""));
            if !w.detected.iter().any(|d| d.found()) {
                out.push(dim(
                    "No harness found: the hook steps are skipped. Install one, then run `adage init` again.",
                ));
            }
            out.push(dim("Nothing is written without asking. y or Enter continues; q stops."));
        }
        Kind::Hooks(h) => {
            let mine: Vec<&Registration> = w.registrations.iter().filter(|r| r.harness == *h).collect();
            for r in &mine {
                out.push(Line::from(Span::styled(
                    format!("Already registered in {}", detect::tilde(&r.location)),
                    t.green(),
                )));
            }
            let scope = if w.scope == "project" { "this repository" } else { "every repository (user scope)" };
            out.push(Line::from(vec![
                Span::raw(format!("Register Adage's hooks with {h} for ")),
                Span::styled(scope.to_string(), t.bold()),
                Span::raw(":"),
            ]));
            out.push(Line::raw(""));
            match &w.plan {
                None => out.push(dim("planning…")),
                Some(Err(e)) => {
                    out.push(Line::from(Span::styled(e.clone(), t.red())));
                    out.push(dim("Tab switches scope; n skips."));
                }
                Some(Ok(p)) => out.extend(
                    install_confirm(p, t)
                        .into_iter()
                        .filter(|l| !l.spans.iter().any(|s| s.content.starts_with("y write"))),
                ),
            }
        }
        Kind::Model => match &w.model {
            Some(m) => {
                out.push(Line::raw(format!("The embedding model is ready: {m} (installed, or bundled in adage).")))
            }
            None => {
                out.push(Line::raw("Optional: the static embedding model (potion-base-8M, 31 MB, SHA-256 verified)."));
                out.push(Line::raw("With it, `adage condition` groups prompts that mean the same thing into intent"));
                out.push(Line::raw(
                    "families and learns what each usually starts with. Everything else works without it.",
                ));
            }
        },
        Kind::Learn => {
            out.push(Line::raw("Run a first `adage condition`: it reads your agents' traces (Claude Code history"));
            out.push(Line::raw("under ~/.claude/projects), mines reflexes, writes tracks, and compiles the index"));
            out.push(Line::raw("hooks read. It reads transcripts; it never runs anything they contain."));
        }
        Kind::AutoLearn => {
            let now = match &w.auto_learn {
                Some((true, src)) => format!("on ({src}): y keeps it so"),
                Some((false, src)) => format!("off ({src})"),
                None => "unknown".to_string(),
            };
            out.push(Line::raw("Condition automatically after agent sessions end, so reflexes keep up with how your"));
            out.push(Line::raw("agents work. This sets `learn.auto = true` in:"));
            out.push(Line::from(Span::styled(format!("  {}", w.config_path), t.bold())));
            out.push(dim(&format!("Now: {now}.")));
        }
        Kind::Finish => match &w.health {
            None => out.push(dim("running `adage doctor`…")),
            Some(h) => {
                out.push(Line::from(Span::styled(
                    format!(
                        "adage doctor: {} passed, {} warning(s), {} failure(s)",
                        h.count(Status::Pass),
                        h.count(Status::Warn),
                        h.count(Status::Fail)
                    ),
                    t.bold(),
                )));
                for c in h.checks.iter().filter(|c| c.status != Status::Pass) {
                    let lamp = if c.status == Status::Fail { Lamp::Fail } else { Lamp::Warn };
                    out.push(Line::from(vec![
                        Span::styled(format!("{} ", t.lamp_mark(lamp)), t.lamp(lamp)),
                        Span::styled(format!("{}: ", c.check), t.bold()),
                        Span::raw(c.detail.clone()),
                    ]));
                    if let Some(f) = &c.fix {
                        out.push(dim(&format!("    fix: {f}")));
                    }
                }
                out.push(Line::raw(""));
                out.push(Line::from(vec![
                    Span::raw("Next: run "),
                    Span::styled("adage watch", t.accent().patch(t.bold())),
                    Span::raw(" in a split pane to see Adage work live, and "),
                    Span::styled("adage ui", t.accent().patch(t.bold())),
                    Span::raw(" for reflexes, review, health and settings."),
                ]));
            }
        },
    }
    if !w.output.is_empty() {
        out.push(Line::raw(""));
        for l in &w.output {
            out.push(Line::from(Span::styled(l.clone(), t.dim())));
        }
    }
    out
}

fn hint(w: &Wizard) -> &'static str {
    if w.progress.is_some() {
        return "working…";
    }
    if !w.output.is_empty() {
        return "y or Enter continues · q stops";
    }
    match w.current() {
        Kind::Harnesses => "y continue · q stop",
        Kind::Hooks(_) => "y install · Tab this repository / user scope · n skip · ↑ ↓ scroll · q stop",
        Kind::Model => "y fetch · n skip · q stop",
        Kind::Learn => "y condition now · n skip · q stop",
        Kind::AutoLearn => "y turn it on · n skip · q stop",
        Kind::Finish => "Enter or q exits",
    }
}

pub fn draw(f: &mut Frame, w: &Wizard, ctx: &Ctx) {
    let t = &ctx.theme;
    let area = f.area();
    let [title, body, keys] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(5), Constraint::Length(1)]).areas(area);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" adage init ", t.title()),
            Span::styled(format!("· set Adage up for {}", ctx.label), t.dim()),
        ])),
        title,
    );
    let left_w = if area.width >= 80 { 24 } else { 18 };
    let [steps, main] = Layout::horizontal([Constraint::Length(left_w), Constraint::Min(20)]).areas(body);
    let lines: Vec<Line> = w
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (mark, style) = match &s.status {
                _ if i == w.cur => ("▸", t.accent().patch(t.bold())),
                StepStatus::Done(_) => ("✓", t.green()),
                StepStatus::Skipped => ("–", t.dim()),
                StepStatus::Failed(_) => ("✗", t.red()),
                StepStatus::Todo => ("·", t.dim()),
            };
            Line::from(vec![Span::styled(format!(" {mark} "), style), Span::styled(s.title(), style)])
        })
        .collect();
    let block = Block::default().borders(Borders::RIGHT).border_style(t.dim());
    let inner = block.inner(steps);
    f.render_widget(block, steps);
    f.render_widget(Paragraph::new(lines), inner);

    let main = main.inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 });
    let (main, gauge) = if w.progress.is_some() {
        let [a, b] = Layout::vertical([Constraint::Min(3), Constraint::Length(2)]).areas(main);
        (a, Some(b))
    } else {
        (main, None)
    };
    f.render_widget(
        Paragraph::new(detail(w, t))
            .block(Block::default().title(Span::styled(w.steps[w.cur].title(), t.title())))
            .wrap(Wrap { trim: false })
            .scroll((w.scroll, 0)),
        main,
    );
    if let (Some(g), Some((status, ratio))) = (gauge, &w.progress) {
        f.render_widget(Gauge::default().gauge_style(t.accent()).ratio(*ratio).label(status.clone()), g);
    }
    f.render_widget(Paragraph::new(Span::styled(format!(" {}", hint(w)), t.dim())), keys);
}

// ---------------------------------------------------------------------------------------------
// Runtime

pub fn run(ctx: Ctx) -> Result<()> {
    let mut w = Wizard::new(detect::harnesses(), detect::registrations(ctx.root.as_deref()), ctx.root.is_some());
    w.model = adage_intent::model::resolve_static(&ctx.paths.models(), false).map(|m| m.id);
    let loaded = config::load(&ctx.paths);
    w.auto_learn = loaded.get("learn.auto").map(|(_, src)| (loaded.config.learn_auto, src.to_string()));
    w.config_path = detect::tilde(&ctx.paths.config());
    let mut job: Option<(Job, Kind)> = None;
    let mut t = Term::enter()?;
    let replan = |w: &mut Wizard| {
        if let Kind::Hooks(h) = w.current().clone() {
            w.plan = Some(crate::install::plan(h, w.scope, ctx.root.as_deref(), None).map_err(|e| format!("{e:#}")));
        }
    };
    loop {
        if let Some((j, kind)) = job.as_mut() {
            match j.poll() {
                Some(done) => {
                    let kind = kind.clone();
                    job = None;
                    w.progress = None;
                    match (kind, done) {
                        (Kind::Model, Ok(lines)) => {
                            w.model = adage_intent::model::resolve_static(&ctx.paths.models(), false).map(|m| m.id);
                            let note = lines.first().cloned().unwrap_or_default();
                            w.advance(StepStatus::Done(note));
                        }
                        (Kind::Learn, Ok(lines)) => {
                            let summary = lines.first().cloned().unwrap_or_default();
                            w.steps[w.cur].status = StepStatus::Done(summary);
                            let n = lines.len();
                            w.output = lines.into_iter().skip(n.saturating_sub(24)).collect();
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
            Cmd::Quit => break,
            Cmd::Replan => {
                replan(&mut w);
                None
            }
            Cmd::Finish => {
                w.health = Some(health::gather(&ctx.paths, ctx.root.as_deref()));
                None
            }
            Cmd::Install(p) => match crate::install::apply_backed_up(&p) {
                Ok(_) => {
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
                w.progress = Some(("reading transcripts…".into(), 0.1));
                None
            }
            // `adage config set`'s own guard and validation.
            Cmd::AutoLearn => match crate::share::require_person("config set")
                .and_then(|()| config::set(&ctx.paths, "learn.auto", "true").map_err(|e| anyhow::anyhow!(e)))
            {
                Ok(_) => {
                    w.auto_learn = Some((true, "file".into()));
                    w.advance(StepStatus::Done(format!("learn.auto = true in {}", w.config_path)))
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
    drop(t);
    for l in w.report() {
        println!("{l}");
    }
    for n in super::take_notices() {
        eprintln!("{n}");
    }
    Ok(())
}
