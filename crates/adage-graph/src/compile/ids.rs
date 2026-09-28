//! Stable, content-derived route ids (Track G §6.3): `<shape>-<stable(canonical context ‖
//! canonical step)>`.
//!
//! Canonical content is what the route says, never the evidence behind it: command keys and
//! globs of the trigger (an intent item by its family, never its centroid), value-type ids, the
//! directed commands, and slot *names*. Epoch label ids, prototype names, counts and dates never
//! enter, so re-clustering or more traces never rename a route. Some shapes have a fixed id form
//! instead ([`super::stable`]).

use adage_tracks::{CtxItem, Slot, SlotSource, Step, Trigger};

fn trigger_parts(t: &Trigger) -> Vec<String> {
    match t {
        Trigger::Prompt { span } => vec!["prompt".into(), format!("{span:?}")],
        Trigger::AfterCommand { command } => vec!["after_command".into(), command.clone()],
        Trigger::AfterFailure { command, signature } => {
            vec!["after_failure".into(), command.clone(), signature.clone()]
        }
        Trigger::AfterEdit { paths } => vec!["after_edit".into(), paths.clone()],
        Trigger::SessionStart => vec!["session_start".into()],
        Trigger::Intent { family, .. } => vec!["intent".into(), family.clone()],
        Trigger::Context { items, within } => {
            let mut v = vec!["context".into(), within.to_string()];
            for it in items {
                v.push(match it {
                    CtxItem::Act { keys } => format!("act:{}", keys.join("|")),
                    CtxItem::Fail { keys, signature } => format!("fail:{}:{signature}", keys.join("|")),
                    CtxItem::Edit { glob } => format!("edit:{glob}"),
                    CtxItem::PromptValue { vtype } => format!("prompt_value:{}", vtype.id),
                    CtxItem::Intent { family, .. } => format!("intent:{family}"),
                });
            }
            v
        }
    }
}

fn step_parts(s: &Step, slots: &[Slot]) -> Vec<String> {
    let mut v = match s {
        Step::Run { command } => vec!["run".into(), command.clone()],
        Step::ReadBatch { slot, max } => vec!["read_batch".into(), slot.clone(), max.to_string()],
        Step::Avoid { avoid, instead, .. } => vec!["avoid".into(), avoid.clone(), instead.clone()],
        Step::Advise { template, .. } => vec!["advise".into(), template.clone()],
        Step::Sequence { commands } => {
            let mut v = vec!["sequence".to_string()];
            v.extend(commands.iter().cloned());
            v
        }
    };
    for sl in slots {
        v.push(format!("slot:{}", sl.name));
        if let Some(t) = sl.source.vtype() {
            v.push(t.id.clone());
        }
        if let SlotSource::EditSegment { glob, depth } = &sl.source {
            v.push(format!("{glob}@{depth}"));
        }
    }
    v
}

/// The id of a route of `shape` with this trigger, step and slots.
pub fn route_id(shape: &str, trigger: &Trigger, step: &Step, slots: &[Slot]) -> String {
    let mut parts = trigger_parts(trigger);
    parts.push(String::from("\u{1f}"));
    parts.extend(step_parts(step, slots));
    let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
    format!("{shape}-{}", super::stable::stable(&refs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_depend_on_content_only() {
        let t = Trigger::AfterEdit { paths: "registry/*.csv".into() };
        let s = Step::Sequence { commands: vec!["make gen".into(), "make check".into()] };
        let a = route_id("sequence", &t, &s, &[]);
        assert!(a.starts_with("sequence-") && a.len() == "sequence-".len() + 6, "{a}");
        assert_eq!(a, route_id("sequence", &t, &s, &[]));
        let s2 = Step::Sequence { commands: vec!["make check".into(), "make gen".into()] };
        assert_ne!(a, route_id("sequence", &t, &s2, &[]), "order matters");
    }
}
