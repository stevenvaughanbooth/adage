//! Scoring: per candidate, similarity, recency, entity match and the kind the verb implies, each
//! in `[0, 1]`, weighted by [`crate::Params`].

use crate::gate::Signal;
use crate::text::{content_words, words};
use crate::WorldCtx;
use adage_intent::{cosine, dequantize};
use adage_trace::{Artifact, ArtifactKind};

/// One candidate's score and its terms, for the record and the evaluation.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Scored {
    pub id: String,
    pub score: f32,
    pub sim: f32,
    pub recency: f32,
    pub entity: f32,
    pub kind: f32,
}

/// The kind a verb implies: `("review", Pr)`. The first match in prompt order wins.
const VERBS: &[(&str, ArtifactKind)] = &[
    ("file", ArtifactKind::Transcript),
    ("summarise", ArtifactKind::Transcript),
    ("summarize", ArtifactKind::Transcript),
    ("follow-ups", ArtifactKind::Transcript),
    ("follow-up", ArtifactKind::Transcript),
    ("action", ArtifactKind::Transcript),
    ("notes", ArtifactKind::Transcript),
    ("transcribe", ArtifactKind::Transcript),
    ("review", ArtifactKind::Pr),
    ("merge", ArtifactKind::Pr),
    ("approve", ArtifactKind::Pr),
    ("rebase", ArtifactKind::Pr),
    ("fix", ArtifactKind::Ticket),
    ("close", ArtifactKind::Ticket),
    ("implement", ArtifactKind::Ticket),
    ("triage", ArtifactKind::Ticket),
    ("read", ArtifactKind::Doc),
    ("edit", ArtifactKind::Doc),
    ("draft", ArtifactKind::Doc),
    ("update", ArtifactKind::Doc),
];

/// The kind the prompt's verb implies, if any.
pub fn implied_kind(prompt_words: &[String]) -> Option<ArtifactKind> {
    prompt_words.iter().find_map(|w| VERBS.iter().find(|(v, _)| v == w).map(|(_, k)| *k))
}

/// Which kinds a named kind stands in for: "the meeting" may mean its transcript.
fn kind_matches(named: ArtifactKind, a: ArtifactKind) -> bool {
    named == a || matches!((named, a), (ArtifactKind::CalendarEvent, ArtifactKind::Transcript))
}

/// Cosine similarity between the prompt and the artifact's text, mapped to `[0, 1]`; 0 without an
/// embedding on either side or when their dimensions differ.
fn similarity(prompt_vec: Option<&[f32]>, a: &Artifact) -> f32 {
    let (Some(p), Some(q)) = (prompt_vec, a.embedding.as_deref()) else { return 0.0 };
    let v = dequantize(q);
    let c = cosine(p, &v);
    if c.is_finite() {
        c.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Share of the prompt's content words found in the artifact's title and summary: the lexical
/// floor under the embedding, so a title word the model's vector misses still counts.
fn overlap(prompt_content: &[String], a: &Artifact) -> f32 {
    if prompt_content.is_empty() {
        return 0.0;
    }
    let text = words(&a.text());
    let hits = prompt_content.iter().filter(|w| text.contains(w)).count();
    hits as f32 / prompt_content.len() as f32
}

/// Score every candidate. `signal.entities` carry the entity term; the one candidate of a kind
/// the prompt names ("the PR" with one open pull request) carries it too.
pub fn score(ctx: &WorldCtx<'_>, signal: &Signal, cands: &[&Artifact]) -> Vec<Scored> {
    let p = ctx.params;
    let content = content_words(&signal.prompt);
    let prompt_vec = (ctx.embed)(&adage_intent::normalize::normalize(&signal.prompt));
    let implied = implied_kind(&words(&signal.prompt));
    // A kind the prompt names ("the PR") with one candidate of that kind names it; with two or
    // three, it names each of them, so the table lists them for the agent to confirm.
    let by_name: Vec<ArtifactKind> = signal
        .kinds
        .iter()
        .copied()
        .filter(|k| (1..=p.max_contenders).contains(&cands.iter().filter(|a| a.kind == *k).count()))
        .collect();
    let mut out: Vec<Scored> = cands
        .iter()
        .map(|a| {
            let sim = similarity(prompt_vec.as_deref(), a).max(overlap(&content, a));
            let minutes = ctx.now.saturating_sub(a.last_touched) as f32 / 60_000.0;
            let mut recency = (-minutes / p.tau_min).exp().clamp(0.0, 1.0);
            // An open pull request or ticket is current by its state, however long ago it was
            // last opened.
            if matches!(a.kind, ArtifactKind::Pr | ArtifactKind::Ticket) && crate::window::is_open(a) {
                recency = recency.max(0.5);
            }
            let entity = if signal.entities.contains(&a.id) || by_name.contains(&a.kind) { 1.0 } else { 0.0 };
            let kind = match implied {
                Some(k) if kind_matches(k, a.kind) => 1.0,
                _ if signal.kinds.iter().any(|k| kind_matches(*k, a.kind)) => 1.0,
                _ => 0.0,
            };
            let score = p.w_sim * sim + p.w_recency * recency + p.w_entity * entity + p.w_kind * kind;
            Scored { id: a.id.clone(), score, sim, recency, entity, kind }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.id.cmp(&b.id))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verb_implies_a_kind() {
        assert_eq!(implied_kind(&words("file the follow-ups")), Some(ArtifactKind::Transcript));
        assert_eq!(implied_kind(&words("review that")), Some(ArtifactKind::Pr));
        assert_eq!(implied_kind(&words("hello")), None);
        assert!(kind_matches(ArtifactKind::CalendarEvent, ArtifactKind::Transcript));
        assert!(!kind_matches(ArtifactKind::Transcript, ArtifactKind::CalendarEvent));
    }
}
