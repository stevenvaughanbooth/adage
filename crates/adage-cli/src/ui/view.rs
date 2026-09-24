//! State shared by every screen, and the pure functions that draw it. Nothing here touches the
//! terminal: each `draw_*` renders state into a ratatui `Frame`, which tests drive with
//! `TestBackend`.

use super::feed::{self, Act, FeedItem};
use super::health::Health;
use super::learnlog::{self, Tick, Tone};
use super::tail::Record;
use super::theme::{Lamp, Theme};
use adage_core::repo;
use adage_store::Paths;
use anyhow::{bail, Context, Result};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::collections::VecDeque;
use std::path::PathBuf;

pub const NBSP: &str = "\u{a0}";

/// Feed items kept in memory.
pub const FEED_CAP: usize = 600;
/// Ticker lines kept in memory.
pub const TICK_CAP: usize = 200;

/// Where the screens look, and how they draw.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub paths: Paths,
    /// The repository the screens are about (`--repo`, else the current directory's).
    pub root: Option<PathBuf>,
    pub label: String,
    pub theme: Theme,
    /// Seconds east of UTC, for clock times.
    pub tz: i32,
    /// Replaying the bundled demo scenario.
    pub demo: bool,
}

impl Ctx {
    pub fn new(paths: Paths, repo_arg: Option<PathBuf>) -> Result<Ctx> {
        let start = match &repo_arg {
            Some(p) => p.canonicalize().with_context(|| format!("--repo {} does not exist", p.display()))?,
            None => std::env::current_dir()?,
        };
        let root = repo::find_root(&start);
        if repo_arg.is_some() && root.is_none() {
            bail!("{} is not inside a git repository", start.display());
        }
        Ok(Ctx::at(paths, root))
    }

    pub fn at(paths: Paths, root: Option<PathBuf>) -> Ctx {
        let label = root.as_deref().map(repo::dir_name).unwrap_or_else(|| "(no repository)".into());
        Ctx { paths, root, label, theme: Theme::from_env(), tz: local_offset(), demo: false }
    }
}

