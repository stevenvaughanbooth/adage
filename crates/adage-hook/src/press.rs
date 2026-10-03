//! The press (architecture §3.12): a token that lets a person's decision, made from the Claude
//! Code mod (an answer in the engine's own dialog, or a focused button), count as a person's, and
//! that a model steered into running a person-only verb cannot obtain.
//!
//! It is not a boundary against an agent that runs as the same user with a shell (nothing in
//! Adage is). It is a record that a person, not the model, decided (`via: press`), and a guard
//! against steering: the model can only call tools, and a Bash command it is talked into running
//! gets no token, because the only window in which one is issued has closed by then, and a start
//! it writes for a new session id lands while its own session is mid-turn ([`Record::contested`]).
//!
//! **Open.** The command hooks' `SessionStart` opens the window, and only for a payload that names
//! the session's own transcript (`transcript_path` ending `<session_id>.jsonl`, as every start
//! the engine sends does). A session whose window opened while another Claude Code session on
//! this machine was mid-turn ([`turn_path`]: from its prompt or tool call to its `Stop`, an hour
//! at most since its last) is *contested* for good: every verb refuses its press. A start written
//! from a Bash call is always written mid-turn, by the session running that call.
//!
//! **Issue.** `adage hook claude Press` (payload `{ session_id, cwd, source }`), asked by the mod
//! right after the command hooks' `SessionStart`, answers `{ "press": <64 hex> }` (32 random
//! bytes) when the session's window is open and nothing was issued for this start: a
//! `startup`/`clear` start opens the window once per session id, a `resume` reopens it once more
//! (and replaces the hash), a `compact` never does. The event runs before the kill switches, so a
//! repository where Adage is off can still be asked to turn it back on.
//!
//! **Close.** The window is a file, `sessions/<id>.press-open`, removed by the first agent-side
//! invocation for the session ([`CLOSERS`]) before any fast path or kill switch: one `unlink`
//! that finds nothing on every later event. A `Press` after that answers nothing.
//!
//! **Hold and spend.** The binary keeps only the token's SHA-256, in `sessions/<id>.press`
//! ([`Record`], the session state's locked two-slot format). A verb that accepts a press reads `{
//! press, session }` on stdin and calls [`spend`]: the hash must match (compared in constant
//! time), the token must be under 24 hours old, the session must not have ended, the verb must run
//! in the session's repository, and the session must not be contested. A
//! spent token is replaced by the next one in the chain, which the verb prints; the old one is
//! refused from then on. `SessionEnd` clears the hash.
//!
//! **Tripwire.** A Bash call that names `hook claude Press` (quotes and backslashes ignored, so
//! `"Pr"ess` names it too), or carries a press on its stdin heredoc, is recorded (`tripwire`) and
//! the person is warned once per session, by the command hook (the mod's own asks for the same
//! call neither warn nor record: their answer is never shown). It is never refused: advice, not a
//! boundary. A press read from a file, or a name the shell builds, goes unseen.
//!
//! Every spool record here is a fact, never the token: `press_issued`, `press_spent`, `tripwire`.
//! Each carries `duration_us` like every other record: the hook's own time for the two a hook
//! writes, and the spend's own time for `press_spent`, which a person-only verb writes.

use crate::answer::{answer, ModAnswer};
use crate::harness::{self, Envelope, EventKind, Harness};
use crate::record::{Record as SpoolRecord, RecordKind};
use adage_core::repo;
use adage_store::paths::sanitise;
use adage_store::{state, Paths};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The native event that asks for a token. Claude Code only: the mod is the only caller.
pub const EVENT: &str = "Press";

/// Whether an event of this kind is the agent acting, which closes the window. `SessionStart`,
/// `Press`, `SessionEnd` and the mod's read-only asks (`Standing`, `SkillPrompt`) are not.
const fn closes(kind: EventKind) -> bool {
    match kind {
        EventKind::Prompt
        | EventKind::PreToolUse
        | EventKind::ToolUse
        | EventKind::ToolBatch
        | EventKind::Failure
        | EventKind::Stop
        | EventKind::TurnStep => true,
        EventKind::SessionStart
        | EventKind::SessionEnd
        | EventKind::Press
        | EventKind::Standing
        | EventKind::SkillPrompt => false,
    }
}

