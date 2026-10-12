//! The live view as a cockpit: `adage watch`, and the Live tab of `adage ui`.
//!
//! * **The pulse band**, on top: a heartbeat lamp that beats as events arrive, the repository,
//!   and stat tiles for what is shown (tool calls watched, reflexes fired, how many the agent
//!   followed with a meter, finish lines met, steps without a model request when there are any),
//!   beside an activity sparkline of events per minute with the minutes a reflex fired in green.
//!   A narrow pane has one line of the same, as chips.
//! * **The session flow**, the main area: each session as a graph that grows ([`super::flow`]).
//! * **The rail**, from [`RAIL_FROM`] columns: the reflexes in play (each with its strength and
//!   meters for how often it fired and was followed), the last conditioning as a card with the
//!   recent runs on a timeline (`l` lists every run), and health as lamps. Narrower, the rail
//!   folds into lines under the flow.
//!
//! Motion is brief and fast (the heartbeat, the newest node sliding in and glowing, a delivered
//! reflex's box flashing once, the follow meter easing); the loop redraws quickly only while
//! something moves ([`super::view::Live::pulsing`]), and nothing moves under
//! `ADAGE_REDUCED_MOTION` or in tests, which draw final frames.

use super::feed::{Act, FeedItem};
use super::flow::{self, Opts};
use super::health::Health;
use super::kit::{self, Card, Section, Tile, Tone};
use super::learnlog::{self, Tick, Tone as TickTone};
use super::theme::Theme;
use super::view::{self, Ctx, Live};
use adage_term::Glyph;
use adage_tracks::Strength;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use std::collections::{BTreeMap, HashSet};

/// The narrowest screen with the rail beside the flow.
pub const RAIL_FROM: u16 = 120;
/// The narrowest (and shortest) screen with stat tiles; tall tiles (big digits) from more.
pub const TILES_FROM: (u16, u16) = (90, 26);
pub const TALL_FROM: (u16, u16) = (120, 40);
/// The activity sparkline's longest reach back, in minutes.
pub const ACTIVITY_MIN: usize = 60;

// ---------------------------------------------------------------------------------------------
// What the band counts

/// What the items shown add up to. The control arm's acts are withheld, so they count for
/// nothing Adage did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub sessions: usize,
    pub first: Option<u64>,
    pub last: Option<u64>,
    /// Tool batches, and the calls in them.
    pub steps: usize,
    pub calls: usize,
    /// Reflexes delivered, by strength (fire, prime, inhibit).
    pub fired: usize,
    pub strengths: [usize; 3],
    pub followed: usize,
    /// Finish lines met, and stops where Adage asked the agent to go on first.
    pub met: usize,
    pub asked: usize,
    /// Steps Adage chose without a model request.
    pub chose: usize,
}

impl Stats {
    pub fn of(items: &[&FeedItem]) -> Stats {
        let mut s = Stats::default();
        let mut sessions = HashSet::new();
        for i in items {
            sessions.insert(i.session.as_str());
            s.first = Some(s.first.map_or(i.ts, |f| f.min(i.ts)));
            s.last = Some(s.last.map_or(i.ts, |l| l.max(i.ts)));
            if i.event == "tools" {
                s.steps += 1;
                s.calls += i.observed;
            }
            if i.control {
                continue;
            }
            for a in &i.acts {
                match a {
                    Act::Directed { strength, .. } => {
                        s.fired += 1;
                        s.strengths[match strength {
                            Strength::Fire => 0,
                            Strength::Prime => 1,
                            Strength::Inhibit => 2,
                        }] += 1;
                    }
                    Act::Followed(_) => s.followed += 1,
                    Act::Met(_) => s.met += 1,
                    Act::StopRequest => s.asked += 1,
                    Act::Other(k, _) if k == "chose" => s.chose += 1,
                    _ => {}
                }
            }
        }
        s.sessions = sessions.len();
        s
    }

    /// Of the reflexes delivered, the share the agent followed (`None` before any).
    pub fn follow_rate(&self) -> Option<f64> {
        (self.fired > 0).then(|| (self.followed as f64 / self.fired as f64).min(1.0))
    }
}

/// Events per minute over the `minutes` before `end` (oldest first), and which minutes a reflex
/// was delivered in.
pub fn activity(items: &[&FeedItem], end: u64, minutes: usize) -> (Vec<u64>, Vec<bool>) {
    let mut counts = vec![0u64; minutes];
    let mut fired = vec![false; minutes];
    for i in items {
        let age = (end.saturating_sub(i.ts) / 60_000) as usize;
        if age >= minutes {
            continue;
        }
        let b = minutes - 1 - age;
        counts[b] += 1;
        if i.fired() && !i.control {
            fired[b] = true;
        }
    }
    (counts, fired)
}

/// A reflex delivered in what is shown: what it tells the agent, its strength, how often it was
/// delivered and followed, and when last.
#[derive(Debug, Clone, PartialEq)]
pub struct InPlay {
    pub route: String,
    pub name: String,
    pub strength: Strength,
    pub fired: usize,
    pub followed: usize,
    pub last: u64,
}

/// The reflexes delivered in `items`, the most recently delivered first.
pub fn in_play(items: &[&FeedItem], gists: &BTreeMap<String, String>) -> Vec<InPlay> {
    let mut out: Vec<InPlay> = Vec::new();
    for i in items.iter().filter(|i| !i.control) {
        for a in &i.acts {
            match a {
                Act::Directed { route, strength, detail, instinct } => {
                    let name = gists.get(route).filter(|g| !g.is_empty()).cloned().unwrap_or_else(|| {
                        if !detail.is_empty() {
                            detail.clone()
                        } else if *instinct {
                            "an instinct built into Adage".into()
                        } else {
                            "a reflex learned here".into()
                        }
                    });
                    match out.iter_mut().find(|p| p.route == *route) {
                        Some(p) => {
                            p.fired += 1;
                            p.strength = *strength;
                            p.last = p.last.max(i.ts);
                        }
                        None => out.push(InPlay {
                            route: route.clone(),
                            name,
                            strength: *strength,
                            fired: 1,
                            followed: 0,
                            last: i.ts,
                        }),
                    }
                }
                Act::Followed(r) => {
                    if let Some(p) = out.iter_mut().find(|p| p.route == *r) {
                        p.followed += 1;
                    }
                }
                _ => {}
            }
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.last));
    out
}

/// The newest conditioning run shown, the changes it made, every run (oldest first), and the
/// newest failure when it is newer than the newest run.
#[derive(Debug, Default)]
pub struct Learned<'a> {
    pub last: Option<&'a Tick>,
    pub changes: Vec<&'a Tick>,
    pub runs: Vec<&'a Tick>,
    pub error: Option<&'a Tick>,
}

pub fn learned<'a>(ticks: &[&'a Tick]) -> Learned<'a> {
    let mut l = Learned::default();
    for k in ticks {
        match k.tone {
            TickTone::Run => {
                l.runs.push(k);
                l.last = Some(k);
                l.changes.clear();
                l.error = None;
            }
            TickTone::Change if l.last.is_some() => l.changes.push(k),
            TickTone::Error => l.error = Some(k),
            _ => {}
        }
    }
    l
}

// ---------------------------------------------------------------------------------------------
// Pieces

