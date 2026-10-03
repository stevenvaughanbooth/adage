//! The offline prediction gate (the Claude Code mod's `turn.step`, plan §2.7 and chunk E2's offline
//! half): recorded sessions replayed through `adage_runtime::decide` with a `TurnStep` injected at
//! every step boundary, as the mod raises them (0 after a prompt, `k` after the turn's k-th tool
//! batch, and `ending` before the turn's stop), once per arm, every learned route but the
//! exec-only ones holding a `predict` grant.
//!
//! - **Requests saved.** A step Adage makes in the model's place is a request the model is not
//!   sent. The treated arm's predicted steps must be at least the opportunities its stream holds,
//!   counted here apart from the runtime: a directive given with its command bound (a pending run,
//!   sequence or skill step, a bound read batch, or at the turn's end an unmet obligation), at a
//!   step after the prompt's own request, never two steps in a row, at most six per session, each
//!   route once. The count on the stream as recorded (the control arm's) is printed beside it: it
//!   can be higher, where a step Adage made met what a later opportunity would have (a sequence's
//!   `make gen` meets the finish line's obligation). In the control arm nothing is predicted.
//! - **Accuracy.** Each step predicted before a request against what the recorded agent did next:
//!   a Bash call is right when it is the agent's next Bash command, a chain of reads when the
//!   agent's next reads cover its files (its next batch with a Read and the all-Read batches right
//!   after it: a chain is one request where the agent made one per file, plan C7). Also printed: the share the agent made at some
//!   later point of the session. A finish-line call at a turn's end has no "next" in a recording
//!   that stopped there, so it is counted, not scored.
//!
//! In the treated arm the stream changes where Adage acts, as it would live: its calls arrive as a
//! batch of `by: adage` calls, the agent's own later call of the same thing is not replayed (its
//! result is Adage's call's result), and a call the agent never made gets an empty result. The
//! recording after an early step is not what that agent would have done next, which is why strict
//! accuracy reads low on sessions recorded without directives: the agent found the lookup on its
//! own, several steps after Adage would have run it. Numbers are printed (`--nocapture`); the
//! floors sit just under the values measured when the gate was set.
#![cfg(unix)]

use adage_core::CommandTable;
use adage_eval::dataset::Sandbox;
use adage_eval::transcript;
use adage_eval::{prepare, Options};
use adage_runtime::{decide, fast, Arm, By, Call, Ctx, Event, Judgement, Policy, Prediction, SessionState};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

fn datasets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/datasets")
}

/// What one arm did over a dataset.
#[derive(Debug, Default)]
struct Tally {
    steps: u32,
    /// Steps Adage made in the model's place: the requests saved.
    predicted: u32,
    would_predict: u32,
    /// Predicted steps before a request (the ones scored), those that were the agent's next action,
    /// and those the agent took at some later point of the session.
    scored: u32,
    correct: u32,
    later: u32,
    /// Bash predictions only (`run`, `sequence`).
    bash_scored: u32,
    bash_correct: u32,
    /// Finish-line calls at a turn's end (not scored).
    at_end: u32,
    /// Adage's calls judged `Predicted` when they came back.
    judged_predicted: u32,
    opportunities: u32,
    /// Steps the hook's fast path skips whose full decision predicted something.
    fast_mismatches: Vec<String>,
    sessions: u32,
}

/// The bounds of the mod's `turn.step` (plan §2.2 item 2b), kept apart from the runtime's own.
#[derive(Default)]
struct Opportunities {
    count: u32,
    last: Option<(String, u32)>,
    routes: Vec<String>,
}

const CAP: u32 = 6;

/// Offline output is deterministic, so a floor sits just under the measured value.
const EPS: f64 = 0.02;

impl Opportunities {
    /// Whether this step is an opportunity, counting it if so: the state holds a directive with
    /// its command bound (a pending run, sequence or skill step, a bound read batch, or at the
    /// turn's end an unmet obligation) whose route may be predicted.
    fn step(&mut self, st: &SessionState, turn: &str, index: u32, ending: bool, granted: &dyn Fn(&str) -> bool) {
        let previous = if ending { Some(index) } else { index.checked_sub(1) };
        if index == 0 || self.count >= CAP || self.last.as_ref().is_some_and(|(t, i)| t == turn && Some(*i) == previous)
        {
            return;
        }
        let open = |r: &String| granted(r) && !self.routes.contains(r);
        let route = st
            .pending
            .iter()
            .filter(|p| !p.command.is_empty())
            .map(|p| &p.route)
            .chain(st.reads.iter().filter(|r| !r.files.is_empty()).map(|r| &r.route))
            .chain(st.obligations.iter().filter(|o| ending && !o.met && !o.command.is_empty()).map(|o| &o.route))
            .find(|r| open(r))
            .cloned();
        if let Some(r) = route {
            self.count += 1;
            self.last = Some((turn.to_string(), index));
            self.routes.push(r);
        }
    }
}

