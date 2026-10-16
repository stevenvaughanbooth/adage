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
//! │  Ready: potion-base-8m (bundled).
//! │  › Continue
//! └  ↑↓ choose · enter confirm · q stop
//! ```
//!
//! Steps (onboarding plan O4, O10): harnesses (read-only); binaries (`adage-exec` and
//! `adage-train` beside adage: fetched from this version's release, verified, on a yes; a checkout
//! build is pointed at `cargo build`); hooks, per harness found. For Claude Code the Adage plugin
//! comes first when it is not installed: adage stages it from this binary and installs it with
//! `claude plugin` ([`crate::claude_plugin`]), or, with no `claude` on `PATH`, the step names the
//! two `/plugin` commands to type; the settings file is the second choice; a settings-file
//! registration beside the plugin is offered for removal (its diff first), so every event is wired
//! once. Other harnesses: the exact file and diff first; this repository or every repository. A
//! harness already registered for this repository, in either scope or by the Claude Code plugin, is
//! never registered a second time. Then the embedding model (bundled in adage, so usually nothing
//! to do); which repositories to condition on (those Claude Code has sessions in, the current one
//! ticked: `learn.repos`, [`crate::repos`]); a first `adage condition` of each one ticked;
//! auto-condition; then one screen: the doctor's summary and at most two `next` lines. `y` takes
//! the first choice, `n` skips, `q` (or esc) stops: nothing already done is undone, and nothing
//! else is written.

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
use anyhow::{Context, Result};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Frame;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Harnesses,
    Binaries,
    Hooks(&'static str),
    Model,
    Repos,
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
            Kind::Binaries => "Binaries".into(),
            Kind::Hooks(h) => format!("Hooks · {h}"),
            Kind::Model => "Embedding model".into(),
            Kind::Repos => "Repositories to condition on".into(),
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
    /// Stage the Adage plugin from this binary and install it with `claude plugin`.
    Connect,
    /// Take Adage's hooks out of a settings file the plugin makes redundant.
    Remove(Box<Plan>),
    /// Fetch the missing companions from this version's release.
    FetchBinaries,
    FetchModel,
    ChooseRepos,
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
    /// The repository being set up, if any: which of its files a registration is.
    pub root: Option<std::path::PathBuf>,
    /// The highlighted choice of the current step.
    pub choice: usize,
    /// The repositories step: every repository with Claude Code sessions (the current one
    /// first), and which are ticked.
    pub repos: Vec<crate::repos::Found>,
    pub ticked: Vec<bool>,
    /// The current repository's key.
    pub here: Option<String>,
    /// `learn.repos = "all"`, chosen on purpose: kept unless a box is unticked.
    pub all_repos: bool,
    pub now_ms: u64,
    /// `claude` is on `PATH`, so adage can install its plugin itself.
    pub claude_on_path: bool,
    /// Where the plugin is staged (`$ADAGE_HOME/plugin`), as people read it.
    pub plugin_root: String,
    /// The Binaries step: what is beside adage.
    pub bins: Bins,
    /// For the Claude Code step: taking Adage's hooks out of the settings file the plugin makes
    /// redundant (its diff), when there is one.
    pub removal: Option<Result<Plan, String>>,
    /// Harnesses whose hook files this run wrote (Codex then asks for an approval in `/hooks`).
    pub wrote: Vec<&'static str>,
    /// Why the plugin route did not finish, when it did not: the step then offers the typed
    /// `/plugin` commands and the settings file.
    pub refused: Option<String>,
}

/// The Binaries step's facts: where adage lives, how it came, which companions are missing, and
/// the `cargo build` that makes them in a checkout.
#[derive(Debug, Clone, Default)]
pub struct Bins {
    pub dir: String,
    pub plugin: bool,
    pub checkout_build: Option<String>,
    pub missing: Vec<&'static str>,
}

/// What a choice of the Claude Code step does.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// Stage and install the Adage plugin with `claude plugin`.
    Plugin,
    /// The plugin, typed in Claude Code (no `claude` on `PATH`).
    Typed,
    /// The settings file, in this scope.
    Settings(&'static str),
    /// Keep what is registered.
    Keep,
    /// Take Adage's hooks out of the settings file beside the plugin.
    Remove,
    Skip,
}

impl Wizard {
    pub fn new(detected: Vec<Detected>, registrations: Vec<Registration>, has_repo: bool) -> Wizard {
        let mut steps = vec![
            Step { kind: Kind::Harnesses, status: StepStatus::Todo },
            Step { kind: Kind::Binaries, status: StepStatus::Todo },
        ];
        for d in detected.iter().filter(|d| d.found()) {
            steps.push(Step { kind: Kind::Hooks(d.harness), status: StepStatus::Todo });
        }
        for k in [Kind::Model, Kind::Repos, Kind::Learn, Kind::AutoLearn, Kind::Finish] {
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
            root: None,
            choice: 0,
            repos: vec![],
            ticked: vec![],
            here: None,
            all_repos: false,
            now_ms: 0,
            claude_on_path: false,
            plugin_root: String::new(),
            bins: Bins::default(),
            removal: None,
            wrote: vec![],
            refused: None,
        }
    }

