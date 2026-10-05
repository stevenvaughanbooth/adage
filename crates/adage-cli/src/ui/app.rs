//! `adage ui`: the full-screen app. Home and a global command palette lead to Live, Routes (every installed
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
use super::palette::{self, Command, Entry, Form, FormKind, Palette};
use super::routes::{self, RouteRow, Routes};
use super::term::{self, Term};
use super::theme::{Lamp, Theme};
use super::tools::{self, Tool};
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
    Home,
    Live,
    Routes,
    Health,
    Settings,
}

const TABS: [(Tab, &str); 5] = [
    (Tab::Home, "Home"),
    (Tab::Live, "Live"),
    (Tab::Routes, "Reflexes"),
    (Tab::Health, "Health"),
    (Tab::Settings, "Settings"),
];

#[derive(Debug, Clone)]
pub enum Action {
    Review { verb: &'static str, reference: String, route: String, expected_revision: String },
    Install(Box<Plan>),
    FetchModel,
    LearnNow,
    SetConfig { key: &'static str, value: String },
    Tool(Tool),
}

pub enum Modal {
    Palette(Palette),
    Form(Form),
    Help,
    Confirm { title: String, lines: Vec<Line<'static>>, action: Action, scroll: u16 },
    Message { title: String, lines: Vec<String>, error: bool, scroll: u16 },
    Progress { title: String, status: String, ratio: f64, cancelable: bool },
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
    OpenLearningLog,
    OpenWeb,
    OpenWebDemo,
    Refresh,
    PrepareTool(Tool),
    HistorySources,
    PreviewLearning,
    OpenRepository(PathBuf),
    CancelJob,
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
    pub pending_job_action: Option<Action>,
}

impl AppState {
    pub fn new(config_path: PathBuf) -> AppState {
        AppState {
            tab: Tab::Home,
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
            pending_job_action: None,
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

    pub(super) fn complete_job(&mut self, title: &str, done: Result<Vec<String>, String>) {
        let pending = self.pending_job_action.take();
        match done {
            Ok(mut lines) if pending.is_some() => {
                lines.extend([
                    String::new(),
                    "Adage will update learned reflexes and record what changed. Inspect results in Reflexes and Learning history.".into(),
                    "Enter / y confirm · Esc / n cancel".into(),
                ]);
                self.confirm("Learn from these sources", lines.into_iter().map(Line::raw).collect(), pending.unwrap());
            }
            Ok(lines) => self.modal = Some(Modal::Viewer { title: title.into(), lines, scroll: 0 }),
            Err(error) => self.message(title, error.lines().map(String::from).collect(), true),
        }
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
            return self.modal_key(m, code, mods);
        }
        if code == KeyCode::Char(':') || (code == KeyCode::Char('k') && mods.contains(KeyModifiers::CONTROL)) {
            self.modal = Some(Modal::Palette(Palette::default()));
            return None;
        }
        match code {
            KeyCode::Char('q') => return Some(Cmd::Quit),
            KeyCode::Char('w') => return Some(Cmd::OpenWeb),
            KeyCode::Char('?') => {
                self.modal = Some(Modal::Help);
                return None;
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('0'..='4') => {
                let i = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0);
                let next = match code {
                    KeyCode::Tab => (i + 1) % TABS.len(),
                    KeyCode::BackTab => (i + TABS.len() - 1) % TABS.len(),
                    KeyCode::Char(c) => c as usize - '0' as usize,
                    _ => i,
                };
                self.tab = TABS[next].0;
                return (self.tab == Tab::Routes).then_some(Cmd::Want(WantKind::Routes));
            }
            _ => {}
        }
        match self.tab {
            Tab::Home => match code {
                KeyCode::Char('d') => Some(Cmd::OpenWebDemo),
                KeyCode::Enter => {
                    self.modal = Some(Modal::Palette(Palette::default()));
                    None
                }
                KeyCode::Char('l') => self.start_fix(Fix::LearnNow),
                KeyCode::Char('i') => {
                    self.modal = Some(Modal::Palette(Palette { query: "connect".into(), selected: 0 }));
                    None
                }
                _ => None,
            },
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

    fn modal_key(&mut self, m: Modal, code: KeyCode, mods: KeyModifiers) -> Option<Cmd> {
        let nudge = |s: &mut u16| match code {
            KeyCode::Up | KeyCode::Char('k') => *s = s.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => *s = s.saturating_add(1),
            KeyCode::PageUp => *s = s.saturating_sub(10),
            KeyCode::PageDown => *s = s.saturating_add(10),
            _ => {}
        };
        match m {
            Modal::Palette(mut p) => {
                if code == KeyCode::Esc || (code == KeyCode::Char('k') && mods.contains(KeyModifiers::CONTROL)) {
                    return None;
                }
                let entries = self.palette_entries();
                let found = palette::matches(&entries, &p.query);
                match code {
                    KeyCode::Up | KeyCode::BackTab => p.selected = p.selected.saturating_sub(1),
                    KeyCode::Down | KeyCode::Tab => p.selected = (p.selected + 1).min(found.len().saturating_sub(1)),
                    KeyCode::PageDown => p.selected = (p.selected + 8).min(found.len().saturating_sub(1)),
                    KeyCode::PageUp => p.selected = p.selected.saturating_sub(8),
                    KeyCode::Home => p.selected = 0,
                    KeyCode::End => p.selected = found.len().saturating_sub(1),
                    KeyCode::Enter => {
                        if let Some(i) = found.get(p.selected) {
                            return self.palette_command(entries[*i].command);
                        }
                    }
                    KeyCode::Backspace => {
                        p.query.pop();
                        p.selected = 0;
                    }
                    KeyCode::Char('u') if mods.contains(KeyModifiers::CONTROL) => {
                        p.query.clear();
                        p.selected = 0;
                    }
                    KeyCode::Char(c)
                        if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && p.query.len() < 128 =>
                    {
                        p.query.push(c);
                        p.selected = 0;
                    }
                    _ => {}
                }
                self.modal = Some(Modal::Palette(p));
                None
            }
            Modal::Form(mut form) => {
                match code {
                    KeyCode::Esc => return None,
                    KeyCode::BackTab | KeyCode::Up => form.selected = form.selected.saturating_sub(1),
                    KeyCode::Tab | KeyCode::Down => form.selected = (form.selected + 1) % form.values.len(),
                    KeyCode::Enter if form.selected + 1 < form.values.len() => form.selected += 1,
                    KeyCode::Enter => return self.submit_form(form),
                    KeyCode::Backspace => {
                        form.values[form.selected].pop();
                        form.error = None;
                    }
                    KeyCode::Char('u') if mods.contains(KeyModifiers::CONTROL) => form.values[form.selected].clear(),
                    KeyCode::Char(c)
                        if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                            && form.values[form.selected].len() < 4096 =>
                    {
                        form.values[form.selected].push(c);
                        form.error = None;
                    }
                    _ => {}
                }
                self.modal = Some(Modal::Form(form));
                None
            }
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
            Modal::Progress { cancelable: true, .. } if code == KeyCode::Esc => Some(Cmd::CancelJob),
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
                KeyCode::Char(c)
                    if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && buffer.len() < 4096 =>
                {
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

    pub fn palette_entries(&self) -> Vec<Entry> {
        let mut entries = vec![
            Entry::new(
                "Navigate",
                "Open a repository",
                "Choose or switch the Git workspace without leaving this app",
                Command::Form(FormKind::OpenRepository),
            ),
            Entry::new(
                "Explore",
                "Open the web map",
                "Start the local server and open the interactive app in your browser",
                Command::Web,
            ),
            Entry::new(
                "Explore",
                "Explore demo trace graph",
                "Open a sample trace in the web map; your own data is unchanged",
                Command::WebDemo,
            ),
            Entry::new(
                "Learn",
                "Learn from this repository",
                "Read agent traces, find repeated patterns, and update reflexes; preview before running",
                Command::Learn,
            ),
            Entry::new(
                "Review",
                "Review learned reflexes",
                "Inspect guidance, supporting evidence, and proposed changes",
                Command::Routes,
            ),
            Entry::new("Navigate", "Home", "Understand Adage and see your next steps", Command::Home),
            Entry::new("Navigate", "Live activity", "Watch guidance and learning as your agents work", Command::Live),
            Entry::new(
                "Navigate",
                "Health and setup",
                "Check readiness, diagnose problems, and apply fixes",
                Command::Health,
            ),
            Entry::new("Navigate", "Settings", "Change learning preferences and thresholds", Command::Settings),
            Entry::new(
                "Setup",
                "Connect Claude Code",
                "Install hooks for this repository or your user account",
                Command::Hooks("claude-code"),
            ),
            Entry::new(
                "Setup",
                "Connect Codex",
                "Install hooks for this repository or your user account",
                Command::Hooks("codex"),
            ),
            Entry::new(
                "Setup",
                "Connect OpenCode",
                "Install hooks for this repository or your user account",
                Command::Hooks("opencode"),
            ),
            Entry::new(
                "Setup",
                "Download embedding model",
                "Fetch the verified 31 MB model for matching intent",
                Command::Model,
            ),
            Entry::new(
                "Setup",
                "History sources",
                "Inspect detected and configured sources shared by Survey and Learn",
                Command::HistorySources,
            ),
            Entry::new(
                "Setup",
                "Configure Claude Code history",
                "Set history source folder when detection needs an override",
                Command::Form(FormKind::HistoryClaude),
            ),
            Entry::new(
                "Setup",
                "Configure Codex history",
                "Set history source folder when detection needs an override",
                Command::Form(FormKind::HistoryCodex),
            ),
            Entry::new(
                "Setup",
                "Configure OpenCode history",
                "Set history source to a folder of existing session exports",
                Command::Form(FormKind::HistoryOpenCode),
            ),
            Entry::new(
                "Setup",
                "Import OpenCode history",
                "Copy local session exports for this repository; run again after new sessions",
                Command::ImportOpenCode,
            ),
            Entry::new(
                "Setup",
                "Install a local model",
                "Use model files already on your computer",
                Command::Form(FormKind::InstallModel),
            ),
            Entry::new(
                "Learn",
                "Enable automatic learning",
                "Learn whenever an agent session stops or ends",
                Command::AutoLearn,
            ),
            Entry::new(
                "Inspect",
                "Installed collections",
                "Browse reflex collections and evidence across all repositories",
                Command::Status,
            ),
            Entry::new(
                "Inspect",
                "Survey session history",
                "Analyze configured and detected agent history for this repository",
                Command::Survey,
            ),
            Entry::new(
                "Inspect",
                "Outcome report",
                "See delivered guidance, observed responses, and control opportunities",
                Command::Report,
            ),
            Entry::new(
                "Inspect",
                "Reflex trace graph",
                "Read the connections between learned reflexes and their triggers",
                Command::Graph,
            ),
            Entry::new(
                "Inspect",
                "Evaluate a dataset",
                "Replay a folder of evaluation traces and inspect results",
                Command::Form(FormKind::Evaluate),
            ),
            Entry::new(
                "Collections",
                "Import a signed pack",
                "Verify and install reflexes from a signed pack folder",
                Command::Form(FormKind::AddTrack),
            ),
            Entry::new(
                "Collections",
                "Export a signed pack",
                "Package a reflex collection using a key from your SSH signing agent",
                Command::Form(FormKind::ShareTrack),
            ),
            Entry::new(
                "Logs",
                "Learning history",
                "Read recent learning runs, changes, and errors",
                Command::LearningLog,
            ),
            Entry::new(
                "Logs",
                "Hook error log",
                "Inspect fail-open errors; agents continue working if a hook fails",
                Command::Errors,
            ),
            Entry::new("Inspect", "Refresh all data", "Reload health checks, reflexes, and settings", Command::Refresh),
            Entry::new(
                "Live",
                if self.live.paused { "Resume live activity" } else { "Pause live activity" },
                "Pause only the display; agents and learning keep running",
                Command::Pause,
            ),
            Entry::new(
                "Live",
                if self.live.all_repos { "Show this repository" } else { "Show all repositories" },
                "Change the scope of the live feed",
                Command::Scope,
            ),
            Entry::new(
                "Live",
                if self.live.hide_quiet { "Show quiet events" } else { "Hide quiet events" },
                "Choose whether to include events without guidance",
                Command::Quiet,
            ),
            Entry::new("Help", "Keyboard shortcuts", "Learn the keys for your current screen", Command::Help),
            Entry::new("App", "Quit Adage", "Close this terminal app and its local web server", Command::Quit),
        ];
        if self.tab == Tab::Routes {
            if let Some(route) = self.selected_route() {
                for (key, verb) in routes::ACTIONS {
                    if route.verbs.contains(&verb) {
                        entries.insert(
                            0,
                            Entry::new(
                                "Selected reflex",
                                format!("{} {}", route_review_label(verb, route), route.id),
                                verb_effect(verb),
                                Command::Review(key),
                            ),
                        );
                    }
                }
            }
        }
        for setting in &self.settings {
            entries.push(Entry::new(
                "Setting",
                format!("Edit {}", setting.key),
                setting.doc,
                Command::Setting(setting.key),
            ));
        }
        for (index, route) in self.route_rows().iter().enumerate() {
            entries.push(Entry::new(
                "Reflex",
                format!("Open reflex {}", route.id),
                format!("{} · {}", route.reference, route.trigger),
                Command::Route(index),
            ));
        }
        entries
    }

    fn palette_command(&mut self, command: Command) -> Option<Cmd> {
        match command {
            Command::Home => self.tab = Tab::Home,
            Command::Live => self.tab = Tab::Live,
            Command::Routes => {
                self.tab = Tab::Routes;
                self.route_sel = self.route_rows().iter().position(|r| !r.reasons.is_empty()).unwrap_or(0);
                self.detail_scroll = 0;
                return Some(Cmd::Want(WantKind::Routes));
            }
            Command::Health => {
                self.tab = Tab::Health;
                return Some(Cmd::Want(WantKind::Health));
            }
            Command::Settings => self.tab = Tab::Settings,
            Command::Web => return Some(Cmd::OpenWeb),
            Command::WebDemo => return Some(Cmd::OpenWebDemo),
            Command::Learn => return self.start_fix(Fix::LearnNow),
            Command::Model => return self.start_fix(Fix::FetchModel),
            Command::Hooks(harness) => return self.start_fix(Fix::InstallHooks(harness.into())),
            Command::AutoLearn => return self.start_fix(Fix::EnableAutoLearn),
            Command::Errors => return Some(Cmd::OpenFailOpen),
            Command::LearningLog => return Some(Cmd::OpenLearningLog),
            Command::Refresh => return Some(Cmd::Refresh),
            Command::Pause => {
                self.live.toggle_pause();
                self.tab = Tab::Live;
            }
            Command::Scope => {
                self.live.all_repos = !self.live.all_repos;
                self.tab = Tab::Live;
            }
            Command::Quiet => {
                self.live.hide_quiet = !self.live.hide_quiet;
                self.tab = Tab::Live;
            }
            Command::Review(key) => return self.routes_key(KeyCode::Char(key)),
            Command::Route(index) => {
                self.tab = Tab::Routes;
                self.route_sel = index;
                self.detail_scroll = 0;
            }
            Command::Setting(key) => {
                self.tab = Tab::Settings;
                if let Some(index) = self.settings.iter().position(|s| s.key == key) {
                    self.setting_sel = index;
                    let setting = &self.settings[index];
                    self.modal = Some(Modal::Edit { key, buffer: setting.value.clone(), error: None });
                }
            }
            Command::Status => return Some(Cmd::Do(Action::Tool(Tool::Status))),
            Command::Survey => return Some(Cmd::Do(Action::Tool(Tool::Survey))),
            Command::HistorySources => return Some(Cmd::HistorySources),
            Command::ImportOpenCode => return Some(Cmd::PrepareTool(Tool::ExportOpenCode)),
            Command::Report => return Some(Cmd::Do(Action::Tool(Tool::Report))),
            Command::Graph => return Some(Cmd::Do(Action::Tool(Tool::Graph))),
            Command::Form(kind) => {
                let mut form = Form::new(kind);
                if kind == FormKind::ShareTrack {
                    if let Some(route) = self.selected_route() {
                        form.values[0] = route.reference.clone();
                    }
                }
                self.modal = Some(Modal::Form(form));
            }
            Command::Help => self.modal = Some(Modal::Help),
            Command::Quit => return Some(Cmd::Quit),
        }
        None
    }

    fn submit_form(&mut self, mut form: Form) -> Option<Cmd> {
        if let Some(index) = form.values.iter().position(|value| value.trim().is_empty()) {
            form.selected = index;
            form.error = Some(format!("Enter {} to continue.", form.kind.fields()[index].0.to_lowercase()));
            self.modal = Some(Modal::Form(form));
            return None;
        }
        let value = |index: usize| form.values[index].trim().to_string();
        let tool = match form.kind {
            FormKind::OpenRepository => return Some(Cmd::OpenRepository(value(0).into())),
            FormKind::Evaluate => Tool::Eval(value(0).into()),
            FormKind::InstallModel => Tool::InstallModel(value(0).into()),
            FormKind::HistoryClaude => {
                Tool::SetHistorySource { harness: "claude-code".into(), directory: value(0).into() }
            }
            FormKind::HistoryCodex => Tool::SetHistorySource { harness: "codex".into(), directory: value(0).into() },
            FormKind::HistoryOpenCode => {
                Tool::SetHistorySource { harness: "opencode".into(), directory: value(0).into() }
            }
            FormKind::AddTrack => {
                let trust = match value(1).to_lowercase().as_str() {
                    "true" => true,
                    "false" => false,
                    _ => {
                        form.selected = 1;
                        form.error = Some("Trust signer must be true or false.".into());
                        self.modal = Some(Modal::Form(form));
                        return None;
                    }
                };
                Tool::AddTrack { directory: value(0).into(), trust, expected_digest: None }
            }
            FormKind::ShareTrack => Tool::ShareTrack { track: value(0), output: value(1).into(), key: None },
        };
        if tool.mutates() {
            Some(Cmd::PrepareTool(tool))
        } else {
            Some(Cmd::Do(Action::Tool(tool)))
        }
    }

    fn routes_key(&mut self, code: KeyCode) -> Option<Cmd> {
        let n = self.route_rows().len();
        match code {
            KeyCode::Char('/') => {
                self.modal = Some(Modal::Palette(Palette { query: "Open reflex ".into(), selected: 0 }));
            }
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
                let Some(preview) = &r.review_preview else {
                    self.message(
                        "Reload before reviewing",
                        vec!["Reload this reflex to inspect its current version and proposed changes.".into()],
                        true,
                    );
                    return Some(Cmd::Want(WantKind::Routes));
                };
                let proposed = preview.proposal.is_some() && verb == "approve";
                let title = if proposed { "Approve proposed change" } else { "Review reflex" };
                let mut lines = vec![
                    Line::from(vec![
                        Span::raw(format!(
                            "{} ",
                            if proposed { "Apply the proposed change to" } else { review_label(verb) }
                        )),
                        Span::styled(r.id.clone(), t.bold()),
                        Span::raw(format!(" in {}?", r.reference)),
                    ]),
                    Line::raw(""),
                ];
                if proposed {
                    lines.push(Line::raw(
                        "Approval replaces the current reflex with the proposed version. It stays pinned.",
                    ));
                } else {
                    lines.push(Line::raw(verb_effect(verb)));
                    if preview.proposal.is_some() {
                        lines.push(Line::raw("The proposed version below will not be installed by this decision."));
                    }
                }
                lines.extend(review_preview_lines(preview, &t));
                lines.push(Line::raw(""));
                lines.push(Line::raw("The decision is saved in review history and the index is rebuilt."));
                lines
                    .push(Line::raw("If this reflex or its proposal changes before confirmation, nothing is written."));
                lines.push(Line::raw(""));
                lines.push(Line::from(Span::styled(
                    if proposed {
                        "y approve proposed change · n cancel · PgDn/PgUp scroll"
                    } else {
                        "y confirm · n cancel · PgDn/PgUp scroll"
                    },
                    t.dim(),
                )));
                self.confirm(
                    title,
                    lines,
                    Action::Review {
                        verb,
                        reference: r.reference.clone(),
                        route: r.id.clone(),
                        expected_revision: preview.revision.clone(),
                    },
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
            KeyCode::Enter => {
                let health = self.health.as_ref()?;
                let fix = health.fixes.get(self.check_sel).cloned().flatten();
                if fix.is_none() {
                    if let Some(check) = health.checks.get(self.check_sel).filter(|check| check.fix.is_some()) {
                        let title = check.check.clone();
                        let lines = vec![check.detail.clone(), String::new(), check.fix.clone().unwrap()];
                        self.message(&title, lines, false);
                    }
                }
                fix
            }
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
                        Line::raw("Download the verified 31 MB model into Adage's data directory."),
                        Line::raw("It helps Adage recognize similar intentions across your traces."),
                        Line::raw(""),
                        hint,
                    ],
                    Action::FetchModel,
                );
                None
            }
            Fix::LearnNow => Some(Cmd::PreviewLearning),
            Fix::SetHistorySource(harness) => self.palette_command(Command::Form(match harness.as_str() {
                "claude-code" => FormKind::HistoryClaude,
                "codex" => FormKind::HistoryCodex,
                _ => FormKind::HistoryOpenCode,
            })),
            Fix::ImportOpenCode => Some(Cmd::PrepareTool(Tool::ExportOpenCode)),
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
    match a {
        adage_tracks::Aspect::Green => "fire",
        adage_tracks::Aspect::Yellow => "prime",
        adage_tracks::Aspect::Red => "inhibit",
    }
}

fn strength_label(value: &str) -> &str {
    match value {
        "green" => "fire",
        "yellow" => "prime",
        "red" => "inhibit",
        other => other,
    }
}

fn lifecycle_label(value: &str) -> &str {
    match value {
        "draft" => "nascent (awaiting review)",
        "probation" => "forming",
        "active" => "firm",
        "retired" => "extinguished",
        "demoted" => "weakened",
        other => other,
    }
}

fn review_label(verb: &str) -> &str {
    match verb {
        "promote" => "strengthen",
        "demote" => "weaken",
        "reject" => "extinguish",
        other => other,
    }
}

fn route_review_label<'a>(verb: &'a str, route: &routes::RouteRow) -> &'a str {
    if verb == "approve" && route.review_preview.as_ref().is_some_and(|preview| preview.proposal.is_some()) {
        "approve proposed change to"
    } else {
        review_label(verb)
    }
}

fn verb_effect(verb: &str) -> &'static str {
    match verb {
        "approve" => "Approve: accept this reflex, apply its proposed change, or start using a new reflex.",
        "reject" => {
            "Extinguish: retire the reflex; learning keeps it retired unless its evidence strengthens materially."
        }
        "demote" => "Weaken: fire → prime. Suggest this response while retaining the review decision.",
        "promote" => "Strengthen: prime → fire. Direct this response, within its allowed effect ceiling.",
        "pin" => "Pin: mark it authored; learning proposes changes instead of overwriting it.",
        "dismiss" => "Dismiss: keep the pinned reflex and silence its proposed change.",
        _ => "",
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

pub fn draw(f: &mut Frame, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let area = f.area();
    f.render_widget(Block::default().style(t.base()), area);
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
        Tab::Home => draw_home(f, body, s, ctx),
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
    if area.width < 65 {
        let name = TABS.iter().find(|(tab, _)| *tab == s.tab).map(|(_, name)| *name).unwrap_or("Home");
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" adage ", t.title()),
                Span::styled(name, t.selected()),
                Span::styled(" · Tab screens · : commands", t.dim()),
            ])),
            area,
        );
        return;
    }
    let mut spans = vec![Span::styled(" adage ", t.title())];
    if ctx.demo {
        spans.push(Span::styled("demo ", t.holdout()));
    }
    for (i, (tab, name)) in TABS.iter().enumerate() {
        let mut label = format!(" {i} {name} ");
        if *tab == Tab::Health && area.width >= 90 {
            if let Some(h) = &s.health {
                let bad = h.count(Status::Warn) + h.count(Status::Fail);
                if bad > 0 {
                    label = format!(" {i} {name} ({bad}) ");
                }
            }
        }
        if *tab == Tab::Routes && area.width >= 90 {
            if let Some(Ok(r)) = &s.routes {
                let q = r.rows.iter().filter(|r| !r.reasons.is_empty()).count();
                if q > 0 {
                    label = format!(" {i} {name} ({q}) ");
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
        Tab::Home => "Enter commands",
        Tab::Live => "p pause · f scope · v quiet",
        Tab::Routes => "/ find · a/r review · R reload",
        Tab::Health => "Enter fix · R recheck",
        Tab::Settings => "Enter edit · Space toggle",
    };
    let left = Line::from(vec![
        Span::styled(format!(" {} ", ctx.label), t.bold()),
        Span::styled(format!("· gen {gen} · {routes} reflexes · auto-learn {auto}"), t.dim()),
    ]);
    // The keys stay whole at the right; the state on the left gives way.
    let right = if area.width < 65 {
        " : commands · w web · ? help · q quit ".to_string()
    } else if area.width < 105 {
        format!(" {keys} · : commands · w web · q quit ")
    } else {
        format!(" {keys} · Ctrl+K commands · w web · ? help · q quit ")
    };
    let right_w = (right.chars().count() as u16).min(area.width);
    let [l, r] = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_w)]).areas(area);
    f.render_widget(Paragraph::new(left), l);
    f.render_widget(Paragraph::new(Span::styled(right, t.dim())), r);
}

fn draw_home(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let routes = s.health.as_ref().map(|h| h.routes_repo.unwrap_or(h.routes_total));
    let review = s.route_rows().iter().filter(|r| !r.reasons.is_empty()).count();
    let issues = s.health.as_ref().map(|h| h.count(Status::Fail) + h.count(Status::Warn));
    let mut lines = vec![
        Line::from(Span::styled("Agent traces become reflexes.", t.title())),
        Line::raw("Repeated work becomes a learned response, with evidence you can review."),
        Line::raw(""),
        Line::from(vec![Span::styled("Workspace  ", t.dim()), Span::styled(ctx.label.clone(), t.bold())]),
        Line::from(vec![
            Span::styled("Readiness  ", t.dim()),
            Span::raw(match issues {
                Some(0) => "All health checks passed".into(),
                Some(n) => format!("{n} setup checks need attention · 3 opens Health"),
                None => "Checking your setup…".into(),
            }),
        ]),
        Line::from(vec![
            Span::styled("Guidance   ", t.dim()),
            Span::raw(match routes {
                Some(n) => {
                    let scope = if s.health.as_ref().is_some_and(|health| health.routes_repo.is_some()) {
                        "here"
                    } else {
                        "installed"
                    };
                    format!("{n} reflexes {scope} · {review} to review across all repositories")
                }
                None => "Loading learned reflexes…".into(),
            }),
        ]),
        Line::raw(""),
        Line::from(Span::styled("Connect → Learn → Review → Observe", t.bold())),
        Line::raw("i  Connect your coding agent, then check History sources in : commands."),
        Line::raw("l  Learn reflexes from repeated patterns in this repository."),
        Line::raw("2  Review each reflex, its evidence, and its response."),
        Line::raw("1  Observe which reflexes fire and how your agents respond."),
        Line::raw(""),
        Line::from(Span::styled("Reflexes are learned. Instincts are built in.", t.bold())),
        Line::raw("This build exposes learned reflexes; instincts are not available yet."),
        Line::raw(""),
        Line::from(Span::styled("Ctrl+K / :  Search every command", t.title())),
        Line::from(Span::styled("w  Open the web map · d  Explore a demo trace", t.accent().patch(t.bold()))),
        Line::raw("The local server opens your browser automatically."),
    ];
    if ctx.root.is_none() && !ctx.demo {
        lines.insert(
            7,
            Line::from(Span::styled("No Git repository selected. Press : and choose Open a repository.", t.yellow())),
        );
    }
    if area.height < 20 {
        lines = vec![
            Line::from(Span::styled("Agent traces become reflexes.", t.title())),
            Line::raw(format!("Workspace: {}", ctx.label)),
            Line::raw("i Connect → l Learn → 2 Reflexes → 1 Live"),
            Line::raw(""),
            Line::from(Span::styled("Enter / :  Search every command", t.bold())),
            Line::from(Span::styled("w Web map · d Demo trace", t.accent())),
            Line::raw("3 Health and setup · 4 Settings"),
        ];
    }
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        area.inner(ratatui::layout::Margin { horizontal: 2, vertical: 1 }),
    );
}

fn kv(k: &str, v: String, t: &Theme) -> Line<'static> {
    Line::from(vec![Span::styled(format!("{k:<11}"), t.dim()), Span::raw(v)])
}

pub fn route_detail(r: &RouteRow, t: &Theme) -> Vec<Line<'static>> {
    let l = &r.live;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{} ", t.aspect_mark(r.aspect)), t.aspect(r.aspect)),
            Span::styled(r.id.clone(), t.bold()),
            Span::styled(
                format!(
                    "  {} · {} (ceiling {})",
                    aspect_name(r.aspect),
                    match r.aspect {
                        adage_tracks::Aspect::Green => "do this",
                        adage_tracks::Aspect::Yellow => "consider this",
                        adage_tracks::Aspect::Red => "avoid this",
                    },
                    strength_label(&r.ceiling)
                ),
                t.aspect(r.aspect),
            ),
        ]),
        kv("collection", format!("{} @ {}{}", r.track, r.version, if r.imported { " (imported)" } else { "" }), t),
        kv("repo", r.repo.clone(), t),
        kv(
            "state",
            format!(
                "{} · effect {}{}{}",
                lifecycle_label(&r.state),
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
            format!("{}/{} traces · rate {:.2} · Wilson lb {:.2}", r.support, r.opportunities, r.rate, r.lower_bound),
            t,
        ),
        kv(
            "live",
            format!("delivered {} · followed {} · not observed {} · met {}", l.emitted, l.followed, l.derailed, l.met),
            t,
        ),
        kv("control", format!("withheld {} · unaided {} · not observed {}", l.would_have, l.unaided, l.not_reached), t),
        kv("counts", r.measurement_info.clone(), t),
        Line::raw("Not observed: the expected action was absent in the next 3 observed tool batches. Codex and OpenCode report per call; this is not a full-session verdict."),
    ];
    if !r.historical_versions.is_empty() {
        lines.push(kv(
            "history",
            format!(
                "{} · Open the web map for version-specific evidence and contributing traces.",
                r.historical_versions.join(", ")
            ),
            t,
        ));
    }
    for (k, d) in &r.reasons {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<11}", "review"), t.dim()),
            Span::styled(format!("{k}: "), t.yellow().patch(t.bold())),
            Span::raw(d.clone()),
        ]));
    }
    let keys: Vec<String> = routes::ACTIONS
        .iter()
        .filter(|(_, v)| r.verbs.contains(v))
        .map(|(k, v)| {
            format!(
                "{k} {}",
                if *v == "approve" && r.review_preview.as_ref().is_some_and(|p| p.proposal.is_some()) {
                    "approve proposed change"
                } else {
                    review_label(v)
                }
            )
        })
        .collect();
    lines.push(kv("keys", keys.join(" · "), t));
    lines.push(Line::raw(""));
    if let Some(preview) = &r.review_preview {
        lines.extend(review_preview_lines(preview, t));
    } else {
        lines.push(Line::from(Span::styled("current directive preview (example values):", t.dim())));
        for d in r.directive.lines() {
            lines.push(Line::from(Span::styled(format!("  {d}"), t.aspect(r.aspect))));
        }
    }
    lines
}

