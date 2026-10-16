//! pytest as the hook reads it, never from a run's output: which tests a command line selects
//! ([`pytest_calls`], [`covers`]), the node ids its cache records ([`pytest_nodeids`],
//! [`test_evidence`]), and what a failed run's `lastfailed` record says about that run
//! ([`failure_record`]).

use adage_hook::Turn;
use adage_runtime::render;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// pytest's own record of collected node ids, a cache file the approval declares.
pub(super) const PYTEST_NODEIDS: &str = ".pytest_cache/v/cache/nodeids";
/// pytest's record of the node ids that failed in its last run (also a declared cache file).
pub(super) const PYTEST_LASTFAILED: &str = ".pytest_cache/v/cache/lastfailed";

/// The largest pytest cache record read.
const MAX_RECORD_BYTES: u64 = 4 << 20;

/// The most test names one receipt or failure lists.
pub(super) const MAX_LISTED_TESTS: usize = 12;

/// A pytest cache record at `path` if it is a regular file within [`MAX_RECORD_BYTES`]:
/// `Some(None)` when there is none yet, `None` when it cannot be read as one.
fn read_record(path: &std::path::Path) -> Option<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(None),
        Ok(m) if m.is_file() && m.len() <= MAX_RECORD_BYTES => std::fs::read(path).ok().map(Some),
        _ => None,
    }
}

/// A pytest cache record's keys (node ids, or file ids for collection errors), or `None` when it
/// is not a JSON object within [`MAX_RECORD_BYTES`].
fn record_keys(bytes: &[u8]) -> Option<BTreeSet<String>> {
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return None;
    }
    let map: serde_json::Map<String, Value> = serde_json::from_slice(bytes).ok()?;
    Some(map.into_iter().map(|(k, _)| k).collect())
}

/// What a failed staged run's declared pytest failure record says about *this* run
/// ([`render::FailureRecord`]), read from the staged change set and the worktree, never from
/// output; `None` when the command is not a pytest run whose approval declares the record.
///
/// pytest's `lastfailed` is cumulative: an entry stays until that test passes or is skipped, and
/// pytest rewrites the file only when its content changes. So the run's record counts only when
/// this run rewrote it (it is in the plan, or [`crate::stage::Staged::rewritten`]); then, within
/// the command's targets, entries it added are this run's failures (file ids, without `::`, are
/// collection errors), and entries it kept from before are failures it did not clear. Anything
/// else gives no names: never a stale list.
///
/// The listed failing tests are also described beyond their names ([`failing_cases`]): from their
/// own source and from `new_tests` (the runtime's names of test functions this session added).
pub(super) fn failure_record(
    staged: &crate::stage::Staged,
    command: &str,
    new_tests: &[String],
) -> Option<render::FailureRecord> {
    let call = pytest_command(command)?;
    if !crate::stage::matches_any(&staged.action.cache, PYTEST_LASTFAILED) {
        return None;
    }
    let unavailable = Some(render::FailureRecord::default());
    // The worktree's record is the one the run started from: a failed run applies nothing.
    let read_worktree = || -> Option<Vec<u8>> {
        read_record(&staged.repo.join(PYTEST_LASTFAILED)).map(|b| b.unwrap_or_else(|| b"{}".to_vec()))
    };
    let after = match staged.plan.apply.iter().find(|c| c.path == PYTEST_LASTFAILED) {
        Some(crate::stage::Change { after: crate::stage::After::File { bytes, .. }, .. }) => bytes.clone(),
        Some(_) => return unavailable,
        // Rewritten with the same bytes: refreshed, and identical to the worktree's.
        None if staged.rewritten.iter().any(|p| p == PYTEST_LASTFAILED) => match read_worktree() {
            Some(b) => b,
            None => return unavailable,
        },
        None => return unavailable,
    };
    let (Some(after), Some(before)) = (record_keys(&after), read_worktree().as_deref().and_then(record_keys)) else {
        return unavailable;
    };
    let scoped = |ids: &BTreeSet<String>| -> Vec<String> {
        ids.iter()
            .filter(|id| (valid_nodeid(id) || valid_file_id(id)) && in_targets(id, &call.targets))
            .cloned()
            .collect()
    };
    let (after, before): (Vec<String>, BTreeSet<String>) = (scoped(&after), scoped(&before).into_iter().collect());
    // A file that failed to collect ran none of its tests: its node ids say nothing about this run.
    let broken: Vec<&String> = after.iter().filter(|id| !id.contains("::")).collect();
    let ran = |id: &&String| !broken.iter().any(|f| id.starts_with(&format!("{f}::")));
    let mut failing = Vec::new();
    let mut collection = Vec::new();
    let mut uncleared = Vec::new();
    for id in after.iter().filter(ran) {
        match (before.contains(id), id.contains("::")) {
            (false, true) => failing.push(id.clone()),
            (false, false) => collection.push(id.clone()),
            (true, _) => uncleared.push(id.clone()),
        }
    }
    // One bound for all three lists, in the order they are read.
    let mut left = MAX_LISTED_TESTS;
    let mut listed = |mut ids: Vec<String>| {
        let n = ids.len().min(left);
        left -= n;
        let more = ids.len() - n;
        ids.truncate(n);
        render::Listed { names: ids, more }
    };
    let (failing, collection, uncleared) = (listed(failing), listed(collection), listed(uncleared));
    let cases = failing_cases(staged, failing.names.iter().chain(&uncleared.names), new_tests);
    Some(render::FailureRecord { refreshed: true, failing, collection, uncleared, cases })
}

