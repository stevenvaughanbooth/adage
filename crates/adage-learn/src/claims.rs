//! Steps a source *claims* (a skill's text), judged on the traces: `adage-train` compiles a
//! skill's plain verify claims and its lookup claims this way, never as a general reflex.
//!
//! - [`claimed_verify_route`]: a check owed before finishing, after any edit.
//! - [`claimed_lookup_route`]: a lookup of the work item a prompt names, when the synthetic tasks
//!   name too few distinct items for value flow to type the slot on its own: the claim's example
//!   id is the evidence that the argument is data, and gives its type.

use crate::evidence::{argv, days, median, models, outcome_lift, shell_join, stable, strength_with_lift, LearnConfig};
use crate::import::Exposure as _;
use crate::import::{Session, StepObs};
use adage_core::spans::SpanKind;
use adage_core::vtype::{self, Class, Run, Runs};
use adage_core::Effect;
use adage_tracks::*;
use std::collections::{BTreeMap, BTreeSet};

/// The glob a claimed verify step is owed after: any edit.
pub const ANY_EDIT: &str = "**/*";

/// A verify step a source *claims* is owed before finishing ("run `make check` before you
/// finish"), compiled when the traces show it: `adage-train` calls this for each plain verify
/// claim of a skill, never as a general learner (a check after an edit is otherwise the agent's
/// habit, not a convention, and is not learned; Track G G3).
///
/// The rule is an obligation's, with the edit glob widened to any edit and the contrast taken
/// in time: among the traces that edited a file (the opportunities), those that ran `command`
/// successfully after their last edit (support) against those that ran it only earlier, with
/// edits still to come (the step is owed at the end, not anywhere). Support's rate minus that
/// one's must reach `finish_line_contrast`. The rate bar, or else the outcome-lift path, decides;
/// the step must classify as verify.
pub fn claimed_verify_route(sessions: &[&Session], command: &str, cfg: &LearnConfig) -> Option<Route> {
    let table = adage_core::CommandTable::load();
    if adage_tracks::io::classify(command, &table) != Effect::Verify {
        return None;
    }
    let want: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    let is_it = |st: &StepObs| !st.call.failed && argv(st).is_some_and(|av| *av == want);
    let (mut open, mut support, mut only_before) = (BTreeSet::new(), BTreeSet::new(), 0u32);
    for s in sessions {
        let Some(last) = s.steps.iter().rposition(|st| adage_core::keys::is_author(&st.norm.action)) else { continue };
        open.insert(s.id.clone());
        if s.steps[last + 1..].iter().any(is_it) {
            support.insert(s.id.clone());
        } else if s.steps[..last].iter().any(is_it) {
            only_before += 1;
        }
    }
    let (n, opp) = (support.len() as u32, open.len() as u32);
    if opp == 0 {
        return None;
    }
    let rate = n as f64 / opp as f64;
    if rate - (only_before as f64 / opp as f64) < cfg.finish_line_contrast {
        return None;
    }
    let id = format!("finish-check-{}", stable(&[ANY_EDIT, command]));
    let lb = wilson_lower(n, opp);
    let lift = outcome_lift(sessions, &open, &support);
    let strength = strength_with_lift(Effect::Verify, true, n, lb, rate, cfg, &id, lift.as_ref())?;
    Some(Route {
        id,
        summary: "A check this repository asks for before finishing, after any edit".into(),
        trigger: Trigger::AfterEdit { paths: ANY_EDIT.into() },
        step: Step::Run { command: command.to_string() },
        slots: vec![],
        effect: Effect::Verify,
        strength,
        state: RouteState::Firm,
        exit: vec![Exit::ExploratoryChange],
        evidence: Evidence {
            opportunities: opp,
            support: n,
            days: days(sessions, &support),
            rate,
            lower_bound: lb,
            provenance: cfg.provenance.clone(),
            models: models(sessions),
            outcome_lift: lift,
            ..Default::default()
        },
        authored: false,
    })
}

