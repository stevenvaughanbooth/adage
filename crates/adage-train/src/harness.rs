//! The harness driver: how one headless session of the person's own agent is started. Pluggable,
//! so the whole pipeline can run against [`crate::fake::Fake`] in tests without spending anything.

use crate::tasks::Task;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Which arm a session belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    /// Exploration: the skill in context, no hooks. Mined into staged reflexes.
    Explore,
    /// Validation: no skill, no hooks.
    Baseline,
    /// Validation: the skill appended to the system prompt, no hooks.
    Skill,
    /// Validation: no skill; Adage hooks on, reading only the staged reflexes.
    Reflex,
}

impl Arm {
    pub fn as_str(self) -> &'static str {
        match self {
            Arm::Explore => "explore",
            Arm::Baseline => "baseline",
            Arm::Skill => "skill",
            Arm::Reflex => "reflex",
        }
    }
    pub fn skill_in_context(self) -> bool {
        matches!(self, Arm::Explore | Arm::Skill)
    }
    pub fn hooks(self) -> bool {
        self == Arm::Reflex
    }
}

/// Everything one session needs.
#[derive(Debug, Clone)]
pub struct SessionSpec {
    /// The job id (`explore-t03`, `validate-v02-reflex`).
    pub job: String,
    pub arm: Arm,
    pub task: Task,
    pub prompt: String,
    /// The disposable clone the agent works in.
    pub cwd: PathBuf,
    /// The complete environment (nothing else of the operator's is inherited).
    pub env: BTreeMap<String, String>,
    /// The harness's config directory (its transcripts land under `<config_dir>/projects`).
    pub config_dir: PathBuf,
    /// Where the harness's raw output is kept (`stream.jsonl`).
    pub session_dir: PathBuf,
    pub model: String,
    pub max_turns: u32,
    /// The skill text, for arms that have it in context.
    pub append_system_prompt: Option<String>,
    /// The settings file the session loads (sandbox; hooks for the reflex arm).
    pub settings: PathBuf,
    /// The harness's own per-session spending cap.
    pub max_budget_usd: f64,
    pub timeout: Duration,
}

/// What a session reported.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Outcome {
    pub session_id: Option<String>,
    pub cost_usd: f64,
    pub turns: u32,
    /// The harness's result subtype (`success`, `error_max_turns`, …).
    pub subtype: Option<String>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub trait Harness: Send + Sync {
    fn name(&self) -> &'static str;
    /// Whether sessions run a real agent (and spend money): `run --execute` then requires a person.
    fn spawns_agents(&self) -> bool;
    /// The operator's variables that carry this harness's credentials; the only ones passed
    /// through besides the isolation allowlist.
    fn auth_env(&self) -> &'static [&'static str];
    /// Put credentials into a fresh config directory, when auth needs a file rather than a
    /// variable. Removed again by [`Harness::cleanup`] right after the session.
    fn prepare(&self, _config_dir: &Path, _base: &BTreeMap<String, String>) -> Result<()> {
        Ok(())
    }
    fn cleanup(&self, _config_dir: &Path) {}
    /// The exact command line a session runs (printed by the dry run).
    fn command(&self, spec: &SessionSpec) -> Vec<String>;
    /// Settings for a session: the sandbox, and for the reflex arm the Adage hooks block.
    fn settings(&self, hooks: Option<&serde_json::Value>) -> serde_json::Value;
    fn run(&self, spec: &SessionSpec) -> Result<Outcome>;
}

/// The tools a training session may use, and those it may not.
pub const ALLOWED_TOOLS: &[&str] = &["Bash", "Read", "Edit", "Write", "Glob", "Grep"];
pub const DISALLOWED_TOOLS: &[&str] = &["WebFetch", "WebSearch", "Task", "Agent"];

/// Claude Code, headless (`claude -p`).
pub struct ClaudeCode {
    /// The `claude` executable.
    pub bin: String,
}

impl Default for ClaudeCode {
    fn default() -> Self {
        ClaudeCode { bin: "claude".into() }
    }
}

/// Claude Code's auth variables: an API key, or a token from `claude setup-token`.
pub const CLAUDE_AUTH_ENV: &[&str] =
    &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"];