/// Where the recording makes a predicted call: `(event, call)` of the agent's call, and whether it
/// is what the agent did next (for Bash, its next Bash command; for a Read, a read in its next
/// batch that reads anything) or only later in the session.
fn find(queue: &VecDeque<Event>, call: &adage_runtime::PredictedCall) -> Option<((usize, usize), bool)> {
    let same = |c: &Call| {
        c.tool == call.tool
            && c.by == By::Agent
            && match call.tool.as_str() {
                "Bash" => c.input["command"].as_str().map(str::trim) == call.input["command"].as_str(),
                _ => c.input["file_path"] == call.input["file_path"],
            }
    };
    let mut first = true;
    for (i, e) in queue.iter().enumerate() {
        let Event::Batch { calls } = e else { continue };
        if let Some(k) = calls.iter().position(same) {
            return Some(((i, k), first));
        }
        first &= !calls.iter().any(|c| c.tool == call.tool && c.by == By::Agent);
    }
    None
}

/// Whether the agent's next reads cover `files`: its first batch with a Read, and the batches right
/// after it whose calls are all Reads (an agent reads a chain's files one request at a time, where
/// Adage reads them in one).
fn next_reads_cover(queue: &VecDeque<Event>, files: &[&serde_json::Value]) -> bool {
    let mut read: Vec<&serde_json::Value> = Vec::new();
    for e in queue {
        let Event::Batch { calls } = e else {
            if read.is_empty() && !matches!(e, Event::Prompt { .. } | Event::Stop { .. } | Event::SessionEnd) {
                continue;
            }
            break;
        };
        let agent: Vec<&Call> = calls.iter().filter(|c| c.by == By::Agent).collect();
        if read.is_empty() {
            read.extend(agent.iter().filter(|c| c.tool == "Read").map(|c| &c.input["file_path"]));
        } else if !agent.is_empty() && agent.iter().all(|c| c.tool == "Read") {
            read.extend(agent.iter().map(|c| &c.input["file_path"]));
        } else {
            break;
        }
    }
    files.iter().all(|f| read.contains(f))
}

/// Replay one session in one arm, `TurnStep`s injected at every step boundary.
#[allow(clippy::too_many_arguments)]
fn replay(
    ctx: &Ctx,
    t: &transcript::Transcript,
    root: &str,
    stop: bool,
    arm: Arm,
    granted: &dyn Fn(&str) -> bool,
    tally: &mut Tally,
) {
    let triggers = ctx.index.triggers(&ctx.repo);
    let mut queue: VecDeque<Event> = transcript::events(t, root, stop).into_iter().map(|s| s.event).collect();
    let mut st = SessionState { arm: Some(arm), repo: Some(ctx.repo.clone()), ..Default::default() };
    let mut opp = Opportunities::default();
    let (mut turn, mut index, mut in_turn) = (0u32, 0u32, false);
    tally.sessions += 1;
    while let Some(ev) = queue.pop_front() {
        let ends = matches!(ev, Event::Prompt { .. } | Event::Stop { .. } | Event::SessionEnd);
        if in_turn && ends {
            // The model's response to request `index` ended the turn.
            queue.push_front(ev);
            if !turn_step(ctx, &triggers, &mut st, &mut queue, &mut opp, (t, root, granted), turn, index, true, tally) {
                in_turn = false;
            }
            continue;
        }
        let d = decide(ctx, &mut st, &ev);
        tally.judged_predicted +=
            d.judgements.iter().filter(|j| matches!(j, Judgement::Predicted { .. })).count() as u32;
        match ev {
            Event::Prompt { .. } => {
                (turn, index, in_turn) = (turn + 1, 0, true);
            }
            Event::Batch { .. } if in_turn => index += 1,
            _ => continue,
        }
        turn_step(ctx, &triggers, &mut st, &mut queue, &mut opp, (t, root, granted), turn, index, false, tally);
    }
    tally.opportunities += opp.count;
}

/// The session a step belongs to: its transcript, the replay's root, and which routes are granted.
type Of<'a> = (&'a transcript::Transcript, &'a str, &'a dyn Fn(&str) -> bool);

