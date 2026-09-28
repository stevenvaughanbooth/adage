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

/// Directed commands are plain argv: no shell operators, substitutions, redirections, or quoting,
/// and no path outside the repository (absolute, or with a `..` segment), in an argument or in a
/// flag's value.
fn safe_command(c: &str) -> bool {
    let inside = |part: &str| !part.starts_with('/') && !part.split('/').any(|seg| seg == "..");
    // Words separated by single spaces: a blank or padded command renders as nothing.
    !c.is_empty()
        && c.len() <= 200
        && c.split(' ').all(|t| !t.is_empty())
        && c.chars().all(|ch| ch.is_ascii_alphanumeric() || " _./:=@,+{}-".contains(ch))
        && c.split(' ').all(|t| t.split(['=', ',']).all(inside))
}

/// Free text rendered to the agent (failure signatures): printable, no quotes or shell syntax,
/// and nothing invisible or direction-changing (text must read the way it is).
pub fn safe_text(t: &str) -> bool {
    t.len() <= 160
        && !t.chars().any(|c| {
            c.is_control() || "`<>\"\\$;|&{}".contains(c) || c == '\u{2028}' || c == '\u{2029}' || invisible(c)
        })
}

/// Zero-width, bidirectional-control and other invisible format characters.
pub fn invisible(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
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
            validate_source(s).map_err(|e| anyhow::anyhow!("{}: slot {}: {e}", ctx(), s.name))?;
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
                // Every slot a run step names must be bound by its trigger's event (Track G §4.3):
                // a prompt binds prompt spans and prompt values, a command its output values, an
                // edit its path's segments. Anything else would render the placeholder itself
                // (`make {item}`) as the directed command.
                for n in slot_refs(command) {
                    let Some(s) = r.slots.iter().find(|s| s.name == n) else { continue };
                    if !trigger_binds(&r.trigger, &s.source) {
                        bail!("{}: command {:?} has slots its trigger cannot bind", ctx(), command);
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
                // `avoid` names one or more failing runners, joined with " / ".
                if !avoid.split(" / ").all(safe_command) || !safe_command(instead) || !safe_text(signature) {
                    bail!("{}: avoid/instead/signature contain characters that may not be rendered", ctx());
                }
                // Nothing binds an avoid step's slots.
                if !slot_refs(avoid).is_empty() || !slot_refs(instead).is_empty() {
                    bail!("{}: an avoid step has slots nothing can bind", ctx());
                }
                if rank(classify(instead, &table)) > rank(Effect::Regenerate) {
                    bail!("{}: the suggested command {:?} is not a verify or regenerate step", ctx(), instead);
                }
            }
            Step::Advise { template, slots } => {
                crate::advice::check(template, slots, &table).map_err(|e| anyhow::anyhow!("{}: {e}", ctx()))?;
                // Advice directs nothing: yellow, observe, no route slots, and a reactive trigger.
                if r.aspect != Aspect::Yellow || r.effect != Effect::Observe || !r.slots.is_empty() {
                    bail!("{}: an advisory step is yellow, observe, with no route slots", ctx());
                }
                if !matches!(
                    r.trigger,
                    Trigger::AfterEdit { .. } | Trigger::AfterCommand { .. } | Trigger::AfterFailure { .. }
                ) {
                    bail!("{}: an advisory step follows an edit, a command or a failure", ctx());
                }
            }
        }
        if let Some(o) = &r.evidence.origin {
            validate_origin(o).map_err(|e| anyhow::anyhow!("{}: {e}", ctx()))?;
        }
        // An instinct is yellow at most and never a Stop request (Design; docs/instincts.md).
        if r.is_instinct() && r.aspect != Aspect::Yellow {
            bail!("{}: an instinct route is yellow", ctx());
        }
        for k in r.evidence.by_origin.keys() {
            if !safe_id(k.replace('_', "-").as_str()) || k.len() > 32 {
                bail!("{}: evidence origin {:?} is not allowed", ctx(), k);
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
        // `stale_since` is shown in `adage review` and `adage status`: a plain date or nothing.
        // (Any state may carry `exposed`; a stale route is delivered at yellow at most whatever
        // its stored aspect, so a promote decision on one is not refused here.)
        if let Some(d) = &r.evidence.stale_since {
            let date = d.len() == 10
                && d.bytes().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == b'-' } else { c.is_ascii_digit() });
            if !date {
                bail!("{}: stale_since {:?} is not a YYYY-MM-DD date", ctx(), d);
            }
        }
    }
    let intents = t.routes.iter().filter(|r| matches!(r.trigger, Trigger::Intent { .. })).count();
    if intents > MAX_INTENT_ROUTES {
        bail!("track {} has {intents} intent routes; at most {MAX_INTENT_ROUTES} are allowed", t.manifest.track.name);
    }
    Ok(())
}

