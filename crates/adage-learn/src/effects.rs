//! Effect classes from evidence, for repository scripts the command table does not know.
//!
//! The command table is authoritative for every program it lists, and an unlisted command
//! defaults to `environment` (Design §8.6), which leaves its reflex a nascent nascent until a person
//! approves it. A repository script is classified observe by its name only when the name leads
//! with a read verb (`wi-lookup`, `fetch-incident`); `tools/case` is not, however plainly the
//! traces show it only reads. So a script is also classified observe **from the traces** when all
//! of these hold ([`infer`]):
//!
//! - it is a repository script the table does not know, run as a plain command, whose name says
//!   nothing of changing, building or sending anything (`adage_core::shell::unlisted_repo_script`);
//! - at least [`MIN_TRACES`] traces ran it successfully;
//! - no run was followed by file changes it caused: a `git status` or `git diff` after it, before
//!   anything else that could have made the change (the agent's own edit, or a command not
//!   classified observe: a heredoc that rewrites a schema, `make gen`), never lists a change, and, where no trace shows the script's own
//!   text, its output never reports writing, creating or updating anything on a line of its own
//!   ([`reports_change`]);
//! - what it printed was used: in at least two thirds of the traces, the paths it printed were
//!   read next (the linked-files pattern), or it was followed by reads within three steps;
//! - wherever the traces show the script's own text (the agent read it), that text uses no
//!   network or process library and writes no file.
//!
//! What a script prints is mostly what it was asked about, not what it did: a lookup prints a work
//! item, and a work item says things like "generated models include it". So the evidence is the
//! command and the script: once a trace shows the script's text and the text writes nothing, what
//! it printed is not read for change words at all; without the text, only a line that opens with
//! one (`Wrote cache.json`, `Updated 3 files`) counts as the script saying it changed something.
//!
//! The route records why, in `evidence.effect_evidence`; track validation accepts an observe
//! declaration over the table's `environment` only for such a script with that note.

use crate::import::Session;
use adage_core::{spans, CanonicalAction, CommandTable, Effect};
use std::collections::BTreeSet;

/// Traces that must have run the script before its effect is inferred.
pub const MIN_TRACES: usize = 3;

/// What a script's text may not contain for it to be classified observe from the traces: network
/// and process libraries, and file writes.
const UNSAFE_TEXT: [&str; 30] = [
    "requests",
    "urllib",
    "http.client",
    "http://",
    "https://",
    "socket",
    "fetch(",
    "axios",
    "require('http",
    "require(\"http",
    "require('net",
    "require(\"net",
    "child_process",
    "subprocess",
    "os.system",
    "os.popen",
    "exec(",
    "spawn(",
    "curl",
    "wget",
    "write_text",
    "write_bytes",
    "writefile",
    "appendfile",
    "fs.write",
    "unlink",
    "os.remove",
    "shutil",
    "rmsync",
    "mkdir",
];

/// Words that open a line of a script's output when it says it changed something.
const CHANGED: [&str; 9] =
    ["wrote ", "written", "created ", "updated ", "saved ", "deleted ", "modified ", "generated ", "removed "];

/// The effect a mined command is directed with, and why when it was inferred from the traces:
/// `effect` as classified, unless it is `environment` for a repository script the traces show
/// only reads ([`infer`]).
pub fn effect_of(effect: Effect, command: &str, sessions: &[&Session]) -> (Effect, Option<String>) {
    if effect != Effect::Environment {
        return (effect, None);
    }
    match infer(command, sessions) {
        Some(note) => (Effect::Observe, Some(note)),
        None => (effect, None),
    }
}

/// Whether a step runs the script at `rel` (directly, or under an interpreter past its flags:
/// `python3 -I scripts/wi-lookup W-1042`).
fn runs(st: &crate::import::StepObs, rel: &str) -> bool {
    let Some(av) = crate::evidence::argv(st) else { return false };
    let is = |a: &String| a.trim_start_matches("./") == rel;
    match av.first() {
        Some(a) if is(a) => true,
        Some(a) if adage_core::shell::SCRIPT_INTERPRETERS.contains(&a.as_str()) => {
            av.iter().skip(1).find(|a| !a.starts_with('-')).is_some_and(is)
        }
        _ => false,
    }
}

