//! `adage marketplace`: where packs come from, as Claude Code's `/plugin marketplace` lists plugin
//! marketplaces (`docs/marketplace.md`).
//!
//! A marketplace is a repository's `.adage/marketplace.toml`: the packs it offers, each pinned by its
//! digest. The first is the Adage repository's own, built into this binary: Adage's base instincts.
//! Every marketplace reaches this machine the same way (`adage_pack::marketplace::Loaded::sync`):
//! its packs installed where `adage add` installs a pack, at their pins, each recorded as from it,
//! and the ones it no longer lists removed. Learning syncs the built-in one before it binds
//! instincts; remote marketplaces will be fetched, then synced the same way.
//!
//! - `adage marketplace [--json]`: each marketplace, its home, and how each of its packs stands
//!   here: installed as pinned, not installed yet, or installed at other bytes.
//! - `adage marketplace show <name>`: its manifest, as published.
//! - `adage marketplace update [<name>]`: sync it now.

use adage_pack::marketplace::{Installed, Loaded};
use adage_store::Paths;
use adage_term::{Glyph, Line, Out, Role};
use anyhow::{anyhow, Context, Result};
use serde_json::json;

const USAGE: &str = "usage: adage marketplace [show <name> | update [<name>]] [--json]";

/// The marketplaces this build knows: the Adage repository's, built in.
fn marketplaces() -> Result<Vec<&'static Loaded>> {
    let b = adage_pack::builtin::marketplace().map_err(|e| anyhow!("the built-in marketplace cannot be read: {e}"))?;
    Ok(vec![b])
}

fn find(name: &str) -> Result<&'static Loaded> {
    marketplaces()?
        .into_iter()
        .find(|m| m.name() == name)
        .ok_or_else(|| crate::hint(format!("no marketplace named {name}"), "`adage marketplace` lists them"))
}

/// How the known marketplaces reach this machine: built into this binary (remote ones will be
/// fetched, then synced the same way).
fn built_in() -> String {
    format!("built into Adage {}", env!("CARGO_PKG_VERSION"))
}

pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    let name = args.get(1).filter(|a| !a.starts_with("--")).cloned();
    match args.first().map(String::as_str) {
        Some("show") => show(&name.context(USAGE)?),
        Some("update") => update(name.as_deref(), paths),
        _ => list(paths, args.iter().any(|a| a == "--json")),
    }
}

fn state(i: Installed) -> &'static str {
    match i {
        Installed::Current => "current",
        Installed::No => "no",
        Installed::Differs => "differs",
    }
}

fn list(paths: &Paths, json: bool) -> Result<()> {
    let ms = marketplaces()?;
    let tracks = paths.tracks();
    if json {
        let v: Vec<_> = ms
            .iter()
            .map(|m| {
                let packs: Vec<_> = m
                    .status(&tracks)
                    .into_iter()
                    .map(|(e, i)| {
                        json!({
                            "name": e.name, "version": e.version, "digest": e.digest,
                            "default": e.default, "installed": state(i),
                        })
                    })
                    .collect();
                let info = &m.manifest.marketplace;
                json!({
                    "name": info.name, "repository": info.repository, "description": info.description,
                    "source": "builtin", "adage": env!("CARGO_PKG_VERSION"), "packs": packs,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json!({ "marketplaces": v }))?);
        return Ok(());
    }
    let mut o = Out::stdout();
    o.header("marketplace", &[&crate::words::count(ms.len(), "marketplace")])?;
    for m in &ms {
        let info = &m.manifest.marketplace;
        let status = m.status(&tracks);
        let current = status.iter().filter(|(_, i)| *i == Installed::Current).count();
        o.blank()?;
        o.line(
            &Line::new()
                .glyph(Role::Accent, Glyph::Active)
                .text(" ")
                .bold(Role::Ink, info.name.clone())
                .sep()
                .quiet(built_in()),
        )?;
        if let Some(r) = &info.repository {
            o.kv("home", Line::new().text(r.clone()))?;
        }
        if !info.description.is_empty() {
            o.kv("about", Line::new().quiet(info.description.clone()))?;
        }
        let summary = if current == status.len() {
            format!("{}, every one installed as pinned", crate::words::count(status.len(), "pack"))
        } else {
            format!(
                "{}, {current} installed as pinned; `adage marketplace update` or the next condition installs the rest",
                crate::words::count(status.len(), "pack")
            )
        };
        o.kv("packs", Line::new().text(summary))?;
        let w = status.iter().map(|(e, _)| e.name.chars().count()).max().unwrap_or(16).clamp(16, 40);
        for (e, i) in &status {
            let l = Line::new().pad_to(13).text(e.name.clone()).pad_to(15 + w).quiet(e.version.clone()).pad_to(23 + w);
            o.line(&match i {
                Installed::Current => l.push(Role::Fire, "installed"),
                Installed::No => l.faint("not installed yet"),
                Installed::Differs => l.push(Role::Prime, "installed at other bytes: an update replaces it"),
            })?;
        }
    }
    o.blank()?;
    o.next("adage packs", "which packs apply here, and turn any off")?;
    o.also("adage marketplace show adage", "its manifest: every pack it offers, pinned by digest")?;
    Ok(())
}

fn show(name: &str) -> Result<()> {
    let m = find(name)?;
    let mut o = Out::stdout();
    o.header("marketplace show", &[name, &built_in()])?;
    o.blank()?;
    o.line(&Line::new().faint("── ").text(adage_pack::marketplace::MANIFEST))?;
    for l in m.manifest_text().lines() {
        o.line(&Line::new().text(l.to_string()))?;
    }
    Ok(())
}

fn update(name: Option<&str>, paths: &Paths) -> Result<()> {
    let ms = match name {
        Some(n) => vec![find(n)?],
        None => marketplaces()?,
    };
    crate::writable_data_dir(paths)?;
    let _tracks = adage_store::lock::exclusive(&paths.tracks_lock())?;
    let mut o = Out::stdout();
    let names: Vec<&str> = ms.iter().map(|m| m.name()).collect();
    o.header("marketplace update", &[&names.join(", ")])?;
    o.blank()?;
    let mut removed = false;
    for m in ms {
        let s = m.sync(&paths.tracks())?;
        if s.is_empty() {
            o.kv(m.name(), Line::new().text("in sync: every pack installed as pinned"))?;
            continue;
        }
        let parts: Vec<String> = [("installed", &s.installed), ("updated", &s.updated), ("removed", &s.removed)]
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(what, v)| format!("{what} {}", v.join(", ")))
            .collect();
        o.kv(m.name(), Line::new().text(parts.join("; ")))?;
        removed |= !s.removed.is_empty();
    }
    // A removed pack's bound routes leave the index now; instinct packs bind again at the next
    // session (their choice of packs changed).
    if removed {
        let all = adage_tracks::io::read_all(&paths.tracks())?;
        let (index, _) = adage_tracks::Index::compile_root(&paths.tracks(), &all);
        index.write_atomic(&paths.index())?;
    }
    o.blank()?;
    o.next("adage packs", "which packs apply here, and turn any off")?;
    Ok(())
}
