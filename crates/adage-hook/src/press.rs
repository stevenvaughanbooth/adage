//! The press (architecture §3.12): a token that lets a person's decision, made from the Claude
//! Code mod (an answer in the engine's own dialog, or a focused button), count as a person's, and
//! that a model steered into running a person-only verb cannot obtain.
//!
//! It is not a boundary against an agent that runs as the same user with a shell (nothing in
//! Adage is). It is a record that a person, not the model, decided (`via: press`), and a guard
//! against steering: the model can only call tools, and a Bash command it is talked into running
//! gets no token, because the only window in which one is issued has closed by then.
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
//! in the session's repository, and the session must be the repository's live one ([`live`]). A
//! spent token is replaced by the next one in the chain, which the verb prints; the old one is
//! refused from then on. `SessionEnd` clears the hash.
//!
//! **Tripwire.** A Bash call that names `hook claude Press`, or carries a press on its stdin, is
//! recorded (`tripwire`) and the person is warned once per session. It is never refused: advice,
//! not a boundary.
//!
//! Every spool record here is a fact, never the token: `press_issued`, `press_spent`, `tripwire`.

use crate::failopen::{self, Failure};
use crate::harness;
use adage_core::repo;
use adage_store::paths::sanitise;
use adage_store::{spool, state, Paths};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The native event that asks for a token. Claude Code only: the mod is the only caller.
pub const EVENT: &str = "Press";

/// The invocations that close the window: anything the agent does. `SessionStart`, `Press`,
/// `SessionEnd` and the mod's read-only asks (`Standing`, `SkillPrompt`) are not the agent acting.
pub const CLOSERS: &[&str] =
    &["UserPromptSubmit", "PreToolUse", "PostToolUse", "PostToolBatch", "PostToolUseFailure", "Stop", "TurnStep"];

/// How long a token is honoured after it was issued.
pub const MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;

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
}

/// `sessions/<id>.press`: the session's [`Record`].
pub fn record_path(paths: &Paths, session: &str) -> PathBuf {
    paths.sessions().join(format!("{}.press", sanitise(session)))
}

/// `sessions/<id>.press-open`: present while the window is open.
pub fn window_path(paths: &Paths, session: &str) -> PathBuf {
    paths.sessions().join(format!("{}.press-open", sanitise(session)))
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

/// Append one press record to the session's spool; a failure loses the record, never the event.
fn note(paths: &Paths, session: &str, rec: &Value) {
    if let Err(e) = spool::append(&paths.spool(session), rec) {
        failopen::record(&paths.root, "claude", EVENT, &Failure::io("spool", "appending to the session spool", &e));
    }
}

/// What [`gate`] decided for one invocation.
#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    /// Not the press's business: the pipeline carries on.
    Pass,
    /// Answer this (possibly nothing) and stop: a `Press`, or a tripwire's warning.
    Answer(Option<String>),
}

/// The press's part of every Claude Code invocation, before the kill switches and the fast path.
/// `off` is true when a kill switch is set: only a `Press` may answer then.
pub fn gate(harness: &str, event: &str, p: &Value, session: &str, cwd: &Path, paths: &Paths, off: bool) -> Gate {
    if harness::adapter(harness).map(|a| a.harness()) != Some(harness::Harness::ClaudeCode) {
        return Gate::Pass;
    }
    match event {
        EVENT => Gate::Answer(issue(p, session, paths)),
        "SessionStart" => {
            open(p, session, cwd, paths);
            Gate::Pass
        }
        "SessionEnd" => {
            end(session, paths);
            Gate::Pass
        }
        e if CLOSERS.contains(&e) => {
            close(session, paths);
            match (e, off) {
                ("PreToolUse", false) => tripwire(p, session, paths).map_or(Gate::Pass, |w| Gate::Answer(Some(w))),
                _ => Gate::Pass,
            }
        }
        _ => Gate::Pass,
    }
}

