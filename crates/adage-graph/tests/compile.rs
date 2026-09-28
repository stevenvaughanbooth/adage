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
        meta.push(StepMeta { turn: i as u32, prompt: 0, effect: effect.to_string(), key: String::new() });
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
        gain: 10.0,
        exposed: 0,
        target: target.map(str::to_string),
        examples: vec!["s0".into(), "s1".into()],
        why: String::new(),
        raw: None,
    }
}

fn compile(reflexes: &[Reflex]) -> compile::Output {
    let c = corpus();
    let p = CompileParams::default();
    compile::compile(&Input { reflexes, corpus: &c, values: None, priors: &[], params: &p })
}

#[test]
fn a_regeneration_after_the_last_edit_is_an_obligation_with_the_miners_id() {
    let out =
        compile(&[reflex(vec![edit("registry/currencies.csv")], vec![act("exec make gen")], Some("exec make gen"))]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
    let c = &out.routes[0];
    assert_eq!(c.shape, "obligation");
    assert!(c.legacy);
    assert_eq!(c.route.id, compile::legacy::finish_line_id("registry/*.csv", "make gen"));
    assert_eq!(c.route.trigger, Trigger::AfterEdit { paths: "registry/*.csv".into() });
    assert_eq!(c.route.step, Step::Run { command: "make gen".into() });
    assert_eq!(compile::legacy_track(&c.route), KIND_FINISH_LINE);
    assert_eq!(c.why.backoff, 1);
    assert_eq!(c.why.examples, ["s0", "s1"]);
}

#[test]
fn two_steps_after_an_edit_are_a_sequence_and_a_repeated_step_counts_once() {
    let r = reflex(
        vec![edit("registry/currencies.csv")],
        vec![act("exec make gen"), act("exec make check"), act("exec make check")],
        Some("exec make gen"),
    );
    let out = compile(&[r]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
    let c = &out.routes[0];
    assert_eq!(c.shape, "sequence");
    assert_eq!(c.route.step, Step::Sequence { commands: vec!["make gen".into(), "make check".into()] });
    assert!(c.route.id.starts_with("sequence-"), "{}", c.route.id);
    assert_eq!(compile::legacy_track(&c.route), "graph");
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
fn an_environment_step_after_an_edit_is_a_draft() {
    let out = compile(&[reflex(vec![edit("requirements.txt")], vec![act("exec make deps")], Some("exec make deps"))]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
    let r = &out.routes[0].route;
    assert_eq!(r.trigger, Trigger::AfterEdit { paths: "requirements.txt".into() });
    assert_eq!((r.effect, r.state, r.aspect), (adage_core::Effect::Environment, RouteState::Draft, Aspect::Yellow));
}

#[test]
fn a_check_or_an_observation_after_an_edit_is_not_compiled() {
    let out = compile(&[
        reflex(vec![edit("registry/currencies.csv")], vec![act("exec make check")], Some("exec make check")),
        reflex(vec![edit("requirements.txt")], vec![act("exec ls")], Some("exec ls")),
    ]);
    assert!(out.routes.is_empty(), "{:?}", out.routes);
    assert_eq!(out.dropped.len(), 2);
}

#[test]
fn a_failed_check_then_a_working_one_is_an_avoid_announced_by_how_often_it_is_run() {
    let pytest = adage_graph::events::act_key(&argv("python3 -m pytest tests -v"));
    let mut x = act(&pytest);
    x.ok = Some(false);
    let out = compile(&[reflex(vec![x], vec![act("exec make check")], Some("exec make check"))]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
    let r = &out.routes[0].route;
    // Every trace ran the runner: announced at session start, red, with the miner's id.
    assert_eq!(r.trigger, Trigger::SessionStart);
    assert_eq!(
        r.step,
        Step::Avoid {
            avoid: "python3 -m pytest".into(),
            instead: "make check".into(),
            signature: "ModuleNotFoundError: No module named pytest".into()
        }
    );
    assert_eq!(r.aspect, Aspect::Red);
    assert_eq!(r.id, compile::legacy::dead_end_id("exec make check", "", 0));
    // A run the avoided runner would take is dropped before emission (interlocking).
    let mut y = act(&pytest);
    y.ok = Some(false);
    let bad = reflex(vec![act("exec make gen")], vec![act("exec make check")], Some("exec make check"));
    let out = compile(&[reflex(vec![y], vec![act("exec make check")], Some("exec make check")), bad]);
    assert!(out.routes.iter().all(|c| !matches!(c.route.trigger, Trigger::AfterCommand { .. })));
}

#[test]
fn a_longer_context_compiles_to_an_inert_context_trigger() {
    let r = reflex(
        vec![edit("registry/currencies.csv"), act("exec make gen")],
        vec![act("exec make check")],
        Some("exec make check"),
    );
    let out = compile(&[r]);
    assert_eq!(out.routes.len(), 1, "{:?}", out.dropped);
    let c = &out.routes[0];
    assert!(matches!(c.route.trigger, Trigger::Context { within: 8, .. }), "{:?}", c.route.trigger);
    assert!(!compile::fires_in_g3(&c.route));
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
        aspect: Aspect::Yellow,
        state: RouteState::Active,
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
