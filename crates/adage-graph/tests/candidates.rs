//! The candidate pass (plan D1; test plan T-D1-01, T-D1-03, T-D1-09): it never changes the
//! reflexes' mining, keeps only the negative border's sequences, and gives each candidate the id
//! its pattern gets as a reflex.
mod support;

use adage_core::CommandTable;
use adage_graph::compile::{self, CompileParams, Corpus, Input};
use adage_graph::events::{self, EventSession, LSession};
use adage_graph::input::{Annotated, Prompt, Session, Step, StepMeta};
use adage_graph::mine::{self, MineParams};
use adage_graph::{proto, select};
use adage_tracks::{Index, Trigger};

/// The datasets under `evals/datasets` with a manifest (the ones the miner reads).
fn datasets() -> Vec<(String, Vec<EventSession>)> {
    let dir = support::repo_root().join("evals/datasets");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("manifest.json").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.into_iter().filter_map(|n| support::dataset(&n).map(|s| (n, s))).filter(|(_, s)| !s.is_empty()).collect()
}

/// T-D1-01: the reflexes' pass is the same, byte for byte, with the candidate pass on or off, on
/// synthetic sessions and on every dataset, and its budget is never exhausted.
#[test]
fn the_reflexes_pass_is_untouched() {
    let mut corpora = vec![("synthetic sessions".to_string(), support::synthetic_sessions(80, 30, 11))];
    corpora.extend(datasets());
    assert!(corpora.len() > 1, "the datasets are in the checkout");
    for (k, (name, sessions)) in corpora.into_iter().enumerate() {
        // Both presets on the synthetic sessions, the default (what a home learns with) on the
        // datasets.
        let presets = if k == 0 {
            vec![select::Params::default(), select::Params::demo()]
        } else {
            vec![select::Params::default()]
        };
        for p in presets {
            let off = select::learn(&sessions, &p);
            let on = select::learn(&sessions, &select::Params { candidates: true, ..p.clone() });
            assert_eq!(serde_json::to_string(&off).unwrap(), serde_json::to_string(&on).unwrap(), "{name}");
            assert_eq!(off.unselected.len(), on.unselected.len(), "{name}");
            assert!(off.candidates.is_empty(), "{name}: off means off");
            assert!(!on.truncated, "{name}: the reflexes' budget ran out");
        }
    }
}

/// The pass itself: every pattern it keeps is a sequence (two or more context items) seen in at
/// least `s_min − border` traces (and never fewer than 2) but fewer than `s_min`.
#[test]
fn the_pass_keeps_the_border_sequences_only() {
    assert_eq!(mine::candidate_floor(5, 2), 3);
    assert_eq!(mine::candidate_floor(3, 2), 2, "never a single trace");
    assert_eq!(mine::candidate_floor(2, 2), 2);
    let sessions = support::synthetic_sessions(80, 30, 11);
    let epoch = proto::build(&sessions, &Default::default());
    let corpus = epoch.label(&sessions);
    let all: Vec<&LSession> = corpus.sessions.iter().collect();
    for min_support in [3, 5, 8] {
        let p = MineParams { min_support, ..Default::default() };
        let (db, mined) = mine::mine_candidates(&corpus.labels, &all, &p);
        let s_min = p.s_min(all.len());
        assert_eq!(mined.s_min, s_min, "the reflexes' s_min");
        assert_eq!(db.s_min, mine::candidate_floor(s_min, p.border), "the second alphabet's floor");
        assert!(!mined.patterns.is_empty(), "min_support {min_support}: a border to mine");
        for m in &mined.patterns {
            let n = m.sessions.len() as u32;
            assert!((db.s_min..s_min).contains(&n), "{n} outside [{}, {s_min})", db.s_min);
            assert!(m.pattern.cut >= 2, "a sequence");
        }
    }
    // No room below s_min: nothing.
    let p = MineParams { min_support: 2, ..Default::default() };
    assert!(mine::mine_candidates(&corpus.labels, &all, &p).1.patterns.is_empty());
}