/// One `TurnStep`: decided, scored, and, when Adage makes the step, its calls put at the front of
/// the stream as the batch the harness reports. Returns whether Adage made it.
#[allow(clippy::too_many_arguments)]
fn turn_step(
    ctx: &Ctx,
    triggers: &adage_tracks::triggers::Triggers,
    st: &mut SessionState,
    queue: &mut VecDeque<Event>,
    opp: &mut Opportunities,
    (t, root, granted): Of,
    turn: u32,
    index: u32,
    ending: bool,
    tally: &mut Tally,
) -> bool {
    let turn_id = format!("t{turn}");
    opp.step(st, &turn_id, index, ending, granted);
    let ev = Event::TurnStep { turn_id: turn_id.clone(), index, model: None, agent: None, ending };
    let skip = fast::skip(triggers, &ev, root, || Some(st.clone()));
    let d = decide(ctx, st, &ev);
    tally.steps += 1;
    if skip && d.would_predict.is_some() {
        tally.fast_mismatches.push(format!("{} {turn_id} {index}", t.id));
    }
    tally.would_predict += u32::from(d.would_predict.is_some());
    let Some(p) = d.predicted else { return false };
    tally.predicted += 1;
    let made = adage_calls(queue, &p, ending, tally);
    let calls: Vec<Call> = made
        .into_iter()
        .enumerate()
        .map(|(k, mut c)| {
            c.by = By::Adage;
            c.tool_use_id = Some(format!("adage_{}_{turn_id}_{index}_{k}", p.route));
            c
        })
        .collect();
    let failed: Vec<Event> = calls.iter().filter(|c| c.failed).map(|c| Event::Failure { call: c.clone() }).collect();
    let batch = Event::Batch { calls: calls.into_iter().map(|c| Call { failed: false, ..c }).collect() };
    queue.push_front(batch);
    for f in failed.into_iter().rev() {
        queue.push_front(f);
    }
    true
}

/// The calls Adage makes for `p`, scored against the recording. A call the agent made later in
/// the session is taken out of the recording (Adage made it; the agent does not make it twice) and
/// keeps the agent's result, which the same call would have had; one it never made gets an empty
/// result.
fn adage_calls(queue: &mut VecDeque<Event>, p: &Prediction, ending: bool, tally: &mut Tally) -> Vec<Call> {
    let found: Vec<Option<((usize, usize), bool)>> = p.calls.iter().map(|c| find(queue, c)).collect();
    if ending {
        tally.at_end += 1;
    } else {
        let next = if p.calls.len() > 1 && p.calls.iter().all(|c| c.tool == "Read") {
            let files: Vec<&serde_json::Value> = p.calls.iter().map(|c| &c.input["file_path"]).collect();
            next_reads_cover(queue, &files)
        } else {
            found.iter().all(|f| f.is_some_and(|(_, next)| next))
        };
        let later = found.iter().all(Option::is_some);
        tally.scored += 1;
        tally.correct += u32::from(next);
        tally.later += u32::from(later);
        if p.calls.iter().all(|c| c.tool == "Bash") {
            tally.bash_scored += 1;
            tally.bash_correct += u32::from(next);
        }
    }
    // The agent's failure of a call came as its own event, right before the call's batch.
    let failure_of = |queue: &VecDeque<Event>, i: usize, c: &Call| {
        (0..i)
            .rev()
            .take_while(|&j| matches!(queue[j], Event::Failure { .. }))
            .find(|&j| matches!(&queue[j], Event::Failure { call } if call.tool == c.tool && call.input == c.input))
    };
    let mut gone = vec![false; queue.len()];
    let mut out = Vec::new();
    for (pc, f) in p.calls.iter().zip(&found) {
        let call = match f {
            Some(((i, k), _)) => {
                let Event::Batch { calls } = &queue[*i] else { unreachable!() };
                let mut c = calls[*k].clone();
                if let Some(j) = failure_of(queue, *i, &c) {
                    gone[j] = true;
                    c.failed = true;
                }
                c
            }
            None => Call {
                tool: pc.tool.clone(),
                input: pc.input.clone(),
                output: String::new(),
                failed: false,
                by: By::Agent,
                tool_use_id: None,
                agent: None,
            },
        };
        out.push(call);
    }
    let mut taken: Vec<(usize, usize)> = found.iter().flatten().map(|(at, _)| *at).collect();
    taken.sort();
    for &(i, k) in taken.iter().rev() {
        if let Event::Batch { calls } = &mut queue[i] {
            calls.remove(k);
        }
    }
    let mut n = 0;
    queue.retain(|e| {
        n += 1;
        !gone[n - 1] && !matches!(e, Event::Batch { calls } if calls.is_empty())
    });
    out
}