/// A moment, as the screen can say it: `12 s ago` with the wall clock, else the clock time.
fn moment(ts: u64, live: &Live, ctx: &Ctx, secs: bool) -> String {
    match live.anim.now_ms {
        Some(now) if now >= ts => crate::helpers::ago_short(now, ts),
        _ => view::clock(ts, ctx.tz, secs),
    }
}

/// The heartbeat: the live lamp (beating while events arrive) and a little trace beside it; or
/// paused, or scrolled back.
fn heartbeat(live: &Live, t: &Theme, trace: bool) -> Vec<Span<'static>> {
    if live.paused {
        return vec![Span::styled(
            format!("{} paused +{}", t.ch("‖", "="), live.held.len()),
            t.yellow().patch(t.bold()),
        )];
    }
    if live.scroll > 0 {
        return vec![Span::styled(format!("{} scrolled {}", t.ch("↑", "^"), live.scroll), t.yellow())];
    }
    if !t.color {
        return vec![Span::styled("LIVE", t.bold())];
    }
    let beat = live.anim.beat;
    let lamp = if beat == 3 { t.ch("◉", "*") } else { t.glyph(Glyph::Lamp) };
    let style = if beat >= 2 { t.green().patch(t.bold()) } else { t.green() };
    let mut v = vec![
        Span::styled(format!("{lamp} "), style),
        Span::styled("live", if beat > 0 { t.bold() } else { Style::default() }),
    ];
    if trace && t.unicode() {
        v.push(Span::raw(" "));
        v.extend(ecg(live, t));
    }
    v
}

/// The heartbeat's trace: a flat line, with a blip running along it after an event.
fn ecg(live: &Live, t: &Theme) -> Vec<Span<'static>> {
    const W: usize = 8;
    let Some(ms) = live.anim.fresh_ms else {
        return vec![Span::styled("▁".repeat(W), t.dim())];
    };
    // The blip enters from the right and runs out to the left.
    let head = (W + 2) as i64 - (ms as i64 * (W as i64 + 4) / view::BEAT_MS as i64);
    (0..W as i64)
        .map(|x| {
            let (c, hot) = match x - head {
                -1 => ("▃", true),
                0 => ("█", true),
                1 => ("▅", true),
                2 => ("▂", true),
                _ => ("▁", false),
            };
            Span::styled(c, if hot { t.green().patch(t.bold()) } else { t.dim() })
        })
        .collect()
}

/// The activity sparkline as styled rows: bars in the accent, the minutes a reflex fired in
/// green, the newest minute brightest.
fn spark_lines(counts: &[u64], fired: &[bool], rows: usize, t: &Theme) -> Vec<Line<'static>> {
    let n = counts.len();
    kit::spark_rows(counts, rows, t)
        .into_iter()
        .map(|row| {
            let spans: Vec<Span<'static>> = row
                .chars()
                .enumerate()
                .map(|(x, c)| {
                    let style = if fired.get(x).copied().unwrap_or(false) {
                        t.green().patch(t.bold())
                    } else if x + 1 == n && counts[x] > 0 {
                        t.accent().patch(t.bold())
                    } else if counts.get(x).copied().unwrap_or(0) == 0 {
                        t.dim()
                    } else {
                        t.accent()
                    };
                    Span::styled(c.to_string(), style)
                })
                .collect();
            Line::from(merge(spans))
        })
        .collect()
}

/// Neighbouring spans of one style as one.
fn merge(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::new();
    for s in spans {
        match out.last_mut() {
            Some(last) if last.style == s.style => last.content = format!("{}{}", last.content, s.content).into(),
            _ => out.push(s),
        }
    }
    out
}

/// Where the activity line ends: now, with the wall clock; else the newest event.
fn activity_end(live: &Live, stats: &Stats) -> u64 {
    match (live.anim.now_ms, stats.last) {
        (Some(now), Some(last)) => now.max(last),
        (Some(now), None) => now,
        (None, last) => last.unwrap_or(0),
    }
}

/// The scope in plain words.
fn scope_name(live: &Live, key: Option<&str>) -> String {
    live.scope(key).map_or_else(|| "all repositories".to_string(), super::scope::friendly)
}

// ---------------------------------------------------------------------------------------------
// The band

/// The watch pane's header: `adage watch`, the repository and its facts, the heartbeat.
fn chrome_header(live: &Live, health: Option<&Health>, stats: &Stats, ctx: &Ctx, width: u16) -> Line<'static> {
    let t = &ctx.theme;
    let gen = health.and_then(|h| h.generation.as_deref()).map(|g| format!("gen {}", &g[..g.len().min(8)]));
    let harnesses = health.map(|h| h.harnesses.join(", ")).filter(|h| !h.is_empty());
    // A newer Adage (the release check's cache): a fact like the others, the first to go.
    let update = crate::update::newer(&ctx.paths).map(|r| format!("{} is out{}adage update", r.version, t.sep()));
    let last = stats.last.map(|ts| format!("last call {}", moment(ts, live, ctx, true)));
    let left = |facts: &[&Option<String>]| -> Vec<Span<'static>> {
        let mut v = vec![Span::styled(" adage", t.title()), Span::styled(" watch", t.bold())];
        let mut parts = vec![ctx.label.clone()];
        parts.extend(facts.iter().filter_map(|f| (*f).clone()));
        v.push(Span::styled(format!("  {}", parts.join(&t.sep())), t.ink2()));
        if ctx.demo {
            v.push(Span::styled(format!("{}demo", t.sep()), t.control()));
        }
        v
    };
    let right = |last: Option<&String>, trace: bool| -> Vec<Span<'static>> {
        let mut v = Vec::new();
        if let Some(l) = last {
            v.push(Span::styled(format!("{l}   "), t.dim()));
        }
        v.extend(heartbeat(live, t, trace));
        v.push(Span::raw(" "));
        v
    };
    let w = |s: &[Span]| kit::spans_width(s);
    let options = [
        (left(&[&gen, &harnesses, &update]), right(last.as_ref(), true)),
        (left(&[&gen, &update]), right(last.as_ref(), true)),
        (left(&[&gen, &harnesses]), right(last.as_ref(), true)),
        (left(&[&gen, &harnesses]), right(None, true)),
        (left(&[&gen]), right(None, true)),
        (left(&[]), right(None, true)),
        (left(&[]), right(None, false)),
    ];
    let (l, r) = options.iter().find(|(l, r)| w(l) + w(r) + 2 <= width as usize).cloned().unwrap_or_else(|| {
        (vec![Span::styled(" adage", t.title()), Span::styled(" watch", t.bold())], right(None, false))
    });
    let gap = (width as usize).saturating_sub(w(&l) + w(&r));
    let mut spans = l;
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(r);
    Line::from(kit::clip_spans(spans, width as usize, t))
}

