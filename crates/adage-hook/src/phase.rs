//! The Claude Code mod's two phases of a Bash call (its architecture §2 rule 2, §3.3, §3.4; plan
//! §2.2 item 1): the fields a `PreToolUse` payload carries only when the mod sends it, and the
//! identity that keeps one call to one decision however many times it is delivered.
//!
//! A command hook's payload never carries `dry_run` or `verdict`, so it is decided exactly as it
//! was before the mod existed ([`Phase::of`] is `None`). The mod asks twice: a dry run for the plan
//! (nothing recorded), then a verdict carrying what the engine found out (decided and recorded
//! once). The command hook that follows the mod for the same call is then answered with nothing,
//! by the identity ring on the session state (`SessionState::seen_pretool`).

use serde_json::Value;

/// What a mod's `PreToolUse` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// `dry_run: true`: the plan (`{"plan": {substitute?, rewrite?, advice?, receipt?}}`), in
    /// either arm; nothing is recorded and no state changes.
    DryRun,
    /// `verdict: {…}`: decide finally, with what the mod found out, and record once.
    Verdict(Verdict),
}

/// What the mod found out before it asks for the final decision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verdict {
    /// `engine`: whether Claude Code's permission engine (`$.tool.check`) decides the command as
    /// written and the planned one alike. Present, it replaces the planner's own reading of the
    /// person's settings files for this decision (`checked_by: engine`).
    pub engine: Option<Engine>,
    /// `answered: "receipt"`: the mod answered the call with the plan's fresh receipt instead of
    /// running it (chunk C3 decides on it).
    pub answered: Option<Answered>,
    /// `substitute: <route>`: the learned dead end the plan named, to replace now.
    pub substitute: Option<String>,
}

/// The engine's comparison of the two commands' permission decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Same,
    Differs,
}

/// How the mod answered the call itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answered {
    Receipt,
}

impl Phase {
    /// The phase a payload asks for, or `None` for a command hook's payload (no `dry_run: true`, no
    /// `verdict`). A dry run wins over a verdict in the same payload: it never records. A `verdict`
    /// that is not an object is a verdict that names nothing: unknown values are left unset, never
    /// guessed.
    pub fn of(p: &Value) -> Option<Phase> {
        if p.get("dry_run").and_then(Value::as_bool) == Some(true) {
            return Some(Phase::DryRun);
        }
        let v = p.get("verdict")?;
        let engine = match v.get("engine").and_then(Value::as_str) {
            Some("same") => Some(Engine::Same),
            Some("differs") => Some(Engine::Differs),
            _ => None,
        };
        let answered = (v.get("answered").and_then(Value::as_str) == Some("receipt")).then_some(Answered::Receipt);
        let substitute = v.get("substitute").and_then(Value::as_str).filter(|r| !r.is_empty()).map(str::to_string);
        Some(Phase::Verdict(Verdict { engine, answered, substitute }))
    }

    /// The value the spooled record carries as `checked_by`: which check decided whether a
    /// rewrite keeps the person's permission outcome.
    pub fn checked_by(&self) -> &'static str {
        match self {
            Phase::Verdict(Verdict { engine: Some(_), .. }) => "engine",
            _ => "planner",
        }
    }
}

/// The longest engine sentence a withheld step's record keeps, in bytes.
const MAX_REASON: usize = 512;
/// The longest tool name it keeps.
const MAX_TOOL: usize = 64;

/// The phase of the mod's two-phase step a `TurnStep` payload asks for
/// ([`adage_runtime::StepPhase`]; the mod's architecture §3.2): `dry_run: true` is the dry run
/// (it wins over a `verdict` in the same payload, as for a Bash call); `verdict: { permission,
/// route?, tool?, reason? }` the verdict; neither, the one-phase step a command hook and a mod
/// without the phases send. Only `permission: "allow"` is an allow: any other value, a missing
/// one, or a `verdict` that is not an object holds the step back (`none`). Text fields are kept
/// only when non-empty, and cut to a bound so the record stays small.
pub fn step_phase(p: &Value) -> adage_runtime::StepPhase {
    use adage_runtime::{Permission, PermissionVerdict, StepPhase};
    if p.get("dry_run").and_then(Value::as_bool) == Some(true) {
        return StepPhase::DryRun;
    }
    let Some(v) = p.get("verdict") else { return StepPhase::Commit };
    let text = |key: &str, max: usize| {
        let s = v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())?;
        let end = s.char_indices().map(|(i, c)| i + c.len_utf8()).take_while(|&e| e <= max).last().unwrap_or(0);
        Some(s[..end].to_string()).filter(|s| !s.is_empty())
    };
    let decision = match v.get("permission").and_then(Value::as_str) {
        Some("allow") => Permission::Allow,
        Some("ask") => Permission::Ask,
        Some("deny") => Permission::Deny,
        _ => Permission::None,
    };
    StepPhase::Verdict {
        route: text("route", 256),
        permission: PermissionVerdict { decision, tool: text("tool", MAX_TOOL), reason: text("reason", MAX_REASON) },
    }
}

