//! The text the model reads: one `<adage-world kind="…">` block per injection class, plain and
//! factual, saying it is from Adage and not from the user, inside its token budget. Never the
//! artifact's content: a locator, a title, a summary the daemon made locally.

use crate::gate::Signal;
use crate::text::{ago, fit, hhmm, local_day, until};
use crate::window::is_open;
use crate::WorldCtx;
use adage_trace::{Artifact, ArtifactKind, CalendarBlock};

use crate::text::FROM_ADAGE;

fn line_for(ctx: &WorldCtx<'_>, a: &Artifact) -> String {
    let when = ago(ctx.now, a.last_touched);
    let mut s = match a.kind {
        ArtifactKind::CalendarEvent => {
            let block = ctx.blocks.iter().find(|b| b.id == a.id);
            match block {
                Some(b) if b.end <= ctx.now => {
                    format!("\"{}\" (meeting, ended {}, {} attendees)", a.title, ago(ctx.now, b.end), b.attendees)
                }
                Some(b) => format!(
                    "\"{}\" (meeting, {} to {})",
                    a.title,
                    hhmm(b.start, ctx.tz_offset_min),
                    hhmm(b.end, ctx.tz_offset_min)
                ),
                None => format!("\"{}\" (meeting, {when})", a.title),
            }
        }
        ArtifactKind::Pr => format!("{} \"{}\" (pull request{}, {when})", a.locator, a.title, state_note(a)),
        ArtifactKind::Ticket => format!("{} \"{}\" (ticket{}, {when})", a.locator, a.title, state_note(a)),
        ArtifactKind::Transcript => format!("\"{}\" (transcript, {when}): {}", a.title, a.locator),
        ArtifactKind::Doc => format!("\"{}\" (document, {when}): {}", a.title, a.locator),
        ArtifactKind::File => format!("{} (file, {when})", a.locator),
        ArtifactKind::Url => format!("\"{}\" (page, {when}): {}", a.title, a.locator),
    };
    if a.kind == ArtifactKind::CalendarEvent && !a.people.is_empty() {
        s.push_str(&format!("; with {}", a.people.join(", ")));
    }
    s
}

fn state_note(a: &Artifact) -> String {
    a.state.as_deref().filter(|s| !s.is_empty()).map(|s| format!(", {}", s.replace(':', " "))).unwrap_or_default()
}

/// The related artifacts a hint lists under its main one: a meeting's transcript and attachments,
/// a ticket's pull request.
fn related<'a>(ctx: &WorldCtx<'a>, a: &Artifact) -> Vec<&'a Artifact> {
    let mut out: Vec<&Artifact> =
        ctx.artifacts.iter().filter(|r| a.related.contains(&r.id) || r.related.contains(&a.id)).collect();
    out.sort_by(|x, y| y.last_touched.cmp(&x.last_touched).then_with(|| x.id.cmp(&y.id)));
    out.truncate(4);
    out
}

/// One likely reference.
pub fn hint(ctx: &WorldCtx<'_>, a: &Artifact, _signal: &Signal) -> String {
    let mut lines = vec!["<adage-world kind=\"hint\">".to_string(), format!("Likely reference: {}", line_for(ctx, a))];
    for r in related(ctx, a) {
        let label = match r.kind {
            ArtifactKind::Transcript => "Transcript",
            ArtifactKind::Doc => "Document",
            ArtifactKind::Pr => "Pull request",
            ArtifactKind::Ticket => "Ticket",
            ArtifactKind::File => "File",
            ArtifactKind::Url => "Page",
            ArtifactKind::CalendarEvent => "Meeting",
        };
        lines.push(format!("{label}: {}", line_for(ctx, r)));
    }
    if let Some(s) = a.summary.as_deref().filter(|s| !s.is_empty()) {
        lines.push(format!("Summary: {s}"));
    }
    lines.push(format!(
        "{FROM_ADAGE} Read the artifact itself if you need it. If this is not what the user meant, say so in one line before going on."
    ));
    lines.push("</adage-world>".into());
    fit(&lines.join("\n"), ctx.params.hint_tokens)
}

/// Two or three contenders, for the agent to confirm.
pub fn disambiguation(ctx: &WorldCtx<'_>, arts: &[&Artifact]) -> String {
    let mut lines =
        vec!["<adage-world kind=\"disambiguation\">".to_string(), "The prompt may refer to one of these:".to_string()];
    for (i, a) in arts.iter().enumerate() {
        lines.push(format!("{}. {}", i + 1, line_for(ctx, a)));
    }
    lines.push(format!("{FROM_ADAGE} Confirm which one with the user in one line before going on."));
    lines.push("</adage-world>".into());
    fit(&lines.join("\n"), ctx.params.hint_tokens)
}