/// Whether a trigger's event supplies what a slot source binds from.
pub fn trigger_binds(t: &Trigger, s: &SlotSource) -> bool {
    matches!(
        (t, s),
        (Trigger::Prompt { .. }, SlotSource::Prompt { .. } | SlotSource::PromptValue { .. })
            | (Trigger::AfterCommand { .. }, SlotSource::OutputValue { .. })
            | (Trigger::AfterEdit { .. }, SlotSource::EditSegment { .. })
    )
}

/// Strings a learned type must accept: for each branch, its shortest and its longest member
/// (first literal of a set, first character of a class).
fn type_examples(runs: &adage_core::vtype::Runs) -> Vec<String> {
    use adage_core::vtype::Run;
    let fill = |class: adage_core::vtype::Class, n: u8| {
        let c = ['A', 'a', '0', '0', 'A'][class as usize];
        std::iter::repeat_n(c, n as usize).collect::<String>()
    };
    let mut out = Vec::new();
    for b in &runs.branches {
        for long in [false, true] {
            let mut s = String::new();
            for r in b {
                match r {
                    Run::Lit { text } => s.push_str(text),
                    Run::Set { texts } => s.push_str(texts.first().map(String::as_str).unwrap_or("")),
                    Run::Class { class, lo, hi } => s.push_str(&fill(*class, if long { *hi } else { *lo })),
                    Run::Star { sep, class, lo, hi, min, max } => {
                        for _ in 0..(if long { *max } else { *min }) {
                            s.push(*sep);
                            s.push_str(&fill(*class, if long { *hi } else { *lo }));
                        }
                    }
                }
            }
            out.push(s);
        }
    }
    out
}

/// A learned value type is only as good as the agreement between what the hook runs (the
/// matcher over `runs`) and what the contract says (the regex): they must be the same language.
fn validate_vtype(t: &ValueType, contract: &Contract) -> Result<()> {
    use adage_core::vtype;
    let hex = |s: &str| s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
    if !(t.id.len() == 19 && t.id.starts_with("vt-") && hex(&t.id[3..])) {
        bail!("value type id {:?} is not allowed", t.id);
    }
    if !(t.samples_hash.is_empty() || (t.samples_hash.len() == 64 && hex(&t.samples_hash))) {
        bail!("value type samples_hash is not a SHA-256");
    }
    let branches = t.runs.branches.len();
    if branches == 0 || branches > vtype::MAX_BRANCHES || t.runs.branches.iter().any(|b| b.is_empty() || b.len() > 64) {
        bail!("value type {} has {branches} branches; 1..={} are allowed", t.id, vtype::MAX_BRANCHES);
    }
    if vtype::max_len(&t.runs) > vtype::MAX_LEN {
        bail!("value type {} accepts values longer than {} bytes", t.id, vtype::MAX_LEN);
    }
    if vtype::to_regex(&t.runs) != t.regex {
        bail!("value type {}: runs and regex {:?} disagree", t.id, t.regex);
    }
    if *contract != (Contract::Pattern { regex: t.regex.clone() }) {
        bail!("value type {}: the slot's contract is not its regex", t.id);
    }
    let re = Regex::new(&t.regex).map_err(|e| anyhow::anyhow!("value type {}: {e}", t.id))?;
    for ex in type_examples(&t.runs) {
        if !vtype::matches(&t.runs, &ex) || !re.is_match(&ex) {
            bail!("value type {}: runs and regex disagree on {ex:?}", t.id);
        }
    }
    // The matcher must refuse what a strict contract refuses (the regex already does).
    let probes = ["a b", "a;b", "a|b", "$(x)", "`x`", "a&b", "a\nb", "-x", "a>b", "a<b", "a'b", "a\"b", ""];
    if let Some(p) = probes.iter().find(|p| vtype::matches(&t.runs, p)) {
        bail!("value type {} accepts {p:?}", t.id);
    }
    Ok(())
}

