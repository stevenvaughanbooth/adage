//! One reflex as every surface shows it: `adage status --json`, `adage review --json` and the
//! map's `/api/v1/graph` all render a [`Reflex`], with the same field names and types, and add
//! only what is theirs (the queue's decisions, the map's live counts). Nothing outside this
//! module builds a reflex's JSON.

use crate::review::{ceiling_str, origin_label, preview, Installed};
use adage_runtime::span_kind_label;
use adage_tracks::why::Why;
use adage_tracks::{index, Route, Step, Trigger};
use serde::Serialize;
use serde_json::{json, Value};

/// A reflex: what it is, where it came from, what it does, and the evidence it was decided on.
#[derive(Debug, Clone, Serialize)]
pub struct Reflex {
    /// The route id, unique within its track.
    pub id: String,
    /// The track's name, and its directory under the tracks directory (`reference`), which the
    /// review verbs take.
    pub track: String,
    pub reference: String,
    pub version: String,
    pub repo: String,
    pub imported: bool,
    /// An instinct (bound from the repository's declared files), not learned from traces.
    pub instinct: bool,
    /// Where it came from: `kind` (`learned`, `imported`, `instinct`), a human `label` when there
    /// is one, and for an instinct the pack, instinct and declared file.
    pub origin: Value,
    pub summary: String,
    /// What it tells the agent, in one sentence (`Route::gist`): how people name it.
    pub gist: String,
    pub strength: String,
    /// The strongest it may become (`ceiling_str`).
    pub ceiling: &'static str,
    pub effect: String,
    /// Its lifecycle state: nascent, forming, firm, dormant or extinguished.
    pub state: String,
    pub authored: bool,
    /// Kept quiet on its uplift: agents already take the step unaided, so it waits for a person.
    pub quiet: bool,
    pub shape: String,
    /// `{kind, label, …}`: when it applies.
    pub trigger: Value,
    /// `{kind, label, …}`: what it directs.
    pub step: Value,
    /// Run only by Adage itself, under a local grant; never directed to the agent.
    pub execution_only: bool,
    /// The directive as the agent receives it; `null` for an execution-only reflex.
    pub directive: Option<String>,
    /// What its Stop check asks before the agent finishes, for a finish line.
    pub obligation: Option<String>,
    /// The evidence its strength was decided on, with `describe` (the sentence people read) and
    /// `basis` (instinct, value, family or count).
    pub evidence: Value,
}

impl Reflex {
    /// `r` of the installed track `inst`, with the explanation learning recorded for it.
    pub fn of(inst: &Installed, r: &Route, why: Option<&Why>) -> Reflex {
        let t = &inst.track;
        let e = &r.evidence;
        let mut step_json = step(&r.step);
        if let Step::Card { card } = &r.step {
            let threshold = match &r.trigger {
                Trigger::Intent { threshold, .. } => Some(*threshold),
                _ => None,
            };
            step_json["card"] = card_json(card, threshold);
        }
        Reflex {
            id: r.id.clone(),
            track: t.manifest.track.name.clone(),
            reference: inst.reference.clone(),
            version: t.manifest.track.version.clone(),
            repo: t.manifest.scope.repo.clone(),
            imported: inst.imported,
            instinct: r.is_instinct() || index::is_instinct_track(t),
            origin: origin(r, inst.imported),
            summary: r.summary.clone(),
            gist: r.gist(),
            strength: lower(r.strength),
            ceiling: ceiling_str(r),
            effect: lower(r.effect),
            state: lower(r.state),
            authored: r.authored,
            quiet: r.state == adage_tracks::RouteState::Nascent
                && !r.authored
                && e.uplift.as_ref().is_some_and(|u| u.gated),
            shape: adage_tracks::shape(r).to_string(),
            trigger: trigger(&r.trigger),
            step: step_json,
            execution_only: e.execution.is_some(),
            directive: e.execution.is_none().then(|| preview(t, r).0),
            obligation: crate::evidence::obligation(r),
            evidence: evidence(r, why),
        }
    }
}

/// Where a reflex came from, as an object: the recorded origin (an instinct's pack, instinct and
/// declared file) or `{kind: learned | imported}`, with its human `label` when it has one.
fn origin(r: &Route, imported: bool) -> Value {
    let mut o = r
        .evidence
        .origin
        .as_ref()
        .and_then(|o| serde_json::to_value(o).ok())
        .unwrap_or_else(|| json!({ "kind": if imported { "imported" } else { "learned" } }));
    o["label"] = json!(origin_label(r));
    o
}