fn is_read(st: &crate::import::StepObs) -> bool {
    matches!(st.norm.action, CanonicalAction::Read { .. })
        || (matches!(st.norm.action, CanonicalAction::Exec { .. }) && st.norm.effect == Effect::Observe)
}

/// Whether a line of `output` opens with a word that says the script changed something (past any
/// indent, list marker or `[tag]`); a change word inside a sentence is what the script printed
/// about something else, a work item's text say.
fn reports_change(output: &str) -> bool {
    output.lines().any(|line| {
        let mut l = line.trim_start().trim_start_matches(['-', '*', '>', ' ']);
        if l.starts_with('[') {
            l = l.split_once(']').map_or(l, |(_, rest)| rest.trim_start());
        }
        let l = format!("{} ", l.to_ascii_lowercase());
        CHANGED.iter().any(|w| l.starts_with(w))
    })
}

/// Whether the step shows the script's own text: the agent read it, or ran a pager over it.
fn shows_text(st: &crate::import::StepObs, rel: &str) -> bool {
    // Multiple cat operands are useful co-read evidence, but their one result has no per-file
    // boundaries. It cannot certify any one script's source or override its reported writes.
    if st.norm.note.contains(adage_core::shell::SHARED_CAT_OUTPUT) {
        return false;
    }
    match &st.norm.action {
        CanonicalAction::Read { .. } => st.rel_path.as_deref() == Some(rel),
        CanonicalAction::Exec { argv, .. } => {
            // Retain the old single-file Exec representation, but not an ambiguous cat
            // exec with options or several inputs. New literal cat operands become Read.
            if argv.first().is_some_and(|a| a == "cat") {
                return argv.len() == 2 && argv[1].trim_start_matches("./") == rel;
            }
            matches!(argv.first().map(String::as_str), Some("head" | "sed" | "less"))
                && argv.iter().skip(1).any(|a| a.trim_start_matches("./") == rel)
        }
        _ => false,
    }
}

fn status_shows_change(st: &crate::import::StepObs) -> bool {
    let Some(av) = crate::evidence::argv(st) else { return false };
    if av.first().map(String::as_str) != Some("git")
        || !matches!(av.get(1).map(String::as_str), Some("status" | "diff"))
    {
        return false;
    }
    let o = &st.call.output;
    o.contains("modified:") || o.contains("deleted:") || o.contains("new file:") || o.contains("diff --git")
}

/// The note for a repository script the traces show only reads, or `None` (see the module
/// documentation).
pub fn infer(command: &str, sessions: &[&Session]) -> Option<String> {
    let table = CommandTable::load();
    let rel = adage_core::shell::unlisted_repo_script(command, table)?;
    // The script's own text, wherever a trace shows it: one unsafe sight of it settles the effect,
    // and a safe one makes its output's wording moot.
    let mut inspected = false;
    for st in sessions.iter().flat_map(|s| &s.steps) {
        if shows_text(st, &rel) && !st.call.failed {
            inspected = true;
            let lower = st.call.output.to_ascii_lowercase();
            if UNSAFE_TEXT.iter().any(|w| lower.contains(w)) || open_for_writing(&lower) {
                return None;
            }
        }
    }
    let mut ran: BTreeSet<&str> = BTreeSet::new();
    let mut used: BTreeSet<&str> = BTreeSet::new();
    // A step that could have changed files itself, so a change listed after it is not the script's.
    let could_change = |st: &crate::import::StepObs| {
        adage_core::keys::is_author(&st.norm.action)
            || (matches!(st.norm.action, CanonicalAction::Exec { .. })
                && st.norm.effect != Effect::Observe
                && !runs(st, &rel))
    };
    for s in sessions {
        let mut edited = false;
        for (i, st) in s.steps.iter().enumerate() {
            if could_change(st) {
                edited = true;
            }
            if !runs(st, &rel) || st.call.failed {
                continue;
            }
            if !inspected && reports_change(&st.call.output) {
                return None;
            }
            let after = &s.steps[i + 1..];
            if !edited {
                let before_edit = after.iter().take_while(|x| !could_change(x));
                if before_edit.into_iter().any(status_shows_change) {
                    return None;
                }
            }
            ran.insert(&s.id);
            let printed: BTreeSet<String> = spans::extract(&st.call.output)
                .into_iter()
                .filter(|x| x.kind == spans::SpanKind::Path)
                .map(|x| x.value)
                .collect();
            let flows = after.iter().take(16).any(|x| {
                matches!(x.norm.action, CanonicalAction::Read { .. })
                    && x.rel_path.as_ref().is_some_and(|p| printed.contains(p))
            });
            if flows || after.iter().take(3).any(is_read) {
                used.insert(&s.id);
            }
        }
    }
    if ran.len() < MIN_TRACES || used.len() * 3 < ran.len() * 2 {
        return None;
    }
    Some(format!(
        "{rel} is not in the command table; classified observe from {} traces: no run changed a file, and its output was read next in {} of them{}",
        ran.len(),
        used.len(),
        if inspected { "; its text uses no network or process library and writes no file" } else { "" }
    ))
}