/// The native names of the invocations that close the window ([`closes`]), in the order of the
/// event table of Claude Code, the one harness that presses ([`presses`]).
const fn closers<const N: usize>() -> [&'static str; N] {
    let table = Harness::ClaudeCode.events();
    let mut out = [""; N];
    let (mut i, mut n) = (0, 0);
    while i < table.len() {
        if closes(table[i].1) {
            assert!(n < N, "more closers than N");
            out[n] = table[i].0;
            n += 1;
        }
        i += 1;
    }
    assert!(n == N, "fewer closers than N");
    out
}

/// The invocations that close the window: anything the agent does (a prompt, a tool call about
/// to run or done, a `Stop`, a step), from Claude Code's event table
/// ([`adage_trace::Harness::events`]).
pub const CLOSERS: &[&str] = &closers::<7>();

/// How long a token is honoured after it was issued.
pub const MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;

/// How long a session counts as mid-turn after its last prompt or tool call with no `Stop`: a
/// session that died mid-turn (no `Stop`, no `SessionEnd`) stops contesting new starts after this.
pub const TURN_MS: u64 = 60 * 60 * 1000;

/// The press state of one session. The token itself is never stored, only its SHA-256.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// SHA-256 (hex) of the token that is valid now; `None` before the first issue and after
    /// `SessionEnd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Which link of the chain the valid token is: 0 when issued, one more per spend.
    #[serde(default)]
    pub chain: u32,
    /// When the valid token was issued (ms since the epoch).
    #[serde(default)]
    pub since: u64,
    /// When this session's window was first opened (ms).
    #[serde(default)]
    pub opened: u64,
    /// When the agent first acted in this session (ms), closing the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<u64>,
    /// A token was issued for the current start.
    #[serde(default)]
    pub issued: bool,
    /// The one reissue a `resume` allows has been used.
    #[serde(default)]
    pub resumed: bool,
    /// `SessionEnd` was seen.
    #[serde(default)]
    pub ended: bool,
    /// The repository root the session runs in, canonical.
    #[serde(default)]
    pub root: String,
    /// The person was warned once about a press seen in a Bash command.
    #[serde(default)]
    pub warned: bool,
    /// The window opened while another session on this machine was mid-turn: as a start written
    /// from that session's Bash call is. Every verb refuses this session's press, for good.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub contested: bool,
}

/// `sessions/<id>.press`: the session's [`Record`].
pub fn record_path(paths: &Paths, session: &str) -> PathBuf {
    paths.sessions().join(format!("{}.press", sanitise(session)))
}

/// `sessions/<id>.press-open`: present while the window is open.
pub fn window_path(paths: &Paths, session: &str) -> PathBuf {
    paths.sessions().join(format!("{}.press-open", sanitise(session)))
}

/// `sessions/<id>.press-turn`: present while the session is mid-turn (made or touched by a prompt
/// or a tool call, removed by `Stop` and `SessionEnd`).
pub fn turn_path(paths: &Paths, session: &str) -> PathBuf {
    paths.sessions().join(format!("{}.press-turn", sanitise(session)))
}

/// SHA-256 of the raw token, as hex.
pub fn hash(token: &str) -> String {
    adage_pack::digest::sha256_hex(token.as_bytes())
}

/// Equal strings, in time that depends only on their lengths.
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 32 random bytes from the operating system, as 64 hex characters.
fn mint() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The repository root a working directory belongs to, canonical where it can be: the session's
/// and a verb's must compare equal whichever way each reached it (a symlink, a trailing dot).
pub fn root_of(cwd: &Path) -> String {
    let start = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let root = repo::find_root(&start).unwrap_or(start);
    root.canonicalize().unwrap_or(root).to_string_lossy().into_owned()
}

fn now() -> u64 {
    adage_core::time::now_ms()
}

/// Append one press record to the session's spool with the time Adage spent on it since
/// `started` (`duration_us`); a failure loses the record, never the event.
fn note(paths: &Paths, session: &str, rec: SpoolRecord, started: Instant) {
    rec.timed(started).append_or_log(paths, session, "claude", EVENT);
}

/// What [`gate`] decided for one invocation.
#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    /// Not the press's business: the pipeline carries on.
    Pass,
    /// Answer this (possibly nothing) and stop: a `Press`, or a tripwire's warning.
    Answer(Option<String>),
}

/// The press's part of one invocation, decided from its event alone ([`act_of`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    /// Not the press's business.
    Pass,
    /// A `Press`: a token, if the window is open.
    Issue,
    /// A session start: the window opens.
    Open,
    /// The session ended: every token is refused from now on.
    End,
    /// The agent acted (`kind`): the window closes; `tripwire` when the act is a command hook's
    /// Bash call, which the tripwire reads.
    Close { kind: EventKind, tripwire: bool },
}