/// The listed failing tests as a failed run's receipt describes them beyond their names
/// ([`render::FailingCase`]), never from the run's output: what each test's own source says
/// ([`adage_runtime::pytest_case::facts`]), read from the file the run used ([`run_source`]), and
/// whether this session's edits added its function (`new_tests`, by name, as a passing receipt
/// names them).
fn failing_cases<'a>(
    staged: &crate::stage::Staged,
    ids: impl Iterator<Item = &'a String>,
    new_tests: &[String],
) -> Vec<render::FailingCase> {
    let ids: Vec<&String> = ids.collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let ignore = adage_core::gitignore::Ignore::new(&staged.repo);
    let base = staged.repo.canonicalize().ok();
    // Each test file is read and parsed once.
    let mut texts: BTreeMap<&str, Option<String>> = BTreeMap::new();
    for file in ids.iter().filter_map(|id| id.split_once("::").map(|(f, _)| f)) {
        texts.entry(file).or_insert_with(|| base.as_deref().and_then(|b| run_source(staged, b, file, &ignore)));
    }
    let sources: BTreeMap<&str, adage_runtime::pytest_case::TestSource> = texts
        .iter()
        .filter_map(|(f, t)| Some((*f, adage_runtime::pytest_case::TestSource::new(t.as_deref()?)?)))
        .collect();
    ids.into_iter()
        .filter(|id| id.contains("::"))
        .map(|id| render::FailingCase {
            id: id.clone(),
            facts: id.split_once("::").and_then(|(f, _)| sources.get(f)).and_then(|s| s.facts(id)),
            added: new_tests.iter().any(|n| n == test_function(id)),
        })
        .collect()
}

/// A test file's text as the run read it: through the orientation card's read gate
/// ([`adage_runtime::orient::readable`]: no dotfile, ignored, secret-holding, oversized or linked
/// file), and only when its bytes are the ones in the run's input snapshot.
fn run_source(
    staged: &crate::stage::Staged,
    base: &std::path::Path,
    rel: &str,
    ignore: &adage_core::gitignore::Ignore,
) -> Option<String> {
    let text = adage_runtime::orient::readable(base, rel, ignore)?;
    let (sha, _) = staged.inputs.get(rel)?.split_once(':')?;
    (crate::sha256_hex(text.as_bytes()) == sha).then_some(text)
}

