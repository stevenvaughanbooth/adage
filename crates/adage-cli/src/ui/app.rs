//! `adage ui`: the full-screen app. Tabs: Live (the watch view, larger), Routes (every installed
//! route, with review actions), Health (every check, live, with one-key fixes), Settings
//! (`config.toml`).
//!
//! [`AppState`] is plain data; [`draw`] renders it and [`AppState::key`] maps a key to a state
//! change or a [`Cmd`] for the runtime ([`run`]) to carry out — writing files, running jobs.
//! Every action that changes something asks first, and review decisions and hook installs go
//! through the same functions as `adage review` and `adage install --write`, with the same
//! refusals (effect ceilings, an agent session).

use super::diff::{self, Op};
use super::health::{Fix, Health};
use super::jobs::{self, Job, Loaded, Loader, Want};
use super::routes::{self, RouteRow, Routes};
use super::term::{self, Term};
use super::theme::{Lamp, Theme};
use super::view::{self, Ctx, Live};
use super::watch::{self, Sources};
use crate::doctor::Status;
use crate::install::Plan;
use adage_store::config;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Wrap};
use ratatui::Frame;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Live,
    Routes,
    Health,
    Settings,
}

const TABS: [(Tab, &str); 4] =
    [(Tab::Live, "Live"), (Tab::Routes, "Routes"), (Tab::Health, "Health"), (Tab::Settings, "Settings")];

