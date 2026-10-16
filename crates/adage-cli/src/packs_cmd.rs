//! `adage packs`: the packs Adage binds, as Claude Code's `/plugin` lists plugins
//! (`docs/marketplace.md`).
//!
//! Adage's base instincts are the packs of the marketplace built into it (the Adage repository's
//! `.adage/marketplace.toml` and `packs/`, `adage_pack::builtin`), pre-installed where `adage add`
//! installs a pack; packs added with `adage add` follow them. For the repository it runs in (or
//! `--repo-path <dir>`), each instinct pack says whether its detectors find what it needs there,
//! and how many reflexes it bound.
//!
//! - `adage packs on <name>` / `adage packs off <name>`: bind an instinct pack wherever its
//!   detectors find what it needs, or stop (`instincts.packs`, `instincts.disabled`), then bind the
//!   repository's instincts again. Every other repository binds them again at its next session.
//!   Turning a pack on is a person's decision (never inside an agent session); turning one off is
//!   not.
//! - `adage packs show <name>`: a pack's files, exactly as installed: what a packaged reflex is.

use crate::autolearn::instinct_config;
use adage_pack::bind::{applies, Applies, Target};
use adage_pack::install::Via;
use adage_pack::instincts::Source;
use adage_pack::Pack;
use adage_store::Paths;
use adage_term::{Glyph, Line, Out, Role};
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: adage packs [on <name> | off <name> | show <name>] [--repo-path <dir>] [--json]";

pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    let at = match args.iter().position(|a| a == "--repo-path") {
        Some(i) => Some(PathBuf::from(args.get(i + 1).context(USAGE)?)),
        None => std::env::current_dir().ok(),
    };
    let root = at.and_then(|d| d.canonicalize().ok()).and_then(|d| adage_core::repo::find_root(&d));
    let name = || args.get(1).filter(|a| !a.starts_with("--")).map(String::as_str).context(USAGE);
    match args.first().map(String::as_str) {
        Some("on") => switch(name()?, true, root.as_deref(), paths),
        Some("off") => switch(name()?, false, root.as_deref(), paths),
        Some("show") => show(name()?, paths),
        _ => list(root.as_deref(), args.iter().any(|a| a == "--json"), paths),
    }
}

/// The live routes each pack bound in a repository's instincts track.
fn bound_by_pack(paths: &Paths, repo: &str) -> BTreeMap<String, usize> {
    let dir = paths.tracks().join(adage_learn::slug(repo)).join(adage_learn::instincts::TRACK);
    let mut out = BTreeMap::new();
    for r in adage_tracks::io::read(&dir).map(|t| t.routes).unwrap_or_default() {
        if let (true, adage_tracks::Origin::Instinct { pack, .. }) = (r.state.is_live(), r.origin()) {
            *out.entry(pack).or_insert(0) += 1;
        }
    }
    out
}

/// The first sentence of a pack's description.
fn gist(description: &str) -> &str {
    description.split_once(". ").map_or(description, |(first, _)| first).trim_end_matches('.')
}

struct Row {
    pack: Pack,
    builtin: bool,
    on: bool,
    /// Whether its detectors find what it needs in the repository (`None`: no repository here).
    applies: Option<bool>,
    bound: usize,
    /// For a built-in pack: whether the installed copy is the pinned one.
    installed: bool,
    /// How it was installed (`installed.json`), when it was.
    via: Option<Via>,
}

/// How a pack was installed, for people.
fn via_text(v: &Via) -> String {
    match v {
        Via::Marketplace { name } => format!("from the {name} marketplace"),
        Via::Signature { signer } => format!("signed by {signer}"),
        Via::Review => "compiled and reviewed here".into(),
    }
}