/// The evidence a reflex's strength was decided on.
fn evidence(r: &Route, why: Option<&Why>) -> Value {
    let e = &r.evidence;
    // What the strength was decided on: shipped facts, a value estimate, or a follow count.
    let basis = if r.is_instinct() {
        "instinct"
    } else if crate::evidence::value_line(r, why).is_some() {
        "value"
    } else if matches!(r.step, Step::Card { .. }) {
        "family"
    } else {
        "count"
    };
    let uplift = e.uplift.as_ref().map(|u| {
        let mut v = serde_json::to_value(u).unwrap_or(Value::Null);
        v["line"] = json!(u.line());
        v
    });
    json!({
        "support": e.support, "opportunities": e.opportunities, "rate": e.rate, "lower_bound": e.lower_bound,
        "turns_saved": e.turns_saved, "days": e.days, "exposed": e.exposed, "dormant_since": e.dormant_since,
        "provenance": e.provenance,
        "describe": crate::evidence::describe(r, why),
        "basis": basis,
        "why": why.map(|w| json!({
            "shape": w.shape, "pattern": w.pattern, "gain": w.gain, "lift": w.lift, "conf": w.conf,
            "conf_lb": w.conf_lb, "support": w.support, "days": w.days, "backoff": w.backoff,
        })),
        "uplift": uplift,
        "outcome_lift": e.outcome_lift,
        "effect_evidence": e.effect_evidence,
        "execution": e.execution,
    })
}

/// The decisions a person can make about a reflex now, and why it is listed: the same object in
/// the review queue and on the map.
#[derive(Debug, Clone, Serialize)]
pub struct Decisions {
    /// The review verbs that apply (`adage review <verb> <reference> <id>`).
    pub verbs: Vec<&'static str>,
    /// Why it is listed, as `{kind, why}`.
    pub reasons: Vec<Reason>,
    /// The command for its next step when it is more than a verb (an execution-only reflex's grant).
    pub next: Option<String>,
    /// A grant Adage could ask the person for (architecture §3.12, "How Adage asks"): set where
    /// the grants and the policy are known (`graph --json`), empty elsewhere.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub offers: Vec<Offer>,
}

/// A grant that would let a reflex act for the agent, and the command a press runs for it. The
/// binary picks the kind: a finish-line step (after an edit) runs as an action in the sandbox; a
/// step due in a context is made as the agent's own call without a model request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Offer {
    /// `execute` (an E2 action, staged then confirmed) or `predict`.
    pub kind: &'static str,
    /// The first act: `review delegate … --level e2 --stage --json`, or the predict grant itself.
    pub command: String,
    /// The second act, for an action: `review delegate … --level e2 --confirm`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirm: Option<String>,
}