#[derive(Debug, Clone)]
pub enum Action {
    Review { verb: &'static str, reference: String, route: String },
    Install(Box<Plan>),
    FetchModel,
    LearnNow,
    SetConfig { key: &'static str, value: String },
}

pub enum Modal {
    Help,
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
    fn boolean(&self) -> bool {
        matches!(self.default, "true" | "false")
    }
}

pub struct AppState {
    pub tab: Tab,
    pub live: Live,
    pub health: Option<Health>,
    pub routes: Option<Result<Routes, String>>,
    pub route_sel: usize,
    pub detail_scroll: u16,
    pub check_sel: usize,
    pub setting_sel: usize,
    pub settings: Vec<SettingRow>,
    /// Problems `adage_store::config` found in the file.
    pub settings_problems: Vec<String>,
    pub config_path: PathBuf,
    pub modal: Option<Modal>,
    pub notices: Vec<String>,
}

impl AppState {
    pub fn new(config_path: PathBuf) -> AppState {
        AppState {
            tab: Tab::Live,
            live: Live::default(),
            health: None,
            routes: None,
            route_sel: 0,
            detail_scroll: 0,
            check_sel: 0,
            setting_sel: 0,
            settings: vec![],
            settings_problems: vec![],
            config_path,
            modal: None,
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

    fn route_rows(&self) -> &[RouteRow] {
        match &self.routes {
            Some(Ok(r)) => &r.rows,
            _ => &[],
        }
    }

    pub fn selected_route(&self) -> Option<&RouteRow> {
        self.route_rows().get(self.route_sel)
    }

    fn checks_len(&self) -> usize {
        self.health.as_ref().map_or(0, |h| h.checks.len())
    }

    /// Map a key to a state change, or a command for the runtime.
    pub fn key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<Cmd> {
        if code == KeyCode::Char('c') && mods.contains(KeyModifiers::CONTROL) {
            return Some(Cmd::Quit);
        }
        if let Some(m) = self.modal.take() {
            return self.modal_key(m, code);
        }
        match code {
            KeyCode::Char('q') => return Some(Cmd::Quit),
            KeyCode::Char('?') => {
                self.modal = Some(Modal::Help);
                return None;
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('1'..='4') => {
                let i = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0);
                let next = match code {
                    KeyCode::Tab => (i + 1) % TABS.len(),
                    KeyCode::BackTab => (i + TABS.len() - 1) % TABS.len(),
                    KeyCode::Char(c) => c as usize - '1' as usize,
                    _ => i,
                };
                self.tab = TABS[next].0;
                return (self.tab == Tab::Routes).then_some(Cmd::Want(WantKind::Routes));
            }
            _ => {}
        }
        match self.tab {
            Tab::Live => {
                if code == KeyCode::Esc {
                    return None;
                }
                watch::live_key(&mut self.live, code, mods);
                None
            }
            Tab::Routes => self.routes_key(code),
            Tab::Health => self.health_key(code),
            Tab::Settings => self.settings_key(code),
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
            Modal::Help => None,
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

    fn confirm(&mut self, title: &str, lines: Vec<Line<'static>>, action: Action) {
        self.modal = Some(Modal::Confirm { title: title.to_string(), lines, action, scroll: 0 });
    }

    fn message(&mut self, title: &str, lines: Vec<String>, error: bool) {
        self.modal = Some(Modal::Message { title: title.to_string(), lines, error, scroll: 0 });
    }

    fn routes_key(&mut self, code: KeyCode) -> Option<Cmd> {
        let n = self.route_rows().len();
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.route_sel = self.route_sel.saturating_sub(1);
                self.detail_scroll = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.route_sel = (self.route_sel + 1).min(n.saturating_sub(1));
                self.detail_scroll = 0;
            }
            KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(5),
            KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(5),
            KeyCode::Char('R') => return Some(Cmd::Want(WantKind::Routes)),
            KeyCode::Char(c) => {
                let (_, verb) = *routes::ACTIONS.iter().find(|(k, _)| *k == c)?;
                let r = self.selected_route()?.clone();
                let Some(verb) = r.verbs.iter().copied().find(|v| *v == verb) else {
                    self.message(
                        "Not available",
                        vec![
                            format!("`{verb}` does not apply to {} as it stands.", r.id),
                            format!("Available: {}", r.verbs.join(", ")),
                        ],
                        true,
                    );
                    return None;
                };
                let t = Theme::from_env();
                let lines = vec![
                    Line::from(vec![
                        Span::raw(format!("{verb} ")),
                        Span::styled(r.id.clone(), t.bold()),
                        Span::raw(format!(" in {}?", r.reference)),
                    ]),
                    Line::raw(""),
                    Line::raw(verb_effect(verb)),
                    Line::raw("The decision is recorded in the track's review.yaml and the index is recompiled,"),
                    Line::raw("exactly as `adage review` does."),
                    Line::raw(""),
                    Line::from(Span::styled("y confirm · n cancel", t.dim())),
                ];
                self.confirm(
                    "Review",
                    lines,
                    Action::Review { verb, reference: r.reference.clone(), route: r.id.clone() },
                );
            }
            _ => {}
        }
        None
    }

    fn health_key(&mut self, code: KeyCode) -> Option<Cmd> {
        let n = self.checks_len();
        let fix_of = |s: &AppState, want: &dyn Fn(&Fix) -> bool| -> Option<Fix> {
            let h = s.health.as_ref()?;
            let sel = h.fixes.get(s.check_sel).cloned().flatten().filter(|f| want(f));
            sel.or_else(|| h.fixes.iter().flatten().find(|f| want(f)).cloned())
        };
        let fix = match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.check_sel = self.check_sel.saturating_sub(1);
                return None;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.check_sel = (self.check_sel + 1).min(n.saturating_sub(1));
                return None;
            }
            KeyCode::Char('R') => return Some(Cmd::Want(WantKind::Health)),
            KeyCode::Enter => self.health.as_ref()?.fixes.get(self.check_sel).cloned().flatten(),
            KeyCode::Char('i') => fix_of(self, &|f| matches!(f, Fix::InstallHooks(_))),
            KeyCode::Char('m') => Some(Fix::FetchModel),
            KeyCode::Char('l') => Some(Fix::LearnNow),
            KeyCode::Char('s') => Some(Fix::OpenSettings),
            KeyCode::Char('o') => Some(Fix::OpenFailOpen),
            _ => None,
        }?;
        self.start_fix(fix)
    }

    pub fn start_fix(&mut self, fix: Fix) -> Option<Cmd> {
        let t = Theme::from_env();
        let hint = Line::from(Span::styled("y confirm · n cancel", t.dim()));
        match fix {
            Fix::InstallHooks(h) => {
                self.modal = Some(Modal::Scope { harness: h });
                None
            }
            Fix::OpenFailOpen => Some(Cmd::OpenFailOpen),
            Fix::FetchModel => {
                self.confirm(
                    "Fetch the embedding model",
                    vec![
                        Line::raw("Download potion-base-8M (31 MB, SHA-256 verified) with curl,"),
                        Line::raw("into Adage's data directory. It lets `adage learn` find intent families."),
                        Line::raw(""),
                        hint,
                    ],
                    Action::FetchModel,
                );
                None
            }
            Fix::LearnNow => {
                self.confirm(
                    "Learn now",
                    vec![
                        Line::raw("Learn this repository now, exactly as a background learner does"),
                        Line::raw("(`adage learn --auto --repo-root <repo>`): read its sessions, mine routes,"),
                        Line::raw("update its tracks, recompile the index, and log what changed."),
                        Line::raw(""),
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
                self.confirm(
                    "Turn auto-learn on",
                    vec![
                        Line::raw("Set learn.auto = true in config.toml: Adage then learns in the background"),
                        Line::raw("whenever an agent session in a repository stops or ends."),
                        Line::raw(""),
                        hint,
                    ],
                    Action::SetConfig { key: "learn.auto", value: "true".into() },
                );
                None
            }
        }
    }

    fn settings_key(&mut self, code: KeyCode) -> Option<Cmd> {
        let n = self.settings.len();
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.setting_sel = self.setting_sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.setting_sel = (self.setting_sel + 1).min(n.saturating_sub(1)),
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('e') => {
                let row = self.settings.get(self.setting_sel)?;
                if row.boolean() {
                    let next = if row.value == "true" { "false" } else { "true" };
                    return Some(Cmd::Do(Action::SetConfig { key: row.key, value: next.into() }));
                }
                if row.key == "learn.thresholds" && code == KeyCode::Char(' ') {
                    let next = if row.value == "demo" { "default" } else { "demo" };
                    return Some(Cmd::Do(Action::SetConfig { key: row.key, value: next.into() }));
                }
                self.modal = Some(Modal::Edit { key: row.key, buffer: row.value.clone(), error: None });
            }
            _ => {}
        }
        None
    }
}

fn aspect_name(a: adage_tracks::Aspect) -> &'static str {
    adage_tracks::review::aspect_str(a)
}

fn verb_effect(verb: &str) -> &'static str {
    match verb {
        "approve" => "Approve: accept the route as it is (or apply its proposed change, or put a draft into service).",
        "reject" => "Reject: retire the route; `adage learn` keeps it retired unless it strengthens materially.",
        "demote" => "Demote: green → yellow, held across re-learning.",
        "promote" => "Promote: yellow → green, never above the route's effect ceiling.",
        "pin" => "Pin: mark it authored; learning proposes changes instead of overwriting it.",
        "dismiss" => "Dismiss: keep the pinned route and silence its proposed change.",
        _ => "",
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

pub fn draw(f: &mut Frame, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let area = f.area();
    if area.width < 40 || area.height < 10 {
        f.render_widget(
            Paragraph::new("adage ui: make the terminal larger (40×10 at least)").wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let [tabs, body, status] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(5), Constraint::Length(1)]).areas(area);
    draw_tabs(f, tabs, s, ctx);
    match s.tab {
        Tab::Live => view::draw_live(f, body, &s.live, s.health.as_ref(), ctx, false),
        Tab::Routes => draw_routes(f, body, s, ctx),
        Tab::Health => draw_health(f, body, s, ctx),
        Tab::Settings => draw_settings(f, body, s, ctx),
    }
    draw_status(f, status, s, ctx);
    if let Some(m) = &s.modal {
        draw_modal(f, body, m, s, t);
    }
}

fn draw_tabs(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let mut spans = vec![Span::styled(" adage ", t.title())];
    if ctx.demo {
        spans.push(Span::styled("demo ", t.holdout()));
    }
    for (i, (tab, name)) in TABS.iter().enumerate() {
        let mut label = format!(" {} {name} ", i + 1);
        if *tab == Tab::Health {
            if let Some(h) = &s.health {
                let bad = h.count(Status::Warn) + h.count(Status::Fail);
                if bad > 0 {
                    label = format!(" {} {name} ({bad}) ", i + 1);
                }
            }
        }
        if *tab == Tab::Routes {
            if let Some(Ok(r)) = &s.routes {
                let q = r.rows.iter().filter(|r| !r.reasons.is_empty()).count();
                if q > 0 {
                    label = format!(" {} {name} ({q} to review) ", i + 1);
                }
            }
        }
        spans.push(Span::styled(label, if *tab == s.tab { t.selected().patch(t.bold()) } else { t.dim() }));
    }
    if s.tab == Tab::Live {
        let state = if s.live.paused { format!("  PAUSED +{}", s.live.held.len()) } else { "  LIVE".into() };
        spans.push(Span::styled(state, if s.live.paused { t.yellow() } else { t.green() }));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_status(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let h = s.health.as_ref();
    let gen = h
        .and_then(|h| h.generation.as_deref())
        .map(|g| g[..g.len().min(8)].to_string())
        .unwrap_or_else(|| "none".into());
    let routes = h.map(|h| h.routes_repo.unwrap_or(h.routes_total)).unwrap_or(0);
    let auto = match h.and_then(|h| h.auto_learn.as_ref()) {
        Some((true, _)) => "on",
        Some((false, _)) => "off",
        None => "?",
    };
    let keys = match s.tab {
        Tab::Live => "p pause · f scope · v quiet",
        Tab::Routes => "a/r/d/p/n/x review · R reload",
        Tab::Health => "Enter fix · i m l o s · R recheck",
        Tab::Settings => "Enter edit · Space toggle",
    };
    let left = Line::from(vec![
        Span::styled(format!(" {} ", ctx.label), t.bold()),
        Span::styled(format!("· gen {gen} · {routes} routes · auto-learn {auto}"), t.dim()),
    ]);
    // The keys stay whole at the right; the state on the left gives way.
    let right = format!(" {keys} · ? help · q quit ");
    let right_w = (right.chars().count() as u16).min(area.width);
    let [l, r] = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_w)]).areas(area);
    f.render_widget(Paragraph::new(left), l);
    f.render_widget(Paragraph::new(Span::styled(right, t.dim())), r);
}

fn kv(k: &str, v: String, t: &Theme) -> Line<'static> {
    Line::from(vec![Span::styled(format!("{k:<9}"), t.dim()), Span::raw(v)])
}

pub fn route_detail(r: &RouteRow, t: &Theme) -> Vec<Line<'static>> {
    let l = &r.live;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{} ", t.aspect_mark(r.aspect)), t.aspect(r.aspect)),
            Span::styled(r.id.clone(), t.bold()),
            Span::styled(format!("  {} (ceiling {})", aspect_name(r.aspect), r.ceiling), t.aspect(r.aspect)),
        ]),
        kv("track", format!("{} @ {}{}", r.track, r.version, if r.imported { " (imported)" } else { "" }), t),
        kv("repo", r.repo.clone(), t),
        kv(
            "state",
            format!(
                "{} · effect {}{}{}",
                r.state,
                r.effect,
                if r.authored { " · pinned" } else { "" },
                if r.in_index { "" } else { " · not in the index" }
            ),
            t,
        ),
        kv("when", r.trigger.clone(), t),
        kv("do", r.step.clone(), t),
        kv(
            "evidence",
            format!("{}/{} sessions · rate {:.2} · Wilson lb {:.2}", r.support, r.opportunities, r.rate, r.lower_bound),
            t,
        ),
        kv(
            "live",
            format!("directed {} · followed {} · derailed {} · met {}", l.emitted, l.followed, l.derailed, l.met),
            t,
        ),
        kv("holdout", format!("would-have {} · unaided {} · missed {}", l.would_have, l.unaided, l.not_reached), t),
    ];
    for (k, d) in &r.reasons {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<9}", "review"), t.dim()),
            Span::styled(format!("{k}: "), t.yellow().patch(t.bold())),
            Span::raw(d.clone()),
        ]));
    }
    let keys: Vec<String> =
        routes::ACTIONS.iter().filter(|(_, v)| r.verbs.contains(v)).map(|(k, v)| format!("{k} {v}")).collect();
    lines.push(kv("keys", keys.join(" · "), t));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("directive, exactly as delivered (example values):", t.dim())));
    for d in r.directive.lines() {
        lines.push(Line::from(Span::styled(format!("  {d}"), t.aspect(r.aspect))));
    }
    lines
}

