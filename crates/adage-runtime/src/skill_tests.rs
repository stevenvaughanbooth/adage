//! The `skill` step in the runtime (plan D3, test plan T-D3-02 and T-D3-03, and the `skill`
//! prediction kind reached from a real route): its fixed template and what reading it back
//! names, its delivery at each stimulus a skill may follow, its follow judgement on the Skill
//! tool, and its invocation at a `TurnStep` only as the policy allows.

use super::*;
use adage_core::spans::SpanKind;
use adage_core::{CommandTable, Effect};
use adage_tracks::*;
use serde_json::json;

const SKILL: &str = "release-checklist";

fn route(id: &str, trigger: Trigger, step: Step) -> Route {
    Route {
        id: id.into(),
        summary: String::new(),
        trigger,
        step,
        slots: vec![],
        effect: Effect::Observe,
        strength: Strength::Prime,
        state: RouteState::Firm,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { support: 5, ..Default::default() },
        authored: false,
    }
}

fn skill(name: &str) -> Step {
    Step::Skill { skill: name.into(), args: None }
}

fn index(routes: Vec<Route>) -> Index {
    let t = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "acme/t".into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes,
    };
    io::validate(&t).unwrap();
    Index::compile(&[t])
}

fn ctx<'a>(dir: &'a tempfile::TempDir, index: &'a Index, table: &'a CommandTable) -> Ctx<'a> {
    Ctx {
        checkout: &crate::checkout::Disk,
        index,
        repo: "acme".into(),
        root: dir.path(),
        table,
        embed: None,
        params: Default::default(),
        policy: Policy::default(),
    }
}

fn call(tool: &str, input: serde_json::Value) -> Call {
    Call {
        tool: tool.into(),
        input,
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
        agent: None,
    }
}

fn use_skill(name: &str) -> Call {
    call("Skill", json!({ "skill": name }))
}

fn edit(dir: &tempfile::TempDir, f: &str) -> Call {
    call("Edit", json!({ "file_path": dir.path().join(f).to_string_lossy() }))
}

fn step(index: u32) -> Event {
    Event::TurnStep { turn_id: "t".into(), index, model: None, agent: None, ending: false }
}

fn after_edit() -> Route {
    route("release", Trigger::AfterEdit { paths: "CHANGELOG.md".into() }, skill(SKILL))
}

/// T-D3-02: the fixed template, and the name read back as a skill, never as a command.
#[test]
fn the_skill_template_names_the_skill_and_no_command() {
    let r = after_edit();
    let body = render::route_text(&r, &[]).unwrap();
    assert_eq!(
        body,
        "In this repository the `release-checklist` skill is used at this point. Ignore this if the user is asking for something else."
    );
    let n = render::named(&body);
    assert_eq!(n.skills, [SKILL]);
    assert!(n.commands.is_empty() && n.avoided.is_empty() && n.files.is_empty(), "{n:?}");
    // Named something, so a session that received it is steered by that skill alone (`all`
    // false).
    assert!(!n.is_empty());
    let with_args = Route { step: Step::Skill { skill: SKILL.into(), args: Some("minor".into()) }, ..r.clone() };
    let body = render::route_text(&with_args, &[]).unwrap();
    assert!(body.contains("skill is used at this point, with the arguments minor. Ignore"), "{body}");
    assert_eq!(render::named(&body).skills, [SKILL]);
    // Inside the wrapper as delivered: `kind="skill"`, prime.
    let d = Directive {
        route: r.id.clone(),
        track: "acme/t".into(),
        version: "0.1.0".into(),
        strength: Strength::Prime,
        text: body,
        kind: shape(&r).into(),
        origin: None,
    };
    assert!(render::wrap_directive(&d).contains("strength=\"prime\" kind=\"skill\""));
    // A name that could not have passed validation is never rendered.
    for bad in ["../x", "a`b", ""] {
        assert!(render::route_text(&Route { step: skill(bad), ..r.clone() }, &[]).is_none(), "{bad:?}");
    }
    let bad_args = Route { step: Step::Skill { skill: SKILL.into(), args: Some("a\u{1b}[1m".into()) }, ..r };
    assert!(render::route_text(&bad_args, &[]).is_none());
}

