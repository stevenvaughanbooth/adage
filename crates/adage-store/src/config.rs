//! `$ADAGE_HOME/config.toml`: the person's settings, read everywhere through [`load`].
//!
//! ```toml
//! [learn]
//! auto = true                # learn automatically when agent sessions stop or end
//! min_interval_secs = 120    # at most one automatic learn per repository per interval
//! thresholds = "default"     # or "demo" (lower evidence bars, for demos and small histories)
//! claude_projects = ""       # Claude Code history ("" = ~/.claude/projects)
//! codex_sessions = "auto"    # Codex rollouts: "auto" ($CODEX_HOME/sessions once Codex hooks are registered), a directory, or "" (none)
//! opencode_exports = ""      # a directory of `opencode export` files ("" = none)
//! repos = "all"              # repositories to learn from: all, or keys, e.g. ["github.com/acme/billing"]
//! parse_cache = true         # keep parsed transcripts under cache/sessions (faster re-learning)
//!
//! [ui]
//! notices = "daily"          # one-line notices at session start (Claude Code), at most daily; or "off"
//!
//! [runtime]
//! control = 0.1              # share of sessions held out as the control arm
//!
//! [skills]
//! invoke = false             # Claude Code mod: invoke an accepted, inert skill at its step
//!
//! [instincts]
//! enabled = true             # bind Adage's built-in defaults from each repository's files
//! packs = ["auto"]           # instinct packs to bind: auto (every one installed) or their names
//! disabled = []              # packs or instinct ids never to bind, e.g. ["adage/lockfiles", "U3"]
//! ```
//!
//! Every key has a default, so a missing file is the default configuration. Environment
//! variables win over the file: `ADAGE_CONTROL` for `runtime.control` and `ADAGE_AUTO_LEARN`
//! (`0`/`1`) for `learn.auto`. A file that cannot be parsed, an unknown key, or a value that fails
//! validation is reported (`adage config`, `adage doctor`) and that key keeps its default: hooks
//! never fail because of the configuration.
//!
//! `adage config set` validates the value and rewrites the file atomically (a temporary file
//! renamed into place), keeping every other key; comments are not kept.

use crate::Paths;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FILE: &str = "config.toml";

/// Which evidence thresholds learning uses (`adage_learn::LearnConfig::default` or `::demo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thresholds {
    Default,
    Demo,
}

impl Thresholds {
    pub fn as_str(self) -> &'static str {
        match self {
            Thresholds::Default => "default",
            Thresholds::Demo => "demo",
        }
    }
}

