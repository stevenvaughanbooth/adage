//! Track directories on disk: `track.toml`, `routes/<id>.yaml`, validation, canonical digest.

use crate::model::*;
use adage_core::{shell::normalize_shell, CommandTable, Effect};
use anyhow::{bail, Context, Result};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn write(dir: &Path, track: &Track) -> Result<()> {
    std::fs::create_dir_all(dir.join("routes"))?;
    // Remove routes that no longer exist so the directory mirrors the track exactly.
    for e in std::fs::read_dir(dir.join("routes"))? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "yaml") {
            std::fs::remove_file(p)?;
        }
    }
    std::fs::write(dir.join("track.toml"), toml::to_string_pretty(&track.manifest)?)?;
    for r in &track.routes {
        if !safe_id(&r.id) {
            bail!("refusing to write route with unsafe id {:?}", r.id);
        }
        std::fs::write(dir.join("routes").join(format!("{}.yaml", r.id)), serde_yaml::to_string(r)?)?;
    }
    Ok(())
}

pub fn read(dir: &Path) -> Result<Track> {
    let manifest: Manifest = toml::from_str(
        &std::fs::read_to_string(dir.join("track.toml")).with_context(|| format!("{}/track.toml", dir.display()))?,
    )?;
    let mut routes = Vec::new();
    let rdir = dir.join("routes");
    if rdir.exists() {
        let mut files: Vec<_> = std::fs::read_dir(&rdir)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for p in files {
            if p.extension().is_some_and(|x| x == "yaml") {
                let r: Route =
                    serde_yaml::from_str(&std::fs::read_to_string(&p)?).with_context(|| format!("{}", p.display()))?;
                routes.push(r);
            }
        }
    }
    let t = Track { manifest, routes };
    validate(&t)?;
    Ok(t)
}

/// Read every track under `root` (one directory per track, possibly nested as `owner/name`).
pub fn read_all(root: &Path) -> Result<Vec<Track>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if d.join("track.toml").exists() {
            // One invalid track must not take down every other track.
            match read(&d) {
                Ok(t) => out.push(t),
                Err(e) => eprintln!("adage: skipping invalid track {}: {e:#}", d.display()),
            }
            continue;
        }
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            // Never follow symlinked directories (loops, and escapes from the tracks directory).
            if e.file_type()?.is_dir() {
                stack.push(e.path());
            }
        }
    }
    out.sort_by(|a, b| a.manifest.track.name.cmp(&b.manifest.track.name));
    Ok(out)
}

pub fn slot_refs(command: &str) -> Vec<String> {
    let re = Regex::new(r"\{([a-z_][a-z0-9_]*)\}").unwrap();
    re.captures_iter(command).map(|c| c[1].to_string()).collect()
}

fn safe_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn safe_name(n: &str) -> bool {
    n.len() <= 128
        && n.split('/').count() <= 2
        && n.split('/').all(|p| {
            !p.is_empty()
                && p.as_bytes()[0].is_ascii_alphanumeric()
                && p.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
        })
}

/// Directed commands are plain argv: no shell operators, substitutions, redirections, or quoting.
fn safe_command(c: &str) -> bool {
    !c.is_empty() && c.len() <= 200 && c.chars().all(|ch| ch.is_ascii_alphanumeric() || " _./:=@,+{}-".contains(ch))
}

/// Free text rendered to the agent (failure signatures): printable, no quotes or shell syntax.
fn safe_text(t: &str) -> bool {
    t.len() <= 160
        && !t.chars().any(|c| c.is_control() || "`<>\"\\$;|&{}".contains(c) || c == '\u{2028}' || c == '\u{2029}')
}

fn safe_glob(g: &str) -> bool {
    !g.is_empty()
        && g.len() <= 120
        && !g.starts_with('/')
        && !g.contains("..")
        && g.chars().all(|c| c.is_ascii_alphanumeric() || "_./*-".contains(c))
}

fn rank(e: Effect) -> u8 {
    match e {
        Effect::Observe => 0,
        Effect::Verify => 1,
        Effect::Regenerate => 2,
        Effect::Environment => 3,
        Effect::Author => 4,
        Effect::Publish => 5,
    }
}

