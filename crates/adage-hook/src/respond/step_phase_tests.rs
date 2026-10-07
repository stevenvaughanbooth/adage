//! T-C4-08, in process: the mod's two phases of a step (`dry_run`, then a `verdict` carrying the
//! person's permissions on the calls), mirroring the two phases of a Bash call
//! (`pretool::phases`). A step with neither field is the command hook's, decided and committed at
//! once as before (the rest of `respond`'s tests and the goldens).

use crate::testkit::*;
use crate::{Delivery, Extension, Site, Turn};
use adage_runtime::{Arm, Directive, SessionState};
use adage_store::state;
use serde_json::{json, Value};

/// A stand-in for adage-exec with a person's `predict` grant on the finish line.
struct Granted;

impl Extension for Granted {
    fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
        Delivery::default()
    }
    fn predict_grants(&self, _: &Site<'_>) -> Vec<(String, String)> {
        vec![("acme/finish-lines".into(), "regenerate-registry".into())]
    }
}
static GRANTED: Granted = Granted;

const ROUTE: &str = "regenerate-registry";

fn finish_line() -> Value {
    json!({ "predict": { "kind": "obligation", "route": ROUTE, "track": "acme/finish-lines",
                          "calls": [{ "tool": "Bash", "input": { "command": "make gen" } }] } })
}

/// The step that ends the turn at `index`, with the mod's phase fields `extra`.
fn step(w: &ObligationWorld, index: u32, extra: Value) -> Option<Value> {
    let mut p = json!({ "hook_event_name": "TurnStep", "turn_id": "t1", "index": index, "model": "m",
                        "ending": true, "via": "mod" });
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    w.event(&GRANTED, "TurnStep", p)
}

fn ask(reason: &str) -> Value {
    json!({ "verdict": { "permission": "ask", "route": ROUTE, "tool": "Bash", "reason": reason } })
}

/// A world whose session `s` is in `arm` with the finish line armed.
fn armed(arm: Arm) -> ObligationWorld {
    let w = ObligationWorld::new();
    state::update::<SessionState, _>(&w.paths.state("s"), |st| st.arm = Some(arm)).unwrap();
    w.edit(&GRANTED);
    assert!(!w.state().obligations.is_empty(), "the edit arms the obligation");
    w
}

fn state_of(w: &ObligationWorld) -> Value {
    serde_json::to_value(w.state()).unwrap()
}

/// The dry run answers the prediction, records nothing and leaves the state as it was; asked
/// again it answers the same. `allow` then decides and records exactly as a step with no phase.
#[test]
fn the_dry_run_answers_and_the_allow_verdict_commits_as_before() {
    let w = armed(Arm::Treated);
    let (records, before) = (spooled(&w).len(), state_of(&w));
    assert_eq!(step(&w, 2, json!({ "dry_run": true })), Some(finish_line()));
    assert_eq!(step(&w, 2, json!({ "dry_run": true })), Some(finish_line()), "nothing was taken");
    assert_eq!((spooled(&w).len(), state_of(&w)), (records, before), "no record, no state change");

    let allowed = step(&w, 2, json!({ "verdict": { "permission": "allow", "route": ROUTE } }));
    assert_eq!(allowed, Some(finish_line()));
    let rec = spooled(&w).pop().unwrap();
    assert_eq!((rec["kind"].as_str(), rec["event"].as_str()), (Some("turn_step"), Some("TurnStep")));
    assert_eq!(rec["decision"]["predicted"]["route"], ROUTE);
    assert_eq!(rec["decision"]["predicted_at"], json!(["t1", 2]));
    assert!(rec["decision"].get("withheld").is_none() && rec["decision"].get("permission").is_none());
    let st = w.state();
    assert_eq!((st.predictions, st.predicted_routes.clone()), (1, vec![ROUTE.to_string()]));

    // The same step with no phase, in a world of its own: the same record but for the clock.
    let plain = armed(Arm::Treated);
    assert_eq!(step(&plain, 2, json!({})), Some(finish_line()));
    let strip = |mut r: Value| {
        for k in ["ts", "duration_us"] {
            r.as_object_mut().unwrap().remove(k);
        }
        r
    };
    assert_eq!(strip(spooled(&plain).pop().unwrap()), strip(rec));
    assert_eq!(serde_json::to_value(plain.state()).unwrap(), state_of(&w));
}

