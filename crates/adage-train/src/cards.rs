//! `adage train cards`: replay the commands of a repository's procedure cards on a clean checkout
//! in adage-exec's sandbox, and record the verdicts (`adage_learn::cards::VERDICTS_FILE`) that the
//! next learn applies: a command that works there is marked so on the card; one that does not is
//! no longer listed as working. The `adage` binary (hooks, learning) never runs anything, so this
//! is the step a person runs, as they run `adage train <skill>`.

use crate::verify::LEND;
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
            for d in LEND {
                rp.lend(d);
            }
            // Only a command that ran gets a verdict (a missing tool says nothing about it).
            let (works, summary) = match rp.run(&argv) {
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
