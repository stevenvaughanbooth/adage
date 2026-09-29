//! `adage train cards`: replay the commands of a repository's procedure cards on a clean checkout
//! in adage-exec's sandbox, and record the verdicts (`adage_learn::cards::VERDICTS_FILE`) that the
//! next learn applies: a command that works there is marked so on the card; one that does not is
//! no longer listed as working. The `adage` binary (hooks, learning) never runs anything, so this
//! is the step a person runs, as they run `adage train <skill>`.

use crate::verify::{unverifiable, LEND};
use adage_exec::replay::{Replay, ReplayLimits};
use adage_learn::cards::{load_verdicts, Verdicts, VERDICTS_FILE};
use adage_tracks::{Index, Step};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

/// One replayed command of a card.
#[derive(Debug, Clone, Serialize)]
pub struct Checked {
    pub card: String,
    pub command: String,
    /// Whether it worked; `None` when it was not replayed to an exit (no verdict recorded).
    pub works: Option<bool>,
    pub summary: String,
}

/// Replay every command of the cards of the repository at `root` (its key in the index at
/// `home`), and merge the verdicts into `home`'s verdicts file.
pub fn verify(home: &Path, root: &Path, wall_ms: u64) -> Result<Vec<Checked>> {
    let index = Index::try_load(&home.join("index.json"))
        .map_err(|e| anyhow::anyhow!("reading the index (run adage condition first): {e:?}"))?;
    let repo = adage_core::repo::repo_key(root);
    let mut out = Vec::new();
    let mut verdicts: Verdicts = load_verdicts(home);
    for ir in index.routes.iter().filter(|r| r.repo == repo) {
        let Step::Card { card } = &ir.route.step else { continue };
        let mut cmds: Vec<String> = card.commands.iter().map(|c| c.command.clone()).collect();
        if let Some(f) = &card.finish {
            if !cmds.contains(f) {
                cmds.push(f.clone());
            }
        }
        for cmd in cmds {
            let argv: Vec<String> = cmd.split_whitespace().map(str::to_string).collect();
            // A finishing check anti-unified from several traces (`go test ./<…>`) has a
            // placeholder, not an argument: replayed literally it always fails (review S9).
            if cmd.contains('<') || cmd.contains('…') {
                out.push(Checked {
                    card: ir.route.id.clone(),
                    command: cmd,
                    works: None,
                    summary: "not replayed: it has a placeholder where each task names its own".into(),
                });
                continue;
            }
            // Installing packages needs the network, which the sandbox does not have: a failure
            // there says nothing about the command, so it gets no verdict.
            if adage_core::action::classify_argv(&argv).role == adage_core::action::Role::EnvSetup {
                out.push(Checked {
                    card: ir.route.id.clone(),
                    command: cmd,
                    works: None,
                    summary: "not replayed: it installs packages, and the sandbox has no network".into(),
                });
                continue;
            }
            let mut rp = Replay::new(root, ReplayLimits { wall_ms, ..Default::default() })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let lent: Vec<String> = LEND.iter().filter(|d| rp.lend(d)).map(|d| d.to_string()).collect();
            // Only a command that ran gets a verdict (a missing tool says nothing about it), and
            // only when its failure is its own: not the network or a module cache the sandbox
            // lacks, nor a lent environment it cannot use (review S9).
            let (works, summary) = match rp.run(&argv) {
                Ok(r) if !r.timed_out && !r.ok() && unverifiable(&argv, &r.output, &lent).is_some() => {
                    let why = unverifiable(&argv, &r.output, &lent).unwrap_or_default();
                    (None, format!("{}: not verified: {why}", r.summary()))
                }
                Ok(r) if !r.timed_out => {
                    verdicts.entry(ir.route.id.clone()).or_default().insert(cmd.clone(), r.ok());
                    (Some(r.ok()), r.summary())
                }
                Ok(r) => (None, format!("{}: not recorded", r.summary())),
                Err(e) => (None, format!("not replayed: {e}")),
            };
            out.push(Checked { card: ir.route.id.clone(), command: cmd, works, summary });
        }
    }
    let p = home.join(VERDICTS_FILE);
    std::fs::create_dir_all(p.parent().context("the verdicts file has a parent")?)?;
    std::fs::write(&p, serde_json::to_vec_pretty(&verdicts)?)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_tracks::card::{Card, CardFile};
    use adage_tracks::*;

    /// Review S9: a finishing check anti-unified to `go test ./<…>` is never replayed literally,
    /// so it never gets a "does not work" verdict that the next learn would act on.
    #[test]
    fn a_placeholder_is_not_replayed() {
        let d = tempfile::tempdir().unwrap();
        let (home, root) = (d.path().join("home"), d.path().join("repo"));
        std::fs::create_dir_all(&root).unwrap();
        let card = Card {
            sessions: 2,
            passed: 2,
            files: vec![CardFile { path: "flag_<x>.go".into(), sessions: 2 }],
            exemplars: vec![],
            commands: vec![],
            avoid: vec![],
            finish: Some("go test ./<…>".into()),
            prompts: vec![],
        };
        let route = Route {
            id: "card-1".into(),
            summary: String::new(),
            trigger: Trigger::Intent {
                family: "card-1".into(),
                model: "m".into(),
                centroid: vec![1; 8],
                threshold: 0.8,
            },
            step: Step::Card { card },
            slots: vec![],
            effect: adage_core::Effect::Observe,
            strength: Strength::Prime,
            state: RouteState::Forming,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence { support: 2, opportunities: 2, days: 1, ..Default::default() },
            authored: false,
        };
        let track = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "r/learned".into(),
                    version: "0.0.0".into(),
                    description: String::new(),
                    provenance: String::new(),
                },
                scope: Scope { repo: adage_core::repo::repo_key(&root) },
                gauge: Gauge::default(),
            },
            routes: vec![route],
        };
        Index::compile(&[track]).write_atomic(&home.join("index.json")).unwrap();
        let out = verify(&home, &root, 1_000).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].works, None, "{}", out[0].summary);
        assert!(out[0].summary.contains("placeholder"), "{}", out[0].summary);
        assert!(load_verdicts(&home).is_empty(), "no verdict recorded");
    }
}