fn draw_routes(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let rows = match &s.routes {
        None => {
            f.render_widget(Paragraph::new(Span::styled("loading routes…", t.dim())), area);
            return;
        }
        Some(Err(e)) => {
            f.render_widget(Paragraph::new(Span::styled(format!("could not load routes: {e}"), t.red())), area);
            return;
        }
        Some(Ok(r)) => &r.rows,
    };
    if rows.is_empty() {
        let lines = vec![
            Line::raw("No routes installed yet."),
            Line::from(Span::styled(
                "`adage learn` builds them from your agents' sessions (Health tab: l learns now).",
                t.dim(),
            )),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
        return;
    }
    let wide = area.width >= 100;
    let [list, detail] = if wide {
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).areas(area)
    } else {
        Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area)
    };
    // The list, grouped by track.
    let mut lines: Vec<Line> = Vec::new();
    let mut sel_line = 0;
    let mut last_ref = None;
    for (i, r) in rows.iter().enumerate() {
        if last_ref != Some(&r.reference) {
            last_ref = Some(&r.reference);
            lines.push(Line::from(vec![
                Span::styled(r.reference.clone(), t.title()),
                Span::styled(format!(" @ {}  [{}]", r.version, r.repo), t.dim()),
            ]));
        }
        if i == s.route_sel {
            sel_line = lines.len();
        }
        let mark = if r.reasons.is_empty() { " " } else { "*" };
        let row_style = if i == s.route_sel { t.selected() } else { Style::default() };
        let counts = format!(" {}/{} · ✓{} ✗{}", r.support, r.opportunities, r.live.followed, r.live.derailed);
        lines.push(Line::from(vec![
            Span::styled(format!("{mark} "), t.yellow().patch(t.bold())),
            Span::styled(format!("{} ", t.aspect_mark(r.aspect)), t.aspect(r.aspect)),
            Span::styled(r.id.clone(), row_style),
            Span::styled(counts, t.dim()),
        ]));
    }
    let block = Block::default().borders(Borders::RIGHT | Borders::BOTTOM).border_style(t.dim());
    let inner = block.inner(list);
    let h = inner.height as usize;
    let offset = sel_line.saturating_sub(h.saturating_sub(2)).min(lines.len().saturating_sub(h));
    f.render_widget(block, list);
    f.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), inner);
    if let Some(r) = s.selected_route() {
        f.render_widget(
            Paragraph::new(route_detail(r, t)).wrap(Wrap { trim: false }).scroll((s.detail_scroll, 0)),
            detail.inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 }),
        );
    }
}