/// The local UTC offset now, in seconds (0, so UTC, if the system will not say).
pub fn local_offset() -> i32 {
    offset::local()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod offset {
    pub fn local() -> i32 {
        // SAFETY: `time` with a null pointer only returns the time; `localtime_r` is the reentrant
        // form, which reads `now` and writes only into `tm`, both of which outlive the call.
        let now = unsafe { libc::time(std::ptr::null_mut()) };
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let ok = unsafe { !libc::localtime_r(&now, &mut tm).is_null() };
        if ok {
            tm.tm_gmtoff as i32
        } else {
            0
        }
    }
}

#[cfg(windows)]
mod offset {
    use winapi::um::timezoneapi::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};

    pub fn local() -> i32 {
        // SAFETY: the call fills the zeroed struct it is given, and nothing else.
        let mut tz: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
        let which = unsafe { GetTimeZoneInformation(&mut tz) };
        // UTC = local time + bias (minutes); daylight (2) and standard (1) time add their own.
        let bias = match which {
            1 => tz.Bias + tz.StandardBias,
            2 => tz.Bias + tz.DaylightBias,
            0 => tz.Bias,
            _ => return 0,
        };
        -bias * 60
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod offset {
    pub fn local() -> i32 {
        0
    }
}

/// A clock time from epoch milliseconds: `HH:MM:SS`, or `HH:MM`.
pub fn clock(ms: u64, tz: i32, secs: bool) -> String {
    let t = (ms / 1000) as i64 + tz as i64;
    let d = t.rem_euclid(86_400);
    if secs {
        format!("{:02}:{:02}:{:02}", d / 3600, d / 60 % 60, d % 60)
    } else {
        format!("{:02}:{:02}", d / 3600, d / 60 % 60)
    }
}

/// The live feed and the learning ticker.
#[derive(Debug, Default)]
pub struct Live {
    pub feed: VecDeque<FeedItem>,
    /// Items that arrived while paused, shown on resume.
    pub held: Vec<FeedItem>,
    pub ticks: VecDeque<Tick>,
    pub paused: bool,
    /// Show every repository's activity, not only this one's.
    pub all_repos: bool,
    /// Hide events where Adage said and judged nothing.
    pub hide_quiet: bool,
    /// Lines scrolled back from the newest (0 follows the feed).
    pub scroll: usize,
    pub help: bool,
}

impl Live {
    pub fn push_records(&mut self, recs: Vec<Record>) {
        for r in recs {
            let item = feed::from_record(&r.session, &r.value);
            if self.paused {
                self.held.push(item);
            } else {
                self.feed.push_back(item);
            }
        }
        while self.feed.len() > FEED_CAP {
            self.feed.pop_front();
        }
        if self.held.len() > FEED_CAP {
            let extra = self.held.len() - FEED_CAP;
            self.held.drain(..extra);
        }
    }

    pub fn push_log(&mut self, lines: Vec<String>) {
        for l in lines {
            self.ticks.extend(learnlog::parse(&l));
        }
        while self.ticks.len() > TICK_CAP {
            self.ticks.pop_front();
        }
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if !self.paused {
            self.feed.extend(self.held.drain(..));
            while self.feed.len() > FEED_CAP {
                self.feed.pop_front();
            }
        }
    }

    /// The items shown, oldest first.
    pub fn visible<'a>(&'a self, repo_key: Option<&'a str>) -> Vec<&'a FeedItem> {
        self.feed
            .iter()
            .filter(|i| self.all_repos || repo_key.is_none_or(|k| i.repo == k))
            .filter(|i| !self.hide_quiet || !i.quiet())
            .collect()
    }

    pub fn visible_ticks<'a>(&'a self, repo_key: Option<&'a str>) -> Vec<&'a Tick> {
        self.ticks
            .iter()
            .filter(|t| self.all_repos || repo_key.is_none_or(|k| t.repo.as_deref().is_none_or(|r| r == k)))
            .collect()
    }

    /// Keys the feed understands; `true` if the key was used.
    pub fn key(&mut self, c: char) -> bool {
        match c {
            'p' | ' ' => self.toggle_pause(),
            'f' => self.all_repos = !self.all_repos,
            'v' => self.hide_quiet = !self.hide_quiet,
            'k' => self.scroll = self.scroll.saturating_add(1),
            'j' => self.scroll = self.scroll.saturating_sub(1),
            'G' => self.scroll = 0,
            _ => return false,
        }
        true
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// What Adage did, as spans; `narrow` drops the verbs the marks already carry.
fn act_spans(a: &Act, t: &Theme, narrow: bool) -> Vec<Span<'static>> {
    let aspect_note = |asp| if t.color { String::new() } else { format!(" {}", t.aspect_mark(asp)) };
    let verb = |v: &str| if narrow { String::new() } else { format!("{v} ") };
    match a {
        Act::Directed { route, aspect } => {
            vec![Span::styled(format!("→ {route}{}", aspect_note(*aspect)), t.aspect(*aspect).patch(t.bold()))]
        }
        Act::Withheld { route, aspect } => {
            vec![Span::styled(format!("withheld {route}{}", aspect_note(*aspect)), t.holdout())]
        }
        Act::Followed(r) => vec![Span::styled(format!("✓ {}{r}", verb("followed")), t.green())],
        Act::Derailed(r) => vec![Span::styled(format!("✗ {}{r}", verb("derailed")), t.red())],
        Act::Met(r) => vec![Span::styled(format!("✓ met {r}"), t.green())],
        Act::Unaided(r) => vec![Span::styled(format!("✓ {}{r}", verb("unaided")), t.holdout())],
        Act::NotReached(r) => vec![Span::styled(format!("· {}{r}", verb("missed")), t.holdout())],
        Act::StopRequest => vec![Span::styled("■ stop-request".to_string(), t.yellow().patch(t.bold()))],
        Act::StopWithheld => vec![Span::styled("withheld stop-request".to_string(), t.holdout())],
    }
}

/// One feed line.
pub fn feed_line(i: &FeedItem, ctx: &Ctx, width: u16) -> Line<'static> {
    let t = &ctx.theme;
    let wide = width >= 90;
    let mut spans = vec![Span::styled(clock(i.ts, ctx.tz, true), t.dim()), Span::raw(" ")];
    let (mark, style) = if i.holdout { ("◌", t.holdout()) } else { ("●", t.accent()) };
    spans.push(Span::styled(format!("{mark}{}", feed::short_session(&i.session, 6)), style));
    spans.push(Span::raw(" "));
    let h = if wide { format!("{:<8}", i.harness) } else { feed::harness_short(&i.harness).to_string() };
    spans.push(Span::styled(h, t.dim()));
    spans.push(Span::raw(" "));
    spans.push(Span::raw(format!("{:<7} ", i.event)));
    if i.acts.is_empty() {
        let n = i.observed;
        let what = match n {
            0 => "· nothing to say".to_string(),
            1 => "· 1 call, nothing to say".to_string(),
            n => format!("· {n} calls, nothing to say"),
        };
        spans.push(Span::styled(what, t.dim()));
    } else {
        for (k, a) in i.acts.iter().enumerate() {
            if k > 0 {
                spans.push(Span::raw("  "));
            }
            spans.extend(act_spans(a, t, width < 72));
        }
    }
    if wide && i.holdout {
        spans.push(Span::styled("  (holdout)", t.holdout()));
    }
    Line::from(spans)
}

/// One ticker line; below 100 columns without the track's name, which the pane's title gives.
pub fn tick_line(k: &Tick, ctx: &Ctx, width: u16) -> Line<'static> {
    let t = &ctx.theme;
    let style = match (k.tone, k.aspect) {
        (Tone::Error, _) => t.red(),
        (Tone::Change, Some(a)) => t.aspect(a),
        (Tone::Change, None) => Style::default(),
        (Tone::Run, _) => t.dim(),
    };
    let lead = if k.tone == Tone::Change { "  " } else { "" };
    Line::from(vec![
        Span::styled(clock(k.ts, ctx.tz, false), t.dim()),
        Span::raw(" "),
        Span::styled(format!("{lead}{}", if width >= 100 { &k.text } else { &k.short }), style),
    ])
}

/// The health lamps on one (wrapping) line.
pub fn lights_line(h: Option<&Health>, t: &Theme) -> Line<'static> {
    let Some(h) = h else { return Line::from(Span::styled("checking health…", t.dim())) };
    let mut spans = Vec::new();
    for (i, l) in h.lights.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        // Non-breaking spaces keep each lamp and its label together when the line wraps.
        spans.push(Span::styled(t.lamp_mark(l.lamp).to_string(), t.lamp(l.lamp)));
        spans.push(Span::styled(
            format!("{NBSP}{}", l.label.replace(' ', NBSP)),
            if l.lamp == Lamp::Off { t.dim() } else { Style::default() },
        ));
    }
    Line::from(spans)
}

pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

/// A bordered popup over `area` with `lines`.
pub fn popup(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>, t: &Theme, scroll: u16) {
    let w = lines
        .iter()
        .map(|l| l.width() as u16)
        .max()
        .unwrap_or(20)
        .saturating_add(4)
        .clamp(30, area.width.saturating_sub(4).max(10));
    let h = (lines.len() as u16).saturating_add(2).min(area.height.saturating_sub(2).max(3));
    let r = centered(area, w, h);
    f.render_widget(Clear, r);
    let block = Block::default().borders(Borders::ALL).title(Span::styled(format!(" {title} "), t.title()));
    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }).scroll((scroll, 0)), r);
}

pub fn watch_help(t: &Theme) -> Vec<Line<'static>> {
    let k = |key: &str, what: &str| {
        Line::from(vec![Span::styled(format!("{key:>7}  "), t.bold()), Span::raw(what.to_string())])
    };
    vec![
        k("q", "quit"),
        k("p", "pause / resume the feed"),
        k("f", "this repository / all repositories"),
        k("v", "hide / show events where Adage said nothing"),
        k("↑ ↓", "scroll the feed (G: back to newest)"),
        k("?", "this help"),
        Line::raw(""),
        Line::from(Span::styled("● treated session   ◌ holdout (nothing delivered)", t.dim())),
        Line::from(vec![
            Span::styled("→ route", t.green()),
            Span::raw(" directive (colour = aspect)  "),
            Span::styled("✓", t.green()),
            Span::raw(" followed  "),
            Span::styled("✗", t.red()),
            Span::raw(" derailed"),
        ]),
    ]
}

