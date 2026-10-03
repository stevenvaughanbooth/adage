//! Track G G4 gates (docs/tracks/track-g-graph-engine.md §7, §10): context triggers at hook time.
//!
//! - **Fast path.** On every trace of every `evals/gen` newkinds bundle, with the routes the
//!   graph engine learns plus a context route, the hook's no-match fast path changes no decision
//!   and no context state (`adage_eval::replay`'s check, which compares the context symbols too).
//! - **A context fires in replay.** The context route `[edit registry/*.csv, make gen] → make
//!   check` fires in exactly the traces that ran `make gen` after editing the registry, at the
//!   turn of that `make gen`, and nowhere else.
//!
//! The suite is generated and rehearsed into a temporary directory (`gen.py --suite newkinds`,
//! `rehearse.py --no-learn`), or reused from `ADAGE_GEN_NEWKINDS=<dir>`; skipped without python3.
#![cfg(unix)]

use adage_core::{CommandTable, Effect};
use adage_eval::{replay, transcript};
use adage_tracks::*;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn suite(tmp: &Path) -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("ADAGE_GEN_NEWKINDS") {
        return Some(PathBuf::from(d));
    }
    if !Command::new("python3").arg("--version").output().is_ok_and(|o| o.status.success()) {
        eprintln!("python3 missing: the newkinds suite is skipped");
        return None;
    }
    let out = tmp.join("newkinds");
    let mut gen = Command::new("python3");
    gen.arg(root().join("evals/gen/gen.py")).args(["--suite", "newkinds", "--out"]).arg(&out);
    let ok = |mut c: Command| c.output().ok().filter(|o| o.status.success()).is_some();
    if !ok(gen) {
        eprintln!("gen.py failed: skipped");
        return None;
    }
    let mut reh = Command::new("python3");
    reh.arg(root().join("evals/gen/rehearse.py")).arg(&out).arg("--no-learn");
    if !ok(reh) {
        eprintln!("rehearse.py failed: skipped");
        return None;
    }
    Some(out)
}

/// The context route this gate adds: `make check` after a registry edit then `make gen`.
fn context_track(repo: &str) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: format!("{repo}/contexts"),
                version: "0.1.0".into(),
                description: "a context route for the G4 gate".into(),
                provenance: "authored".into(),
            },
            scope: Scope { repo: repo.into() },
            gauge: Gauge::default(),
        },
        routes: vec![Route {
            id: "context-check-after-gen".into(),
            summary: "make check after a registry edit and make gen".into(),
            trigger: Trigger::Context {
                items: vec![
                    CtxItem::Edit { glob: "registry/*.csv".into() },
                    CtxItem::Act { keys: vec!["exec make gen".into()] },
                ],
                within: 8,
            },
            step: Step::Run { command: "make check".into() },
            slots: vec![],
            effect: Effect::Verify,
            strength: Strength::Prime,
            state: RouteState::Firm,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence { support: 4, ..Default::default() },
            authored: true,
        }],
    }
}

#[test]
fn contexts_fire_in_replay_and_the_fast_path_stays_exact_on_newkinds() {
    let tmp = Tmp(std::env::temp_dir().join(format!("adage-g4-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&tmp.0);
    std::fs::create_dir_all(&tmp.0).unwrap();
    let Some(s) = suite(&tmp.0) else { return };
    let mut bundles: Vec<PathBuf> = std::fs::read_dir(&s)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("rehearsal/transcripts").is_dir())
        .collect();
    bundles.sort();
    assert!(!bundles.is_empty(), "no rehearsed bundle in {}", s.display());
    let table = CommandTable::load();
    for b in bundles {
        let name = b.file_name().unwrap().to_string_lossy().to_string();
        let tasks: serde_json::Value = serde_json::from_slice(&std::fs::read(b.join("tasks.json")).unwrap()).unwrap();
        let repo = tasks["repo"].as_str().unwrap().to_string();
        let mut transcripts: Vec<PathBuf> = Vec::new();
        for proj in std::fs::read_dir(b.join("rehearsal/transcripts")).unwrap().flatten() {
            for f in std::fs::read_dir(proj.path()).unwrap().flatten() {
                if f.path().extension().is_some_and(|x| x == "jsonl") {
                    transcripts.push(f.path());
                }
            }
        }
        transcripts.sort();
        // Learn with the graph engine, then add the context route.
        let home = tempfile::tempdir().unwrap();
        let cfg = adage_learn::LearnConfig::demo();
        let inputs = adage_learn::Inputs {
            identities: Default::default(),
            transcripts: transcripts.clone(),
            outcomes: Default::default(),
            repo_override: Some(repo.clone()),
            models: None,
            cache: None,
            only_repo: None,
        };
        let tracks_root = home.path().join("tracks");
        adage_learn::learn(&inputs, &tracks_root, &home.path().join("index.json"), &cfg).unwrap();
        let mut tracks = io::read_all(&tracks_root).unwrap();
        let ctx = context_track(&repo);
        io::validate(&ctx).unwrap();
        tracks.push(ctx);
        let (index, _) = Index::compile_with_conflicts(&tracks);
        assert!(!index.contexts(&repo).is_empty());
        let words = &index.triggers(&repo).words;
        assert!(words.iter().any(|w| w == "make"), "{words:?}");
        let (mut fired_in, mut expected_in, mut skipped, mut events) = (0, 0, 0, 0);
        for p in &transcripts {
            let t = transcript::read(p).unwrap();
            let r = replay::replay(
                &t,
                &adage_runtime::Ctx {
                    checkout: &adage_runtime::checkout::Disk,
                    index: &index,
                    repo: repo.to_string(),
                    root: &b.join("repo"),
                    table,
                    embed: None,
                    params: Default::default(),
                    policy: Default::default(),
                },
                true,
            );
            assert!(r.fast.mismatches.is_empty(), "{name}: {:#?}", r.fast.mismatches);
            skipped += r.fast.skipped;
            events += r.fast.events;
            // Expected: the turn of the first successful `make gen` after a registry edit, with at
            // most 8 alphabet events (registry edits, successful `make gen`) between them.
            let edit = glob::Pattern::new("registry/*.csv").unwrap();
            let alpha: Vec<(u32, bool, bool)> = r
                .actions
                .iter()
                .filter_map(|a| {
                    let is_edit = a.author && a.rel_path.as_deref().is_some_and(|p| edit.matches(p));
                    let is_gen = !a.failed && adage_core::pattern::meets("exec make gen", &a.key);
                    (is_edit || is_gen).then_some((a.turn, is_edit, is_gen))
                })
                .collect();
            let want = (0..alpha.len())
                .find_map(|q| (alpha[q].2 && (0..q).any(|p| alpha[p].1 && q - p <= 8)).then_some(alpha[q].0));
            let got: Vec<u32> =
                r.fired.iter().filter(|f| f.directive.route == "context-check-after-gen").map(|f| f.turn).collect();
            assert_eq!(got, want.into_iter().collect::<Vec<_>>(), "{name}: {}", t.id);
            fired_in += usize::from(!got.is_empty());
            expected_in += usize::from(want.is_some());
            if let Some(f) = r.fired.iter().find(|f| f.directive.route == "context-check-after-gen") {
                assert_eq!(f.event, "PostToolBatch");
                assert!(f.directive.text.contains("\n  make check "), "{}", f.directive.text);
            }
        }
        eprintln!("{name}: the context route fired in {fired_in} traces ({expected_in} expected); fast path skipped {skipped} of {events} events, no mismatch");
        assert!(fired_in >= 2, "{name}: the registry traces run make gen after the edit");
        assert!(skipped > 0 && skipped < events);
    }
}