/// The 64-bit hash a call's harness identity is kept as (`SessionState::seen_pretool`): the first
/// eight bytes of its SHA-256, so the state stays small and the identity's text is never stored.
pub fn identity_hash(identity: &str) -> u64 {
    adage_runtime::identity_hash(identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_command_hooks_payload_has_no_phase() {
        assert_eq!(Phase::of(&json!({ "tool_name": "Bash", "tool_input": { "command": "ls" } })), None);
        assert_eq!(Phase::of(&json!({ "dry_run": false })), None);
    }

    #[test]
    fn the_mods_fields_are_read_and_unknown_values_left_unset() {
        assert_eq!(Phase::of(&json!({ "dry_run": true })), Some(Phase::DryRun));
        assert_eq!(Phase::of(&json!({ "dry_run": true, "verdict": { "engine": "same" } })), Some(Phase::DryRun));
        let v = Phase::of(&json!({ "verdict": { "engine": "differs", "answered": "receipt", "substitute": "r" } }));
        assert_eq!(
            v,
            Some(Phase::Verdict(Verdict {
                engine: Some(Engine::Differs),
                answered: Some(Answered::Receipt),
                substitute: Some("r".into())
            }))
        );
        assert_eq!(v.unwrap().checked_by(), "engine");
        let odd = Phase::of(&json!({ "verdict": { "engine": "maybe", "answered": "result", "substitute": "" } }));
        assert_eq!(odd, Some(Phase::Verdict(Verdict::default())));
        assert_eq!(odd.unwrap().checked_by(), "planner");
        assert_eq!(Phase::of(&json!({ "verdict": "same" })), Some(Phase::Verdict(Verdict::default())));
    }

    /// T-C4-08: a step's phase: none for a command hook's payload, the dry run winning over a
    /// verdict, only `allow` an allow, the engine's sentence kept and bounded.
    #[test]
    fn a_steps_phase_is_read_and_only_allow_allows() {
        use adage_runtime::{Permission, PermissionVerdict, StepPhase};
        let step = json!({ "hook_event_name": "TurnStep", "turn_id": "t", "index": 1 });
        assert_eq!(step_phase(&step), StepPhase::Commit);
        assert_eq!(step_phase(&json!({ "dry_run": false })), StepPhase::Commit);
        assert_eq!(step_phase(&json!({ "dry_run": true, "verdict": { "permission": "allow" } })), StepPhase::DryRun);
        let v = step_phase(&json!({ "verdict": { "permission": "ask", "route": "gen", "tool": "Bash",
                                                  "reason": "This command requires approval" } }));
        assert_eq!(
            v,
            StepPhase::Verdict {
                route: Some("gen".into()),
                permission: PermissionVerdict {
                    decision: Permission::Ask,
                    tool: Some("Bash".into()),
                    reason: Some("This command requires approval".into())
                }
            }
        );
        for (said, decision) in [
            (json!("allow"), Permission::Allow),
            (json!("deny"), Permission::Deny),
            (json!("none"), Permission::None),
            (json!("maybe"), Permission::None),
            (json!(true), Permission::None),
            (Value::Null, Permission::None),
        ] {
            let StepPhase::Verdict { permission, route } =
                step_phase(&json!({ "verdict": { "permission": said, "route": "" } }))
            else {
                panic!("a verdict")
            };
            assert_eq!((permission.decision, route), (decision, None));
        }
        assert_eq!(
            step_phase(&json!({ "verdict": "allow" })),
            StepPhase::Verdict { route: None, permission: PermissionVerdict::default() }
        );
        let long = "é".repeat(400);
        let StepPhase::Verdict { permission, .. } = step_phase(&json!({ "verdict": { "reason": long } })) else {
            panic!("a verdict")
        };
        let reason = permission.reason.unwrap();
        assert!(reason.len() <= 512 && reason.len() >= 510 && long.starts_with(&reason));
    }

    #[test]
    fn identities_hash_stably_and_apart() {
        assert_eq!(identity_hash("a"), identity_hash("a"));
        assert_ne!(identity_hash("a"), identity_hash("b"));
    }
}
