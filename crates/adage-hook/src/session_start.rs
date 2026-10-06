//! A session start: its record and arm, the bootstrap or rebind learner, the extension's session
//! texts, and the one line for the person. Also the `seen` record a session in a repository with
//! no reflexes yet leaves at its events ([`record_seen`]). Entry: [`session_start`], from
//! `handle`.

use crate::ctx::HookCtx;
use crate::extension::guarded;
use crate::failopen::{self, Failure};
use crate::harness::Output;
use crate::record::{Record, RecordKind};
use crate::respond::first_lapse_notice;
use crate::{autolearn, site, SessionTexts, Site};
use adage_core::repo;
use adage_runtime::{Arm, Event, SessionState};
use adage_store::{state, Paths};
use adage_tracks::Index;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// A session start (docs/zero-config.md §3.2, §3.3):
///
/// 1. In a repository with reflexes, the session is assigned its arm now and one `session_start`
///    record is spooled (`arm`, `propensity`, `repo`, `generation`): every exposed session is
///    then counted by the savings estimate, including those where nothing ever matched, and the
///    bootstrap session below, whose reflexes arrive while it runs.
/// 2. **Bootstrap.** With `learn.auto` on, a repository with no reflexes whose learner has not
///    run since the index last changed, or a damaged index, starts a learner at once
///    (`condition --auto --bootstrap`): instincts and the index within about 100 ms, then the
///    repository's own traces.
/// 3. **Notice.** One line for the person (Claude Code's `systemMessage`; the model never sees
///    it): at the first bootstrap, what Adage is doing; afterwards, the learner's latest notice,
///    at most once a day and only when new. Rebuilding a damaged index says nothing.
pub(crate) fn session_start(
    s: &HookCtx<'_>,
    damaged: bool,
    learner: &dyn Fn() -> Result<PathBuf, Failure>,
) -> Result<Option<String>, Failure> {
    let routed = site::routed_key(s.index(), s.root, s.paths, false);
    if let Some(repo_key) = &routed {
        if let Err(f) = record_start(s, repo_key) {
            failopen::record(&s.paths.root, s.harness, s.event, &f);
        }
    }
    // Outside a repository: nothing to learn, so no learner and nothing to say. (Not even a
    // damaged index is rebuilt from here: the next session in a repository does that.)
    if !s.in_repo {
        return Ok(None);
    }
    let cfg = s.config();
    let key = routed.clone().unwrap_or_else(|| s.repo_key().to_string());
    let q = autolearn::Queue::new(s.paths, &key);
    let run: Option<autolearn::RunState> = autolearn::read_json(&q.state());
    let first = run.is_none() && !q.shown().exists();
    let stale = || {
        let index_at = std::fs::metadata(s.paths.index())
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64);
        match (&run, index_at) {
            (None, _) => true,
            (Some(r), Some(at)) => r.finished_at.is_some_and(|f| f < at),
            (Some(_), None) => true,
        }
    };
    // A repository `learn.repos` leaves out is recorded, never learned (`autolearn::on_turn_end`).
    let learns = cfg.learn_auto && cfg.learns(&key);
    let bootstrap = learns && (damaged || (routed.is_none() && stale()));
    // The bootstrap session is a session in this repository too: its reflexes arrive while it
    // runs, so it is assigned its arm now and counted by the savings estimate like any other
    // (without this record it never was, and the card's session count lagged one behind).
    if bootstrap && routed.is_none() {
        if let Err(f) = record_start(s, &key) {
            failopen::record(&s.paths.root, s.harness, s.event, &f);
        }
    } else if routed.is_none() {
        // No reflexes here yet, and none on the way this session: seen, not exposed.
        record_seen(s, &Event::SessionStart, &key);
    }
    // Instincts bind against the live checkout, this session's (docs/instincts.md): bound against
    // another one (a per-task worktree, a clone the traces name), or before its declared files
    // changed (a `.venv` made since), they are bound again, by the same fast first phase as a
    // bootstrap; so are they when other instinct packs are selected (`adage packs off`) or this build
    // ships other base packs. A few dozen `stat`s decide it (`adage_instincts::fingerprint`).
    let rebind = !bootstrap
        && !damaged
        && learns
        && cfg.instincts_enabled
        && adage_pack::binding::stale_binding(
            &s.paths.tracks(),
            &key,
            s.root,
            &adage_pack::binding::selection(&cfg.instincts_packs, &cfg.instincts_disabled),
        );
    if bootstrap || rebind {
        let asked = learner().and_then(|l| autolearn::on_turn_end(s.paths, s.root, s.adapter.name(), None, &l, true));
        if let Err(f) = asked {
            failopen::record(&s.paths.root, s.harness, s.event, &f);
        }
    }
    // The command hooks' set on purpose: a session start is always a command hook's (the mod never
    // sends one), and the contract is delivered on that hook's own context channel.
    let context = if !damaged
        && s.adapter.harness().capabilities().session_contract
        && state::try_read::<SessionState>(&s.paths.state(s.session))
            .ok()
            .flatten()
            .is_some_and(|st| st.arm == Some(Arm::Treated))
    {
        let site = Site { repo_key: &key, root: s.root, index: s.index(), paths: s.paths };
        s.opts.extension.map(|ext| guarded(|| ext.session_texts(&site))).unwrap_or_default()
    } else {
        SessionTexts::default()
    };
    // A grant that stopped matching is said once per session, beside any other notice.
    let lapse = context.notice.filter(|_| cfg.ui_notices && first_lapse_notice(s.paths, s.session));
    let context = context.context.filter(|text| text.len() <= s.adapter.max_context());
    if damaged || !cfg.ui_notices {
        return Ok(
            context.and_then(|context| s.adapter.render(s.event, Output::SessionContext { context, notice: None }))
        );
    }
    // Set up from a terminal (`adage init --yes`, `adage condition`): reflexes are in place before
    // the first session here, so there is no bootstrap to announce. Say what is in effect, once.
    let ready = first && !bootstrap && cfg.learn_auto && routed.is_some();
    let text = if (bootstrap || ready) && first {
        let now = autolearn::now_ms();
        let id = if ready { "ready" } else { "bootstrap" };
        match autolearn::write_json(&q.shown(), &autolearn::Shown { id: id.into(), at: now }) {
            Ok(()) if ready => Some(ready_notice(s.index(), &key)),
            Ok(()) => Some(first_notice(s.root, cfg)),
            // The data directory cannot be written: nothing can be learned, and a notice shown now
            // could not be remembered, so it would repeat at every session start. Say what is
            // wrong instead, once.
            Err(e) => unwritable_notice(s.paths, &e),
        }
    } else {
        autolearn::take_notice(s.paths, &key)
    };
    let text = match (text, lapse) {
        (Some(t), Some(l)) => Some(format!("{t}\n{l}")),
        (t, l) => t.or(l),
    };
    Ok(match context {
        Some(context) => s.adapter.render(s.event, Output::SessionContext { context, notice: text }),
        None => text.and_then(|t| s.adapter.render(s.event, Output::Notice(t))),
    })
}