/// Whether a harness issues presses at all: one whose event table has a `Press` (the Claude Code
/// mod is the only caller). Every other harness's invocations are not the press's business.
fn presses(h: Harness) -> bool {
    h.event_kind(EVENT) == Some(EventKind::Press)
}

/// What the press does at one invocation, from its harness's event table.
pub(crate) fn act_of(harness: &str, event: &str, p: &Value) -> Act {
    let Some(h) = harness::adapter(harness).map(|a| a.harness()).filter(|h| presses(*h)) else { return Act::Pass };
    let Some(kind) = h.event_kind(event) else { return Act::Pass };
    match kind {
        EventKind::Press => Act::Issue,
        EventKind::SessionStart => Act::Open,
        EventKind::SessionEnd => Act::End,
        // The mod's asks (a dry run, a verdict) come before the command hook for the same call;
        // their answer is never shown, so only the command hook warns (and records).
        k if !closes(k) => Act::Pass,
        EventKind::PreToolUse => Act::Close { kind, tripwire: !harness::claude::is_mods(event, p) },
        _ => Act::Close { kind, tripwire: false },
    }
}

/// The press's part of every Claude Code invocation, before the kill switches and the fast path:
/// `at` is the payload's session and working directory. `off` is true when a kill switch is set:
/// only a `Press` may answer then. `started` is when the hook began (its records' `duration_us`).
pub fn gate(harness: &str, event: &str, p: &Value, at: &Envelope, paths: &Paths, off: bool, started: Instant) -> Gate {
    let (session, cwd) = (at.session.as_str(), at.cwd.as_path());
    match act_of(harness, event, p) {
        Act::Pass => Gate::Pass,
        Act::Issue => Gate::Answer(issue(p, session, paths, started)),
        Act::Open => {
            open(p, session, cwd, paths);
            Gate::Pass
        }
        Act::End => {
            end(session, paths);
            Gate::Pass
        }
        Act::Close { kind, tripwire: warns } => {
            close(session, paths);
            turn(kind, session, paths);
            if warns && !off {
                return tripwire(p, session, paths, started).map_or(Gate::Pass, |w| Gate::Answer(Some(w)));
            }
            Gate::Pass
        }
    }
}

/// [`gate`] for a payload not yet parsed: the kill switch `ADAGE_DISABLE` returns before the
/// pipeline parses anything, and the press still runs there.
pub fn before_kill_switch(harness: &str, event: &str, buf: &str, paths: &Paths, started: Instant) -> Option<String> {
    // The press's business is decided by the event alone: only a payload it acts on is parsed.
    if act_of(harness, event, &Value::Null) == Act::Pass {
        return None;
    }
    let p: Value = serde_json::from_str(buf).ok()?;
    let envelope = harness::adapter(harness)?.envelope(&p)?;
    match gate(harness, event, &p, &envelope, paths, true, started) {
        Gate::Answer(out) => out,
        Gate::Pass => None,
    }
}

/// A start opens the window: `startup` and `clear` (and a start that names no source) once per
/// session id, `resume` once more, `compact` never (it fires mid-turn).
fn open(p: &Value, session: &str, cwd: &Path, paths: &Paths) {
    let source = p.get("source").and_then(Value::as_str).unwrap_or("startup");
    if source == "compact" {
        return;
    }
    if !names_own_transcript(p, session) {
        return;
    }
    let path = record_path(paths, session);
    let fresh = !path.exists();
    if !fresh && source != "resume" {
        return;
    }
    let root = root_of(cwd);
    let contested = busy_elsewhere(paths, session);
    let opened = state::update::<Record, _>(&path, |r| {
        if fresh {
            *r = Record { opened: now(), root, contested, ..Record::default() };
            if source == "resume" {
                r.resumed = true;
            }
            true
        } else if !r.resumed && !r.ended {
            r.resumed = true;
            r.issued = false;
            r.contested |= contested;
            true
        } else {
            false
        }
    });
    if opened.unwrap_or(false) {
        let _ = std::fs::write(window_path(paths, session), b"");
    }
}