/// Any verdict but `allow` answers nothing, records the step as withheld with the engine's
/// verdict and sentence, counts it against no bound, and holds its route for the session.
#[test]
fn a_verdict_other_than_allow_records_the_step_withheld() {
    for permission in ["ask", "deny", "none", "unknown"] {
        let w = armed(Arm::Treated);
        assert!(step(&w, 2, json!({ "dry_run": true })).is_some());
        let mut v = ask("This command requires approval");
        v["verdict"]["permission"] = json!(permission);
        assert_eq!(step(&w, 2, v), None, "{permission}");
        let rec = spooled(&w).pop().unwrap();
        assert_eq!(rec["kind"], "turn_step");
        let d = &rec["decision"];
        assert_eq!(d["withheld"], "permission", "{rec}");
        let said = if permission == "unknown" { "none" } else { permission };
        assert_eq!(
            d["permission"],
            json!({ "decision": said, "tool": "Bash", "reason": "This command requires approval" })
        );
        assert_eq!(d["would_predict"]["route"], ROUTE);
        assert!(d.get("predicted").is_none() && d.get("predicted_at").is_none(), "{rec}");
        let st = w.state();
        assert_eq!((st.predictions, st.last_step_predicted.clone()), (0, None));
        assert!(st.predicted_routes.is_empty());
        assert_eq!(st.withheld_routes, vec![ROUTE.to_string()]);
        // Not offered again this session: the next ending step, dry run or not, says nothing
        // and records nothing.
        let records = spooled(&w).len();
        assert_eq!(step(&w, 4, json!({ "dry_run": true })), None);
        assert_eq!(step(&w, 4, json!({})), None);
        assert_eq!(spooled(&w).len(), records);
    }
}

/// The held route stays held after the state is lost: rebuilt from the `withheld` record.
#[test]
fn a_held_route_is_rebuilt_from_the_spool() {
    let w = armed(Arm::Treated);
    assert_eq!(step(&w, 2, ask("x")), None);
    std::fs::write(w.paths.state("s"), b"\x00garbage that is no state").unwrap();
    assert_eq!(step(&w, 4, json!({ "dry_run": true })), None);
    assert_eq!(w.state().withheld_routes, vec![ROUTE.to_string()]);
}

/// The control arm and observe mode yield nothing, so their dry run waits for no verdict: it
/// decides and records `would_predict` at once, as the step with no phase does.
#[test]
fn a_dry_run_that_yields_nothing_records_as_before() {
    for (arm, observe) in [(Arm::Control, false), (Arm::Treated, true)] {
        let w = armed(arm);
        let mut extra = json!({ "dry_run": true });
        if observe {
            extra["observe"] = json!(true);
        }
        assert_eq!(step(&w, 2, extra), None, "{arm:?}");
        let rec = spooled(&w).pop().unwrap();
        assert_eq!(rec["kind"], "turn_step");
        assert_eq!(rec["decision"]["would_predict"]["route"], ROUTE);
        assert_eq!(rec["decision"]["predicted_at"], json!(["t1", 2]));
        assert!(rec["decision"].get("predicted").is_none() && rec["decision"].get("withheld").is_none());
        assert_eq!(w.state().predictions, 1);
    }
}

/// A verdict for a route the step no longer finds (the plan moved since the dry run) answers
/// nothing and records nothing.
#[test]
fn a_verdict_for_a_moved_plan_records_nothing() {
    let w = armed(Arm::Treated);
    let (records, before) = (spooled(&w).len(), state_of(&w));
    for permission in ["allow", "ask"] {
        let v = json!({ "verdict": { "permission": permission, "route": "another-route" } });
        assert_eq!(step(&w, 2, v), None);
    }
    assert_eq!(spooled(&w).len(), records);
    assert_eq!(state_of(&w)["withheld_routes"], before["withheld_routes"]);
    assert_eq!(w.state().predictions, 0);
}