/// T-D3-03: a Skill tool use of the same skill within the window is a follow; another skill is
/// not, and the directive is ignored once its window passes.
#[test]
fn a_skill_directive_is_followed_by_using_that_skill() {
    let dir = tempfile::tempdir().unwrap();
    let (index, table) = (index(vec![after_edit()]), CommandTable::load());
    let ctx = ctx(&dir, &index, table);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit(&dir, "CHANGELOG.md")] });
    assert_eq!(d.directives.len(), 1, "{d:?}");
    assert_eq!((d.directives[0].kind.as_str(), d.directives[0].strength), ("skill", Strength::Prime));
    assert_eq!(st.pending[0].expect_key, "control Skill release-checklist");
    assert_eq!(st.pending[0].command, SKILL);
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![use_skill(SKILL)] });
    assert!(matches!(&d.judgements[..], [Judgement::Followed { route }] if route == "release"), "{d:?}");
    assert!(st.pending.is_empty());
    // Said once a session.
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit(&dir, "CHANGELOG.md")] });
    assert!(d.directives.is_empty());

    let mut st = SessionState::default();
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit(&dir, "CHANGELOG.md")] });
    let mut judged = Vec::new();
    for _ in 0..ctx.params.follow_window {
        let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![use_skill("other-skill")] });
        judged.extend(d.judgements);
    }
    assert!(matches!(&judged[..], [Judgement::Ignored { route }] if route == "release"), "{judged:?}");
    // A Skill use the mod made in the agent's place is predicted, never a follow.
    let mut st = SessionState::default();
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit(&dir, "CHANGELOG.md")] });
    let adage = Call { by: By::Adage, tool_use_id: Some("adage_release_t_1_0".into()), ..use_skill(SKILL) };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![adage] });
    assert!(matches!(&d.judgements[..], [Judgement::Predicted { route }] if route == "release"), "{d:?}");
}

/// A skill step follows a prompt carrying a span of its kind, or a learned context, and is
/// judged the same way; nothing is bound into it.
#[test]
fn a_skill_follows_a_prompt_span_and_a_context() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let on_ticket = route("ticket-skill", Trigger::Prompt { span: SpanKind::Ticket }, skill("work-item"));
    let ctx_skill = route(
        "ctx-skill",
        Trigger::Context {
            items: vec![CtxItem::Act { keys: vec!["exec make test".into()] }, CtxItem::Edit { glob: "VERSION".into() }],
            within: 4,
        },
        skill(SKILL),
    );
    let index = index(vec![on_ticket, ctx_skill]);
    let ctx = ctx(&dir, &index, table);
    let mut st = SessionState::default();
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "tidy the readme".into() });
    assert!(d.would_have.is_empty(), "no ticket, no skill: {d:?}");
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "Work on W-2042 please".into() });
    assert_eq!(d.would_have.len(), 1, "{d:?}");
    assert!(d.would_have[0].text.contains("`work-item` skill"));
    assert_eq!(st.pending[0].expect_key, "control Skill work-item");
    let bash = call("Bash", json!({ "command": "make test" }));
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash] });
    assert!(d.would_have.is_empty());
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit(&dir, "VERSION")] });
    assert_eq!(d.would_have.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["ctx-skill"], "{d:?}");
    assert!(st.pending.iter().any(|p| p.route == "ctx-skill" && p.command == SKILL));
}

/// The `skill` prediction kind from a real route: only with the route's `predict` grant and the
/// skill in the policy's invocable set (inert, accepted, `skills.invoke` on); its arguments go
/// with it; the control arm records `would_predict` and invokes nothing.
#[test]
fn a_skill_step_is_invoked_only_as_the_policy_allows() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let with_args = route(
        "release",
        Trigger::AfterEdit { paths: "CHANGELOG.md".into() },
        Step::Skill { skill: SKILL.into(), args: Some("minor".into()) },
    );
    let index = index(vec![with_args]);
    let granted = |invoke: &[&str]| {
        let mut c = ctx(&dir, &index, table);
        c.policy.predict_grants = vec![("acme/t".into(), "release".into())];
        c.policy.invoke_skills = invoke.iter().map(|s| s.to_string()).collect();
        c
    };
    let delivered = |c: &Ctx, arm: Arm| {
        let mut st = SessionState { arm: Some(arm), ..Default::default() };
        decide(c, &mut st, &Event::Batch { calls: vec![edit(&dir, "CHANGELOG.md")] });
        st
    };
    // Granted, but the skill is not invocable (not inert, not accepted, or skills.invoke off).
    let c = granted(&[]);
    let mut st = delivered(&c, Arm::Treated);
    let d = decide(&c, &mut st, &step(1));
    assert!(d.predicted.is_none() && d.would_predict.is_none(), "{d:?}");
    // Invocable, but no grant on the route.
    let mut c = granted(&[SKILL]);
    c.policy.predict_grants.clear();
    let mut st = delivered(&c, Arm::Treated);
    assert!(decide(&c, &mut st, &step(1)).predicted.is_none());
    // Both: the Skill tool with the step's arguments.
    let c = granted(&[SKILL]);
    let mut st = delivered(&c, Arm::Treated);
    let p = decide(&c, &mut st, &step(1)).predicted.expect("a skill prediction");
    assert_eq!((p.kind, p.route.as_str(), p.track.as_str()), (PredictionKind::Skill, "release", "acme/t"));
    assert_eq!(p.calls, [PredictedCall { tool: "Skill".into(), input: json!({ "skill": SKILL, "args": "minor" }) }]);
    // The control arm: recorded, never invoked.
    let mut st = delivered(&c, Arm::Control);
    let d = decide(&c, &mut st, &step(1));
    assert!(d.predicted.is_none());
    assert_eq!(d.would_predict.map(|p| p.kind), Some(PredictionKind::Skill));
    // Another skill's name in the invocable set invokes nothing.
    let c = granted(&["other-skill"]);
    let mut st = delivered(&c, Arm::Treated);
    assert!(decide(&c, &mut st, &step(1)).would_predict.is_none());
}