/// A repository-relative Python file as pytest names it: no absolute path, no `..` or empty
/// part, only `[A-Za-z0-9_./-]`.
fn valid_py_file(file: &str) -> bool {
    file.ends_with(".py")
        && !file.starts_with('/')
        && !file.split('/').any(|p| p == ".." || p.is_empty())
        && file.chars().all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c))
}

/// A pytest file id as its failure record names a module that failed to collect.
fn valid_file_id(id: &str) -> bool {
    id.len() <= 200 && !id.contains("::") && valid_py_file(id)
}

/// A well-formed `path.py::name` node id inside the repository.
pub(super) fn valid_nodeid(id: &str) -> bool {
    let Some((file, name)) = id.split_once("::") else { return false };
    id.len() <= 200
        && valid_py_file(file)
        && !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "_:.-[]".contains(c))
}

/// Whether a node or file id lies inside a pytest call's targets (every id, for a call that names
/// none: it collects the whole configured suite).
pub(super) fn in_targets(id: &str, targets: &[String]) -> bool {
    let file = id.split_once("::").map_or(id, |(f, _)| f);
    targets.is_empty()
        || targets.iter().any(|t| file == t || file.starts_with(&format!("{t}/")) || id.starts_with(&format!("{t}::")))
}

/// A node id's test function: its last part, without parameters (`a.py::C::test_x[1]` → `test_x`).
pub(super) fn test_function(id: &str) -> &str {
    let last = id.rsplit("::").next().unwrap_or(id);
    last.split_once('[').map_or(last, |(name, _)| name)
}

/// The node ids in the worktree's pytest record, when this is a pytest verification whose approval
/// declares that cache file: `Some(empty)` when there is no record yet, `None` when not applicable
/// or unreadable. Only well-formed `path.py::name` ids inside the repository are kept.
pub(super) fn pytest_nodeids(
    root: &std::path::Path,
    req: &crate::request::ExecRequest,
    command: &str,
) -> Option<BTreeSet<String>> {
    let declared = req.action.as_ref().is_some_and(|a| crate::stage::matches_any(&a.cache, PYTEST_NODEIDS));
    if !declared || pytest_command(command).is_none() {
        return None;
    }
    let Some(bytes) = read_record(&root.join(PYTEST_NODEIDS))? else { return Some(Default::default()) };
    let ids: Vec<String> = serde_json::from_slice(&bytes).ok()?;
    Some(ids.into_iter().filter(|id| valid_nodeid(id)).collect())
}

/// The applied run's test evidence: its pytest record, bound to the journal (the file now in the
/// worktree must be the applied output the receipt recorded), restricted to the command's targets.
/// The tests this session's edits added (`edited`, by function name: the runtime's `new_tests`)
/// are named when the record has them; otherwise the ids new since the last recorded run.
pub(super) fn test_evidence(
    turn: &Turn<'_>,
    req: &crate::request::ExecRequest,
    command: &str,
    operation: Option<&str>,
    before: Option<&BTreeSet<String>>,
    edited: &[String],
) -> Option<render::TestEvidence> {
    let before = before?;
    let call = pytest_command(command)?;
    let receipt =
        crate::journal::list(&turn.paths.root).ok()?.into_iter().rev().find(|r| Some(r.id.as_str()) == operation)?;
    let crate::stage::FileState::File { sha256, .. } = receipt.outputs.get(PYTEST_NODEIDS)? else { return None };
    if crate::sha256_hex(&std::fs::read(turn.root.join(PYTEST_NODEIDS)).ok()?) != *sha256 {
        return None;
    }
    let after = pytest_nodeids(turn.root, req, command)?;
    let inside = |id: &String| in_targets(id, &call.targets);
    let collected = before.is_empty().then(|| after.iter().filter(|id| inside(id)).count());
    let yours: Vec<String> =
        after.iter().filter(|id| inside(id) && edited.iter().any(|n| n == test_function(id))).cloned().collect();
    let (new, yours) = if yours.is_empty() {
        (after.difference(before).filter(|id| inside(id)).cloned().collect::<Vec<String>>(), false)
    } else {
        (yours, true)
    };
    if collected.is_none() && new.is_empty() {
        return None;
    }
    let more_new = new.len().saturating_sub(MAX_LISTED_TESTS);
    Some(render::TestEvidence { collected, new: new.into_iter().take(MAX_LISTED_TESTS).collect(), more_new, yours })
}