/// The effect Adage itself assigns to a command template (slots filled with an inert value). The
/// strongest effect of any action in the command wins.
pub fn classify(command: &str, table: &CommandTable) -> Effect {
    let filled = regex::Regex::new(r"\{[a-z_][a-z0-9_]*\}").unwrap().replace_all(command, "X-1");
    normalize_shell(&filled, "/repo", table)
        .iter()
        .map(|n| n.effect)
        .max_by_key(|e| rank(*e))
        .unwrap_or(Effect::Environment)
}

/// A contract must reject values that could carry shell syntax, whitespace, traversal, or flags.
fn contract_is_strict(regex: &str) -> bool {
    let Ok(re) = Regex::new(regex) else { return false };
    let probes = ["a b", "a;b", "a|b", "$(x)", "`x`", "a&b", "a\nb", "../x", "-x", "a>b", "a<b", "a'b", "a\"b", ""];
    regex.starts_with('^') && regex.ends_with('$') && !probes.iter().any(|p| re.is_match(p))
}

/// Structural and safety validation. A track is untrusted input: every claim it makes about
/// itself is re-derived here, and it is refused at install, at learn, and at index time if any
/// check fails.
pub fn validate(t: &Track) -> Result<()> {
    let table = CommandTable::load();
    if !safe_name(&t.manifest.track.name) {
        bail!("track name {:?} is not allowed", t.manifest.track.name);
    }
    let v = &t.manifest.track.version;
    if v.split('.').count() != 3 || !v.split('.').all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit())) {
        bail!("track version {v:?} is not semver");
    }
    for r in &t.routes {
        if !safe_id(&r.id) {
            bail!("route id {:?} is not allowed", r.id);
        }
        let ctx = || format!("route {} in {}", r.id, t.manifest.track.name);
        let by_edit = matches!(r.trigger, Trigger::AfterEdit { .. });
        match ceiling(r.effect, by_edit) {
            None => bail!("{}: effect {:?} may never be directed", ctx(), r.effect),
            Some(max) if r.aspect == Aspect::Green && max != Aspect::Green => {
                bail!("{}: aspect green exceeds the ceiling for {:?}", ctx(), r.effect)
            }
            _ => {}
        }
        for s in &r.slots {
            if !safe_id(&s.name.replace('_', "-")) {
                bail!("{}: slot name {:?} is not allowed", ctx(), s.name);
            }
            if let Contract::Pattern { regex } = &s.contract {
                if !contract_is_strict(regex) {
                    bail!("{}: slot {} contract {:?} is not anchored and strict", ctx(), s.name, regex);
                }
            }
        }
        let declared = |n: &str| r.slots.iter().any(|s| s.name == n);
        match &r.step {
            Step::Run { command } => {
                if !safe_command(command) {
                    bail!("{}: command {:?} contains characters a directed command may not", ctx(), command);
                }
                for n in slot_refs(command) {
                    if !declared(&n) {
                        bail!("{}: command references undeclared slot {{{n}}}", ctx());
                    }
                }
                // Never trust a declared effect: classify the command ourselves.
                let actual = classify(command, &table);
                if rank(actual) > rank(r.effect) {
                    bail!("{}: command {:?} is {:?}, but the track declares {:?}", ctx(), command, actual, r.effect);
                }
                if r.aspect == Aspect::Red {
                    bail!("{}: a run step cannot be red", ctx());
                }
            }
            Step::ReadBatch { slot, max } => {
                let s = r.slots.iter().find(|s| &s.name == slot);
                match s {
                    Some(s) if s.contract == Contract::RepoPath => {}
                    _ => bail!("{}: read_batch slot must be declared with a repo_path contract", ctx()),
                }
                if *max == 0 || *max > 10 {
                    bail!("{}: read_batch max must be 1..=10", ctx());
                }
            }
            Step::Avoid { avoid, instead, signature } => {
                if r.aspect != Aspect::Red {
                    bail!("{}: an avoid step must be red", ctx());
                }
                if !safe_command(avoid) || !safe_command(instead) || !safe_text(signature) {
                    bail!("{}: avoid/instead/signature contain characters that may not be rendered", ctx());
                }
                if rank(classify(instead, &table)) > rank(Effect::Regenerate) {
                    bail!("{}: the suggested command {:?} is not a verify or regenerate step", ctx(), instead);
                }
            }
        }
        match &r.trigger {
            Trigger::AfterEdit { paths } if !safe_glob(paths) => bail!("{}: glob {:?} is not allowed", ctx(), paths),
            Trigger::AfterFailure { command, signature } if !safe_text(signature) || command.len() > 200 => {
                bail!("{}: failure trigger contains unsafe text", ctx())
            }
            Trigger::Intent { family, model, centroid, threshold } => {
                validate_intent(family, model, centroid, *threshold).map_err(|e| anyhow::anyhow!("{}: {e}", ctx()))?;
                // An intent route directs one fixed command: no slots, so nothing from the prompt
                // (or the track) is interpolated into what the agent reads.
                if !matches!(r.step, Step::Run { .. }) || !r.slots.is_empty() {
                    bail!("{}: an intent route must be a run step with no slots", ctx());
                }
            }
            _ => {}
        }
    }
    let intents = t.routes.iter().filter(|r| matches!(r.trigger, Trigger::Intent { .. })).count();
    if intents > MAX_INTENT_ROUTES {
        bail!("track {} has {intents} intent routes; at most {MAX_INTENT_ROUTES} are allowed", t.manifest.track.name);
    }
    Ok(())
}

