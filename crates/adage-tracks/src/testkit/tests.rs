//! The kit builds what the hand-written literals it replaces built, and what it builds is valid.

use super::kinds::{self, act, edit};
use super::*;
use crate::{io, wilson_lower, Evidence, Exit, Index, Route, RouteState, Step, Strength, Trigger};
use adage_core::spans::SpanKind;
use adage_core::Effect;
use serde_json::{json, Value};

#[test]
fn a_route_starts_firm_fired_and_bare() {
    let r = route("r", Trigger::SessionStart, Step::Run { command: "make test".into() }).build();
    let literal = Route {
        id: "r".into(),
        summary: String::new(),
        trigger: Trigger::SessionStart,
        step: Step::Run { command: "make test".into() },
        slots: vec![],
        effect: Effect::Verify,
        strength: Strength::Fire,
        state: RouteState::Firm,
        exit: vec![],
        evidence: Evidence::default(),
        authored: false,
    };
    assert_eq!(serde_json::to_value(&r).unwrap(), serde_json::to_value(&literal).unwrap());
}

#[test]
fn measured_evidence_is_what_the_learner_writes() {
    let e = route("r", Trigger::SessionStart, Step::Run { command: "x".into() }).measured(7, 8).build().evidence;
    assert_eq!((e.support, e.opportunities, e.rate, e.lower_bound), (7, 8, 0.875, wilson_lower(7, 8)));
    let none = route("r", Trigger::SessionStart, Step::Run { command: "x".into() }).measured(0, 0).build().evidence;
    assert_eq!(none.rate, 0.0);
}

#[test]
fn a_lookup_binds_every_placeholder_as_a_work_item() {
    let r = kinds::lookup("lookup", "./scripts/wi-lookup {item} --json").build();
    assert_eq!(r.trigger, Trigger::Prompt { span: SpanKind::Ticket });
    assert_eq!(r.effect, Effect::Observe);
    assert_eq!(r.slots, vec![kinds::ticket_slot("item")]);
    assert!(kinds::lookup("l", "make test").build().slots.is_empty());
}

#[test]
fn every_kind_is_one_valid_track_with_each_shape() {
    let t = track("acme/every", "acme").routes(kinds::every_kind()).validated();
    let shapes: std::collections::BTreeSet<&str> = t.routes.iter().map(crate::shape).collect();
    assert_eq!(shapes, ["avoid", "obligation", "run", "sequence"].into(), "{shapes:?}");
    let ids: std::collections::BTreeSet<&str> = t.routes.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids.len(), t.routes.len(), "ids do not collide");
    let index = Index::compile(&[t]);
    assert_eq!(index.routes.len(), kinds::every_kind().len());
    assert!(!index.contexts("acme").is_empty(), "the context route gives the repository a net");
}

#[test]
#[should_panic(expected = "is not valid")]
fn an_invalid_track_fails_where_it_is_built() {
    let _ = track("acme/bad", "acme")
        .route(route("Bad Id!", Trigger::SessionStart, Step::Run { command: "x".into() }))
        .validated();
}

#[test]
fn a_hand_written_index_keeps_its_order_and_generation() {
    let i = IndexBuilder::new("g")
        .route("acme", "acme/a", kinds::finish_line("b", "src/*.rs", "make gen"))
        .versioned("acme", "acme/a", "0.2.0", kinds::context("a", vec![act("exec make"), edit("x/*")], 3, "make"))
        .build();
    assert_eq!(i.generation, "g");
    let ids: Vec<_> = i.routes.iter().map(|r| (r.route.id.as_str(), r.version.as_str())).collect();
    assert_eq!(ids, [("b", "0.1.0"), ("a", "0.2.0")]);
}

#[test]
fn a_track_carries_its_manifest() {
    let t = track("acme/work", "acme").version("1.2.0").description("d").provenance("authored").build();
    assert_eq!(
        (
            t.manifest.track.version.as_str(),
            t.manifest.track.description.as_str(),
            t.manifest.track.provenance.as_str()
        ),
        ("1.2.0", "d", "authored")
    );
    assert_eq!(t.manifest.scope.repo, "acme");
}

#[test]
fn a_learned_home_holds_its_index_tracks_state_and_spools() {
    let w = TempWorld::new(&["repo/.git"]);
    let home = w.home("home").without_auto_learn();
    assert!(std::fs::read_to_string(home.paths.config()).unwrap().contains("auto = false"));
    let t = track("acme/work", "acme")
        .route(kinds::finish_line("gen", "schema/*.json", "make gen").until_intent_changes())
        .build();
    home.install("acme/work", &t);
    let compiled = home.recompile();
    assert_eq!(serde_json::to_string(&compiled).unwrap(), serde_json::to_string(&home.loaded()).unwrap());
    assert_eq!(home.loaded().routes[0].route.exit, vec![Exit::IntentChanges]);
    assert_eq!(io::read(&home.paths.tracks().join("acme/work")).unwrap(), t);

    // The state seeder is generic over the runtime's type; a JSON value stands in for it here.
    home.seed::<Value>("s", |v| *v = json!({ "arm": "treated" }));
    home.seed::<Value>("s", |v| v["prompts"] = json!(1));
    assert_eq!(home.state::<Value>("s"), json!({ "arm": "treated", "prompts": 1 }));
    assert_eq!(home.state::<Value>("never"), Value::Null);

    assert!(home.spooled("s").is_empty());
    home.spool("s", &json!({ "kind": "pretool", "n": 1 })).spool("s", &json!({ "kind": "batch" }));
    assert_eq!(home.spooled("s").len(), 2);
    assert_eq!(home.spooled_kind("s", "pretool"), vec![json!({ "kind": "pretool", "n": 1 })]);
}

#[test]
fn a_temp_world_writes_repositories_files_and_tools() {
    let w = TempWorld::new(&["a/b"]);
    assert!(w.p("a/b").is_dir() && w.root.is_absolute());
    assert!(w.repo("acme").join(".git").is_dir());
    assert_eq!(std::fs::read_to_string(w.write("x/y/z.txt", "hi")).unwrap(), "hi");
    let tool = w.executable("bin/make", "#!/bin/sh\nexit 1\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(tool).unwrap().permissions().mode() & 0o777, 0o755);
    }
    #[cfg(not(unix))]
    assert!(tool.exists());
}