/// The start the engine sends names the session's transcript, `<session_id>.jsonl` (the file need
/// not exist yet). A payload without one opens no window.
fn names_own_transcript(p: &Value, session: &str) -> bool {
    p.get("transcript_path")
        .and_then(Value::as_str)
        .map(Path::new)
        .is_some_and(|t| t.extension().is_some_and(|x| x == "jsonl") && t.file_stem().is_some_and(|s| s == session))
}

/// Another session on this machine is mid-turn now: its [`turn_path`] exists and was touched less
/// than [`TURN_MS`] ago.
fn busy_elsewhere(paths: &Paths, session: &str) -> bool {
    let mine = turn_path(paths, session);
    let Ok(dir) = std::fs::read_dir(paths.sessions()) else { return false };
    let now = std::time::SystemTime::now();
    dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "press-turn") && *p != mine).any(
        |p| {
            std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .is_ok_and(|t| now.duration_since(t).map_or(true, |d| d.as_millis() <= u128::from(TURN_MS)))
        },
    )
}

/// A prompt or a tool call marks the session mid-turn (touching the marker, so a long turn stays
/// marked); `Stop` clears it. A failure loses the mark, never the event.
fn turn(kind: EventKind, session: &str, paths: &Paths) {
    let path = turn_path(paths, session);
    match kind {
        EventKind::Prompt | EventKind::PreToolUse => {
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
                .and_then(|f| f.set_modified(std::time::SystemTime::now()));
        }
        EventKind::Stop => {
            let _ = std::fs::remove_file(path);
        }
        _ => {}
    }
}

/// The agent acted: the window closes, and the record notes when (once).
fn close(session: &str, paths: &Paths) {
    if std::fs::remove_file(window_path(paths, session)).is_ok() {
        let at = now();
        let _ = state::update::<Record, _>(&record_path(paths, session), |r| {
            r.closed.get_or_insert(at);
        });
    }
}

/// `SessionEnd`: every token of the session is refused from now on.
fn end(session: &str, paths: &Paths) {
    let _ = std::fs::remove_file(window_path(paths, session));
    let _ = std::fs::remove_file(turn_path(paths, session));
    let path = record_path(paths, session);
    if path.exists() {
        let _ = state::update::<Record, _>(&path, |r| {
            r.hash = None;
            r.ended = true;
        });
    }
}

/// `Press`: a token while the window is open and none was issued for this start.
fn issue(p: &Value, session: &str, paths: &Paths, started: Instant) -> Option<String> {
    let source = p.get("source").and_then(Value::as_str).unwrap_or("startup");
    if source == "compact" || !window_path(paths, session).exists() {
        return None;
    }
    let token = mint()?;
    let h = hash(&token);
    let issued = state::update::<Record, _>(&record_path(paths, session), |r| {
        if r.issued || r.ended {
            return None;
        }
        r.issued = true;
        r.hash = Some(h);
        r.chain = 0;
        r.since = now();
        Some(r.chain)
    })
    .ok()
    .flatten()?;
    note(
        paths,
        session,
        SpoolRecord::new(RecordKind::PressIssued, EVENT).with("source", source).with("chain", issued),
        started,
    );
    Some(answer(&ModAnswer::Press { press: token }))
}

/// A Bash call naming the press hook, or carrying a press on its stdin: recorded every time, and
/// the person warned once per session. Never a refusal, never a rewrite.
fn tripwire(p: &Value, session: &str, paths: &Paths, started: Instant) -> Option<String> {
    let command = p.get("tool_input")?.get("command")?.as_str()?;
    if !names_press(command) {
        return None;
    }
    let warn = state::update::<Record, _>(&record_path(paths, session), |r| !std::mem::replace(&mut r.warned, true))
        .unwrap_or(true);
    note(paths, session, SpoolRecord::new(RecordKind::Tripwire, "PreToolUse").with("warned", warn), started);
    let text = "adage: a shell command in this session names Adage's press. A press is a person's answer in the session, never a command; the command will not be honoured as one.";
    warn.then(|| json!({ "systemMessage": text }).to_string())
}

/// The command asks for a press (`hook claude Press`, read with the shell's quotes and backslashes
/// taken out, so `"Pr"ess`, `P\ress` and `'Press'<p.json` name it too) or hands one to a verb
/// (`"press"` on its stdin: a heredoc, an `echo` or a `printf`). A name the shell builds from a
/// variable or a substitution is not read.
pub fn names_press(command: &str) -> bool {
    let bare: String = command.chars().filter(|c| !matches!(c, '"' | '\'' | '\\')).collect();
    let words: Vec<&str> = bare.split_whitespace().collect();
    words.windows(3).any(|w| {
        w[0] == "hook" && w[1] == "claude" && w[2].split(|c: char| !c.is_ascii_alphanumeric()).next() == Some(EVENT)
    }) || command.contains("\"press\"")
        || command.contains("\\\"press\\\"")
}