impl Offer {
    /// The offer for `r` (in the track installed as `reference`), if it qualifies: a fire reflex
    /// in service that runs one fixed command with a verify or regenerate effect, after an edit
    /// or in a context, with no execution evidence of its own (an execution-only reflex has its
    /// own grant path), holding no grant (`granted`), where policy would permit it (`permitted`).
    pub fn of(reference: &str, r: &Route, granted: bool, permitted: bool) -> Option<Offer> {
        use adage_core::Effect;
        use adage_tracks::{RouteState, Step, Strength, Trigger};
        let qualifies = r.strength == Strength::Fire
            && matches!(r.state, RouteState::Firm | RouteState::Forming)
            && matches!(r.effect, Effect::Verify | Effect::Regenerate)
            && matches!(r.step, Step::Run { .. })
            && r.slots.is_empty()
            && r.evidence.execution.is_none()
            && !granted
            && permitted;
        if !qualifies {
            return None;
        }
        let base = format!("adage review delegate {reference} {}", r.id);
        match r.trigger {
            Trigger::AfterEdit { .. } => Some(Offer {
                kind: "execute",
                command: format!("{base} --level e2 --stage --json"),
                confirm: Some(format!("{base} --level e2 --confirm")),
            }),
            Trigger::Context { .. } => {
                Some(Offer { kind: "predict", command: format!("{base} --predict"), confirm: None })
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Reason {
    pub kind: &'static str,
    pub why: String,
}

impl Decisions {
    pub fn of(reference: &str, r: &Route, verbs: Vec<&'static str>, reasons: &[(&'static str, String)]) -> Decisions {
        Decisions {
            verbs,
            reasons: reasons.iter().map(|(kind, why)| Reason { kind, why: why.clone() }).collect(),
            next: r
                .evidence
                .execution
                .as_ref()
                .map(|_| format!("adage review delegate {reference} {} --level e2", r.id)),
            offers: Vec::new(),
        }
    }
}

/// What a reflex did in sessions, from the spools: in treated sessions, how agents responded to it
/// being delivered; in control sessions (`control`), whether they took its path unaided.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Live {
    pub emitted: u64,
    pub would_have: u64,
    pub followed: u64,
    pub ignored: u64,
    pub misfired: u64,
    pub obligation_met: u64,
    pub control: Control,
    /// Distinct sessions it was delivered in.
    pub sessions: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Control {
    pub unaided: u64,
    pub not_reached: u64,
    pub obligation_met: u64,
}

pub(crate) fn lower(x: impl std::fmt::Debug) -> String {
    format!("{x:?}").to_lowercase()
}

pub fn trigger(t: &Trigger) -> Value {
    let (kind, label) = match t {
        Trigger::Prompt { span } => ("prompt", format!("a prompt names a {}", span_kind_label(*span))),
        Trigger::AfterCommand { command } => ("after_command", format!("`{command}` succeeds")),
        Trigger::AfterFailure { command, signature } => ("after_failure", format!("`{command}` fails: {signature}")),
        Trigger::AfterEdit { paths } => ("after_edit", format!("an edit touches {paths}")),
        Trigger::SessionStart => ("session_start", "the first prompt of a session".to_string()),
        Trigger::Intent { family, .. } => ("intent", format!("a prompt means: {family}")),
        Trigger::Context { items, .. } => ("context", format!("a context of {} events", items.len())),
    };
    let mut v = json!({ "kind": kind, "label": label });
    match t {
        // How close a prompt must be to the family (cosine), what a card's grey zone is measured from.
        Trigger::Intent { family, threshold, .. } => {
            v["family"] = json!(family);
            v["threshold"] = json!(round(f64::from(*threshold)));
        }
        // The events, in time order, each within `within` of the one before.
        Trigger::Context { items, within } => {
            v["within"] = json!(within);
            v["items"] = items.iter().map(ctx_item).collect();
        }
        _ => {}
    }
    v
}

/// One event of a context trigger, as `{kind, label}`.
fn ctx_item(i: &adage_tracks::CtxItem) -> Value {
    use adage_tracks::CtxItem;
    let first =
        |keys: &[String]| keys.first().map(|k| k.strip_prefix("exec ").unwrap_or(k).to_string()).unwrap_or_default();
    let (kind, label) = match i {
        CtxItem::Act { keys } => ("act", format!("`{}` succeeds", first(keys))),
        CtxItem::Fail { keys, signature } => ("fail", format!("`{}` fails: {signature}", first(keys))),
        CtxItem::Edit { glob } => ("edit", format!("an edit touches {glob}")),
        CtxItem::PromptValue { .. } => ("prompt_value", "a prompt names a known value".to_string()),
        CtxItem::Intent { family, .. } => ("intent", format!("a prompt means: {family}")),
    };
    json!({ "kind": kind, "label": label })
}

/// Three decimals: thresholds and posteriors as people compare them.
pub(crate) fn round(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// A procedure card's contents (`adage_tracks::card`): files in order, the commands that worked
/// (with their replay verdict: `verified` true, false, or null when not replayed), the commands
/// that mostly failed, and the finishing check. Delivered once a session at a prompt at least
/// `threshold` close to the family, or, from `grey_from` up, at the first edit of one of its
/// files. It never travels in a pack.
fn card_json(card: &adage_tracks::card::Card, threshold: Option<f32>) -> Value {
    let cmd = |c: &adage_tracks::card::CardCommand| {
        json!({ "command": c.command, "sessions": c.sessions, "runs": c.runs, "failed": c.failed,
                "error": (!c.error.is_empty()).then_some(&c.error), "verified": c.verified })
    };
    json!({
        "sessions": card.sessions,
        "passed": card.passed,
        "files": card.files.iter().map(|f| json!({ "path": f.path, "sessions": f.sessions })).collect::<Vec<_>>(),
        "exemplars": card.exemplars.iter().map(|x| json!({ "path": x.path, "lines": x.lines })).collect::<Vec<_>>(),
        "commands": card.commands.iter().map(cmd).collect::<Vec<_>>(),
        "avoid": card.avoid.iter().map(cmd).collect::<Vec<_>>(),
        "finish": card.finish,
        "threshold": threshold.map(|t| round(f64::from(t))),
        "grey_from": threshold.map(|t| round(f64::from(t - adage_tracks::card::GREY_MARGIN))),
        "shared": false,
    })
}

fn step(s: &Step) -> Value {
    match s {
        Step::Run { command } => json!({ "kind": "run", "label": command }),
        Step::ReadBatch { slot, max } => {
            json!({ "kind": "read_batch", "label": format!("read ≤{max} listed files at once"), "slot": slot })
        }
        Step::Avoid { avoid, instead, signature } => {
            json!({ "kind": "avoid", "label": instead, "avoid": avoid, "signature": signature })
        }
        Step::Advise { template, slots } => {
            json!({ "kind": "advise", "label": adage_tracks::advice::text(template, slots).unwrap_or_default(), "template": template })
        }
        Step::Sequence { commands } => {
            json!({ "kind": "sequence", "label": commands.join(" → "), "commands": commands })
        }
        Step::Card { card } => json!({
            "kind": "card",
            "label": format!(
                "procedure card: {} · from {}",
                crate::words::count(card.files.len(), "file"),
                crate::words::count(card.sessions as usize, "trace")
            ),
        }),
        Step::Skill { skill, args } => {
            let mut v = json!({ "kind": "skill", "label": format!("use the {skill} skill"), "skill": skill });
            if let Some(a) = args {
                v["args"] = json!(a);
            }
            v
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Offer;
    use adage_core::Effect;
    use adage_tracks::*;

    fn route(trigger: Trigger, effect: Effect) -> Route {
        Route {
            id: "regenerate".into(),
            summary: String::new(),
            trigger,
            step: Step::Run { command: "make gen".into() },
            slots: vec![],
            effect,
            strength: Strength::Fire,
            state: RouteState::Firm,
            exit: vec![],
            evidence: Evidence::default(),
            authored: false,
        }
    }

    /// The binary picks the kind of grant an ask offers, and offers none where it would not hold.
    #[test]
    fn offers_name_the_grant_the_binary_would_ask_for() {
        let edit = route(Trigger::AfterEdit { paths: "schema/*.toml".into() }, Effect::Regenerate);
        let o = Offer::of("acme/finish", &edit, false, true).unwrap();
        assert_eq!(o.kind, "execute");
        assert_eq!(o.command, "adage review delegate acme/finish regenerate --level e2 --stage --json");
        assert_eq!(o.confirm.as_deref(), Some("adage review delegate acme/finish regenerate --level e2 --confirm"));
        let ctx = route(Trigger::Context { items: vec![], within: 3 }, Effect::Verify);
        let o = Offer::of("acme/finish", &ctx, false, true).unwrap();
        assert_eq!((o.kind, o.command.as_str()), ("predict", "adage review delegate acme/finish regenerate --predict"));
        // Granted, not permitted, not fire, observe, or execution-only: nothing to offer.
        assert_eq!(Offer::of("t", &edit, true, true), None);
        assert_eq!(Offer::of("t", &edit, false, false), None);
        let mut prime = edit.clone();
        prime.strength = Strength::Prime;
        assert_eq!(Offer::of("t", &prime, false, true), None);
        assert_eq!(
            Offer::of("t", &route(Trigger::AfterEdit { paths: "x".into() }, Effect::Observe), false, true),
            None
        );
        let mut exec_only = edit.clone();
        exec_only.evidence.execution = Some(ExecutionEvidence { opportunities: 3, successful: 3, sessions: 3 });
        assert_eq!(Offer::of("t", &exec_only, false, true), None);
        assert_eq!(
            Offer::of(
                "t",
                &route(Trigger::Prompt { span: adage_core::spans::SpanKind::Ticket }, Effect::Verify),
                false,
                true
            ),
            None
        );
    }
}
