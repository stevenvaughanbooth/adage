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
