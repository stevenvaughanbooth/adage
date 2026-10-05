//! Who may decide. Decisions that change trust (reviewing reflexes, sharing, installing packs,
//! delegating execution) are a person's: they never run inside an agent session. A [`Person`] is
//! the proof that the check passed, and the operations that need one take it, so no front end
//! (the CLI, the TUI, a future HTTP API) can reach them without making the check.

use crate::error::hint;
use anyhow::Result;
use std::io::IsTerminal;

/// The environment variables each harness sets inside its agent sessions.
pub const AGENT_ENV: [&str; 4] = ["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED"];

/// A person is deciding: made only by [`require`].
#[derive(Debug)]
pub struct Person(());

#[cfg(test)]
impl Person {
    /// The crate's own tests decide as a person without a terminal.
    pub(crate) fn for_tests() -> Person {
        Person(())
    }
}

/// Review finding F11: commands that publish or change trust refuse to run inside an agent session
/// or without a terminal. `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal requirement (for
/// CI and tests); nothing lifts the agent-session refusal. Environment detection is a guard against
/// an agent being steered into these commands, not a sandbox.
pub fn require(cmd: &str) -> Result<Person> {
    // Every harness's marker, not only Claude Code's: an OpenCode or Codex agent must not be able
    // to share, trust, or review either.
    if let Some(var) = AGENT_ENV.iter().find(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty())) {
        return Err(hint(
            format!(
                "`adage {cmd}` is a decision for a person ({}), so it will not run inside an agent session ({var} is set)",
                what_it_changes(cmd)
            ),
            format!("run `adage {cmd}` yourself, in a terminal"),
        ));
    }
    if std::env::var("ADAGE_ALLOW_NONINTERACTIVE").as_deref() == Ok("1") {
        return Ok(Person(()));
    }
    if !std::io::stdin().is_terminal() {
        return Err(hint(
            format!("`adage {cmd}` needs a person at a terminal (stdin is not a terminal)"),
            "run it from a terminal; for CI, set ADAGE_ALLOW_NONINTERACTIVE=1",
        ));
    }
    Ok(Person(()))
}

/// Why a command is a person's decision, in its own terms, for the refusal inside an agent session.
fn what_it_changes(cmd: &str) -> &'static str {
    let verb = cmd.split_whitespace().collect::<Vec<_>>();
    match verb.as_slice() {
        ["review", "delegate" | "revoke", ..] | ["delegate", "on", ..] => {
            "it changes what Adage may run on this machine"
        }
        ["review", ..] => "it changes which reflexes your agents receive, and how strongly",
        ["share", ..] => "it publishes a signed pack of your reflexes",
        ["add", ..] => "it installs a pack and trusts its signer",
        ["train", "accept", ..] => "it installs a reviewed pack of reflexes",
        ["config", "set", ..] => "it changes Adage's settings",
        ["init", ..] => "it downloads into Adage's data directory and binds instincts",
        _ => "it changes what is shared, trusted or configured",
    }
}

#[cfg(test)]
mod tests {
    use super::what_it_changes;

    /// QA first-run #15: each refusal names what that command changes.
    #[test]
    fn refusals_say_what_each_command_changes() {
        assert_eq!(what_it_changes("review delegate"), "it changes what Adage may run on this machine");
        assert_eq!(what_it_changes("delegate on"), "it changes what Adage may run on this machine");
        assert_eq!(
            what_it_changes("review approve"),
            "it changes which reflexes your agents receive, and how strongly"
        );
        assert_eq!(what_it_changes("share"), "it publishes a signed pack of your reflexes");
        assert_eq!(what_it_changes("config set learn.auto"), "it changes Adage's settings");
    }
}