/// The world as it stands: today's calendar, open pull requests, assigned tickets.
pub fn snapshot(ctx: &WorldCtx<'_>) -> String {
    let today = local_day(ctx.now, ctx.tz_offset_min);
    let mut blocks: Vec<&CalendarBlock> =
        ctx.blocks.iter().filter(|b| local_day(b.start, ctx.tz_offset_min) == today && b.accepted).collect();
    blocks.sort_by_key(|b| (b.start, b.id.clone()));
    let mut lines = vec!["<adage-world kind=\"snapshot\">".to_string()];
    if blocks.is_empty() {
        lines.push("Today: no meetings on the calendar.".into());
    } else {
        let items: Vec<String> =
            blocks.iter().map(|b| format!("{} {}", hhmm(b.start, ctx.tz_offset_min), b.title)).collect();
        lines.push(format!(
            "Today: {} meeting{} ({}).",
            blocks.len(),
            if blocks.len() == 1 { "" } else { "s" },
            items.join(", ")
        ));
    }
    let mut prs: Vec<&Artifact> = ctx.artifacts.iter().filter(|a| a.kind == ArtifactKind::Pr && is_open(a)).collect();
    prs.sort_by(|a, b| b.last_touched.cmp(&a.last_touched).then_with(|| a.id.cmp(&b.id)));
    if !prs.is_empty() {
        let items: Vec<String> =
            prs.iter().take(8).map(|a| format!("{} \"{}\"{}", a.locator, a.title, state_note(a))).collect();
        lines.push(format!("Open pull requests: {}.", items.join("; ")));
    }
    let mut tickets: Vec<&Artifact> =
        ctx.artifacts.iter().filter(|a| a.kind == ArtifactKind::Ticket && is_open(a)).collect();
    tickets.sort_by(|a, b| b.last_touched.cmp(&a.last_touched).then_with(|| a.id.cmp(&b.id)));
    if !tickets.is_empty() {
        let items: Vec<String> =
            tickets.iter().take(8).map(|a| format!("{} \"{}\"{}", a.locator, a.title, state_note(a))).collect();
        lines.push(format!("Assigned tickets: {}.", items.join("; ")));
    }
    lines.push(format!(
        "{FROM_ADAGE} It describes the person's day as of {}; changes arrive with later prompts.",
        hhmm(ctx.now, ctx.tz_offset_min)
    ));
    lines.push("</adage-world>".into());
    fit(&lines.join("\n"), ctx.params.snapshot_tokens)
}

/// What changed since the last turn, coalesced.
pub fn delta(ctx: &WorldCtx<'_>, changes: &[String]) -> String {
    let mut lines =
        vec!["<adage-world kind=\"delta\">".to_string(), format!("Since last turn: {}", changes.join("; "))];
    lines.push(FROM_ADAGE.into());
    lines.push("</adage-world>".into());
    fit(&lines.join("\n"), ctx.params.delta_tokens)
}

/// A change that blocks the session's work, or a schedule boundary.
pub fn interrupt(ctx: &WorldCtx<'_>, text: &str) -> String {
    let lines = [
        "<adage-world kind=\"interrupt\">".to_string(),
        text.to_string(),
        FROM_ADAGE.to_string(),
        "</adage-world>".to_string(),
    ];
    fit(&lines.join("\n"), ctx.params.interrupt_tokens)
}

/// "Meeting starts in 4 min: Marketing sync."
pub fn block_starting(ctx: &WorldCtx<'_>, b: &CalendarBlock) -> String {
    format!("Meeting starts {}: \"{}\".", until(ctx.now, b.start), b.title)
}

/// "User unavailable until 10:30 (Marketing sync). Continue with the stated defaults where safe;
/// queue questions; write a review brief when done."
pub fn autonomy(ctx: &WorldCtx<'_>, b: &CalendarBlock, brief_path: &str) -> String {
    format!(
        "The user is unavailable until {} (\"{}\"). Continue with the stated defaults where safe; queue anything that needs their judgement instead of asking now; when done, write a short review brief to {brief_path} saying what was done and what is queued. {FROM_ADAGE}",
        hhmm(b.end, ctx.tz_offset_min),
        b.title
    )
}

/// "The user returns at 10:30. Finalize the brief."
pub fn finalize(ctx: &WorldCtx<'_>, b: &CalendarBlock, brief_path: &str) -> String {
    format!(
        "The user returns at {}. Finish the review brief at {brief_path} now. {FROM_ADAGE}",
        hhmm(b.end, ctx.tz_offset_min)
    )
}
