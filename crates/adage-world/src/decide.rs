//! The decision table (the design's "Decision"): the top candidate at or above the threshold with
//! a clear lead is attached; at or above the threshold with up to three contenders within the
//! margin, a one-line disambiguation lists them; otherwise nothing, and the miss is logged.

use crate::gate::Signal;
use crate::render;
use crate::score::Scored;
use crate::WorldCtx;
use adage_trace::InjectionClass;

/// What the resolver delivers.
#[derive(Debug, Clone, PartialEq)]
pub struct Hint {
    pub cls: InjectionClass,
    /// The `<adage-world>` block the model reads, inside the hint budget.
    pub text: String,
    /// The artifacts named, top first.
    pub artifacts: Vec<String>,
    /// The top score.
    pub confidence: f32,
    /// Every candidate's score, for the record.
    pub scored: Vec<Scored>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Attach(Hint),
    Disambiguate(Hint),
    None { reason: &'static str },
}

impl Decision {
    pub fn hint(&self) -> Option<&Hint> {
        match self {
            Decision::Attach(h) | Decision::Disambiguate(h) => Some(h),
            Decision::None { .. } => None,
        }
    }
}

/// The table over scored candidates (sorted, top first).
pub fn decide(ctx: &WorldCtx<'_>, signal: &Signal, scored: Vec<Scored>) -> Decision {
    let p = ctx.params;
    let find = |id: &str| ctx.artifacts.iter().find(|a| a.id == id);
    // A key or a pull request number spelled in the prompt that exactly one candidate carries
    // names it beyond doubt, whatever its score.
    if !signal.keys.is_empty() {
        let pw: Vec<String> = signal.keys.clone();
        let named: Vec<&Scored> =
            scored.iter().filter(|s| find(&s.id).is_some_and(|a| crate::gate::names_by_key(a, &pw))).collect();
        if let [one] = named[..] {
            if let Some(a) = find(&one.id) {
                let text = render::hint(ctx, a, signal);
                return Decision::Attach(Hint {
                    cls: InjectionClass::Hint,
                    text,
                    artifacts: vec![one.id.clone()],
                    confidence: one.score.max(p.threshold),
                    scored,
                });
            }
        }
    }
    let Some(top) = scored.first() else {
        return Decision::None { reason: "no candidates" };
    };
    if top.score < p.threshold {
        return Decision::None { reason: "below threshold" };
    }
    // A contender that is related to the top (a meeting and its transcript, a ticket and its pull
    // request) rides on the top's hint rather than competing with it.
    let related_to_top = |id: &str| {
        let (Some(t), Some(c)) = (find(&top.id), find(id)) else { return false };
        t.related.contains(&c.id) || c.related.contains(&t.id)
    };
    let contenders: Vec<&Scored> = scored
        .iter()
        .take_while(|s| top.score - s.score < p.margin)
        .filter(|s| s.id == top.id || !related_to_top(&s.id))
        .collect();
    if contenders.len() == 1 {
        let Some(a) = find(&top.id) else {
            return Decision::None { reason: "candidate vanished" };
        };
        let text = render::hint(ctx, a, signal);
        return Decision::Attach(Hint {
            cls: InjectionClass::Hint,
            text,
            artifacts: vec![top.id.clone()],
            confidence: top.score,
            scored,
        });
    }
    if contenders.len() > p.max_contenders {
        return Decision::None { reason: "too many contenders" };
    }
    let arts: Vec<_> = contenders.iter().filter_map(|s| find(&s.id)).collect();
    if arts.len() != contenders.len() {
        return Decision::None { reason: "candidate vanished" };
    }
    let text = render::disambiguation(ctx, &arts);
    Decision::Disambiguate(Hint {
        cls: InjectionClass::Disambiguation,
        text,
        artifacts: contenders.iter().map(|s| s.id.clone()).collect(),
        confidence: top.score,
        scored,
    })
}