/// The band's first line in the app: the heartbeat, the repository, and what is shown.
fn caption(live: &Live, key: Option<&str>, stats: &Stats, ctx: &Ctx, width: u16) -> Line<'static> {
    let t = &ctx.theme;
    let mut spans = vec![Span::raw(" ")];
    spans.extend(heartbeat(live, t, true));
    spans.push(Span::raw("   "));
    spans.push(Span::styled(scope_name(live, key), t.bold()));
    let mut facts = Vec::new();
    match stats.sessions {
        0 => {}
        1 => facts.push("this session".to_string()),
        n => facts.push(format!("{n} sessions")),
    }
    if let Some(first) = stats.first {
        facts.push(format!("since {}", view::clock(first, ctx.tz, false)));
    }
    if let Some(last) = stats.last {
        facts.push(format!("last call {}", moment(last, live, ctx, true)));
    }
    // Short of room, the facts go from the right before anything is cut.
    let wide = |facts: &[String]| {
        kit::spans_width(&spans) + facts.iter().map(|f| kit::width(&t.sep()) + kit::width(f)).sum::<usize>()
    };
    while !facts.is_empty() && wide(&facts) > width as usize {
        facts.pop();
    }
    if !facts.is_empty() {
        spans.push(Span::styled(format!("{}{}", t.sep(), facts.join(&t.sep())), t.ink2()));
    }
    Line::from(kit::clip_spans(spans, width as usize, t))
}

/// The band in one line, for a narrow pane: what Adage did as chips, and the last minutes'
/// activity at the right.
fn chips(live: &Live, items: &[&FeedItem], stats: &Stats, ctx: &Ctx, width: u16, lamp: bool) -> Line<'static> {
    let t = &ctx.theme;
    let mut parts: Vec<Vec<Span<'static>>> = Vec::new();
    let num = |n: usize, tone: Tone| {
        Span::styled(n.to_string(), if n > 0 { tone.style(t).patch(t.bold()) } else { t.bold() })
    };
    if stats.fired == 0 {
        parts.push(vec![Span::styled(format!("{} no reflex fired yet", t.glyph(Glyph::Reflex)), t.dim())]);
    } else {
        parts.push(vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Reflex)), t.accent()),
            num(stats.fired, Tone::Ok),
            Span::styled(" fired", t.ink2()),
        ]);
        let rate = live.anim.follow.or(stats.follow_rate()).unwrap_or(0.0);
        let mut followed = vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Ok)), t.green()),
            num(stats.followed, Tone::Ok),
            Span::styled(" followed ", t.ink2()),
        ];
        followed.extend(kit::meter(rate, 6, Tone::Ok, t));
        parts.push(followed);
    }
    if stats.met + stats.asked > 0 {
        parts.push(vec![
            Span::styled(format!("{} ", t.ch("■", "#")), t.yellow()),
            num(stats.met, Tone::Ok),
            Span::styled(if stats.met == 1 { " finish line" } else { " finish lines" }, t.ink2()),
        ]);
    }
    if stats.chose > 0 {
        parts.push(vec![num(stats.chose, Tone::Ok), Span::styled(" without a model", t.ink2())]);
    }
    parts.push(vec![num(stats.calls, Tone::Plain), Span::styled(" calls", t.ink2())]);
    let mut lead = vec![Span::raw(" ")];
    if lamp {
        lead.extend(heartbeat(live, t, false));
        lead.push(Span::raw("   "));
    }
    // The last minutes, as a one-row sparkline at the right, where there is room.
    let minutes = 12usize;
    let (counts, fired) = activity(items, activity_end(live, stats), minutes);
    let spark = spark_lines(&counts, &fired, 1, t).remove(0).spans;
    let join = |ps: &[Vec<Span<'static>>]| -> Vec<Span<'static>> {
        let mut v = lead.clone();
        for (k, p) in ps.iter().enumerate() {
            if k > 0 {
                v.push(Span::styled(t.sep(), t.dim()));
            }
            v.extend(p.iter().cloned());
        }
        v
    };
    let mut kept = parts.len();
    while kept > 1 && kit::spans_width(&join(&parts[..kept])) > width as usize {
        kept -= 1;
    }
    let mut spans = join(&parts[..kept]);
    let used = kit::spans_width(&spans);
    if used + minutes + 4 <= width as usize {
        spans.push(Span::raw(" ".repeat(width as usize - used - minutes - 1)));
        spans.extend(spark);
    }
    Line::from(kit::clip_spans(spans, width as usize, t))
}

/// The stat tiles, with the activity tile first.
fn draw_tiles(f: &mut Frame, area: Rect, live: &Live, items: &[&FeedItem], stats: &Stats, ctx: &Ctx, tall: bool) {
    let t = &ctx.theme;
    let n_stats = 4 + u16::from(stats.chose > 0);
    // The activity tile takes two shares of the width where there is room, one where there is not.
    let act_shares = if area.width >= 120 { 2 } else { 1 };
    let stat_inner = {
        let inner_w = area.width.saturating_sub(2);
        let share = inner_w.saturating_sub(n_stats) / (n_stats + act_shares);
        (share as usize).saturating_sub(4)
    };
    let mut tiles: Vec<Tile> = Vec::new();
    let more = |l: Vec<Span<'static>>| if tall { vec![Line::from(l)] } else { vec![] };
    // Narrow tiles take the short names (a title is its icon and name between two border dashes).
    let label = |long: &'static str, short: &'static str| if long.len() + 4 <= stat_inner { long } else { short };
    let sessions = match stats.sessions {
        0 => "no session yet".to_string(),
        1 => "this session".to_string(),
        n => format!("{n} sessions"),
    };
    tiles.push(
        Tile::new(label("Tool calls", "Calls"), stats.calls)
            .icon(t.ch("▪", "*"))
            .note(format!("in {}", crate::helpers::plural(stats.steps, "step", "steps")))
            .more(more(vec![Span::styled(sessions, t.dim())])),
    );
    // The strengths delivered: `● fire 4  ● prime 1`, or just the lamps and counts where the
    // tile is narrow (`FIRE 4 PRIME 1` without colour).
    let mix_of = |words: bool| -> Vec<Span<'static>> {
        [Strength::Fire, Strength::Prime, Strength::Inhibit]
            .iter()
            .zip(stats.strengths)
            .filter(|(_, n)| *n > 0)
            .flat_map(|(s, n)| {
                let mut v = if words || !t.color {
                    kit::strength(*s, t)
                } else {
                    vec![Span::styled(t.glyph(Glyph::Lamp), t.strength(*s))]
                };
                v.push(Span::styled(format!(" {n}  "), t.ink2()));
                v
            })
            .collect()
    };
    let mut mix = mix_of(true);
    if kit::spans_width(&mix) > stat_inner + 2 {
        mix = mix_of(false);
    }
    tiles.push(
        Tile::new("Fired", stats.fired)
            .icon(t.glyph(Glyph::Reflex))
            .tone(if stats.fired > 0 { Tone::Ok } else { Tone::Muted })
            .note_spans(if mix.is_empty() { vec![Span::styled("none yet", t.ink2())] } else { mix })
            .more(more(vec![Span::styled("reflexes delivered", t.dim())])),
    );
    let rate = live.anim.follow.or(stats.follow_rate()).unwrap_or(0.0);
    let meter_w = (area.width as usize / 10).clamp(6, 12);
    let mut meter = kit::meter(rate, meter_w, Tone::Ok, t);
    if stats.fired > 0 {
        meter.push(Span::styled(format!(" {:.0}%", rate * 100.0), t.ink2()));
    }
    let followed = match (stats.fired, tall) {
        (0, _) => "-".to_string(),
        (_, true) => stats.followed.to_string(),
        (n, false) => format!("{} of {n}", stats.followed),
    };
    tiles.push(
        Tile::new("Followed", followed)
            .icon(t.glyph(Glyph::Ok))
            .tone(match (stats.fired, stats.followed) {
                (0, _) => Tone::Muted,
                (_, 0) => Tone::Plain,
                _ => Tone::Ok,
            })
            .note_spans(meter)
            .more(more(vec![Span::styled(
                if stats.fired == 0 { "none to follow yet".to_string() } else { format!("of {} fired", stats.fired) },
                t.dim(),
            )])),
    );
    tiles.push(
        Tile::new(label("Finish lines", "Finish"), stats.met)
            .icon(t.ch("■", "#"))
            .tone(if stats.met > 0 { Tone::Ok } else { Tone::Muted })
            .note("met")
            .more(more(vec![Span::styled(
                match stats.asked {
                    0 => "no reminder needed".to_string(),
                    n => crate::helpers::plural(n, "reminder sent", "reminders sent"),
                },
                t.dim(),
            )])),
    );
    if stats.chose > 0 {
        tiles.push(
            Tile::new(label("No model", "Chosen"), stats.chose)
                .icon(t.glyph(Glyph::Active))
                .tone(Tone::Ok)
                .note("steps Adage chose")
                .more(more(vec![Span::styled("no model request", t.dim())])),
        );
    }
    // The widths: the activity tile two shares (to at most an hour of minutes), each stat one.
    let inner = Rect { x: area.x + 1, width: area.width.saturating_sub(2), ..area };
    let n = tiles.len() as u16;
    let gaps = n;
    let share = inner.width.saturating_sub(gaps) / (n + act_shares);
    let act_w = (inner.width.saturating_sub(gaps) - share * n).min(ACTIVITY_MIN as u16 + 4);
    let stat_w = (inner.width.saturating_sub(gaps + act_w)) / n;
    draw_activity(f, Rect { width: act_w, ..inner }, live, items, stats, ctx, tall);
    let mut x = inner.x + act_w + 1;
    for (k, tile) in tiles.iter().enumerate() {
        let w = if k + 1 == tiles.len() { inner.right().saturating_sub(x) } else { stat_w };
        tile.render(f, Rect { x, width: w, ..inner }, t);
        x += w + 1;
    }
}

