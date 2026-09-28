//! `adage ui`: the full-screen app. Tabs: Live (the watch view, larger), Reflexes (every reflex
//! and instinct, with review actions), Map (stimulus → reflex → response), Health (every check,
//! live, with one-key fixes), Settings (`config.toml`). A command palette (`:` or ctrl-k) reaches
//! every action and reflex; `?` lists the keys, from the same table as the status bar.
//!
//! [`AppState`] is plain data; [`draw`] renders it and [`AppState::key`] maps a key to a state
//! change or a [`Cmd`] for the runtime ([`run`]) to carry out — writing files, running jobs.
//! Each tab lives in `tabs/`: its keys and its drawing. Every action that changes something asks
//! first, and review decisions and hook installs go through the same functions as `adage review`
//! and `adage install --write`, with the same refusals (effect ceilings, an agent session).

use super::health::{Fix, Health};
use super::jobs::{self, Job, Loaded, Loader, Want};
use super::keys;
use super::palette::{self, Palette};
use super::routes::{RouteRow, Routes};
use super::tabs;
use super::term::{self, Term};
use super::theme::Theme;
use super::view::{self, Ctx, Live};
use super::watch::{self, Sources};
use super::words;
use crate::doctor::Status;
use crate::install::Plan;
use adage_store::config;
use adage_term::Glyph;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Live,
    Reflexes,
    Map,
    Health,
    Settings,
}

pub const TABS: [(Tab, &str); 5] = [
    (Tab::Live, "Live"),
    (Tab::Reflexes, "Reflexes"),
    (Tab::Map, "Map"),
    (Tab::Health, "Health"),
    (Tab::Settings, "Settings"),
];

impl Tab {
    /// This tab's keys, for the status bar and the overlay.
    pub fn keys(self) -> &'static [keys::Key] {
        match self {
            Tab::Live => keys::LIVE,
            Tab::Reflexes => keys::REFLEXES,
            Tab::Map => keys::MAP,
            Tab::Health => keys::HEALTH,
            Tab::Settings => keys::SETTINGS,
        }
    }

    pub fn name(self) -> &'static str {
        TABS.iter().find(|(t, _)| *t == self).map_or("", |(_, n)| n)
    }

    fn wants_routes(self) -> bool {
        matches!(self, Tab::Reflexes | Tab::Map)
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    /// A review decision: `verb` is the one `crate::review` records (`promote`, …).
    Review {
        verb: &'static str,
        reference: String,
        route: String,
    },
    Install(Box<Plan>),
    FetchModel,
    LearnNow,
    SetConfig {
        key: &'static str,
        value: String,
    },
}

