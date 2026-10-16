//! Verification evidence computed from the original call; transport status stays unchanged.

use crate::StepObs;
use adage_core::outcome::{self, VerificationOutcome, VerificationReport};
use adage_core::CanonicalAction;

impl StepObs {
    /// A known runner's outcome, distinct from `Call.failed` (the harness/tool status). A
    /// compound call's shared output is not assigned to each flattened runner. Unsupported
    /// commands return None; recognized commands with insufficient evidence return Unknown.
    pub fn verification(&self) -> Option<VerificationReport> {
        let CanonicalAction::Exec { argv, .. } = &self.norm.action else { return None };
        // Establish runner identity before inspecting any output, including tool refusals.
        outcome::verification_report(argv, "")?;
        if self.call.refused() {
            return Some(VerificationReport { outcome: VerificationOutcome::SetupFailure, tests_run: None });
        }
        if self.call.tool == "Bash" {
            if let Some(command) = self.call.input.get("command").and_then(|c| c.as_str()) {
                return outcome::verification_for_command(argv, command, &self.call.output, self.call.failed);
            }
        }
        // A cached normalized argv alone does not establish ownership of a shared tool result.
        Some(VerificationReport { outcome: VerificationOutcome::Unknown, tests_run: None })
    }
}