/// The effective configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub learn_auto: bool,
    /// `learn.repos`: the repositories whose history Adage reads, by key; `None` is `all`.
    /// Hooks still record sessions everywhere they are registered.
    pub learn_repos: Option<Vec<String>>,
    pub learn_min_interval_secs: u64,
    pub learn_thresholds: Thresholds,
    /// `None` = the default location, `~/.claude/projects`.
    pub learn_claude_projects: Option<PathBuf>,
    pub learn_codex_sessions: Option<PathBuf>,
    /// `learn.codex_sessions = "auto"` (the default): automatic learning reads
    /// `$CODEX_HOME/sessions` (else `~/.codex/sessions`) once Codex hooks are registered.
    pub learn_codex_auto: bool,
    pub learn_opencode_exports: Option<PathBuf>,
    /// Keep parsed transcripts in `$ADAGE_HOME/cache/sessions` (`adage_learn::cache`).
    pub learn_parse_cache: bool,
    /// `ui.notices`: one-line notices at session start (`daily`, the default) or none (`off`).
    pub ui_notices: bool,
    /// The control share; as with `ADAGE_CONTROL`, 1 or more is shadow mode.
    pub runtime_control: f64,
    /// `runtime.rewrite`: `PreToolUse` rewrites to the project's environment (on by default).
    pub runtime_rewrite: bool,
    /// `runtime.routine_model`: the model the Claude Code mod sends a routine step to (a step the
    /// model takes only to do what a delivered directive says), at low effort. `""` routes
    /// nothing. The binary never sees it: the mod reads it from `adage --json`.
    pub runtime_routine_model: String,
    /// `runtime.predict_cap`: at most this many steps a session's model does not take, predicted
    /// by Adage under a person's `predict` grant (architecture §3.2). 6 by default; 0 predicts none.
    pub runtime_predict_cap: u32,
    /// `skills.invoke`: a `skill` reflex with a person's `predict` grant may invoke its skill at
    /// its step (the Claude Code mod's `turn.step`), when `adage train` recorded the skill as inert
    /// in a pack the person accepted. Off by default.
    pub skills_invoke: bool,
    /// `ui.spinner`, `ui.turn_line`, `ui.noticed`: what the Claude Code mod draws beside Claude
    /// Code's own lines (a spinner suffix while a reflex was delivered this turn, a turn summary,
    /// the noticed line under a batch). Off by default; read by the mod from `adage --json`.
    pub ui_spinner: bool,
    pub ui_turn_line: bool,
    pub ui_noticed: bool,
    /// Bind instincts (prebuilt routes) from each repository's declared files when learning.
    /// Learn procedure cards for families of similar tasks (`learn.cards`), and how similar a
    /// prompt must be to a family for its card (`learn.card_threshold`).
    pub learn_cards: bool,
    pub learn_card_threshold: f64,
    pub instincts_enabled: bool,
    /// Instinct packs to bind.
    pub instincts_packs: Vec<String>,
    /// Instinct ids never to bind.
    pub instincts_disabled: Vec<String>,
}

impl Config {
    /// Does Adage read history for the repository with this key (`learn.repos`)?
    pub fn learns(&self, repo: &str) -> bool {
        self.learn_repos.as_ref().is_none_or(|keys| keys.iter().any(|k| k == repo))
    }
}