/// pytest options that change only what is printed or when a passing run stops, never which
/// tests run (registered with the launch-gate scorer).
const OUTPUT_FLAGS: &[&str] =
    &["-q", "-qq", "-v", "-vv", "-s", "-x", "-rA", "-ra", "--no-header", "--disable-warnings", "--quiet", "--verbose"];

/// A pytest invocation: interpreter word, non-output options, sorted targets.
#[derive(Debug, PartialEq)]
pub(super) struct PytestCall {
    pub(super) interpreter: String,
    pub(super) options: Vec<String>,
    pub(super) targets: Vec<String>,
}

/// Commands a pytest call may share a command line with, because they only read or print (a
/// pipe to `tail`, an `echo` after): none changes the directory, the environment or a file.
const PASSIVE: &[&str] =
    &["tail", "head", "grep", "egrep", "fgrep", "echo", "printf", "true", "wc", "sort", "uniq", "cut"];

/// The pytest invocations in a plain command line (`&&`, `||`, `;` and `|` lists, with `2>&1`).
/// `None` for anything a word split cannot read faithfully (quotes, expansions, redirections,
/// globs), for no pytest at all, and for a line whose other commands could change what pytest
/// sees: `cd`, `pushd`, `popd`, `export`, a variable assignment, or any command not in
/// [`PASSIVE`] (review F5).
pub(super) fn pytest_calls(command: &str, root: &std::path::Path) -> Option<Vec<PytestCall>> {
    if command.chars().any(|c| "'\"`$<(){}*?[]#\\\n".contains(c)) {
        return None;
    }
    let words: Vec<&str> = command.split_whitespace().filter(|w| *w != "2>&1").collect();
    if words.iter().any(|w| w.contains('>')) {
        return None;
    }
    let prefix = format!("{}/", root.to_string_lossy());
    let mut calls = Vec::new();
    for segment in words.split(|w| matches!(*w, "&&" | "||" | ";" | "|")) {
        let argv: Vec<&str> = segment.iter().map(|w| w.strip_prefix(prefix.as_str()).unwrap_or(w)).collect();
        if let Some(call) = pytest_call(&argv) {
            calls.push(call);
            continue;
        }
        let Some(first) = argv.first() else { continue };
        if first.contains('=') || !PASSIVE.contains(first) {
            return None;
        }
        if argv.iter().any(|w| w.contains("pytest") || w.ends_with(';') || w.contains('&')) {
            return None;
        }
    }
    (!calls.is_empty()).then_some(calls)
}

/// A whole command line as one pytest invocation ([`pytest_call`] over its words).
pub(super) fn pytest_command(command: &str) -> Option<PytestCall> {
    pytest_call(&command.split_whitespace().collect::<Vec<_>>())
}

/// `<python> -m pytest …` as a [`PytestCall`]; `None` for any other argv.
pub(super) fn pytest_call(argv: &[&str]) -> Option<PytestCall> {
    let python = |w: &str| {
        let name = w.rsplit('/').next().unwrap_or(w);
        name == "python"
            || name
                .strip_prefix("python")
                .is_some_and(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
    };
    let (interpreter, rest) = match argv {
        [i, "-m", "pytest", rest @ ..] if python(i) => (i.to_string(), rest),
        _ => return None,
    };
    let (mut options, mut targets) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i];
        if a == "-p" && rest.get(i + 1) == Some(&"no:cacheprovider") {
            i += 2;
            continue;
        }
        let output = OUTPUT_FLAGS.contains(&a)
            || a.starts_with("--tb=")
            || a.starts_with("--color=")
            || (a.len() > 1
                && a.starts_with('-')
                && !a.starts_with("--")
                && a[1..].chars().all(|c| "qvsx".contains(c)));
        if !output {
            if a.starts_with('-') {
                options.push(a.to_string());
            } else {
                targets.push(a.trim_end_matches('/').to_string());
            }
        }
        i += 1;
    }
    targets.sort();
    targets.dedup();
    Some(PytestCall { interpreter, options, targets })
}

