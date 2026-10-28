//! The candidate window: the artifacts a prompt may refer to. Touched within the window, or a
//! meeting that ended within the meeting window with its attachments, or an open pull request or
//! ticket whatever its age; most recently touched first; capped.

use crate::WorldCtx;
use adage_trace::{Artifact, ArtifactKind};
use std::collections::BTreeSet;

/// Whether an artifact's state means it is still open (a pull request not merged or closed, a
/// ticket not done). An artifact with no state is taken as open.
pub fn is_open(a: &Artifact) -> bool {
    match a.state.as_deref() {
        None => true,
        Some(s) => {
            let s = s.to_ascii_lowercase();
            !(s.contains("closed") || s.contains("merged") || s.contains("done") || s.contains("resolved"))
        }
    }
}

/// The candidates for `ctx.now`, most recently touched first, ties by id.
pub fn candidates<'a>(ctx: &WorldCtx<'a>) -> Vec<&'a Artifact> {
    let p = ctx.params;
    let since = ctx.now.saturating_sub(p.window_ms);
    let meeting_since = ctx.now.saturating_sub(p.meeting_window_ms);
    let mut ids: BTreeSet<&str> = BTreeSet::new();
    for a in ctx.artifacts {
        let recent = a.last_touched >= since && a.last_touched <= ctx.now.saturating_add(p.meeting_window_ms);
        let open_item = matches!(a.kind, ArtifactKind::Pr | ArtifactKind::Ticket) && is_open(a);
        let meeting = a.kind == ArtifactKind::CalendarEvent
            && ctx.blocks.iter().any(|b| b.id == a.id && b.end >= meeting_since && b.start <= ctx.now);
        if recent || open_item || meeting {
            ids.insert(&a.id);
            if meeting {
                for r in &a.related {
                    ids.insert(r);
                }
            }
        }
    }
    let mut out: Vec<&Artifact> = ctx.artifacts.iter().filter(|a| ids.contains(a.id.as_str())).collect();
    out.sort_by(|a, b| b.last_touched.cmp(&a.last_touched).then_with(|| a.id.cmp(&b.id)));
    out.truncate(p.max_candidates);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Params;
    use adage_trace::CalendarBlock;

    fn art(id: &str, kind: ArtifactKind, touched: u64, state: Option<&str>, related: &[&str]) -> Artifact {
        Artifact {
            id: id.into(),
            kind,
            title: id.into(),
            locator: id.into(),
            summary: None,
            embedding: None,
            first_seen: touched,
            last_touched: touched,
            related: related.iter().map(|r| r.to_string()).collect(),
            state: state.map(str::to_string),
            people: vec![],
        }
    }

    #[test]
    fn the_window_keeps_recent_open_and_meeting_items_only() {
        let h = 3_600_000;
        let now = 100 * h;
        let arts = vec![
            art("file:old", ArtifactKind::File, now - 5 * h, None, &[]),
            art("file:new", ArtifactKind::File, now - h, None, &[]),
            art("github:pr:1", ArtifactKind::Pr, now - 30 * h, Some("open"), &[]),
            art("github:pr:2", ArtifactKind::Pr, now - 30 * h, Some("merged"), &[]),
            art("calendar:m", ArtifactKind::CalendarEvent, now - 3 * h, None, &["doc:brief"]),
            art("doc:brief", ArtifactKind::Doc, now - 20 * h, None, &[]),
            art("calendar:stale", ArtifactKind::CalendarEvent, now - 10 * h, None, &["doc:stale"]),
            art("doc:stale", ArtifactKind::Doc, now - 20 * h, None, &[]),
        ];
        let blocks = vec![
            CalendarBlock {
                id: "calendar:m".into(),
                start: now - 3 * h,
                end: now - h,
                title: "m".into(),
                busy: true,
                accepted: true,
                focus: false,
                attendees: 2,
            },
            CalendarBlock {
                id: "calendar:stale".into(),
                start: now - 10 * h,
                end: now - 9 * h,
                title: "s".into(),
                busy: true,
                accepted: true,
                focus: false,
                attendees: 2,
            },
        ];
        let params = Params::default();
        let embed = |_: &str| None;
        let ctx = WorldCtx {
            now,
            tz_offset_min: 0,
            artifacts: &arts,
            events: &[],
            blocks: &blocks,
            embed: &embed,
            params: &params,
        };
        let ids: Vec<&str> = candidates(&ctx).iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["file:new", "calendar:m", "doc:brief", "github:pr:1"]);
    }
}