/// The activity tile: events per minute as a sparkline, the minutes a reflex fired in green.
fn draw_activity(f: &mut Frame, area: Rect, live: &Live, items: &[&FeedItem], stats: &Stats, ctx: &Ctx, tall: bool) {
    let t = &ctx.theme;
    let minutes = (area.width as usize).saturating_sub(4).clamp(1, ACTIVITY_MIN);
    let (counts, fired) = activity(items, activity_end(live, stats), minutes);
    let peak = counts.iter().copied().max().unwrap_or(0);
    let title = Line::from(vec![
        Span::styled(t.border().horizontal_top, t.rule()),
        Span::styled(format!(" {} Activity ", t.ch("▁▃▅", ".:|")), t.ink2()),
    ]);
    let note = if peak > 0 { format!(" peak {peak}/min ") } else { " quiet ".to_string() };
    let mut block = Block::default().borders(Borders::ALL).border_set(t.border()).border_style(t.rule());
    // The note only where it does not run into the title.
    if title.width() + kit::width(&note) + 3 <= area.width as usize {
        block = block.title(
            Line::from(vec![Span::styled(note, t.dim()), Span::styled(t.border().horizontal_top, t.rule())])
                .right_aligned(),
        );
    }
    let block = block.title(title);
    let inner = block.inner(area).inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 });
    f.render_widget(block, area);
    let rows = if tall { inner.height.saturating_sub(2).max(1) } else { inner.height.max(1) } as usize;
    let mut lines = spark_lines(&counts, &fired, rows, t);
    if tall && inner.height >= 3 {
        // Under the bars, a rule with a mark at each minute a reflex fired; then the axis.
        let marks: Vec<Span<'static>> = fired
            .iter()
            .map(|f| {
                if *f {
                    Span::styled(t.glyph(Glyph::Active), t.green().patch(t.bold()))
                } else {
                    Span::styled(t.ch("─", "-"), t.rule())
                }
            })
            .collect();
        lines.push(Line::from(merge(marks)));
        let ago = format!("{minutes} min ago");
        let now = "now";
        let pad = minutes.saturating_sub(ago.len() + now.len());
        lines.push(Line::from(vec![
            Span::styled(ago, t.dim()),
            Span::raw(" ".repeat(pad)),
            Span::styled(now, t.accent()),
        ]));
    }
    f.render_widget(Paragraph::new(t.lines(lines)), inner);
}

// ---------------------------------------------------------------------------------------------
// The flow

/// The session's graph in `area`; returns the newest item it shows (what `enter` inspects).
fn draw_flow(f: &mut Frame, area: Rect, live: &Live, items: &[&FeedItem], key: Option<&str>, ctx: &Ctx) -> Option<u64> {
    let t = &ctx.theme;
    if area.height == 0 || area.width < 4 {
        return None;
    }
    let mut area = area;
    if live.paused && area.height > 2 {
        let banner = Line::from(vec![
            Span::raw(" "),
            kit::chip(&format!("{} PAUSED", t.ch("‖", "=")), Tone::Warn, t),
            Span::styled(
                format!("  {} arrived since{}", crate::helpers::plural(live.held.len(), "event", "events"), t.sep()),
                t.ink2(),
            ),
            Span::styled("p", t.key()),
            Span::styled(" resumes", t.ink2()),
        ]);
        f.render_widget(
            Paragraph::new(kit::clip_spans(banner.spans, area.width as usize, t).into_iter().collect::<Line>()),
            Rect { height: 1, ..area },
        );
        area = Rect { y: area.y + 2, height: area.height - 2, ..area };
    }
    if items.is_empty() {
        draw_waiting(f, area, live, key, ctx);
        return None;
    }
    let inner = Rect { x: area.x + 1, width: area.width - 1, ..area };
    let boxes = inner.width >= flow::BOXES_FROM && inner.height >= flow::BOX_ROWS;
    let o = Opts {
        width: inner.width,
        boxes,
        repo: live.scope(key).is_none(),
        gists: &live.gists,
        pulse: live.pulse,
        glow: live.anim.glow(),
    };
    let (lines, edge) = flow::lines(items, ctx, &o);
    let w = flow::window(&lines, inner.height as usize, live.scroll);
    let mut out = Vec::new();
    if let Some(e) = w.earlier {
        out.push(flow::earlier_line(e, boxes, t));
    }
    if let Some(k) = w.sticky {
        out.push(lines[k].render(edge, 0, t));
    }
    let slide = live.anim.slide();
    for l in &lines[w.from..w.to] {
        let mut line = l.render(edge, if l.newest { slide } else { 0 }, t);
        if l.newest && slide > 3 {
            for s in &mut line.spans {
                s.style = s.style.add_modifier(Modifier::DIM);
            }
        }
        out.push(line);
    }
    if w.newer > 0 {
        out.push(flow::newer_line(w.newer, boxes, t));
    }
    f.render_widget(Paragraph::new(t.lines(out)), inner);
    lines[w.from..w.to].iter().map(|l| l.seq).max()
}

