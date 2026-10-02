//! Trace formats: one decoder per harness history format, chosen by the trace's first bytes.
//!
//! A [`Format`] turns the bytes of one trace into the canonical [`Session`], whatever the bytes
//! came from ([`crate::source`]). Adding a harness's history means adding one `Format` here and
//! listing it in [`FORMATS`]; nothing past ingestion changes.

use crate::locate::{LocalCheckout, Locator};
use adage_core::CommandTable;
use adage_trace::{Harness, Session};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// What a decoder needs besides the trace.
#[derive(Clone, Copy)]
pub struct Decode<'a> {
    pub table: &'a CommandTable,
    /// Which repository the trace's recorded directory belongs to.
    pub locator: &'a dyn Locator,
    /// `--repo <key>`: the repository key to use instead of the derived one.
    pub repo_override: Option<&'a str>,
    /// The trace is a sub-agent's own (Claude Code `<session>/subagents/*.jsonl`).
    pub subagent: bool,
}

impl<'a> Decode<'a> {
    /// Decoding on this machine: repositories are found from their local checkouts.
    pub fn local(table: &'a CommandTable, repo_override: Option<&'a str>) -> Self {
        Decode { table, locator: &LocalCheckout, repo_override, subagent: false }
    }
}

/// One harness's history format.
pub trait Format: Sync {
    /// The harness whose history this is ([`Session::harness`] holds its id).
    fn harness(&self) -> Harness;
    /// Whether a trace that begins with `head` is in this format. `head` is the trace's first line
    /// (at most [`HEAD_LINE`] bytes), extended to at least [`HEAD_MIN`] bytes when the trace has
    /// them.
    fn sniff(&self, head: &[u8]) -> bool;
    /// The session in the trace; `None` when it holds no usable session (no prompt, a sub-agent
    /// thread its format skips, unreadable).
    fn decode(&self, trace: &mut dyn BufRead, cx: &Decode<'_>) -> Option<Session>;
}

/// The longest first line read to recognise a format (a Codex rollout's `session_meta`).
pub const HEAD_LINE: u64 = 1 << 20;
/// The fewest leading bytes read to recognise a format (an OpenCode export's opening key).
pub const HEAD_MIN: usize = 64;

/// Every format, in the order they are tried. Claude Code transcripts recognise anything (their
/// decoder finds no session in what is not one), so they come last.
pub static FORMATS: [&dyn Format; 3] = [&crate::codex::Rollout, &crate::opencode::Export, &crate::claude::Transcript];

/// The format of a trace that begins with `head`.
pub fn detect(head: &[u8]) -> Option<&'static dyn Format> {
    FORMATS.iter().copied().find(|f| f.sniff(head))
}

/// The first line of `r` (at most [`HEAD_LINE`] bytes), extended to [`HEAD_MIN`] bytes.
fn head(r: &mut impl BufRead) -> std::io::Result<Vec<u8>> {
    let mut head = Vec::new();
    r.by_ref().take(HEAD_LINE).read_until(b'\n', &mut head)?;
    if head.len() < HEAD_MIN {
        r.by_ref().take((HEAD_MIN - head.len()) as u64).read_to_end(&mut head)?;
    }
    Ok(head)
}

/// Decode one trace from any reader, recognising its format by its first bytes.
pub fn decode_trace(trace: impl Read, cx: &Decode<'_>) -> Option<Session> {
    let mut r = BufReader::new(trace);
    let head = head(&mut r).ok()?;
    let format = detect(&head)?;
    let mut whole = BufReader::new(std::io::Cursor::new(head).chain(r));
    format.decode(&mut whole, cx)
}

/// The format of the trace at `path`, recognised as [`decode_trace`] would.
pub fn detect_path(path: &Path) -> Option<&'static dyn Format> {
    let mut r = BufReader::new(std::fs::File::open(path).ok()?);
    detect(&head(&mut r).ok()?)
}

/// Read one session file on this machine: a Claude Code transcript, an OpenCode `opencode export`
/// file, or a Codex rollout (recognised by content).
pub fn read_session(path: &Path, table: &CommandTable, repo_override: Option<&str>) -> Option<Session> {
    read_trace(path, &Decode::local(table, repo_override))
}

