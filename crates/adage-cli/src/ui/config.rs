//! `$ADAGE_HOME/config.toml`, as the Settings tab and the init wizard read and edit it.
//!
//! **A stand-in.** The live-learning work owns Adage's configuration (auto-learn, its interval
//! and thresholds, the holdout share). Until that module lands, this is a minimal reader and
//! writer for the same keys, behind [`ConfigStore`], so the real module can replace [`TomlFile`]
//! without touching the screens. It edits the file line by line, so comments and every other key
//! are kept; a file it cannot parse is never rewritten.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use toml::Value;

pub const FILE: &str = "config.toml";

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Bool,
    Int { min: i64, max: i64 },
    Float { min: f64, max: f64 },
    Choice(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy)]
pub struct Setting {
    pub key: &'static str,
    pub kind: Kind,
    pub help: &'static str,
}

/// The keys the UI edits.
pub const SETTINGS: &[Setting] = &[
    Setting { key: "learn.auto", kind: Kind::Bool, help: "learn automatically after agent sessions end" },
    Setting {
        key: "learn.min_interval_secs",
        kind: Kind::Int { min: 60, max: 7 * 24 * 3600 },
        help: "the shortest time between automatic learning runs, in seconds",
    },
    Setting {
        key: "learn.thresholds",
        kind: Kind::Choice(&["default", "demo"]),
        help: "the evidence bars routes must clear: default, or demo (for small demonstrations)",
    },
    Setting {
        key: "runtime.holdout",
        kind: Kind::Float { min: 0.0, max: 1.0 },
        help: "the share of sessions held out as a control (nothing delivered); ADAGE_HOLDOUT overrides it",
    },
];

pub fn setting(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
}

/// Where settings live and how they change. [`TomlFile`] is the stand-in implementation.
pub trait ConfigStore {
    fn path(&self) -> &Path;
    /// The value set for `key` (`section.name`), if any.
    fn get(&self, key: &str) -> Result<Option<Value>>;
    /// Set `key`, keeping everything else in the file.
    fn set(&self, key: &str, value: Value) -> Result<()>;
}

pub struct TomlFile {
    path: PathBuf,
}

impl TomlFile {
    pub fn in_home(root: &Path) -> TomlFile {
        TomlFile { path: root.join(FILE) }
    }

    fn read(&self) -> Result<Option<String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", self.path.display())),
        }
    }
}

fn lookup<'a>(t: &'a toml::Table, key: &str) -> Option<&'a Value> {
    let mut cur: Option<&Value> = None;
    for (i, part) in key.split('.').enumerate() {
        cur = if i == 0 { t.get(part) } else { cur?.as_table()?.get(part) };
    }
    cur
}

impl ConfigStore for TomlFile {
    fn path(&self) -> &Path {
        &self.path
    }

    fn get(&self, key: &str) -> Result<Option<Value>> {
        let Some(text) = self.read()? else { return Ok(None) };
        let t: toml::Table =
            toml::from_str(&text).with_context(|| format!("{} is not valid TOML", self.path.display()))?;
        Ok(lookup(&t, key).cloned())
    }

    fn set(&self, key: &str, value: Value) -> Result<()> {
        let text = self.read()?.unwrap_or_default();
        if !text.trim().is_empty() {
            toml::from_str::<toml::Table>(&text)
                .with_context(|| format!("{} is not valid TOML; fix it by hand first", self.path.display()))?;
        }
        let out = set_in(&text, key, &value)?;
        let parsed: toml::Table = toml::from_str(&out).with_context(|| {
            format!("{} sets {key} in a form this editor does not handle; edit it by hand", self.path.display())
        })?;
        if lookup(&parsed, key) != Some(&value) {
            bail!("{} sets {key} in a form this editor does not handle; edit it by hand", self.path.display());
        }
        let dir = self.path.parent().context("no parent directory")?;
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".config-{}.tmp", std::process::id()));
        std::fs::write(&tmp, out)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// The key a `name = value` line sets, if it is one.
fn line_key(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if t.starts_with('#') || t.starts_with('[') {
        return None;
    }
    let (k, _) = t.split_once('=')?;
    Some(k.trim().trim_matches('"'))
}

fn header(line: &str) -> Option<String> {
    let t = line.trim();
    if t.starts_with("[[") {
        return Some(String::new()); // an array of tables: never ours
    }
    let inner = t.strip_prefix('[')?.split(']').next()?;
    Some(inner.trim().to_string())
}

/// `text` with `section.name = value` set: the existing line replaced, or a new one added at the
/// end of its section (or a new section at the end of the file).
pub fn set_in(text: &str, key: &str, value: &Value) -> Result<String> {
    let (section, name) = key.split_once('.').context("keys are section.name")?;
    let line = format!("{name} = {value}");
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut current: Option<String> = None;
    let mut last_in_section: Option<usize> = None;
    for i in 0..lines.len() {
        if let Some(h) = header(&lines[i]) {
            current = Some(h.clone());
            if h == section {
                last_in_section = Some(i);
            }
            continue;
        }
        if current.as_deref() == Some(section) {
            if line_key(&lines[i]) == Some(name) {
                lines[i] = line;
                return Ok(lines.join("\n") + "\n");
            }
            if !lines[i].trim().is_empty() {
                last_in_section = Some(i);
            }
        }
    }
    match last_in_section {
        Some(i) => lines.insert(i + 1, line),
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(format!("[{section}]"));
            lines.push(line);
        }
    }
    Ok(lines.join("\n") + "\n")
}

/// Parse and validate what a person typed for `s`.
pub fn parse_value(s: &Setting, raw: &str) -> Result<Value> {
    let raw = raw.trim();
    Ok(match s.kind {
        Kind::Bool => match raw {
            "true" | "on" | "yes" => Value::Boolean(true),
            "false" | "off" | "no" => Value::Boolean(false),
            _ => bail!("{} is true or false", s.key),
        },
        Kind::Int { min, max } => {
            let n: i64 = raw.parse().with_context(|| format!("{} is a whole number", s.key))?;
            if !(min..=max).contains(&n) {
                bail!("{} must be between {min} and {max}", s.key);
            }
            Value::Integer(n)
        }
        Kind::Float { min, max } => {
            let x: f64 = raw.parse().with_context(|| format!("{} is a number", s.key))?;
            if !(min..=max).contains(&x) || x.is_nan() {
                bail!("{} must be between {min} and {max}", s.key);
            }
            Value::Float(x)
        }
        Kind::Choice(options) => {
            if !options.contains(&raw) {
                bail!("{} is one of: {}", s.key, options.join(", "));
            }
            Value::String(raw.to_string())
        }
    })
}

/// A value as shown: TOML's own spelling, without quotes around strings.
pub fn show(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Table(_) => "(a table; edit config.toml by hand)".into(),
        other => other.to_string(),
    }
}

/// `learn.auto`, if it is set to a boolean.
pub fn auto_learn(root: &Path) -> Option<bool> {
    TomlFile::in_home(root).get("learn.auto").ok().flatten().and_then(|v| v.as_bool())
}