/// The feed, newest at the bottom, in `area` (no border).
fn draw_feed(f: &mut Frame, area: Rect, live: &Live, key: Option<&str>, ctx: &Ctx) {
    let t = &ctx.theme;
    let items = live.visible(key);
    if items.is_empty() {
        let scope = if live.all_repos || key.is_none() { "any repository".to_string() } else { ctx.label.clone() };
        let lines = vec![
            Line::from(Span::styled(format!("No hook activity yet for {scope}."), t.dim())),
            Line::from(Span::styled(
                "Start an agent session there: each hook call appears here as it happens.".to_string(),
                t.dim(),
            )),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
        return;
    }
    let h = area.height as usize;
    let max_scroll = items.len().saturating_sub(h);
    let scroll = live.scroll.min(max_scroll);
    let end = items.len() - scroll;
    let start = end.saturating_sub(h);
    let lines: Vec<Line> = items[start..end].iter().map(|i| feed_line(i, ctx, area.width)).collect();
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_ticker(f: &mut Frame, area: Rect, live: &Live, key: Option<&str>, ctx: &Ctx) {
    let t = &ctx.theme;
    let ticks = live.visible_ticks(key);
    if ticks.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Nothing learned yet. Learning runs appear here as they finish.",
                t.dim(),
            )))
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let h = area.height as usize;
    let lines: Vec<Line> =
        ticks[ticks.len().saturating_sub(h)..].iter().map(|k| tick_line(k, ctx, area.width)).collect();
    f.render_widget(Paragraph::new(lines), area);
}

fn section(title: String, t: &Theme) -> Block<'static> {
    Block::default().borders(Borders::TOP).border_style(t.dim()).title(Span::styled(title, t.title()))
}

/// The live view: lamps, feed and ticker. `chrome` adds the watch pane's own header and key line
/// (the full app has its own tab bar and status bar).
pub fn draw_live(f: &mut Frame, area: Rect, live: &Live, health: Option<&Health>, ctx: &Ctx, chrome: bool) {
    let t = &ctx.theme;
    if area.width < 30 || area.height < 8 {
        f.render_widget(
            Paragraph::new("adage watch: make this pane larger (30×8 at least)").wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let key = health.and_then(|h| h.repo_key.as_deref());
    let lights = lights_line(health, t);
    let lights_h = ((lights.width() as u16).div_ceil(area.width.max(1))).clamp(1, 3);
    let ticker_h = match area.height {
        0..=14 => 3,
        15..=30 => 4,
        _ => 8,
    };
    let (header_h, keys_h) = if chrome { (1, 1) } else { (0, 0) };
    let [header, lamps, feed_area, ticker, keys] = Layout::vertical([
        Constraint::Length(header_h),
        Constraint::Length(lights_h),
        Constraint::Min(3),
        Constraint::Length(ticker_h),
        Constraint::Length(keys_h),
    ])
    .areas(area);

    if chrome {
        let gen = health.and_then(|h| h.generation.as_deref()).map(|g| format!(" · gen {}", &g[..g.len().min(8)]));
        let left = Line::from(vec![
            Span::styled(" adage watch", t.title()),
            Span::styled(format!(" · {}{}", ctx.label, gen.unwrap_or_default()), t.dim()),
            Span::styled(if ctx.demo { " · demo" } else { "" }.to_string(), t.holdout()),
        ]);
        let state = if live.paused {
            Span::styled(format!("PAUSED +{} ", live.held.len()), t.yellow().patch(t.bold()))
        } else if live.scroll > 0 {
            Span::styled(format!("SCROLLED -{} ", live.scroll), t.yellow())
        } else {
            Span::styled("LIVE ", t.green().patch(t.bold()))
        };
        let [l, r] = Layout::horizontal([Constraint::Min(10), Constraint::Length(state.width() as u16)]).areas(header);
        f.render_widget(Paragraph::new(left), l);
        f.render_widget(Paragraph::new(Line::from(state)), r);
    }
    f.render_widget(Paragraph::new(lights).wrap(Wrap { trim: true }), lamps);

    let scope = if live.all_repos || key.is_none() { "all repositories".to_string() } else { ctx.label.clone() };
    let feed_block = section(format!(" activity · {scope} "), t);
    let inner = feed_block.inner(feed_area);
    f.render_widget(feed_block, feed_area);
    draw_feed(f, inner, live, key, ctx);

    let tick_block = section(" learning ".to_string(), t);
    let inner = tick_block.inner(ticker);
    f.render_widget(tick_block, ticker);
    draw_ticker(f, inner, live, key, ctx);

    if chrome {
        let pause = if live.paused { "resume" } else { "pause" };
        let scope = if live.all_repos { "this repo" } else { "all repos" };
        let quiet = if live.hide_quiet { "show quiet" } else { "hide quiet" };
        let hint = format!(" q quit · p {pause} · f {scope} · v {quiet} · ? help");
        f.render_widget(Paragraph::new(Span::styled(hint, t.dim())), keys);
    }
    if live.help {
        popup(f, area, "adage watch", watch_help(t), t, 0);
    }
}
