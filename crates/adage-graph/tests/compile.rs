//! The compiler (Track G §6, G3): reflexes → delivery shapes → routes. Hand-built reflexes over a
//! small corpus of traces, so each row of the shape table is checked on its own.

use adage_graph::compile::{self, CompileParams, Corpus, Input, Prior};
use adage_graph::events::{Level, NodeType};
use adage_graph::input::{self, Annotated, Prompt, Session, StepMeta};
use adage_graph::select::{ItemView, Reflex};
use adage_tracks::*;

fn argv(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// A trace: steps as (`$ cmd` | `! cmd` for a failure | `r path` | `e path`), with an effect.
fn trace(id: &str, steps: &[(&str, &str)]) -> Annotated {
    let mut out = Vec::new();
    let mut meta = Vec::new();
    for (i, (s, effect)) in steps.iter().enumerate() {
        let (tag, rest) = s.split_at(2);
        out.push(match tag {
            "$ " => input::Step::Exec { argv: argv(rest), output: "ok".into(), failed: false },
            "! " => input::Step::Exec {
                argv: argv(rest),
                output: "Traceback\nModuleNotFoundError: No module named pytest".into(),
                failed: true,
            },
            "r " => input::Step::Read { path: rest.into() },
            _ => input::Step::Edit { path: rest.into() },
        });
        meta.push(StepMeta {
            project_pytest_root_cwd: false,
            turn: i as u32,
            prompt: 0,
            effect: effect.to_string(),
            key: String::new(),
            verification: None,
        });
    }
    Annotated {
        session: Session { id: id.into(), prompts: vec![Prompt { text: "fix it".into(), at_step: 0 }], steps: out },
        meta,
        day: "2026-09-01".into(),
    }
}

fn corpus() -> Corpus {
    let mut ts = Vec::new();
    for (i, pkg) in ["ledger", "rates", "export", "ledger", "rates", "export"].iter().enumerate() {
        ts.push(trace(
            &format!("s{i}"),
            &[
                ("e registry/currencies.csv", ""),
                ("$ make gen", "regenerate"),
                ("$ make check", "verify"),
                ("! python3 -m pytest tests -v", "verify"),
                ("$ make check", "verify"),
                (&format!("e packages/{pkg}/core.py"), ""),
                (&format!("$ make -C packages/{pkg} check"), "verify"),
                ("e requirements.txt", ""),
                ("$ make deps", "environment"),
                ("$ ls", "observe"),
            ],
        ));
    }
    Corpus::of(&ts, &[])
}

fn item(ty: NodeType, level: Level, label: &str, members: &[(&str, u32)]) -> ItemView {
    ItemView {
        ty,
        level,
        label: label.into(),
        ok: (ty == NodeType::Act).then_some(true),
        last: false,
        same_batch: false,
        flow: false,
        same_art: false,
        identity: None,
        members: members.iter().map(|(k, n)| (k.to_string(), *n)).collect(),
    }
}

fn act(key: &str) -> ItemView {
    item(NodeType::Act, Level::Exact, key, &[(key, 6)])
}

fn edit(path: &str) -> ItemView {
    let mut v = item(NodeType::Edit, Level::Exact, path, &[(path, 6)]);
    v.last = true;
    v
}

fn reflex(context: Vec<ItemView>, continuation: Vec<ItemView>, target: Option<&str>) -> Reflex {
    Reflex {
        pattern: format!("{} items", context.len() + continuation.len()),
        shape: String::new(),
        context,
        continuation,
        constraints: vec![],
        support: 6,
        opportunities: 6,
        days: 1,
        conf: 0.95,
        conf_lb: 0.85,
        conf_cal: 0.95,
        conf_lb_cal: 0.85,
        lift: 3.0,
        contrast: 0.9,
        consistency: None,
        value: 1.0,
        execution_candidate: false,
        execution_only: false,
        gain: 10.0,
        exposed: 0,
        target: target.map(str::to_string),
        examples: vec!["s0".into(), "s1".into()],
        why: String::new(),
        raw: None,
        sessions: vec!["s0".into(), "s1".into()],
        lag: 0,
    }
}

fn compile(reflexes: &[Reflex]) -> compile::Output {
    let c = corpus();
    let p = CompileParams::default();
    compile::compile(&Input { reflexes, corpus: &c, values: None, priors: &[], params: &p })
}

#[test]
fn a_regeneration_after_the_last_edit_is_an_obligation_with_its_fixed_id() {
    let out =
        compile(&[reflex(vec![edit("registry/currencies.csv")], vec![act("exec make gen")], Some("exec make gen"))]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
    let c = &out.routes[0];
    assert_eq!(c.shape, "obligation");
    assert!(c.fixed_id);
    assert_eq!(c.route.id, compile::stable::finish_line_id("registry/*.csv", "make gen"));
    assert_eq!(c.route.trigger, Trigger::AfterEdit { paths: "registry/*.csv".into() });
    assert_eq!(c.route.step, Step::Run { command: "make gen".into() });
    assert_eq!(shape(&c.route), "obligation");
    assert_eq!(c.why.backoff, 1);
    assert_eq!(c.why.examples, ["s0", "s1"]);
}

#[test]
fn two_steps_after_an_edit_are_a_sequence_and_a_repeated_step_counts_once() {
    // A sequence that starts with a regeneration also owes that step: its obligation (with the
    // Stop request a sequence never makes) is compiled beside it.
    let r = reflex(
        vec![edit("registry/currencies.csv")],
        vec![act("exec make gen"), act("exec make check"), act("exec make check")],
        Some("exec make gen"),
    );
    let out = compile(&[r]);
    assert_eq!(out.routes.len(), 2, "{:?}", out.dropped);
    let o = out.routes.iter().find(|c| c.shape == "obligation").expect("the first step's obligation");
    assert_eq!(o.route.id, compile::stable::finish_line_id("registry/*.csv", "make gen"));
    let c = out.routes.iter().find(|c| c.shape == "sequence").expect("the sequence");
    assert_eq!(c.route.step, Step::Sequence { commands: vec!["make gen".into(), "make check".into()] });
    assert!(c.route.id.starts_with("sequence-"), "{}", c.route.id);
    assert_eq!(shape(&c.route), "sequence");
    let t = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "x/graph".into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: String::new(),
            },
            scope: Scope { repo: "x".into() },
            gauge: Gauge::default(),
        },
        routes: vec![c.route.clone()],
    };
    io::validate(&t).unwrap();
}

