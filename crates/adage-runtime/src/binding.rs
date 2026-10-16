//! Slot binding: a route's slots filled from the prompt, the triggering command's output, or the
//! edited path, each value checked against the slot's contract before anything is directed. A
//! value that does not validate drops the route; nothing is ever bound unchecked.

use crate::{command_key, extract_json, render, Ctx};
use adage_core::keys;
use adage_core::repo;
use adage_core::spans::{self, SpanKind};
use adage_tracks::{Contract, Route, SlotSource, Step, Trigger};
use regex::Regex;
use serde_json::Value;

/// Belt and braces: whatever a contract says, a bound value is a plain token.
fn plain_token(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 200
        && !v.starts_with('-')
        && v.chars().all(|c| c.is_ascii_alphanumeric() || "_./:@=+,-".contains(c))
}

fn contract_ok(c: &Contract, v: &str, ctx: &Ctx) -> bool {
    contract_ok_typed(c, None, v, ctx)
}

/// [`contract_ok`], where a slot with a learned value type is checked by the O(len) matcher
/// instead of a compiled regex: only when the type's runs write exactly the contract's regex
/// (`io::validate` refuses a track where they do not), so the two are the same language and no
/// regex is compiled in the hook (Track G §4.3, §7.4).
fn contract_ok_typed(c: &Contract, t: Option<&adage_tracks::ValueType>, v: &str, ctx: &Ctx) -> bool {
    if !plain_token(v) {
        return false;
    }
    match c {
        Contract::Pattern { regex } => match t {
            Some(t) if t.regex == *regex && adage_core::vtype::to_regex(&t.runs) == *regex => {
                adage_core::vtype::matches(&t.runs, v)
            }
            _ => Regex::new(regex).map(|r| r.is_match(v)).unwrap_or(false),
        },
        Contract::RepoPath => {
            // Symlinks resolved: the real file must still be inside the repository.
            repo::is_safe_relative(v) && ctx.checkout.is_file(ctx.root, v)
        }
    }
}

/// Bind a prompt route's slots from the prompt: a span slot from its typed spans, a learned
/// value type's slot from the first prompt token the type accepts (`adage_core::tokens`, at most
/// `PROMPT_TOKENS`, sub-tokens included). Every slot must validate ([`contract_ok_typed`], whose
/// `plain_token` check is the final gate).
pub(crate) fn bind_prompt_route(
    r: &Route,
    spans: &[spans::Span],
    text: &str,
    ctx: &Ctx,
) -> Option<(String, String, String, String)> {
    if let Step::Skill { skill, .. } = &r.step {
        // A skill binds nothing; under a prompt trigger the prompt must still carry a span of
        // its kind. Judged on the agent using the skill.
        if matches!(&r.trigger, Trigger::Prompt { span } if !spans.iter().any(|x| x.kind == *span)) {
            return None;
        }
        let text = render::route_text(r, &[])?;
        return Some((text, adage_tracks::skill_key(skill), String::new(), skill.clone()));
    }
    let Step::Run { command } = &r.step else { return None };
    let mut bound: Vec<(String, String)> = Vec::new();
    let mut toks: Option<Vec<(adage_core::tokens::Token, adage_core::tokens::Mention)>> = None;
    for s in &r.slots {
        let v = match &s.source {
            SlotSource::Prompt { span } => {
                spans.iter().find(|x| x.kind == *span && contract_ok(&s.contract, &x.value, ctx))?.value.clone()
            }
            SlotSource::PromptValue { vtype, mentions } => {
                let toks = toks
                    .get_or_insert_with(|| adage_core::tokens::with_mentions(text, adage_core::tokens::PROMPT_TOKENS));
                toks.iter()
                    .find(|(t, m)| {
                        (mentions.is_empty() || mentions.contains(m))
                            && adage_core::vtype::matches(&vtype.runs, &t.text)
                            && contract_ok_typed(&s.contract, Some(vtype), &t.text, ctx)
                    })?
                    .0
                    .text
                    .clone()
            }
            _ => return None,
        };
        bound.push((s.name.clone(), v));
    }
    bound_run(r, command, &bound, ctx)
}

/// A bound command: the directive's text, the key a follow is judged on, its head, and the
/// command as rendered.
fn bound_run(
    r: &Route,
    command: &str,
    bound: &[(String, String)],
    ctx: &Ctx,
) -> Option<(String, String, String, String)> {
    let text = render::route_text(r, bound)?;
    let rendered = render::bind(command, bound);
    let argv: Vec<String> = rendered.split_whitespace().map(str::to_string).collect();
    Some((text, command_key(&rendered, ctx.table), keys::command_head(&argv), rendered))
}