/// `name@digest`: a model name and the first 12 hex digits of its table's SHA-256.
fn safe_model_id(m: &str) -> bool {
    let Some((name, digest)) = m.split_once('@') else { return false };
    !name.is_empty()
        && name.len() <= 48
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-".contains(&c))
        && digest.len() == 12
        && digest.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn validate_intent(family: &str, model: &str, centroid: &[i16], threshold: f32) -> Result<()> {
    if !safe_id(family) {
        bail!("intent family id {family:?} is not allowed");
    }
    if !safe_model_id(model) {
        bail!("intent model id {model:?} is not allowed");
    }
    if !(INTENT_MIN_DIM..=INTENT_MAX_DIM).contains(&centroid.len()) {
        bail!("intent centroid has {} dimensions; {INTENT_MIN_DIM}..={INTENT_MAX_DIM} are allowed", centroid.len());
    }
    if centroid.iter().any(|&x| !(-10_000..=10_000).contains(&x)) {
        bail!("intent centroid components must lie in -10000..=10000");
    }
    // A stored centroid is a unit vector: anything else would make the threshold meaningless.
    let norm2: f64 = centroid.iter().map(|&x| (x as f64 / 1e4).powi(2)).sum();
    if !(0.98..=1.02).contains(&norm2) {
        bail!("intent centroid is not a unit vector (|c|^2 = {norm2:.3})");
    }
    if !threshold.is_finite() || !(INTENT_MIN_THRESHOLD..=INTENT_MAX_THRESHOLD).contains(&threshold) {
        bail!("intent threshold {threshold} is outside {INTENT_MIN_THRESHOLD}..={INTENT_MAX_THRESHOLD}");
    }
    Ok(())
}

/// Canonical digest of a track directory (spike S6): every file, sorted by relative path, line
/// endings normalised, hashed; then the hash of that manifest. Signatures refer to this digest.
pub fn digest(dir: &Path) -> Result<String> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n != "track.sig") {
                files.push(p);
            }
        }
    }
    let mut entries: Vec<(String, String)> = files
        .iter()
        .map(|p| {
            let rel = p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
            let bytes = std::fs::read(p)?;
            let norm: Vec<u8> = if rel.starts_with("scripts/") {
                bytes
            } else {
                String::from_utf8_lossy(&bytes).replace("\r\n", "\n").into_bytes()
            };
            Ok((rel, hex(&Sha256::digest(&norm))))
        })
        .collect::<Result<_>>()?;
    entries.sort();
    let manifest: String = entries.iter().map(|(p, h)| format!("{h}  {p}\n")).collect();
    Ok(hex(&Sha256::digest(manifest.as_bytes())))
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