/// Both arms over a dataset's test split; prints the numbers.
fn run(name: &str, split: Option<&str>) -> (Tally, Tally) {
    let dir = datasets().join(name);
    let opts = Options { split: split.map(str::to_string), ..Default::default() };
    let p = prepare(&dir, &opts).unwrap_or_else(|e| panic!("{name}: {e:#}"));
    let table = CommandTable::load();
    let sandbox = Sandbox::create(&p.ds.sandbox().unwrap()).unwrap();
    let repo = p.ds.manifest.repo.clone();
    let grants: Vec<(String, String)> = p
        .index
        .for_repo(&repo)
        .filter(|ir| !ir.exec_only())
        .map(|ir| (ir.track.clone(), ir.route.id.clone()))
        .collect();
    let granted = |route: &str| grants.iter().any(|(_, r)| r == route);
    let ctx = Ctx {
        checkout: &adage_runtime::checkout::Disk,
        index: &p.index,
        repo: repo.clone(),
        root: &sandbox.root,
        table: &table,
        embed: None,
        params: p.params.runtime,
        policy: Policy { predict_grants: grants.clone(), ..Default::default() },
    };
    let root = sandbox.root.to_string_lossy().to_string();
    let (mut treated, mut control) = (Tally::default(), Tally::default());
    let mut test = p.split.test.clone();
    test.sort();
    for id in &test {
        let meta = p.ds.session(id).unwrap();
        let t = transcript::read(&p.ds.path(meta)).unwrap();
        // As the eval scores: a session that was given live directives is not what an agent does
        // on its own.
        if t.steered {
            continue;
        }
        replay(&ctx, &t, &root, meta.stopped(), Arm::Treated, &granted, &mut treated);
        replay(&ctx, &t, &root, meta.stopped(), Arm::Control, &granted, &mut control);
    }
    let ratio = |a: u32, b: u32| if b == 0 { f64::NAN } else { f64::from(a) / f64::from(b) };
    println!(
        "{name} {}: {} sessions, {} steps, {} routes granted; opportunities {} (as recorded: {}); requests saved: treated {} (would {}), control {} (would {}); accuracy {:.4} ({}/{}), Bash {:.4} ({}/{}), made later in the session {:.4} ({}/{}); finish-line calls at a turn's end {}; judged predicted {}",
        p.split_name,
        treated.sessions,
        treated.steps,
        grants.len(),
        treated.opportunities,
        control.opportunities,
        treated.predicted,
        treated.would_predict,
        control.predicted,
        control.would_predict,
        ratio(treated.correct, treated.scored),
        treated.correct,
        treated.scored,
        ratio(treated.bash_correct, treated.bash_scored),
        treated.bash_correct,
        treated.bash_scored,
        ratio(treated.later, treated.scored),
        treated.later,
        treated.scored,
        treated.at_end,
        treated.judged_predicted,
    );
    (treated, control)
}

fn check(name: &str, split: Option<&str>, min_accuracy: f64) {
    let (treated, control) = run(name, split);
    assert!(
        treated.fast_mismatches.is_empty(),
        "{name}: fast path skipped a prediction: {:?}",
        treated.fast_mismatches
    );
    assert!(control.fast_mismatches.is_empty(), "{name}: {:?}", control.fast_mismatches);
    assert!(treated.opportunities > 0, "{name}: the dataset holds no opportunity, so the gate means nothing");
    assert!(
        treated.predicted >= treated.opportunities,
        "{name}: {} requests saved, below the {} opportunities the dataset holds",
        treated.predicted,
        treated.opportunities
    );
    assert_eq!(control.predicted, 0, "{name}: the control arm never acts");
    assert_eq!(control.judged_predicted, 0, "{name}");
    // A call can stand for two routes at once (an obligation carried by a sequence's first step).
    assert!(treated.judged_predicted >= treated.predicted, "{name}: each of Adage's steps judged predicted");
    let accuracy = f64::from(treated.correct) / f64::from(treated.scored.max(1));
    assert!(accuracy >= min_accuracy, "{name}: accuracy {accuracy:.4} below the gate {min_accuracy}");
}

#[test]
fn predicted_steps_save_requests_and_match_the_agent() {
    // Measured when the gate was set: demo round 2, 34 steps for 34 opportunities (35 as
    // recorded), accuracy 6/34 = 0.1765 (Bash 3/22), 23/34 made later in the session; synthetic,
    // 25 for 25, accuracy 12/24 = 0.5, 20/24 later. With chains (plan C7), measured: demo 34 for
    // 26, 7/34; synthetic 30 for 19, 22/29 (a chain right after Adage's lookup, scored against
    // the agent's next reads, one file per request); the floors are unchanged.
    check("demo-haiku45", Some("round2"), 0.1765 - EPS);
    check("synthetic", None, 0.5 - EPS);
}
