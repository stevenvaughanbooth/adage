//! Conservative recovery observations, never source snapshots, receipts or task grades.
use adage_core::{outcome, CanonicalAction, CommandTable, Effect};
use adage_trace::{By, Call};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const TEMPLATE: &str = "verification-unexplained-recovery";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Pending {
    command: String,
    cwd: String,
    agent: Option<String>,
    call_id: String,
    tests_run: u64,
    remaining: usize,
}

/// The ordinary session's bounded watch. Lost state starts empty, never inferred from keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending: Option<Pending>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    seen: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    pub command: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub agent: Option<String>,
    pub failed_call: String,
    pub passed_call: String,
    pub tests_run: u64,
}

impl RecoveryState {
    pub fn is_empty(&self) -> bool {
        self.pending.is_none()
    }
    pub fn is_initial(&self) -> bool {
        self.pending.is_none() && self.seen.is_empty()
    }

    /// Task boundaries, exposure and unknown history invalidate the observation.
    pub fn clear(&mut self) {
        self.pending = None;
    }

    /// Observe one original call, not every flattened action. `within` is the caller's history
    /// bound. Another agent/cwd, another check, edits and opaque activity are barriers. Only
    /// clearly observational reads/searches/lists may separate the two test attempts.
    pub fn observe(&mut self, call: &Call, root: &Path, table: &CommandTable, within: usize) -> Option<Recovery> {
        let Some(id) = call.tool_use_id.as_deref().filter(|id| !id.is_empty() && id.len() <= 256) else {
            self.clear();
            return None;
        };
        let identity = format!("{:x}", Sha256::digest(format!("{:?}:{id}", call.agent).as_bytes()));
        if self.seen.contains(&identity) {
            return None;
        }
        if within == 0 {
            self.clear();
            return None;
        }
        self.seen.push(identity);
        if self.seen.len() > within {
            self.seen.drain(..self.seen.len() - within);
        }
        let cwd = root.to_string_lossy().into_owned();
        if self.pending.as_ref().is_some_and(|p| p.agent != call.agent || p.cwd != cwd || p.remaining == 0) {
            self.clear();
        }
        if let Some(p) = &mut self.pending {
            p.remaining = p.remaining.saturating_sub(1);
        }
        if call.by != By::Agent || call.refused() {
            self.clear();
            return None;
        }
        let norms = adage_core::tools::normalize_tool(&call.tool, &call.input, &cwd, table);
        let observational = matches!(call.tool.as_str(), "Read" | "Grep" | "Glob" | "LS")
            && !norms.is_empty()
            && norms.iter().all(|n| {
                !n.mutating
                    && n.effect == Effect::Observe
                    && matches!(
                        n.action,
                        CanonicalAction::Read { .. } | CanonicalAction::Search { .. } | CanonicalAction::List { .. }
                    )
            });
        if observational && !call.failed {
            return None;
        }
        let previous = self.pending.take();
        let [n] = norms.as_slice() else { return None };
        let CanonicalAction::Exec { argv, cwd: actual_cwd } = &n.action else { return None };
        let command = call.input.get("command")?.as_str()?;
        if call.tool != "Bash"
            || actual_cwd != &cwd
            || !crate::advice::plain_command(command)
            || adage_core::secrets::has_secret(command)
        {
            return None;
        }
        let report = outcome::verification_test_attempt(argv, command, &call.output, call.failed)?;
        let tests_run = report.tests_run?;
        if report.outcome == outcome::VerificationOutcome::Failed {
            self.pending = Some(Pending {
                command: command.into(),
                cwd,
                agent: call.agent.clone(),
                call_id: id.into(),
                tests_run,
                remaining: within,
            });
            return None;
        }
        let p = previous?;
        if p.command != command || p.tests_run != tests_run {
            return None;
        }
        Some(Recovery {
            command: command.into(),
            argv: argv.clone(),
            cwd,
            agent: call.agent.clone(),
            failed_call: p.call_id,
            passed_call: id.into(),
            tests_run,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn call(id: &str, command: &str, output: &str, failed: bool) -> Call {
        Call {
            tool: "Bash".into(),
            input: json!({"command":command}),
            output: output.into(),
            failed,
            by: By::Agent,
            tool_use_id: Some(id.into()),
            agent: None,
        }
    }
    #[test]
    fn same_scope_retry_and_duplicate_failure() {
        let mut s = RecoveryState::default();
        let root = Path::new("/repo");
        let red = call("red", "pytest tests/test_a.py", "1 failed, 1 passed in 0.1s", true);
        let green = call("green", "pytest tests/test_a.py", "2 passed in 0.1s", false);
        assert!(s.observe(&red, root, CommandTable::load(), 4).is_none());
        assert!(s.observe(&red, root, CommandTable::load(), 4).is_none());
        let read = Call {
            tool: "Read".into(),
            input: json!({"file_path":"/repo/src/a.py"}),
            ..call("read", "", "source", false)
        };
        s.observe(&read, root, CommandTable::load(), 4);
        let hit = s.observe(&green, root, CommandTable::load(), 4).unwrap();
        assert_eq!((hit.failed_call.as_str(), hit.passed_call.as_str(), hit.tests_run), ("red", "green", 2));
        assert!(s.observe(&green, root, CommandTable::load(), 4).is_none());
        assert!(s.is_empty());
    }
    #[test]
    fn repairs_opaque_activity_and_scope_changes_abstain() {
        let root = Path::new("/repo");
        let red = call("red", "pytest tests/test_a.py", "1 failed, 1 passed in 0.1s", true);
        for middle in [
            call("x", "python -c print", "", false),
            call("x", "sed -i s/a/b/ src/a.py", "", false),
            call("x", "export FLAG=1", "", false),
            call("x", "pytest tests/b.py", "2 passed in 0.1s", false),
            call("x", "cat src/a.py > src/b.py", "", false),
            call("x", "rg --pre tool pattern", "", false),
            Call {
                tool: "Edit".into(),
                input: json!({"file_path":"/repo/src/a.py","old_string":"a","new_string":"b"}),
                ..call("x", "", "", false)
            },
        ] {
            let mut s = RecoveryState::default();
            s.observe(&red, root, CommandTable::load(), 4);
            s.observe(&middle, root, CommandTable::load(), 4);
            assert!(
                s.observe(
                    &call("green", "pytest tests/test_a.py", "2 passed in 0.1s", false),
                    root,
                    CommandTable::load(),
                    4
                )
                .is_none(),
                "{middle:?}"
            );
        }
        for (command, output, failed) in [
            ("pytest tests/test_a.py -k one", "2 passed in 0.1s", false),
            ("pytest tests/test_a.py", "1 passed in 0.1s", false),
            ("pytest tests/test_a.py", "2 passed in 0.1s\n[output truncated]", false),
            ("pytest tests/test_a.py", "no tests ran in 0.1s", false),
            ("pytest tests/test_a.py", "ModuleNotFoundError", true),
            ("pytest tests/test_a.py | tail -5", "2 passed in 0.1s", false),
            ("echo 2 passed in 0.1s", "2 passed in 0.1s", false),
        ] {
            let mut s = RecoveryState::default();
            s.observe(&red, root, CommandTable::load(), 4);
            assert!(s.observe(&call("green", command, output, failed), root, CommandTable::load(), 4).is_none());
        }
        for (agent, cwd) in [(Some("child".into()), root), (None, Path::new("/other"))] {
            let mut s = RecoveryState::default();
            s.observe(&red, root, CommandTable::load(), 4);
            let mut green = call("green", "pytest tests/test_a.py", "2 passed in 0.1s", false);
            green.agent = agent;
            assert!(s.observe(&green, cwd, CommandTable::load(), 4).is_none());
        }
    }

    #[test]
    fn absent_identity_expired_window_and_refusal_do_not_create_recovery() {
        let root = Path::new("/repo");
        let red = call("red", "pytest", "1 failed in 0.1s", true);
        for barrier in [
            Call { tool_use_id: None, ..call("", "pytest", "1 passed in 0.1s", false) },
            call("refused", "pytest", "<tool_use_error>permission denied</tool_use_error>", true),
        ] {
            let mut s = RecoveryState::default();
            s.observe(&red, root, CommandTable::load(), 2);
            s.observe(&barrier, root, CommandTable::load(), 2);
            assert!(s.is_empty());
            assert!(s
                .observe(&call("green", "pytest", "1 passed in 0.1s", false), root, CommandTable::load(), 2)
                .is_none());
        }
        let mut s = RecoveryState::default();
        s.observe(&red, root, CommandTable::load(), 1);
        let read =
            Call { tool: "Read".into(), input: json!({"file_path":"/repo/a.py"}), ..call("read", "", "source", false) };
        s.observe(&read, root, CommandTable::load(), 1);
        assert!(s
            .observe(&call("green", "pytest", "1 passed in 0.1s", false), root, CommandTable::load(), 1)
            .is_none());
    }
}
