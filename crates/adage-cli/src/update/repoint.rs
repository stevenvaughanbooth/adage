//! Re-pointing registrations after the binaries moved or changed (the plan's `install::repoint`,
//! lifted from the loop `adage delegate on` runs over every harness and scope; it lives here so
//! `install.rs` keeps its own shape, and `delegate on` keeps its own loop until its owner moves it
//! over).
//!
//! A registration is this install's when the binary it runs is one of the install's old paths,
//! sits in the directory they sat in, or no longer exists; it is re-pointed to the binary of the
//! same name the update installed, through `install::plan` (the same merge `adage install
//! --write` makes, so the harness's events are this version's too). Registrations that run
//! another Adage (the Claude Code plugin's stable link, a second install) are left alone. Only a
//! file `adage install` writes is rewritten; any other is named, to edit by hand.
//!
//! Rewriting happens only at a terminal outside an agent session (or with
//! `ADAGE_ALLOW_NONINTERACTIVE=1`), each file's diff shown first and its previous text kept in
//! `<file>.adage-bak` (`install::apply_backed_up`); inside an agent session, or under a press, the
//! changes are only reported, each with the `adage install` command that makes it.

use super::channel::exe_name;
use crate::detect;
use crate::install::{self, Plan};
use adage_term::{Line, Out, Role};
use anyhow::Result;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// One registration to rewrite.
pub(crate) struct Change {
    pub(crate) harness: &'static str,
    pub(crate) scope: &'static str,
    pub(crate) plan: Plan,
    /// The binary it ran, and the one it will.
    pub(crate) was: PathBuf,
    pub(crate) bin: PathBuf,
}

impl Change {
    /// The command that makes this change by hand.
    pub(crate) fn command(&self) -> String {
        format!(
            "adage install {} --write --scope {} --bin {}",
            self.harness,
            self.scope,
            detect::command_for(&self.bin)
        )
    }
}

/// What re-pointing found.
#[derive(Default)]
pub(crate) struct Found {
    pub(crate) changes: Vec<Change>,
    /// Registrations of this install that cannot be rewritten for the person: (harness, file, why).
    pub(crate) by_hand: Vec<(&'static str, PathBuf, String)>,
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(x, y)| x == y)
}

/// `adage` or `adage-exec`, from a binary's file name.
fn binary_name(p: &Path) -> Option<&'static str> {
    let n = p.file_name()?.to_string_lossy().to_string();
    let n = n.strip_suffix(".exe").unwrap_or(&n);
    ["adage", "adage-exec"].into_iter().find(|b| *b == n)
}

/// The registrations (in user scope, and in the repository `root` if given) of the install whose
/// binaries were `old` (name → path) that do not run `new_adage` (or the companion beside it) with
/// this version's events.
pub(crate) fn plan(old: &BTreeMap<String, PathBuf>, new_adage: &Path, root: Option<&Path>) -> Found {
    let new_dir = new_adage.parent().unwrap_or(new_adage);
    let old_dirs: Vec<&Path> = old.values().filter_map(|p| p.parent()).collect();
    let mut found = Found::default();
    let mut seen: Vec<PathBuf> = Vec::new();
    for r in detect::registrations(root) {
        if !matches!(r.kind, "settings" | "plugin") || seen.contains(&r.location) {
            continue;
        }
        let Some(b) = r.binary.as_deref() else { continue };
        let Some(name) = binary_name(b) else { continue };
        let ours = old.values().any(|o| same_file(o, b))
            || b.parent().is_some_and(|p| old_dirs.iter().any(|d| same_file(d, p)))
            || !b.exists();
        if !ours {
            continue;
        }
        seen.push(r.location.clone());
        let bin = if name == "adage" { new_adage.to_path_buf() } else { new_dir.join(exe_name(name)) };
        if !bin.is_file() {
            found.by_hand.push((
                r.harness,
                r.location.clone(),
                format!("it runs {name}, which this install no longer has"),
            ));
            continue;
        }
        let Some(scope) = install::scope_of(r.harness, &r.location, root) else {
            found.by_hand.push((r.harness, r.location.clone(), format!("edit it to run {}", detect::tilde(&bin))));
            continue;
        };
        match install::plan(r.harness, scope, root, Some(bin.clone())) {
            Ok(p) if p.before.as_deref() != Some(p.after.as_str()) => {
                found.changes.push(Change { harness: r.harness, scope, plan: p, was: b.to_path_buf(), bin })
            }
            Ok(_) => {}
            Err(e) => found.by_hand.push((r.harness, r.location.clone(), format!("{e:#}"))),
        }
    }
    found
}

/// How the changes are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Write them (at a terminal: after one question, unless `--yes`).
    Write { ask: bool },
    /// Say what would change and how to change it, write nothing (an agent session, a press,
    /// `--print`).
    Report,
}

/// Show and make the changes; returns how many files were rewritten. A file that cannot be
/// rewritten is a warning naming the command that rewrites it, never a silent skip.
pub(crate) fn apply<W: Write>(o: &mut Out<W>, found: &Found, mode: Mode) -> Result<usize> {
    for (h, at, why) in &found.by_hand {
        o.warn(&format!("{h}: {} still runs the old adage: {why}", detect::tilde(at)))?;
    }
    if found.changes.is_empty() {
        return Ok(0);
    }
    for c in &found.changes {
        o.kv(
            "hooks",
            Line::new().text(format!("{} {}", c.harness, detect::tilde(&c.plan.target))).quiet(format!(
                " runs {}, not {}",
                detect::tilde(&c.bin),
                detect::tilde(&c.was)
            )),
        )?;
        if mode != Mode::Report {
            for l in install::line_diff(c.plan.before.as_deref().unwrap_or(""), &c.plan.after).lines() {
                let role = if l.starts_with('+') { Role::Fire } else { Role::Inhibit };
                o.line(&Line::new().pad_to(adage_term::out::KEY_WIDTH + 2).push(role, l.to_string()))?;
            }
        }
    }
    match mode {
        Mode::Report => {
            for c in &found.changes {
                o.kv("", Line::new().quiet("in a terminal: ").cmd(c.command()))?;
            }
            Ok(0)
        }
        Mode::Write { ask } => {
            if ask && !confirm(o, "Re-point them? [y/N] ")? {
                o.note("nothing re-pointed; each command above does it")?;
                for c in &found.changes {
                    o.also(&c.command(), "")?;
                }
                return Ok(0);
            }
            let mut n = 0;
            for c in &found.changes {
                match install::apply_backed_up(&c.plan) {
                    Ok(backup) => {
                        n += 1;
                        let kept = backup.map(|b| format!(" · previous in {}", detect::tilde(&b))).unwrap_or_default();
                        o.kv("", Line::new().quiet(format!("rewrote {}{kept}", detect::tilde(&c.plan.target))))?;
                    }
                    Err(e) => o.warn(&format!(
                        "could not rewrite {} ({e:#}); run `{}`",
                        detect::tilde(&c.plan.target),
                        c.command()
                    ))?,
                }
            }
            Ok(n)
        }
    }
}

/// Ask at the terminal; yes only for `y` or `yes`.
fn confirm<W: Write>(o: &mut Out<W>, question: &str) -> Result<bool> {
    o.flush()?;
    print!("{question}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}