fn draw_health(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let Some(h) = &s.health else {
        f.render_widget(Paragraph::new(Span::styled("running checks…", t.dim())), area);
        return;
    };
    let [list, detail] = Layout::vertical([Constraint::Min(3), Constraint::Length(6)]).areas(area);
    let name_w = h.checks.iter().map(|c| c.check.chars().count()).max().unwrap_or(10).min(28);
    let mut lines: Vec<Line> = Vec::new();
    for (i, c) in h.checks.iter().enumerate() {
        let lamp = match c.status {
            Status::Pass => Lamp::Ok,
            Status::Warn => Lamp::Warn,
            Status::Fail => Lamp::Fail,
        };
        let sel = i == s.check_sel;
        let fixable = h.fixes.get(i).is_some_and(|f| f.is_some());
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", t.lamp_mark(lamp)), t.lamp(lamp)),
            Span::styled(format!("{:<name_w$}", c.check), if sel { t.selected() } else { t.bold() }),
            Span::styled(if fixable { " ⏎ " } else { "   " }, t.accent()),
            Span::raw(c.detail.clone()),
        ]));
    }
    for n in s.notices.iter().rev().take(3) {
        lines.push(Line::from(Span::styled(n.clone(), t.yellow())));
    }
    let hgt = list.height as usize;
    let offset = s.check_sel.saturating_sub(hgt.saturating_sub(1));
    f.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), list);

    let mut d: Vec<Line> = Vec::new();
    if let Some(c) = h.checks.get(s.check_sel) {
        d.push(Line::from(Span::styled(c.check.clone(), t.bold())));
        d.push(Line::raw(c.detail.clone()));
        if let Some(fx) = &c.fix {
            d.push(Line::from(vec![Span::styled("fix: ", t.dim()), Span::raw(fx.clone())]));
        }
        if let Some(Some(fix)) = h.fixes.get(s.check_sel) {
            d.push(Line::from(Span::styled(format!("Enter: {}", fix.label()), t.accent().patch(t.bold()))));
        }
    }
    let block = Block::default().borders(Borders::TOP).border_style(t.dim()).title(Span::styled(
        format!(
            " {} passed · {} warnings · {} failures ",
            h.count(Status::Pass),
            h.count(Status::Warn),
            h.count(Status::Fail)
        ),
        t.title(),
    ));
    let inner = block.inner(detail);
    f.render_widget(block, detail);
    f.render_widget(Paragraph::new(d).wrap(Wrap { trim: false }), inner);
}