impl ClaudeCode {
    /// The operator's credentials file, used when no auth variable is set (Linux; macOS keeps
    /// credentials in the Keychain, so set a variable there).
    fn credentials(base: &BTreeMap<String, String>) -> Option<PathBuf> {
        let dir = base
            .get("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| base.get("HOME").map(|h| Path::new(h).join(".claude")))?;
        let p = dir.join(".credentials.json");
        p.is_file().then_some(p)
    }

    /// How sessions will authenticate, for the dry run and the preflight.
    pub fn auth_source(base: &BTreeMap<String, String>) -> Option<String> {
        let vars: Vec<&str> =
            CLAUDE_AUTH_ENV[..3].iter().copied().filter(|k| base.get(*k).is_some_and(|v| !v.is_empty())).collect();
        if !vars.is_empty() {
            return Some(format!("env: {}", vars.join(", ")));
        }
        Self::credentials(base).map(|p| format!("file: a copy of {} per session, removed after it", p.display()))
    }
}

impl Harness for ClaudeCode {
    fn name(&self) -> &'static str {
        "claude-code"
    }
    fn spawns_agents(&self) -> bool {
        true
    }
    fn auth_env(&self) -> &'static [&'static str] {
        CLAUDE_AUTH_ENV
    }
    fn prepare(&self, config_dir: &Path, base: &BTreeMap<String, String>) -> Result<()> {
        if CLAUDE_AUTH_ENV[..3].iter().any(|k| base.get(*k).is_some_and(|v| !v.is_empty())) {
            return Ok(());
        }
        if let Some(src) = Self::credentials(base) {
            let dest = config_dir.join(".credentials.json");
            std::fs::create_dir_all(config_dir)?;
            std::fs::copy(&src, &dest).with_context(|| format!("copying {}", src.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o600))?;
            }
        }
        Ok(())
    }
    fn cleanup(&self, config_dir: &Path) {
        let _ = std::fs::remove_file(config_dir.join(".credentials.json"));
    }
    fn command(&self, s: &SessionSpec) -> Vec<String> {
        let mut c: Vec<String> = vec![
            self.bin.clone(),
            "-p".into(),
            s.prompt.clone(),
            "--model".into(),
            s.model.clone(),
            "--max-turns".into(),
            s.max_turns.to_string(),
            "--max-budget-usd".into(),
            format!("{:.2}", s.max_budget_usd),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
            "--setting-sources".into(),
            "project,local".into(),
            "--strict-mcp-config".into(),
            "--settings".into(),
            s.settings.to_string_lossy().to_string(),
            "--permission-mode".into(),
            "dontAsk".into(),
        ];
        if let Some(t) = &s.append_system_prompt {
            c.push("--append-system-prompt".into());
            c.push(t.clone());
        }
        c.push("--allowedTools".into());
        c.extend(ALLOWED_TOOLS.iter().map(|t| t.to_string()));
        c.push("--disallowedTools".into());
        c.extend(DISALLOWED_TOOLS.iter().map(|t| t.to_string()));
        c
    }
    fn settings(&self, hooks: Option<&serde_json::Value>) -> serde_json::Value {
        // Claude Code's sandbox (bubblewrap on Linux, Seatbelt on macOS): Bash runs sandboxed,
        // writes confined to the working directory, and no command may escape it. Unverified
        // here: no session was run; see docs/training.md.
        let mut v = serde_json::json!({
            "sandbox": {
                "enabled": true,
                "autoAllowBashIfSandboxed": true,
                "allowUnsandboxedCommands": false
            },
            "permissions": { "deny": ["WebFetch", "WebSearch"] }
        });
        if let Some(h) = hooks.and_then(|h| h.get("hooks")) {
            v["hooks"] = h.clone();
        }
        v
    }
    fn run(&self, s: &SessionSpec) -> Result<Outcome> {
        let argv = self.command(s);
        std::fs::create_dir_all(&s.session_dir)?;
        let mut raw = std::fs::File::create(s.session_dir.join("stream.jsonl"))?;
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(&s.cwd)
            .env_clear()
            .envs(&s.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("starting {}", argv[0]))?;
        let stdout = child.stdout.take().context("no stdout")?;
        let reader = std::thread::spawn(move || -> Vec<String> {
            std::io::BufReader::new(stdout).lines().map_while(Result::ok).collect()
        });
        let start = Instant::now();
        let mut timed_out = false;
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if start.elapsed() > s.timeout {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let lines = reader.join().unwrap_or_default();
        let mut out = Outcome::default();
        for l in &lines {
            let _ = writeln!(raw, "{l}");
            let Ok(m) = serde_json::from_str::<serde_json::Value>(l) else { continue };
            match m.get("type").and_then(|t| t.as_str()) {
                Some("system") if m.get("subtype").and_then(|x| x.as_str()) == Some("init") => {
                    out.session_id = m.get("session_id").and_then(|x| x.as_str()).map(str::to_string);
                }
                Some("result") => {
                    out.cost_usd = m.get("total_cost_usd").and_then(|x| x.as_f64()).unwrap_or(0.0);
                    out.turns = m.get("num_turns").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                    out.subtype = m.get("subtype").and_then(|x| x.as_str()).map(str::to_string);
                    out.ok = m.get("is_error").and_then(|x| x.as_bool()) == Some(false);
                }
                _ => {}
            }
        }
        if timed_out {
            out.ok = false;
            out.error = Some(format!("timed out after {}s", s.timeout.as_secs()));
        }
        Ok(out)
    }
}

/// The transcript a session wrote under its config directory, if any.
pub fn find_transcript(config_dir: &Path, session_id: &str) -> Option<PathBuf> {
    let projects = config_dir.join("projects");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&projects).ok()?.flatten().map(|e| e.path()).collect();
    dirs.sort();
    dirs.into_iter().map(|d| d.join(format!("{session_id}.jsonl"))).find(|p| p.is_file())
}