/// Read the trace file at `path` through `cx`, whose locator decides what repository it is. Whether
/// it is a sub-agent's own trace is read from the path.
pub fn read_trace(path: &Path, cx: &Decode<'_>) -> Option<Session> {
    let cx = Decode { subagent: crate::claude::is_subagent(path), ..*cx };
    decode_trace(std::fs::File::open(path).ok()?, &cx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn harness(head: &[u8]) -> Option<&'static str> {
        detect(head).map(|f| f.harness().id())
    }

    #[test]
    fn each_format_is_recognised_by_its_first_bytes() {
        let rollout = format!("{}\n", json!({"timestamp": "t", "type": "session_meta", "payload": {"id": "x"}}));
        assert_eq!(harness(rollout.as_bytes()), Some("codex"));
        // `opencode export` pretty-prints: the first line is `{` alone, the key on the next.
        assert_eq!(harness(b"{\n  \"info\": {\n    \"id\": \"ses_1\""), Some("opencode"));
        assert_eq!(harness(b"{\"info\":{\"id\":\"ses_1\"},\"messages\":[]}"), Some("opencode"));
        let transcript = format!("{}\n", json!({"type": "user", "sessionId": "s", "cwd": "/r"}));
        assert_eq!(harness(transcript.as_bytes()), Some("claude-code"));
        assert_eq!(harness(b""), Some("claude-code"), "the fallback decodes it to no session");
    }

    #[test]
    fn near_misses_are_not_taken_for_another_format() {
        // A `session_meta` line without its payload object or timestamp is not a rollout head.
        let partial = format!("{}\n", json!({"type": "session_meta", "payload": "x"}));
        assert_eq!(harness(partial.as_bytes()), Some("claude-code"));
        // An `info` key that is not the first key is not an export's opening.
        assert_eq!(harness(b"{\"messages\": [], \"info\": {}}"), Some("claude-code"));
        // Only the first line is read for a rollout head: a later `session_meta` does not count.
        let later = format!(
            "{}\n{}\n",
            json!({"type": "user"}),
            json!({"timestamp": "t", "type": "session_meta", "payload": {}})
        );
        assert_eq!(harness(later.as_bytes()), Some("claude-code"));
    }

    /// The registry reads every file exactly as the per-format readers it replaced did.
    #[test]
    fn the_registry_matches_each_formats_own_reader() {
        let d = tempfile::tempdir().unwrap();
        let table = CommandTable::load();
        let rollout = [
            json!({"timestamp": "2026-09-01T10:00:00Z", "type": "session_meta", "payload": {"id": "x1", "cwd": "/tmp/acme", "timestamp": "2026-09-01T10:00:00Z"}}),
            json!({"timestamp": "2026-09-01T10:00:01Z", "type": "event_msg", "payload": {"type": "user_message", "message": "run the tests"}}),
            json!({"timestamp": "2026-09-01T10:00:02Z", "type": "response_item", "payload": {"type": "function_call", "name": "exec_command", "call_id": "k1", "arguments": "{\"cmd\":\"make test\"}"}}),
            json!({"timestamp": "2026-09-01T10:00:03Z", "type": "response_item", "payload": {"type": "function_call_output", "call_id": "k1", "output": "Process exited with code 2\nOutput:\nboom"}}),
        ];
        let export = json!({
            "info": {"id": "ses_1", "directory": "/tmp/acme", "time": {"created": 1_788_000_000_000i64}},
            "messages": [
                {"info": {"role": "user"}, "parts": [{"type": "text", "text": "run the tests"}]},
                {"info": {"role": "assistant", "modelID": "m"}, "parts": [
                    {"type": "tool", "tool": "bash", "state": {"status": "completed", "input": {"command": "make test"}, "output": "ok", "metadata": {"exit": 0}}}
                ]}
            ]
        });
        let transcript = [
            json!({"type": "user", "sessionId": "c1", "cwd": "/tmp/acme", "message": {"role": "user", "content": "run the tests"}}),
            json!({"type": "assistant", "sessionId": "c1", "cwd": "/tmp/acme", "message": {"id": "m1", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "make test"}}]}}),
        ];
        let lines = |v: &[serde_json::Value]| v.iter().map(|l| format!("{l}\n")).collect::<String>();
        let (r, e, t) = (d.path().join("r.jsonl"), d.path().join("e.json"), d.path().join("t.jsonl"));
        std::fs::write(&r, lines(&rollout)).unwrap();
        std::fs::write(&e, serde_json::to_string_pretty(&export).unwrap()).unwrap();
        std::fs::write(&t, lines(&transcript)).unwrap();

        let own_r = crate::codex::parse_rollout(&crate::codex::read_rollout(&r).unwrap(), &table, Some("acme"));
        let own_e = crate::opencode::parse_export(&crate::opencode::read_export(&e).unwrap(), &table, Some("acme"));
        let own_t = crate::claude::parse_transcript(&t, &table, Some("acme"));
        for (path, own) in [(&r, own_r), (&e, own_e), (&t, own_t)] {
            let own = own.unwrap_or_else(|| panic!("{}: no session", path.display()));
            assert_eq!(read_session(path, &table, Some("acme")).as_ref(), Some(&own), "{}", path.display());
            assert_eq!(detect_path(path).map(|f| f.harness().id()), Some(own.harness.as_str()));
        }
        assert!(read_session(&d.path().join("missing.jsonl"), &table, None).is_none());
    }
}
