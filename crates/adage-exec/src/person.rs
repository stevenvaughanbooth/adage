//! Approval changes are a person's decision (Track C §4.3): they refuse inside an agent session
//! and need a terminal. The same rule as `adage`'s `require_person` (adage-cli `share.rs`), with
//! the full list of agent markers (`adage-train`'s `isolate::AGENT_ENV`).

use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::io::IsTerminal;

/// Variables that mark an agent session in every harness Adage supports, plus Adage's own markers
/// for sessions it started.
pub const AGENT_ENV: &[&str] =
    &["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED", "ADAGE_ARM", "ADAGE_TRAIN_SESSION"];

/// Refuse unless a person is at a terminal and not inside an agent session.
/// `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal requirement (for CI); nothing lifts the
/// agent-session refusal.
pub fn require_person_in(cmd: &str, env: &BTreeMap<String, String>, stdin_is_tty: bool) -> Result<()> {
    if let Some(v) = AGENT_ENV.iter().find(|v| env.get(**v).is_some_and(|x| !x.is_empty())) {
        bail!(
            "`adage-exec {cmd}` is a decision for a person (it changes what Adage may run on this machine), so it \
             will not run inside an agent session ({v} is set). Run it yourself, in a terminal."
        );
    }
    if env.get("ADAGE_ALLOW_NONINTERACTIVE").map(String::as_str) == Some("1") {
        return Ok(());
    }
    if !stdin_is_tty {
        bail!(
            "`adage-exec {cmd}` needs a person at a terminal (stdin is not a terminal). For CI, set \
             ADAGE_ALLOW_NONINTERACTIVE=1."
        );
    }
    Ok(())
}

pub fn require_person(cmd: &str) -> Result<()> {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    require_person_in(cmd, &env, std::io::stdin().is_terminal())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn refuses_agents_and_non_terminals() {
        assert!(require_person_in("approve", &env(&[]), true).is_ok());
        assert!(require_person_in("approve", &env(&[]), false).is_err());
        assert!(require_person_in("approve", &env(&[("ADAGE_ALLOW_NONINTERACTIVE", "1")]), false).is_ok());
        for v in AGENT_ENV {
            let e = env(&[(v, "1"), ("ADAGE_ALLOW_NONINTERACTIVE", "1")]);
            assert!(require_person_in("approve", &e, true).unwrap_err().to_string().contains(v));
        }
    }
}
