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

use super::forms::{self, Form, Kind as FormKind};
use super::health::{Fix, Health};
use super::jobs::{self, Job, Loaded, Loader, Want};
use super::keys;
use super::palette::{self, Palette};
use super::routes::{RouteRow, Routes};
use super::splash;
use super::tabs;
use super::term::{self, Term};
use super::theme::Theme;
use super::tools::{self, Tool};
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
    Home,
    Live,
    Reflexes,
    Map,
    Health,
    Settings,
    Traces,
}

pub const TABS: [(Tab, &str); 7] = [
    (Tab::Live, "Live"),
    (Tab::Reflexes, "Reflexes"),
    (Tab::Map, "Map"),
    (Tab::Health, "Health"),
    (Tab::Settings, "Settings"),
    (Tab::Home, "Home"),
    (Tab::Traces, "Traces"),
];

impl Tab {
    /// This tab's keys, for the status bar and the overlay.
    pub fn keys(self) -> &'static [keys::Key] {
        match self {
            Tab::Home => keys::HOME,
            Tab::Live => keys::LIVE,
            Tab::Reflexes => keys::REFLEXES,
            Tab::Map => keys::MAP,
            Tab::Health => keys::HEALTH,
            Tab::Settings => keys::SETTINGS,
            Tab::Traces => keys::TRACES,
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
    /// A review decision: `verb` is the one `crate::review` takes (`strengthen`, …).
    Review {
        verb: &'static str,
        reference: String,
        route: String,
        expected_revision: String,
    },
    Install(Box<Plan>),
    FetchModel,
    LearnNow,
    Tool(Tool),
    SetConfig {
        key: &'static str,
        value: String,
    },
}

pub enum Modal {
    ReflexReview(Box<super::reflex_review::State>),
    Form(Form),
    Help {
        scroll: u16,
    },
    Confirm {
        title: String,
        lines: Vec<Line<'static>>,
        action: Action,
        scroll: u16,
    },
    Message {
        title: String,
        lines: Vec<String>,
        error: bool,
        scroll: u16,
    },
    Progress {
        title: String,
        status: String,
        ratio: f64,
    },
    /// Lines to read: `source` is the file they come from, when they come from one.
    Viewer {
        title: String,
        source: Option<String>,
        lines: Vec<String>,
        scroll: u16,
    },
    Scope {
        harness: String,
    },
    Edit {
        key: &'static str,
        buffer: String,
        error: Option<String>,
    },
    /// The repository switcher: which repository Live, Reflexes and Map show.
    Repos {
        query: String,
        sel: usize,
    },
    /// Conditioning: what it will read, how far it has got, what it did (`conditioned`).
    Conditioning(Box<super::conditioned::Flow>),
}

/// What the runtime must do after a key.
#[derive(Debug)]
pub enum Cmd {
    Quit,
    Do(Action),
    PlanInstall { harness: String, scope: &'static str },
    OpenFailOpen,
    OpenTrace { session: String, repo: String },
    PreviewLearning,
    PrepareTool(Tool),
    OpenRepository(PathBuf),
    OpenWeb { focus: Option<String>, traces: bool },
    CancelJob,
    Want(WantKind),
}

#[derive(Debug, Clone, Copy)]
pub enum WantKind {
    Traces,
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
    pub traces: tabs::traces::State,
    pub map: tabs::map::State,
    pub check_sel: usize,
    pub setting_sel: usize,
    pub settings: Vec<SettingRow>,
    /// Problems `adage_store::config` found in the file.
    pub settings_problems: Vec<String>,
    pub config_path: PathBuf,
    /// `exec/rewrite-off`: the marker an older `adage delegate off` wrote, which turns rewrites
    /// off whatever `runtime.rewrite` says. Settings shows the effective state.
    pub rewrite_marker: Option<PathBuf>,
    pub modal: Option<Modal>,
    pub palette: Option<Palette>,
    pub toasts: Vec<Toast>,
    pub notices: Vec<String>,
    pub pending_job_action: Option<Action>,
    pub job_cancelable: bool,
    pub home_sel: usize,
    /// The key of the repository the app was opened in (`None` outside one): the scope Live,
    /// Reflexes and Map start with. Health's `repo_key`, computed the same way, stands in until set.
    pub here: Option<String>,
}

impl AppState {
    pub fn new(config_path: PathBuf) -> AppState {
        AppState {
            tab: Tab::Home,
            live: Live::default(),
            health: None,
            routes: None,
            reflexes: tabs::reflexes::State::default(),
            traces: tabs::traces::State::default(),
            map: tabs::map::State::default(),
            check_sel: 0,
            setting_sel: 0,
            settings: vec![],
            settings_problems: vec![],
            config_path,
            rewrite_marker: None,
            modal: None,
            palette: None,
            toasts: vec![],
            notices: vec![],
            pending_job_action: None,
            job_cancelable: false,
            home_sel: 0,
            here: None,
        }
    }

    /// The repository the app was opened in.
    pub fn here(&self) -> Option<&str> {
        self.here.as_deref().or_else(|| self.health.as_ref().and_then(|h| h.repo_key.as_deref()))
    }

    /// The repository Live, Reflexes and Map show: `None` for every repository.
    pub fn scope_key(&self) -> Option<&str> {
        self.live.scope(self.here())
    }

    /// Whether a reflex belongs to the repository shown. A track with no repository applies to
    /// every one.
    pub fn in_scope(&self, r: &RouteRow) -> bool {
        self.scope_key().is_none_or(|k| r.repo == k || r.repo.is_empty())
    }

    /// The reflexes of the repository shown, for its counts.
    fn scoped_routes(&self) -> Option<Routes> {
        match &self.routes {
            Some(Ok(r)) => {
                Some(Routes { rows: r.rows.iter().filter(|x| self.in_scope(x)).cloned().collect(), traces: r.traces })
            }
            _ => None,
        }
    }

    /// The live view's review counts, for the repository shown (its reflex names stay for every
    /// repository: the ticker names reflexes by them).
    pub fn refresh_counts(&mut self) {
        if let Some(Ok(all)) = &self.routes {
            self.live.gists = all.gists();
        }
        if let Some(r) = self.scoped_routes() {
            self.live.waiting = Some(r.waiting());
            self.live.quiet = r.quiet();
            self.live.needs = r.needs();
        }
    }

