//! Claude Code's `Standing`: the mod's cached system-prompt section, and what a subagent is told
//! at its spawn. Entry: [`standing`], from `handle`.

use crate::answer::ModAnswer;
use crate::ctx::HookCtx;
use crate::failopen::Failure;
use crate::harness;
use crate::pretool::u11_in_service;
use crate::site::{self, observed};
use adage_core::CommandTable;
use adage_runtime::{render, Arm, Ctx, SessionState};
use adage_store::{state, Paths};
use adage_tracks::Index;
use serde_json::Value;
use std::path::Path;

/// Claude Code's `Standing` (the mod's cached system-prompt section; its architecture §3.6, §3.7;
/// plan §2.2 item 8): `{"standing", "environment", "compact"}`, asked once by the mod after the
/// session starts. Read-only for the spool, the learner and the notices (the command hooks'
/// `SessionStart` records the start); it assigns the session's arm if it has none, as `PreToolUse`
/// does, and keeps the first answer on the session state ([`SessionState::standing`]), so every
/// later answer in the session is the same and the first prompt does not announce the same
/// reflexes again. In the control arm, and in observe mode, `standing` and `environment` are empty
/// and nothing is kept. `compact` (what compaction is asked to keep) is said in both arms. Nothing
/// at all outside a repository with reflexes.
///
/// **A subagent** (the mod's `agent.spawn`, mod architecture §3.8): a payload carrying `agent:
/// {type?, description?, prompt?}` is also answered `subagent`, the text the subagent is told
/// before it starts ([`adage_runtime::delegation`]: the prompt reflexes its own prompt binds, then
/// the standing reflexes), empty in the control arm. Read-only like the rest: what a subagent is
/// told is not what the session was told. Each field is answered by the harness's capability
/// ([`harness::Adapter::capabilities`], [`adage_runtime::standing_for`]).
///
/// **The shell tool's description** (the mod's `tool.describe`, architecture §1b.3): a payload
/// carrying `conventions: true` is also answered `conventions`, the repository's lookup
/// conventions one sentence each ([`adage_runtime::Standing::conventions`]), fixed for the session
/// like the rest, empty in the control arm and in observe mode. Without it the answer has no such
/// field.
pub(crate) fn standing(ctx: &HookCtx<'_>) -> Result<Option<String>, Failure> {
    let HookCtx { adapter, p, session, root, paths, .. } = *ctx;
    // Only a harness whose mod keeps a cached section asks it.
    let caps = adapter.capabilities("Standing", p);
    if !caps.cached_section {
        return Ok(None);
    }
    let index = ctx.index();
    let Some(key) = site::routed_key(index, root, paths, false) else { return Ok(None) };
    let known = state::try_read::<SessionState>(&paths.state(session)).ok().flatten();
    let answer = match known.as_ref() {
        _ if observed(p) => None,
        Some(st) if st.arm == Some(Arm::Control) => None,
        Some(SessionState { standing: Some(s), .. }) => Some(s.clone()),
        _ => {
            let environment = environment_note(root, paths, index, &key).unwrap_or_default();
            let made = adage_runtime::standing(index, &key, &adage_runtime::Policy::default(), environment);
            let arms = ctx.arms();
            state::update::<SessionState, _>(&paths.state(session), |st| {
                arms.ensure(st, session, &key);
                if st.arm == Some(Arm::Control) {
                    return None;
                }
                Some(st.standing.get_or_insert(made).clone())
            })
            .map_err(|e| Failure::io("state", "keeping the session's standing text", &e))?
        }
    };
    let s = adage_runtime::standing_for(&caps, answer.unwrap_or_default());
    let compact = if caps.compaction { render::COMPACT_INSTRUCTION } else { "" };
    let subagent = p.get("agent").filter(|a| a.is_object()).map(|agent| {
        // The state as the answer above left it: the arm is assigned by now (unless observing).
        let st = state::try_read::<SessionState>(&paths.state(session)).ok().flatten();
        subagent_text(agent, &key, root, index, st.as_ref(), &caps, observed(p))
    });
    // A mod that places the lookup conventions in the shell tool's description says so; to one
    // that does not, the answer is as before.
    let conventions = (p.get("conventions").and_then(Value::as_bool) == Some(true)).then_some(s.conventions);
    let standing = ModAnswer::Standing {
        standing: s.standing,
        environment: s.environment,
        compact: compact.into(),
        subagent,
        conventions,
    };
    Ok(Some(crate::answer::answer(&standing)))
}