fn draw_settings(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let mut lines = vec![
        Line::from(vec![Span::styled("config ", t.dim()), Span::raw(crate::detect::tilde(&s.config_path))]),
        Line::from(Span::styled(
            "the same settings as `adage config`; environment variables win over the file",
            t.dim(),
        )),
        Line::raw(""),
    ];
    for p in &s.settings_problems {
        lines.push(Line::from(Span::styled(format!("! {p}"), t.yellow())));
    }
    if !s.settings_problems.is_empty() {
        lines.push(Line::raw(""));
    }
    let key_w = s.settings.iter().map(|r| r.key.len()).max().unwrap_or(10);
    let val_w = s.settings.iter().map(|r| r.value.chars().count().max(2)).max().unwrap_or(8).min(40);
    for (i, row) in s.settings.iter().enumerate() {
        let sel = i == s.setting_sel;
        let shown = if row.value.is_empty() { "\"\"".to_string() } else { row.value.clone() };
        let source_style = if row.source == "default" { t.dim() } else { t.accent() };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<key_w$}", row.key), if sel { t.selected() } else { Style::default() }),
            Span::raw("  "),
            Span::styled(format!("{shown:<val_w$}"), t.bold()),
            Span::raw("  "),
            Span::styled(format!("({})", row.source), source_style),
        ]));
        lines.push(Line::from(Span::styled(format!("  {}", row.doc), t.dim())));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Enter edits (Space toggles on/off and switches thresholds); every value is validated before it is saved.",
        t.dim(),
    )));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn app_help(tab: Tab, t: &Theme) -> Vec<Line<'static>> {
    let k = |key: &str, what: &str| {
        Line::from(vec![Span::styled(format!("{key:>9}  "), t.bold()), Span::raw(what.to_string())])
    };
    let mut v = vec![k("1-4 Tab", "switch tabs"), k("q", "quit"), k("?", "this help"), Line::raw("")];
    match tab {
        Tab::Live => v.extend(view::watch_help(t).into_iter().skip(1)),
        Tab::Routes => {
            v.push(k("↑ ↓", "select a route"));
            v.push(k("PgUp PgDn", "scroll the detail"));
            for (c, verb) in routes::ACTIONS {
                v.push(k(&c.to_string(), verb_effect(verb)));
            }
            v.push(k("R", "reload"));
            v.push(Line::from(Span::styled("* marks routes awaiting review", t.dim())));
        }
        Tab::Health => {
            v.push(k("↑ ↓", "select a check"));
            v.push(k("Enter", "fix the selected check (⏎ marks fixable ones)"));
            v.push(k("i", "install hooks for a harness"));
            v.push(k("m", "fetch the embedding model"));
            v.push(k("l", "run learn now"));
            v.push(k("s", "open the settings"));
            v.push(k("o", "open the fail-open log"));
            v.push(k("R", "re-run the checks"));
        }
        Tab::Settings => {
            v.push(k("↑ ↓", "select a setting"));
            v.push(k("Enter", "edit (toggles booleans)"));
            v.push(k("Space", "toggle on/off, or switch thresholds"));
        }
    }
    v
}