    /// Show one repository (`Some(key)`), or every one (`None`).
    pub fn set_scope(&mut self, key: Option<String>) {
        let name = key.as_deref().map_or_else(|| "every repository".to_string(), super::scope::friendly);
        self.live.all_repos = key.is_none();
        if key.is_some() {
            self.live.repo = key;
        }
        self.live.scroll = 0;
        self.reflexes.sel = None;
        self.map.sel = None;
        self.traces.selected = 0;
        self.refresh_counts();
        self.toast(format!("Live, Reflexes and Map show {name}"), true);
    }

    /// The repositories the switcher offers, most recently active first.
    pub fn known_repos(&self) -> Vec<super::scope::Known> {
        let feed: Vec<&super::feed::FeedItem> = self.live.feed.iter().collect();
        let ticks: Vec<(&str, u64)> = self.live.ticks.iter().filter_map(|t| Some((t.repo.as_deref()?, t.ts))).collect();
        super::scope::known(&feed, &ticks, self.route_rows(), self.here(), self.scope_key())
    }

    /// The switcher's choices for `query`: each repository (`Some`), then every repository (`None`).
    pub fn repo_choices(&self, query: &str) -> Vec<Option<super::scope::Known>> {
        let q = query.trim();
        let mut v: Vec<Option<super::scope::Known>> = self
            .known_repos()
            .into_iter()
            .filter(|k| {
                q.is_empty() || super::fuzzy::score(q, &k.name).is_some() || super::fuzzy::score(q, &k.key).is_some()
            })
            .map(Some)
            .collect();
        if q.is_empty() || super::fuzzy::score(q, "all repositories").is_some() {
            v.push(None);
        }
        v
    }