fn trace(id: &str, day: &str, cmds: &[&str]) -> (Annotated, EventSession) {
    let steps: Vec<Step> = cmds
        .iter()
        .map(|c| match c.strip_prefix("e ") {
            Some(p) => Step::Edit { path: p.into() },
            None => Step::Exec {
                argv: c.split_whitespace().map(str::to_string).collect(),
                output: "ok".into(),
                failed: false,
            },
        })
        .collect();
    let meta: Vec<StepMeta> =
        (0..steps.len()).map(|i| StepMeta { turn: i as u32, effect: String::new(), ..Default::default() }).collect();
    let session = Session { id: id.into(), prompts: vec![Prompt { text: "fix it".into(), at_step: 0 }], steps };
    let a = Annotated { session, meta, day: day.into() };
    let m = events::Meta {
        day: Some(day.into()),
        turns: Some(a.meta.iter().map(|m| m.turn).collect()),
        ..Default::default()
    };
    let e = events::from_input(&a.session, &m, &CommandTable::load());
    (a, e)
}

/// T-D1-03 at the compiler: a candidate's id is the id the same pattern gets when it is selected
/// as a reflex, and a candidate is always a context of two or three events, flagged.
#[test]
fn a_candidate_compiles_to_the_id_of_its_reflex() {
    let mut annotated = Vec::new();
    let mut sessions = Vec::new();
    for i in 0..10 {
        let day = format!("2026-09-{:02}", i + 1);
        let cmds: Vec<&str> = if i < 4 {
            vec!["make deps", "e acme/a.py", "make test", "make lint"]
        } else {
            vec!["e acme/a.py", "make test"]
        };
        let (a, e) = trace(&format!("s{i}"), &day, &cmds);
        annotated.push(a);
        sessions.push(e);
    }
    let p = select::Params { candidates: true, ..select::Params::default() };
    let learned = select::learn(&sessions, &p);
    assert!(!learned.candidates.is_empty());
    let corpus = Corpus::of(&annotated, &[]);
    let params = CompileParams::default();
    let input = |reflexes| Input { reflexes, corpus: &corpus, values: None, priors: &[], params: &params };
    let out = compile::compile_candidates(&input(&learned.candidates));
    assert!(!out.routes.is_empty(), "{:#?}", out.dropped);
    for c in &out.routes {
        assert!(c.candidate);
        let Trigger::Context { items, .. } = &c.route.trigger else { panic!("{:?}", c.route.trigger) };
        assert!((2..=3).contains(&items.len()));
        assert_eq!(c.sessions, ["s0", "s1", "s2", "s3"]);
    }
    // The same reflexes, as if the cover had selected them with a posterior over the bar: the
    // ids the routes get are the candidates' own.
    let selected: Vec<select::Reflex> = learned
        .candidates
        .iter()
        .map(|r| select::Reflex { gain: 1.0, conf: 0.9, conf_lb: 0.9, value: 1.0, ..r.clone() })
        .collect();
    let routes = compile::compile(&input(&selected));
    let route_ids: Vec<&str> = routes.routes.iter().map(|c| c.route.id.as_str()).collect();
    for c in &out.routes {
        assert!(route_ids.contains(&c.route.id.as_str()), "{} not in {route_ids:?}", c.route.id);
        assert!(routes.routes.iter().all(|r| !r.candidate));
    }
}

/// T-D1-09: every index recorded with the datasets still loads.
#[test]
fn every_recorded_index_loads() {
    let dir = support::repo_root().join("evals/datasets");
    let mut n = 0;
    for d in std::fs::read_dir(&dir).unwrap().flatten() {
        for f in std::fs::read_dir(d.path().join("recorded")).into_iter().flatten().flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            if name.starts_with("index-") && name.ends_with(".json") {
                let index = Index::try_load(&f.path()).unwrap_or_else(|e| panic!("{}: {e:?}", f.path().display()));
                assert!(!index.routes.is_empty(), "{}", f.path().display());
                assert!(index.candidates.is_empty(), "recorded before candidates");
                n += 1;
            }
        }
    }
    assert_eq!(n, 26, "the recorded indexes");
}