/// The narrowest type of a claimed example id: letters literal, digits a class of their length,
/// punctuation literal (`W-2042` → `W` `-` `[0-9]{4}`), widened to the digit lengths seen in
/// `seen`. `None` for an id with a mixed letters-and-digits run or no digits.
fn example_type(example: &str, seen: &[String]) -> Option<Runs> {
    let segs = vtype::segments(example);
    let mut runs: Vec<Run> = Vec::new();
    for sg in &segs {
        if sg.chars().all(|c| c.is_ascii_digit()) {
            runs.push(Run::Class { class: Class::D, lo: sg.len() as u8, hi: sg.len() as u8 });
        } else if sg.chars().all(|c| c.is_ascii_alphabetic()) || !sg.chars().any(|c| c.is_ascii_alphanumeric()) {
            runs.push(Run::Lit { text: sg.to_string() });
        } else {
            return None;
        }
    }
    if !runs.iter().any(|r| matches!(r, Run::Class { .. })) {
        return None;
    }
    // Widen each digit run to the lengths of the values seen in its place.
    for v in seen {
        let vs = vtype::segments(v);
        if vs.len() != runs.len() {
            continue;
        }
        let fits = runs.iter().zip(&vs).all(|(r, x)| match r {
            Run::Lit { text } => text == x,
            Run::Class { .. } => x.chars().all(|c| c.is_ascii_digit()),
            _ => false,
        });
        if !fits {
            continue;
        }
        for (r, x) in runs.iter_mut().zip(&vs) {
            if let Run::Class { lo, hi, .. } = r {
                *lo = (*lo).min(x.len() as u8);
                *hi = (*hi).max(x.len() as u8);
            }
        }
    }
    Some(Runs { branches: vec![runs] })
}

