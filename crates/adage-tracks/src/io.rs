//! Track directories on disk: `track.toml`, `routes/<id>.yaml`, validation, canonical digest.

use crate::model::*;
use adage_core::{shell::normalize_shell, CommandTable, Effect};
use anyhow::{bail, Context, Result};
use regex::Regex;
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
            // Hidden directories are never tracks in service.
            if e.file_type()?.is_dir() && !e.file_name().to_string_lossy().starts_with('.') {
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

pub fn safe_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

pub fn safe_name(n: &str) -> bool {
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
pub fn safe_command(c: &str) -> bool {
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

/// Whether a run step declared observe over the table's `environment` is a repository script
/// learning classified from the traces (`adage_learn::effects`): the command runs an unlisted
/// repository script whose name says nothing of changing anything, and the route says why.
pub fn inferred_observe(r: &Route, command: &str, actual: Effect, table: &CommandTable) -> bool {
    r.effect == Effect::Observe
        && actual == Effect::Environment
        && r.evidence.effect_evidence.as_deref().is_some_and(|n| !n.trim().is_empty())
        && adage_core::shell::unlisted_repo_script(command, table).is_some()
}

/// A contract must reject values that could carry shell syntax, whitespace, traversal, or flags.
pub fn contract_is_strict(regex: &str) -> bool {
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
            Some(max) if r.strength == Strength::Fire && max != Strength::Fire => {
                bail!("{}: strength fire exceeds the ceiling for {:?}", ctx(), r.effect)
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
                // Never trust a declared effect: classify the command ourselves. The one exception
                // is a repository script the table does not know, which learning classified
                // observe from the traces and says why (`evidence.effect_evidence`).
                let actual = classify(command, &table);
                if rank(actual) > rank(r.effect) && !inferred_observe(r, command, actual, &table) {
                    bail!("{}: command {:?} is {:?}, but the track declares {:?}", ctx(), command, actual, r.effect);
                }
                if r.strength == Strength::Inhibit {
                    bail!("{}: a run step cannot be inhibit", ctx());
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
                if r.strength != Strength::Inhibit {
                    bail!("{}: an avoid step must be inhibit", ctx());
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
            Step::Sequence { commands } => {
                if !(2..=SEQUENCE_MAX_COMMANDS).contains(&commands.len()) {
                    bail!("{}: a sequence has 2 to {SEQUENCE_MAX_COMMANDS} commands", ctx());
                }
                let by_edit = matches!(r.trigger, Trigger::AfterEdit { .. });
                for command in commands {
                    if !safe_command(command) {
                        bail!("{}: command {:?} contains characters a directed command may not", ctx(), command);
                    }
                    // Nothing binds a sequence's slots (its commands are fixed).
                    if !slot_refs(command).is_empty() {
                        bail!("{}: a sequence step has slots nothing can bind", ctx());
                    }
                    let actual = classify(command, &table);
                    if rank(actual) > rank(r.effect) {
                        bail!(
                            "{}: command {:?} is {:?}, but the track declares {:?}",
                            ctx(),
                            command,
                            actual,
                            r.effect
                        );
                    }
                    // The strength is at most the lowest ceiling of its commands.
                    match ceiling(actual, by_edit) {
                        None => bail!("{}: command {:?} may never be directed", ctx(), command),
                        Some(max) if r.strength == Strength::Fire && max != Strength::Fire => {
                            bail!("{}: strength fire exceeds the ceiling of {:?}", ctx(), command)
                        }
                        _ => {}
                    }
                }
                if r.strength == Strength::Inhibit {
                    bail!("{}: a sequence cannot be inhibit", ctx());
                }
                if !matches!(
                    r.trigger,
                    Trigger::AfterEdit { .. }
                        | Trigger::AfterCommand { .. }
                        | Trigger::AfterFailure { .. }
                        | Trigger::Context { .. }
                ) {
                    bail!("{}: a sequence follows an edit, a command, a failure or a context", ctx());
                }
            }
            Step::Card { card } => {
                crate::card::check(card).map_err(|e| anyhow::anyhow!("{}: {e}", ctx()))?;
                // A card suggests; it directs nothing on its own: prime, observe, no slots, and
                // the prompt of a task of its family.
                if r.strength != Strength::Prime || r.effect != Effect::Observe || !r.slots.is_empty() {
                    bail!("{}: a card is prime, observe, with no slots", ctx());
                }
                if !matches!(r.trigger, Trigger::Intent { .. }) {
                    bail!("{}: a card follows the prompt of a task of its family (an intent)", ctx());
                }
            }
            Step::Advise { template, slots } => {
                crate::advice::check(template, slots, &table).map_err(|e| anyhow::anyhow!("{}: {e}", ctx()))?;
                // Advice directs nothing: prime, observe, no route slots, and a reactive trigger.
                if r.strength != Strength::Prime || r.effect != Effect::Observe || !r.slots.is_empty() {
                    bail!("{}: an advisory step is prime, observe, with no route slots", ctx());
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
        // An instinct is prime at most and never a Stop request (Design; docs/instincts.md).
        if r.is_instinct() && r.strength != Strength::Prime {
            bail!("{}: an instinct route is prime", ctx());
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
                if !matches!(r.step, Step::Run { .. } | Step::Card { .. }) || !r.slots.is_empty() {
                    bail!("{}: an intent route must be a run step or a card, with no slots", ctx());
                }
            }
            Trigger::Context { items, within } => {
                validate_context(items, *within).map_err(|e| anyhow::anyhow!("{}: {e}", ctx()))?;
                // A context binds no slot (its items are matched symbolically, and no value of
                // theirs is kept), so its step directs fixed commands only.
                if !r.slots.is_empty() || !matches!(r.step, Step::Run { .. } | Step::Sequence { .. }) {
                    bail!("{}: a context route is a run or sequence step with no slots", ctx());
                }
            }
            _ => {}
        }
        // `dormant_since` is shown in `adage review` and `adage status`: a plain date or nothing.
        // (Any state may carry `exposed`; a dormant route is delivered at prime at most whatever
        // its stored strength, so a strengthen decision on one is not refused here.)
        if let Some(d) = &r.evidence.dormant_since {
            let date = d.len() == 10
                && d.bytes().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == b'-' } else { c.is_ascii_digit() });
            if !date {
                bail!("{}: dormant_since {:?} is not a YYYY-MM-DD date", ctx(), d);
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
            | (Trigger::AfterEdit { .. }, SlotSource::EditSegment { .. } | SlotSource::EditTests { .. })
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

/// At most this many test files an `edit_tests` slot binds.
pub const MAX_EDIT_TESTS: u8 = 5;
/// At most this many edited files an `edit_tests` map lists.
pub const MAX_TEST_MAP: usize = 4096;

/// Checks on a slot's source beyond its contract: a learned value type agrees with its contract,
/// an edit segment names a safe glob, and an edit-tests map lists plain repository paths.
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
        SlotSource::EditTests { glob, k, map } => {
            if !safe_glob(glob) || !(1..=MAX_EDIT_TESTS).contains(k) || map.len() > MAX_TEST_MAP {
                bail!("edit tests {glob:?} (k {k}, {} files) are not allowed", map.len());
            }
            if s.contract != Contract::RepoPath {
                bail!("edit tests need a repository-path contract");
            }
            let path_ok = |p: &str| {
                p.len() <= 200
                    && adage_core::repo::is_safe_relative(p)
                    && p.bytes().all(|c| c.is_ascii_alphanumeric() || b"_./-@+".contains(&c))
            };
            for (src, tests) in map {
                if !path_ok(src) || tests.is_empty() || tests.len() > *k as usize || !tests.iter().all(|t| path_ok(t)) {
                    bail!("edit tests map entry {src:?} → {tests:?} is not allowed");
                }
            }
            Ok(())
        }
        SlotSource::Prompt { .. } | SlotSource::ResultField { .. } => Ok(()),
    }
}

/// A context trigger (Track G §6.2): 2 to 3 items, a gap of 1 to 8 events, and every item safe:
/// command keys are short plain keys, globs are repository globs (never `**/*.ext`), signatures
/// are renderable text, and a value type or intent item passes the checks its own trigger or
/// slot would.
fn validate_context(items: &[CtxItem], within: u8) -> Result<()> {
    if !(2..=CONTEXT_MAX_ITEMS).contains(&items.len()) {
        bail!("a context has 2 to {CONTEXT_MAX_ITEMS} items");
    }
    if !(1..=CONTEXT_MAX_WITHIN).contains(&within) {
        bail!("a context's gap is 1 to {CONTEXT_MAX_WITHIN} events");
    }
    let key_ok = |k: &str| {
        k.len() <= 200 && k.starts_with("exec ") && !k.chars().any(|c| c.is_control() || "`<>\"\\$;|&".contains(c))
    };
    let keys_ok = |keys: &[String]| !keys.is_empty() && keys.len() <= 16 && keys.iter().all(|k| key_ok(k));
    for it in items {
        match it {
            CtxItem::Act { keys } => {
                if !keys_ok(keys) {
                    bail!("context command keys {keys:?} are not allowed");
                }
            }
            CtxItem::Fail { keys, signature } => {
                if !keys_ok(keys) || !safe_text(signature) {
                    bail!("context failure {keys:?} ({signature:?}) is not allowed");
                }
            }
            CtxItem::Edit { glob } => {
                if !safe_glob(glob) || glob.starts_with("**") {
                    bail!("context glob {glob:?} is not allowed");
                }
            }
            CtxItem::PromptValue { vtype } => validate_vtype(vtype, &Contract::Pattern { regex: vtype.regex.clone() })?,
            CtxItem::Intent { family, model, centroid, threshold } => {
                validate_intent(family, model, centroid, *threshold)?
            }
        }
    }
    Ok(())
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

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