/// Why a press was not honoured. The verb behaves exactly as without one; the reason is for tests
/// and the notes, never shown in place of the person-only message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// No press state for that session id, or no token issued.
    Unknown,
    /// The token does not match the valid one: wrong, or spent.
    Mismatch,
    /// Issued more than 24 hours ago.
    Stale,
    /// The session ended.
    Ended,
    /// The verb runs in another repository than the session.
    OtherRepository,
    /// Another session on this machine was mid-turn when this one started
    /// ([`Record::contested`]).
    NotLive,
    /// No token could be minted for the next link.
    Unavailable,
}

impl Refused {
    pub fn as_str(self) -> &'static str {
        match self {
            Refused::Unknown => "no press was issued for that session",
            Refused::Mismatch => "the press does not match (wrong, or already spent)",
            Refused::Stale => "the press is more than 24 hours old",
            Refused::Ended => "the session has ended",
            Refused::OtherRepository => "the press belongs to a session in another repository",
            Refused::NotLive => "the press belongs to a session started while another session was at work",
            Refused::Unavailable => "no next press could be made",
        }
    }
}

/// Spend `token` for `session` in the repository at `root` (as [`root_of`] gives it): on success
/// the next token of the chain, which replaces it. `cmd` names the verb in the spool record.
pub fn spend(paths: &Paths, token: &str, session: &str, root: &str, cmd: &str) -> Result<String, Refused> {
    let started = Instant::now();
    let path = record_path(paths, session);
    if session.is_empty() || !path.exists() {
        return Err(Refused::Unknown);
    }
    let current = state::read::<Record>(&path).map_err(|_| Refused::Unknown)?;
    if current.root != root {
        return Err(Refused::OtherRepository);
    }
    if current.contested {
        return Err(Refused::NotLive);
    }
    let next = mint().ok_or(Refused::Unavailable)?;
    let presented = hash(token);
    let at = now();
    let spent = state::update::<Record, _>(&path, |r| {
        if r.ended {
            return Err(Refused::Ended);
        }
        let Some(h) = r.hash.as_deref() else { return Err(Refused::Unknown) };
        if !same(h, &presented) {
            return Err(Refused::Mismatch);
        }
        if at.saturating_sub(r.since) > MAX_AGE_MS {
            return Err(Refused::Stale);
        }
        r.chain += 1;
        r.hash = Some(hash(&next));
        r.since = at;
        Ok(r.chain)
    })
    .map_err(|_| Refused::Unknown)??;
    note(
        paths,
        session,
        SpoolRecord::at(RecordKind::PressSpent, EVENT, at).with("chain", spent).with("verb", cmd),
        started,
    );
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_store::spool;

    fn at(session: &str, cwd: &Path) -> Envelope {
        Envelope { session: session.into(), cwd: cwd.to_path_buf() }
    }

    fn world() -> (tempfile::TempDir, Paths, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path().join("home"));
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        (dir, paths, repo)
    }

    /// A start as the engine sends it: it names the session's transcript.
    fn payload(session: &str, cwd: &Path, source: &str) -> Value {
        let transcript = format!("/home/dev/.claude/projects/p/{session}.jsonl");
        json!({ "session_id": session, "cwd": cwd, "source": source, "transcript_path": transcript })
    }

    fn start(paths: &Paths, repo: &Path, session: &str, source: &str) {
        assert_eq!(
            gate(
                "claude",
                "SessionStart",
                &payload(session, repo, source),
                &at(session, repo),
                paths,
                false,
                Instant::now()
            ),
            Gate::Pass
        );
    }

    fn press(paths: &Paths, repo: &Path, session: &str, source: &str) -> Option<String> {
        match gate("claude", EVENT, &payload(session, repo, source), &at(session, repo), paths, false, Instant::now()) {
            Gate::Answer(Some(out)) => {
                Some(serde_json::from_str::<Value>(&out).unwrap()["press"].as_str().unwrap().to_string())
            }
            Gate::Answer(None) => None,
            Gate::Pass => panic!("a press always answers"),
        }
    }

    fn act(paths: &Paths, repo: &Path, session: &str, event: &str) {
        let p = json!({ "session_id": session, "cwd": repo, "tool_name": "Bash", "tool_input": { "command": "ls" } });
        assert_eq!(gate("claude", event, &p, &at(session, repo), paths, false, Instant::now()), Gate::Pass);
    }

    /// T-A7-07: the hash is SHA-256 of the raw token, compared in constant time; the chain moves
    /// one link per spend and an earlier link is refused; a token over 24 hours old is refused.
    #[test]
    fn hashing_chaining_and_age() {
        assert_eq!(hash("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(same("abcd", "abcd"));
        assert!(!same("abcd", "abce"));
        assert!(!same("abcd", "abc"));

        let (_d, paths, repo) = world();
        start(&paths, &repo, "s1", "startup");
        let t0 = press(&paths, &repo, "s1", "startup").unwrap();
        assert_eq!(t0.len(), 64);
        assert!(t0.bytes().all(|b| b.is_ascii_hexdigit()));
        let root = root_of(&repo);
        let rec = state::read::<Record>(&record_path(&paths, "s1")).unwrap();
        assert_eq!(rec.hash.as_deref(), Some(hash(&t0).as_str()));
        assert_eq!(rec.chain, 0);
        let t1 = spend(&paths, &t0, "s1", &root, "review approve").unwrap();
        assert_eq!(state::read::<Record>(&record_path(&paths, "s1")).unwrap().chain, 1);
        assert_eq!(spend(&paths, &t0, "s1", &root, "review approve"), Err(Refused::Mismatch));
        let t2 = spend(&paths, &t1, "s1", &root, "review approve").unwrap();
        assert_eq!(spend(&paths, &t1, "s1", &root, "review approve"), Err(Refused::Mismatch));
        // The last character changed.
        let mut wrong = t2.clone();
        let last = if wrong.ends_with('0') { '1' } else { '0' };
        wrong.pop();
        wrong.push(last);
        assert_eq!(spend(&paths, &wrong, "s1", &root, "review approve"), Err(Refused::Mismatch));
        // 25 hours later.
        state::update::<Record, _>(&record_path(&paths, "s1"), |r| r.since -= 25 * 60 * 60 * 1000).unwrap();
        assert_eq!(spend(&paths, &t2, "s1", &root, "review approve"), Err(Refused::Stale));
        // Never the token, on disk anywhere.
        for e in std::fs::read_dir(paths.sessions()).unwrap().flatten() {
            let bytes = std::fs::read(e.path()).unwrap();
            for t in [&t0, &t1, &t2] {
                assert!(!String::from_utf8_lossy(&bytes).contains(t.as_str()), "{} holds a token", e.path().display());
            }
        }
    }

    /// T-A7-01 and T-A7-02 at the unit level: once per start, and the first agent-side invocation
    /// of any kind closes the window.
    #[test]
    fn issued_once_and_closed_by_the_first_act() {
        for closer in CLOSERS {
            let (_d, paths, repo) = world();
            start(&paths, &repo, "s", "startup");
            assert!(window_path(&paths, "s").exists());
            act(&paths, &repo, "s", closer);
            assert!(!window_path(&paths, "s").exists(), "{closer} closes the window");
            assert_eq!(press(&paths, &repo, "s", "startup"), None, "after {closer}");
        }
        let (_d, paths, repo) = world();
        start(&paths, &repo, "s", "startup");
        assert!(press(&paths, &repo, "s", "startup").is_some());
        assert_eq!(press(&paths, &repo, "s", "startup"), None);
        // A press for a session that never started is nothing.
        assert_eq!(press(&paths, &repo, "never", "startup"), None);
    }

    /// T-A7-03: compact never opens; resume reissues once and replaces the hash; a second start
    /// for a session id that acted opens nothing.
    #[test]
    fn sources() {
        let (_d, paths, repo) = world();
        start(&paths, &repo, "s", "startup");
        let first = press(&paths, &repo, "s", "startup").unwrap();
        act(&paths, &repo, "s", "UserPromptSubmit");
        start(&paths, &repo, "s", "compact");
        assert_eq!(press(&paths, &repo, "s", "compact"), None);
        assert_eq!(press(&paths, &repo, "s", "startup"), None);
        start(&paths, &repo, "s", "startup");
        start(&paths, &repo, "s", "clear");
        assert_eq!(press(&paths, &repo, "s", "startup"), None, "a start written again for a session that acted");
        start(&paths, &repo, "s", "resume");
        let resumed = press(&paths, &repo, "s", "resume").unwrap();
        assert_ne!(resumed, first);
        let root = root_of(&repo);
        assert_eq!(spend(&paths, &first, "s", &root, "on"), Err(Refused::Mismatch));
        act(&paths, &repo, "s", "PreToolUse");
        start(&paths, &repo, "s", "resume");
        assert_eq!(press(&paths, &repo, "s", "resume"), None, "a second resume");
        // A new session id (`clear`) issues.
        start(&paths, &repo, "s2", "clear");
        assert!(press(&paths, &repo, "s2", "clear").is_some());
        // SessionEnd clears the hash.
        assert!(spend(&paths, &resumed, "s", &root, "on").is_ok());
        let p = json!({ "session_id": "s", "cwd": repo });
        assert_eq!(gate("claude", "SessionEnd", &p, &at("s", &repo), &paths, false, Instant::now()), Gate::Pass);
        assert_eq!(state::read::<Record>(&record_path(&paths, "s")).unwrap().hash, None);
    }

    /// T-X-13 (L2, the hand-written start), and review M4: a start written while another session
    /// on this machine is mid-turn (as a Bash call's always is) is contested for good, in this
    /// repository or a fresh one, whatever acts in it later; a start after that session's `Stop`,
    /// or after its mark went stale, is not.
    #[test]
    fn a_start_written_by_hand_is_not_the_live_session() {
        let (_d, paths, repo) = world();
        start(&paths, &repo, "real", "startup");
        let real = press(&paths, &repo, "real", "startup").unwrap();
        act(&paths, &repo, "real", "UserPromptSubmit");
        assert!(turn_path(&paths, "real").exists(), "mid-turn");
        start(&paths, &repo, "forged", "startup");
        let forged = press(&paths, &repo, "forged", "startup").unwrap();
        let root = root_of(&repo);
        assert_eq!(spend(&paths, &forged, "forged", &root, "review approve"), Err(Refused::NotLive));
        // Acting in the forged session (a hand-written prompt) does not make it live.
        act(&paths, &repo, "forged", "UserPromptSubmit");
        assert_eq!(spend(&paths, &forged, "forged", &root, "review approve"), Err(Refused::NotLive));
        assert!(spend(&paths, &real, "real", &root, "review approve").is_ok());
        // A fresh `git init` elsewhere, for a machine-wide verb: refused the same way.
        let other = repo.parent().unwrap().join("x");
        std::fs::create_dir_all(other.join(".git")).unwrap();
        start(&paths, &other, "elsewhere", "startup");
        let t = press(&paths, &other, "elsewhere", "startup").unwrap();
        assert_eq!(spend(&paths, &t, "elsewhere", &root_of(&other), "delegate on"), Err(Refused::NotLive));
        // Another repository's verb refuses the real token too.
        assert_eq!(spend(&paths, "00", "real", &root_of(&other), "on"), Err(Refused::OtherRepository));
        // The real session's turn ends (and the forged ones'): a start in another terminal now is
        // the person's, and its press is honoured.
        for s in ["real", "forged"] {
            act(&paths, &repo, s, "Stop");
        }
        assert!(!turn_path(&paths, "real").exists());
        start(&paths, &other, "second", "startup");
        let t = press(&paths, &other, "second", "startup").unwrap();
        assert!(spend(&paths, &t, "second", &root_of(&other), "delegate on").is_ok());
        // A session that died mid-turn stops contesting an hour after its last act.
        act(&paths, &repo, "real", "PreToolUse");
        let stale = std::time::SystemTime::now() - std::time::Duration::from_millis(TURN_MS + 60_000);
        std::fs::File::options().write(true).open(turn_path(&paths, "real")).unwrap().set_modified(stale).unwrap();
        start(&paths, &repo, "third", "startup");
        let t = press(&paths, &repo, "third", "startup").unwrap();
        assert!(spend(&paths, &t, "third", &root, "on").is_ok());
        // SessionEnd removes the mark.
        act(&paths, &repo, "real", "PreToolUse");
        let p = json!({ "session_id": "real", "cwd": repo });
        assert_eq!(gate("claude", "SessionEnd", &p, &at("real", &repo), &paths, false, Instant::now()), Gate::Pass);
        assert!(!turn_path(&paths, "real").exists());
        // Before anything acts, a lone session's press is honoured.
        let (_d2, paths2, repo2) = world();
        start(&paths2, &repo2, "only", "startup");
        let t = press(&paths2, &repo2, "only", "startup").unwrap();
        assert!(spend(&paths2, &t, "only", &root_of(&repo2), "on").is_ok());
    }

    /// Review M4: a start that does not name the session's own transcript (as a payload written
    /// by hand from the review's example does not) opens no window.
    #[test]
    fn a_start_without_its_transcript_opens_nothing() {
        let (_d, paths, repo) = world();
        for p in [
            json!({ "session_id": "f", "cwd": repo, "source": "startup" }),
            json!({ "session_id": "f", "cwd": repo, "source": "startup", "transcript_path": "/t/other.jsonl" }),
            json!({ "session_id": "f", "cwd": repo, "source": "startup", "transcript_path": "/t/f.txt" }),
        ] {
            assert_eq!(gate("claude", "SessionStart", &p, &at("f", &repo), &paths, false, Instant::now()), Gate::Pass);
            assert!(!window_path(&paths, "f").exists(), "{p}");
            assert_eq!(press(&paths, &repo, "f", "startup"), None);
        }
        start(&paths, &repo, "f", "startup");
        assert!(press(&paths, &repo, "f", "startup").is_some());
    }

    /// T-A7-08 at the unit level: recorded every time, warned once, never a refusal.
    #[test]
    fn the_tripwire_warns_once() {
        assert!(names_press("adage hook claude Press < p.json"));
        assert!(names_press("adage review approve t r <<'EOF'\n{\"press\": \"ab\", \"session\": \"s\"}\nEOF"));
        assert!(names_press("echo '{\"press\":\"x\"}' | adage on"));
        // Review M4: the shell's quoting does not hide the name.
        for evasive in [
            "adage hook claude \"Pr\"ess < p.json",
            "adage hook claude P\\ress < p.json",
            "adage hook claude 'Press'<p.json",
            "adage 'hook' \"claude\" Pr''ess",
            "cat p.json | adage hook claude Press|tee out",
        ] {
            assert!(names_press(evasive), "{evasive}");
        }
        assert!(!names_press("adage hook claude PressRelease"));
        assert!(!names_press("adage hook claude PreToolUse < p.json"));
        assert!(!names_press("adage review approve t r"));
        assert!(!names_press("git commit -m 'press the button'"));
        let (_d, paths, repo) = world();
        let p = json!({ "session_id": "s", "cwd": repo, "tool_name": "Bash", "tool_input": { "command": "adage hook claude Press" } });
        let Gate::Answer(Some(w)) = gate("claude", "PreToolUse", &p, &at("s", &repo), &paths, false, Instant::now())
        else {
            panic!()
        };
        let v: Value = serde_json::from_str(&w).unwrap();
        assert!(v["systemMessage"].as_str().unwrap().contains("press"));
        assert!(v.get("hookSpecificOutput").is_none(), "never a deny or an updatedInput");
        assert_eq!(gate("claude", "PreToolUse", &p, &at("s", &repo), &paths, false, Instant::now()), Gate::Pass);
        let recs: Vec<Value> = spool::read_all(&paths.spool("s")).unwrap();
        assert_eq!(recs.iter().filter(|r| r["kind"] == "tripwire").count(), 2);
        // Review M3: the mod's asks for a call (a dry run, a verdict) neither warn nor record; the
        // command hook that follows them warns, once.
        let (_d2, paths2, _) = world();
        for phase in [json!({ "dry_run": true }), json!({ "verdict": { "engine": "same" } }), json!({ "via": "mod" })] {
            let mut m = p.clone();
            for (k, v) in phase.as_object().unwrap() {
                m[k] = v.clone();
            }
            assert_eq!(
                gate("claude", "PreToolUse", &m, &at("s", &repo), &paths2, false, Instant::now()),
                Gate::Pass,
                "{m}"
            );
        }
        assert!(spool::read_all::<Value>(&paths2.spool("s")).unwrap_or_default().is_empty());
        let Gate::Answer(Some(_)) = gate("claude", "PreToolUse", &p, &at("s", &repo), &paths2, false, Instant::now())
        else {
            panic!("the command hook warns")
        };
        // Not under a kill switch, and not for another harness.
        assert_eq!(gate("claude", "PreToolUse", &p, &at("s", &repo), &paths, true, Instant::now()), Gate::Pass);
        assert_eq!(gate("codex", EVENT, &p, &at("s", &repo), &paths, false, Instant::now()), Gate::Pass);
    }
}
