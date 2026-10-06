//! The `skill` step (plan D3, test plan T-D3-01): its YAML form, its shape and ceiling, and what
//! validation refuses. A track is untrusted input; a skill name reaches the directive, the
//! canonical key and the Skill tool's input, so only a plain name passes.

use adage_core::Effect;
use adage_tracks::*;

fn track(trigger: Trigger, step: Step, slots: Vec<Slot>, effect: Effect, strength: Strength) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "acme/t".into(),
                version: "1.0.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes: vec![Route {
            id: "release".into(),
            summary: String::new(),
            trigger,
            step,
            slots,
            effect,
            strength,
            state: RouteState::Firm,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence::default(),
            authored: false,
        }],
    }
}

fn skill(name: &str) -> Step {
    Step::Skill { skill: name.into(), args: None }
}

fn edit() -> Trigger {
    Trigger::AfterEdit { paths: "CHANGELOG.md".into() }
}

fn intent() -> Trigger {
    // A unit centroid in eight dimensions.
    let mut centroid = vec![0i16; 8];
    centroid[0] = 10_000;
    Trigger::Intent { family: "cut-a-release".into(), model: "minilm@0123456789ab".into(), centroid, threshold: 0.8 }
}

fn context() -> Trigger {
    Trigger::Context {
        items: vec![CtxItem::Act { keys: vec!["exec make test".into()] }, CtxItem::Edit { glob: "VERSION".into() }],
        within: 4,
    }
}

#[test]
fn a_skill_step_round_trips_through_yaml_with_its_shape_and_ceiling() {
    let t = track(edit(), skill("release-checklist"), vec![], Effect::Observe, Strength::Prime);
    let r = &t.routes[0];
    let yaml = serde_yaml::to_string(r).unwrap();
    assert!(yaml.contains("do: skill\n  skill: release-checklist\n"), "{yaml}");
    assert!(!yaml.contains("args"), "no args, no key: {yaml}");
    assert_eq!(&serde_yaml::from_str::<Route>(&yaml).unwrap(), r);
    let with_args = Step::Skill { skill: "release-checklist".into(), args: Some("minor".into()) };
    let yaml = serde_yaml::to_string(&with_args).unwrap();
    assert_eq!(serde_yaml::from_str::<Step>(&yaml).unwrap(), with_args);
    assert_eq!(shape(r), "skill");
    assert_eq!(SHAPES.len(), 8);
    assert!(SHAPES.contains(&"skill"));
    assert_eq!(SKILL_CEILING, Strength::Prime);
    for effect in [Effect::Observe, Effect::Verify, Effect::Regenerate] {
        assert_eq!(step_ceiling(&r.step, effect, true), Some(Strength::Prime), "{effect:?}");
    }
    assert_eq!(step_ceiling(&r.step, Effect::Publish, true), None);
    assert_eq!(skill_key("release-checklist"), "control Skill release-checklist");
    assert_eq!(r.gist(), "after editing CHANGELOG.md, use the release-checklist skill");
    io::validate(&t).unwrap();
}

#[test]
fn a_skill_follows_a_prompt_an_intent_an_edit_or_a_context() {
    let prompt = Trigger::Prompt { span: adage_core::spans::SpanKind::Ticket };
    for trigger in [prompt, intent(), edit(), context()] {
        let t = track(trigger.clone(), skill("release-checklist"), vec![], Effect::Observe, Strength::Prime);
        io::validate(&t).unwrap_or_else(|e| panic!("{trigger:?}: {e:#}"));
    }
    for trigger in [
        Trigger::SessionStart,
        Trigger::AfterCommand { command: "exec make test".into() },
        Trigger::AfterFailure { command: "exec make test".into(), signature: "x".into() },
    ] {
        let t = track(trigger.clone(), skill("release-checklist"), vec![], Effect::Observe, Strength::Prime);
        assert!(io::validate(&t).is_err(), "{trigger:?} must be refused");
    }
}

#[test]
fn validation_refuses_what_a_skill_step_may_not_carry() {
    let ok = |step: Step| track(edit(), step, vec![], Effect::Observe, Strength::Prime);
    for name in ["release-checklist", "Release_Checklist2", "a", &"x".repeat(64)] {
        io::validate(&ok(skill(name))).unwrap_or_else(|e| panic!("{name}: {e:#}"));
    }
    for name in ["../x", "", &"x".repeat(65), "a b", "plugin:skill", "x/y", "`x`", "x\n", "é"] {
        assert!(io::validate(&ok(skill(name))).is_err(), "{name:?} must be refused");
    }
    for args in ["", " minor", "a;b", "$(x)", "{slot}", "a\nb", "\"x\"", &"a".repeat(161)] {
        let step = Step::Skill { skill: "release-checklist".into(), args: Some(args.into()) };
        assert!(io::validate(&ok(step)).is_err(), "args {args:?} must be refused");
    }
    io::validate(&ok(Step::Skill { skill: "release-checklist".into(), args: Some("minor --dry-run".into()) })).unwrap();
    // No slots: nothing is bound into a skill's use.
    let slot = Slot {
        name: "item".into(),
        source: SlotSource::EditSegment { glob: "CHANGELOG.md".into(), depth: 0 },
        contract: Contract::Pattern { regex: "^[a-z]+$".into() },
    };
    assert!(io::validate(&track(edit(), skill("x"), vec![slot], Effect::Observe, Strength::Prime)).is_err());
    // Never a dead end, never above prime, always observe.
    assert!(io::validate(&track(edit(), skill("x"), vec![], Effect::Observe, Strength::Inhibit)).is_err());
    assert!(io::validate(&track(edit(), skill("x"), vec![], Effect::Observe, Strength::Fire)).is_err());
    assert!(io::validate(&track(edit(), skill("x"), vec![], Effect::Verify, Strength::Prime)).is_err());
    // A skill is never carried out by routine execution.
    let mut t = ok(skill("x"));
    t.routes[0].evidence.execution = Some(ExecutionEvidence { opportunities: 3, successful: 3, sessions: 3 });
    assert!(io::validate(&t).is_err());
}

#[test]
fn an_index_with_a_skill_step_carries_the_format_that_says_so() {
    let t = track(edit(), skill("release-checklist"), vec![], Effect::Observe, Strength::Prime);
    let index = Index::compile(&[t]);
    assert_eq!(index.format, index::INDEX_FORMAT);
    // The format that may hold a skill step (an older binary cannot read one).
    const _: () = assert!(index::INDEX_FORMAT >= 1);
    let back: Index = serde_json::from_slice(&serde_json::to_vec(&index).unwrap()).unwrap();
    assert_eq!(back, index);
    assert!(matches!(back.routes[0].route.step, Step::Skill { .. }));
}