/// Checks on a slot's source beyond its contract: a learned value type agrees with its contract,
/// and an edit segment names a safe glob.
fn validate_source(s: &Slot) -> Result<()> {
    match &s.source {
        SlotSource::PromptValue { vtype, mentions } => {
            let affix =
                |a: &str| a.len() <= 16 && a.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_./#:@+=".contains(&c));
            if mentions.len() > 4 || !mentions.iter().all(|m| affix(&m.prefix) && affix(&m.suffix)) {
                bail!("prompt mentions {mentions:?} are not allowed");
            }
            validate_vtype(vtype, &s.contract)
        }
        SlotSource::OutputValue { field, vtype } => {
            if let Some(f) = field {
                let ok = !f.is_empty()
                    && f.len() <= 128
                    && f.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.-[]".contains(&c));
                if !ok {
                    bail!("output field {f:?} is not allowed");
                }
            }
            validate_vtype(vtype, &s.contract)
        }
        SlotSource::EditSegment { glob, depth } => {
            if !safe_glob(glob) || *depth > 32 {
                bail!("edit segment {glob:?} at depth {depth} is not allowed");
            }
            if !matches!(s.contract, Contract::Pattern { .. }) {
                bail!("an edit segment needs a pattern contract");
            }
            Ok(())
        }
        SlotSource::Prompt { .. } | SlotSource::ResultField { .. } => Ok(()),
    }
}

/// Where a route came from: every field is rendered into the directive's wrapper, so each is a
/// short safe token from a closed set or a repository-relative path.
fn validate_origin(o: &Origin) -> Result<()> {
    if let Origin::Instinct { pack, version, instinct, ecosystem, declared } = o {
        let token = |s: &str| s.len() <= 32 && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c));
        if !token(pack) || !token(version) || !token(instinct) {
            bail!("instinct origin {pack:?}@{version:?} ({instinct:?}) is not allowed");
        }
        if let Some(e) = ecosystem {
            if !ECOSYSTEMS.contains(&e.as_str()) {
                bail!("ecosystem {e:?} is not one of {ECOSYSTEMS:?}");
            }
        }
        if let Some(d) = declared {
            if !crate::advice::safe_glob(d) || d.contains('*') {
                bail!("declared file {d:?} is not a repository-relative path");
            }
        }
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
    // The canonical digest (spike S6 FINDINGS, "Canonical-digest spec"): walk without following
    // symlinks; skip `track.sig` and any path component starting with `.`; hash `scripts/`
    // verbatim and everything else with `\r\n` and lone `\r` normalised to `\n` — on the bytes,
    // never through a lossy UTF-8 decode (which would map different invalid bytes to the same
    // text, and so to the same digest).
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            let p = e.path();
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let t = e.file_type()?;
            if t.is_symlink() {
                bail!("{} is a symlink; a track may not contain symlinks", p.display());
            } else if t.is_dir() {
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
            let norm = if rel.starts_with("scripts/") { bytes } else { normalise_line_endings(&bytes) };
            Ok((rel, hex(&Sha256::digest(&norm))))
        })
        .collect::<Result<_>>()?;
    entries.sort();
    let manifest: String = entries.iter().map(|(p, h)| format!("{h}  {p}\n")).collect();
    Ok(hex(&Sha256::digest(manifest.as_bytes())))
}

/// `\r\n` and lone `\r` become `\n`; every other byte is kept as is.
fn normalise_line_endings(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\r' {
            out.push(b'\n');
            if b.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    out
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod digest_tests {
    use super::{digest, normalise_line_endings};

    #[test]
    fn line_endings_normalise_on_bytes() {
        assert_eq!(normalise_line_endings(b"a\r\nb\rc\n"), b"a\nb\nc\n");
        assert_eq!(normalise_line_endings(b"\r\r\n"), b"\n\n");
        // Invalid UTF-8 survives byte for byte: different bytes, different digests.
        assert_ne!(normalise_line_endings(&[0xff, 0x0a]), normalise_line_endings(&[0xfe, 0x0a]));
    }

    #[test]
    fn digest_follows_the_s6_spec() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("routes")).unwrap();
        std::fs::write(d.path().join("track.toml"), "a\r\nb\n").unwrap();
        std::fs::write(d.path().join("routes/r.yaml"), "x\ry\n").unwrap();
        let base = digest(d.path()).unwrap();
        // Hidden files and the signature do not count; line endings do not either.
        std::fs::write(d.path().join(".DS_Store"), "junk").unwrap();
        std::fs::write(d.path().join("track.sig"), "sig").unwrap();
        std::fs::write(d.path().join("track.toml"), "a\nb\n").unwrap();
        std::fs::write(d.path().join("routes/r.yaml"), "x\ny\n").unwrap();
        assert_eq!(digest(d.path()).unwrap(), base);
        // Different invalid bytes are different content.
        std::fs::write(d.path().join("routes/r.yaml"), [0xffu8, b'\n']).unwrap();
        let a = digest(d.path()).unwrap();
        std::fs::write(d.path().join("routes/r.yaml"), [0xfeu8, b'\n']).unwrap();
        assert_ne!(digest(d.path()).unwrap(), a);
        // Symlinks are refused, never followed.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", d.path().join("routes/link")).unwrap();
            assert!(digest(d.path()).is_err());
        }
    }
}