fn review_preview_lines(preview: &crate::review::ReviewPreview, t: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(proposal) = &preview.proposal {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(format!("Proposed change · {}", proposal.on), t.bold())));
        lines.push(Line::raw(proposal.reason.clone()));
        lines.push(Line::raw("Current → proposed fields:"));
        for (field, before, after) in &proposal.diff {
            let display = |value: &str| match *field {
                "aspect" => strength_label(value).to_string(),
                "state" => lifecycle_label(value).to_string(),
                _ => value.to_string(),
            };
            lines.push(Line::from(Span::styled(format!("  {field}"), t.bold())));
            lines.push(Line::raw(format!("    Current:  {}", display(before))));
            lines.push(Line::raw(format!("    Proposed: {}", display(after))));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled("Proposed directive preview (example values):", t.bold())));
        lines.extend(proposal.directive.lines().map(|line| Line::raw(format!("  {line}"))));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("Current directive preview (example values):", t.dim())));
    lines.extend(preview.current_directive.lines().map(|line| Line::raw(format!("  {line}"))));
    lines
}

fn draw_routes(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let rows = match &s.routes {
        None => {
            f.render_widget(Paragraph::new(Span::styled("loading reflexes…", t.dim())), area);
            return;
        }
        Some(Err(e)) => {
            f.render_widget(Paragraph::new(Span::styled(format!("could not load reflexes: {e}"), t.red())), area);
            return;
        }
        Some(Ok(r)) => &r.rows,
    };
    if rows.is_empty() {
        let lines = vec![
            Line::raw("No learned reflexes yet."),
            Line::from(Span::styled(
                "Press : and choose Learn from this repository to build reflexes from agent traces.",
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
        let fixable = h.fixes.get(i).is_some_and(|f| f.is_some()) || c.fix.is_some();
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", t.lamp_mark(lamp)), t.lamp(lamp)),
            Span::styled(format!("{:<name_w$}", c.check), if sel { t.selected() } else { t.bold() }),
            Span::styled(if fixable { " ⏎ " } else { "   " }, t.accent()),
            Span::raw(view::display_counts(&c.detail)),
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
        d.push(Line::raw(view::display_counts(&c.detail)));
        if let Some(Some(fix)) = h.fixes.get(s.check_sel) {
            d.push(Line::from(Span::styled(format!("Enter: {}", fix.label()), t.accent().patch(t.bold()))));
        } else if let Some(fx) = &c.fix {
            d.push(Line::from(vec![Span::styled("detail: ", t.dim()), Span::raw(fx.clone())]));
            d.push(Line::raw("Enter: read full setup instructions · : find setup actions"));
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
            "Choose a setting and press Enter to edit. Environment variables override saved values.",
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
    let mut selected_line = 0;
    for (i, row) in s.settings.iter().enumerate() {
        let sel = i == s.setting_sel;
        if sel {
            selected_line = lines.len();
        }
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
    // Measure wrapping so the selected setting and its explanation stay visible on short terminals.
    let wrapped_before: usize =
        lines.iter().take(selected_line).map(|line| line.width().max(1).div_ceil(area.width.max(1) as usize)).sum();
    let offset = wrapped_before.saturating_sub(area.height.saturating_sub(4) as usize);
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((offset.min(u16::MAX as usize) as u16, 0)),
        area,
    );
}

fn app_help(tab: Tab, t: &Theme) -> Vec<Line<'static>> {
    let k = |key: &str, what: &str| {
        Line::from(vec![Span::styled(format!("{key:>9}  "), t.bold()), Span::raw(what.to_string())])
    };
    let mut v = vec![
        k("Ctrl+K :", "search every command"),
        k("0-4 Tab", "switch screens (0 is Home)"),
        k("w", "start and open the web map"),
        k("q", "quit"),
        k("?", "this help"),
        Line::raw(""),
    ];
    match tab {
        Tab::Home => {
            v.push(k("d", "explore a demo trace in the web map"));
            v.push(k("Enter", "open the command menu"));
            v.push(k("i", "connect a coding agent"));
            v.push(k("l", "learn from this repository"));
        }
        Tab::Live => v.extend(view::watch_help(t).into_iter().skip(1)),
        Tab::Routes => {
            v.push(k("/", "find a reflex by name, collection, or trigger"));
            v.push(k("↑ ↓", "select a reflex"));
            v.push(k("PgUp PgDn", "scroll the detail"));
            for (c, verb) in routes::ACTIONS {
                v.push(k(&c.to_string(), verb_effect(verb)));
            }
            v.push(k("R", "reload"));
            v.push(Line::from(Span::styled("* marks reflexes awaiting review", t.dim())));
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
        Modal::Palette(p) => draw_palette(f, area, p, s, t),
        Modal::Form(form) => {
            let mut lines = vec![Line::raw("Paths may be absolute, relative to this repository, or start with ~/.")];
            for (index, ((label, hint), value)) in form.kind.fields().iter().zip(&form.values).enumerate() {
                lines.push(Line::raw(""));
                lines.push(Line::from(Span::styled(label.to_string(), t.bold())));
                lines.push(Line::from(Span::styled(hint.to_string(), t.dim())));
                let style = if index == form.selected { t.selected() } else { Style::default() };
                let cursor = if index == form.selected { "▏" } else { "" };
                lines.push(Line::from(Span::styled(format!("> {value}{cursor}"), style)));
            }
            if let Some(error) = &form.error {
                lines.push(Line::from(Span::styled(error.clone(), t.red())));
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "Tab / ↑ ↓ field · Enter next / continue · Ctrl+U clear · Esc cancel",
                t.dim(),
            )));
            // Keep the focused field visible when a form is taller than the terminal.
            let scroll = if area.height < 18 { form.selected.saturating_mul(5) as u16 } else { 0 };
            view::popup(f, area, form.kind.title(), lines, t, scroll);
        }
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
        Modal::Progress { title, status, ratio, cancelable } => {
            let r = view::centered(area, 64.min(area.width.saturating_sub(2)), 6);
            f.render_widget(ratatui::widgets::Clear, r);
            let block = Block::default()
                .style(t.surface())
                .borders(Borders::ALL)
                .title(Span::styled(format!(" {title} "), t.title()));
            let inner = block.inner(r);
            f.render_widget(block, r);
            let [g, st, hint] =
                Layout::vertical([Constraint::Length(1), Constraint::Length(2), Constraint::Length(1)]).areas(inner);
            f.render_widget(Gauge::default().gauge_style(t.accent()).ratio(*ratio), g);
            f.render_widget(Paragraph::new(Span::styled(status.clone(), t.dim())).wrap(Wrap { trim: true }), st);
            f.render_widget(
                Paragraph::new(Span::styled(
                    if *cancelable { "Esc cancels this report" } else { "Please wait while changes are saved" },
                    t.dim(),
                )),
                hint,
            );
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

fn draw_palette(f: &mut Frame, area: Rect, p: &Palette, s: &AppState, t: &Theme) {
    let r = view::centered(area, 90.min(area.width.saturating_sub(2)), 22.min(area.height));
    f.render_widget(ratatui::widgets::Clear, r);
    let block = Block::default()
        .style(t.surface())
        .borders(Borders::ALL)
        .border_style(t.accent())
        .title(Span::styled(" Commands ", t.title()));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let [search, count, list, detail, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(if inner.height >= 12 { 3 } else { 1 }),
        Constraint::Length(1),
    ])
    .areas(inner);
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(" > ", t.accent()), Span::raw(format!("{}▏", p.query))])),
        search,
    );
    let entries = s.palette_entries();
    let found = palette::matches(&entries, &p.query);
    f.render_widget(
        Paragraph::new(Span::styled(format!(" {} commands · type to search", found.len()), t.dim())),
        count,
    );
    if found.is_empty() {
        f.render_widget(
            Paragraph::new(" No matches. Try “model”, “connect”, or “logs”.").wrap(Wrap { trim: true }),
            list,
        );
    } else {
        let offset = p.selected.saturating_sub(list.height.saturating_sub(1) as usize);
        let lines: Vec<Line> = found
            .iter()
            .enumerate()
            .skip(offset)
            .take(list.height as usize)
            .map(|(index, i)| {
                let entry = &entries[*i];
                let mark = if index == p.selected { "›" } else { " " };
                let label = if list.width >= 64 {
                    format!(" {mark} {:<14} {}", entry.group, entry.title)
                } else {
                    format!(" {mark} {}", entry.title)
                };
                Line::from(Span::styled(label, if index == p.selected { t.selected() } else { Style::default() }))
            })
            .collect();
        f.render_widget(Paragraph::new(lines), list);
        if let Some(i) = found.get(p.selected) {
            f.render_widget(
                Paragraph::new(Span::styled(format!(" {}", entries[*i].detail), t.dim())).wrap(Wrap { trim: true }),
                detail,
            );
        }
    }
    let hint = if keys.width >= 64 {
        " ↑ ↓ choose · Enter run · Ctrl+U clear · Esc close"
    } else {
        " ↑ ↓ choose · Enter run · Esc close"
    };
    f.render_widget(Paragraph::new(Span::styled(hint, t.accent())), keys);
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

/// A scripted demo can review its own fixture, but setup belongs to an explicitly opened
/// real workspace. In particular, user-scope hook installs must never escape the demo.
pub(super) fn needs_real_workspace(cmd: &Cmd) -> bool {
    match cmd {
        Cmd::PlanInstall { .. } | Cmd::PrepareTool(_) | Cmd::PreviewLearning | Cmd::HistorySources => true,
        Cmd::Do(Action::Tool(tool)) => tool.mutates() || matches!(tool, Tool::Survey),
        Cmd::Do(Action::Install(_) | Action::FetchModel | Action::LearnNow | Action::SetConfig { .. }) => true,
        _ => false,
    }
}

/// Carry out a command. Returns `true` to quit.
fn execute(
    cmd: Cmd,
    s: &mut AppState,
    ctx: &Ctx,
    loader: &Loader,
    job: &mut Option<Job>,
    dashboard: &mut Option<crate::serve::Dashboard>,
) -> bool {
    let t = &ctx.theme;
    if ctx.demo && needs_real_workspace(&cmd) {
        s.message("Open your repository to set up Adage", vec![
            "You are exploring a scripted demo. Setup and learning start in your own workspace.".into(),
            "Press : and choose Open a repository to leave the demo. Adage will use your normal data directory and keep this temporary demo separate.".into(),
        ], false);
        return false;
    }
    match cmd {
        Cmd::Quit => return true,
        Cmd::OpenRepository(_) => unreachable!("repository changes are handled by the event loop"),
        Cmd::CancelJob => {
            *job = None;
            s.pending_job_action = None;
            s.message("Report canceled", vec!["No changes were made.".into()], false);
        }
        Cmd::HistorySources => {
            let mut lines = crate::detect::history_summary(&config::load(&ctx.paths).config);
            lines.push(String::new());
            lines.push("Use Configure Claude Code / Codex / OpenCode history to select a different folder.".into());
            lines.push(
                "Use Import OpenCode history to copy this repository's local sessions into an exports folder.".into(),
            );
            lines.push(
                "Then Survey session history, or Learn from this repository to preview readable session counts.".into(),
            );
            s.modal = Some(Modal::Viewer { title: "History sources".into(), lines, scroll: 0 });
        }
        Cmd::PreviewLearning => {
            let Some(root) = &ctx.root else {
                s.message(
                    "Choose a repository first",
                    vec!["Press : and choose Open a repository, then learn from its history.".into()],
                    false,
                );
                return false;
            };
            let j = jobs::preview_learning(ctx.paths.clone(), root.clone());
            s.pending_job_action = Some(Action::LearnNow);
            s.modal = Some(Modal::Progress {
                title: j.title.clone(),
                status: j.status.clone(),
                ratio: 0.0,
                cancelable: true,
            });
            *job = Some(j);
        }
        Cmd::Want(WantKind::Routes) => loader.want(Want::Routes),
        Cmd::Want(WantKind::Health) => loader.want(Want::Health),
        Cmd::Refresh => {
            loader.want(Want::Health);
            loader.want(Want::Routes);
            s.load_settings(&config::load(&ctx.paths));
            s.message("Refreshing", vec!["Health, reflexes, and settings are being refreshed.".into()], false);
        }
        Cmd::OpenWeb | Cmd::OpenWebDemo => {
            let demo = matches!(cmd, Cmd::OpenWebDemo);
            if dashboard.is_none() {
                match crate::serve::Dashboard::start(&ctx.paths) {
                    Ok(server) => *dashboard = Some(server),
                    Err(error) => {
                        s.message(
                            "Could not start the web map",
                            vec![format!("{error:#}"), "Press w to try again.".into()],
                            true,
                        );
                        return false;
                    }
                }
            }
            let server = dashboard.as_ref().unwrap();
            match if demo { server.open_demo() } else { server.open() } {
                Ok(url) => s.message(
                    "Web map opened",
                    vec![
                        url,
                        "Keep this terminal app open while using the dashboard.".into(),
                        "Press w any time to open it again.".into(),
                    ],
                    false,
                ),
                Err(error) => s.message(
                    "Web map is ready",
                    vec![
                        format!("Browser could not open: {error:#}"),
                        format!("Open {} in your browser, or press w to retry.", server.url()),
                    ],
                    true,
                ),
            }
        }
        Cmd::PrepareTool(mut tool) => {
            let mut lines = vec![Line::raw("Review before writing files:"), Line::raw("")];
            match &mut tool {
                Tool::AddTrack { directory, trust, expected_digest } => {
                    match tools::import_summary(directory, ctx.root.as_deref()) {
                        Ok((summary, digest)) => {
                            lines.extend(summary.into_iter().map(Line::raw));
                            *expected_digest = Some(digest);
                        }
                        Err(error) => {
                            s.message(
                                "Cannot import this pack",
                                format!("{error:#}").lines().map(String::from).collect(),
                                true,
                            );
                            return false;
                        }
                    }
                    lines.push(Line::raw(if *trust {
                        "Explicitly trust this signing identity for future imports: YES"
                    } else {
                        "Only an already trusted signing identity will be accepted."
                    }));
                }
                Tool::InstallModel(source) => lines.push(Line::raw(format!(
                    "Install model files from {} into Adage's data directory.",
                    source.display()
                ))),
                Tool::ShareTrack { track, output, .. } => {
                    lines.push(Line::raw(format!("Export {track} to {}", output.display())));
                    lines.push(Line::raw(
                        "Sign the copy using your SSH agent. Existing destination files are protected.",
                    ));
                }
                Tool::SetHistorySource { harness, directory } => {
                    match tools::history_source_directory(directory, ctx.root.as_deref()) {
                        Ok(path) => *directory = path,
                        Err(error) => {
                            s.message("Cannot use this history folder", vec![format!("{error:#}")], true);
                            return false;
                        }
                    }
                    lines.push(Line::raw(format!("Use {} for {harness} history.", directory.display())));
                    lines.push(Line::raw(
                        "Save this source in Adage's config for Survey, Learn now, and automatic learning.",
                    ));
                    lines.push(Line::raw(
                        "This overrides automatic detection across repositories. Original session files are unchanged.",
                    ));
                }
                Tool::ExportOpenCode => {
                    let Some(root) = &ctx.root else {
                        s.message(
                            "Choose a repository first",
                            vec!["Press : and choose Open a repository before importing its history.".into()],
                            false,
                        );
                        return false;
                    };
                    let cfg = config::load(&ctx.paths).config;
                    let destination =
                        cfg.learn_opencode_exports.unwrap_or_else(|| ctx.paths.root.join("history/opencode"));
                    lines.push(Line::raw(format!("Repository: {}", root.display())));
                    lines.push(Line::raw(
                        "Use OpenCode's local export command to copy up to 50 matching sessions from its latest 500.",
                    ));
                    lines.push(Line::raw(format!(
                        "Save or refresh exports in {} and configure this history source.",
                        destination.display()
                    )));
                    lines.push(Line::raw("Original sessions are unchanged. Run Import OpenCode history again after new sessions; hooks do not export them automatically."));
                }
                _ => {}
            }
            lines.push(Line::raw(""));
            lines.push(Line::raw("Enter / y confirm · Esc / n cancel"));
            s.confirm(tool.title(), lines, Action::Tool(tool));
        }
        Cmd::OpenLearningLog => {
            let path = ctx.paths.learn_log();
            let lines = match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let all: Vec<_> = text
                        .lines()
                        .flat_map(super::learnlog::parse)
                        .map(|tick| {
                            format!("{} {}", view::clock(tick.ts, ctx.tz, true), view::learning_label(&tick.text))
                        })
                        .collect();
                    if all.is_empty() {
                        vec!["No learning runs recorded yet. Use the command menu to learn from this repository.".into()]
                    } else {
                        all.into_iter().rev().take(500).collect()
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    vec!["No learning runs recorded yet. Use the command menu to learn from this repository.".into()]
                }
                Err(error) => vec![format!("Could not read learning history: {error}")],
            };
            s.modal = Some(Modal::Viewer { title: "Learning history · newest first".into(), lines, scroll: 0 });
        }
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
            Action::Tool(tool) => {
                let j = tools::run(ctx.paths.clone(), ctx.root.clone(), tool);
                s.modal = Some(Modal::Progress {
                    title: j.title.clone(),
                    status: j.status.clone(),
                    ratio: 0.0,
                    cancelable: j.cancelable,
                });
                *job = Some(j);
            }
            Action::Review { verb, reference, route, expected_revision } => {
                match crate::review::decide_quietly(verb, &reference, &route, &expected_revision, &ctx.paths) {
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
                        lines.push("Registration saved. Start a new agent session in this repository, then check Health for the first hook receipt.".into());
                        lines.extend(crate::detect::history_summary(&config::load(&ctx.paths).config));
                        lines.push(
                            "Next: History sources → Survey session history → Learn from this repository.".into(),
                        );
                        s.message("Hooks registered · awaiting activity", lines, false)
                    }
                    Err(e) => s.message("Not written", vec![format!("{e:#}")], true),
                }
                loader.want(Want::Health);
            }
            Action::FetchModel => {
                let j = jobs::fetch_model(ctx.paths.clone());
                s.modal = Some(Modal::Progress {
                    title: j.title.clone(),
                    status: j.status.clone(),
                    ratio: 0.0,
                    cancelable: j.cancelable,
                });
                *job = Some(j);
            }
            Action::LearnNow => {
                if ctx.root.is_none() {
                    s.message(
                        "Choose a repository first",
                        vec!["Press : and choose Open a repository, then learn from its traces.".into()],
                        false,
                    );
                    return false;
                }
                let j = jobs::learn_now(ctx.paths.clone(), ctx.root.clone());
                s.modal = Some(Modal::Progress {
                    title: j.title.clone(),
                    status: j.status.clone(),
                    ratio: 0.0,
                    cancelable: j.cancelable,
                });
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
    let mut loader = Loader::spawn(ctx.paths.clone(), ctx.root.clone());
    loader.want(Want::Routes);
    let mut sources = Sources::new(&ctx);
    let mut s = AppState::new(ctx.paths.config());
    s.load_settings(&config::load(&ctx.paths));
    let mut job: Option<Job> = None;
    let mut dashboard: Option<crate::serve::Dashboard> = None;
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
                    s.complete_job(&title, done);
                    loader.want(Want::Health);
                    loader.want(Want::Routes);
                    s.load_settings(&config::load(&ctx.paths));
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
                if let Cmd::OpenRepository(directory) = cmd {
                    match repository_context(&ctx, &directory) {
                        Ok(next) => {
                            let leaving_demo = ctx.demo;
                            if leaving_demo {
                                // The old server owns the temporary demo paths. A later w opens a
                                // server on the user's persistent data, never the demo directory.
                                dashboard = None;
                            }
                            loader = Loader::spawn(next.paths.clone(), next.root.clone());
                            loader.want(Want::Routes);
                            sources = Sources::new(&next);
                            s = AppState::new(next.paths.config());
                            s.load_settings(&config::load(&next.paths));
                            ctx = next;
                            if leaving_demo {
                                s.message("Your workspace is open", vec![
                                    format!("Repository: {}", ctx.label),
                                    format!("Adage data: {}", ctx.paths.root.display()),
                                    "You have left the demo. Choose Connect your agent, then History sources in the command palette.".into(),
                                ], false);
                            }
                        }
                        Err(error) => {
                            let mut form = Form::new(FormKind::OpenRepository);
                            form.values[0] = directory.display().to_string();
                            form.error = Some(format!("{error:#}"));
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
    if ctx.demo && ctx.root.as_ref().is_some_and(|demo_root| root.starts_with(demo_root)) {
        anyhow::bail!("This is the temporary demo repository. Choose your own repository folder to leave the demo.");
    }
    let paths = if ctx.demo { adage_store::Paths::from_env() } else { ctx.paths.clone() };
    let mut next = Ctx::at(paths, Some(root));
    next.theme = ctx.theme;
    Ok(next)
}