/// No events yet: a card that says what will appear, while it watches.
fn draw_waiting(f: &mut Frame, area: Rect, live: &Live, key: Option<&str>, ctx: &Ctx) {
    let t = &ctx.theme;
    if !live.filter.is_empty() {
        let line = Line::from(Span::styled(
            format!(" Nothing matches {}. esc clears the filter.", t.quote(&live.filter)),
            t.dim(),
        ));
        f.render_widget(Paragraph::new(view::wrap_lines(vec![line], area.width)), area);
        return;
    }
    let scope = live.scope(key).map_or_else(|| "any repository".to_string(), super::scope::friendly);
    let next =
        |why: &'static str| Line::from(vec![Span::styled(why, t.dim()), Span::styled("adage init", t.command())]);
    if area.width < 52 || area.height < 11 {
        // A small pane: the plain lines, the explanation the first to go.
        let title = Line::from(Span::styled(format!(" No traces yet for {scope}."), t.dim()));
        let full = view::wrap_lines(
            vec![title.clone(), next(" Start an agent there: each step appears here as it happens. No hooks yet? ")],
            area.width,
        );
        let lines = if full.len() <= area.height as usize {
            full
        } else {
            let short = view::wrap_lines(vec![title, next(" No hooks yet? ")], area.width);
            short[short.len().saturating_sub(area.height as usize)..].to_vec()
        };
        f.render_widget(Paragraph::new(lines), area);
        return;
    }
    let w = area.width.saturating_sub(2).min(76);
    let inner_w = w as usize - 4;
    // A slow scan along a rule under the title while it waits (still with reduced motion).
    let scan: Vec<Span<'static>> = {
        let n = inner_w;
        let at = if t.motion { (live.anim.spin_ms / 90) as usize % (n + 8) } else { n + 8 };
        merge(
            (0..n)
                .map(|x| {
                    let near = at >= x && at - x < 8;
                    if near {
                        Span::styled(t.ch("━", "="), t.accent())
                    } else {
                        Span::styled(t.ch("─", "-"), t.rule())
                    }
                })
                .collect(),
        )
    };
    let title = Line::from(vec![Span::styled("No traces yet: waiting for your agent.", t.bold())]);
    let about = Line::from(Span::styled(
        "Start Claude Code, Codex or OpenCode in this repository. Each turn and tool step grows here as a graph, and every reflex Adage fires branches off the step it came after, lit green once the agent follows it.",
        t.ink2(),
    ));
    let mut body =
        vec![title.clone(), Line::from(scan.clone()), Line::raw(""), about, Line::raw(""), next("No hooks yet?  ")];
    if live.hide_quiet {
        body.push(Line::from(Span::styled("Quiet events are hidden (v shows them).", t.dim())));
    }
    let mut lines = kit::wrap_lines(t.lines(body), inner_w, 0);
    // Short of rows, the explanation goes first; the next command always stays.
    if lines.len() + 2 > area.height as usize {
        lines = kit::wrap_lines(t.lines(vec![title, Line::from(scan), next("No hooks yet?  ")]), inner_w, 0);
    }
    let h = (lines.len() as u16 + 2).min(area.height);
    let r = Rect { x: area.x + 1, y: area.y, width: w, height: h };
    let spin = kit::spinner(live.anim.spin_ms, t);
    Card::new(format!("{spin} watching {scope}")).tone(Tone::Accent).render(f, r, lines, t);
}

// ---------------------------------------------------------------------------------------------
// The rail, and the lines that stand in for it

/// The waiting-for-review line: `○ 1 needs you · 7 to look over · adage review`.
fn waiting_line(live: &Live, t: &Theme, width: u16) -> Option<Line<'static>> {
    let n = live.waiting.filter(|n| *n > 0)?;
    let line = |what: String, lead: &str| {
        Line::from(vec![
            Span::styled(format!("{lead}{} ", t.glyph(Glyph::Off)), t.dim()),
            Span::styled(format!("{what}{}", t.sep()), t.ink2()),
            Span::styled("adage review", t.command()),
        ])
    };
    let needs = live.needs.min(n);
    let words = crate::review::waiting_words(needs, n - needs).unwrap_or_default();
    let full = line(words, " ");
    // Narrow panes keep the command whole.
    Some(if full.width() <= width as usize { full } else { line(format!("{n} to review"), " ") })
}

/// The last conditioning in one line (the rail's card, folded): `◆ conditioned 19:49 · 108
/// traces · no reflex changed · 6 patterns forming   l all runs`.
fn learning_line(live: &Live, ticks: &[&Tick], ctx: &Ctx, width: u16) -> Line<'static> {
    let t = &ctx.theme;
    let l = learned(ticks);
    let mut spans = vec![Span::raw(" ")];
    if !live.learning.is_empty() {
        spans.push(Span::styled(
            format!("{} conditioning now{}  ", kit::spinner(live.anim.spin_ms, t), t.more()),
            t.yellow().patch(t.bold()),
        ));
    }
    let Some(run) = l.last else {
        // Short of room, the explanation goes and the command stays.
        for why in [
            format!(
                "{} Nothing conditioned yet{}runs show here as they finish{}",
                t.glyph(Glyph::Active),
                t.sep(),
                t.sep()
            ),
            format!("Nothing conditioned yet{}", t.sep()),
            String::new(),
        ] {
            let mut v = spans.clone();
            v.push(Span::styled(why, t.dim()));
            v.push(Span::styled("adage condition", t.command()));
            if kit::spans_width(&v) <= width as usize {
                return Line::from(v);
            }
        }
        return Line::from(vec![Span::styled(" adage condition", t.command())]);
    };
    let r = run.run.clone().unwrap_or_default();
    spans.push(Span::styled(format!("{} ", t.glyph(Glyph::Active)), t.accent()));
    spans.push(Span::styled(format!("conditioned {}", moment(run.ts, live, ctx, false)), t.bold()));
    let mut facts = Vec::new();
    if let Some(used) = r.used {
        facts.push(Span::styled(format!("{}{used} traces", t.sep()), t.ink2()));
    }
    facts.push(match r.changes {
        Some(0) | None => Span::styled(format!("{}no reflex changed", t.sep()), t.ink2()),
        Some(n) => Span::styled(
            format!("{}{}", t.sep(), crate::helpers::plural(n, "change", "changes")),
            t.green().patch(t.bold()),
        ),
    });
    if r.forming > 0 {
        facts.push(Span::styled(format!("{}{}", t.sep(), learnlog::forming_words(r.forming)), t.accent()));
    }
    if let Some(e) = l.error {
        facts.push(Span::styled(format!("{}{} {}", t.sep(), t.glyph(Glyph::Fail), e.short), t.red()));
    }
    spans.extend(facts);
    let hint = kit::hints(&[("l", "all runs")], t, 20);
    let used = kit::spans_width(&spans);
    let hw = kit::spans_width(&hint);
    if used + hw + 3 <= width as usize {
        spans.push(Span::raw(" ".repeat(width as usize - used - hw - 1)));
        spans.extend(hint);
    }
    Line::from(kit::clip_spans(spans, width as usize, t))
}