/// Whether a route's slots are all bound from its triggering command's output (and it has one).
pub(crate) fn binds_output(r: &Route) -> bool {
    !r.slots.is_empty() && r.slots.iter().all(|s| matches!(s.source, SlotSource::OutputValue { .. }))
}

/// The candidate values of an output: the JSON leaves at a dotted `field` (through arrays), or
/// the tokens of the text (`adage_core::tokens`), bounded.
fn output_values(output: &str, field: Option<&str>) -> Vec<String> {
    let out = adage_core::tokens::bounded(output, 64 * 1024);
    let Some(field) = field else {
        return adage_core::tokens::tokens(out, 512).into_iter().map(|t| t.text).collect();
    };
    let Some(json) = extract_json(out) else { return Vec::new() };
    let mut cur = vec![&json];
    for part in field.split('.') {
        let (name, each) = match part.strip_suffix("[]") {
            Some(n) => (n, true),
            None => (part, false),
        };
        let mut next = Vec::new();
        for v in cur {
            let Some(x) = (if name.is_empty() { Some(v) } else { v.get(name) }) else { continue };
            match (each, x.as_array()) {
                (true, Some(a)) => next.extend(a.iter()),
                _ => next.push(x),
            }
        }
        cur = next;
    }
    cur.into_iter()
        .take(512)
        .filter_map(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .collect()
}

/// Bind a run step's slots from the triggering command's output; every slot must validate.
pub(crate) fn bind_output_route(r: &Route, output: &str, ctx: &Ctx) -> Option<(String, String, String, String)> {
    let Step::Run { command } = &r.step else { return None };
    let mut bound: Vec<(String, String)> = Vec::new();
    for s in &r.slots {
        let SlotSource::OutputValue { field, vtype } = &s.source else { return None };
        let v = output_values(output, field.as_deref()).into_iter().find(|v| {
            adage_core::vtype::matches(&vtype.runs, v) && contract_ok_typed(&s.contract, Some(vtype), v, ctx)
        })?;
        bound.push((s.name.clone(), v));
    }
    bound_run(r, command, &bound, ctx)
}

/// Bind a run step's slots from the edited path: segment `depth` of `rel`, which must match the
/// slot's glob and validate. `Some(empty)` for a route with no slots.
pub(crate) fn bind_edit_slots(r: &Route, rel: &str, ctx: &Ctx) -> Option<Vec<(String, String)>> {
    let mut bound = Vec::new();
    for s in &r.slots {
        match &s.source {
            SlotSource::EditSegment { glob, depth } => {
                if !glob::Pattern::new(glob).is_ok_and(|g| g.matches(rel)) {
                    return None;
                }
                let seg = rel.split('/').filter(|x| !x.is_empty()).nth(*depth as usize)?;
                if !contract_ok(&s.contract, seg, ctx) {
                    return None;
                }
                bound.push((s.name.clone(), seg.to_string()));
            }
            // The edited file's tests from the learned map: each must exist in the repository.
            SlotSource::EditTests { glob, k, map } => {
                if !glob::Pattern::new(glob).is_ok_and(|g| g.matches(rel)) {
                    return None;
                }
                let tests: Vec<&str> = map
                    .get(rel)?
                    .iter()
                    .filter(|t| contract_ok(&s.contract, t, ctx))
                    .take(*k as usize)
                    .map(String::as_str)
                    .collect();
                if tests.is_empty() {
                    return None;
                }
                bound.push((s.name.clone(), tests.join(" ")));
            }
            _ => return None,
        }
    }
    Some(bound)
}

/// Bind a route's result-field slot from the triggering command's JSON output: the directive's
/// text and the files bound.
pub(crate) fn bind_result_route(r: &Route, output: &str, ctx: &Ctx) -> Option<(String, Vec<String>)> {
    let Step::ReadBatch { slot, max } = &r.step else { return None };
    let s = r.slots.iter().find(|s| &s.name == slot)?;
    let SlotSource::ResultField { field } = &s.source else { return None };
    let candidates: Vec<String> =
        match extract_json(output).and_then(|j| j.get(field).and_then(|v| v.as_array()).cloned()) {
            Some(list) => list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            // Plain-text output, or the "*paths" pseudo-field: repository paths it mentions.
            None => spans::extract(output).into_iter().filter(|x| x.kind == SpanKind::Path).map(|x| x.value).collect(),
        };
    let files: Vec<String> = candidates.into_iter().filter(|p| contract_ok(&s.contract, p, ctx)).take(*max).collect();
    if files.is_empty() {
        return None;
    }
    let text = render::route_text(r, &[(slot.clone(), files.join(", "))])?;
    Some((text, files))
}