    /// Claude Code's registrations in settings files (one per file).
    fn claude_settings(&self) -> Vec<&Registration> {
        let mut out: Vec<&Registration> = Vec::new();
        for r in self.registrations.iter().filter(|r| r.harness == "claude-code" && r.kind == "settings") {
            if !out.iter().any(|o| o.location == r.location) {
                out.push(r);
            }
        }
        out
    }

    /// The Adage plugin is installed (or has staged its binary) for Claude Code.
    fn plugin_installed(&self) -> bool {
        self.registrations.iter().any(|r| r.harness == "claude-code" && r.kind == "plugin-data")
    }

    /// The Claude Code step's choices, each with what it does: the plugin first when it is not
    /// installed; the settings file second; beside an installed plugin, taking a settings-file
    /// registration out (each hook would run twice).
    pub fn claude_acts(&self) -> Vec<(String, Act)> {
        let settings = self.claude_settings();
        let removable = matches!(self.removal, Some(Ok(_)));
        let mut v: Vec<(String, Act)> = Vec::new();
        if self.plugin_installed() {
            if let (Some(r), true) = (settings.first(), removable) {
                v.push((
                    format!(
                        "Take Adage's hooks out of {} · the plugin runs each one already",
                        detect::tilde(&r.location)
                    ),
                    Act::Remove,
                ));
                v.push(("Keep both · every hook runs twice".into(), Act::Keep));
            } else {
                v.push(("Keep it · the Adage plugin is installed".into(), Act::Keep));
            }
        } else {
            let then = if settings.is_empty() || !removable {
                ""
            } else {
                ", then takes Adage's hooks out of the settings file"
            };
            if self.claude_on_path {
                v.push((
                    format!("The Adage plugin (recommended) · adage installs it with claude plugin{then}"),
                    Act::Plugin,
                ));
            } else {
                v.push((
                    format!(
                        "The Adage plugin (recommended) · in Claude Code: {}, then {}",
                        crate::claude_plugin::SLASH_ADD,
                        crate::claude_plugin::SLASH_INSTALL
                    ),
                    Act::Typed,
                ));
            }
            match settings.first() {
                Some(r) => v.push((
                    format!("Keep the settings file · already registered in {}", detect::tilde(&r.location)),
                    Act::Keep,
                )),
                None => {
                    if self.has_repo {
                        v.push(("The settings file · this repository".into(), Act::Settings("project")));
                    }
                    v.push((
                        "The settings file · every repository you open with claude-code".into(),
                        Act::Settings("user"),
                    ));
                }
            }
        }
        v.push(("Skip claude-code".into(), Act::Skip));
        v
    }

    /// The highlighted choice of the Claude Code step.
    fn claude_act(&self) -> Act {
        self.claude_acts().get(self.choice).map(|(_, a)| a.clone()).unwrap_or(Act::Skip)
    }

    /// The Claude Code step's keys.
    fn claude_key(&mut self, code: KeyCode) -> Option<Cmd> {
        let acts = self.claude_acts();
        let n = acts.len();
        let moved = match code {
            KeyCode::Up | KeyCode::Char('k') if self.choice > 0 => {
                self.choice -= 1;
                true
            }
            KeyCode::Down | KeyCode::Char('j') if self.choice + 1 < n => {
                self.choice += 1;
                true
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(5);
                return None;
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(5).min(self.max_scroll.get());
                return None;
            }
            _ => false,
        };
        if moved {
            self.scroll = 0;
            if let Act::Settings(scope) = self.claude_act() {
                if scope != self.scope {
                    self.scope = scope;
                    return Some(Cmd::Replan);
                }
            }
            return None;
        }
        let act = match code {
            KeyCode::Char('n') | KeyCode::Char('s') => Act::Skip,
            KeyCode::Char('y') => acts[0].1.clone(),
            KeyCode::Enter => self.claude_act(),
            _ => return None,
        };
        match act {
            Act::Plugin => Some(Cmd::Connect),
            Act::Typed => self.advance(StepStatus::Done(format!(
                "in Claude Code: {}, then {}",
                crate::claude_plugin::SLASH_ADD,
                crate::claude_plugin::SLASH_INSTALL
            ))),
            Act::Settings(_) => match &self.plan {
                Some(Ok(p)) => Some(Cmd::Install(Box::new(p.clone()))),
                _ => None,
            },
            Act::Keep => {
                let note = self.registered_note();
                self.advance(StepStatus::Done(note))
            }
            Act::Remove => match &self.removal {
                Some(Ok(p)) => Some(Cmd::Remove(Box::new(p.clone()))),
                _ => None,
            },
            Act::Skip => self.advance(StepStatus::Skipped),
        }
    }