/// The rail: the reflexes in play, the last conditioning, health.
fn draw_rail(
    f: &mut Frame,
    area: Rect,
    live: &Live,
    items: &[&FeedItem],
    ticks: &[&Tick],
    health: Option<&Health>,
    ctx: &Ctx,
) {
    let t = &ctx.theme;
    let w = area.width as usize;
    // Health at the bottom, the conditioning card above it, the reflexes in play from the top.
    let mut lamps = view::lights_lines(health, t, area.width.saturating_sub(1));
    // Wrapped, not clipped: the rail is narrow and the reason is the part worth reading.
    if let Some(s) = health.and_then(|h| h.savings.as_ref()) {
        lamps.extend(view::wrap_hanging(vec![view::savings_line(s, t, u16::MAX)], area.width.saturating_sub(1), true));
    }
    let mut health_lines = vec![Section::new("Health").line(area.width, t)];
    health_lines.extend(lamps);
    let card = learning_card(live, ticks, ctx, w.saturating_sub(1));
    let mut learn_lines = vec![Section::new("Learning").line(area.width, t)];
    learn_lines.extend(card.into_iter().map(|l| {
        let mut v = vec![Span::raw(" ")];
        v.extend(l.spans);
        Line::from(v)
    }));
    learn_lines.extend(waiting_line(live, t, area.width));
    // The reflexes in play take the rows the other two leave, then the rest follow them.
    let room = (area.height as usize).saturating_sub(health_lines.len() + learn_lines.len() + 2).max(3);
    let mut lines = play_lines(live, items, ctx, area.width, room);
    lines.truncate(room);
    lines.push(Line::raw(""));
    lines.extend(learn_lines);
    lines.push(Line::raw(""));
    lines.extend(health_lines);
    f.render_widget(Paragraph::new(t.lines(lines)), area);
}

/// The reflexes delivered in what is shown, each with its strength, what it tells the agent, and
/// meters for how often it fired and how often the agent followed it.
fn play_lines(live: &Live, items: &[&FeedItem], ctx: &Ctx, width: u16, rows: usize) -> Vec<Line<'static>> {
    let t = &ctx.theme;
    let w = width as usize;
    let play = in_play(items, &live.gists);
    let mut lines = vec![if play.is_empty() {
        Section::new("Reflexes in play").line(width, t)
    } else {
        Section::new("Reflexes in play").count(play.len()).line(width, t)
    }];
    if play.is_empty() {
        let body = Line::from(Span::styled(
            "None fired yet. Each reflex Adage delivers shows here, with how often the agent took it.",
            t.dim(),
        ));
        for l in kit::wrap_spans(body.spans, w.saturating_sub(2), 0) {
            let mut v = vec![Span::raw(" ")];
            v.extend(l.spans);
            lines.push(Line::from(v));
        }
        return lines;
    }
    let max = play.iter().map(|p| p.fired).max().unwrap_or(1).max(1);
    let meter_w = (w.saturating_sub(30) / 2).clamp(3, 12);
    let hot = live.pulse.level > 0;
    let newest = play.first().map(|p| p.route.clone());
    for (k, p) in play.iter().enumerate() {
        if lines.len() + 3 > rows && k > 0 {
            lines.push(Line::from(Span::styled(format!(" +{} more", play.len() - k), t.dim())));
            break;
        }
        let word = Theme::strength_word(p.strength).to_ascii_uppercase();
        let flash = hot && newest.as_deref() == Some(p.route.as_str()) && k == 0;
        let mut head = vec![Span::raw(" "), kit::chip(&word, Tone::of_strength(p.strength), t), Span::raw(" ")];
        head.push(Span::styled(t.text(&p.name), if flash { t.bold().patch(t.accent()) } else { t.bold() }));
        lines.push(Line::from(kit::clip_spans(head, w, t)));
        let mut meters = vec![Span::styled("   fired ", t.dim()), Span::styled(format!("{:<3}", p.fired), t.bold())];
        meters.extend(kit::meter(p.fired as f64 / max as f64, meter_w, Tone::Accent, t));
        meters.push(Span::styled("  followed ", t.dim()));
        meters.push(Span::styled(
            format!("{:<3}", p.followed),
            if p.followed > 0 { t.green().patch(t.bold()) } else { t.bold() },
        ));
        meters.extend(kit::meter(p.followed as f64 / p.fired as f64, meter_w, Tone::Ok, t));
        lines.push(Line::from(kit::clip_spans(meters, w, t)));
    }
    lines
}

/// The last conditioning as a card: when, from how many traces, what it changed, the patterns
/// forming, and the recent runs on a timeline.
fn learning_card(live: &Live, ticks: &[&Tick], ctx: &Ctx, width: usize) -> Vec<Line<'static>> {
    let t = &ctx.theme;
    let l = learned(ticks);
    let inner = width.saturating_sub(4);
    let mut body: Vec<Line<'static>> = Vec::new();
    if !live.learning.is_empty() {
        body.push(Line::from(Span::styled(
            format!("{} conditioning now{}", kit::spinner(live.anim.spin_ms, t), t.more()),
            t.yellow().patch(t.bold()),
        )));
    }
    let Some(run) = l.last else {
        body.push(Line::from(Span::styled("Nothing conditioned yet.", t.bold())));
        body.push(Line::from(Span::styled("Runs show here as they finish.", t.dim())));
        body.push(Line::from(Span::styled("adage condition", t.command())));
        return Card::new("Conditioning").lines(body, width, t);
    };
    let r = run.run.clone().unwrap_or_default();
    let mut first = Vec::new();
    if let (Some(used), Some(read)) = (r.used, r.read) {
        first.push(Span::styled(format!("{used}"), t.bold()));
        first.push(Span::styled(format!(" of {read} traces"), t.ink2()));
    }
    if let Some(ms) = r.duration_ms {
        first.push(Span::styled(format!("{}{}", t.sep(), learnlog::duration(ms)), t.dim()));
    }
    if !first.is_empty() {
        body.push(Line::from(first));
    }
    match r.changes {
        Some(0) | None => body.push(Line::from(Span::styled("no reflex changed", t.ink2()))),
        Some(n) => {
            body.push(Line::from(Span::styled(
                crate::helpers::plural(n, "change", "changes"),
                t.green().patch(t.bold()),
            )));
            for c in l.changes.iter().filter_map(|k| k.change.as_ref()).filter(|c| c.what != "forming").take(2) {
                let name = live.gists.get(&c.route).map_or(c.name(), String::as_str).to_string();
                let to = match (&c.from, &c.to) {
                    (Some(a), Some(b)) => format!(" {a} {} {b}", t.glyph(Glyph::Arrow)),
                    (None, Some(b)) => format!(" {} {b}", c.what),
                    _ => format!(" {}", c.what),
                };
                let tw = kit::width(&to);
                body.push(Line::from(vec![
                    Span::styled(format!("{} ", t.glyph(Glyph::Reflex)), t.accent()),
                    Span::raw(t.fit(&name, inner.saturating_sub(tw + 2))),
                    Span::styled(to, t.green()),
                ]));
            }
        }
    }
    if r.forming > 0 {
        body.push(Line::from(Span::styled(
            format!("{} {}", t.glyph(Glyph::Off), learnlog::forming_words(r.forming)),
            t.accent(),
        )));
        // How close the closest is to being a reflex: traces seen of those it needs.
        if let (Some(seen), Some(need)) = (r.forming_seen, r.forming_need.filter(|n| *n > 0)) {
            let words = format!("  closest {seen} of {need} traces ");
            let mw = inner.saturating_sub(kit::width(&words)).clamp(3, 16);
            let mut v = vec![Span::styled(words, t.dim())];
            v.extend(kit::meter(seen as f64 / need as f64, mw, Tone::Accent, t));
            body.push(Line::from(v));
        }
    }
    if let Some(e) = l.error {
        body.push(Line::from(Span::styled(format!("{} {}", t.glyph(Glyph::Fail), e.short), t.red())));
    }
    body.push(timeline(&l.runs, inner, t));
    let title = format!("{} conditioned {}", t.glyph(Glyph::Active), moment(run.ts, live, ctx, false));
    let tone = if r.changes.unwrap_or(0) > 0 { Tone::Ok } else { Tone::Accent };
    Card::new(title)
        .tone(tone)
        .note(if r.auto { "auto" } else { "by hand" })
        .hints(&[("l", "all runs")])
        .lines(body, width, t)
}

