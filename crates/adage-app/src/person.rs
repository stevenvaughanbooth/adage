//! Who may decide. Decisions that change trust (reviewing reflexes, sharing, installing packs,
//! delegating execution) are a person's: they never run inside an agent session. A [`Person`] is
//! the proof that the check passed, and the operations that need one take it, so no front end
//! (the CLI, the TUI, a future HTTP API) can reach them without making the check.
//!
//! A person reaches Adage two ways: at a terminal ([`require`]), or, at the call sites the design
//! lists (architecture §3.12), with a **press** from the Claude Code session ([`require_or_press`]):
//! a single-use token the binary issued to the mod before the agent's first act, read from stdin.
//! A press is not a boundary against an agent with a shell; it is a record that a person decided
//! (`via: press`) and a guard against the model being steered into deciding.

use crate::error::hint;
use adage_store::Paths;
pub use adage_tracks::review::Via;
use anyhow::Result;
use serde::Deserialize;
use std::io::{IsTerminal, Read};
use std::time::Duration;

/// The environment variables each harness sets inside its agent sessions.
pub const AGENT_ENV: [&str; 4] = ["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED"];

/// A person is deciding: made only by [`require`] and [`require_or_press`].
#[derive(Debug)]
pub struct Person {
    via: Via,
    next: Option<String>,
}

impl Person {
    /// How the person decided, for the record of the decision.
    pub fn via(&self) -> Via {
        self.via
    }

    /// The next token of the session's chain, when this decision spent a press: the front end
    /// hands it back to the mod (`{ "next": <token> }`), since the one it spent is now refused.
    pub fn next(&self) -> Option<&str> {
        self.next.as_deref()
    }

    /// The crate's own tests decide as a person without a terminal.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Person {
        Person { via: Via::Terminal, next: None }
    }
}

/// Review finding F11: commands that publish or change trust refuse to run inside an agent session
/// or without a terminal. `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal requirement (for
/// CI and tests); nothing lifts the agent-session refusal. Environment detection is a guard against
/// an agent being steered into these commands, not a sandbox.
pub fn require(cmd: &str) -> Result<Person> {
    // Every harness's marker, not only Claude Code's: an OpenCode or Codex agent must not be able
    // to share, trust, or review either.
    if let Some(var) = agent_var() {
        return Err(hint(
            format!(
                "`adage {cmd}` is a decision for a person ({}), so it will not run inside an agent session ({var} is set)",
                what_it_changes(cmd)
            ),
            format!("run `adage {cmd}` yourself, in a terminal"),
        ));
    }
    if allow_noninteractive() {
        return Ok(Person { via: Via::Terminal, next: None });
    }
    if !std::io::stdin().is_terminal() {
        return Err(hint(
            format!("`adage {cmd}` needs a person at a terminal (stdin is not a terminal)"),
            "run it from a terminal; for CI, set ADAGE_ALLOW_NONINTERACTIVE=1",
        ));
    }
    Ok(Person { via: Via::Terminal, next: None })
}

/// A press as a verb reads it on stdin: `{ "press": <token>, "session": <id> }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Press {
    pub press: String,
    pub session: String,
}

/// What a person-only verb reads on stdin when stdin is not a terminal: a press, and for `review
/// delegate --confirm` the digest the stage answered (`expect`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stdin {
    pub press: Option<Press>,
    pub expect: Option<String>,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    press: Option<String>,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    expect: Option<String>,
}

/// The most a verb reads from stdin, and how long it waits for the writer to finish: the mod
/// writes one small object and closes the pipe, and a pipe that stays open (a CI step that
/// inherited one) must not hold a verb up.
const MAX_STDIN: u64 = 64 << 10;
const STDIN_WAIT: Duration = Duration::from_secs(3);