pub enum Modal {
    Help { scroll: u16 },
    Confirm { title: String, lines: Vec<Line<'static>>, action: Action, scroll: u16 },
    Message { title: String, lines: Vec<String>, error: bool, scroll: u16 },
    Progress { title: String, status: String, ratio: f64 },
    Viewer { title: String, lines: Vec<String>, scroll: u16 },
    Scope { harness: String },
    Edit { key: &'static str, buffer: String, error: Option<String> },
}

/// What the runtime must do after a key.
#[derive(Debug)]
pub enum Cmd {
    Quit,
    Do(Action),
    PlanInstall { harness: String, scope: &'static str },
    OpenFailOpen,
    Want(WantKind),
}

#[derive(Debug, Clone, Copy)]
pub enum WantKind {
    Routes,
    Health,
}

/// A short note at the bottom of the screen that goes away by itself.
#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub ok: bool,
    /// Frames left.
    pub ttl: u16,
}

/// Frames a toast stays (about four seconds).
pub const TOAST_FRAMES: u16 = 26;

/// One `adage config` key as the Settings tab shows it.
pub struct SettingRow {
    pub key: &'static str,
    pub value: String,
    /// `default`, `file`, or `env VAR`.
    pub source: String,
    pub doc: &'static str,
    pub default: &'static str,
}

impl SettingRow {
    pub fn boolean(&self) -> bool {
        matches!(self.default, "true" | "false")
    }
}

pub struct AppState {
    pub tab: Tab,
    pub live: Live,
    pub health: Option<Health>,
    pub routes: Option<Result<Routes, String>>,
    pub reflexes: tabs::reflexes::State,
    pub map: tabs::map::State,
    pub check_sel: usize,
    pub setting_sel: usize,
    pub settings: Vec<SettingRow>,
    /// Problems `adage_store::config` found in the file.
    pub settings_problems: Vec<String>,
    pub config_path: PathBuf,
    pub modal: Option<Modal>,
    pub palette: Option<Palette>,
    pub toasts: Vec<Toast>,
    pub notices: Vec<String>,
}

impl AppState {
    pub fn new(config_path: PathBuf) -> AppState {
        AppState {
            tab: Tab::Live,
            live: Live::default(),
            health: None,
            routes: None,
            reflexes: tabs::reflexes::State::default(),
            map: tabs::map::State::default(),
            check_sel: 0,
            setting_sel: 0,
            settings: vec![],
            settings_problems: vec![],
            config_path,
            modal: None,
            palette: None,
            toasts: vec![],
            notices: vec![],
        }
    }

    /// The effective configuration, as `adage config` shows it.
    pub fn load_settings(&mut self, loaded: &config::Loaded) {
        self.settings = config::KEYS
            .iter()
            .map(|k| {
                let (value, source) =
                    loaded.get(k.name).cloned().unwrap_or((k.default.to_string(), config::Source::Default));
                SettingRow { key: k.name, value, source: source.to_string(), doc: k.doc, default: k.default }
            })
            .collect();
        self.settings_problems = loaded.problems.clone();
        self.setting_sel = self.setting_sel.min(self.settings.len().saturating_sub(1));
    }

    pub fn route_rows(&self) -> &[RouteRow] {
        match &self.routes {
            Some(Ok(r)) => &r.rows,
            _ => &[],
        }
    }

    /// The reflex selected in the Reflexes tab.
    pub fn selected_route(&self) -> Option<&RouteRow> {
        tabs::reflexes::selected(self)
    }

    pub fn checks_len(&self) -> usize {
        self.health.as_ref().map_or(0, |h| h.checks.len())
    }

    pub fn toast(&mut self, text: impl Into<String>, ok: bool) {
        self.toasts.push(Toast { text: text.into(), ok, ttl: TOAST_FRAMES });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    /// Age the toasts by a frame.
    pub fn tick(&mut self) {
        for t in &mut self.toasts {
            t.ttl = t.ttl.saturating_sub(1);
        }
        self.toasts.retain(|t| t.ttl > 0);
    }

    fn switch(&mut self, tab: Tab) -> Option<Cmd> {
        self.tab = tab;
        tab.wants_routes().then_some(Cmd::Want(WantKind::Routes))
    }

    /// Whether the current tab is taking typed text (a filter), so letters are not commands.
    fn typing(&self) -> bool {
        match self.tab {
            Tab::Live => self.live.filtering,
            Tab::Reflexes => self.reflexes.filtering,
            _ => false,
        }
    }

    /// Map a key to a state change, or a command for the runtime.
    pub fn key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<Cmd> {
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        if code == KeyCode::Char('c') && ctrl {
            return Some(Cmd::Quit);
        }
        if let Some(p) = self.palette.take() {
            return self.palette_key(p, code);
        }
        if let Some(m) = self.modal.take() {
            return self.modal_key(m, code);
        }
        if !self.typing() {
            match code {
                KeyCode::Char('k') if ctrl => {
                    self.palette = Some(Palette::default());
                    return None;
                }
                KeyCode::Char(':') => {
                    self.palette = Some(Palette::default());
                    return None;
                }
                KeyCode::Char('q') => return Some(Cmd::Quit),
                KeyCode::Char('?') => {
                    self.modal = Some(Modal::Help { scroll: 0 });
                    return None;
                }
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('1'..='5') => {
                    let i = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0);
                    let next = match code {
                        KeyCode::Tab => (i + 1) % TABS.len(),
                        KeyCode::BackTab => (i + TABS.len() - 1) % TABS.len(),
                        KeyCode::Char(c) => c as usize - '1' as usize,
                        _ => i,
                    };
                    return self.switch(TABS[next].0);
                }
                _ => {}
            }
        }
        match self.tab {
            Tab::Live => {
                let open =
                    self.live.inspect || self.live.map_hint || self.live.filtering || !self.live.filter.is_empty();
                if code == KeyCode::Esc && !open {
                    return None;
                }
                watch::live_key(&mut self.live, code, mods);
                None
            }
            Tab::Reflexes => tabs::reflexes::key(self, code),
            Tab::Map => tabs::map::key(self, code),
            Tab::Health => tabs::health::key(self, code),
            Tab::Settings => tabs::settings::key(self, code),
        }
    }

    fn palette_key(&mut self, mut p: Palette, code: KeyCode) -> Option<Cmd> {
        let found = palette::matches(self, &p.query);
        match code {
            KeyCode::Esc => return None,
            KeyCode::Up => p.sel = p.sel.saturating_sub(1),
            KeyCode::Down => p.sel = (p.sel + 1).min(found.len().saturating_sub(1)),
            KeyCode::Tab => {
                if let Some((e, _)) = found.get(p.sel) {
                    p.query = e.title.trim_end_matches('…').to_string();
                    p.sel = 0;
                }
            }
            KeyCode::Backspace => {
                p.query.pop();
                p.sel = 0;
            }
            KeyCode::Enter => {
                let (e, _) = found.into_iter().nth(p.sel)?;
                return self.run_entry(e.run);
            }
            KeyCode::Char(c) if p.query.chars().count() < 60 => {
                p.query.push(c);
                p.sel = 0;
            }
            _ => {}
        }
        self.palette = Some(p);
        None
    }

    fn run_entry(&mut self, run: palette::Run) -> Option<Cmd> {
        match run {
            palette::Run::Tab(t) => self.switch(t),
            palette::Run::Setting(i) => {
                self.setting_sel = i.min(self.settings.len().saturating_sub(1));
                self.switch(Tab::Settings)
            }
            palette::Run::Reflex(reference, id) => {
                tabs::reflexes::select(self, &reference, &id);
                self.switch(Tab::Reflexes)
            }
            palette::Run::Key(tab, c) => {
                let want = self.switch(tab);
                let cmd = self.key(KeyCode::Char(c), KeyModifiers::NONE);
                cmd.or(want)
            }
        }
    }

    fn modal_key(&mut self, m: Modal, code: KeyCode) -> Option<Cmd> {
        let nudge = |s: &mut u16| match code {
            KeyCode::Up | KeyCode::Char('k') => *s = s.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => *s = s.saturating_add(1),
            KeyCode::PageUp => *s = s.saturating_sub(10),
            KeyCode::PageDown => *s = s.saturating_add(10),
            _ => {}
        };
        match m {
            Modal::Help { mut scroll } => match code {
                // Scrolls when taller than the screen; any other key closes it.
                KeyCode::Up
                | KeyCode::Char('k')
                | KeyCode::Down
                | KeyCode::Char('j')
                | KeyCode::PageUp
                | KeyCode::PageDown => {
                    nudge(&mut scroll);
                    self.modal = Some(Modal::Help { scroll });
                    None
                }
                _ => None,
            },
            Modal::Confirm { title, lines, action, mut scroll } => match code {
                KeyCode::Char('y') | KeyCode::Enter => Some(Cmd::Do(action)),
                KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => None,
                _ => {
                    nudge(&mut scroll);
                    self.modal = Some(Modal::Confirm { title, lines, action, scroll });
                    None
                }
            },
            Modal::Message { title, lines, error, scroll: mut s } => {
                if matches!(code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char(' ')) {
                    return None;
                }
                nudge(&mut s);
                self.modal = Some(Modal::Message { title, lines, error, scroll: s });
                None
            }
            Modal::Viewer { title, lines, scroll: mut s } => {
                if matches!(code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                    return None;
                }
                nudge(&mut s);
                self.modal = Some(Modal::Viewer { title, lines, scroll: s });
                None
            }
            Modal::Progress { .. } => {
                self.modal = Some(m);
                None
            }
            Modal::Scope { harness } => match code {
                KeyCode::Char('p') | KeyCode::Char('r') => Some(Cmd::PlanInstall { harness, scope: "project" }),
                KeyCode::Char('u') => Some(Cmd::PlanInstall { harness, scope: "user" }),
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => None,
                _ => {
                    self.modal = Some(Modal::Scope { harness });
                    None
                }
            },
            Modal::Edit { key, mut buffer, .. } => match code {
                KeyCode::Esc => None,
                // Validated by `adage_store::config::set`; an error reopens the editor.
                KeyCode::Enter => Some(Cmd::Do(Action::SetConfig { key, value: buffer })),
                KeyCode::Backspace => {
                    buffer.pop();
                    self.modal = Some(Modal::Edit { key, buffer, error: None });
                    None
                }
                KeyCode::Char(c) if buffer.len() < 64 => {
                    buffer.push(c);
                    self.modal = Some(Modal::Edit { key, buffer, error: None });
                    None
                }
                _ => {
                    self.modal = Some(Modal::Edit { key, buffer, error: None });
                    None
                }
            },
        }
    }

    pub fn confirm(&mut self, title: &str, lines: Vec<Line<'static>>, action: Action) {
        self.modal = Some(Modal::Confirm { title: title.to_string(), lines, action, scroll: 0 });
    }

    pub fn message(&mut self, title: &str, lines: Vec<String>, error: bool) {
        self.modal = Some(Modal::Message { title: title.to_string(), lines, error, scroll: 0 });
    }

    pub fn start_fix(&mut self, fix: Fix) -> Option<Cmd> {
        let t = Theme::from_env();
        let hint = Line::from(vec![
            Span::styled("y", t.key()),
            Span::styled(format!(" confirm{}", t.sep()), t.dim()),
            Span::styled("n", t.key()),
            Span::styled(" cancel", t.dim()),
        ]);
        let same = |cmd: &str| {
            Line::from(vec![
                Span::styled("same as ", t.dim()),
                Span::styled(cmd.to_string(), t.command()),
                Span::styled(" from any shell", t.dim()),
            ])
        };
        match fix {
            Fix::InstallHooks(h) => {
                self.modal = Some(Modal::Scope { harness: h });
                None
            }
            Fix::OpenFailOpen => Some(Cmd::OpenFailOpen),
            Fix::FetchModel => {
                self.confirm(
                    "Fetch the intent model",
                    vec![
                        Line::raw("Download potion-base-8M (31 MB, SHA-256 verified) with curl,"),
                        Line::raw("into Adage's data directory. It lets conditioning find intent families."),
                        Line::raw(""),
                        same("adage models fetch"),
                        hint,
                    ],
                    Action::FetchModel,
                );
                None
            }
            Fix::LearnNow => {
                self.confirm(
                    "Condition now",
                    vec![
                        Line::raw("Condition this repository now, exactly as a background conditioner does:"),
                        Line::raw("read its traces, form reflexes, update its tracks, recompile the index,"),
                        Line::raw("and log what changed."),
                        Line::raw(""),
                        same("adage condition --auto --repo-root <repo>"),
                        hint,
                    ],
                    Action::LearnNow,
                );
                None
            }
            Fix::OpenSettings => {
                self.tab = Tab::Settings;
                None
            }
            Fix::EnableAutoLearn => {
                self.confirm_auto_condition(true);
                None
            }
        }
    }

    /// Ask before turning auto-condition on or off: the Health fix and the Settings toggle say
    /// the same thing.
    pub fn confirm_auto_condition(&mut self, on: bool) {
        let t = Theme::from_env();
        let (title, value, what) = if on {
            ("Turn auto-condition on", "true", "Adage then conditions in the background")
        } else {
            ("Turn auto-condition off", "false", "Adage then no longer conditions in the background")
        };
        self.confirm(
            title,
            vec![
                Line::raw(format!("Set learn.auto = {value} in config.toml: {what}")),
                Line::raw("whenever an agent's trace in a repository stops or ends."),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("same as ", t.dim()),
                    Span::styled(format!("adage config set learn.auto {value}"), t.command()),
                    Span::styled(" from any shell", t.dim()),
                ]),
                Line::from(vec![
                    Span::styled("y", t.key()),
                    Span::styled(format!(" confirm{}", t.sep()), t.dim()),
                    Span::styled("n", t.key()),
                    Span::styled(" cancel", t.dim()),
                ]),
            ],
            Action::SetConfig { key: "learn.auto", value: value.into() },
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

pub fn draw(f: &mut Frame, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let area = f.area();
    if area.width < 40 || area.height < 10 {
        f.render_widget(
            Paragraph::new(t.text("adage ui: make the terminal larger (40×10 at least)")).wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let [tabs_a, rule_a, body, rule_b, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_tabs(f, tabs_a, s, ctx);
    f.render_widget(Paragraph::new(view::rule("", None, area.width, t)), rule_a);
    match s.tab {
        Tab::Live => view::draw_live(f, body, &s.live, s.health.as_ref(), ctx, false),
        Tab::Reflexes => tabs::reflexes::draw(f, body, s, ctx),
        Tab::Map => tabs::map::draw(f, body, s, ctx),
        Tab::Health => tabs::health::draw(f, body, s, ctx),
        Tab::Settings => tabs::settings::draw(f, body, s, ctx),
    }
    draw_toasts(f, body, s, t);
    f.render_widget(Paragraph::new(view::rule("", None, area.width, t)), rule_b);
    draw_status(f, status, s, ctx);
    if let Some(m) = &s.modal {
        draw_modal(f, body, m, s, t);
    }
    if let Some(p) = &s.palette {
        palette::draw(f, area, s, p, t);
    }
}

fn draw_tabs(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    // Narrow terminals name only the current tab (the others keep their number), so where you
    // are is never cut off the edge.
    let tabs = |compact: bool| {
        let mut spans = vec![Span::styled(" adage", t.title()), Span::raw(if compact { " " } else { "   " })];
        for (i, (tab, name)) in TABS.iter().enumerate() {
            let badge = match tab {
                Tab::Health => s.health.as_ref().map_or(0, |h| h.count(Status::Warn) + h.count(Status::Fail)),
                Tab::Reflexes => match &s.routes {
                    Some(Ok(r)) => r.waiting(),
                    _ => 0,
                },
                _ => 0,
            };
            if *tab == s.tab {
                spans.push(Span::styled(format!(" {} {name} ", i + 1), t.accent().patch(t.selected()).patch(t.bold())));
            } else if compact {
                spans.push(Span::styled(format!(" {}", i + 1), t.ink2()));
            } else {
                spans.push(Span::styled(format!(" {} ", i + 1), t.dim()));
                spans.push(Span::styled(format!("{name} "), t.ink2()));
            }
            if badge > 0 {
                spans.push(Span::styled(format!("({badge})"), t.yellow()));
            }
            if !compact {
                spans.push(Span::raw(" "));
            }
        }
        spans
    };
    let full = tabs(false);
    let mut spans =
        if full.iter().map(|s| s.width()).sum::<usize>() <= area.width as usize { full } else { tabs(true) };
    let mut right = vec![Span::styled(format!("{}{}", ctx.label, t.sep()), t.ink2())];
    if ctx.demo {
        right.insert(0, Span::styled(format!("demo{}", t.sep()), t.holdout()));
    }
    if s.live.paused {
        right.push(Span::styled(
            format!("{} paused +{}", t.ch("‖", "="), s.live.held.len()),
            t.yellow().patch(t.bold()),
        ));
    } else if t.color {
        right.push(Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.green()));
        right.push(Span::raw("live"));
    } else {
        right.push(Span::styled("LIVE", t.bold()));
    }
    right.push(Span::raw(" "));
    let lw: usize = spans.iter().map(|s| s.width()).sum();
    let rw: usize = right.iter().map(|s| s.width()).sum();
    if lw + rw < area.width as usize {
        spans.push(Span::raw(" ".repeat(area.width as usize - lw - rw)));
        spans.extend(right);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_status(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let room = (area.width as usize).saturating_sub(9);
    let mut spans = vec![Span::raw(" ")];
    let typing = match s.tab {
        Tab::Live if s.live.filtering => Some(&s.live.filter),
        Tab::Reflexes if s.reflexes.filtering => Some(&s.reflexes.filter),
        _ => None,
    };
    if let Some(q) = typing {
        spans.push(Span::styled("/ ", t.key()));
        spans.push(Span::raw(q.clone()));
        spans.push(Span::styled(t.ch("▌", "_"), t.accent()));
        spans.push(Span::styled(format!("   enter keep{}esc clear", t.sep()), t.dim()));
    } else {
        spans.extend(keys::bar(&[keys::GLOBAL, s.tab.keys()], t, room));
    }
    let used: usize = spans.iter().map(|s| s.width()).sum();
    spans.push(Span::raw(" ".repeat((area.width as usize).saturating_sub(used + 7))));
    spans.push(Span::styled("q", t.key()));
    spans.push(Span::styled(" quit ", t.dim()));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_toasts(f: &mut Frame, body: Rect, s: &AppState, t: &Theme) {
    for (k, toast) in s.toasts.iter().rev().enumerate() {
        let y = body.bottom().saturating_sub(1 + k as u16);
        if y < body.top() {
            break;
        }
        let (mark, style) = if toast.ok { (t.glyph(Glyph::Ok), t.green()) } else { (t.glyph(Glyph::Fail), t.red()) };
        let text = t.fit(&t.text(&toast.text), (body.width as usize).saturating_sub(8));
        let line =
            Line::from(vec![Span::styled(format!(" {mark} "), style.patch(t.bold())), Span::raw(format!("{text} "))]);
        let w = (line.width() as u16).min(body.width);
        let r = Rect { x: body.right().saturating_sub(w + 1), y, width: w, height: 1 };
        f.render_widget(ratatui::widgets::Clear, r);
        f.render_widget(Paragraph::new(line), r);
    }
}

fn app_help(tab: Tab, t: &Theme) -> Vec<Line<'static>> {
    let mut v = keys::overlay(&[("everywhere", keys::GLOBAL), (tab.name(), tab.keys())], t);
    if tab == Tab::Live {
        v.push(Line::raw(""));
        v.extend(view::legend(t));
    }
    v
}

fn draw_modal(f: &mut Frame, area: Rect, m: &Modal, s: &AppState, t: &Theme) {
    match m {
        Modal::Help { scroll } => view::popup_keys(f, area, app_help(s.tab, t), t, *scroll),
        Modal::Confirm { title, lines, scroll, .. } => view::popup(f, area, title, lines.clone(), t, *scroll),
        Modal::Message { title, lines, error, scroll } => {
            let style = if *error { t.red() } else { Style::default() };
            let mut ls: Vec<Line> = lines.iter().map(|l| Line::from(Span::styled(l.clone(), style))).collect();
            ls.push(Line::raw(""));
            ls.push(Line::from(Span::styled("enter or esc closes", t.dim())));
            view::popup(f, area, title, ls, t, *scroll)
        }
        Modal::Viewer { title, lines, scroll } => {
            let mut ls: Vec<Line> = lines.iter().map(|l| Line::raw(l.clone())).collect();
            ls.push(Line::from(Span::styled(format!("{} scroll{}esc closes", t.arrows(), t.sep()), t.dim())));
            view::popup(f, area, title, ls, t, *scroll)
        }
        Modal::Progress { title, status, ratio } => {
            let r = view::centered(area, 64.min(area.width.saturating_sub(2)), 5);
            f.render_widget(ratatui::widgets::Clear, r);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_set(t.border())
                .border_style(t.accent())
                .title(Span::styled(format!(" {} ", t.text(title)), t.command().patch(t.bold())));
            let inner = block.inner(r);
            f.render_widget(block, r);
            let [g, st] = Layout::vertical([Constraint::Length(1), Constraint::Length(2)]).areas(inner);
            // The bar as the wizard draws it, so ADAGE_ASCII reaches it too.
            let pct = format!(" {:>3.0}%", ratio.clamp(0.0, 1.0) * 100.0);
            let bar_w = (g.width as usize).saturating_sub(pct.len());
            let done = (ratio.clamp(0.0, 1.0) * bar_w as f64).round() as usize;
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(t.ch("━", "=").repeat(done), t.accent()),
                    Span::styled(t.ch("─", "-").repeat(bar_w - done), t.dim()),
                    Span::styled(pct, t.dim()),
                ])),
                g,
            );
            f.render_widget(Paragraph::new(Span::styled(t.text(status), t.dim())).wrap(Wrap { trim: true }), st);
        }
        Modal::Scope { harness } => view::popup(
            f,
            area,
            &format!("Install {harness} hooks"),
            vec![
                Line::raw("Where should Adage's hooks be registered?"),
                Line::raw(""),
                Line::from(vec![Span::styled("  p  ", t.key()), Span::raw("this repository only")]),
                Line::from(vec![Span::styled("  u  ", t.key()), Span::raw("user scope: every repository")]),
                Line::raw(""),
                Line::from(Span::styled(
                    "You will see the exact file and diff before anything is written. esc cancels.",
                    t.dim(),
                )),
            ],
            t,
            0,
        ),
        Modal::Edit { key, buffer, error } => {
            let doc = config::key(key).map(|k| k.doc).unwrap_or("");
            let mut ls = vec![
                Line::from(Span::styled(key.to_string(), t.bold())),
                Line::from(Span::styled(doc.to_string(), t.dim())),
                Line::raw(""),
                Line::from(vec![Span::raw("> "), Span::styled(format!("{buffer}{}", t.ch("▏", "_")), t.accent())]),
            ];
            if let Some(e) = error {
                ls.push(Line::from(Span::styled(e.clone(), t.red())));
            }
            ls.push(Line::raw(""));
            ls.push(Line::from(vec![
                Span::styled("same as ", t.dim()),
                Span::styled(format!("adage config set {key} <value>"), t.command()),
            ]));
            ls.push(Line::from(Span::styled(format!("enter saves{}esc cancels", t.sep()), t.dim())));
            view::popup(f, area, "Edit setting", ls, t, 0)
        }
    }
}

/// The confirmation for an install: the exact file and a diff of what changes.
pub fn install_confirm(p: &Plan, t: &Theme) -> Vec<Line<'static>> {
    use super::diff::{self, Op};
    let mut lines = vec![
        Line::from(vec![Span::raw("Write "), Span::styled(p.target.display().to_string(), t.bold())]),
        Line::from(Span::styled(
            if p.before.is_some() {
                "It exists: Adage's own entries are replaced, everything else is kept."
            } else {
                "It does not exist yet: it will be created."
            },
            t.dim(),
        )),
        Line::raw(""),
    ];
    let d = diff::lines(p.before.as_deref().unwrap_or(""), &p.after);
    if d.iter().all(|(o, _)| *o == Op::Keep) {
        lines.push(Line::from(Span::styled("No change: it already holds exactly this.", t.green())));
    }
    for (op, l) in d {
        lines.push(match op {
            Op::Keep => Line::from(Span::styled(format!("  {l}"), t.dim())),
            Op::Add => Line::from(Span::styled(format!("+ {l}"), t.green())),
            Op::Remove => Line::from(Span::styled(format!("- {l}"), t.red())),
        });
    }
    lines.push(Line::raw(""));
    if p.harness == "codex" {
        lines.push(Line::raw("Codex then asks you to approve new hooks: open /hooks in Codex."));
    }
    lines.push(Line::from(Span::styled(format!("y write{s}n cancel{s}{} scroll", t.arrows(), s = t.sep()), t.dim())));
    lines
}

// ---------------------------------------------------------------------------------------------
// Runtime

/// Carry out a command. Returns `true` to quit.
fn execute(cmd: Cmd, s: &mut AppState, ctx: &Ctx, loader: &Loader, job: &mut Option<Job>) -> bool {
    let t = &ctx.theme;
    match cmd {
        Cmd::Quit => return true,
        Cmd::Want(WantKind::Routes) => loader.want(Want::Routes),
        Cmd::Want(WantKind::Health) => loader.want(Want::Health),
        Cmd::OpenFailOpen => {
            let p = ctx.paths.root.join(crate::doctor::FAIL_OPEN_LOG);
            let lines: Vec<String> = match std::fs::read_to_string(&p) {
                Ok(text) => {
                    let all: Vec<String> = text.lines().map(pretty_fail_open).collect();
                    all[all.len().saturating_sub(200)..].to_vec()
                }
                Err(_) => vec!["No fail-open errors recorded.".into()],
            };
            let scroll = lines.len().saturating_sub(10) as u16;
            s.modal = Some(Modal::Viewer { title: crate::detect::tilde(&p), lines, scroll });
        }
        Cmd::PlanInstall { harness, scope } => match crate::install::plan(&harness, scope, ctx.root.as_deref(), None) {
            Ok(p) => {
                let lines = install_confirm(&p, t);
                s.confirm(&format!("Install {harness} hooks ({scope})"), lines, Action::Install(Box::new(p)));
            }
            Err(e) => s.message("Cannot install", vec![format!("{e:#}")], true),
        },
        Cmd::Do(action) => match action {
            Action::Review { verb, reference, route } => {
                // `crate::review` records the stored verb; people see the lexicon's.
                match crate::review::decide_quietly(verb, &reference, &route, &ctx.paths) {
                    Ok(_) => s.toast(format!("{route}: {} recorded", words::verb(verb)), true),
                    Err(e) => s.message("Refused", vec![format!("{e:#}")], true),
                }
                loader.want(Want::Routes);
                loader.want(Want::Health);
            }
            Action::Install(p) => {
                match crate::install::apply(&p) {
                    Ok(()) => {
                        let mut lines = vec![format!("wrote {}", p.target.display())];
                        if p.harness == "codex" {
                            lines.push(
                                "Codex asks you to review new hooks before they run: open /hooks in Codex.".into(),
                            );
                        }
                        s.message("Hooks installed", lines, false)
                    }
                    Err(e) => s.message("Not written", vec![format!("{e:#}")], true),
                }
                loader.want(Want::Health);
            }
            Action::FetchModel => {
                let j = jobs::fetch_model(ctx.paths.clone());
                s.modal = Some(Modal::Progress { title: j.title.clone(), status: j.status.clone(), ratio: 0.0 });
                *job = Some(j);
            }
            Action::LearnNow => {
                let j = jobs::learn_now(ctx.paths.clone(), ctx.root.clone());
                s.modal = Some(Modal::Progress { title: j.title.clone(), status: j.status.clone(), ratio: 0.0 });
                *job = Some(j);
            }
            Action::SetConfig { key, value } => {
                // `adage config set`'s own guard and validation.
                let set = crate::share::require_person("config set")
                    .and_then(|()| config::set(&ctx.paths, key, &value).map_err(|e| anyhow::anyhow!(e)));
                let loaded = config::load(&ctx.paths);
                s.load_settings(&loaded);
                match set {
                    Ok(stored) => {
                        if let Some((v, src @ config::Source::Env(_))) = loaded.get(key) {
                            s.message(
                                "Saved, but overridden",
                                vec![format!("{key} = {stored} is in config.toml, but {src} overrides it: the effective value is {v}.")],
                                false,
                            );
                        } else {
                            s.toast(format!("{key} = {stored}"), true);
                        }
                    }
                    Err(e) => s.modal = Some(Modal::Edit { key, buffer: value, error: Some(format!("{e:#}")) }),
                }
                loader.want(Want::Health);
            }
        },
    }
    false
}

fn pretty_fail_open(line: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(v) => {
            let when = v["ts"].as_u64().map(|ts| view::clock(ts, view::local_offset(), true)).unwrap_or_default();
            format!(
                "{when} {} {} {}: {}",
                v["harness"].as_str().unwrap_or("?"),
                v["event"].as_str().unwrap_or("?"),
                v["class"].as_str().unwrap_or(""),
                v["error"].as_str().unwrap_or(line)
            )
        }
        Err(_) => line.to_string(),
    }
}

pub fn run(ctx: Ctx, mut before_tick: Option<Box<dyn FnMut()>>) -> Result<()> {
    let loader = Loader::spawn(ctx.paths.clone(), ctx.root.clone());
    // The Live tab's review count and the palette's reflexes need them from the start.
    loader.want(Want::Routes);
    let mut sources = Sources::new(&ctx);
    let mut s = AppState::new(ctx.paths.config());
    s.load_settings(&config::load(&ctx.paths));
    let mut job: Option<Job> = None;
    let mut ctx = ctx;
    let mut t = Term::enter()?;
    let mut ticks = 0u64;
    loop {
        if let Some(f) = before_tick.as_mut() {
            f();
        }
        while let Ok(m) = loader.rx.try_recv() {
            match m {
                Loaded::Health(h) => {
                    s.health = Some(*h);
                    let n = s.checks_len();
                    s.check_sel = s.check_sel.min(n.saturating_sub(1));
                    ctx.tz = view::local_offset();
                }
                Loaded::Routes(r) => {
                    if let Ok(rs) = &r {
                        s.live.waiting = Some(rs.waiting());
                    }
                    s.routes = Some(r);
                }
            }
        }
        if let Some(j) = job.as_mut() {
            match j.poll() {
                Some(done) => {
                    let title = j.title.clone();
                    job = None;
                    s.modal = None;
                    match done {
                        Ok(lines) => {
                            let last = lines.iter().rev().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default();
                            s.toast(format!("{title}: {}", if last.is_empty() { "done".into() } else { last }), true);
                        }
                        Err(e) => s.message(&title, vec![e], true),
                    }
                    loader.want(Want::Health);
                    loader.want(Want::Routes);
                }
                None => {
                    if let Some(Modal::Progress { status, ratio, .. }) = &mut s.modal {
                        *status = j.status.clone();
                        *ratio = j.ratio;
                    }
                }
            }
        }
        let key = s.health.as_ref().and_then(|h| h.repo_key.clone());
        sources.poll(&mut s.live, key.as_deref());
        s.live.tick_pulse(Instant::now(), ctx.theme.motion);
        ticks += 1;
        s.tick();
        if ticks.is_multiple_of(20) {
            s.load_settings(&config::load(&ctx.paths));
        }
        s.notices.extend(super::take_notices());
        if s.notices.len() > 20 {
            let extra = s.notices.len() - 20;
            s.notices.drain(..extra);
        }
        t.terminal.draw(|f| draw(f, &s, &ctx))?;
        if let Some(k) = term::next_key_within(watch::wait(&s.live))? {
            if let Some(cmd) = s.key(k.code, k.modifiers) {
                if execute(cmd, &mut s, &ctx, &loader, &mut job) {
                    break;
                }
            }
        }
    }
    drop(t);
    Ok(())
}