/// Whether `done` already ran every test `asked` selects: same options, and each requested
/// target is one of its targets or a node or path inside one. A different interpreter word
/// is accepted only for a bare system `python`, which the advice beside this points at the
/// project's environment; the note says so.
pub(super) fn covers(done: &PytestCall, asked: &PytestCall) -> bool {
    let within = |t: &str| {
        done.targets.iter().any(|s| t == s || t.starts_with(&format!("{s}::")) || t.starts_with(&format!("{s}/")))
    };
    let interpreter = asked.interpreter == done.interpreter || !asked.interpreter.contains('/');
    interpreter
        && asked.options == done.options
        && !done.targets.is_empty()
        && !asked.targets.is_empty()
        && asked.targets.iter().all(|t| within(t))
}

#[cfg(test)]
mod tests {
    use super::super::stop_gate::gate_answer;
    use super::super::testkit::*;
    use super::*;
    use adage_hook::StopGate;
    use adage_runtime::Event;
    use serde_json::json;

    #[test]
    fn pytest_scope_equivalence_is_narrow() {
        let root = std::path::Path::new("/repo");
        let done = pytest_call(&[".venv/bin/python", "-m", "pytest", "tests/test_x.py", "-q"]).unwrap();
        let covered = |c: &str| pytest_calls(c, root).is_some_and(|v| v.iter().all(|a| covers(&done, a)));
        for yes in [
            ".venv/bin/python -m pytest tests/test_x.py -q",
            ".venv/bin/python -m pytest tests/test_x.py::test_one -xvs",
            "/repo/.venv/bin/python -m pytest /repo/tests/test_x.py --tb=short -p no:cacheprovider 2>&1 | tail -20",
            "python -m pytest tests/test_x.py::test_one -v && .venv/bin/python -m pytest tests/test_x.py -q",
        ] {
            assert!(covered(yes), "{yes}");
        }
        for no in [
            ".venv/bin/python -m pytest tests/test_x.py -k one",
            ".venv/bin/python -m pytest tests/",
            ".venv/bin/python -m pytest",
            ".venv/bin/python -m pytest tests/test_y.py",
            "other/bin/python -m pytest tests/test_x.py",
            ".venv/bin/pytest tests/test_x.py",
            ".venv/bin/python -m pytest \"$T\"",
            ".venv/bin/python -m pytest tests/test_x.py > out.txt",
            "git status",
        ] {
            assert!(!covered(no), "{no}");
        }
    }