fn list(root: Option<&Path>, json: bool, paths: &Paths) -> Result<()> {
    let settings = adage_store::config::load(paths).config;
    let ic = instinct_config(&settings, paths);
    let (available, problems) = adage_pack::instincts::available(&paths.tracks());
    let repo = root.map(|r| adage_core::repo::repo_key_cached(r, &paths.repo_keys()));
    let bound = repo.as_deref().map(|k| bound_by_pack(paths, k)).unwrap_or_default();
    let builtin = adage_pack::builtin::marketplace().ok();
    let records = adage_pack::install::records(&paths.tracks());
    let rows: Vec<Row> = available
        .into_iter()
        .map(|(pack, source)| {
            let name = pack.manifest.pack.name.clone();
            let pin = builtin.and_then(|b| b.get(&name)).map(|(e, _)| e.digest.clone());
            let installed = match &pin {
                Some(d) => adage_pack::digest_dir(&crate::packs::pack_dir(paths, &name)).is_ok_and(|x| &x == d),
                None => true,
            };
            Row {
                applies: root.map(|r| applies(&pack, &Target::checkout(r)) == Applies::Yes),
                bound: bound.get(&name).copied().unwrap_or(0),
                on: ic.wants(&name),
                builtin: source == Source::Builtin,
                installed,
                via: records.get(&name).map(|r| r.via.clone()),
                pack,
            }
        })
        .collect();
    // Packs added with `adage add` that are not instinct packs: where they are bound.
    let mut added: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for (_, b) in crate::packs::bindings(paths) {
        let live = b.routes.iter().filter(|r| r.status.is_live()).count();
        let inert = b.routes.len() - live;
        let e = added.entry(b.pack.clone()).or_insert_with(|| (b.version.clone(), Vec::new()));
        e.1.push(format!("{} ({live} live, {inert} inert)", b.repo));
    }
    if json {
        let packs: Vec<_> = rows
            .iter()
            .map(|r| {
                let m = &r.pack.manifest.pack;
                json!({
                    "name": m.name, "version": m.version, "description": m.description,
                    "source": if r.builtin { "builtin" } else { "installed" },
                    "on": r.on, "applies": r.applies, "bound": r.bound, "installed": r.installed,
                    "via": r.via,
                })
            })
            .collect();
        let added: Vec<_> = added
            .iter()
            .map(|(name, (version, bound))| {
                json!({ "name": name, "version": version, "bound": bound, "via": records.get(name).map(|r| &r.via) })
            })
            .collect();
        let v = json!({
            "repo": repo,
            "marketplace": builtin.map(|b| json!({ "name": b.manifest.marketplace.name, "built_in": true, "packs": b.packs.len() })),
            "instincts": packs,
            "added": added,
            "problems": problems.iter().map(|(who, why)| format!("{who}: {why}")).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }

    let mut o = Out::stdout();
    let here = repo.clone().unwrap_or_else(|| "not in a repository".into());
    o.header("packs", &[&here])?;
    o.blank()?;
    let from = builtin.map_or("the adage marketplace".to_string(), |b| {
        format!("the {} marketplace, built into Adage {}", b.manifest.marketplace.name, env!("CARGO_PKG_VERSION"))
    });
    o.section(&format!("Base instincts · {from}"))?;
    let w = rows.iter().map(|r| r.pack.manifest.pack.name.chars().count()).max().unwrap_or(16).clamp(16, 32);
    for r in rows.iter().filter(|r| r.builtin) {
        row(&mut o, r, w)?;
    }
    let others: Vec<&Row> = rows.iter().filter(|r| !r.builtin).collect();
    if !others.is_empty() || !added.is_empty() {
        o.blank()?;
        o.section("Added with adage add")?;
        for r in others {
            row(&mut o, r, w)?;
        }
        for (name, (version, bound)) in &added {
            let mut l = Line::new()
                .text("  ")
                .glyph(Role::Accent, Glyph::Reflex)
                .text(format!(" {name} {version}"))
                .pad_to(11 + w)
                .text(format!("bound to {}", bound.join(", ")));
            if let Some(r) = records.get(name) {
                l = l.sep().faint(via_text(&r.via));
            }
            o.line(&l)?;
        }
    }
    for (who, why) in &problems {
        o.warn(&format!("{who}: {why}"))?;
    }
    o.blank()?;
    o.next("adage packs off <name>", "stop binding an instinct pack, here and everywhere")?;
    o.also("adage packs show <name>", "a pack's files: what a packaged reflex is")?;
    Ok(())
}

/// One instinct pack: on or off, what it finds here, and the first sentence of its description.
fn row<W: std::io::Write>(o: &mut Out<W>, r: &Row, w: usize) -> Result<()> {
    let name = &r.pack.manifest.pack.name;
    let (glyph, role) = if r.on { (Glyph::Instinct, Role::Accent) } else { (Glyph::Off, Role::Ink3) };
    // `  ◉ adage/python-env     on   bound here: 10 reflexes`
    let mut l = Line::new().text("  ").glyph(role, glyph).text(" ").text(name.clone()).pad_to(6 + w);
    l = if r.on { l.push(Role::Fire, "on") } else { l.faint("off") }.pad_to(11 + w);
    l = match (r.applies, r.bound) {
        (_, n) if n > 0 => l.text(format!("bound here: {}", crate::words::count(n, "reflex"))),
        (Some(true), _) if r.on => l.text("applies here"),
        (Some(true), _) => l.quiet("would apply here"),
        (Some(false), _) => l.faint("nothing to bind here"),
        (None, _) => l,
    };
    if !r.installed {
        l = l.sep().faint("not installed yet: `adage marketplace update`");
    }
    if let (false, Some(v)) = (r.builtin, &r.via) {
        l = l.sep().faint(via_text(v));
    }
    o.line(&l)?;
    o.line(&Line::new().pad_to(6).quiet(gist(&r.pack.manifest.pack.description).to_string()))?;
    Ok(())
}

fn switch(name: &str, on: bool, root: Option<&Path>, paths: &Paths) -> Result<()> {
    if on {
        crate::share::require_person("packs on")?;
    }
    let (available, _) = adage_pack::instincts::available(&paths.tracks());
    if !available.iter().any(|(p, _)| p.manifest.pack.name == name) {
        return Err(crate::hint(
            format!("{name} is not an instinct pack Adage has"),
            "`adage packs` lists them; `adage add <dir>` adds one",
        ));
    }
    let settings = adage_store::config::load(paths).config;
    let mut disabled = settings.instincts_disabled.clone();
    let mut packs = settings.instincts_packs.clone();
    disabled.retain(|d| d != name);
    if on {
        // AUTO omits optional built-ins. An explicit selection must remain explicit even when
        // the default packs are also selected, so `packs on` can enable an optional pack.
        if !packs.iter().any(|p| p == name) {
            packs.push(name.to_string());
            adage_store::config::set(paths, "instincts.packs", &packs.join(",")).map_err(anyhow::Error::msg)?;
        }
    } else {
        disabled.push(name.to_string());
    }
    let written =
        adage_store::config::set(paths, "instincts.disabled", &disabled.join(",")).map_err(anyhow::Error::msg)?;
    let mut o = Out::stdout();
    o.header("packs", &[if on { "on" } else { "off" }, name])?;
    o.blank()?;
    o.kv(
        "instincts",
        Line::new().text(match (on, settings.instincts_enabled) {
            (true, true) => format!("{name} binds wherever its detectors find what it needs"),
            (true, false) => format!("{name} is on, but instincts are off (`adage config set instincts.enabled true`)"),
            (false, _) => format!("{name} binds nowhere until `adage packs on {name}`"),
        }),
    )?;
    o.kv(
        "config",
        Line::new().quiet(format!("instincts.disabled = {}", if written.is_empty() { "(none)" } else { &written })),
    )?;
    o.kv("elsewhere", Line::new().quiet("every other repository binds its instincts again at its next session"))?;
    o.blank()?;
    o.flush()?;
    match root {
        Some(r) => crate::instincts_only(&["--repo-path".to_string(), r.to_string_lossy().into_owned()], paths),
        None => Ok(()),
    }
}

fn show(name: &str, paths: &Paths) -> Result<()> {
    let installed = crate::packs::pack_dir(paths, name);
    let files = match adage_pack::builtin::marketplace().ok().and_then(|b| b.files(name)) {
        Some(f) => f,
        None if installed.join(adage_pack::validate::MANIFEST).is_file() => {
            adage_pack::validate::read_dir_files(&installed)?
        }
        None => {
            return Err(crate::hint(
                format!("no pack named {name} is built in or installed"),
                "`adage packs` lists them",
            ))
        }
    };
    // The manifest, then the routes, then everything else, as a directory listing would show them.
    let order = |p: &str| match p {
        adage_pack::validate::MANIFEST => 0,
        p if p.starts_with("routes/") => 1,
        p if p.starts_with("types/") => 2,
        adage_pack::validate::CLOSURE => 4,
        _ => 3,
    };
    let mut names: Vec<&String> = files.keys().collect();
    names.sort_by_key(|p| (order(p), p.as_str()));
    let mut o = Out::stdout();
    o.header("packs show", &[name, &installed.display().to_string()])?;
    for p in names {
        o.blank()?;
        o.line(&Line::new().faint("── ").text(p.clone()))?;
        if p.starts_with("scripts/") {
            o.line(&Line::new().quiet(format!("({} bytes: a vendored script)", files[p].len())))?;
            continue;
        }
        for l in String::from_utf8_lossy(&files[p]).lines() {
            o.line(&Line::new().text(l.to_string()))?;
        }
    }
    Ok(())
}