#[test]
fn a_segment_of_the_edited_path_binds_the_scope_of_a_check() {
    let mut e = item(
        NodeType::Edit,
        Level::Head,
        "packages/**/*.py",
        &[("packages/ledger/core.py", 2), ("packages/rates/core.py", 2), ("packages/export/core.py", 2)],
    );
    e.last = true;
    let target = item(NodeType::Act, Level::Exact, "exec make -C {path} check", &[("exec make -C {path} check", 6)]);
    let out = compile(&[reflex(vec![e], vec![target], Some("exec make -C {path} check"))]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
    let r = &out.routes[0].route;
    assert_eq!(r.trigger, Trigger::AfterEdit { paths: "packages/**/*.py".into() });
    assert_eq!(r.step, Step::Run { command: "make -C packages/{part} check".into() });
    assert_eq!(r.slots[0].source, SlotSource::EditSegment { glob: "packages/**/*.py".into(), depth: 1 });
    assert_eq!(r.effect, adage_core::Effect::Verify);
}

#[test]
fn an_environment_step_after_an_edit_is_a_nascent() {
    let out = compile(&[reflex(vec![edit("requirements.txt")], vec![act("exec make deps")], Some("exec make deps"))]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
    let r = &out.routes[0].route;
    assert_eq!(r.trigger, Trigger::AfterEdit { paths: "requirements.txt".into() });
    assert_eq!(
        (r.effect, r.state, r.strength),
        (adage_core::Effect::Environment, RouteState::Nascent, Strength::Prime)
    );
}

/// A check after an edit compiles as a prime run (the judge then asks that agents do not run it
/// anyway); an observation after an edit does not compile.
#[test]
fn a_check_after_an_edit_is_a_prime_run_and_an_observation_is_not_compiled() {
    let out = compile(&[
        reflex(vec![edit("registry/currencies.csv")], vec![act("exec make check")], Some("exec make check")),
        reflex(vec![edit("requirements.txt")], vec![act("exec ls")], Some("exec ls")),
    ]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.routes);
    let r = &out.routes[0];
    assert_eq!(r.shape, "run");
    assert_eq!(r.route.trigger, Trigger::AfterEdit { paths: "registry/*.csv".into() });
    assert_eq!(r.route.step, Step::Run { command: "make check".into() });
    assert_eq!((r.route.effect, r.route.strength), (adage_core::Effect::Verify, Strength::Prime));
    assert_eq!(out.dropped.len(), 1);
}

#[test]
fn a_failed_check_then_a_working_one_is_an_avoid_announced_by_how_often_it_is_run() {
    let pytest = adage_graph::events::act_key(&argv("python3 -m pytest tests -v"));
    let mut x = act(&pytest);
    x.ok = Some(false);
    let out = compile(&[reflex(vec![x], vec![act("exec make check")], Some("exec make check"))]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
    let r = &out.routes[0].route;
    // Every trace ran the runner: announced at session start, inhibit, with the miner's id.
    assert_eq!(r.trigger, Trigger::SessionStart);
    assert_eq!(
        r.step,
        Step::Avoid {
            avoid: "python3 -m pytest".into(),
            instead: "make check".into(),
            signature: "ModuleNotFoundError: No module named pytest".into()
        }
    );
    assert_eq!(r.strength, Strength::Inhibit);
    assert_eq!(r.id, compile::stable::dead_end_id("exec make check", "", 0));
    // A run the avoided runner would take is dropped before emission (interlocking).
    let mut y = act(&pytest);
    y.ok = Some(false);
    let bad = reflex(vec![act("exec make gen")], vec![act("exec make check")], Some("exec make check"));
    let out = compile(&[reflex(vec![y], vec![act("exec make check")], Some("exec make check")), bad]);
    assert!(out.routes.iter().all(|c| !matches!(c.route.trigger, Trigger::AfterCommand { .. })));
}

#[test]
fn a_longer_context_compiles_to_a_context_trigger() {
    let r = reflex(
        vec![edit("registry/currencies.csv"), act("exec make gen")],
        vec![act("exec make check")],
        Some("exec make check"),
    );
    let out = compile(&[r]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
    let c = &out.routes[0];
    assert!(matches!(c.route.trigger, Trigger::Context { within: 8, .. }), "{:?}", c.route.trigger);
    let Trigger::Context { items, .. } = &c.route.trigger else { unreachable!() };
    assert_eq!(
        items,
        &[CtxItem::Edit { glob: "registry/*.csv".into() }, CtxItem::Act { keys: vec!["exec make gen".into()] }]
    );
}

#[test]
fn a_variable_argument_with_no_bound_source_is_not_directed() {
    // `make -C packages/<p> check` after a lookup: no edit to bind the package from.
    let target = item(NodeType::Act, Level::Exact, "exec make -C {path} check", &[("exec make -C {path} check", 6)]);
    let out =
        compile(&[reflex(vec![act("exec make gen"), act("exec ls")], vec![target], Some("exec make -C {path} check"))]);
    assert!(out.routes.is_empty(), "{:?}", out.routes);
    assert!(out.dropped.iter().any(|d| d.reason.contains("variable argument")), "{:?}", out.dropped);
}

#[test]
fn an_instinct_directing_the_same_step_is_recorded_as_a_prior() {
    let prior = Route {
        id: "finish-line-inst".into(),
        summary: String::new(),
        trigger: Trigger::AfterEdit { paths: "registry/*.csv".into() },
        step: Step::Run { command: "make gen".into() },
        slots: vec![],
        effect: adage_core::Effect::Regenerate,
        strength: Strength::Prime,
        state: RouteState::Firm,
        exit: vec![],
        evidence: Evidence { provenance: "instinct:core@0.1.0".into(), ..Default::default() },
        authored: false,
    };
    let c = corpus();
    let p = CompileParams::default();
    let rs = [reflex(vec![edit("registry/currencies.csv")], vec![act("exec make gen")], Some("exec make gen"))];
    let out = compile::compile(&Input {
        reflexes: &rs,
        corpus: &c,
        values: None,
        priors: &[Prior { route: prior }],
        params: &p,
    });
    assert_eq!(out.routes[0].why.prior.as_deref(), Some("finish-line-inst"));
}

#[test]
fn a_dead_ends_signature_is_the_cause_not_makes_status_line() {
    // Regression (evals/gen, the instinct violation): a retired `make test` prints its reason, then
    // make's own `Error 1` status line. The signature is the reason: the status line is the same
    // whatever failed.
    let failing = |id: &str| {
        let mut a = trace(id, &[("$ make check", "verify")]);
        a.session.steps.insert(
            0,
            input::Step::Exec {
                argv: argv("make test"),
                output: "Exit code 2\nerror: 'make test' is retired, the suite runs with 'make check'\n\
                         make: *** [Makefile:15: test] Error 1"
                    .into(),
                failed: true,
            },
        );
        a.meta.insert(
            0,
            StepMeta {
                project_pytest_root_cwd: false,
                turn: 0,
                prompt: 0,
                effect: "verify".into(),
                key: String::new(),
                verification: None,
            },
        );
        a
    };
    let c = Corpus::of(&[failing("s0"), failing("s1")], &[]);
    let sigs: Vec<(&str, u32)> = c.acts["exec make test"].signatures.iter().map(|(s, n)| (s.as_str(), *n)).collect();
    assert_eq!(sigs, [("error: 'make test' is retired, the suite runs with 'make check'", 2)]);
}

#[test]
fn execution_only_admission_cannot_fall_back_to_advice() {
    let mut r = reflex(vec![edit("registry/currencies.csv")], vec![act("exec make gen")], Some("exec make gen"));
    r.execution_candidate = true;
    r.execution_only = true;
    // Even a positive advisory value and last-edit guard cannot erase admission provenance.
    let out = compile(&[r]);
    assert!(!out.routes.is_empty(), "{:?}", out.dropped);
    assert!(out.routes.iter().all(|c| c.execution_candidate && !c.advisory_eligible));
}

#[test]
fn an_execution_only_prefix_does_not_shadow_the_advice_of_the_same_route() {
    let advice = reflex(vec![edit("registry/currencies.csv")], vec![act("exec make gen")], Some("exec make gen"));
    let mut prefix = advice.clone();
    prefix.pattern = "0 prefix".into();
    prefix.support = 7;
    prefix.execution_candidate = true;
    prefix.execution_only = true;
    for reflexes in [[prefix.clone(), advice.clone()], [advice.clone(), prefix.clone()]] {
        let out = compile(&reflexes);
        assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
        let c = &out.routes[0];
        // The advisory reflex's route is kept, with its own advisory judgement.
        assert_eq!(c.why.pattern, advice.pattern);
        assert!(!c.execution_only && !c.execution_candidate);
    }
    // Alone, the prefix still compiles for execution judgement only.
    let out = compile(&[prefix]);
    assert!(!out.routes.is_empty() && out.routes.iter().all(|c| c.execution_only && !c.advisory_eligible));
}

/// Plan §2.1.2: a skill used after an edit compiles to the `skill` shape, `after_edit` →
/// `Step::Skill`, observe, prime at most whatever its posterior; after a selected context of two
/// events, to a context trigger; never from a single command, and never a name a directive could
/// not carry.
#[test]
fn a_skill_used_after_its_context_compiles_to_the_skill_shape() {
    let skill = |name: &str| item(NodeType::Skill, Level::Exact, name, &[(name, 6)]);
    let out = compile(&[reflex(vec![edit("registry/currencies.csv")], vec![skill("release-checklist")], None)]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
    let c = &out.routes[0];
    assert_eq!(c.shape, "skill");
    assert_eq!(c.route.step, Step::Skill { skill: "release-checklist".into(), args: None });
    assert!(matches!(c.route.trigger, Trigger::AfterEdit { .. }));
    assert_eq!((c.route.effect, c.route.strength), (adage_core::Effect::Observe, Strength::Prime));
    assert!(c.route.id.starts_with("skill-"));
    assert_eq!(adage_tracks::shape(&c.route), "skill");

    let mut r = reflex(vec![act("exec make gen"), act("exec make check")], vec![skill("release-checklist")], None);
    r.conf_lb = 0.99;
    let out = compile(&[r]);
    assert!(matches!(out.routes[0].route.trigger, Trigger::Context { .. }));
    assert_eq!(out.routes[0].route.strength, Strength::Prime, "fire is above a skill's ceiling");

    assert!(compile(&[reflex(vec![act("exec make gen")], vec![skill("release-checklist")], None)]).routes.is_empty());
    assert!(compile(&[reflex(vec![edit("registry/currencies.csv")], vec![skill("../x")], None)]).routes.is_empty());
}