    #[test]
    fn receipt_lists_tests_from_the_declared_pytest_record_bound_to_the_journal() {
        let f = pytest_fixture();
        let req = f.request();
        let command = ".venv/bin/python -m pytest tests/test_x.py -q";
        let record = f.root.join(PYTEST_NODEIDS);
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        let ids = |v: &[&str]| serde_json::to_vec(&v).unwrap();
        let before = pytest_nodeids(&f.root, &req, command).unwrap();
        assert!(before.is_empty(), "no record yet");
        std::fs::write(
            &record,
            ids(&[
                "tests/test_x.py::test_one",
                "tests/test_x.py::test_new[a-b]",
                "tests/other.py::t",
                "/etc/x.py::t",
                "bad id",
            ]),
        )
        .unwrap();
        let write_receipt = |sha: &str| {
            let mut outputs = BTreeMap::new();
            outputs
                .insert(PYTEST_NODEIDS.to_string(), crate::stage::FileState::File { sha256: sha.into(), exec: false });
            journal(&f, &crate::journal::Receipt { outputs, ..completed("fixture-session", applied(), "x") });
        };
        let event = Event::Batch { calls: vec![] };
        let turn = f.turn(&event);
        write_receipt("not-the-applied-output");
        assert!(
            test_evidence(&turn, &req, command, Some(""), Some(&before), &[]).is_none(),
            "unbound record is ignored"
        );
        write_receipt(&crate::sha256_hex(&std::fs::read(&record).unwrap()));
        let t = test_evidence(&turn, &req, command, Some(""), Some(&before), &[]).unwrap();
        assert_eq!(t.collected, Some(2), "only the command's targets, only well-formed ids");
        assert_eq!(t.new, vec!["tests/test_x.py::test_new[a-b]".to_string(), "tests/test_x.py::test_one".to_string()]);
        // With an earlier record, only the new ids are listed and no total is claimed.
        let earlier: BTreeSet<String> = ["tests/test_x.py::test_one".to_string()].into();
        // The session's own new tests, by function name, are named instead (dev-v8: a fresh
        // home listed the first 12 of 69 ids, never the agent's own).
        let mine = test_evidence(&turn, &req, command, Some(""), Some(&before), &["test_one".to_string()]).unwrap();
        assert_eq!(
            (mine.collected, mine.new.clone(), mine.yours),
            (Some(2), vec!["tests/test_x.py::test_one".to_string()], true)
        );
        let param = test_evidence(&turn, &req, command, Some(""), Some(&before), &["test_new".to_string()]).unwrap();
        assert_eq!(param.new, vec!["tests/test_x.py::test_new[a-b]".to_string()], "parameters are the same test");
        let absent = test_evidence(&turn, &req, command, Some(""), Some(&before), &["test_gone".to_string()]).unwrap();
        assert!(!absent.yours && absent.new.len() == 2, "a name the record lacks falls back to the listing");
        assert_eq!(test_function("a.py::C::test_x[1-2]"), "test_x");
        let t = test_evidence(&turn, &req, command, Some(""), Some(&earlier), &[]).unwrap();
        assert_eq!((t.collected, t.new.len()), (None, 1));
        let text = render::action_predicted(
            &check_directive(),
            command,
            &[],
            true,
            &render::CpuReceipt { tests: Some(&t), operation: "op", input_digest: "d", exit: Some(0), duration_ms: 5 },
        );
        assert!(text.contains("new since the last recorded run: tests/test_x.py::test_new[a-b]"), "{text}");
        assert!(text.contains("stays current until a file changes"), "{text}");
        assert!(!pytest_nodeids(&f.root, &req, "sh tools/check.sh").is_some(), "only pytest commands");
    }