/// What a subagent is told at its spawn ([`adage_runtime::delegation`]), from its description
/// and prompt (each bounded), read-only: the session's state is read, never written.
fn subagent_text(
    agent: &Value,
    key: &str,
    root: &Path,
    index: &Index,
    st: Option<&SessionState>,
    caps: &harness::Capabilities,
    observe: bool,
) -> String {
    let field = |k: &str| agent.get(k).and_then(Value::as_str).unwrap_or("");
    let mut text = format!("{}\n{}", field("description"), field("prompt"));
    if text.len() > 16 << 10 {
        let mut end = 16 << 10;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    let table = CommandTable::load();
    let ctx = Ctx {
        checkout: &adage_runtime::checkout::Disk,
        index,
        repo: key.to_string(),
        root,
        table,
        embed: None,
        params: Default::default(),
        policy: adage_runtime::Policy { observe, ..Default::default() },
    };
    let fresh = SessionState::default();
    adage_runtime::delegation(&ctx, st.unwrap_or(&fresh), &text, caps)
}

/// The environment sentence for the standing text ([`render::environment_note`]): when U11 is in
/// service here and its binding verified an environment for this checkout.
fn environment_note(root: &Path, paths: &Paths, index: &Index, key: &str) -> Option<String> {
    if !u11_in_service(index, key) {
        return None;
    }
    let binding = adage_pack::binding::binding(&paths.tracks(), key)?;
    let env = binding.env?;
    if Path::new(&binding.root) != root || !root.join(&env.interpreter).is_file() {
        return None;
    }
    render::environment_note(&env.interpreter, env.pytest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use crate::{handle, Options};
    use serde_json::json;
    use std::time::Instant;

    fn standing_event(w: &ObligationWorld, session: &str, extra: Value) -> Option<Value> {
        let mut p = json!({ "session_id": session, "cwd": w.root, "hook_event_name": "Standing" });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        let opts = Options { learner: None, extension: Some(&NO_EXTENSION) };
        handle("claude", "Standing", &p.to_string(), opts, &w.paths, Instant::now())
            .unwrap()
            .map(|o| serde_json::from_str(&o).unwrap())
    }

    fn prompt_event(w: &ObligationWorld, session: &str) -> Option<Value> {
        let p = json!({ "session_id": session, "cwd": w.root, "hook_event_name": "UserPromptSubmit", "prompt": "fix the invoice rounding" });
        let opts = Options { learner: None, extension: Some(&NO_EXTENSION) };
        handle("claude", "UserPromptSubmit", &p.to_string(), opts, &w.paths, Instant::now())
            .unwrap()
            .map(|o| serde_json::from_str(&o).unwrap())
    }

    /// T-C5-01: `Standing` answers the session-start reflexes once and the same for the whole
    /// session, within its bound; it records nothing in the spool, starts no learner and takes no
    /// notice; after it the first prompt does not say the same reflex again. A control session
    /// gets empty text, and so does observe mode; `compact` is said in both arms.
    #[test]
    fn the_standing_text_is_stable_bounded_and_read_only() {
        let w = dead_end_world(Arm::Treated);
        let first = standing_event(&w, "s", json!({})).expect("an answer");
        let text = first["standing"].as_str().unwrap();
        assert!(
            text.contains("route=\"dead-end-pytest\"") && text.contains("Use this instead:\n  make test"),
            "{text}"
        );
        assert!(text.len() <= render::MAX_STANDING);
        assert_eq!(
            (first["environment"].as_str(), first["compact"].as_str()),
            (Some(""), Some(render::COMPACT_INSTRUCTION))
        );
        assert_eq!(standing_event(&w, "s", json!({})), Some(first.clone()), "the same for the session");
        assert!(!w.paths.spool("s").exists(), "no spool record");
        assert!(!w.paths.root.join("learn").exists() && !w.paths.learn_queue().exists(), "no learner, no notice");
        assert_eq!(w.state().standing.as_ref().map(|s| s.routes.clone()), Some(vec!["dead-end-pytest".to_string()]));
        let told = prompt_event(&w, "s");
        assert!(told.as_ref().is_none_or(|t| !t.to_string().contains("dead-end-pytest")), "{told:?}");
        // Without the standing text, the first prompt says it.
        state::update::<SessionState, _>(&w.paths.state("s2"), |st| st.arm = Some(Arm::Treated)).unwrap();
        assert!(prompt_event(&w, "s2").unwrap().to_string().contains("dead-end-pytest"));
        // Control, and observe mode: nothing to place, nothing kept.
        state::update::<SessionState, _>(&w.paths.state("c"), |st| st.arm = Some(Arm::Control)).unwrap();
        let control = standing_event(&w, "c", json!({})).unwrap();
        assert_eq!(control, json!({ "standing": "", "environment": "", "compact": render::COMPACT_INSTRUCTION }));
        assert!(state::try_read::<SessionState>(&w.paths.state("c")).unwrap().unwrap().standing.is_none());
        let observed = standing_event(&w, "o", json!({ "observe": true })).unwrap();
        assert_eq!(observed["standing"], "");
        assert!(!w.paths.state("o").exists(), "observe mode keeps nothing");
        // Only Claude Code's mod asks it; nothing outside a repository with reflexes.
        let p = json!({ "session_id": "s", "cwd": w.root });
        let opts = Options { learner: None, extension: None };
        assert!(handle("codex", "Standing", &p.to_string(), opts, &w.paths, Instant::now()).unwrap().is_none());
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(elsewhere.path().join(".git")).unwrap();
        let p = json!({ "session_id": "s", "cwd": elsewhere.path() });
        assert!(handle("claude", "Standing", &p.to_string(), opts, &w.paths, Instant::now()).unwrap().is_none());
    }

    /// The environment sentence, when U11 bound the project's environment for this checkout.
    #[test]
    fn the_standing_answer_names_the_bound_environment() {
        let w = u11_world(Arm::Treated);
        let a = standing_event(&w, "s", json!({})).unwrap();
        let env = a["environment"].as_str().unwrap();
        assert!(
            env.starts_with("In this repository, Python runs in the project's environment, .venv/bin/python"),
            "{env}"
        );
        assert!(env.ends_with("Tests run with .venv/bin/python -m pytest."), "{env}");
    }

    /// A world with a work-item lookup bound from the prompt beside the dead end.
    fn delegation_world(arm: Arm) -> ObligationWorld {
        use adage_tracks::{
            index::IndexedRoute, Contract, Evidence, Route, RouteState, Slot, SlotSource, Step, Strength, Trigger,
        };
        let w = dead_end_world(arm);
        let mut index = Index::try_load(&w.paths.index()).unwrap();
        index.routes.push(IndexedRoute {
            repo: "acme".into(),
            track: "acme/lookups".into(),
            version: "0.1.0".into(),
            exec: None,
            route: Route {
                id: "lookup-work-item".into(),
                summary: String::new(),
                trigger: Trigger::Prompt { span: adage_core::spans::SpanKind::Ticket },
                step: Step::Run { command: "python3 scripts/wi-lookup {item}".into() },
                slots: vec![Slot {
                    name: "item".into(),
                    source: SlotSource::Prompt { span: adage_core::spans::SpanKind::Ticket },
                    contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
                }],
                effect: adage_core::Effect::Observe,
                strength: Strength::Fire,
                state: RouteState::Firm,
                exit: vec![],
                evidence: Evidence { support: 9, opportunities: 10, days: 5, ..Default::default() },
                authored: false,
            },
        });
        index.write_atomic(&w.paths.index()).unwrap();
        w
    }

    /// O0 live notes, gap 6 (T-C9-05's premise): the Bash tool's description carries the
    /// repository's lookup convention, before the model's first call, when the mod asks for it
    /// (`conventions: true`); a mod that does not ask gets the answer it always had. The answer is
    /// fixed for the session, empty in the control arm and in observe mode, and the prompt that
    /// names an item is still told the bound command.
    #[test]
    fn the_shell_tools_description_carries_the_lookup_convention_when_asked() {
        let w = delegation_world(Arm::Treated);
        let plain = standing_event(&w, "s", json!({})).unwrap();
        assert!(plain.get("conventions").is_none(), "{plain}");
        let asked = standing_event(&w, "s", json!({ "conventions": true })).unwrap();
        assert_eq!(
            asked["conventions"],
            "In this repository, work item details are looked up with python3 scripts/wi-lookup {item}, where {item} is the work item the request names."
        );
        assert_eq!((&asked["standing"], &asked["environment"]), (&plain["standing"], &plain["environment"]));
        assert_eq!(standing_event(&w, "s", json!({ "conventions": true })), Some(asked), "the same for the session");
        let p = json!({ "session_id": "s", "cwd": w.root, "hook_event_name": "UserPromptSubmit",
                        "prompt": "Please work on W-2041." });
        let opts = Options { learner: None, extension: Some(&NO_EXTENSION) };
        let told = handle("claude", "UserPromptSubmit", &p.to_string(), opts, &w.paths, Instant::now()).unwrap();
        assert!(told.is_some_and(|t| t.contains("python3 scripts/wi-lookup W-2041")), "the prompt's own directive");
        let c = delegation_world(Arm::Control);
        assert_eq!(standing_event(&c, "s", json!({ "conventions": true })).unwrap()["conventions"], "");
        let o = standing_event(&w, "o", json!({ "conventions": true, "observe": true })).unwrap();
        assert_eq!(o["conventions"], "");
    }

    /// T-C9-03's binary half, T-C9-04's: the mod's `agent.spawn` asks `Standing` with the
    /// subagent's description and prompt, and the subagent is told the lookup its prompt binds and
    /// the standing dead end, before its first request; the session's own answer is unchanged,
    /// nothing is spooled, and a control session's subagent is told nothing. Without `agent` the
    /// answer has no `subagent` field (wave one's shape).
    #[test]
    fn a_subagent_is_told_what_its_prompt_binds_in_the_treated_arm_only() {
        let w = delegation_world(Arm::Treated);
        let plain = standing_event(&w, "s", json!({})).unwrap();
        assert!(plain.get("subagent").is_none(), "{plain}");
        let agent = json!({ "agent": { "type": "Explore", "description": "Look up the work item",
                                       "prompt": "Find what W-2045 asks for and which files it names." } });
        let out = standing_event(&w, "s", agent.clone()).unwrap();
        let told = out["subagent"].as_str().unwrap();
        assert!(told.contains("python3 scripts/wi-lookup W-2045"), "{told}");
        assert!(told.contains("route=\"dead-end-pytest\""), "{told}");
        assert!(told.starts_with("<adage-reflex") && told.len() <= render::MAX_STANDING, "{told}");
        assert_eq!(
            (out["standing"].clone(), out["environment"].clone()),
            (plain["standing"].clone(), plain["environment"].clone())
        );
        assert!(!w.paths.spool("s").exists(), "read-only");
        let unbound = standing_event(&w, "s", json!({ "agent": { "prompt": "Summarise acme/billing.py" } })).unwrap();
        assert!(!unbound["subagent"].as_str().unwrap().contains("wi-lookup"), "{unbound}");
        let c = delegation_world(Arm::Control);
        assert_eq!(standing_event(&c, "s", agent.clone()).unwrap()["subagent"], "", "control: nothing");
        assert_eq!(
            standing_event(&w, "s", json!({ "agent": { "prompt": "W-2045" }, "observe": true })).unwrap()["subagent"],
            ""
        );
    }
}
