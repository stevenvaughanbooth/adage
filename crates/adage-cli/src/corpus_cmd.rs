use crate::cli::{CorpusAction, CorpusArgs};
use adage_learn::{
    corpus,
    store::{Files, Store},
    LearnConfig,
};
use adage_store::Paths;
use anyhow::{ensure, Context, Result};
use clap::FromArgMatches;
use std::io::{BufReader, BufWriter, Write};

pub fn run(matches: &clap::ArgMatches, paths: &Paths) -> Result<()> {
    let args = CorpusArgs::from_arg_matches(matches)?;
    let store = Files::new(&paths.tracks(), &paths.index());
    match args.action {
        CorpusAction::Export { transcripts, repo, identities } => {
            let identities = identities
                .map(|p| -> Result<_> { Ok(serde_json::from_slice(&std::fs::read(p)?)?) })
                .transpose()?
                .unwrap_or_default();
            let mut out = BufWriter::new(std::io::stdout().lock());
            corpus::export(&transcripts, repo.as_deref(), &identities, &mut out)?;
            out.flush()?;
        }
        CorpusAction::Import { input, repo, replace } => {
            let source = std::fs::File::open(&input).with_context(|| format!("cannot read {}", input.display()))?;
            crate::writable_data_dir(paths)?;
            let _lock = adage_store::lock::exclusive(&paths.tracks_lock())?;
            let mode = if replace { corpus::Mode::Replace } else { corpus::Mode::Append };
            let report = corpus::import(BufReader::new(source), &repo, mode, Default::default(), &store)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            ensure!(report.committed, "corpus rejected; the committed evidence snapshot is unchanged");
        }
        CorpusAction::Inspect { repo } => {
            let manifest = store.corpus_manifest(&repo)?.context("no canonical corpus for this repository")?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
        CorpusAction::Learn { repo, demo } => {
            crate::writable_data_dir(paths)?;
            let _lock = adage_store::lock::exclusive(&paths.tracks_lock())?;
            let cfg = if demo { LearnConfig::demo() } else { LearnConfig::default() };
            let report = corpus::learn(&repo, Some(&paths.models()), &store, &cfg)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}