    /// Review F2: pytest's `lastfailed` is cumulative, so names come only from what this run
    /// changed in it, within the command's targets: new entries failed now, file ids are
    /// collection errors (their files' node ids say nothing), kept entries were not cleared, and
    /// another file's earlier failure is not this run's. A record this run did not rewrite gives
    /// no names at all.
    #[test]
    fn failing_tests_are_only_what_this_run_recorded_within_its_targets() {
        let f = pytest_fixture();
        let command = ".venv/bin/python -m pytest tests/test_b.py tests/test_c.py -q";
        let before = r#"{"tests/test_a.py::test_other": true, "tests/test_b.py::test_old": true, "tests/test_c.py::test_stale": true}"#;
        let after = r#"{"tests/test_a.py::test_other": true, "tests/test_b.py::test_old": true, "tests/test_b.py::test_new": true, "tests/test_c.py": true, "tests/test_c.py::test_stale": true, "/etc/x.py::t": true}"#;
        let r = failure_record(&failed_run(&f, Some(before), Some(after), false), command, &[]).unwrap();
        assert!(r.refreshed);
        assert_eq!(r.failing.names, ["tests/test_b.py::test_new"]);
        assert_eq!(r.collection.names, ["tests/test_c.py"]);
        assert_eq!(r.uncleared.names, ["tests/test_b.py::test_old"]);
        let text = render::stop_gate_failed(command, Some(1), Some(&r));
        assert!(text.contains("Failing tests: tests/test_b.py::test_new."), "{text}");
        assert!(text.contains("Collection errors (the file could not be imported) in: tests/test_c.py."), "{text}");
        assert!(!text.contains("test_other") && !text.contains("test_stale"), "{text}");
        // The agent already saw these failures: the run left the record byte-identical, so pytest
        // did not rewrite it. Nothing can be attributed to this run.
        let r = failure_record(&failed_run(&f, Some(before), None, false), command, &[]).unwrap();
        assert!(!r.refreshed && r.failing.is_empty() && r.uncleared.is_empty());
        let text = render::stop_gate_failed(command, Some(1), Some(&r));
        assert!(
            text.contains("not updated by this run, so no failing test names") && !text.contains("test_old"),
            "{text}"
        );
        // Rewritten with the same bytes (refreshed by this run): what it kept, it did not clear.
        let r = failure_record(&failed_run(&f, Some(before), None, true), command, &[]).unwrap();
        assert!(r.refreshed && r.failing.is_empty());
        assert_eq!(r.uncleared.names, ["tests/test_b.py::test_old", "tests/test_c.py::test_stale"]);
        // No earlier record: everything in it is this run's.
        let r =
            failure_record(&failed_run(&f, None, Some(r#"{"tests/test_b.py::test_new": true}"#), false), command, &[])
                .unwrap();
        assert_eq!(r.failing.names, ["tests/test_b.py::test_new"]);
        // Names are bounded across the lists.
        let many: serde_json::Map<String, Value> =
            (0..20).map(|i| (format!("tests/test_b.py::test_{i:02}"), json!(true))).collect();
        let r =
            failure_record(&failed_run(&f, None, Some(&Value::Object(many).to_string()), false), command, &[]).unwrap();
        assert_eq!((r.failing.names.len(), r.failing.more), (MAX_LISTED_TESTS, 20 - MAX_LISTED_TESTS));
        // Not a pytest run: nothing is said about test names.
        assert!(failure_record(&failed_run(&f, Some(before), Some(after), false), "sh tools/check.sh", &[]).is_none());
    }

    /// The held-out b08 shape (launch-gate-v8): a session's own parametrized test that declares a
    /// valid (latitude, longitude) pair invalid. The run's record names only the case's id.
    const B08_TESTS: &str = r#"import pytest

from marshmallow import validate
from marshmallow.exceptions import ValidationError


def test_and():
    validator = validate.And(validate.Range(min=0))
    assert validator(2)


@pytest.mark.parametrize(
    "valid_latlong",
    [
        (0, 0),
        (45, 90),
        (-45, -90),
        (90, 180),
        (-90, -180),
        (0.5, 0.5),
        (-45.5, 135.5),
        (45.0, -135.0),
    ],
)
def test_latlong_valid(valid_latlong):
    validator = validate.LatLong()
    assert validator(valid_latlong) == valid_latlong


@pytest.mark.parametrize(
    "invalid_latlong",
    [
        (91, 0),
        (-91, 0),
        (0, 181),
        (0, -181),
        (90.1, 0),
        (-90.1, 0),
        (0, 180.1),
        (0, -180.1),
        (100, 200),
        (45,),
        (45, 90, 45),
        [],
        [45, 90],
        "45, 90",
        45,
        None,
        (None, 0),
        (0, None),
        ("invalid", 0),
        (0, "invalid"),
    ],
)
def test_latlong_invalid(invalid_latlong):
    validator = validate.LatLong()
    with pytest.raises(ValidationError):
        validator(invalid_latlong)
"#;

    /// A failed run's receipt and the finish-line continuation describe a failing test beyond its
    /// id, from the test source the run used (never the run's output): the parametrized case with
    /// its value, and the test's one check. A test this session added is said to be the agent's
    /// own; one it did not add is not. A source that changed since the run is not described.
    #[test]
    fn a_failing_case_is_described_from_the_source_the_run_used() {
        let f = pytest_fixture();
        std::fs::write(f.root.join("tests/test_validate.py"), B08_TESTS).unwrap();
        let command = ".venv/bin/python -m pytest tests/test_validate.py -q";
        let id = "tests/test_validate.py::test_latlong_invalid[invalid_latlong12]";
        let mut staged = failed_run(&f, None, Some(&json!({ id: true }).to_string()), false);
        staged.inputs = crate::stage::input_snapshot(&f.root.canonicalize().unwrap(), &staged.action).unwrap();
        let added = ["test_latlong_valid".to_string(), "test_latlong_invalid".to_string()];
        let r = failure_record(&staged, command, &added).unwrap();
        assert_eq!(r.failing.names, [id]);
        let case = &r.cases[0];
        assert!(case.added, "{case:?}");
        let facts = case.facts.as_ref().expect("the source the run used is read");
        let line = |needle: &str| B08_TESTS.lines().position(|l| l.contains(needle)).unwrap() + 1;
        assert_eq!(facts.case, Some((line("        [45, 90],"), Some("invalid_latlong = [45, 90]".into()))));
        assert_eq!(facts.check, Some((line("pytest.raises"), "with pytest.raises(ValidationError):".into())));

        let detail = format!(
            "From the test source the run used (pytest's own failure messages are not passed on):\n\
             test_latlong_invalid[invalid_latlong12] is the case invalid_latlong = [45, 90] (tests/test_validate.py line {}); \
             the test's only check is line {}: with pytest.raises(ValidationError):\n\
             test_latlong_invalid[invalid_latlong12] was added in this session; check it against the request before changing the implementation.",
            line("        [45, 90],"),
            line("pytest.raises")
        );
        let receipt = render::action_failed(&check_directive(), command, Some(1), Some(&r), "959cb36ea93c0123");
        let expected = format!(
            "Adage ran `{command}` after your edits: it failed (exit 1; receipt 959cb36ea93c). Nothing was changed.\n\
             Failing tests: {id}.\n{detail}\n</adage-reflex>"
        );
        assert!(receipt.ends_with(&expected), "{receipt}");
        assert!(receipt.len() < 1200, "{}", receipt.len());
        assert!(render::named(&receipt).commands == [command], "only the command reads as named: {receipt}");
        // The finish-line continuation says the same, in one paragraph.
        let failed = crate::stage::Outcome::Failed { exit: Some(1), signal: None };
        let StopGate::Block(gate) = gate_answer(&failed, command, &staged, &added, true, false) else { panic!() };
        assert!(gate.starts_with(render::FINISH_LINE) && gate.ends_with(render::NOT_FROM_USER), "{gate}");
        assert!(gate.contains(&format!("Failing tests: {id}. {}", detail.replace('\n', " "))), "{gate}");
        assert!(adage_hook::harness::is_own_text(&gate), "{gate}");
        assert_eq!(render::named(&gate).commands, [command], "{gate}");

        // A failing test this session did not add: described, not flagged.
        let r = failure_record(&staged, command, &["test_other".to_string()]).unwrap();
        assert!(!r.cases[0].added && r.cases[0].facts.is_some());
        let text = render::action_failed(&check_directive(), command, Some(1), Some(&r), "959cb36ea93c0123");
        assert!(
            text.contains("is the case invalid_latlong = [45, 90]") && !text.contains("added in this session"),
            "{text}"
        );

        // The file changed after the run: its text is not the one that ran, so nothing is read.
        std::fs::write(f.root.join("tests/test_validate.py"), format!("{B08_TESTS}\n# edited\n")).unwrap();
        let r = failure_record(&staged, command, &added).unwrap();
        assert!(r.cases[0].facts.is_none() && r.cases[0].added, "{:?}", r.cases);
        let text = render::stop_gate_failed(command, Some(1), Some(&r));
        assert!(!text.contains("From the test source") && text.contains("was added in this session"), "{text}");
    }
}
