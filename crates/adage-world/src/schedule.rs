//! The calendar's levers (the design's "Calendar and schedule policy"): the prompt-cache TTL when a
//! gap is predicted and the session holds enough cached tokens to make a longer TTL pay, an
//! autonomy window when the session goes idle inside a busy block, a finalize nudge before the
//! block ends, and a proposed review block when briefs are pending and the calendar has room.
//! Keepalive pings are never proposed.

use crate::render;
use crate::WorldCtx;
use adage_trace::CalendarBlock;

/// What the hook knows of the session when it asks.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Liveness {
    /// The session is idle (a `Stop` or an idle notification) since this time.
    pub idle_since: Option<u64>,
    /// Tokens the session holds in the prompt cache (its context fill).
    pub cached_tokens: u64,
    /// Review briefs written and not yet read, across sessions.
    pub briefs_pending: u32,
    /// An autonomy message was already sent for this block.
    pub autonomy_sent_for: Option<String>,
    /// The brief's path, for the texts.
    pub brief_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ttl {
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Autonomy {
    pub block: String,
    pub until: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    pub title: String,
    pub start: u64,
    pub end: u64,
}

/// The scheduler's answer: each lever, or nothing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    /// Set the main conversation's TTL before the next request (`1h` across a predicted gap, `5m`
    /// once the person is back).
    pub ttl: Option<Ttl>,
    /// Submit this when the session is idle: the person is in a block.
    pub autonomy: Option<Autonomy>,
    /// Submit the finalize text at this time (block end minus the lead).
    pub finalize_at: Option<(u64, String)>,
    /// Propose (never create) a review block.
    pub review: Option<Review>,
    /// A block longer than the long-block bound covers the gap: let idle compaction work, say so.
    pub long_block: bool,
}

/// The next busy block that starts within `lookahead` of `now`, or covers `now`.
fn upcoming(blocks: &[CalendarBlock], now: u64, lookahead: u64) -> Option<&CalendarBlock> {
    blocks
        .iter()
        .filter(|b| b.busy && b.accepted && b.end > now && b.start <= now.saturating_add(lookahead))
        .min_by_key(|b| (b.start, b.id.clone()))
}

/// The scheduler.
pub fn schedule(ctx: &WorldCtx<'_>, live: &Liveness) -> Plan {
    let p = ctx.params;
    let mut plan = Plan::default();
    let Some(idle_since) = live.idle_since else {
        // The person just spoke: a longer TTL set across a gap is reverted.
        plan.ttl = Some(Ttl::FiveMinutes);
        plan.review = review(ctx, live);
        return plan;
    };
    let _ = idle_since;
    if let Some(b) = upcoming(ctx.blocks, ctx.now, p.gap_lookahead_ms) {
        let gap = b.end.saturating_sub(ctx.now);
        // Break-even: a 1h write costs 2x base input against 1.25x for 5m; one avoided rewrite
        // pays for it, so flip only across a gap longer than the short TTL, on a heavy session.
        if gap > 5 * 60_000 && live.cached_tokens >= p.ttl_min_cached && b.end - b.start <= p.long_block_ms {
            plan.ttl = Some(Ttl::OneHour);
        }
        if b.end - b.start > p.long_block_ms {
            plan.long_block = true;
        }
        if b.covers(ctx.now) && live.autonomy_sent_for.as_deref() != Some(b.id.as_str()) {
            plan.autonomy =
                Some(Autonomy { block: b.id.clone(), until: b.end, text: render::autonomy(ctx, b, &live.brief_path) });
            plan.finalize_at =
                Some((b.end.saturating_sub(p.finalize_before_ms), render::finalize(ctx, b, &live.brief_path)));
        }
    }
    plan.review = review(ctx, live);
    plan
}

/// A free slot of at least the review length within the lookahead, when enough briefs wait.
fn review(ctx: &WorldCtx<'_>, live: &Liveness) -> Option<Review> {
    let p = ctx.params;
    if live.briefs_pending < p.review_min_briefs {
        return None;
    }
    let horizon = ctx.now.saturating_add(p.review_lookahead_ms);
    let mut busy: Vec<(u64, u64)> = ctx
        .blocks
        .iter()
        .filter(|b| b.busy && b.accepted && b.end > ctx.now && b.start < horizon)
        .map(|b| (b.start, b.end))
        .collect();
    busy.sort_unstable();
    let mut cursor = ctx.now;
    for (s, e) in busy {
        if s.saturating_sub(cursor) >= p.review_slot_ms {
            return Some(Review {
                title: format!("Agent review: {} briefs", live.briefs_pending),
                start: cursor,
                end: cursor + p.review_slot_ms,
            });
        }
        cursor = cursor.max(e);
    }
    (horizon.saturating_sub(cursor) >= p.review_slot_ms).then(|| Review {
        title: format!("Agent review: {} briefs", live.briefs_pending),
        start: cursor,
        end: cursor + p.review_slot_ms,
    })
}