    /// Open the repository switcher, on the repository shown now.
    pub fn open_repos(&mut self) {
        let current = self.scope_key().map(String::from);
        let sel = self.repo_choices("").iter().position(|c| c.as_ref().map(|k| k.key.clone()) == current).unwrap_or(0);
        self.modal = Some(Modal::Repos { query: String::new(), sel });
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
        // Rewrites are off while the older marker exists, whatever the file says.
        if self.rewrite_marker.as_ref().is_some_and(|m| m.exists()) {
            if let Some(r) = self.settings.iter_mut().find(|r| r.key == "runtime.rewrite") {
                r.value = "false".into();
                r.source = "exec/rewrite-off marker".into();
            }
        }
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

    pub fn complete_job(&mut self, title: &str, done: Result<jobs::Outcome, String>) {
        use super::conditioned::Flow;
        // A conditioning run can end while its card is hidden and another is open: that one stays.
        let busy = !matches!(self.modal, None | Some(Modal::Progress { .. } | Modal::Conditioning(_)));
        let done = match done {
            Ok(jobs::Outcome::Conditioned(card)) if busy => {
                self.toast(format!("Conditioning done: {}", card.sentence()), true);
                return;
            }
            Ok(jobs::Outcome::Conditioned(card)) => {
                // Its sections appear in turn, unless motion is reduced (and never in tests).
                let reveal = Theme::from_env().motion && !cfg!(test);
                self.modal = Some(Modal::Conditioning(Box::new(Flow::result(*card, reveal))));
                self.job_cancelable = false;
                return;
            }
            Ok(jobs::Outcome::Sources(p)) if matches!(self.pending_job_action, Some(Action::LearnNow)) => {
                self.pending_job_action = None;
                self.job_cancelable = false;
                let repo = self.here().unwrap_or_default().to_string();
                self.modal = Some(Modal::Conditioning(Box::new(Flow::confirm(*p, repo))));
                return;
            }
            other => other.map(jobs::Outcome::into_lines),
        };
        self.modal = None;
        self.job_cancelable = false;
        let pending = self.pending_job_action.take();
        match done {
            Ok(mut lines) => {
                if let Some(action) = pending {
                    lines.extend([String::new(),"Conditioning reads these sources, learns reflexes, binds applicable instincts, and recompiles the index.".into(),"Enter / y confirms. Esc / n cancels.".into()]);
                    self.confirm("Condition from these sources", lines.into_iter().map(Line::raw).collect(), action);
                } else {
                    self.modal = Some(Modal::Viewer { title: title.into(), source: None, lines, scroll: 0 });
                }
            }
            Err(e) => self.message(title, vec![e], true),
        }
    }

    fn switch(&mut self, tab: Tab) -> Option<Cmd> {
        self.tab = tab;
        if tab == Tab::Traces {
            Some(Cmd::Want(WantKind::Traces))
        } else {
            tab.wants_routes().then_some(Cmd::Want(WantKind::Routes))
        }
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
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('1'..='7') => {
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
            Tab::Home => super::tabs::home::key(self, code),
            Tab::Live => {
                let open = self.live.inspect
                    || self.live.map_hint
                    || self.live.runs
                    || self.live.filtering
                    || !self.live.filter.is_empty();
                if code == KeyCode::Esc && !open {
                    return None;
                }
                // The map in the browser, as `adage watch`'s `m` opens it (onboarding plan O10).
                if code == KeyCode::Char('m') && !open {
                    return Some(Cmd::OpenWeb { focus: None, traces: false });
                }
                // The repository switcher (the watch pane's `f` only toggles every repository).
                if code == KeyCode::Char('f') && !open {
                    self.open_repos();
                    return None;
                }
                watch::live_key(&mut self.live, code, mods);
                None
            }
            Tab::Reflexes => tabs::reflexes::key(self, code),
            Tab::Map => tabs::map::key(self, code),
            Tab::Health => tabs::health::key(self, code),
            Tab::Settings => tabs::settings::key(self, code),
            Tab::Traces => tabs::traces::key(self, code),
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

    pub(super) fn run_entry(&mut self, run: palette::Run) -> Option<Cmd> {
        match run {
            palette::Run::Form(kind) => {
                let mut form = Form::new(kind);
                if let Some(row) = self.selected_route() {
                    form.reflex = Some((row.reference.clone(), row.id.clone()));
                    if kind == FormKind::SharePack {
                        form.values[0] = row.reference.clone();
                    }
                }
                if kind == FormKind::Delegate && form.reflex.is_none() {
                    self.message(
                        "Select a reflex",
                        vec!["Select a reflex in Reflexes, then choose Delegate.".into()],
                        false,
                    );
                } else {
                    self.modal = Some(Modal::Form(form));
                }
                None
            }
            palette::Run::Revoke => {
                if let Some(row) = self.selected_route() {
                    Some(Cmd::PrepareTool(Tool::Revoke { reference: row.reference.clone(), route: row.id.clone() }))
                } else {
                    self.message(
                        "Select a reflex",
                        vec!["Select a reflex in Reflexes, then revoke its delegation.".into()],
                        false,
                    );
                    None
                }
            }
            palette::Run::Connect(harness) => self.start_fix(Fix::InstallHooks(harness.into())),
            // Conditioning starts from the card that shows what it will read.
            palette::Run::Tool(Tool::Condition) => Some(Cmd::PreviewLearning),
            palette::Run::Tool(tool) => {
                Some(if tool.mutates() { Cmd::PrepareTool(tool) } else { Cmd::Do(Action::Tool(tool)) })
            }
            palette::Run::Web { traces } => Some(Cmd::OpenWeb { focus: None, traces }),
            palette::Run::Tab(t) => self.switch(t),
            palette::Run::Repos => {
                self.open_repos();
                None
            }
            palette::Run::Scope(key) => {
                self.set_scope(key);
                None
            }
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
            Modal::ReflexReview(review) => super::reflex_review::key(self, review, code),
            Modal::Form(form) => forms::key(self, form, code),
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
            Modal::Viewer { title, source, lines, scroll: mut s } => {
                if matches!(code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                    return None;
                }
                nudge(&mut s);
                self.modal = Some(Modal::Viewer { title, source, lines, scroll: s });
                None
            }
            Modal::Conditioning(flow) => super::conditioned::key(self, flow, code),
            Modal::Progress { .. } => {
                if code == KeyCode::Esc && self.job_cancelable {
                    return Some(Cmd::CancelJob);
                }
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
            Modal::Repos { mut query, mut sel } => {
                let n = self.repo_choices(&query).len();
                match code {
                    KeyCode::Esc => return None,
                    KeyCode::Enter => {
                        let choice = self.repo_choices(&query).into_iter().nth(sel)?;
                        self.set_scope(choice.map(|k| k.key));
                        return None;
                    }
                    KeyCode::Up => sel = sel.saturating_sub(1),
                    KeyCode::Down => sel = (sel + 1).min(n.saturating_sub(1)),
                    KeyCode::Backspace => {
                        query.pop();
                        sel = 0;
                    }
                    KeyCode::Char(c) if query.chars().count() < 60 => {
                        query.push(c);
                        sel = 0;
                    }
                    _ => {}
                }
                self.modal = Some(Modal::Repos { query, sel });
                None
            }
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
                        Line::raw("Download potion-base-8M (SHA-256 verified) into Adage's data directory."),
                        Line::raw("It lets conditioning find intent families; installed, it is used instead"),
                        Line::raw("of the copy bundled in adage."),
                        Line::raw(""),
                        same("adage models fetch"),
                        hint,
                    ],
                    Action::FetchModel,
                );
                None
            }
            Fix::LearnNow => Some(Cmd::PreviewLearning),
            Fix::History(h) => self.run_entry(palette::Run::Form(match h.as_str() {
                "codex" => FormKind::HistoryCodex,
                "opencode" => FormKind::HistoryOpenCode,
                _ => FormKind::HistoryClaude,
            })),
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
    if draw_base(f, s, ctx).is_some() {
        draw_overlays(f, s, ctx);
    }
}

/// The app's rows: the tab bar, its rule, the body, the rule above the status bar, the status bar.
fn regions(area: Rect) -> [Rect; 5] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area)
}

/// Everything but a popup or the palette. Returns the area the tab's content took (where a tab
/// switch eases in), or `None` when the terminal is too small to draw the app at all.
pub(super) fn draw_base(f: &mut Frame, s: &AppState, ctx: &Ctx) -> Option<Rect> {
    let t = &ctx.theme;
    let area = f.area();
    if area.width < 40 || area.height < 10 {
        f.render_widget(
            Paragraph::new(t.text("adage ui: make the terminal larger (40×10 at least)")).wrap(Wrap { trim: true }),
            area,
        );
        return None;
    }
    let [tabs_a, rule_a, body, rule_b, status] = regions(area);
    // Toasts never cover the tab: the newest sits on the rule above the status bar, and any
    // older ones take rows from the bottom of the tab while they last.
    let extra = (s.toasts.len() as u16).saturating_sub(1).min(body.height.saturating_sub(5));
    let [content, _] = Layout::vertical([Constraint::Min(1), Constraint::Length(extra)]).areas(body);
    let active = draw_tabs(f, tabs_a, s, ctx);
    f.render_widget(Paragraph::new(tab_rule(area.width, active, t)), rule_a);
    match s.tab {
        Tab::Home => tabs::home::draw(f, content, s, ctx),
        Tab::Live => view::draw_live_in(f, content, &s.live, s.health.as_ref(), s.here(), ctx, false),
        Tab::Reflexes => tabs::reflexes::draw(f, content, s, ctx),
        Tab::Map => tabs::map::draw(f, content, s, ctx),
        Tab::Health => tabs::health::draw(f, content, s, ctx),
        Tab::Settings => tabs::settings::draw(f, content, s, ctx),
        Tab::Traces => tabs::traces::draw(f, content, s, ctx),
    }
    f.render_widget(Paragraph::new(view::rule("", None, area.width, t)), rule_b);
    draw_toasts(f, Rect { height: extra + 1, y: rule_b.y - extra, ..rule_b }, s, t);
    draw_status(f, status, s, ctx);
    Some(content)
}

/// The open popup, then the palette, over the app.
pub(super) fn draw_overlays(f: &mut Frame, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let area = f.area();
    let [_, _, body, _, _] = regions(area);
    match &s.modal {
        Some(Modal::ReflexReview(review)) => super::reflex_review::draw(f, area, s, review, ctx),
        Some(Modal::Repos { query, sel }) => draw_repos(f, body, s, query, *sel, ctx),
        Some(m) => draw_modal(f, body, m, s, t),
        None => {}
    }
    if let Some(p) = &s.palette {
        palette::draw(f, area, s, p, t);
    }
}

/// The rule under the tab bar, with the current tab underlined in the accent: `───━━━━━━━───`.
fn tab_rule(width: u16, active: Option<(u16, u16)>, t: &Theme) -> Line<'static> {
    let Some((x, w)) = active.filter(|(x, w)| x + w <= width) else { return view::rule("", None, width, t) };
    let thin = t.ch("─", "-");
    Line::from(vec![
        Span::styled(thin.repeat(x as usize), t.rule()),
        Span::styled(t.ch("━", "=").repeat(w as usize), t.accent()),
        Span::styled(thin.repeat((width - x - w) as usize), t.rule()),
    ])
}

/// The tab bar. Returns the current tab's columns (where its underline goes).
fn draw_tabs(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) -> Option<(u16, u16)> {
    let t = &ctx.theme;
    // Narrow terminals name only the current tab (the others keep their number), so where you
    // are is never cut off the edge.
    let tabs = |compact: bool| {
        let mut spans = vec![Span::styled(" adage", t.title()), Span::raw(if compact { " " } else { "   " })];
        let mut active = None;
        for (i, (tab, name)) in TABS.iter().enumerate() {
            let badge = match tab {
                Tab::Health => s.health.as_ref().map_or(0, |h| h.count(Status::Warn) + h.count(Status::Fail)),
                Tab::Reflexes => s.scoped_routes().map_or(0, |r| r.waiting()),
                _ => 0,
            };
            if *tab == s.tab {
                let x: usize = spans.iter().map(|s| s.width()).sum();
                let pill = format!(" {} {name} ", i + 1);
                active = Some((x as u16, pill.chars().count() as u16));
                spans.push(Span::styled(pill, t.accent().patch(t.selected()).patch(t.bold())));
                if badge > 0 {
                    spans.push(Span::raw(" "));
                }
            } else if compact {
                spans.push(Span::styled(format!(" {}", i + 1), t.ink2()));
            } else {
                spans.push(Span::styled(format!(" {} ", i + 1), t.dim()));
                spans.push(Span::styled(format!("{name} "), t.ink2()));
            }
            if badge > 0 {
                // The same pill as a chip anywhere: what waits on this tab.
                let tone = if *tab == Tab::Health && s.health.as_ref().is_some_and(|h| h.count(Status::Fail) > 0) {
                    super::kit::Tone::Fail
                } else {
                    super::kit::Tone::Warn
                };
                spans.push(super::kit::badge(badge, tone, t));
            }
            if !compact {
                spans.push(Span::raw(" "));
            }
        }
        (spans, active)
    };
    let full = tabs(false);
    let (mut spans, mut active) =
        if full.0.iter().map(|s| s.width()).sum::<usize>() <= area.width as usize { full } else { tabs(true) };
    // The repository Live, Reflexes and Map show, on those tabs (`f` or the palette switches it);
    // elsewhere, the repository the app was opened in.
    let scoped = matches!(s.tab, Tab::Live | Tab::Reflexes | Tab::Map | Tab::Traces);
    let mut right = if scoped { scope_chip(s, t, false) } else { vec![Span::styled(ctx.label.clone(), t.ink2())] };
    right.push(Span::styled(t.sep(), t.ink2()));
    if ctx.demo {
        right.insert(0, Span::styled(format!("demo{}", t.sep()), t.control()));
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
    let mut lw: usize = spans.iter().map(|s| s.width()).sum();
    let mut rw: usize = right.iter().map(|s| s.width()).sum();
    // Short of room, the scope keeps its last name (`billing`), not the whole origin.
    if scoped && lw + rw >= area.width as usize {
        let long: usize = scope_chip(s, t, false).iter().map(|s| s.width()).sum();
        let chip = scope_chip(s, t, true);
        rw = rw - long + chip.iter().map(|s| s.width()).sum::<usize>();
        let n = chip.len();
        right.splice(usize::from(ctx.demo)..usize::from(ctx.demo) + n, chip);
    }
    if scoped && lw + rw >= area.width as usize {
        (spans, active) = tabs(true);
        lw = spans.iter().map(|s| s.width()).sum();
    }
    if lw + rw < area.width as usize {
        spans.push(Span::raw(" ".repeat(area.width as usize - lw - rw)));
        spans.extend(right);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    active.map(|(x, w)| (x + area.x, w))
}

/// The scope in the tab bar: `◆ github.com/acme/billing ▾`, or `◆ all repositories ▾`.
fn scope_chip(s: &AppState, t: &Theme, short: bool) -> Vec<Span<'static>> {
    let name = match s.scope_key() {
        None if short => "all".to_string(),
        None => "all repositories".to_string(),
        Some(k) if short => super::scope::short(k),
        Some(k) => super::scope::friendly(k),
    };
    vec![
        Span::styled(format!("{} ", t.ch("◆", "*")), t.accent()),
        Span::styled(t.fit(&name, 48), t.bold()),
        Span::styled(format!(" {}", t.ch("▾", "v")), t.dim()),
    ]
}

/// The repository switcher: a search line, each repository with what is known of it (most
/// recently active first), then every repository.
fn draw_repos(f: &mut Frame, area: Rect, s: &AppState, query: &str, sel: usize, ctx: &Ctx) {
    let t = &ctx.theme;
    let choices = s.repo_choices(query);
    let sel = sel.min(choices.len().saturating_sub(1));
    let current = s.scope_key();
    let name_w = choices.iter().flatten().map(|k| k.name.chars().count()).max().unwrap_or(16).clamp(16, 44);
    let mut lines = vec![
        Line::from(Span::styled("Live, Reflexes, Map and Traces show one repository at a time.", t.dim())),
        Line::raw(""),
        Line::from(vec![
            Span::styled("/ ", t.key()),
            Span::styled(query.to_string(), t.bold()),
            Span::styled(t.ch("▌", "_"), t.accent()),
        ]),
        Line::raw(""),
    ];
    let head = lines.len();
    for (i, c) in choices.iter().enumerate() {
        let chosen = i == sel;
        let pointer = if chosen {
            Span::styled(format!("{} ", t.glyph(Glyph::Pointer)), t.accent().patch(t.bold()))
        } else {
            Span::raw("  ")
        };
        let base = if chosen { t.bold() } else { Style::default() };
        let (mark, name, facts, shown) = match c {
            Some(k) => {
                let mut facts = Vec::new();
                if k.here {
                    facts.push("opened here".to_string());
                }
                match k.reflexes {
                    0 => facts.push("no reflexes yet".into()),
                    1 => facts.push("1 reflex".into()),
                    n => facts.push(format!("{n} reflexes")),
                }
                if let Some(ts) = k.last {
                    facts.push(format!("active {}", view::clock(ts, ctx.tz, false)));
                }
                (t.ch("◆", "*"), k.name.clone(), facts.join(&t.sep()), current == Some(k.key.as_str()))
            }
            None => (
                t.ch("◇", "o"),
                "All repositories".to_string(),
                "every repository's activity and reflexes".to_string(),
                current.is_none(),
            ),
        };
        let mut spans = vec![
            pointer,
            Span::styled(format!("{mark} "), t.accent()),
            Span::styled(t.pad(&t.fit(&name, name_w), name_w + 2), base),
            Span::styled(facts, t.ink2()),
        ];
        if shown {
            spans.push(Span::styled(format!("  {} shown", t.glyph(Glyph::Ok)), t.green()));
        }
        lines.push(Line::from(spans));
    }
    if choices.is_empty() {
        lines.push(Line::from(Span::styled(format!("Nothing matches {}.", t.quote(query)), t.dim())));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!("{} move{}enter show{}type to search{}esc close", t.arrows(), t.sep(), t.sep(), t.sep()),
        t.dim(),
    )));
    // Scrolled so the selection shows above the hint lines.
    let room = (area.height as usize).saturating_sub(4 + 2);
    let scroll = (head + sel + 1).saturating_sub(room);
    view::popup(f, area, "Repository", lines, t, scroll as u16);
}

fn draw_status(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let room = (area.width as usize).saturating_sub(11);
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
        spans.push(Span::raw("   "));
        spans.extend(super::kit::hints(&[("enter", "keep"), ("esc", "clear")], t, room));
    } else {
        spans.extend(keys::bar(&[keys::GLOBAL, s.tab.keys()], t, room));
    }
    let used: usize = spans.iter().map(|s| s.width()).sum();
    spans.push(Span::raw(" ".repeat((area.width as usize).saturating_sub(used + 9))));
    spans.extend(super::kit::hints(&[("q", "quit")], t, 8));
    spans.push(Span::raw(" "));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The toasts, newest at the bottom of `body`, right-aligned.
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
        // Drawn by `draw_repos`, which needs the clock's offset.
        Modal::Repos { .. } | Modal::ReflexReview(_) => {}
        Modal::Form(form) => forms::draw(f, area, form, t),
        Modal::Help { scroll } => view::popup_keys(f, area, app_help(s.tab, t), t, *scroll),
        Modal::Confirm { title, lines, scroll, .. } => view::popup(f, area, title, lines.clone(), t, *scroll),
        Modal::Message { title, lines, error, scroll } => {
            let style = if *error { t.red() } else { Style::default() };
            let mut ls: Vec<Line> = lines.iter().map(|l| Line::from(Span::styled(l.clone(), style))).collect();
            ls.push(Line::raw(""));
            ls.push(Line::from(Span::styled("enter or esc closes", t.dim())));
            view::popup(f, area, title, ls, t, *scroll)
        }
        Modal::Viewer { title, source, lines, scroll } => {
            let mut ls: Vec<Line> = Vec::new();
            if let Some(p) = source {
                // A long path keeps its end, the file's name: `…/adage/fail-open.log`.
                let room = (area.width as usize).saturating_sub(8);
                ls.push(Line::from(Span::styled(words::fit_left(p, room, t.more()), t.dim())));
                ls.push(Line::raw(""));
            }
            ls.extend(lines.iter().map(|l| Line::raw(l.clone())));
            ls.push(Line::from(Span::styled(format!("{} scroll{}esc closes", t.arrows(), t.sep()), t.dim())));
            view::popup(f, area, title, ls, t, *scroll)
        }
        Modal::Conditioning(flow) => super::conditioned::draw(f, area, flow, t),
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
            let status = if s.job_cancelable { format!("{status} · Esc cancels") } else { status.clone() };
            f.render_widget(Paragraph::new(Span::styled(t.text(&status), t.dim())).wrap(Wrap { trim: true }), st);
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

/// What `Update Adage` will do, said before it is confirmed (onboarding plan O6).
fn update_preview(paths: &adage_store::Paths) -> Vec<String> {
    let have = env!("CARGO_PKG_VERSION");
    vec![
        match crate::update::newer(paths) {
            Some(r) => format!("Update Adage {have} to {}, the way it was installed: its binaries, its hooks and the plugin.", r.version),
            None => format!("Check for a newer Adage than {have}, and update to it if there is one: its binaries, its hooks and the plugin."),
        },
        "The binaries it replaces are kept; adage update --rollback puts them back.".into(),
    ]
}

/// Carry out a command. Returns `true` to quit.
fn execute(
    cmd: Cmd,
    s: &mut AppState,
    ctx: &Ctx,
    loader: &Loader,
    job: &mut Option<Job>,
    dashboard: &mut Option<crate::map::Server>,
) -> bool {
    let t = &ctx.theme;
    if ctx.demo
        && matches!(
            &cmd,
            Cmd::PlanInstall { .. }
                | Cmd::PreviewLearning
                | Cmd::PrepareTool(_)
                | Cmd::Do(Action::Install(_) | Action::FetchModel | Action::LearnNow | Action::SetConfig { .. })
        )
    {
        s.message("Demo workspace", vec!["The demo uses temporary sample data. To set up your agent or condition real history, choose Open repository from the palette first.".into()], false);
        return false;
    }
    // A conditioning run goes on in the background with its card hidden: what would start another
    // job shows it again instead.
    if let Some(j) = job.as_ref().filter(|j| j.stages.is_some()) {
        if matches!(
            &cmd,
            Cmd::PreviewLearning
                | Cmd::PrepareTool(_)
                | Cmd::Do(Action::Tool(_) | Action::FetchModel | Action::LearnNow)
        ) {
            s.modal = Some(Modal::Conditioning(Box::new(super::conditioned::Flow::running(
                j,
                s.here().unwrap_or_default().to_string(),
            ))));
            return false;
        }
    }
    match cmd {
        Cmd::OpenRepository(_) => unreachable!("handled by the event loop"),
        Cmd::CancelJob => { *job = None; s.pending_job_action = None; s.job_cancelable = false; s.modal = None; },
        Cmd::OpenWeb { focus, traces } => {
            let result = (|| -> Result<String> {
                if dashboard.is_none() { *dashboard = Some(crate::map::Server::start(ctx.paths.clone(), ctx.root.clone(), ctx.demo)?); }
                let server = dashboard.as_ref().expect("started");
                let url = server.url(focus.as_deref(), traces);
                server.open(&url)?;
                Ok(url)
            })();
            match result { Ok(url) => s.message("Web map opened", vec![url, "Keep this terminal app open while using the live map. Use its Traces view to inspect complete sessions.".into()], false), Err(e) => s.message("Could not open the browser", vec![format!("{e:#}"), dashboard.as_ref().map(|d|d.url(focus.as_deref(),traces)).unwrap_or_default()], true) }
        },
        Cmd::PreviewLearning => {
            if let Some(root) = ctx.root.clone() {
                let paths = ctx.paths.clone();
                let j = Job::spawn("Preview conditioning sources", move |_| crate::history::preview(&paths, &root));
                s.pending_job_action = Some(Action::LearnNow);
                s.job_cancelable = true;
                s.modal = Some(Modal::Progress {title:j.title.clone(),status:"Reading sources; Esc cancels".into(),ratio:0.0});
                *job = Some(j);
            } else { s.message("Open a repository", vec!["Choose Open repository from the palette, then Condition now.".into()],false); }
        },
        Cmd::PrepareTool(mut tool) => {
            let preview = (|| -> Result<Vec<String>> {
                let mut lines = vec![format!("Repository: {}", ctx.label)];
                match &mut tool {
                    Tool::AddTrack { directory, trust, expected_digest } => {
                        *directory = tools::history_source_directory(directory, ctx.root.as_deref())?;
                        let (summary, token) = crate::add::preview_import(directory, ctx.root.as_deref(), *trust)?;
                        *expected_digest = Some(token); lines.extend(summary);
                        lines.push("No execution grant is created.".into());
                    },
                    Tool::SetHistorySource { harness, directory } => {
                        *directory = tools::history_source_directory(directory, ctx.root.as_deref())?;
                        lines.push(format!("Set {harness} history to {}.", directory.display()));
                        lines.push("This source replaces automatic detection for Sense and Condition, including background conditioning.".into());
                    },
                    Tool::ExportOpenCode => lines.extend(["Read OpenCode's local session list and export up to 50 sessions for this repository from the latest 500.".into(),"Copies are stored in the configured exports folder, or Adage's history/opencode folder. Run this import again for new history.".into()]),
                    Tool::ShareTrack { track, output, .. } => lines.push(format!("Sign and export {track} to {} using your SSH signing agent.", output.display())),
                    Tool::InstallModel(dir) => lines.push(format!("Validate and install the local embedding model from {}.",dir.display())),
                    Tool::Delegate { reference, route, level, outputs, cache, effect } => {
                        lines.push(format!("Grant {level} execution to {route} in {reference}."));
                        lines.push(format!("E2 outputs: {outputs}; cache inputs: {cache}; effect: {effect}."));
                        lines.push("Delegation authorizes hooks to execute this reflex within its approved limits. Existing policy checks still apply.".into());
                    },
                    Tool::Revoke { route, .. } => lines.push(format!("Withdraw the execution grant for {route}.")),
                    Tool::Pack { name, enabled } => lines.push(format!("{} pack {name} in this repository and recompile its bindings.", if *enabled {"Enable"}else{"Disable"})),
                    Tool::Update => lines.extend(update_preview(&ctx.paths)),
                    _ => {},
                }
                lines.push("Enter / y confirms. Esc / n cancels.".into()); Ok(lines)
            })();
            match preview { Ok(lines) => s.confirm(tool.title(),lines.into_iter().map(Line::raw).collect(),Action::Tool(tool)),Err(e)=>s.message("Cannot prepare action",vec![format!("{e:#}")],true) }
        },
        Cmd::Quit => return true,
        Cmd::OpenTrace { session, repo } => {
            if job.is_some() {
                s.message("Job running", vec!["Wait for the current job before opening a recorded session.".into()], false);
                return false;
            }
            let paths = ctx.paths.clone();
            let j = Job::spawn("Recorded session", move |_| {
                let trace = adage_app::graph::trace(&paths, &session, &repo)?
                    .ok_or_else(|| anyhow::anyhow!("This recorded session is no longer available. Reload Traces."))?;
                Ok(tabs::traces::detail_lines(&trace))
            });
            s.modal = Some(progress_modal(&j, s));
            *job = Some(j);
        },
        Cmd::Want(WantKind::Traces) => loader.want(Want::Traces),
        Cmd::Want(WantKind::Routes) => loader.want(Want::Routes),
        Cmd::Want(WantKind::Health) => loader.want(Want::Health),
        Cmd::OpenFailOpen => s.modal = Some(fail_open_viewer(&ctx.paths)),
        Cmd::PlanInstall { harness, scope } => match crate::install::plan(&harness, scope, ctx.root.as_deref(), None) {
            Ok(p) => {
                let lines = install_confirm(&p, t);
                s.confirm(&format!("Install {harness} hooks ({scope})"), lines, Action::Install(Box::new(p)));
            }
            Err(e) => s.message("Cannot install", vec![format!("{e:#}")], true),
        },
        Cmd::Do(action) => match action {
            Action::Tool(tool) => {
                if ctx.demo && tool.mutates() { s.message("Demo workspace",vec!["Choose Open repository to work with your own data.".into()],false); return false; }
                let j=tools::run(ctx.paths.clone(),ctx.root.clone(),tool);
                s.job_cancelable=j.cancelable;
                s.modal=Some(progress_modal(&j, s)); *job=Some(j);
            },
            Action::Review { verb, reference, route, expected_revision } => {
                // `crate::review` takes the lexicon's verbs.
                match crate::review::decide_quietly(verb, &reference, &route, &expected_revision, &ctx.paths) {
                    Ok(outcome) => {
                        s.message("Decision saved", super::reflex_review::outcome_lines(&outcome), false);
                    },
                    Err(e) => s.message("Refused", vec![format!("{e:#}")], true),
                }
                loader.want(Want::Routes);
                loader.want(Want::Health);
            }
            Action::Install(p) => {
                match crate::install::apply_backed_up(&p) {
                    Ok(backup) => {
                        let mut lines = vec![format!("wrote {}", p.target.display())];
                        if let Some(b) = backup {
                            lines.push(format!("the previous version is in {}", b.display()));
                        }
                        if p.harness == "codex" {
                            lines.push(
                                "Codex asks you to review new hooks before they run: open /hooks in Codex.".into(),
                            );
                        }
                        lines.push("Hooks are registered. Health shows whether the first hook has arrived.".into());
                        lines.push("Next: History sources in the palette, then Condition now and Review in Reflexes.".into());
                        s.message("Hooks registered", lines, false)
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
            Action::LearnNow => match ctx.root.clone() {
                Some(root) => {
                    let j = tools::run(ctx.paths.clone(), Some(root), Tool::Condition);
                    s.modal = Some(progress_modal(&j, s));
                    *job = Some(j);
                }
                None => s.message(
                    "Not inside a repository",
                    vec!["Conditioning reads one repository's traces: open adage inside one, or run `adage condition --all` for every repository's.".into()],
                    true,
                ),
            },
            Action::SetConfig { key, value } => {
                // `adage config set`'s own guard and validation.
                let set = crate::share::require_person("config set")
                    .and_then(|()| config::set(&ctx.paths, key, &value).map_err(|e| anyhow::anyhow!(e)));
                // Turning rewrites on clears the older marker too, as `adage delegate rewrites on`.
                if key == "runtime.rewrite" && value == "true" && set.is_ok() {
                    let _ = std::fs::remove_file(ctx.paths.rewrite_off());
                }
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

/// The card a job shows while it runs: a conditioning run's stages, else a progress bar.
fn progress_modal(j: &Job, s: &AppState) -> Modal {
    if j.stages.is_some() {
        Modal::Conditioning(Box::new(super::conditioned::Flow::running(j, s.here().unwrap_or_default().to_string())))
    } else {
        Modal::Progress { title: j.title.clone(), status: j.status.clone(), ratio: 0.0 }
    }
}

/// The fail-open log, newest at the bottom: titled for what it is, the file named inside.
pub fn fail_open_viewer(paths: &adage_store::Paths) -> Modal {
    let p = paths.root.join(crate::doctor::FAIL_OPEN_LOG);
    let lines: Vec<String> = match std::fs::read_to_string(&p) {
        Ok(text) => {
            let all: Vec<String> = text.lines().map(pretty_fail_open).collect();
            all[all.len().saturating_sub(200)..].to_vec()
        }
        Err(_) => vec!["No fail-open errors recorded.".into()],
    };
    let scroll = lines.len().saturating_sub(10) as u16;
    Modal::Viewer { title: "Fail-open errors".into(), source: Some(crate::detect::tilde(&p)), lines, scroll }
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

pub fn run(ctx: Ctx, before_tick: Option<Box<dyn FnMut()>>) -> Result<()> {
    run_inner(ctx, before_tick, false, false)
}
pub(super) fn from_watch(ctx: Ctx, before_tick: Option<Box<dyn FnMut()>>, web: bool) -> Result<()> {
    run_inner(ctx, before_tick, true, web)
}
fn run_inner(ctx: Ctx, mut before_tick: Option<Box<dyn FnMut()>>, from_watch: bool, web: bool) -> Result<()> {
    let mut loader = Loader::spawn(ctx.paths.clone(), ctx.root.clone());
    // The Live tab's review count and the palette's reflexes need them from the start.
    loader.want(Want::Routes);
    let mut sources = Sources::new(&ctx);
    let mut s = AppState::new(ctx.paths.config());
    s.here = here_key(&ctx);
    s.rewrite_marker = Some(ctx.paths.rewrite_off());
    s.load_settings(&config::load(&ctx.paths));
    let mut job: Option<Job> = None;
    let mut dashboard = None;
    let mut ctx = ctx;
    let mut t = Term::enter()?;
    // The start screen (`splash`): Adage's brain for a moment, over the app it dissolves into.
    let mut splash = t.terminal.size().ok().and_then(|z| splash::start(&ctx, (z.width, z.height), from_watch));
    if from_watch {
        s.tab = Tab::Live;
        if web {
            execute(Cmd::OpenWeb { focus: None, traces: false }, &mut s, &ctx, &loader, &mut job, &mut dashboard);
        } else {
            s.palette = Some(Palette::default());
        }
    }
    let mut ticks = 0u64;
    // Tab switches ease in and popups grow in (`motion`), at the start screen's cadence while
    // they run; never under ADAGE_REDUCED_MOTION.
    let mut motion = super::motion::Motion::new(&s, ctx.theme.motion);
    loop {
        if let Some(f) = before_tick.as_mut() {
            f();
        }
        while let Ok(m) = loader.rx.try_recv() {
            match m {
                Loaded::Health(h) => {
                    // The first checks select what the Health tab shows first (its first problem).
                    let first = s.health.is_none();
                    s.health = Some(*h);
                    let n = s.checks_len();
                    s.check_sel = s.check_sel.min(n.saturating_sub(1));
                    if first {
                        s.check_sel = s.health.as_ref().map_or(0, tabs::health::first);
                    }
                    ctx.tz = view::local_offset();
                }
                Loaded::Traces(data) => s.traces.data = Some(data),
                Loaded::Routes(r) => {
                    s.routes = Some(r);
                    s.refresh_counts();
                }
            }
        }
        if let Some(j) = job.as_mut() {
            match j.poll() {
                Some(done) => {
                    let title = j.title.clone();
                    job = None;
                    s.complete_job(&title, done);
                    s.load_settings(&config::load(&ctx.paths));
                    loader.want(Want::Health);
                    loader.want(Want::Routes);
                }
                None => {
                    if let Some(Modal::Progress { status, ratio, .. }) = &mut s.modal {
                        *status = j.status.clone();
                        *ratio = j.ratio;
                    }
                    if let Some(Modal::Conditioning(flow)) = &mut s.modal {
                        flow.follow(j);
                    }
                }
            }
        }
        let here = s.here().map(String::from);
        sources.poll(&mut s.live, here.as_deref());
        // Opened outside a repository: the most recently active one, once there is one.
        let unsettled = s.live.repo.is_none();
        s.live.settle(here.as_deref());
        if unsettled && s.live.repo.is_some() {
            s.refresh_counts();
        }
        s.live.tick(Instant::now(), ctx.theme.motion, here.as_deref(), adage_core::time::now_ms());
        // The app's frames count toasts' lives and the settings reload; the start screen's
        // faster frames do not (nothing of the app shows under it).
        if splash.is_none() {
            ticks += 1;
            s.tick();
            if ticks.is_multiple_of(20) {
                s.load_settings(&config::load(&ctx.paths));
            }
        }
        s.notices.extend(super::take_notices());
        if s.notices.len() > 20 {
            let extra = s.notices.len() - 20;
            s.notices.drain(..extra);
        }
        let now = Instant::now();
        if splash.as_ref().is_some_and(|sp| sp.done(now)) {
            splash = None;
        }
        motion.observe(&s, now);
        t.terminal.draw(|f| {
            if !motion.active() {
                draw(f, &s, &ctx);
            } else if let Some(content) = draw_base(f, &s, &ctx) {
                motion.tab_in(f.buffer_mut(), content, now);
                let before = motion.growing().then(|| f.buffer_mut().clone());
                draw_overlays(f, &s, &ctx);
                if let Some(before) = before {
                    motion.pop_in(f.buffer_mut(), &before, now, &ctx.theme);
                }
            }
            if let Some(sp) = splash.as_mut() {
                let area = f.area();
                sp.draw(f.buffer_mut(), area, now, &ctx.theme);
            }
        })?;
        // The live view's motion (its heartbeat, a new node sliding in) only where it shows.
        let mut wait = match (splash.is_some(), s.tab) {
            (true, _) => splash::FRAME,
            (false, Tab::Live) => watch::wait(&s.live),
            (false, _) => term::TICK,
        };
        // A conditioning card's spinner and reveal want quicker frames while they move.
        if let Some(Modal::Conditioning(flow)) = &s.modal {
            wait = flow.frame(Instant::now()).map_or(wait, |w| w.min(wait));
        }
        if motion.active() {
            wait = wait.min(splash::FRAME);
        }
        // The Reflexes card's meters fill for a moment after the selection moves.
        if s.tab == Tab::Reflexes && ctx.theme.motion && tabs::reflexes::animating(&s.reflexes, Instant::now()) {
            wait = wait.min(tabs::reflexes::FRAME);
        }
        if let Some(k) = term::next_key_within(wait)? {
            // Any key ends the start screen at once; only the keys people press to skip it stop
            // there, and every other key acts as it would have.
            if splash.take().is_some() && splash::swallows(k.code) {
                continue;
            }
            if let Some(cmd) = s.key(k.code, k.modifiers) {
                if let Cmd::OpenRepository(directory) = cmd {
                    match repository_context(&ctx, &directory) {
                        Ok(next) => {
                            // Stop the old server and demo player before changing any data source.
                            dashboard = None;
                            before_tick = None;
                            job = None;
                            loader = Loader::spawn(next.paths.clone(), next.root.clone());
                            loader.want(Want::Routes);
                            sources = Sources::new(&next);
                            s = AppState::new(next.paths.config());
                            s.here = here_key(&next);
                            s.rewrite_marker = Some(adage_hook::rewrite_off(&next.paths));
                            s.load_settings(&config::load(&next.paths));
                            ctx = next;
                            s.message("Repository opened",vec![format!("Repository: {}",ctx.label),format!("Adage data: {}",ctx.paths.root.display()),"Choose Connect your agent, then History sources and Condition now from the palette.".into()],false);
                        }
                        Err(e) => {
                            let mut form = Form::new(FormKind::OpenRepository);
                            form.values[0] = directory.display().to_string();
                            form.error = Some(format!("{e:#}"));
                            s.modal = Some(Modal::Form(form));
                        }
                    }
                    continue;
                }
                if execute(cmd, &mut s, &ctx, &loader, &mut job, &mut dashboard) {
                    break;
                }
            }
        }
    }
    drop(t);
    Ok(())
}

/// The key of the repository `ctx` is about, as health and `adage watch --follow` compute it
/// (the index's key for the checkout, else its own).
fn here_key(ctx: &Ctx) -> Option<String> {
    let root = ctx.root.as_deref()?;
    Some(super::health::repo_key(&ctx.paths, root, &adage_tracks::Index::load(&ctx.paths.index())))
}

pub(super) fn repository_context(ctx: &Ctx, directory: &std::path::Path) -> Result<Ctx> {
    use anyhow::Context;
    let expanded = match directory.strip_prefix("~") {
        Ok(rest) => adage_store::home_dir().context("Could not find your home folder; enter a full path")?.join(rest),
        Err(_) => directory.to_path_buf(),
    };
    let path = if expanded.is_absolute() {
        expanded
    } else {
        ctx.root.clone().unwrap_or(std::env::current_dir()?).join(expanded)
    };
    let path = path.canonicalize().with_context(|| format!("{} is not an accessible folder", path.display()))?;
    if !path.is_dir() {
        anyhow::bail!("Choose a folder, not a file.");
    }
    let root = adage_core::repo::find_root(&path)
        .context("That folder is not inside a Git repository. Choose a repository folder.")?;
    if ctx.demo
        && ctx.root.as_ref().and_then(|p| p.canonicalize().ok()).is_some_and(|demo_root| root.starts_with(demo_root))
    {
        anyhow::bail!("This is the temporary demo repository. Choose your own repository folder to leave the demo.");
    }
    let paths = if ctx.demo { adage_store::Paths::from_env() } else { ctx.paths.clone() };
    let mut next = Ctx::at(paths, Some(root));
    next.theme = ctx.theme;
    Ok(next)
}