    /// The Binaries step's choices.
    fn bins_fetchable(&self) -> bool {
        !self.bins.plugin && self.bins.checkout_build.is_none() && !self.bins.missing.is_empty()
    }

    /// Ask about these repositories, with `chosen` ticked.
    pub fn set_repos(&mut self, rows: Vec<crate::repos::Found>, chosen: &[String]) {
        self.ticked = rows.iter().map(|f| chosen.contains(&f.key)).collect();
        self.repos = rows;
    }

    /// The ticked repositories.
    pub fn chosen(&self) -> Vec<&crate::repos::Found> {
        self.repos.iter().zip(&self.ticked).filter(|(_, t)| **t).map(|(f, _)| f).collect()
    }

    /// What the repositories step did: `conditions on github.com/acme/app`, `conditions on 3:
    /// a, b, c`.
    pub fn chosen_note(&self) -> String {
        let names: Vec<String> = self.chosen().iter().map(|f| f.label()).collect();
        match names.len() {
            0 => "conditions on none · sessions are recorded, not read".into(),
            1 => format!("conditions on {}", names[0]),
            n if n <= 3 => format!("conditions on {n}: {}", names.join(", ")),
            n => format!("conditions on {n}: {}, …", names[..3].join(", ")),
        }
    }

    /// The repositories step's keys: up and down move, space ticks, `a` ticks all, `n` none,
    /// Enter (or `y`) stores the choice.
    fn repos_key(&mut self, code: KeyCode) -> Option<Cmd> {
        let n = self.repos.len();
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.choice = self.choice.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') if self.choice + 1 < n => self.choice += 1,
            KeyCode::Char(' ') if n > 0 => self.ticked[self.choice] = !self.ticked[self.choice],
            KeyCode::Char('a') => self.ticked.iter_mut().for_each(|t| *t = true),
            KeyCode::Char('n') => self.ticked.iter_mut().for_each(|t| *t = false),
            KeyCode::Enter | KeyCode::Char('y') if n == 0 => {
                return self.advance(StepStatus::Done("no Claude Code sessions yet · nothing to choose".into()))
            }
            KeyCode::Enter | KeyCode::Char('y') => return Some(Cmd::ChooseRepos),
            _ => {}
        }
        None
    }

    pub fn current(&self) -> &Kind {
        &self.steps[self.cur].kind
    }

    /// For a hooks step: where its harness's hooks are registered already (one entry per place),
    /// counting the Claude Code plugin. A harness registered anywhere that applies here is not
    /// registered again: a second registration runs every hook twice.
    pub fn existing(&self) -> Vec<&Registration> {
        let Kind::Hooks(h) = self.current() else { return vec![] };
        let mut out: Vec<&Registration> = Vec::new();
        for r in self.registrations.iter().filter(|r| r.harness == *h) {
            if !out.iter().any(|o| o.location == r.location) {
                out.push(r);
            }
        }
        out
    }

    /// For a hooks step whose harness is registered already: the scope of the registration
    /// `adage install` can rewrite in place (to point it at this adage), if it is one.
    fn update_scope(&self) -> Option<&'static str> {
        let Kind::Hooks(h) = self.current() else { return None };
        self.existing().iter().find_map(|r| crate::install::scope_of(h, &r.location, self.root.as_deref()))
    }

    /// "already registered in ~/.claude/settings.json" (or "by the Adage plugin").
    fn registered_note(&self) -> String {
        let places: Vec<String> = self
            .existing()
            .iter()
            .map(|r| {
                if r.kind == "plugin-data" {
                    "by the Adage plugin".into()
                } else {
                    format!("in {}", detect::tilde(&r.location))
                }
            })
            .collect();
        format!("already registered {} · nothing written", places.join(" and "))
    }

    /// The current step's choices; the last one of a step that can be skipped is the skip.
    pub fn choices(&self) -> Vec<String> {
        if !self.output.is_empty() {
            return vec!["Continue".into()];
        }
        match self.current() {
            Kind::Harnesses => vec!["Continue".into()],
            Kind::Binaries if self.bins_fetchable() => {
                vec!["Fetch them · each verified against the release's checksums".into(), "Skip for now".into()]
            }
            Kind::Binaries => vec!["Continue".into()],
            Kind::Hooks("claude-code") => self.claude_acts().into_iter().map(|(l, _)| l).collect(),
            Kind::Hooks(h) if !self.existing().is_empty() => {
                let mut v = vec!["Keep it · already registered, nothing to do".to_string()];
                if self.update_scope().is_some() {
                    v.push("Rewrite that registration · point it at this adage".to_string());
                }
                v.push(format!("Skip {h}"));
                v
            }
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
            Kind::Model => vec!["Fetch it · SHA-256 verified".into(), "Skip for now".into()],
            Kind::Repos if self.repos.is_empty() => vec!["Continue".into()],
            Kind::Repos => {
                let w = self.repos.iter().map(|f| f.label().chars().count()).max().unwrap_or(0).min(REPO_LABEL);
                self.repos
                    .iter()
                    .zip(&self.ticked)
                    .map(|(f, on)| {
                        let label = clip(&f.label(), w);
                        let here =
                            if self.here.as_deref() == Some(f.key.as_str()) { " · this repository" } else { "" };
                        let usage = crate::repos::usage(f, self.now_ms);
                        format!("{} {label:<w$}  {usage}{here}", if *on { "[x]" } else { "[ ]" })
                    })
                    .collect()
            }
            Kind::Learn if self.chosen().is_empty() => vec!["Continue".into()],
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
            Kind::Hooks("claude-code") => {
                self.scope = match self.claude_act() {
                    Act::Settings(s) => s,
                    _ => {
                        if self.has_repo {
                            "project"
                        } else {
                            "user"
                        }
                    }
                };
                Some(Cmd::Replan)
            }
            Kind::Hooks(_) => {
                // Registered already: the only write offered is rewriting that registration.
                self.scope = self.update_scope().unwrap_or(if self.has_repo { "project" } else { "user" });
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
        if *self.current() == Kind::Repos && self.output.is_empty() {
            return self.repos_key(code);
        }
        if *self.current() == Kind::Hooks("claude-code") && self.output.is_empty() {
            return self.claude_key(code);
        }
        let n = self.choices().len();
        let registered =
            matches!(self.current(), Kind::Hooks(_)) && self.output.is_empty() && !self.existing().is_empty();
        let hooks = matches!(self.current(), Kind::Hooks(_)) && self.output.is_empty() && !registered;
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
            Kind::Binaries if yes && self.bins_fetchable() => Some(Cmd::FetchBinaries),
            Kind::Binaries if skip => self.advance(StepStatus::Skipped),
            Kind::Binaries if yes => {
                let note = self.bins_note();
                self.advance(StepStatus::Done(note))
            }
            // Registered already: keep it (the default), rewrite that same registration, or skip.
            // Never a second registration elsewhere.
            Kind::Hooks(_) if registered && skip => self.advance(StepStatus::Skipped),
            Kind::Hooks(_) if registered && yes && self.choice == 0 => {
                let note = self.registered_note();
                self.advance(StepStatus::Done(note))
            }
            Kind::Hooks(_) if registered && yes => match &self.plan {
                Some(Ok(p)) if p.before.as_deref() != Some(p.after.as_str()) => Some(Cmd::Install(Box::new(p.clone()))),
                Some(Ok(_)) => {
                    let note = self.registered_note();
                    self.advance(StepStatus::Done(note))
                }
                _ => None,
            },
            Kind::Hooks(_) if registered => None,
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
                self.advance(StepStatus::Done(format!("{m} ready")))
            }
            Kind::Model if yes => Some(Cmd::FetchModel),
            Kind::Learn if yes && self.chosen().is_empty() => {
                self.advance(StepStatus::Done("no repository chosen".into()))
            }
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

    /// What the Binaries step found, in a few words.
    fn bins_note(&self) -> String {
        if self.bins.plugin {
            return "the Claude Code plugin's adage: the plugin bundles them".into();
        }
        match (&self.bins.checkout_build, self.bins.missing.as_slice()) {
            (_, []) => format!("adage, adage-exec and adage-train in {}", self.bins.dir),
            (Some(cmd), m) => format!("{} not built · {cmd}", m.join(" and ")),
            (None, m) => format!("{} missing · {}", m.join(" and "), crate::init::CARGO_INSTALL),
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
        Kind::Binaries => {
            let b = &w.bins;
            if b.plugin {
                out.extend(text("This adage is the Claude Code plugin's: the plugin bundles adage-exec and adage-train and links them beside it.", plain));
            } else if b.missing.is_empty() {
                out.extend(text(&format!("adage, adage-exec and adage-train are in {}. Nothing to do.", b.dir), t.green()));
            } else {
                let names = b.missing.join(" and ");
                out.extend(text(
                    &format!(
                        "{names} {} not beside adage in {}. adage-exec runs the steps you allow (adage delegate); adage-train compiles skills (adage train). Everything else works without them.",
                        if b.missing.len() == 1 { "is" } else { "are" },
                        b.dir
                    ),
                    plain,
                ));
                match &b.checkout_build {
                    Some(cmd) => out.extend(text(&format!("This adage is a checkout build: `{cmd}` builds them beside it."), t.dim())),
                    None => out.extend(text(
                        &format!(
                            "Fetching downloads them from release {} for this machine, checks each archive against the .sha256 the release publishes, and places them beside adage. Or build them: `{}`.",
                            env!("CARGO_PKG_VERSION"),
                            crate::init::CARGO_INSTALL
                        ),
                        t.dim(),
                    )),
                }
            }
        }
        Kind::Hooks("claude-code") => {
            if let Some(e) = &w.refused {
                out.extend(text(&format!("The plugin route did not finish: {e}"), t.red()));
                out.push(Line::raw(""));
            }
            let removal_diff = |out: &mut Vec<Line<'static>>| match &w.removal {
                Some(Ok(p)) => out.extend(removal_lines(p, t)),
                Some(Err(e)) => out.extend(text(e, t.red())),
                None => {}
            };
            match w.claude_act() {
                Act::Plugin => {
                    out.extend(text(
                        "The Adage plugin for Claude Code: its hooks, the band above the prompt, the pane and /adage, with adage, adage-exec and adage-train bundled and verified at each session start.",
                        plain,
                    ));
                    out.extend(text(&format!("adage stages it from this binary in {}, then runs:", w.plugin_root), t.dim()));
                    out.push(Line::from(Span::styled(format!("  claude plugin marketplace add {}", w.plugin_root), t.command())));
                    out.push(Line::from(Span::styled(
                        format!("  claude plugin install {}", crate::claude_plugin::PLUGIN_ID),
                        t.command(),
                    )));
                    if matches!(w.removal, Some(Ok(_))) {
                        out.push(Line::raw(""));
                        out.extend(text("Then it takes Adage's hooks out of the settings file, so each runs once (the previous version is kept beside it):", plain));
                        removal_diff(&mut out);
                    }
                }
                Act::Typed => {
                    out.extend(text("`claude` is not on PATH here, so adage cannot install its plugin for you. In Claude Code, type:", plain));
                    out.push(Line::from(Span::styled(format!("  {}", crate::claude_plugin::SLASH_ADD), t.command())));
                    out.push(Line::from(Span::styled(format!("  {}", crate::claude_plugin::SLASH_INSTALL), t.command())));
                    out.extend(text("Or choose the settings file below: the hooks, without the band and pane.", t.dim()));
                }
                Act::Settings(_) => match &w.plan {
                    None => out.push(Line::from(Span::styled("planning…", t.dim()))),
                    Some(Err(e)) => out.extend(text(e, t.red())),
                    Some(Ok(p)) => out.extend(
                        install_confirm(p, t).into_iter().filter(|l| !l.spans.iter().any(|s| s.content.starts_with("y write"))),
                    ),
                },
                Act::Remove => {
                    out.extend(text("The plugin registers every hook; this file registers them too, so each runs twice. Taking Adage's entries out keeps everything else in the file as it is:", plain));
                    removal_diff(&mut out);
                }
                Act::Keep | Act::Skip => {
                    for r in w.existing() {
                        let at = if r.kind == "plugin-data" {
                            "by the Adage plugin".to_string()
                        } else {
                            format!("in {}", detect::tilde(&r.location))
                        };
                        out.extend(text(&format!("Already registered {at}."), t.green()));
                    }
                    if w.existing().is_empty() {
                        out.extend(text("Claude Code sessions will not hear from Adage until it is connected (`adage init` again, any time).", plain));
                    }
                }
            }
        }
        Kind::Hooks(h) if !w.existing().is_empty() => {
            for r in w.existing() {
                let at = if r.kind == "plugin-data" {
                    "by the Adage plugin".to_string()
                } else {
                    format!("in {}", detect::tilde(&r.location))
                };
                out.extend(text(&format!("Already registered {at}."), t.green()));
            }
            let rewrite = w.update_scope().is_some() && w.choice == 1;
            match &w.plan {
                Some(Ok(p)) if rewrite => out.extend(
                    install_confirm(p, t)
                        .into_iter()
                        .filter(|l| !l.spans.iter().any(|s| s.content.starts_with("y write"))),
                ),
                Some(Err(e)) if rewrite => out.extend(text(e, t.red())),
                _ => {
                    out.extend(text(
                        "Nothing to write: a second registration would run every hook twice, so each directive and notice would arrive twice.",
                        plain,
                    ));
                    out.extend(text(
                        &format!("`adage install {h} --remove` takes a registration out; `adage doctor` checks them."),
                        t.dim(),
                    ));
                }
            }
        }
        Kind::Hooks(_) => {
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
            Some(m) => out.extend(text(
                &format!("Ready: {m}. Prompts that mean the same thing (\"run the tests\", \"check my change\") share a reflex."),
                plain,
            )),
            None => out.extend(text(
                "No usable embedding model (none installed, and none bundled in this adage). With one, prompts that mean the same thing (\"run the tests\", \"check my change\") share a reflex. Everything else works without it.",
                plain,
            )),
        },
        Kind::Repos if w.repos.is_empty() => out.extend(text(
            "No Claude Code sessions found yet. Adage conditions on each repository you work in until you choose (`adage init` again, once there are some).",
            plain,
        )),
        Kind::Repos => {
            out.extend(text(
                "Adage reads only the repositories you tick, and only their Claude Code sessions (Codex's too once its hooks are registered). Sessions elsewhere are recorded, not read.",
                plain,
            ));
            if w.all_repos {
                out.extend(text("Now: every repository (learn.repos = all); untick one to choose instead.", t.dim()));
            }
        }
        Kind::Learn if w.chosen().is_empty() => {
            out.extend(text("No repository chosen, so there is nothing to condition on yet.", plain))
        }
        Kind::Learn => {
            let what = match w.chosen().as_slice() {
                [one] => one.label(),
                many => format!("the {} repositories you chose", many.len()),
            };
            out.extend(text(
                &format!("Condition {what} from existing Claude Code history (Codex's when its hooks are registered, and configured OpenCode exports). Bind instincts from the files, learn reflexes, and compile the index hooks read. It only reads transcripts; it never runs anything they contain."),
                plain,
            ))
        }
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
                        found.push(Line::from(vec![Span::styled("  fix ", t.dim()), Span::styled(fx.prose(), t.dim())]));
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

/// Taking Adage's entries out of a file: the file, then the diff (`adage install --remove`'s).
fn removal_lines(p: &Plan, t: &Theme) -> Vec<Line<'static>> {
    use super::diff::{self, Op};
    let mut lines = vec![Line::from(vec![
        Span::raw(if p.after.is_empty() { "Remove " } else { "Change " }),
        Span::styled(p.target.display().to_string(), t.bold()),
    ])];
    for (op, l) in diff::lines(p.before.as_deref().unwrap_or(""), &p.after) {
        lines.push(match op {
            Op::Keep => Line::from(Span::styled(format!("  {l}"), t.dim())),
            Op::Add => Line::from(Span::styled(format!("+ {l}"), t.green())),
            Op::Remove => Line::from(Span::styled(format!("- {l}"), t.red())),
        });
    }
    lines
}

/// The most choices shown at once; the list scrolls with the highlight.
const MAX_CHOICES: usize = 8;
/// The widest repository name shown before it is cut.
const REPO_LABEL: usize = 40;

/// `s` cut to `width` characters, ending in `…` when cut.
fn clip(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn hint(w: &Wizard) -> &'static str {
    if w.progress.is_some() {
        return "working… q stops";
    }
    if *w.current() == Kind::Repos && w.output.is_empty() && !w.repos.is_empty() {
        return "↑↓ move · space tick · a all · n none · enter continue · q stop";
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
    // The body scrolls (PgUp/PgDn) inside what is left once the choices and hint are placed. A
    // long list of choices shows a window around the highlighted one.
    let choices = w.choices();
    let listed = choices.len() > 1 || (*w.current() == Kind::Repos && !w.repos.is_empty());
    let window = choices.len().min(MAX_CHOICES);
    let first = w.choice.saturating_sub(window / 2).min(choices.len() - window);
    let more = choices.len() > window;
    let fixed = 1 + 1 + if listed || w.progress.is_some() { window + 1 + usize::from(more) } else { 0 } + 2;
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
    } else if listed {
        lines.push(Line::from(rail(t, true)));
        for (i, c) in choices.iter().enumerate().skip(first).take(window) {
            let sel = i == w.choice;
            lines.push(Line::from(vec![
                rail(t, true),
                Span::styled(if sel { format!("{} ", t.glyph(Glyph::Pointer)) } else { "  ".into() }, t.accent()),
                Span::styled(clip(c, width.saturating_sub(2)), if sel { t.bold() } else { t.dim() }),
            ]));
        }
        if more {
            let below = choices.len() - first - window;
            lines.push(Line::from(vec![
                rail(t, true),
                Span::styled(format!("  {first} above · {below} below"), t.dim()),
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
            " Start a normal agent session. Available instincts and learned guidance apply through the hooks."
                .to_string(),
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
                out.push(Line::from(vec![Span::styled("       fix ", t.dim()), Span::styled(fx.prose(), t.dim())]));
            }
        }
    }
    out.push(Line::raw(""));
    // At most two: what needs doing first, then where to go (onboarding plan O10).
    let mut next: Vec<(String, String)> = Vec::new();
    if failing > 0 {
        next.push(("adage doctor".into(), "what needs fixing, and how".into()));
    } else if let Some(h) = unhooked.first() {
        next.push(("adage init".into(), format!("again, to connect {h} (every step already done is kept)")));
    }
    if w.registrations.iter().any(|r| r.harness == "claude-code") {
        next.push(("claude".into(), "start a session; Adage says what it is doing on its first line".into()));
    }
    if w.wrote.contains(&"codex") {
        next.push(("codex".into(), "open /hooks in Codex to approve the new hooks".into()));
    }
    if next.is_empty() {
        next.push(("adage ui".into(), "review, health and settings in one place".into()));
    }
    next.truncate(2);
    out.extend(next_lines(&next, t, cols));
    out.push(Line::from(vec![
        Span::raw("         "),
        Span::styled(
            "Later: `adage update` updates Adage, its hooks and the plugin together; `adage doctor` if anything looks wrong.",
            t.dim(),
        ),
    ]));
    out
}

/// The `next` rows: a command column, then each description wrapped to `cols` under its own
/// first line. The column is as wide as the longest command when that leaves the descriptions
/// room; otherwise a longer command takes a line of its own, its description on the next line
/// in the same column as the others. Narrower still, descriptions go under their commands.
fn next_lines(rows: &[(String, String)], t: &Theme, cols: usize) -> Vec<Line<'static>> {
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

/// What hooks embed with, when it is a model: an installed one, or the one bundled in adage
/// (`potion-base-8m (bundled)`). `None` for the hashed fallback.
fn usable_model(ctx: &Ctx) -> Option<String> {
    let (embedder, _) = crate::status_card::embeddings(&ctx.paths);
    (embedder != "hashed fallback").then_some(embedder)
}

/// The Binaries step's facts for the running adage.
fn bins_of(exe: &std::path::Path) -> Bins {
    let dir = exe.parent().map(detect::tilde).unwrap_or_default();
    match crate::init::origin(exe) {
        crate::init::Origin::Plugin => Bins { dir, plugin: true, checkout_build: None, missing: vec![] },
        crate::init::Origin::Checkout { root, profile } => {
            let missing = crate::init::missing_companions(exe);
            let build = (!missing.is_empty()).then(|| crate::init::cargo_build(&root, &profile, &missing));
            Bins { dir, plugin: false, checkout_build: build, missing }
        }
        crate::init::Origin::Installed => {
            Bins { dir, plugin: false, checkout_build: None, missing: crate::init::missing_companions(exe) }
        }
    }
}

pub fn run(ctx: Ctx) -> Result<()> {
    let mut w = Wizard::new(detect::harnesses(), detect::registrations(ctx.root.as_deref()), ctx.root.is_some());
    w.root = ctx.root.clone();
    w.model = usable_model(&ctx);
    let exe = std::env::current_exe().map(|e| e.canonicalize().unwrap_or(e))?;
    w.bins = bins_of(&exe);
    w.claude_on_path = crate::claude_plugin::System::on_path().is_some();
    w.plugin_root = detect::tilde(&ctx.paths.plugin());
    let loaded = config::load(&ctx.paths);
    w.auto_learn = loaded.get("learn.auto").map(|(_, src)| (loaded.config.learn_auto, src.to_string()));
    w.config_path = detect::tilde(&ctx.paths.config());
    // Repositories with Claude Code sessions: project directories listed, one transcript head
    // read per directory; nothing parsed.
    w.here = ctx.root.as_deref().map(|r| adage_core::repo::repo_key_cached(r, &ctx.paths.repo_keys()));
    w.all_repos = loaded.config.learn_repos.is_none()
        && loaded.get("learn.repos").is_some_and(|(_, src)| *src != config::Source::Default);
    w.now_ms = crate::learnlog::now_ms();
    let found = crate::repos::found(&ctx.paths);
    let chosen = crate::repos::preselected(&ctx.paths, &found, w.here.as_deref());
    let rows = crate::repos::rows(found, &chosen, w.here.as_deref().zip(ctx.root.as_deref()));
    w.set_repos(rows, &chosen);
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
            if h == "claude-code" {
                // The settings-file registration the plugin would make redundant, if `adage
                // install` wrote it (another file is the person's to edit).
                let root = ctx.root.as_deref();
                w.removal =
                    w.claude_settings().iter().find_map(|r| crate::install::scope_of(h, &r.location, root)).map(|s| {
                        match crate::install::plan_removal(h, s, root) {
                            Ok(Some(p)) => Ok(p),
                            Ok(None) => Err("nothing to take out".to_string()),
                            Err(e) => Err(format!("{e:#}")),
                        }
                    });
            }
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
                    let done = done.map(jobs::Outcome::into_lines);
                    let kind = kind.clone();
                    job = None;
                    w.progress = None;
                    match (kind, done) {
                        (Kind::Binaries, Ok(lines)) => {
                            w.bins = bins_of(&exe);
                            let note = if w.bins.missing.is_empty() {
                                format!("fetched, verified · adage, adage-exec and adage-train in {}", w.bins.dir)
                            } else {
                                lines.last().cloned().unwrap_or_default()
                            };
                            w.steps[w.cur].status = if w.bins.missing.is_empty() {
                                StepStatus::Done(note)
                            } else {
                                StepStatus::Failed(note)
                            };
                            w.output = lines;
                        }
                        (Kind::Hooks("claude-code"), Ok(lines)) => {
                            // Installed: the settings file the plugin makes redundant comes out.
                            let mut note = lines.first().cloned().unwrap_or_default();
                            if let Some(Ok(p)) = w.removal.take() {
                                note = match crate::install::remove_backed_up(&p) {
                                    Ok(_) => format!("{note}; took Adage's hooks out of {}", detect::tilde(&p.target)),
                                    Err(e) => format!(
                                        "{note}; {} still registers Adage's hooks ({e:#})",
                                        detect::tilde(&p.target)
                                    ),
                                };
                            }
                            w.registrations = detect::registrations(ctx.root.as_deref());
                            w.advance(StepStatus::Done(note));
                        }
                        (Kind::Hooks("claude-code"), Err(e)) => {
                            // Offer what is left: the typed commands, the settings file.
                            w.refused = Some(e);
                            w.claude_on_path = false;
                            w.choice = 0;
                        }
                        (Kind::Model, Ok(lines)) => {
                            w.model = usable_model(&ctx);
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
            Cmd::Connect => {
                let (paths, cwd) =
                    (ctx.paths.clone(), ctx.root.clone().or_else(|| std::env::current_dir().ok()).unwrap_or_default());
                let exe = exe.clone();
                job = Some((
                    Job::spawn("Installing the Adage plugin", move |tx| {
                        let _ = tx.send(jobs::JobMsg::Progress(0.2, "staging the plugin from this adage…".into()));
                        let cli = crate::claude_plugin::System::on_path().context("`claude` is not on PATH")?;
                        let done = crate::claude_plugin::connect(&cli, &paths, &exe, "user", &cwd)?;
                        let from = done
                            .installed
                            .read_from
                            .as_deref()
                            .map(detect::tilde)
                            .unwrap_or_else(|| detect::tilde(&done.staged.plugin));
                        Ok(vec![format!(
                            "the Adage plugin ({}), installed and enabled · read from {from}",
                            crate::claude_plugin::PLUGIN_ID
                        )])
                    }),
                    Kind::Hooks("claude-code"),
                ));
                w.progress = Some(("staging the plugin from this adage…".into(), 0.1));
                None
            }
            Cmd::Remove(p) => match crate::install::remove_backed_up(&p) {
                Ok(backup) => {
                    w.registrations = detect::registrations(ctx.root.as_deref());
                    w.removal = None;
                    w.advance(StepStatus::Done(format!(
                        "took Adage's hooks out of {} (previous version in {}) · the plugin runs each one",
                        detect::tilde(&p.target),
                        detect::tilde(&backup)
                    )))
                }
                Err(e) => {
                    w.removal = Some(Err(format!("not removed: {e:#}")));
                    None
                }
            },
            Cmd::FetchBinaries => {
                let (paths, missing) = (ctx.paths.clone(), w.bins.missing.clone());
                let dir = exe.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
                job = Some((
                    Job::spawn("Fetching adage-exec and adage-train", move |tx| {
                        let _ = tx.send(jobs::JobMsg::Progress(0.2, format!("fetching {}…", missing.join(" and "))));
                        let (fetched, problems) = crate::init::fetch_companions(&paths, &dir, &missing);
                        let mut lines: Vec<String> =
                            fetched.iter().map(|f| format!("{f}: fetched, verified")).collect();
                        lines.extend(problems);
                        Ok(lines)
                    }),
                    Kind::Binaries,
                ));
                w.progress = Some(("fetching…".into(), 0.1));
                None
            }
            Cmd::Install(p) => match crate::install::apply_backed_up(&p) {
                Ok(backup) => {
                    w.registrations = detect::registrations(ctx.root.as_deref());
                    if let Some(h) = crate::detect::HARNESSES.into_iter().find(|h| *h == p.harness) {
                        w.wrote.push(h);
                    }
                    let kept =
                        backup.map(|b| format!(" (previous version in {})", detect::tilde(&b))).unwrap_or_default();
                    w.advance(StepStatus::Done(format!("wrote {}{kept}", detect::tilde(&p.target))))
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
            // Every repository still ticked: kept as `all` when that was the choice already.
            Cmd::ChooseRepos if w.all_repos && w.ticked.iter().all(|t| *t) => {
                w.advance(StepStatus::Done("every repository (learn.repos = all)".into()))
            }
            Cmd::ChooseRepos => {
                let keys: Vec<String> = w.chosen().iter().map(|f| f.key.clone()).collect();
                match crate::repos::choose(&ctx.paths, &keys) {
                    Ok(()) => w.advance(StepStatus::Done(w.chosen_note())),
                    Err(e) => w.advance(StepStatus::Failed(format!("{e:#}"))),
                }
            }
            Cmd::Learn => {
                // The ticked repositories still on this machine, the current one first.
                let roots: Vec<(String, std::path::PathBuf)> =
                    w.chosen().iter().filter(|f| f.root.is_dir()).map(|f| (f.label(), f.root.clone())).collect();
                job = Some((jobs::learn_now(ctx.paths.clone(), roots), Kind::Learn));
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
