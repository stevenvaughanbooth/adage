//! Harness adapters (Architecture §4.11). The decision core in `hook.rs` knows only the normalised
//! [`Event`] (prompt submitted, tool batch completed, tool failed, stop requested, session start and
//! end) and the normalised [`Output`]. Each harness contributes two pure functions — translate a
//! native payload into an event, and render an output as the native response — plus its limits.
//!
//! What each harness documents, and what is inferred, is recorded in `docs/harnesses.md`.

pub mod claude;
pub mod codex;
pub mod opencode;

use adage_runtime::Call;
pub use adage_runtime::Event;
use serde_json::Value;
use std::path::PathBuf;

/// What the core decided, before harness rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Directive text (already wrapped and capped) for the model's next turn.
    Context(String),
    /// A stop-hook request to continue, with its reason (fixed template).
    Continue(String),
}

/// Who is calling: the session and working directory every native payload must carry.
pub struct Envelope {
    pub session: String,
    pub cwd: PathBuf,
}

pub trait Adapter: Sync {
    /// The name recorded in spools.
    fn name(&self) -> &'static str;
    /// Session and cwd from the native payload. `None` → do nothing.
    fn envelope(&self, p: &Value) -> Option<Envelope> {
        Some(Envelope {
            session: p.get("session_id")?.as_str()?.to_string(),
            cwd: PathBuf::from(p.get("cwd")?.as_str()?),
        })
    }
    /// Native event name and payload → normalised event. `None` → do nothing.
    fn translate(&self, event: &str, p: &Value, cwd: &str) -> Option<Event>;
    /// Normalised output → the exact bytes the harness expects on stdout.
    fn render(&self, event: &str, out: Output) -> Option<String>;
    /// The largest directive text this harness accepts in one response.
    fn max_context(&self) -> usize {
        9_500
    }
}

pub fn adapter(harness: &str) -> Option<&'static dyn Adapter> {
    match harness {
        "claude" => Some(&claude::Claude),
        "opencode" => Some(&opencode::OpenCode),
        "codex" => Some(&codex::Codex),
        _ => None,
    }
}

/// Text Adage itself wrote: a stop-hook reason (`render::stop_reason`) or a directive. On harnesses
/// where these re-enter as user messages (Codex Stop, OpenCode's context message), they must not
/// be mistaken for the user's prompt.
pub fn is_own_text(prompt: &str) -> bool {
    let t = prompt.trim_start();
    t.starts_with("Adage finish-line check:") || t.starts_with("<adage-signal ")
}

/// Calls for translated `(tool, input)` pairs, with relative file paths anchored at the session's
/// working directory (patch tools name files relative to it).
pub fn calls(pairs: Vec<(String, Value)>, cwd: &str, output: &str, failed: bool) -> Vec<Call> {
    pairs
        .into_iter()
        .map(|(tool, mut input)| {
            if let Some(p) = input.get("file_path").and_then(|v| v.as_str()) {
                if !p.is_empty() && !p.starts_with('/') {
                    input["file_path"] = Value::String(adage_core::shell::resolve_path(cwd, p));
                }
            }
            Call { tool, input, output: output.to_string(), failed }
        })
        .collect()
}