/// What the first session in a repository says: how many of its earlier traces Adage is learning
/// from (Claude Code project directories, counted by name: nothing is opened here).
fn first_notice(root: &Path, cfg: &adage_store::config::Config) -> String {
    let projects = cfg
        .learn_claude_projects
        .clone()
        .or_else(|| adage_store::home_dir().map(|h| h.join(".claude").join("projects")));
    let n = projects
        .map(|p| adage_ingest::scope::count_claude_sessions(&p, &adage_ingest::scope::checkouts(root)))
        .unwrap_or(0);
    match n {
        0 => "adage: learning from this repo as you work; its first reflexes form in a few sessions. Nothing runs without you.".into(),
        1 => "adage: learning from 1 past session in this repo; reflexes arrive within a minute. Nothing runs without you.".into(),
        n => format!("adage: learning from {n} past sessions in this repo; reflexes arrive within a minute. Nothing runs without you."),
    }
}

/// What the first session says in a repository set up from a terminal: what is in effect (as the
/// status card counts it: reflexes said to the agent, learned and instincts) and that Adage keeps
/// learning.
fn ready_notice(index: &Index, repo: &str) -> String {
    let (mut learned, mut instincts) = (0usize, 0usize);
    for r in index.for_repo(repo).filter(|r| !r.exec_only()) {
        if r.route.is_instinct() {
            instincts += 1;
        } else {
            learned += 1;
        }
    }
    let n = |k: usize, one: &str, many: &str| if k == 1 { format!("1 {one}") } else { format!("{k} {many}") };
    let what = match (learned, instincts) {
        (0, 0) => return "adage: set up in this repo; it keeps learning as you work. Nothing runs without you.".into(),
        (0, i) => n(i, "instinct", "instincts"),
        (l, 0) => n(l, "reflex", "reflexes"),
        (l, i) => format!("{} and {}", n(l, "reflex", "reflexes"), n(i, "instinct", "instincts")),
    };
    format!("adage: {what} active in this repo; it keeps learning as you work. Nothing runs without you.")
}

/// The one line said when the data directory cannot be written (at most once per data
/// directory: a marker in the system's temporary directory remembers it; if that cannot be
/// written either, nothing is said rather than the same line at every session start).
fn unwritable_notice(paths: &Paths, e: &std::io::Error) -> Option<String> {
    let dir = paths.root.to_string_lossy();
    let marker = std::env::temp_dir().join(format!("adage-unwritable-{}", repo::short_hash(&dir)));
    std::fs::OpenOptions::new().write(true).create_new(true).open(marker).ok()?;
    let why = e.to_string();
    let why = why.split(" (os error").next().unwrap_or(&why).to_lowercase();
    Some(format!(
        "adage: can't write its data directory {dir} ({why}), so it cannot learn here. Set ADAGE_HOME to a writable directory; `adage doctor` checks it."
    ))
}