/// `open(…, "w")` and friends, in lower-cased script text.
fn open_for_writing(lower: &str) -> bool {
    let re = regex::Regex::new(r#"open\([^)]*['"][wax]\+?b?['"]"#).expect("regex");
    re.is_match(lower)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{Ending, PromptObs, StepObs};

    fn step(i: u32, action: CanonicalAction, effect: Effect, output: &str) -> StepObs {
        let rel_path = adage_core::keys::action_path(&action).map(str::to_string);
        StepObs {
            project_pytest_root_cwd: false,
            turn: i,
            key: adage_core::keys::template_key(&action),
            norm: adage_core::Normalized { action, effect, mutating: false, confidence: 1.0, note: String::new() },
            call: adage_trace::Call {
                tool: "Bash".into(),
                input: serde_json::Value::Null,
                output: output.into(),
                failed: false,
                by: adage_trace::By::Agent,
                tool_use_id: None,
                agent: None,
            },
            rel_path,
            prompt: 0,
        }
    }

    fn exec(i: u32, cmd: &str, effect: Effect, output: &str) -> StepObs {
        let argv = cmd.split(' ').map(str::to_string).collect();
        step(i, CanonicalAction::Exec { argv, cwd: "/r".into() }, effect, output)
    }

    fn read(i: u32, path: &str) -> StepObs {
        step(i, CanonicalAction::Read { path: path.into(), range: None }, Effect::Observe, "")
    }

    fn session(id: &str, steps: Vec<StepObs>) -> Session {
        Session {
            delegation: None,
            id: id.into(),
            root: "/r".into(),
            repo: "clinic".into(),
            model: None,
            day: "2026-09-28".into(),
            prompt: "Fix CARE-1".into(),
            steps,
            steered: false,
            directed: BTreeSet::new(),
            influence: vec![],
            outcome: None,
            harness: "claude-code".into(),
            prompts: vec![PromptObs { text: "Fix CARE-1".into(), at_step: 0 }],
            identity: Default::default(),
            ending: Ending::default(),
        }
    }

    fn lookup_then_read(id: &str, out: &str) -> Session {
        session(id, vec![exec(0, "python3 tools/case CARE-1", Effect::Environment, out), read(1, "clinic/intake.py")])
    }

    #[test]
    fn a_script_the_traces_show_only_reading_is_observe() {
        let ss: Vec<Session> =
            (0..3).map(|i| lookup_then_read(&format!("s{i}"), "CARE-1: files clinic/intake.py")).collect();
        let refs: Vec<&Session> = ss.iter().collect();
        let (e, note) = effect_of(Effect::Environment, "python3 tools/case {item}", &refs);
        assert_eq!(e, Effect::Observe);
        assert!(note.unwrap().contains("from 3 traces"));
        // Too few traces.
        assert_eq!(effect_of(Effect::Environment, "python3 tools/case {item}", &refs[..2]).0, Effect::Environment);
        // The table stays authoritative, and only environment is ever lowered.
        assert_eq!(effect_of(Effect::Verify, "python3 tools/case {item}", &refs).0, Effect::Verify);
        assert_eq!(effect_of(Effect::Environment, "make case", &refs).0, Effect::Environment);
    }

    #[test]
    fn a_change_word_in_what_it_looked_up_is_not_a_change() {
        // A work item's text, as a lookup prints it: prose about the work, not about the script.
        let out =
            "CARE-1: files clinic/intake.py\nAcceptance: generated forms include the field; records updated nightly";
        let ss: Vec<Session> = (0..3).map(|i| lookup_then_read(&format!("s{i}"), out)).collect();
        let refs: Vec<&Session> = ss.iter().collect();
        assert_eq!(effect_of(Effect::Environment, "python3 tools/case {item}", &refs).0, Effect::Observe);
        // Its text, read once and writing nothing, makes even a leading change word moot.
        let mut ss: Vec<Session> = (0..3).map(|i| lookup_then_read(&format!("s{i}"), "Updated 3 records")).collect();
        assert!(infer("python3 tools/case {item}", &ss.iter().collect::<Vec<_>>()).is_none());
        ss[2].steps.insert(
            0,
            exec(0, "cat tools/case", Effect::Observe, "import json\nprint(json.load(open('cases.json')))\n"),
        );
        let note = infer("python3 tools/case {item}", &ss.iter().collect::<Vec<_>>()).expect("observe");
        assert!(note.contains("its text uses no network or process library"), "{note}");
    }

    #[test]
    fn ambiguous_cat_exec_does_not_establish_script_source() {
        for cmd in ["cat README.md tools/case", "cat --unknown tools/case", "cat - tools/case"] {
            let mut ss: Vec<Session> =
                (0..3).map(|i| lookup_then_read(&format!("s{i}"), "Updated 3 records")).collect();
            ss[0].steps.insert(0, exec(0, cmd, Effect::Observe, "source with no file boundaries"));
            assert!(infer("python3 tools/case {item}", &ss.iter().collect::<Vec<_>>()).is_none(), "{cmd}");
        }
    }

    #[test]
    fn writes_network_and_unused_output_keep_it_environment() {
        // It reports writing a file.
        let ss: Vec<Session> = (0..3).map(|i| lookup_then_read(&format!("s{i}"), "wrote clinic/cache.json")).collect();
        let refs: Vec<&Session> = ss.iter().collect();
        assert!(infer("python3 tools/case {item}", &refs).is_none());
        // It says so on a line of its own, past a tag.
        let ss: Vec<Session> =
            (0..3).map(|i| lookup_then_read(&format!("s{i}"), "CARE-1\n[cache] Updated clinic/cache.json")).collect();
        let refs: Vec<&Session> = ss.iter().collect();
        assert!(infer("python3 tools/case {item}", &refs).is_none());
        // A git status right after it shows a change the agent did not make.
        let mut ss: Vec<Session> = (0..3).map(|i| lookup_then_read(&format!("s{i}"), "ok")).collect();
        ss[1].steps.insert(1, exec(1, "git status", Effect::Observe, "\tmodified:   clinic/db.json"));
        let refs: Vec<&Session> = ss.iter().collect();
        assert!(infer("python3 tools/case {item}", &refs).is_none());
        // A change listed after the agent's own command that writes is not the script's.
        let mut ss: Vec<Session> = (0..3).map(|i| lookup_then_read(&format!("s{i}"), "ok")).collect();
        ss[1].steps.insert(1, exec(1, "python3 -c <stdin>", Effect::Environment, ""));
        ss[1].steps.insert(2, exec(2, "git status", Effect::Observe, "\tmodified:   clinic/db.json"));
        let refs: Vec<&Session> = ss.iter().collect();
        assert!(infer("python3 tools/case {item}", &refs).is_some());
        // Its text, where a trace shows it, uses the network.
        let mut ss: Vec<Session> = (0..3).map(|i| lookup_then_read(&format!("s{i}"), "ok")).collect();
        ss[0].steps.insert(0, exec(0, "cat tools/case", Effect::Observe, "import urllib.request\n"));
        let refs: Vec<&Session> = ss.iter().collect();
        assert!(infer("python3 tools/case {item}", &refs).is_none());
        // Nothing reads what it printed.
        let ss: Vec<Session> = (0..3)
            .map(|i| session(&format!("s{i}"), vec![exec(0, "python3 tools/case CARE-1", Effect::Environment, "ok")]))
            .collect();
        let refs: Vec<&Session> = ss.iter().collect();
        assert!(infer("python3 tools/case {item}", &refs).is_none());
    }
}