/// [`gate`] for a payload not yet parsed: the kill switch `ADAGE_DISABLE` returns before the
/// pipeline parses anything, and the press still runs there.
pub fn before_kill_switch(harness: &str, event: &str, buf: &str, paths: &Paths) -> Option<String> {
    if event != EVENT && event != "SessionStart" && event != "SessionEnd" && !CLOSERS.contains(&event) {
        return None;
    }
    let p: Value = serde_json::from_str(buf).ok()?;
    let envelope = harness::adapter(harness)?.envelope(&p)?;
    match gate(harness, event, &p, &envelope.session, &envelope.cwd, paths, true) {
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
    let path = record_path(paths, session);
    let fresh = !path.exists();
    if !fresh && source != "resume" {
        return;
    }
    let root = root_of(cwd);
    let opened = state::update::<Record, _>(&path, |r| {
        if fresh {
            *r = Record { opened: now(), root, ..Record::default() };
            if source == "resume" {
                r.resumed = true;
            }
            true
        } else if !r.resumed && !r.ended {
            r.resumed = true;
            r.issued = false;
            true
        } else {
            false
        }
    });
    if opened.unwrap_or(false) {
        let _ = std::fs::write(window_path(paths, session), b"");
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
    let path = record_path(paths, session);
    if path.exists() {
        let _ = state::update::<Record, _>(&path, |r| {
            r.hash = None;
            r.ended = true;
        });
    }
}

/// `Press`: a token while the window is open and none was issued for this start.
fn issue(p: &Value, session: &str, paths: &Paths) -> Option<String> {
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
        &json!({ "ts": now(), "event": EVENT, "kind": "press_issued", "source": source, "chain": issued }),
    );
    Some(json!({ "press": token }).to_string())
}

/// A Bash call naming the press hook, or carrying a press on its stdin: recorded every time, and
/// the person warned once per session. Never a refusal, never a rewrite.
fn tripwire(p: &Value, session: &str, paths: &Paths) -> Option<String> {
    let command = p.get("tool_input")?.get("command")?.as_str()?;
    if !names_press(command) {
        return None;
    }
    let warn = state::update::<Record, _>(&record_path(paths, session), |r| !std::mem::replace(&mut r.warned, true))
        .unwrap_or(true);
    note(paths, session, &json!({ "ts": now(), "event": "PreToolUse", "kind": "tripwire", "warned": warn }));
    let text = "adage: a shell command in this session names Adage's press. A press is a person's answer in the session, never a command; the command will not be honoured as one.";
    warn.then(|| json!({ "systemMessage": text }).to_string())
}

/// The command asks for a press (`hook claude Press`) or hands one to a verb (`"press"` on its
/// stdin: a heredoc, an `echo` or a `printf`).
pub fn names_press(command: &str) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    words.windows(3).any(|w| w[0] == "hook" && w[1] == "claude" && w[2].trim_matches(['"', '\'']) == EVENT)
        || command.contains("\"press\"")
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
    /// Another session in this repository was already at work when this one started, and this
    /// one has not acted yet ([`live`]).
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
            Refused::NotLive => "the press belongs to a session that is not this repository's live session",
            Refused::Unavailable => "no next press could be made",
        }
    }
}