/// Where a value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Default,
    File,
    Env(&'static str),
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Default => write!(f, "default"),
            Source::File => write!(f, "file"),
            Source::Env(v) => write!(f, "env {v}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    /// Seconds, 0 to one day.
    Secs,
    Thresholds,
    /// A directory: absolute, or starting with `~/`; `""` for none.
    Dir,
    /// A share between 0 and 1.
    Share,
    /// A directory, `""` for none, or `auto`.
    DirOrAuto,
    /// `daily` or `off`.
    Notices,
    /// A list of short ids (`[A-Za-z0-9_-]`, at most 32 characters each, at most 64 of them),
    /// given on the command line comma-separated; `""` for none.
    Ids,
    /// `all`, or a list of repository keys (`github.com/acme/billing`, `notes@1a2b3c4d5e6f`),
    /// given on the command line comma-separated; `""` for none.
    Repos,
    /// A model name or alias as the harness takes it (`haiku`, `claude-haiku-4-5`, a provider's
    /// `us.anthropic.…:0`): one word of `[A-Za-z0-9._:/@-]`, at most 128 characters; `""` for none.
    Model,
    /// A whole number from 0 to [`MAX_PREDICT_CAP`].
    Cap,
}

/// The highest `runtime.predict_cap` a person can set.
pub const MAX_PREDICT_CAP: u32 = 50;

/// One documented key.
#[derive(Debug)]
pub struct Key {
    pub name: &'static str,
    kind: Kind,
    pub default: &'static str,
    pub env: Option<&'static str>,
    pub doc: &'static str,
    /// A setting that changes what Adage learns from or how much evidence it needs (its sources,
    /// thresholds, instinct packs): `adage config set` asks for a person at a terminal. Every
    /// other key only tunes or turns something down, and can be set from a script or a sandbox.
    pub guarded: bool,
}

/// Every key, in the order `adage config` prints them.
pub const KEYS: &[Key] = &[
    Key {
        name: "learn.auto",
        kind: Kind::Bool,
        default: "true",
        env: Some("ADAGE_AUTO_LEARN"),
        doc: "condition automatically, in the background, when an agent session stops or ends",
        guarded: false,
    },
    Key {
        name: "learn.min_interval_secs",
        kind: Kind::Secs,
        default: "120",
        env: None,
        doc: "at most one automatic conditioning run per repository in this many seconds (0 to 86400)",
        guarded: false,
    },
    Key {
        name: "learn.thresholds",
        kind: Kind::Thresholds,
        default: "default",
        env: None,
        doc: "evidence bars for `adage condition` and automatic conditioning: default, or demo (lower)",
        guarded: true,
    },
    Key {
        name: "learn.claude_projects",
        kind: Kind::Dir,
        default: "",
        env: None,
        doc: "Claude Code traces to condition on (\"\" = ~/.claude/projects)",
        guarded: true,
    },
    Key {
        name: "learn.codex_sessions",
        kind: Kind::DirOrAuto,
        default: "auto",
        env: None,
        doc: "Codex rollouts to condition on: auto (once Codex hooks are registered), a directory, or \"\" (none)",
        guarded: true,
    },
    Key {
        name: "learn.opencode_exports",
        kind: Kind::Dir,
        default: "",
        env: None,
        doc: "a directory of `opencode export` files automatic conditioning reads (\"\" = none)",
        guarded: true,
    },
    Key {
        name: "learn.repos",
        kind: Kind::Repos,
        default: "all",
        env: None,
        doc: "repositories to condition on, comma-separated keys as `adage init` lists them, or all",
        guarded: true,
    },
    Key {
        name: "learn.parse_cache",
        kind: Kind::Bool,
        default: "true",
        env: None,
        doc: "keep parsed traces in cache/sessions so conditioning again parses only what changed",
        guarded: false,
    },
    Key {
        name: "learn.cards",
        kind: Kind::Bool,
        default: "true",
        env: None,
        doc: "learn procedure cards for families of similar tasks (with the installed or bundled model)",
        guarded: false,
    },
    Key {
        name: "learn.card_threshold",
        kind: Kind::Share,
        default: "0.811",
        env: None,
        doc: "how similar a prompt must be to a family's prompts for its procedure card (0.5 to 0.99)",
        guarded: true,
    },
    Key {
        name: "instincts.enabled",
        kind: Kind::Bool,
        default: "true",
        env: None,
        doc: "bind Adage's built-in defaults (instincts) from each repository's declared files",
        guarded: false,
    },
    Key {
        name: "instincts.packs",
        kind: Kind::Ids,
        default: "auto",
        env: None,
        doc: "instinct packs to bind, comma-separated: auto (every installed instinct pack) or names, e.g. adage/python-env",
        guarded: true,
    },
    Key {
        name: "instincts.disabled",
        kind: Kind::Ids,
        default: "",
        env: None,
        doc: "instinct packs or ids never to bind, comma-separated, e.g. adage/lockfiles,U3 (see docs/instincts.md)",
        guarded: false,
    },
    Key {
        name: "ui.notices",
        kind: Kind::Notices,
        default: "daily",
        env: None,
        doc: "one-line notices at session start (Claude Code): daily, only when something is new; or off",
        guarded: false,
    },
    Key {
        name: "ui.spinner",
        kind: Kind::Bool,
        default: "false",
        env: None,
        doc: "Claude Code mod: add an adage suffix to the spinner while a reflex was delivered this turn",
        guarded: false,
    },
    Key {
        name: "ui.turn_line",
        kind: Kind::Bool,
        default: "false",
        env: None,
        doc: "Claude Code mod: a one-line adage summary beside the turn's duration, for turns with Adage activity",
        guarded: false,
    },
    Key {
        name: "ui.noticed",
        kind: Kind::Bool,
        default: "false",
        env: None,
        doc: "Claude Code mod: a line under a tool batch when Adage notices a habit it is still counting",
        guarded: false,
    },
    Key {
        name: "runtime.control",
        kind: Kind::Share,
        default: "0.1",
        env: Some("ADAGE_CONTROL"),
        doc: "share of sessions held out as the control arm (Adage decides but says nothing)",
        guarded: false,
    },
    Key {
        name: "runtime.rewrite",
        kind: Kind::Bool,
        default: "true",
        env: None,
        doc: "point Python test runs under the system Python at the project's verified environment before they run (Claude Code PreToolUse); false: leave every command as the agent wrote it",
        guarded: false,
    },
    Key {
        name: "runtime.routine_model",
        kind: Kind::Model,
        default: "",
        env: None,
        doc: "Claude Code mod: the model a step goes to, at low effort, when it only does what a delivered directive says; \"\" sends every step to the session's model",
        guarded: false,
    },
    Key {
        name: "runtime.predict_cap",
        kind: Kind::Cap,
        default: "6",
        env: None,
        doc: "Claude Code mod: at most this many steps per session that Adage takes in the model's place under a predict grant (0 to 50; 0: none)",
        guarded: true,
    },
    Key {
        name: "skills.invoke",
        kind: Kind::Bool,
        default: "false",
        env: None,
        doc: "Claude Code mod: invoke a skill at the step a skill reflex names, when it holds a predict grant and adage train recorded the skill inert in a pack you accepted",
        guarded: true,
    },
];

pub fn key(name: &str) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.name == name)
}

