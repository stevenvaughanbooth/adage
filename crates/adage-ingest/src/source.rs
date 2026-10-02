//! Trace sources: where traces come from, kept apart from what they mean ([`crate::format`]).
//!
//! A [`TraceSource`] lists traces and opens each one as bytes. The local sources here read a
//! harness's history on this machine; a source for a team's trace store (an object store, an HTTP
//! collector) implements the same two methods and needs nothing else from ingestion. Bulk learning
//! (day 0, `adage condition`) walks a whole source ([`ingest`]); live learning decodes the one
//! trace a hook names ([`ingest_one`]). Both go through the same decoder.

use crate::format::{decode_trace, Decode};
use adage_trace::Session;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// One trace, as its source names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TraceRef {
    /// The source's name for the trace: a path on this machine, an object key, a URL.
    pub id: String,
    /// The file on this machine, for a local trace (what the parse cache and scoping key on).
    pub path: Option<PathBuf>,
    /// A sub-agent's own trace (Claude Code `<session>/subagents/*.jsonl`).
    pub subagent: bool,
}

impl TraceRef {
    /// A trace file on this machine.
    pub fn file(path: &Path) -> Self {
        TraceRef {
            id: path.to_string_lossy().into_owned(),
            path: Some(path.to_path_buf()),
            subagent: crate::claude::is_subagent(path),
        }
    }
}

/// Where traces come from.
pub trait TraceSource {
    /// Every trace the source holds, sorted.
    fn list(&self) -> Vec<TraceRef>;
    /// The bytes of one trace.
    fn open(&self, trace: &TraceRef) -> io::Result<Box<dyn Read + '_>>;
}

/// Decode one trace from a source: what live learning does with the trace a hook names.
pub fn ingest_one(source: &dyn TraceSource, trace: &TraceRef, cx: &Decode<'_>) -> Option<Session> {
    let cx = Decode { subagent: trace.subagent, ..*cx };
    decode_trace(source.open(trace).ok()?, &cx)
}

/// Decode every trace of a source, in order, handing each to `sink` with the session it holds
/// (`None`: none). What bulk learning does with a harness's whole history.
pub fn ingest(source: &dyn TraceSource, cx: &Decode<'_>, mut sink: impl FnMut(&TraceRef, Option<Session>)) {
    for t in source.list() {
        let s = ingest_one(source, &t, cx);
        sink(&t, s);
    }
}

/// Opens a local trace file.
fn open_file(trace: &TraceRef) -> io::Result<Box<dyn Read + 'static>> {
    let path = trace.path.as_deref().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not a local trace"))?;
    Ok(Box::new(std::fs::File::open(path)?))
}

/// Trace files on this machine, named one by one (`--transcripts`, a hook's `transcript_path`).
#[derive(Debug, Clone, Default)]
pub struct Files(pub Vec<PathBuf>);

impl TraceSource for Files {
    fn list(&self) -> Vec<TraceRef> {
        let mut v: Vec<TraceRef> = self.0.iter().map(|p| TraceRef::file(p)).collect();
        v.sort();
        v
    }

    fn open(&self, trace: &TraceRef) -> io::Result<Box<dyn Read + '_>> {
        open_file(trace)
    }
}

/// A Claude Code projects directory (`~/.claude/projects`), sub-agent traces included.
#[derive(Debug, Clone)]
pub struct ClaudeProjects(pub PathBuf);

impl TraceSource for ClaudeProjects {
    fn list(&self) -> Vec<TraceRef> {
        crate::claude::discover(&self.0).iter().map(|p| TraceRef::file(p)).collect()
    }

    fn open(&self, trace: &TraceRef) -> io::Result<Box<dyn Read + '_>> {
        open_file(trace)
    }
}

/// A Codex sessions directory (`$CODEX_HOME/sessions`). Compressed rollouts are not listed.
#[derive(Debug, Clone)]
pub struct CodexSessions(pub PathBuf);

impl TraceSource for CodexSessions {
    fn list(&self) -> Vec<TraceRef> {
        crate::codex::discover(&self.0).0.iter().map(|p| TraceRef::file(p)).collect()
    }

    fn open(&self, trace: &TraceRef) -> io::Result<Box<dyn Read + '_>> {
        open_file(trace)
    }
}

/// Traces held in memory: what a remote source hands over once fetched, and what tests build.
#[derive(Debug, Clone, Default)]
pub struct Memory(pub Vec<(String, Vec<u8>)>);

impl TraceSource for Memory {
    fn list(&self) -> Vec<TraceRef> {
        let mut v: Vec<TraceRef> =
            self.0.iter().map(|(id, _)| TraceRef { id: id.clone(), path: None, subagent: false }).collect();
        v.sort();
        v
    }

