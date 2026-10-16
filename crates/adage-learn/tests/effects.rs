//! The effect of a repository lookup script, from the traces live run 2 learned it from
//! (`fixtures/lookup-traces-live2.json`: the twelve sessions that formed `lookup-work-item` on the
//! acme-billing sandbox, as the learner imported them). Two of them looked up W-1046, whose text
//! reads "generated models include it"; the learner took that line of the work item for the
//! script saying it wrote something and kept the route `environment`, so it could not be granted
//! predict. Every session read `scripts/wi-lookup` first, and its text writes nothing.

use adage_core::Effect;
use adage_learn::effects::{effect_of, infer};
use adage_trace::Session;

const COMMAND: &str = "python3 scripts/wi-lookup {item}";

fn sessions() -> Vec<Session> {
    serde_json::from_str(include_str!("fixtures/lookup-traces-live2.json")).expect("the recorded traces")
}

#[test]
fn the_lookup_is_observe_from_its_command_and_script_not_a_work_items_prose() {
    let ss = sessions();
    assert_eq!(ss.len(), 12);
    // The recorded excerpt still carries the prose that misled the learner.
    let prose =
        ss.iter().flat_map(|s| &s.steps).filter(|st| st.call.output.contains("generated models include it")).count();
    assert!(prose >= 2, "{prose}");

    let refs: Vec<&Session> = ss.iter().collect();
    let (effect, note) = effect_of(Effect::Environment, COMMAND, &refs);
    assert_eq!(effect, Effect::Observe);
    let note = note.expect("why");
    assert!(
        note.starts_with("scripts/wi-lookup is not in the command table; classified observe from 11 traces"),
        "{note}"
    );
    assert!(note.ends_with("its text uses no network or process library and writes no file"), "{note}");
}

#[test]
fn without_the_scripts_text_a_change_report_still_keeps_it_environment() {
    // The same traces with every sight of the script's text removed, and one run that opens a line
    // with what it wrote: that is the script speaking, not the work item.
    let mut ss = sessions();
    for s in &mut ss {
        s.steps.retain(|st| st.rel_path.as_deref() != Some("scripts/wi-lookup"));
    }
    let refs: Vec<&Session> = ss.iter().collect();
    assert!(infer(COMMAND, &refs).is_some(), "the prose alone does not count");

    let run = ss[2].steps.iter_mut().find(|st| st.call.output.starts_with("ID: W-")).expect("a lookup's output");
    run.call.output.push_str("\nWrote .tracker/last-lookup.json\n");
    let refs: Vec<&Session> = ss.iter().collect();
    assert!(infer(COMMAND, &refs).is_none());
}

#[test]
fn concatenated_cat_output_is_not_an_individual_scripts_source() {
    let mut ss = sessions();
    for s in &mut ss {
        let original = s.steps.iter().find(|st| st.rel_path.as_deref() == Some("scripts/wi-lookup")).unwrap().clone();
        s.steps.retain(|st| st.rel_path.as_deref() != Some("scripts/wi-lookup"));
        let mut call = original.call.clone();
        call.input = serde_json::json!({"command": "cat README.md scripts/wi-lookup"});
        // A tool result is retained intact, not split or copied as certified source per file.
        call.output = "documentation and concatenated source with no file boundaries".into();
        let actions = adage_core::tools::normalize_tool(
            "Bash",
            &call.input,
            s.root.to_str().unwrap(),
            adage_core::CommandTable::load(),
        );
        let steps = adage_ingest::signals::call_steps(original.turn, original.prompt, &call, actions, &s.root);
        assert_eq!(
            steps.iter().filter_map(|st| st.rel_path.as_deref()).collect::<Vec<_>>(),
            ["README.md", "scripts/wi-lookup"]
        );
        assert!(steps.iter().all(|st| st.call.output == call.output));
        s.steps.splice(0..0, steps);
    }
    let note = infer(COMMAND, &ss.iter().collect::<Vec<_>>()).expect("independent run-and-use evidence remains");
    assert!(!note.contains("its text"), "combined output is not individual source evidence: {note}");
    let run = ss[2].steps.iter_mut().find(|st| st.call.output.starts_with("ID: W-")).unwrap();
    run.call.output.push_str("\nWrote .tracker/last-lookup.json\n");
    assert!(
        infer(COMMAND, &ss.iter().collect::<Vec<_>>()).is_none(),
        "a combined read cannot suppress the script's own change report"
    );
}
