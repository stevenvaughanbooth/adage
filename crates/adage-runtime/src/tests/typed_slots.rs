//! Learned value types (Track G §4.3): prompt values, output values and edit segments.

use super::*;
use adage_core::vtype::{Class, Run, Runs};

/// `#482`-style trackers: three digits.
fn digits3() -> ValueType {
    let runs = Runs { branches: vec![vec![Run::Class { class: Class::D, lo: 3, hi: 3 }]] };
    ValueType {
        id: "vt-0123456789abcdef".into(),
        regex: adage_core::vtype::to_regex(&runs),
        runs,
        samples_hash: String::new(),
    }
}

fn crate_name() -> ValueType {
    let runs =
        Runs { branches: vec![vec![Run::Lit { text: "pkg".into() }, Run::Class { class: Class::D, lo: 1, hi: 2 }]] };
    ValueType {
        id: "vt-00000000000000aa".into(),
        regex: adage_core::vtype::to_regex(&runs),
        runs,
        samples_hash: String::new(),
    }
}

fn slot(name: &str, source: SlotSource, t: &ValueType) -> Slot {
    Slot { name: name.into(), source, contract: Contract::Pattern { regex: t.regex.clone() } }
}

fn track(routes: Vec<Route>) -> Track {
    Track {
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
    }
}

fn routes() -> Vec<Route> {
    let t = digits3();
    let c = crate_name();
    vec![
        route(
            "lookup-work-item",
            Trigger::Prompt { span: SpanKind::Ticket },
            Step::Run { command: "./scripts/wi-lookup {item}".into() },
            vec![slot("item", SlotSource::PromptValue { vtype: t.clone(), mentions: vec![] }, &t)],
            Effect::Observe,
            Strength::Fire,
        ),
        route(
            "show-owner",
            Trigger::AfterCommand { command: "exec scripts/wi-lookup {arg}".into() },
            Step::Run { command: "./scripts/wi-lookup {id} --json".into() },
            vec![slot("id", SlotSource::OutputValue { field: Some("owner.id".into()), vtype: t.clone() }, &t)],
            Effect::Observe,
            Strength::Prime,
        ),
        route(
            "test-crate",
            Trigger::AfterEdit { paths: "crates/*/src/*.rs".into() },
            Step::Run { command: "cargo test -p {p}".into() },
            vec![slot("p", SlotSource::EditSegment { glob: "crates/*/src/*.rs".into(), depth: 1 }, &c)],
            Effect::Verify,
            Strength::Fire,
        ),
    ]
}

#[test]
fn typed_tracks_validate_and_bad_ones_do_not() {
    adage_tracks::io::validate(&track(routes())).unwrap();
    // Runs and regex that disagree.
    let mut bad = routes();
    if let SlotSource::PromptValue { vtype, .. } = &mut bad[0].slots[0].source {
        vtype.regex = "^(?:[0-9]{4})$".into();
    }
    bad[0].slots[0].contract = Contract::Pattern { regex: "^(?:[0-9]{4})$".into() };
    assert!(adage_tracks::io::validate(&track(bad)).unwrap_err().to_string().contains("disagree"));
    // A contract that is not the type's regex.
    let mut bad = routes();
    bad[0].slots[0].contract = Contract::Pattern { regex: "^[0-9]{3}$".into() };
    assert!(adage_tracks::io::validate(&track(bad)).is_err());
    // A prompt value is not bound by an after-command trigger, nor an edit segment by a prompt.
    let mut bad = routes();
    bad[1].slots[0].source = SlotSource::PromptValue { vtype: digits3(), mentions: vec![] };
    assert!(adage_tracks::io::validate(&track(bad)).unwrap_err().to_string().contains("cannot bind"));
    let mut bad = routes();
    bad[0].slots[0].source = SlotSource::EditSegment { glob: "a/*".into(), depth: 0 };
    assert!(adage_tracks::io::validate(&track(bad)).is_err());
    // A typed route round-trips through YAML.
    let y = serde_json::to_string(&routes()[0]).unwrap();
    let back: Route = serde_json::from_str(&y).unwrap();
    assert_eq!(back, routes()[0]);
}

fn ctx<'a>(index: &'a Index, root: &'a Path, table: &'a CommandTable) -> Ctx<'a> {
    Ctx {
        checkout: &crate::checkout::Disk,
        index,
        repo: "acme".into(),
        root,
        table,
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    }
}