/// The configuration with, per key, its value as text and where it came from, and anything wrong
/// with the file.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub config: Config,
    pub values: BTreeMap<&'static str, (String, Source)>,
    pub problems: Vec<String>,
}

impl Loaded {
    pub fn get(&self, name: &str) -> Option<&(String, Source)> {
        self.values.get(name)
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "on" | "yes" => Some(true),
        "false" | "0" | "off" | "no" => Some(false),
        _ => None,
    }
}

/// A value given as text (on the command line, or in an environment variable) → the TOML value to
/// store, validated.
fn from_text(kind: Kind, s: &str) -> Result<toml::Value, String> {
    let s = s.trim();
    match kind {
        Kind::Bool => parse_bool(s).map(toml::Value::Boolean).ok_or_else(|| format!("{s:?} is not true or false")),
        Kind::Secs => match s.parse::<i64>() {
            Ok(n) if (0..=86_400).contains(&n) => Ok(toml::Value::Integer(n)),
            _ => Err(format!("{s:?} is not a whole number of seconds between 0 and 86400")),
        },
        Kind::Thresholds => match s {
            "default" | "demo" => Ok(toml::Value::String(s.into())),
            _ => Err(format!("{s:?} is not default or demo")),
        },
        Kind::DirOrAuto if s == "auto" => Ok(toml::Value::String(s.into())),
        Kind::Notices => match s {
            "daily" | "off" => Ok(toml::Value::String(s.into())),
            _ => Err(format!("{s:?} is not daily or off")),
        },
        Kind::Dir | Kind::DirOrAuto => {
            if s.chars().any(char::is_control) || s.len() > 4096 {
                return Err("not a usable path".into());
            }
            if !s.is_empty() && !s.starts_with("~/") && !Path::new(s).is_absolute() {
                return Err(format!("{s:?} is not an absolute path (or one starting with ~/)"));
            }
            Ok(toml::Value::String(s.into()))
        }
        Kind::Cap => match s.parse::<u32>() {
            Ok(n) if n <= MAX_PREDICT_CAP => Ok(toml::Value::Integer(i64::from(n))),
            _ => Err(format!("{s:?} is not a whole number between 0 and {MAX_PREDICT_CAP}")),
        },
        Kind::Share => match s.parse::<f64>() {
            Ok(x) if (0.0..=1.0).contains(&x) => Ok(toml::Value::Float(x)),
            _ => Err(format!("{s:?} is not a share between 0 and 1")),
        },
        Kind::Model => {
            let ok = s.len() <= 128 && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"._:/@-".contains(&c));
            if ok {
                Ok(toml::Value::String(s.into()))
            } else {
                Err(format!("{s:?} is not a model name (letters, digits, . _ : / @ -)"))
            }
        }
        Kind::Ids => {
            let ids = split_ids(s);
            // An id (`U3`, `auto`) or a pack name (`adage/python-env`).
            let ok = |i: &String| {
                i.len() <= 64
                    && i.split('/').count() <= 2
                    && i.split('/').all(|p| {
                        p.as_bytes().first().is_some_and(|c| c.is_ascii_alphanumeric())
                            && p.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                    })
            };
            if ids.len() > 64 || !ids.iter().all(ok) {
                return Err(format!("{s:?} is not a comma-separated list of ids"));
            }
            Ok(toml::Value::Array(ids.into_iter().map(toml::Value::String).collect()))
        }
        Kind::Repos if s == "all" => Ok(toml::Value::String(s.into())),
        Kind::Repos => {
            let keys = split_ids(s);
            let ok = |k: &String| {
                k != "all" && k.len() <= 512 && !k.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"')
            };
            if keys.len() > 256 || !keys.iter().all(ok) {
                return Err(format!("{s:?} is not all, or a comma-separated list of repository keys"));
            }
            Ok(toml::Value::Array(keys.into_iter().map(toml::Value::String).collect()))
        }
    }
}

