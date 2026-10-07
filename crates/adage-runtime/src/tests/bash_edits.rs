//! An edit made through Bash (the first live run: every schema edit was a `python3 - <<'E'`
//! program, and none armed the finish line): a file a redirection, `tee` or an inline Python
//! program writes is an edit of that path for the triggers, the context alphabets, the observed
//! keys and the finish line, as an edit tool's is; the plain normalisation is untouched.

use super::*;

/// The Bash shapes the live run's agents edited a schema with, each writing `schema/invoice.json`.
fn bash_edits() -> Vec<String> {
    vec![
        "cat > schema/invoice.json <<'EOF'\n{\"id\": {\"type\": \"str\"}}\nEOF".into(),
        "cat <<'EOF' | tee schema/invoice.json > /dev/null\n{}\nEOF".into(),
        "printf '%s\\n' '{}' > schema/invoice.json".into(),
        "python3 - <<'E'\np='schema/invoice.json'\ns=open(p).read()\nopen(p,'w').write(s.replace('a','b'))\nE".into(),
        "python3 -c \"open('schema/invoice.json','w').write('{}')\"".into(),
    ]
}

fn plain_ctx<'a>(dir: &'a tempfile::TempDir, index: &'a Index, table: &'a CommandTable) -> Ctx<'a> {
    Ctx {
        checkout: &crate::checkout::Disk,
        index,
        repo: "acme".into(),
        root: dir.path(),
        table,
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    }
}

/// Each shape arms the finish line as an Edit of the file does: the directive, the obligation on
/// the path, the first edit's batch, the `write {path}` key in what is observed and in the
/// history ring; the Stop gate asks for `make gen` once. The fast path decides each batch the same.
#[test]
fn an_edit_made_through_bash_arms_the_finish_line() {
    let (dir, index, table) = fixture();
    let ctx = plain_ctx(&dir, &index, table);
    for cmd in bash_edits() {
        let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
        let d = decide_both(&ctx, &mut st, &mut fast_st, &Event::Batch { calls: vec![bash(&cmd, "")] });
        assert!(d.directives.iter().any(|x| x.route == "gen" && x.text.contains("make gen")), "{cmd}: {d:?}");
        assert_eq!(st.obligations.len(), 1, "{cmd}");
        assert_eq!((st.obligations[0].edited.as_str(), st.obligations[0].met), ("schema/invoice.json", false), "{cmd}");
        assert_eq!(st.first_edit_at_batch, Some(1), "{cmd}");
        assert!(d.observed.iter().any(|k| k == "write {path}"), "{cmd}: {:?}", d.observed);
        assert!(st.history.iter().any(|k| k == "write {path}"), "{cmd}: {:?}", st.history);
        let stop = Event::Stop { stop_hook_active: false };
        assert!(decide(&ctx, &mut st, &stop).continue_reason.is_some_and(|r| r.contains("make gen")), "{cmd}");
    }
    // A command that writes nothing the trigger names arms nothing.
    let mut st = SessionState::default();
    for cmd in ["cat schema/invoice.json", "make test > /dev/null 2>&1", "printf x > notes.txt"] {
        decide(&ctx, &mut st, &Event::Batch { calls: vec![bash(cmd, "")] });
    }
    assert!(st.obligations.is_empty(), "{:?}", st.obligations);
}

/// The live run's own shape: the program's write and `make gen` in one Bash call. The write arms
/// the finish line and the `make gen` after it meets it, in that order, so the Stop gate has
/// nothing to ask; the same when `make gen` follows the heredoc's start line.
#[test]
fn the_step_that_meets_it_in_the_same_command_is_seen_after_the_edit() {
    let (dir, index, table) = fixture();
    let ctx = plain_ctx(&dir, &index, table);
    for cmd in [
        "python3 - <<'E'\np='schema/invoice.json'\ns=open(p).read()\nopen(p,'w').write(s)\nE\nmake gen && make ci",
        "cat > schema/invoice.json <<'EOF' && make gen\n{}\nEOF",
    ] {
        let mut st = SessionState::default();
        let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash(cmd, "ok")] });
        assert!(d.directives.iter().any(|x| x.route == "gen"), "{cmd}: {d:?}");
        assert!(
            d.judgements.iter().any(|j| matches!(j, Judgement::ObligationMet { route } if route == "gen")),
            "{cmd}"
        );
        assert!(st.obligations[0].met, "{cmd}");
        assert!(decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false }).continue_reason.is_none(), "{cmd}");
    }
}

/// A context's `edit` item is met by a Bash edit too: the registry written by a heredoc, then
/// `make gen`, fires the context's `make check`.
#[test]
fn a_bash_edit_is_an_edit_in_the_context_alphabet() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let routes = vec![ctx_route(
        "run-ctx",
        vec![CtxItem::Edit { glob: "registry/*.csv".into() }, act_item("exec make gen")],
        3,
        Step::Run { command: "make check".into() },
        Effect::Verify,
    )];
    let t = track("acme/graph", "desire-path", routes);
    adage_tracks::io::validate(&t).unwrap();
    let index = Index::compile(&[t]);
    let ctx = plain_ctx(&dir, &index, table);
    let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
    let write = bash("cat >> registry/currencies.csv <<'EOF'\nGBP\nEOF", "");
    decide_both(&ctx, &mut st, &mut fast_st, &Event::Batch { calls: vec![write] });
    let d = decide_both(&ctx, &mut st, &mut fast_st, &Event::Batch { calls: vec![bash("make gen", "ok")] });
    assert!(d.directives.iter().any(|x| x.route == "run-ctx" && x.text.contains("make check")), "{d:?}");
}

/// A state rebuilt from the spool reads a Bash edit's `write {path}` key as the edit it was.
#[test]
fn a_rebuilt_state_keeps_a_bash_edit() {
    let (dir, index, table) = fixture();
    let ctx = plain_ctx(&dir, &index, table);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash(&bash_edits()[3], "")] });
    let record = json!({ "kind": "batch", "arm": "treated", "repo": "acme", "decision": d });
    let rebuilt = crate::rebuild::rebuild(&[record], &index, "acme", table).unwrap();
    assert_eq!(rebuilt.first_edit_at_batch, Some(1));
    assert_eq!(rebuilt.obligations.iter().map(|o| o.route.as_str()).collect::<Vec<_>>(), ["gen"]);
}