/// The recent runs on a line, oldest at the left: `◆` a run that changed something, `◇` one that
/// did not; then how many.
fn timeline(runs: &[&Tick], width: usize, t: &Theme) -> Line<'static> {
    let label = format!("  {}", crate::helpers::plural(runs.len(), "run", "runs"));
    let w = width.saturating_sub(kit::width(&label)).max(4);
    let (first, last) = match (runs.first(), runs.last()) {
        (Some(a), Some(b)) => (a.ts, b.ts),
        _ => return Line::default(),
    };
    let span = (last - first).max(1) as f64;
    let mut cells: Vec<Option<bool>> = vec![None; w];
    for r in runs {
        let x = (((r.ts - first) as f64 / span) * (w - 1) as f64).round() as usize;
        let changed = r.run.as_ref().and_then(|x| x.changes).unwrap_or(0) > 0;
        cells[x.min(w - 1)] = Some(cells[x.min(w - 1)].unwrap_or(false) || changed);
    }
    let mut spans: Vec<Span<'static>> = cells
        .into_iter()
        .map(|c| match c {
            Some(true) => Span::styled(t.glyph(Glyph::Active), t.green().patch(t.bold())),
            Some(false) => Span::styled(t.glyph(Glyph::Done), t.accent()),
            None => Span::styled(t.ch("─", "-"), t.rule()),
        })
        .collect();
    spans = merge(spans);
    spans.push(Span::styled(label, t.dim()));
    Line::from(spans)
}

// ---------------------------------------------------------------------------------------------
// The whole