    fn open(&self, trace: &TraceRef) -> io::Result<Box<dyn Read + '_>> {
        let (_, bytes) = self
            .0
            .iter()
            .find(|(id, _)| *id == trace.id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, trace.id.clone()))?;
        Ok(Box::new(bytes.as_slice()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locate::AsRecorded;
    use adage_core::CommandTable;
    use serde_json::json;

    fn claude(cwd: &str) -> Vec<u8> {
        [
            json!({"type": "user", "sessionId": "c1", "cwd": cwd, "timestamp": "2026-09-01T10:00:00Z",
                   "message": {"role": "user", "content": "run the tests"}}),
            json!({"type": "assistant", "sessionId": "c1", "cwd": cwd,
                   "message": {"id": "m1", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "make test"}}]}}),
            json!({"type": "user", "sessionId": "c1", "cwd": cwd,
                   "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok", "is_error": false}]}}),
        ]
        .iter()
        .map(|l| format!("{l}\n"))
        .collect::<String>()
        .into_bytes()
    }

    fn codex(cwd: &str) -> Vec<u8> {
        [
            json!({"timestamp": "2026-09-01T10:00:00Z", "type": "session_meta", "payload": {"id": "x1", "cwd": cwd, "timestamp": "2026-09-01T10:00:00Z"}}),
            json!({"timestamp": "2026-09-01T10:00:01Z", "type": "event_msg", "payload": {"type": "user_message", "message": "run the tests"}}),
            json!({"timestamp": "2026-09-01T10:00:02Z", "type": "response_item", "payload": {"type": "function_call", "name": "exec_command", "call_id": "k1", "arguments": "{\"cmd\":\"make test\"}"}}),
            json!({"timestamp": "2026-09-01T10:00:03Z", "type": "response_item", "payload": {"type": "function_call_output", "call_id": "k1", "output": "Process exited with code 0\nOutput:\nok"}}),
        ]
        .iter()
        .map(|l| format!("{l}\n"))
        .collect::<String>()
        .into_bytes()
    }

    fn opencode(cwd: &str) -> Vec<u8> {
        serde_json::to_vec_pretty(&json!({
            "info": {"id": "ses_1", "directory": cwd, "time": {"created": 1_788_000_000_000i64}},
            "messages": [
                {"info": {"role": "user"}, "parts": [{"type": "text", "text": "run the tests"}]},
                {"info": {"role": "assistant", "modelID": "m"}, "parts": [
                    {"type": "tool", "tool": "bash", "state": {"status": "completed", "input": {"command": "make test"}, "output": "ok", "metadata": {"exit": 0}}}
                ]}
            ]
        }))
        .unwrap()
    }

    /// Every format decodes from bytes alone, with no file and no checkout on this machine: what a
    /// remote trace store needs.
    #[test]
    fn every_format_decodes_from_memory_without_the_filesystem() {
        let cwd = "/nowhere/on/this/machine/acme";
        let source = Memory(vec![
            ("a-claude".into(), claude(cwd)),
            ("b-codex".into(), codex(cwd)),
            ("c-opencode".into(), opencode(cwd)),
            ("d-junk".into(), b"not a trace".to_vec()),
        ]);
        let table = CommandTable::load();
        let cx = Decode { table: &table, locator: &AsRecorded, repo_override: None, subagent: false };
        let mut got = Vec::new();
        ingest(&source, &cx, |t, s| got.push((t.id.clone(), s)));
        let harnesses: Vec<(&str, Option<&str>)> =
            got.iter().map(|(id, s)| (id.as_str(), s.as_ref().map(|s| s.harness.as_str()))).collect();
        assert_eq!(
            harnesses,
            vec![
                ("a-claude", Some("claude-code")),
                ("b-codex", Some("codex")),
                ("c-opencode", Some("opencode")),
                ("d-junk", None),
            ]
        );
        for (id, s) in got.iter().filter_map(|(id, s)| s.as_ref().map(|s| (id, s))) {
            assert_eq!(s.repo, "acme", "{id}: keyed from the trace alone");
            assert_eq!(s.root, PathBuf::from(cwd), "{id}");
            assert_eq!(s.prompt, "run the tests", "{id}");
            assert!(s.steps.iter().any(|st| st.call.input["command"] == "make test"), "{id}: {:?}", s.steps);
        }
    }

    #[test]
    fn an_explicit_repository_key_wins_over_the_recorded_directory() {
        let source = Memory(vec![("t".into(), claude("/x/y"))]);
        let table = CommandTable::load();
        let cx =
            Decode { table: &table, locator: &AsRecorded, repo_override: Some("github.com/acme/app"), subagent: false };
        let s = ingest_one(&source, &source.list()[0], &cx).unwrap();
        assert_eq!(s.repo, "github.com/acme/app");
    }
}