/// `"U3, N1"` → `["U3", "N1"]`; `""` → `[]`.
fn split_ids(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

/// A value read from the file → its canonical text, validated.
fn from_toml(kind: Kind, v: &toml::Value) -> Result<String, String> {
    let text = match (kind, v) {
        (Kind::Bool, toml::Value::Boolean(b)) => b.to_string(),
        (Kind::Secs | Kind::Cap, toml::Value::Integer(n)) => n.to_string(),
        (Kind::Share, toml::Value::Float(x)) => x.to_string(),
        (Kind::Share, toml::Value::Integer(n)) => n.to_string(),
        (Kind::Thresholds | Kind::Dir | Kind::DirOrAuto | Kind::Notices | Kind::Model, toml::Value::String(s)) => {
            s.clone()
        }
        (Kind::Ids | Kind::Repos, toml::Value::String(s)) => s.clone(),
        (Kind::Ids | Kind::Repos, toml::Value::Array(a)) => {
            let ids: Option<Vec<&str>> = a.iter().map(|v| v.as_str()).collect();
            ids.ok_or_else(|| format!("{v} is not a list of ids"))?.join(",")
        }
        _ => return Err(format!("{v} has the wrong type")),
    };
    from_text(kind, &text).map(|_| text)
}

fn home_relative(s: &str) -> Option<PathBuf> {
    if s.is_empty() {
        return None;
    }
    match s.strip_prefix("~/") {
        Some(rest) => crate::home_dir().map(|h| h.join(rest)),
        None => Some(PathBuf::from(s)),
    }
}

/// The configuration: defaults, then the file, then the environment.
pub fn load(paths: &Paths) -> Loaded {
    load_from(&paths.config(), &|v| std::env::var(v).ok())
}

fn load_from(file: &Path, env: &dyn Fn(&str) -> Option<String>) -> Loaded {
    let mut problems = Vec::new();
    let table: toml::Table = match std::fs::read_to_string(file) {
        Ok(text) => match text.parse::<toml::Table>() {
            Ok(t) => t,
            Err(e) => {
                let first = e.to_string().lines().next().unwrap_or("").to_string();
                problems.push(format!("{FILE} cannot be parsed, so every key has its default: {first}"));
                toml::Table::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => {
            problems.push(format!("{FILE} cannot be read ({:?}), so every key has its default", e.kind()));
            toml::Table::new()
        }
    };
    // Unknown sections and keys: probably a typo.
    for (section, v) in &table {
        match v.as_table() {
            Some(t) => {
                for k in t.keys() {
                    if key(&format!("{section}.{k}")).is_none() {
                        problems.push(format!("unknown key {section}.{k} (ignored)"));
                    }
                }
            }
            None => problems.push(format!("unknown key {section} (ignored)")),
        }
    }
    let mut values = BTreeMap::new();
    for k in KEYS {
        let (section, name) = k.name.split_once('.').expect("keys are section.name");
        let mut value = (k.default.to_string(), Source::Default);
        if let Some(v) = table.get(section).and_then(|s| s.get(name)) {
            match from_toml(k.kind, v) {
                Ok(text) => value = (text, Source::File),
                Err(e) => problems.push(format!("{}: {e}; using {}", k.name, value.0)),
            }
        }
        if let Some(var) = k.env {
            if let Some(raw) = env(var) {
                // ADAGE_CONTROL keeps its meaning from before the file: any number, where 1 or
                // more is shadow mode.
                let ok = match k.kind {
                    Kind::Share => raw.trim().parse::<f64>().ok().filter(|x| x.is_finite()).map(|x| x.to_string()),
                    Kind::Bool => parse_bool(&raw).map(|b| b.to_string()),
                    _ => from_text(k.kind, &raw).ok().map(|_| raw.trim().to_string()),
                };
                match ok {
                    Some(text) => value = (text, Source::Env(var)),
                    None => problems.push(format!("{var}={raw:?} is not valid for {} (ignored)", k.name)),
                }
            }
        }
        values.insert(k.name, value);
    }
    let text = |n: &str| values[n].0.clone();
    let config = Config {
        learn_auto: parse_bool(&text("learn.auto")).unwrap_or(true),
        learn_repos: Some(text("learn.repos")).filter(|s| s != "all").map(|s| split_ids(&s)),
        learn_min_interval_secs: text("learn.min_interval_secs").parse().unwrap_or(120),
        learn_thresholds: if text("learn.thresholds") == "demo" { Thresholds::Demo } else { Thresholds::Default },
        learn_claude_projects: home_relative(&text("learn.claude_projects")),
        learn_codex_sessions: Some(text("learn.codex_sessions"))
            .filter(|s| s != "auto")
            .and_then(|s| home_relative(&s)),
        learn_codex_auto: text("learn.codex_sessions") == "auto",
        ui_notices: text("ui.notices") != "off",
        learn_opencode_exports: home_relative(&text("learn.opencode_exports")),
        learn_parse_cache: parse_bool(&text("learn.parse_cache")).unwrap_or(true),
        runtime_control: text("runtime.control").parse().unwrap_or(0.1),
        runtime_rewrite: parse_bool(&text("runtime.rewrite")).unwrap_or(true),
        runtime_routine_model: text("runtime.routine_model"),
        runtime_predict_cap: text("runtime.predict_cap").parse().unwrap_or(6),
        skills_invoke: parse_bool(&text("skills.invoke")).unwrap_or(false),
        ui_spinner: parse_bool(&text("ui.spinner")).unwrap_or(false),
        ui_turn_line: parse_bool(&text("ui.turn_line")).unwrap_or(false),
        ui_noticed: parse_bool(&text("ui.noticed")).unwrap_or(false),
        learn_cards: parse_bool(&text("learn.cards")).unwrap_or(true),
        learn_card_threshold: text("learn.card_threshold").parse().unwrap_or(0.811),
        instincts_enabled: parse_bool(&text("instincts.enabled")).unwrap_or(true),
        instincts_packs: split_ids(&text("instincts.packs")),
        instincts_disabled: split_ids(&text("instincts.disabled")),
    };
    Loaded { config, values, problems }
}

/// Set `name` to `value` in the file, validated, keeping every other key. Returns the value as
/// stored. Fails, changing nothing, for an unknown key, an invalid value, or a file that cannot be
/// parsed (fix or remove it first).
pub fn set(paths: &Paths, name: &str, value: &str) -> Result<String, String> {
    let k = key(name).ok_or_else(|| {
        format!("unknown key {name}; keys: {}", KEYS.iter().map(|k| k.name).collect::<Vec<_>>().join(", "))
    })?;
    let v = from_text(k.kind, value).map_err(|e| format!("{name}: {e}"))?;
    let file = paths.config();
    let mut table: toml::Table = match std::fs::read_to_string(&file) {
        Ok(text) => {
            text.parse().map_err(|e| format!("{} cannot be parsed; fix or remove it first: {e}", file.display()))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(format!("{} cannot be read: {e}", file.display())),
    };
    let (section, key_name) = name.split_once('.').expect("keys are section.name");
    let sect = table.entry(section.to_string()).or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let Some(sect) = sect.as_table_mut() else {
        return Err(format!("{}: [{section}] is not a table; fix or remove it first", file.display()));
    };
    let stored = from_toml(k.kind, &v).unwrap_or_else(|_| value.to_string());
    sect.insert(key_name.to_string(), v);
    let body = toml::to_string_pretty(&table).map_err(|e| e.to_string())?;
    write_atomic(&file, body.as_bytes()).map_err(|e| format!("writing {}: {e}", file.display()))?;
    Ok(stored)
}

/// Write `bytes` to `path` through a temporary file renamed into place.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn defaults_without_a_file() {
        let d = tempfile::tempdir().unwrap();
        let l = load_from(&d.path().join(FILE), &no_env);
        assert!(l.problems.is_empty());
        let c = &l.config;
        assert!(c.learn_auto);
        assert_eq!(c.learn_min_interval_secs, 120);
        assert_eq!(c.learn_thresholds, Thresholds::Default);
        assert_eq!((c.learn_claude_projects.clone(), c.learn_codex_sessions.clone()), (None, None));
        assert!(c.learn_codex_auto && c.ui_notices);
        assert!(c.learn_parse_cache);
        assert_eq!(c.runtime_control, 0.1);
        assert!(c.instincts_enabled);
        assert_eq!(c.instincts_packs, vec!["auto".to_string()]);
        assert!(c.instincts_disabled.is_empty());
        assert_eq!(c.learn_repos, None, "every repository");
        assert!(c.learns("github.com/acme/billing"));
        assert!(l.values.values().all(|(_, s)| *s == Source::Default));
        assert_eq!(l.values.len(), KEYS.len());
    }

    #[test]
    fn env_beats_file_beats_default() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join(FILE);
        std::fs::write(&f, "[learn]\nauto = false\nthresholds = \"demo\"\n[runtime]\ncontrol = 0.25\n").unwrap();
        let l = load_from(&f, &no_env);
        assert!(!l.config.learn_auto);
        assert_eq!(l.config.learn_thresholds, Thresholds::Demo);
        assert_eq!(l.config.runtime_control, 0.25);
        assert_eq!(l.get("learn.auto").unwrap().1, Source::File);
        assert_eq!(l.get("learn.min_interval_secs").unwrap(), &("120".to_string(), Source::Default));
        let env = |v: &str| match v {
            "ADAGE_CONTROL" => Some("1".to_string()),
            "ADAGE_AUTO_LEARN" => Some("1".to_string()),
            _ => None,
        };
        let l = load_from(&f, &env);
        assert!(l.config.learn_auto, "ADAGE_AUTO_LEARN wins over the file");
        assert_eq!(l.config.runtime_control, 1.0, "ADAGE_CONTROL keeps shadow mode");
        assert_eq!(l.get("runtime.control").unwrap().1, Source::Env("ADAGE_CONTROL"));
        // An invalid environment value is ignored, and reported.
        let bad = |v: &str| (v == "ADAGE_CONTROL").then(|| "lots".to_string());
        let l = load_from(&f, &bad);
        assert_eq!(l.config.runtime_control, 0.25);
        assert!(l.problems.iter().any(|p| p.contains("ADAGE_CONTROL")), "{:?}", l.problems);
    }

    #[test]
    fn bad_values_keep_their_default_and_are_reported() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join(FILE);
        std::fs::write(
            &f,
            "[learn]\nauto = \"yes please\"\nmin_interval_secs = -5\nthresholds = \"lax\"\ncodex_sessions = \"relative/dir\"\ntypo = 1\n[runtime]\ncontrol = 3.0\n",
        )
        .unwrap();
        let l = load_from(&f, &no_env);
        assert_eq!(l.config, load_from(&d.path().join("none.toml"), &no_env).config);
        assert_eq!(l.problems.len(), 6, "{:?}", l.problems);
        assert!(l.problems.iter().any(|p| p.contains("unknown key learn.typo")));
        std::fs::write(&f, "[learn\nauto = false").unwrap();
        let l = load_from(&f, &no_env);
        assert!(l.config.learn_auto, "an unparseable file means defaults");
        assert!(l.problems[0].contains("cannot be parsed"));
    }

    #[test]
    fn set_validates_and_keeps_other_keys() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        std::fs::write(paths.config(), "[learn]\nauto = false\n[other]\nkept = 1\n").unwrap();
        assert_eq!(set(&paths, "learn.min_interval_secs", "30").unwrap(), "30");
        assert_eq!(set(&paths, "learn.thresholds", "demo").unwrap(), "demo");
        assert_eq!(set(&paths, "runtime.control", "0.2").unwrap(), "0.2");
        assert_eq!(set(&paths, "runtime.predict_cap", "2").unwrap(), "2");
        assert_eq!(set(&paths, "learn.codex_sessions", "~/.codex/sessions").unwrap(), "~/.codex/sessions");
        assert_eq!(set(&paths, "instincts.disabled", "adage/lockfiles, U3").unwrap(), "adage/lockfiles,U3");
        assert_eq!(set(&paths, "instincts.disabled", "U3, N1").unwrap(), "U3,N1");
        assert_eq!(set(&paths, "instincts.enabled", "false").unwrap(), "false");
        assert_eq!(
            set(&paths, "learn.repos", "github.com/acme/billing, notes@1a2b3c4d5e6f").unwrap(),
            "github.com/acme/billing,notes@1a2b3c4d5e6f"
        );
        for (k, v) in [
            ("learn.min_interval_secs", "soon"),
            ("learn.min_interval_secs", "100000"),
            ("learn.thresholds", "lax"),
            ("runtime.control", "1.5"),
            ("runtime.predict_cap", "51"),
            ("runtime.predict_cap", "-1"),
            ("runtime.predict_cap", "two"),
            ("learn.auto", "maybe"),
            ("learn.codex_sessions", "relative"),
            ("learn.nope", "1"),
            ("instincts.disabled", "U3;rm -rf"),
            ("instincts.packs", "a b"),
            ("instincts.packs", "adage/python-env/x"),
            ("instincts.disabled", "../lockfiles"),
            ("learn.repos", "all, github.com/acme/billing"),
            ("learn.repos", "github.com/acme/my billing"),
        ] {
            assert!(set(&paths, k, v).is_err(), "{k} = {v} must be refused");
        }
        let l = load_from(&paths.config(), &no_env);
        assert!(!l.config.learn_auto, "untouched keys are kept");
        assert_eq!(l.config.learn_min_interval_secs, 30);
        assert_eq!(l.config.learn_thresholds, Thresholds::Demo);
        assert_eq!(l.config.runtime_control, 0.2);
        assert_eq!(l.config.runtime_predict_cap, 2);
        assert!(l.config.learn_codex_sessions.as_ref().unwrap().ends_with(".codex/sessions"));
        assert_eq!(l.config.instincts_disabled, vec!["U3".to_string(), "N1".to_string()]);
        assert!(!l.config.instincts_enabled);
        let repos = vec!["github.com/acme/billing".to_string(), "notes@1a2b3c4d5e6f".to_string()];
        assert_eq!(l.config.learn_repos, Some(repos));
        assert!(l.config.learns("notes@1a2b3c4d5e6f") && !l.config.learns("github.com/acme/other"));
        assert_eq!(l.problems, vec!["unknown key other.kept (ignored)".to_string()], "the list reads back cleanly");
        let text = std::fs::read_to_string(paths.config()).unwrap();
        assert!(text.contains("kept = 1"), "{text}");
        // An unparseable file is never overwritten.
        std::fs::write(paths.config(), "[learn\n").unwrap();
        assert!(set(&paths, "learn.auto", "true").is_err());
        assert_eq!(std::fs::read_to_string(paths.config()).unwrap(), "[learn\n");
    }
}