/// Read the verb's stdin, once: nothing at a terminal (a press is never typed), nothing for input
/// that is not one JSON object.
pub fn read_stdin() -> Stdin {
    if std::io::stdin().is_terminal() {
        return Stdin::default();
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::Builder::new().spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::stdin().lock().take(MAX_STDIN).read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    if reader.is_err() {
        return Stdin::default();
    }
    rx.recv_timeout(STDIN_WAIT).map(|b| parse_stdin(&b)).unwrap_or_default()
}

/// [`read_stdin`] where a press may be on it: only inside an agent session (an agent marker set;
/// the mod runs every verb with `CLAUDECODE=1`). At a terminal, or in CI (no marker), nothing is
/// read and nothing waited for, so an inherited pipe that stays open costs a verb nothing.
pub fn read_press() -> Stdin {
    if agent_var().is_none() {
        return Stdin::default();
    }
    read_stdin()
}

/// [`read_stdin`]'s parse, apart for tests.
pub fn parse_stdin(bytes: &[u8]) -> Stdin {
    let Ok(raw) = serde_json::from_slice::<Raw>(bytes) else { return Stdin::default() };
    let nonempty = |s: Option<String>| s.filter(|v| !v.trim().is_empty());
    let press = match (nonempty(raw.press), nonempty(raw.session)) {
        (Some(press), Some(session)) => Some(Press { press: press.trim().to_string(), session }),
        _ => None,
    };
    Stdin { press, expect: nonempty(raw.expect) }
}

/// [`require`], or a valid press: called only at the sites architecture §3.12 lists (the review
/// verbs, `review revoke`, `review delegate`, `delegate on`, `delegate policy --max-level e2`,
/// `delegate predict on`, `on`, and `add <pack> --trust` for a pack committed in the
/// repository). Without a press it is exactly [`require`]. With one, the agent-session and
/// terminal checks are lifted when the press is valid for this repository's live session
/// (`adage_hook::press::spend`), the chain moves on, and the [`Person`] carries the next token.
/// A press beside `ADAGE_ALLOW_NONINTERACTIVE=1` is refused: the two never travel together. A
/// press that is not valid behaves exactly as no press, and records nothing.
pub fn require_or_press(cmd: &str, press: Option<&Press>, paths: &Paths) -> Result<Person> {
    let Some(p) = press else { return require(cmd) };
    if allow_noninteractive() {
        return match require(cmd) {
            Err(e) => Err(e),
            Ok(_) => Err(hint(
                format!("`adage {cmd}` was given a press and ADAGE_ALLOW_NONINTERACTIVE=1 together, and refuses both"),
                "a press is a person's answer in the session; run the command at a terminal, or in CI without a press",
            )),
        };
    }
    let root = std::env::current_dir().map(|d| adage_hook::press::root_of(&d)).unwrap_or_default();
    match adage_hook::press::spend(paths, &p.press, &p.session, &root, cmd) {
        Ok(next) => Ok(Person { via: Via::Press, next: Some(next) }),
        Err(_) => require(cmd),
    }
}

/// When the press session `session` opened its window (ms since the epoch): what a press is held
/// to where its verb asks whether something is older than the session (`add --trust`).
pub fn press_opened(paths: &Paths, session: &str) -> Option<u64> {
    let path = adage_hook::press::record_path(paths, session);
    adage_store::state::read::<adage_hook::press::Record>(&path).ok().map(|r| r.opened).filter(|&t| t > 0)
}

fn agent_var() -> Option<&'static str> {
    AGENT_ENV.iter().copied().find(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty()))
}

fn allow_noninteractive() -> bool {
    std::env::var("ADAGE_ALLOW_NONINTERACTIVE").as_deref() == Ok("1")
}

/// Why a command is a person's decision, in its own terms, for the refusal inside an agent session.
fn what_it_changes(cmd: &str) -> &'static str {
    let verb = cmd.split_whitespace().collect::<Vec<_>>();
    match verb.as_slice() {
        ["review", "delegate" | "revoke", ..] | ["delegate", "on" | "policy" | "predict", ..] => {
            "it changes what Adage may run on this machine"
        }
        ["review", ..] => "it changes which reflexes your agents receive, and how strongly",
        ["share", ..] => "it publishes a signed pack of your reflexes",
        ["add", ..] => "it installs a pack and trusts its signer",
        ["train", "accept", ..] => "it installs a reviewed pack of reflexes",
        ["config", "set", ..] => "it changes Adage's settings",
        ["init", ..] => "it downloads into Adage's data directory and binds instincts",
        ["on", ..] => "it turns Adage back on in this repository",
        _ => "it changes what is shared, trusted or configured",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA first-run #15: each refusal names what that command changes.
    #[test]
    fn refusals_say_what_each_command_changes() {
        assert_eq!(what_it_changes("review delegate"), "it changes what Adage may run on this machine");
        assert_eq!(what_it_changes("delegate on"), "it changes what Adage may run on this machine");
        assert_eq!(what_it_changes("delegate predict on"), "it changes what Adage may run on this machine");
        assert_eq!(what_it_changes("delegate policy --max-level e2"), "it changes what Adage may run on this machine");
        assert_eq!(
            what_it_changes("review approve"),
            "it changes which reflexes your agents receive, and how strongly"
        );
        assert_eq!(what_it_changes("share"), "it publishes a signed pack of your reflexes");
        assert_eq!(what_it_changes("config set learn.auto"), "it changes Adage's settings");
        assert_eq!(what_it_changes("on"), "it turns Adage back on in this repository");
    }

    #[test]
    fn stdin_holds_a_press_only_with_both_fields() {
        let s = parse_stdin(br#"{"press":"ab","session":"s1","expect":"d"}"#);
        assert_eq!(s.press, Some(Press { press: "ab".into(), session: "s1".into() }));
        assert_eq!(s.expect.as_deref(), Some("d"));
        assert_eq!(parse_stdin(br#"{"press":"ab"}"#).press, None);
        assert_eq!(parse_stdin(br#"{"press":"","session":"s"}"#).press, None);
        assert_eq!(parse_stdin(b"not json"), Stdin::default());
        assert_eq!(parse_stdin(br#"{"expect":"d"}"#), Stdin { press: None, expect: Some("d".into()) });
    }
}