fn draw_modal(f: &mut Frame, area: Rect, m: &Modal, s: &AppState, t: &Theme) {
    match m {
        Modal::Help => view::popup(f, area, "help", app_help(s.tab, t), t, 0),
        Modal::Confirm { title, lines, scroll, .. } => view::popup(f, area, title, lines.clone(), t, *scroll),
        Modal::Message { title, lines, error, scroll } => {
            let style = if *error { t.red() } else { Style::default() };
            let mut ls: Vec<Line> = lines.iter().map(|l| Line::from(Span::styled(l.clone(), style))).collect();
            ls.push(Line::raw(""));
            ls.push(Line::from(Span::styled("Enter or Esc closes", t.dim())));
            view::popup(f, area, title, ls, t, *scroll)
        }
        Modal::Viewer { title, lines, scroll } => {
            let mut ls: Vec<Line> = lines.iter().map(|l| Line::raw(l.clone())).collect();
            ls.push(Line::from(Span::styled("↑ ↓ scroll · Esc closes", t.dim())));
            view::popup(f, area, title, ls, t, *scroll)
        }
        Modal::Progress { title, status, ratio } => {
            let r = view::centered(area, 64.min(area.width.saturating_sub(2)), 5);
            f.render_widget(ratatui::widgets::Clear, r);
            let block = Block::default().borders(Borders::ALL).title(Span::styled(format!(" {title} "), t.title()));
            let inner = block.inner(r);
            f.render_widget(block, r);
            let [g, st] = Layout::vertical([Constraint::Length(1), Constraint::Length(2)]).areas(inner);
            f.render_widget(Gauge::default().gauge_style(t.accent()).ratio(*ratio), g);
            f.render_widget(Paragraph::new(Span::styled(status.clone(), t.dim())).wrap(Wrap { trim: true }), st);
        }
        Modal::Scope { harness } => view::popup(
            f,
            area,
            &format!("Install {harness} hooks"),
            vec![
                Line::raw("Where should Adage's hooks be registered?"),
                Line::raw(""),
                Line::from(vec![Span::styled("  p  ", t.bold()), Span::raw("this repository only")]),
                Line::from(vec![Span::styled("  u  ", t.bold()), Span::raw("user scope: every repository")]),
                Line::raw(""),
                Line::from(Span::styled(
                    "You will see the exact file and diff before anything is written. Esc cancels.",
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
                Line::from(vec![Span::raw("> "), Span::styled(format!("{buffer}▏"), t.accent())]),
            ];
            if let Some(e) = error {
                ls.push(Line::from(Span::styled(e.clone(), t.red())));
            }
            ls.push(Line::raw(""));
            ls.push(Line::from(Span::styled("Enter saves · Esc cancels", t.dim())));
            view::popup(f, area, "Edit setting", ls, t, 0)
        }
    }
}

/// The confirmation for an install: the exact file and a diff of what changes.
pub fn install_confirm(p: &Plan, t: &Theme) -> Vec<Line<'static>> {
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
    lines.push(Line::from(Span::styled("y write · n cancel · ↑ ↓ scroll", t.dim())));
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
                match crate::review::decide_quietly(verb, &reference, &route, &ctx.paths) {
                    Ok(lines) => s.message("Review recorded", lines, false),
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
                    // Keep the selection on the same route across reloads.
                    let keep = s.selected_route().map(|r| (r.reference.clone(), r.id.clone()));
                    s.routes = Some(r);
                    if let Some((reference, id)) = keep {
                        if let Some(i) = s.route_rows().iter().position(|r| r.reference == reference && r.id == id) {
                            s.route_sel = i;
                        }
                    }
                    s.route_sel = s.route_sel.min(s.route_rows().len().saturating_sub(1));
                }
            }
        }
        if let Some(j) = job.as_mut() {
            match j.poll() {
                Some(done) => {
                    let title = j.title.clone();
                    job = None;
                    match done {
                        Ok(lines) => s.message(&title, tail_lines(lines, 30), false),
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
        ticks += 1;
        if ticks.is_multiple_of(20) {
            s.load_settings(&config::load(&ctx.paths));
        }
        s.notices.extend(super::take_notices());
        if s.notices.len() > 20 {
            let extra = s.notices.len() - 20;
            s.notices.drain(..extra);
        }
        t.terminal.draw(|f| draw(f, &s, &ctx))?;
        if let Some(k) = term::next_key()? {
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

fn tail_lines(lines: Vec<String>, n: usize) -> Vec<String> {
    let skip = lines.len().saturating_sub(n);
    lines.into_iter().skip(skip).collect()
}