/// Spend `token` for `session` in the repository at `root` (as [`root_of`] gives it): on success
/// the next token of the chain, which replaces it. `cmd` names the verb in the spool record.
pub fn spend(paths: &Paths, token: &str, session: &str, root: &str, cmd: &str) -> Result<String, Refused> {
    let path = record_path(paths, session);
    if session.is_empty() || !path.exists() {
        return Err(Refused::Unknown);
    }
    let current = state::read::<Record>(&path).map_err(|_| Refused::Unknown)?;
    if current.root != root {
        return Err(Refused::OtherRepository);
    }
    if !live(paths, session, &current) {
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
    note(paths, session, &json!({ "ts": at, "event": EVENT, "kind": "press_spent", "chain": spent, "verb": cmd }));
    Ok(next)
}

/// Whether `session` is a live session of its repository: one the agent has acted in, or, before
/// that, one that started while no other session there was at work. A start written by hand (a
/// `SessionStart` payload for a new id, from a Bash call in a running session) comes after that
/// running session acted, so its press is refused until something acts in it too.
pub fn live(paths: &Paths, session: &str, me: &Record) -> bool {
    if me.closed.is_some() {
        return true;
    }
    let mine = record_path(paths, session);
    let Ok(dir) = std::fs::read_dir(paths.sessions()) else { return true };
    let at = now();
    !dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "press") && *p != mine).any(|p| {
        state::read::<Record>(&p).is_ok_and(|r| {
            r.root == me.root
                && !r.ended
                && r.closed.is_some_and(|c| c <= me.opened && at.saturating_sub(c) <= MAX_AGE_MS)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> (tempfile::TempDir, Paths, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path().join("home"));
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        (dir, paths, repo)
    }

    fn payload(session: &str, cwd: &Path, source: &str) -> Value {
        json!({ "session_id": session, "cwd": cwd, "source": source })
    }

    fn start(paths: &Paths, repo: &Path, session: &str, source: &str) {
        assert_eq!(
            gate("claude", "SessionStart", &payload(session, repo, source), session, repo, paths, false),
            Gate::Pass
        );
    }

    fn press(paths: &Paths, repo: &Path, session: &str, source: &str) -> Option<String> {
        match gate("claude", EVENT, &payload(session, repo, source), session, repo, paths, false) {
            Gate::Answer(Some(out)) => {
                Some(serde_json::from_str::<Value>(&out).unwrap()["press"].as_str().unwrap().to_string())
            }
            Gate::Answer(None) => None,
            Gate::Pass => panic!("a press always answers"),
        }
    }

    fn act(paths: &Paths, repo: &Path, session: &str, event: &str) {
        let p = json!({ "session_id": session, "cwd": repo, "tool_name": "Bash", "tool_input": { "command": "ls" } });
        assert_eq!(gate("claude", event, &p, session, repo, paths, false), Gate::Pass);
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
        assert_eq!(gate("claude", "SessionEnd", &p, "s", &repo, &paths, false), Gate::Pass);
        assert_eq!(state::read::<Record>(&record_path(&paths, "s")).unwrap().hash, None);
    }

    /// T-X-13 (L2, the hand-written start): a start for a new session id, written while another
    /// session in the repository is at work, yields a token no verb in that repository honours.
    #[test]
    fn a_start_written_by_hand_is_not_the_live_session() {
        let (_d, paths, repo) = world();
        start(&paths, &repo, "real", "startup");
        let real = press(&paths, &repo, "real", "startup").unwrap();
        act(&paths, &repo, "real", "UserPromptSubmit");
        std::thread::sleep(std::time::Duration::from_millis(5));
        start(&paths, &repo, "forged", "startup");
        let forged = press(&paths, &repo, "forged", "startup").unwrap();
        let root = root_of(&repo);
        assert_eq!(spend(&paths, &forged, "forged", &root, "review approve"), Err(Refused::NotLive));
        assert!(spend(&paths, &real, "real", &root, "review approve").is_ok());
        // Another repository's verb refuses the real token too.
        let other = repo.parent().unwrap().join("other");
        std::fs::create_dir_all(other.join(".git")).unwrap();
        let next = state::read::<Record>(&record_path(&paths, "real")).unwrap();
        assert!(next.hash.is_some());
        assert_eq!(spend(&paths, "00", "real", &root_of(&other), "on"), Err(Refused::OtherRepository));
        // Before anything acts, a lone session's press is honoured.
        let (_d2, paths2, repo2) = world();
        start(&paths2, &repo2, "only", "startup");
        let t = press(&paths2, &repo2, "only", "startup").unwrap();
        assert!(spend(&paths2, &t, "only", &root_of(&repo2), "on").is_ok());
    }

    /// T-A7-08 at the unit level: recorded every time, warned once, never a refusal.
    #[test]
    fn the_tripwire_warns_once() {
        assert!(names_press("adage hook claude Press < p.json"));
        assert!(names_press("adage review approve t r <<'EOF'\n{\"press\": \"ab\", \"session\": \"s\"}\nEOF"));
        assert!(names_press("echo '{\"press\":\"x\"}' | adage on"));
        assert!(!names_press("adage review approve t r"));
        assert!(!names_press("git commit -m 'press the button'"));
        let (_d, paths, repo) = world();
        let p = json!({ "session_id": "s", "cwd": repo, "tool_name": "Bash", "tool_input": { "command": "adage hook claude Press" } });
        let Gate::Answer(Some(w)) = gate("claude", "PreToolUse", &p, "s", &repo, &paths, false) else { panic!() };
        let v: Value = serde_json::from_str(&w).unwrap();
        assert!(v["systemMessage"].as_str().unwrap().contains("press"));
        assert!(v.get("hookSpecificOutput").is_none(), "never a deny or an updatedInput");
        assert_eq!(gate("claude", "PreToolUse", &p, "s", &repo, &paths, false), Gate::Pass);
        let recs: Vec<Value> = spool::read_all(&paths.spool("s")).unwrap();
        assert_eq!(recs.iter().filter(|r| r["kind"] == "tripwire").count(), 2);
        // Not under a kill switch, and not for another harness.
        assert_eq!(gate("claude", "PreToolUse", &p, "s", &repo, &paths, true), Gate::Pass);
        assert_eq!(gate("codex", EVENT, &p, "s", &repo, &paths, false), Gate::Pass);
    }
}