/// A lookup a source claims (`scripts/wi-lookup W-2042`), compiled when the traces show it: the
/// traces whose prompt holds an id of the example's type (see [`example_type`]) are the
/// opportunities; those that then ran `tool` with that id before their first edit, the support.
/// The rate bar, or else the outcome-lift path, decides; the effect ceiling comes first.
pub fn claimed_lookup_route(sessions: &[&Session], tool: &str, example: &str, cfg: &LearnConfig) -> Option<Route> {
    let tool_words: Vec<&str> = tool.split_whitespace().collect();
    let runs_tool = |av: &[String]| {
        let words: Vec<&str> = av.iter().map(|a| a.trim_start_matches("./")).collect();
        tool_words.iter().all(|t| words.contains(&t.trim_start_matches("./")))
    };
    // The values the traces looked up with the tool.
    let mut seen: Vec<String> = Vec::new();
    for s in sessions {
        for st in s.discovery() {
            if let Some(av) = argv(st).filter(|av| runs_tool(av)) {
                seen.extend(av.iter().filter(|a| a.chars().any(|c| c.is_ascii_digit())).cloned());
            }
        }
    }
    let runs = example_type(example, &seen)?;
    let (mut opportunities, mut open, mut support) = (0u32, BTreeSet::new(), BTreeSet::new());
    let (mut positions, mut turns, mut variants) = (Vec::new(), Vec::new(), BTreeMap::<String, u32>::new());
    let mut effect = Effect::Observe;
    for s in sessions {
        let values = crate::graph::prompt_values(s, &runs);
        if values.is_empty()
            || s.exposed_identity(&adage_graph::events::act_identity(
                &tool_words.iter().map(|w| w.to_string()).collect::<Vec<_>>(),
            ))
        {
            continue;
        }
        opportunities += 1;
        open.insert(s.id.clone());
        for (i, st) in s.discovery().iter().enumerate() {
            let Some(av) = argv(st) else { continue };
            if st.call.failed || !runs_tool(av) {
                continue;
            }
            let Some(v) = values.iter().find(|v| av.contains(v)) else { continue };
            let templ: Vec<String> = av.iter().map(|a| if a == v { "{item}".to_string() } else { a.clone() }).collect();
            *variants.entry(shell_join(&templ)).or_default() += 1;
            support.insert(s.id.clone());
            positions.push(i as f64);
            turns.push(st.turn as f64);
            effect = crate::evidence::MaxRank::max_by_rank(effect, st.norm.effect);
            break;
        }
    }
    let command = variants.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).map(|(k, _)| k.clone())?;
    let effect =
        crate::evidence::MaxRank::max_by_rank(effect, io::classify(&command, &adage_core::CommandTable::load()));
    ceiling(effect, false)?;
    let n = support.len() as u32;
    let rate = n as f64 / opportunities.max(1) as f64;
    let lb = wilson_lower(n, opportunities);
    let lift = outcome_lift(sessions, &open, &support);
    let strength = strength_with_lift(effect, false, n, lb, rate, cfg, "lookup-work-item", lift.as_ref())?;
    let regex = vtype::to_regex(&runs);
    let mut samples: Vec<String> = seen.iter().filter(|v| vtype::matches(&runs, v)).cloned().collect();
    samples.push(example.to_string());
    samples.sort();
    samples.dedup();
    let identity = adage_graph::events::act_identity(&tool_words.iter().map(|w| w.to_string()).collect::<Vec<_>>());
    let vt = ValueType {
        id: adage_graph::types::type_id(&format!("{identity} #0"), &runs),
        runs: runs.clone(),
        regex: regex.clone(),
        samples_hash: adage_graph::types::samples_hash(&samples),
    };
    Some(Route {
        id: "lookup-work-item".into(),
        summary: "Look up the work item named in the prompt".into(),
        trigger: Trigger::Prompt { span: SpanKind::Ticket },
        step: Step::Run { command },
        slots: vec![Slot {
            name: "item".into(),
            source: SlotSource::PromptValue { vtype: vt, mentions: vec![] },
            contract: Contract::Pattern { regex },
        }],
        effect,
        strength,
        state: initial_state(effect),
        exit: vec![Exit::IntentChanges],
        evidence: Evidence {
            opportunities,
            support: n,
            days: days(sessions, &support),
            rate,
            lower_bound: lb,
            median_position: median(&mut positions),
            turns_saved: median(&mut turns),
            provenance: cfg.provenance.clone(),
            models: models(sessions),
            outcome_lift: lift,
            ..Default::default()
        },
        authored: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::CanonicalAction;

    #[test]
    fn a_claimed_verify_step_compiles_when_traces_run_it_after_their_edits() {
        let with_check = |id: &str, check: bool| {
            let mut s = editing(id, &["acme/billing.py"], false);
            if check {
                let argv = vec!["make".to_string(), "check".to_string()];
                s.steps.push(step(9, CanonicalAction::Exec { argv, cwd: "/r".into() }, Effect::Verify));
            }
            s
        };
        let ss: Vec<Session> = (0..4).map(|i| with_check(&format!("s{i}"), i < 3)).collect();
        let refs: Vec<&Session> = ss.iter().collect();
        let r = claimed_verify_route(&refs, "make check", &LearnConfig::demo()).expect("3 of 4 ran it at the end");
        assert_eq!(r.trigger, Trigger::AfterEdit { paths: ANY_EDIT.into() });
        assert_eq!(r.step, Step::Run { command: "make check".into() });
        assert_eq!((r.evidence.support, r.evidence.opportunities), (3, 4));
        // Too few traces ran it.
        let few: Vec<Session> = (0..4).map(|i| with_check(&format!("s{i}"), i < 1)).collect();
        assert!(claimed_verify_route(&few.iter().collect::<Vec<_>>(), "make check", &LearnConfig::demo()).is_none());
        // Not a verify step: never compiled this way.
        assert!(claimed_verify_route(&refs, "make deploy", &LearnConfig::demo()).is_none());
        // Traces that edited nothing and checked anyway neither count nor hurt.
        let mut idle: Vec<Session> = ss.clone();
        for i in 0..4 {
            let mut s = with_check(&format!("n{i}"), true);
            s.steps.retain(|st| !adage_core::keys::is_author(&st.norm.action));
            idle.push(s);
        }
        assert!(claimed_verify_route(&idle.iter().collect::<Vec<_>>(), "make check", &LearnConfig::demo()).is_some());
        // No contrast in time: as many traces ran it only before edits still to come.
        let mut early: Vec<Session> = ss.clone();
        for i in 0..3 {
            let mut s = with_check(&format!("e{i}"), true);
            let check = s.steps.pop().unwrap();
            s.steps.insert(0, check);
            early.push(s);
        }
        assert!(claimed_verify_route(&early.iter().collect::<Vec<_>>(), "make check", &LearnConfig::demo()).is_none());
    }

    fn step(i: u32, action: CanonicalAction, effect: Effect) -> StepObs {
        let rel_path = adage_core::keys::action_path(&action).map(str::to_string);
        StepObs {
            project_pytest_root_cwd: false,
            turn: i,
            key: adage_core::keys::template_key(&action),
            norm: adage_core::Normalized { action, effect, mutating: false, confidence: 1.0, note: String::new() },
            call: adage_trace::Call {
                tool: "Bash".into(),
                input: serde_json::Value::Null,
                output: String::new(),
                failed: false,
            },
            rel_path,
            prompt: 0,
        }
    }

    /// A session editing these files, then (if `regen`) running `make gen`.
    fn editing(id: &str, files: &[&str], regen: bool) -> Session {
        let mut steps: Vec<StepObs> = files
            .iter()
            .enumerate()
            .map(|(i, f)| step(i as u32, CanonicalAction::Edit { path: f.to_string() }, Effect::Author))
            .collect();
        if regen {
            let argv = vec!["make".to_string(), "gen".to_string()];
            steps.push(step(9, CanonicalAction::Exec { argv, cwd: "/r".into() }, Effect::Regenerate));
        }
        Session {
            id: id.into(),
            root: "/r".into(),
            repo: "acme".into(),
            model: None,
            day: "2026-01-01".into(),
            prompt: "p".into(),
            steps,
            steered: false,
            directed: BTreeSet::new(),
            influence: vec![],
            outcome: None,
            harness: "claude-code".into(),
            prompts: vec![crate::import::PromptObs { text: "p".into(), at_step: 0 }],
            ending: Default::default(),
        }
    }
}