/// The cockpit in `area`. `chrome` adds the watch pane's header and key line.
pub fn draw(
    f: &mut Frame,
    area: Rect,
    live: &Live,
    health: Option<&Health>,
    here: Option<&str>,
    ctx: &Ctx,
    chrome: bool,
) {
    let t = &ctx.theme;
    if area.width < 30 || area.height < 8 {
        f.render_widget(
            Paragraph::new(t.text("adage watch: make this pane larger (30×8 at least)")).wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let key = here;
    let items = live.visible(key);
    let stats = Stats::of(&items);
    let ticks = live.visible_ticks(key);
    let rail = area.width >= RAIL_FROM && area.height >= 22;

    // The footer, where there is no rail: the last conditioning, the review line, the lamps.
    let learn = learning_line(live, &ticks, ctx, area.width);
    let waiting = waiting_line(live, t, area.width);
    let mut lamps = view::lights_lines(health, t, area.width);
    let savings = health.and_then(|h| h.savings.as_ref()).map(|s| view::savings_line(s, t, area.width));

    let mut header_h = u16::from(chrome);
    let mut tiles = area.width >= TILES_FROM.0 && area.height >= TILES_FROM.1;
    let mut tall = area.width >= TALL_FROM.0 && area.height >= TALL_FROM.1;
    let mut band_on = true;
    let mut gap = 1u16;
    let mut rule_foot = u16::from(!rail);
    let mut keys_h = if chrome { 2 } else { 0 };
    let mut sav_h = u16::from(savings.is_some() && !rail);
    let mut lamps_h = if rail { 0 } else { (lamps.len() as u16).clamp(1, 2) };
    let foot_fixed = if rail { 0 } else { 1 + u16::from(waiting.is_some()) };
    let mut flow_min = 2u16;
    let band_h = |tiles: bool, tall: bool, on: bool| -> u16 {
        match (on, tiles) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => (if tall { Tile::TALL } else { Tile::SHORT }) + u16::from(!chrome),
        }
    };
    loop {
        let used = header_h
            + band_h(tiles, tall, band_on)
            + gap
            + flow_min
            + rule_foot
            + foot_fixed
            + lamps_h
            + sav_h
            + keys_h;
        if used <= area.height {
            break;
        }
        if gap > 0 {
            gap = 0;
        } else if tall {
            tall = false;
        } else if rule_foot > 0 {
            rule_foot = 0;
        } else if keys_h == 2 {
            keys_h = 1;
        } else if tiles {
            tiles = false;
        } else if sav_h > 0 {
            sav_h = 0;
        } else if lamps_h > 1 {
            lamps_h -= 1;
        } else if band_on {
            band_on = false;
        } else if header_h > 0 {
            header_h = 0;
        } else if flow_min > 1 {
            flow_min = 1;
        } else {
            break;
        }
    }
    let foot_h = if rail { 0 } else { foot_fixed + lamps_h + sav_h };
    let [header_a, band_a, _, body_a, rule_a, foot_a, keys_a] = Layout::vertical([
        Constraint::Length(header_h),
        Constraint::Length(band_h(tiles, tall, band_on)),
        Constraint::Length(gap),
        Constraint::Min(flow_min),
        Constraint::Length(rule_foot),
        Constraint::Length(foot_h),
        Constraint::Length(keys_h),
    ])
    .areas(area);

    if header_h > 0 {
        f.render_widget(Paragraph::new(chrome_header(live, health, &stats, ctx, area.width)), header_a);
    }
    if band_on {
        if tiles {
            let tiles_a = if chrome {
                band_a
            } else {
                f.render_widget(
                    Paragraph::new(caption(live, key, &stats, ctx, area.width)),
                    Rect { height: 1, ..band_a },
                );
                Rect { y: band_a.y + 1, height: band_a.height - 1, ..band_a }
            };
            draw_tiles(f, tiles_a, live, &items, &stats, ctx, tall);
        } else {
            f.render_widget(Paragraph::new(chips(live, &items, &stats, ctx, area.width, !chrome)), band_a);
        }
    }

    // The flow's own header: what is shown, when it is not simply this repository.
    let mut notes = Vec::new();
    match live.scope(key) {
        None => notes.push("all repositories".to_string()),
        // Another repository than the one opened here (chosen, or the most recently active).
        Some(k) if Some(k) != key => notes.push(super::scope::friendly(k)),
        Some(_) => {}
    }
    if !live.filter.is_empty() {
        notes.push(format!("filter {}", t.quote(&live.filter)));
    }
    if live.hide_quiet {
        notes.push("quiet hidden".to_string());
    }
    let (flow_a, rail_a) = if rail {
        let rail_w = (area.width * 3 / 10).clamp(38, 56);
        let [flow_a, sep_a, rail_a] =
            Layout::horizontal([Constraint::Min(40), Constraint::Length(2), Constraint::Length(rail_w)]).areas(body_a);
        let bar: Vec<Line> = (0..sep_a.height).map(|_| Line::from(Span::styled(t.ch("│", "|"), t.rule()))).collect();
        f.render_widget(Paragraph::new(bar), Rect { x: sep_a.x + 1, width: 1, ..sep_a });
        (flow_a, Some(rail_a))
    } else {
        (body_a, None)
    };
    let flow_a = if notes.is_empty() || flow_a.height < 4 {
        flow_a
    } else {
        let note = Span::styled(notes.join(&t.sep()), t.dim());
        f.render_widget(Paragraph::new(view::rule("now", Some(note), flow_a.width, t)), Rect { height: 1, ..flow_a });
        Rect { y: flow_a.y + 1, height: flow_a.height - 1, ..flow_a }
    };
    let shown = draw_flow(f, flow_a, live, &items, key, ctx);

    if let Some(r) = rail_a {
        draw_rail(f, r, live, &items, &ticks, health, ctx);
    } else {
        if rule_foot > 0 {
            f.render_widget(Paragraph::new(view::rule("", None, area.width, t)), rule_a);
        }
        let mut lines = vec![learn];
        lines.extend(waiting);
        lamps.truncate(lamps_h as usize);
        lines.extend(lamps);
        if sav_h > 0 {
            lines.extend(savings);
        }
        f.render_widget(Paragraph::new(t.lines(lines)), foot_a);
    }

    if keys_h > 0 {
        let [r, k] = Layout::vertical([Constraint::Length(keys_h - 1), Constraint::Length(1)]).areas(keys_a);
        f.render_widget(Paragraph::new(view::rule("", None, area.width, t)), r);
        f.render_widget(Paragraph::new(view::footer(live, t, area.width)), k);
    }
    draw_popups(f, area, live, &items, &ticks, shown, ctx);
}

/// The open popup: the keys, the map, every conditioning run, or the inspector.
fn draw_popups(
    f: &mut Frame,
    area: Rect,
    live: &Live,
    items: &[&FeedItem],
    ticks: &[&Tick],
    shown: Option<u64>,
    ctx: &Ctx,
) {
    let t = &ctx.theme;
    if live.help {
        view::popup_keys(f, area, view::watch_help(t), t, live.popup_scroll);
    } else if live.map_hint {
        view::popup(f, area, "the map", view::map_hint_lines(t), t, live.popup_scroll);
    } else if live.runs {
        let w = area.width.saturating_sub(8);
        let mut lines: Vec<Line<'static>> = if ticks.is_empty() {
            vec![Line::from(vec![
                Span::styled("Nothing conditioned yet. Runs appear here as they finish", t.dim()),
                Span::styled(t.sep(), t.dim()),
                Span::styled("adage condition", t.command()),
            ])]
        } else {
            ticks.iter().map(|k| view::tick_line(k, ctx, w, &live.gists)).collect()
        };
        lines.extend(waiting_line(live, t, w));
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled("esc closes", t.dim())));
        // The newest at the bottom, in view.
        let hidden = (lines.len() as u16).saturating_sub(area.height.saturating_sub(4));
        view::popup(f, area, "conditioning runs", lines, t, live.popup_scroll.max(hidden));
    } else if live.inspect {
        // The newest event shown (under the scroll) that did something.
        let upto: Vec<&&FeedItem> = items.iter().filter(|i| shown.is_some_and(|s| i.seq <= s)).collect();
        match upto.iter().rev().find(|i| !i.quiet()).or(upto.last()) {
            Some(i) => view::popup(
                f,
                area,
                "inspect",
                view::inspect_lines(i, ctx, area.width.saturating_sub(8)),
                t,
                live.popup_scroll,
            ),
            None => view::popup(
                f,
                area,
                "inspect",
                vec![Line::from(Span::styled("Nothing to inspect yet.", t.dim()))],
                t,
                0,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::feed;
    use serde_json::json;

    const T0: u64 = 1_790_165_040_000;

    fn item(k: u64, session: &str, event: &str, kind: &str, d: serde_json::Value) -> FeedItem {
        let mut i = feed::from_record(
            session,
            &json!({ "ts": T0 + k * 60_000, "event": event, "kind": kind, "arm": "treated", "decision": d }),
        );
        i.seq = k + 1;
        i
    }

    #[test]
    fn stats_count_what_adage_did_and_never_the_control_arm() {
        let fire = json!({ "observed": ["exec ls", "read {path}"], "directives": [{ "route": "a", "strength": "fire", "text": "x" }, { "route": "b", "strength": "prime", "text": "y" }] });
        let mut control = item(
            3,
            "c",
            "PostToolBatch",
            "batch",
            json!({ "observed": ["exec ls"], "judgements": [{ "kind": "obligation_met", "route": "a" }] }),
        );
        control.control = true;
        let items = [
            item(0, "s", "UserPromptSubmit", "prompt", json!({})),
            item(1, "s", "PostToolBatch", "batch", fire),
            item(
                2,
                "s",
                "PostToolBatch",
                "batch",
                json!({ "observed": ["exec ls"], "judgements": [{ "kind": "followed", "route": "a" }, { "kind": "obligation_met", "route": "c" }] }),
            ),
            control,
            item(4, "s", "Stop", "stop", json!({ "continue_reason": "finish line" })),
        ];
        let refs: Vec<&FeedItem> = items.iter().collect();
        let s = Stats::of(&refs);
        assert_eq!((s.sessions, s.steps, s.calls), (2, 3, 4));
        assert_eq!((s.fired, s.strengths, s.followed, s.met, s.asked), (2, [1, 1, 0], 1, 1, 1));
        assert_eq!(s.follow_rate(), Some(0.5));
        assert_eq!((s.first, s.last), (Some(T0), Some(T0 + 4 * 60_000)));
        assert_eq!(Stats::default().follow_rate(), None);
        // Activity: one event a minute, the minute a reflex fired marked.
        let (counts, fired) = activity(&refs, T0 + 4 * 60_000, 6);
        assert_eq!(counts, vec![0, 1, 1, 1, 1, 1]);
        assert_eq!(fired, vec![false, false, true, false, false, false]);
        // In play: each reflex once, newest first, its follows counted.
        let play = in_play(&refs, &[("a".to_string(), "after x, run y".to_string())].into());
        assert_eq!(play.len(), 2);
        let a = play.iter().find(|p| p.route == "a").unwrap();
        assert_eq!((a.name.as_str(), a.fired, a.followed), ("after x, run y", 1, 1));
        assert_eq!(play.iter().find(|p| p.route == "b").unwrap().name, "y", "no gist: what it said");
    }

    #[test]
    fn the_last_run_and_its_changes_are_found() {
        let lines = [
            json!({ "ts": T0, "kind": "condition", "trigger": "auto", "repo": "r", "sessions_read": 3, "sessions_used": 2, "changes": [] }),
            json!({ "ts": T0 + 1, "kind": "condition", "trigger": "manual", "repo": "r", "sessions_read": 4, "sessions_used": 4, "changes": [
                { "route": "x", "track": "r/learned", "change": "strength", "from": "prime", "to": "fire", "summary": "4/4" } ] }),
            json!({ "ts": T0 + 2, "kind": "condition_error", "repo": "r", "class": "io", "error": "busy" }),
        ];
        let ticks: Vec<Tick> = lines.iter().flat_map(|l| learnlog::parse(&l.to_string())).collect();
        let refs: Vec<&Tick> = ticks.iter().collect();
        let l = learned(&refs);
        assert_eq!(l.runs.len(), 2);
        assert_eq!(l.last.map(|k| k.ts), Some(T0 + 1));
        assert_eq!(l.changes.len(), 1);
        assert!(l.error.is_some(), "a failure after the newest run is said");
        let t = Theme::with_color(false);
        let line = timeline(&l.runs, 20, &t).to_string();
        assert!(line.starts_with('◇') && line.contains("◆  2 runs"), "{line}");
    }
}