/// Assign the session's arm (if it has none yet) and spool one `session_start` record.
fn record_start(s: &HookCtx<'_>, repo_key: &str) -> Result<(), Failure> {
    let arms = s.arms();
    let (arm, propensity) = state::update::<SessionState, _>(&s.paths.state(s.session), |st| {
        arms.ensure(st, s.session, repo_key);
        (st.arm, st.propensity)
    })
    .map_err(|e| Failure::io("state", "assigning the session's arm", &e))?;
    let mut rec = Record::new(RecordKind::SessionStart, s.event)
        .with("arm", arm)
        .with("propensity", propensity)
        .with("repo", repo_key)
        .with("generation", &s.index().generation)
        .harness(s.adapter)
        .timed(s.started);
    if let Some(bin) = serving_bin() {
        rec.set("bin", bin);
    }
    rec.append(s.paths, s.session)
}

/// Which binary served a session's start, by its file name: the Claude Code plugin's shim links
/// `adage` or `adage-exec` at every start (plan A8), and exactly one of them answers its hooks.
fn serving_bin() -> Option<String> {
    // macOS may report the shim's `bin/hook` symlink; resolve it so the record identifies
    // the verified adage or adage-exec binary, as it already does on Linux.
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
}

/// One record that a session event happened in a repository with no reflexes yet: the event, the
/// repository and how many tool calls, so `adage watch` and the status card show every session
/// Adage sees (docs/design/everyday-ux.md, state S1). Nothing else: no arm or propensity (the
/// savings estimate counts only sessions exposed to reflexes), no decision (a rebuild replays only
/// decisions), and no command, path or prompt. A failure is recorded and forgotten.
pub(crate) fn record_seen(ctx: &HookCtx<'_>, ev: &Event, repo_key: &str) {
    let HookCtx { adapter, event, session, paths, started, .. } = *ctx;
    let calls = match ev {
        Event::Batch { calls } => calls.len(),
        Event::Failure { .. } => 1,
        _ => 0,
    };
    let mut rec = Record::new(RecordKind::of(ev), event).with("repo", repo_key).with("unrouted", true);
    if calls > 0 {
        rec.set("calls", calls);
    }
    let mut rec = rec.harness(adapter).timed(started);
    if matches!(ev, Event::SessionStart) {
        if let Some(bin) = serving_bin() {
            rec.set("bin", bin);
        }
    }
    rec.append_or_log(paths, session, adapter.name(), event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::Reads;
    use crate::harness;
    use crate::{Delivery, Extension, Options, Turn};
    use adage_runtime::Directive;
    use serde_json::Value;
    use std::time::Instant;

    #[test]
    fn session_contract_and_person_notice_use_separate_channels() {
        use crate::harness::Adapter;
        let value = harness::claude::Claude
            .render(
                "SessionStart",
                Output::SessionContext { context: "model contract".into(), notice: Some("person notice".into()) },
            )
            .unwrap();
        let value: Value = serde_json::from_str(&value).unwrap();
        assert_eq!(value["hookSpecificOutput"]["additionalContext"], "model contract");
        assert_eq!(value["systemMessage"], "person notice");
    }

    #[test]
    fn session_contract_is_treatment_only_and_independent_of_ui_notices() {
        struct Contract;
        impl Extension for Contract {
            fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
                Delivery::default()
            }
            fn session_context(&self, _: &Site<'_>) -> Option<String> {
                Some("test execution contract".into())
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths::at(temp.path().join("home"));
        std::fs::create_dir_all(&paths.root).unwrap();
        std::fs::write(paths.root.join("config.toml"), "[learn]\nauto = false\n[ui]\nnotices = \"off\"\n").unwrap();
        static CONTRACT: Contract = Contract;
        let p = Value::Null;
        let start = |extension: Option<&'static dyn Extension>| HookCtx {
            adapter: &harness::claude::Claude,
            harness: "claude",
            event: "SessionStart",
            p: &p,
            session: "contract-test",
            cwd: temp.path(),
            root: temp.path(),
            in_repo: true,
            paths: &paths,
            started: Instant::now(),
            opts: Options { learner: None, extension },
            reads: Reads::default(),
        };
        let learner = || Err(Failure::new("test", "no learner needed"));
        for (arm, expected) in [(Arm::Control, false), (Arm::Treated, true)] {
            state::update::<SessionState, _>(&paths.state("contract-test"), |st| st.arm = Some(arm)).unwrap();
            let result = session_start(&start(Some(&CONTRACT)), false, &learner).unwrap();
            assert_eq!(result.is_some(), expected);
            if let Some(result) = result {
                let result: Value = serde_json::from_str(&result).unwrap();
                assert_eq!(result["hookSpecificOutput"]["additionalContext"], "test execution contract");
                assert!(result.get("systemMessage").is_none());
            }
        }
        assert!(session_start(&start(Some(&CONTRACT)), true, &learner).unwrap().is_none());
        assert!(session_start(&start(None), false, &learner).unwrap().is_none());
    }
}
