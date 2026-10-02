//! Trace formats: one decoder per harness history format, chosen by the trace's first bytes.
//!
//! A [`Format`] turns the bytes of one trace into the canonical [`Session`], whatever the bytes
//! came from ([`crate::source`]). Adding a harness's history means adding one `Format` here and
//! listing it in [`FORMATS`]; nothing past ingestion changes.

use crate::locate::{LocalCheckout, Locator};
use adage_core::CommandTable;
use adage_trace::Session;
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
    /// The harness, as [`Session::harness`] and a track's `gauge.harness` name it.
    fn harness(&self) -> &'static str;
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
    let cx = Decode { subagent: crate::claude::is_subagent(path), ..Decode::local(table, repo_override) };
    decode_trace(std::fs::File::open(path).ok()?, &cx)
}
