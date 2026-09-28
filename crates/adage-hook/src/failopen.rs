//! The hook's fail-open record: `$ADAGE_HOME/fail-open.log`, read by `adage doctor`.
//!
//! A hook that hits a panic or an internal error says nothing to the agent and exits 0. It also
//! appends one line here, so the failure is not silent to the person. Each line is one JSON object:
//!
//! ```json
//! {"ts":1758620000000,"harness":"claude","event":"UserPromptSubmit","class":"panic","error":"panic at adage-runtime/src/lib.rs:42:9","version":"0.1.0"}
//! ```
//!
//! - `ts` is milliseconds since the Unix epoch.
//! - `harness` and `event` are the hook's own arguments, reduced to `[A-Za-z0-9._-]`, 40 characters.
//! - `class` is one of [`Failure`]'s fixed classes; `error` is a fixed message, an I/O error kind,
//!   or a panic's source location (the last three path components). It never carries the payload:
//!   no prompt, tool output, command or path, and a panic's message is never recorded, because it
//!   can quote the data that caused it.
//!
//! The file is bounded: at [`ROTATE_BYTES`] it is renamed to `fail-open.log.1` (replacing the
//! previous one) before the next line is written. Recording is itself fail-open: every error is
//! ignored, and nothing is written if the data directory does not exist (a hook never creates
//! it). It runs only on the failure path, so a hook that succeeds pays nothing for it.

use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const FAIL_OPEN_LOG: &str = "fail-open.log";
/// The previous log, after rotation.
pub const FAIL_OPEN_LOG_PREV: &str = "fail-open.log.1";
/// The log is rotated when it reaches this size: 256 KiB, about 1,500 entries.
pub const ROTATE_BYTES: u64 = 256 << 10;

/// Why a hook said nothing because something went wrong (not because there was nothing to say).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// `panic`, `thread`, `config`, `io`, `payload`, `state` or `spool`.
    pub class: &'static str,
    pub message: String,
}

impl Failure {
    pub fn new(class: &'static str, message: impl Into<String>) -> Failure {
        Failure { class, message: message.into() }
    }

    /// An I/O failure, described by its kind only: an `io::Error`'s text can name a path.
    pub fn io(class: &'static str, what: &str, e: &std::io::Error) -> Failure {
        Failure::new(class, format!("{what}: {:?}", e.kind()))
    }
}

/// Where the last panic happened, set by [`install_panic_hook`].
static PANIC_AT: Mutex<Option<String>> = Mutex::new(None);

/// Replace the panic hook with one that prints nothing (nothing but our JSON may reach the
/// harness) and remembers only the panic's source location.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        if let (Some(l), Ok(mut at)) = (info.location(), PANIC_AT.try_lock()) {
            *at = Some(format!("{}:{}:{}", short_source(l.file()), l.line(), l.column()));
        }
    }));
}

/// The failure for a caught panic: its location, never its message.
pub fn panic_failure() -> Failure {
    let at = PANIC_AT.try_lock().ok().and_then(|mut g| g.take());
    Failure::new("panic", format!("panic at {}", at.as_deref().unwrap_or("an unknown location")))
}

/// The last three components of a source path: `adage-runtime/src/lib.rs`, never a home directory
/// (dependencies' sources live under `~/.cargo/registry`).
fn short_source(file: &str) -> String {
    let parts: Vec<&str> = file.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    parts[parts.len().saturating_sub(3)..].join("/")
}

/// A hook argument, reduced to a safe token.
fn token(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).take(40).collect()
}

/// Append one entry to `<root>/fail-open.log`, rotating it first if it is full. Never fails.
pub fn record(root: &Path, harness: &str, event: &str, f: &Failure) {
    let _ = std::panic::catch_unwind(|| try_record(root, harness, event, f));
}

fn try_record(root: &Path, harness: &str, event: &str, f: &Failure) -> std::io::Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    let path = root.join(FAIL_OPEN_LOG);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= ROTATE_BYTES) {
        std::fs::rename(&path, root.join(FAIL_OPEN_LOG_PREV))?;
    }
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let message: String = f.message.chars().filter(|c| !c.is_control()).take(200).collect();
    let line = json!({
        "ts": ts,
        "harness": token(harness),
        "event": token(event),
        "class": f.class,
        "error": format!("{}: {message}", f.class),
        "version": env!("CARGO_PKG_VERSION"),
    });
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    // One write per entry: concurrent hooks append whole lines.
    file.write_all(format!("{line}\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_bounded_and_rotate() {
        let d = tempfile::tempdir().unwrap();
        let f = Failure::new("state", "update: WouldBlock");
        std::fs::write(d.path().join(FAIL_OPEN_LOG), vec![b'x'; ROTATE_BYTES as usize]).unwrap();
        record(d.path(), "claude", "Stop", &f);
        let prev = std::fs::metadata(d.path().join(FAIL_OPEN_LOG_PREV)).unwrap().len();
        assert_eq!(prev, ROTATE_BYTES, "the full log became the previous one");
        let text = std::fs::read_to_string(d.path().join(FAIL_OPEN_LOG)).unwrap();
        let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(v["error"], "state: update: WouldBlock");
        assert_eq!(
            (v["harness"].as_str(), v["event"].as_str(), v["class"].as_str()),
            (Some("claude"), Some("Stop"), Some("state"))
        );
        assert!(v["ts"].as_u64().unwrap() > 0);
        // A second rotation replaces the previous log: at most two files, each bounded.
        std::fs::write(d.path().join(FAIL_OPEN_LOG), vec![b'y'; ROTATE_BYTES as usize + 10]).unwrap();
        record(d.path(), "claude", "Stop", &f);
        assert_eq!(std::fs::metadata(d.path().join(FAIL_OPEN_LOG_PREV)).unwrap().len(), ROTATE_BYTES + 10);
    }

    #[test]
    fn arguments_and_messages_are_reduced() {
        let d = tempfile::tempdir().unwrap();
        let f = Failure::new("payload", format!("x\n{}", "y".repeat(500)));
        record(d.path(), "claude; rm -rf ~", "Stop\u{202e}/../../x", &f);
        let v: serde_json::Value =
            serde_json::from_str(std::fs::read_to_string(d.path().join(FAIL_OPEN_LOG)).unwrap().trim()).unwrap();
        assert_eq!(v["harness"], "clauderm-rf");
        assert_eq!(v["event"], "Stop....x");
        assert!(v["error"].as_str().unwrap().len() < 220);
        assert!(!v["error"].as_str().unwrap().contains('\n'));
    }

    #[test]
    fn nothing_is_written_without_a_data_directory() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("nowhere");
        record(&root, "claude", "Stop", &Failure::new("io", "x"));
        assert!(!root.exists(), "a hook never creates the data directory");
    }

    #[test]
    fn source_locations_keep_no_home_directory() {
        assert_eq!(short_source("crates/adage-runtime/src/lib.rs"), "adage-runtime/src/lib.rs");
        assert_eq!(
            short_source("/Users/someone/.cargo/registry/src/index/tree-sitter-0.24.7/binding_rust/lib.rs"),
            "tree-sitter-0.24.7/binding_rust/lib.rs"
        );
        assert_eq!(short_source(r"C:\Users\someone\x\src\lib.rs"), "x/src/lib.rs");
    }
}