#[test]
fn prompt_values_bind_the_first_accepted_token() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let index = Index::compile(&[track(routes())]);
    let c = ctx(&index, dir.path(), table);
    let mut st = SessionState::default();
    let d = decide(&c, &mut st, &Event::Prompt { text: "Please close out 12 things, starting with #482.".into() });
    assert_eq!(d.directives.len(), 1, "{d:?}");
    assert!(d.directives[0].text.contains("./scripts/wi-lookup 482"), "{}", d.directives[0].text);
    assert!(d.directives[0].text.contains("work item details"));
    // Hostile neighbours never bind: only a whole token the type accepts does.
    for text in ["run -482 now", "run 482;id", "try 4821 or $(id)", "x`482`y", "a 48 b"] {
        let mut st = SessionState::default();
        let d = decide(&c, &mut st, &Event::Prompt { text: text.into() });
        for x in &d.directives {
            assert!(x.text.contains("wi-lookup 482\n"), "{text}: {}", x.text);
        }
    }
    // Followed like any directed command.
    let d = decide(&c, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup 482", "{}")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Followed { .. })), "{d:?}");
    // Learned mentions: only `#482` names an item, not a stray number.
    let mut rs = routes();
    if let SlotSource::PromptValue { mentions, .. } = &mut rs[0].slots[0].source {
        mentions.push(adage_core::tokens::Mention { prefix: "#".into(), suffix: String::new() });
    }
    adage_tracks::io::validate(&track(rs.clone())).unwrap();
    let index = Index::compile(&[track(rs)]);
    let c = ctx(&index, dir.path(), table);
    let mut st = SessionState::default();
    let d = decide(&c, &mut st, &Event::Prompt { text: "must return 120, see #482".into() });
    assert!(d.directives[0].text.contains("wi-lookup 482\n"), "{d:?}");
    let mut st = SessionState::default();
    assert!(decide(&c, &mut st, &Event::Prompt { text: "must return 120".into() }).directives.is_empty());
}

#[test]
fn output_values_and_edit_segments_bind() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let index = Index::compile(&[track(routes())]);
    let c = ctx(&index, dir.path(), table);
    let mut st = SessionState::default();
    let out = r#"{"id": "482", "owner": {"id": 731, "name": "x"}}"#;
    let d = decide(&c, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup 482", out)] });
    let texts: Vec<&str> = d.directives.iter().map(|x| x.text.as_str()).collect();
    assert!(texts.iter().any(|t| t.contains("./scripts/wi-lookup 731 --json")), "{texts:?}");
    // No value of the type in the field: no directive.
    let mut st = SessionState::default();
    let bad = r#"{"owner": {"id": "-1"}}"#;
    let d = decide(&c, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup 482", bad)] });
    assert!(d.directives.iter().all(|x| x.route != "show-owner"));
    // An edit in crates/pkg7 directs and obliges `cargo test -p pkg7`.
    let edit = |p: &str| Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join(p).to_string_lossy(), "old_string": "a", "new_string": "b" }),
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
        agent: None,
    };
    let mut st = SessionState::default();
    let d = decide(&c, &mut st, &Event::Batch { calls: vec![edit("crates/pkg7/src/lib.rs")] });
    assert!(d.directives.iter().any(|x| x.text.contains("cargo test -p pkg7")), "{d:?}");
    assert_eq!(st.obligations[0].command, "cargo test -p pkg7");
    let d = decide(&c, &mut st, &Event::Batch { calls: vec![bash("cargo test -p pkg7", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::ObligationMet { .. })), "{d:?}");
    // A segment the contract refuses binds nothing.
    let mut st = SessionState::default();
    let d = decide(&c, &mut st, &Event::Batch { calls: vec![edit("crates/evil/src/lib.rs")] });
    assert!(d.directives.is_empty() && st.obligations.is_empty(), "{d:?}");
}

#[test]
fn the_fast_path_skips_only_prompts_no_type_accepts() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let index = Index::compile(&[track(routes())]);
    let t = index.triggers("acme");
    assert_eq!(t.prompt_types.len(), 1);
    assert!(t.prompt_spans.is_empty(), "a typed slot does not need the span kind");
    let c = ctx(&index, dir.path(), table);
    let prompted = || Some(SessionState { prompts: 1, ..Default::default() });
    for (text, skip) in [("look at #482", false), ("look at ops/482", false), ("look at W-2042", true), ("hi", true)] {
        assert_eq!(fast::skip(&t, &Event::Prompt { text: text.into() }, "/r", prompted), skip, "{text}");
        // Skipping changes no decision.
        let mut st = SessionState { prompts: 1, ..Default::default() };
        let d = decide(&c, &mut st, &Event::Prompt { text: text.into() });
        assert_eq!(d.directives.is_empty(), skip, "{text}");
    }
}
