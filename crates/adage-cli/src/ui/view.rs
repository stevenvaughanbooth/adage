//! State shared by every screen, and the pure functions that draw it. Nothing here touches the
//! terminal: each `draw_*` renders state into a ratatui `Frame`, which tests drive with
//! `TestBackend`.
//!
//! The live view (`adage watch`, and the Live tab of `adage ui`) has three sections under thin
//! rules: **now** (each hook call, newest at the bottom), **conditioning** (what each run
//! changed) and **health** (lamps, each with the command that fixes it).

use super::feed::{self, Act, FeedItem};
use super::health::Health;
use super::keys;
use super::learnlog::{self, Tick, Tone};
use super::tail::Record;
use super::theme::{Lamp, Theme};
use super::words::{fit, pad};
use adage_core::repo;
use adage_store::Paths;
use adage_term::Glyph;
use adage_tracks::Aspect;
use anyhow::{bail, Context, Result};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

pub const NBSP: &str = "\u{a0}";

/// Feed items kept in memory.
pub const FEED_CAP: usize = 600;
/// Ticker lines kept in memory.
pub const TICK_CAP: usize = 200;
/// How long a firing reflex's row pulses.
pub const PULSE_MS: u128 = 900;

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

/// The newest firing's one-shot pulse: which item, and how bright (3 down to 0).
#[derive(Debug, Clone, Copy, Default)]
pub struct Pulse {
    pub seq: u64,
    pub level: u8,
    pub at: Option<Instant>,
}

/// The live feed and the conditioning ticker.
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
    /// Repositories a background conditioner is working on now.
    pub learning: Vec<String>,
    /// The `/` filter, and whether it is being typed.
    pub filter: String,
    pub filtering: bool,
    /// The inspector popup is open.
    pub inspect: bool,
    /// The "open the map" popup is open.
    pub map_hint: bool,
    /// Lines scrolled in the open popup.
    pub popup_scroll: u16,
    /// Reflexes waiting for review (nascent, or flagged), once known.
    pub waiting: Option<usize>,
    pub pulse: Pulse,
    next_seq: u64,
    primed: bool,
}

impl Live {
    pub fn push_records(&mut self, recs: Vec<Record>) {
        let mut fired = None;
        for r in recs {
            let mut item = feed::from_record(&r.session, &r.value);
            self.next_seq += 1;
            item.seq = self.next_seq;
            if item.fired() && !item.holdout {
                fired = Some(item.seq);
            }
            if self.paused {
                self.held.push(item);
            } else {
                self.feed.push_back(item);
            }
        }
        // The history read at the start does not pulse; what arrives after it does.
        if let (Some(seq), true, false) = (fired, self.primed, self.paused) {
            self.pulse = Pulse { seq, level: 3, at: Some(Instant::now()) };
        }
        self.primed = true;
        while self.feed.len() > FEED_CAP {
            self.feed.pop_front();
        }
        if self.held.len() > FEED_CAP {
            let extra = self.held.len() - FEED_CAP;
            self.held.drain(..extra);
        }
    }

    /// Dim the pulse with time; `motion` false (reduced motion) turns it off.
    pub fn tick_pulse(&mut self, now: Instant, motion: bool) {
        let Some(at) = self.pulse.at else { return };
        let ms = now.saturating_duration_since(at).as_millis();
        self.pulse.level = match ms {
            _ if !motion => 0,
            0..300 => 3,
            300..600 => 2,
            600..PULSE_MS => 1,
            _ => 0,
        };
        if self.pulse.level == 0 {
            self.pulse.at = None;
        }
    }

    /// Whether a pulse is running (the screen then redraws faster).
    pub fn pulsing(&self) -> bool {
        self.pulse.at.is_some()
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
        let needle = self.filter.to_lowercase();
        self.feed
            .iter()
            .filter(|i| self.all_repos || repo_key.is_none_or(|k| i.repo == k))
            .filter(|i| !self.hide_quiet || !i.quiet())
            .filter(|i| needle.is_empty() || i.haystack().to_lowercase().contains(&needle))
            .collect()
    }

    pub fn visible_ticks<'a>(&'a self, repo_key: Option<&'a str>) -> Vec<&'a Tick> {
        self.ticks
            .iter()
            .filter(|t| self.all_repos || repo_key.is_none_or(|k| t.repo.as_deref().is_none_or(|r| r == k)))
            .collect()
    }

    /// Firings among the items shown, by strength: (fire, prime, inhibit).
    pub fn counts(&self, repo_key: Option<&str>) -> (usize, usize, usize) {
        let mut c = (0, 0, 0);
        for i in self.visible(repo_key) {
            for a in &i.acts {
                if let Act::Directed { aspect, .. } = a {
                    match aspect {
                        Aspect::Green => c.0 += 1,
                        Aspect::Yellow => c.1 += 1,
                        Aspect::Red => c.2 += 1,
                    }
                }
            }
        }
        c
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
            '/' => self.filtering = true,
            'm' => self.map_hint = true,
            _ => return false,
        }
        true
    }

    /// The item the inspector opens: the newest shown (under the scroll) that did something.
    pub fn current<'a>(&'a self, repo_key: Option<&'a str>, ctx: &Ctx, width: u16) -> Option<&'a FeedItem> {
        let items = self.visible(repo_key);
        let lines = feed_lines(&items, ctx, width, &self.pulse);
        let end = lines.len().saturating_sub(self.scroll.min(lines.len()));
        let last_seq = lines.get(end.checked_sub(1)?).map(|(s, _)| *s)?;
        let upto: Vec<&FeedItem> = items.into_iter().filter(|i| i.seq <= last_seq).collect();
        upto.iter().rev().find(|i| !i.quiet()).or(upto.last()).copied()
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing: pieces

/// A thin rule with a label: `─ now ─────────`.
pub fn rule(label: &str, note: Option<Span<'static>>, width: u16, t: &Theme) -> Line<'static> {
    let dash = t.ch("─", "-");
    if label.is_empty() {
        return Line::from(Span::styled(dash.repeat(width as usize), t.rule()));
    }
    let mut spans =
        vec![Span::styled(format!("{dash} "), t.rule()), Span::styled(label.to_string(), t.ink2().patch(t.bold()))];
    if let Some(n) = note {
        spans.push(Span::styled(" · ", t.dim()));
        spans.push(n);
    }
    let used: usize = spans.iter().map(|s| s.width()).sum();
    let rest = (width as usize).saturating_sub(used + 1);
    spans.push(Span::styled(format!(" {}", dash.repeat(rest)), t.rule()));
    Line::from(spans)
}

/// The lamp and word of a strength, padded to one column width: `● fire     ` / `FIRE       `.
fn strength_cell(a: Aspect, t: &Theme, w: usize, bold: bool) -> Vec<Span<'static>> {
    let style = if bold { t.aspect(a).patch(t.bold()) } else { t.aspect(a) };
    if t.color {
        vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.aspect(a)),
            Span::styled(pad(Theme::strength_word(a), w.saturating_sub(2)), style),
        ]
    } else {
        vec![Span::styled(pad(&Theme::strength_word(a).to_ascii_uppercase(), w), style)]
    }
}

/// How wide the feed is laid out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fit {
    /// ≥ 90 columns: every column, ids padded, the directive's command.
    Wide,
    /// 72–89: short harness names.
    Mid,
    /// < 72: no command, no verbs the glyphs already carry.
    Narrow,
}

fn fit_for(width: u16) -> Fit {
    match width {
        90.. => Fit::Wide,
        72..90 => Fit::Mid,
        _ => Fit::Narrow,
    }
}

/// One act, as spans, within `room` columns.
fn act_spans(a: &Act, t: &Theme, fit_: Fit, room: usize, hot: u8) -> Vec<Span<'static>> {
    let narrow = fit_ == Fit::Narrow;
    let verb = |v: &str| if narrow { String::new() } else { format!("{v} ") };
    let off = t.glyph(Glyph::Off);
    let mut spans = match a {
        Act::Directed { route, aspect, detail, instinct } => {
            let glyph = if *instinct { Glyph::Instinct } else { Glyph::Reflex };
            let id_w = match fit_ {
                Fit::Wide => 25,
                Fit::Mid => 22,
                Fit::Narrow => route.chars().count() + 1,
            };
            let id_style = match hot {
                0 => Style::default(),
                1 => t.bold(),
                _ => t.accent().patch(t.bold()),
            };
            let mut v = vec![
                Span::styled(format!("{} ", t.glyph(Glyph::Arrow)), t.accent()),
                Span::styled(format!("{} ", t.glyph(glyph)), t.accent()),
                Span::styled(pad(route, id_w), id_style),
            ];
            let cell_w = if narrow { Theme::strength_word(*aspect).len() + 3 } else { 11 };
            let mut cell = strength_cell(*aspect, t, cell_w, hot > 0);
            if hot == 3 {
                if let Some(first) = cell.first_mut() {
                    first.style = first.style.add_modifier(Modifier::REVERSED);
                }
            }
            v.extend(cell);
            if !narrow && !detail.is_empty() {
                let used: usize = v.iter().map(|s| s.width()).sum();
                v.push(Span::styled(fit(detail, room.saturating_sub(used)), t.ink2()));
            }
            v
        }
        Act::Withheld { route, aspect } => {
            vec![Span::styled(format!("{off} would {} {route} (withheld)", Theme::strength_word(*aspect)), t.holdout())]
        }
        Act::Followed(r) => vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Ok)), t.green()),
            Span::styled(verb("followed"), t.ink2()),
            Span::raw(r.clone()),
        ],
        Act::Derailed(r) => vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Fail)), t.red()),
            Span::styled(verb("ignored"), t.ink2()),
            Span::raw(r.clone()),
        ],
        Act::Met(r) => vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Ok)), t.green()),
            Span::styled("finish line met ".to_string(), t.ink2()),
            Span::raw(r.clone()),
        ],
        Act::Unaided(r) => vec![Span::styled(format!("{off} found {r} unaided"), t.holdout())],
        Act::NotReached(r) => vec![Span::styled(format!("{off} missed {r}"), t.holdout())],
        Act::StopRequest => vec![
            Span::styled(format!("{} ", t.ch("■", "#")), t.yellow().patch(t.bold())),
            Span::raw("finish line: asked the agent to continue".to_string()),
        ],
        Act::StopWithheld => {
            vec![Span::styled(format!("{off} would ask the agent to continue (withheld)"), t.holdout())]
        }
    };
    // Whatever is left over is cut to the room there is (padding goes first, without an
    // ellipsis).
    let mut used = 0;
    spans.retain_mut(|s| {
        let w = s.width();
        if used >= room {
            return false;
        }
        if used + w > room {
            let trimmed = s.content.trim_end().to_string();
            s.content =
                if trimmed.chars().count() <= room - used { trimmed } else { fit(&trimmed, room - used) }.into();
        }
        used += w;
        true
    });
    spans
}

/// Each item as lines (one per act), tagged with the item's `seq`.
pub fn feed_lines(items: &[&FeedItem], ctx: &Ctx, width: u16, pulse: &Pulse) -> Vec<(u64, Line<'static>)> {
    let t = &ctx.theme;
    let f = fit_for(width);
    let mut out = Vec::new();
    for i in items {
        let dimmed = i.holdout;
        let st = |s: Style| if dimmed { t.holdout() } else { s };
        let (sid_n, harness) = match f {
            Fit::Wide => (6, pad(feed::harness_name(&i.harness), 9)),
            Fit::Mid => (6, pad(feed::harness_short(&i.harness), 3)),
            Fit::Narrow => (4, pad(feed::harness_short(&i.harness), 3)),
        };
        let gap = if f == Fit::Wide { "  " } else { " " };
        let hot = if pulse.seq == i.seq { pulse.level } else { 0 };
        let lead = if hot > 0 { Span::styled(t.ch("▌", "|"), t.accent()) } else { Span::raw(" ") };
        let prefix = vec![
            lead,
            Span::styled(clock(i.ts, ctx.tz, true), t.dim()),
            Span::raw(gap),
            Span::styled(feed::short_session(&i.session, sid_n), st(t.ink2())),
            Span::raw(gap),
            Span::styled(harness, st(t.ink2())),
            Span::raw(gap),
            Span::styled(pad(&i.event, 8), st(Style::default())),
            Span::raw(" "),
        ];
        let indent: usize = prefix.iter().map(|s| s.width()).sum();
        let room = (width as usize).saturating_sub(indent);
        if i.acts.is_empty() {
            let what = match i.observed {
                0 => "· nothing to say".to_string(),
                1 => "· 1 call, nothing to say".to_string(),
                n => format!("· {n} calls, nothing to say"),
            };
            let mut spans = prefix;
            spans.push(Span::styled(fit(&what, room), t.dim()));
            out.push((i.seq, Line::from(spans)));
            continue;
        }
        for (k, a) in i.acts.iter().enumerate() {
            let mut spans = if k == 0 { prefix.clone() } else { vec![Span::raw(" ".repeat(indent))] };
            let act_hot = if matches!(a, Act::Directed { .. }) { hot } else { 0 };
            spans.extend(act_spans(a, t, f, room, act_hot));
            out.push((i.seq, Line::from(spans)));
        }
    }
    out
}

/// One conditioning line.
pub fn tick_line(k: &Tick, ctx: &Ctx, width: u16) -> Line<'static> {
    let t = &ctx.theme;
    let wide = width >= 90;
    match (k.tone, &k.change, &k.run) {
        (Tone::Error, _, _) => Line::from(vec![
            Span::styled(format!(" {} ", t.glyph(Glyph::Fail)), t.red()),
            Span::styled(format!("{} ", clock(k.ts, ctx.tz, false)), t.dim()),
            Span::styled(fit(if wide { &k.text } else { &k.short }, (width as usize).saturating_sub(9)), t.red()),
        ]),
        (Tone::Change, Some(c), _) => {
            let id_w = if wide { 26 } else { 20 };
            let glyph = if c.what == "written" { t.glyph(Glyph::Done) } else { t.glyph(Glyph::Reflex) };
            let mut spans = vec![
                Span::raw("   "),
                Span::styled(format!("{glyph} "), t.accent()),
                Span::styled(pad(&c.route, id_w), t.bold()),
            ];
            let word_style = |w: &str| match w {
                "fire" | "firm" => t.green().patch(t.bold()),
                "prime" | "weakened" => t.yellow().patch(t.bold()),
                "inhibit" => t.red().patch(t.bold()),
                "forming" => t.accent().patch(t.bold()),
                _ => t.dim(),
            };
            match c.what {
                "evidence" => spans.push(Span::styled(pad("evidence", 19), t.ink2())),
                "proposal" => spans.push(Span::styled(pad("change proposed", 19), t.ink2())),
                "new" => {
                    spans.push(Span::styled("new · ", t.ink2()));
                    let to = c.to.clone().unwrap_or_default();
                    spans.push(Span::styled(pad(&to, 13), word_style(&to)));
                }
                "written" => spans.push(Span::styled(pad(&c.to.clone().unwrap_or_default(), 19), t.ink2())),
                _ => {
                    let from = c.from.as_deref().map(|f| format!("{f} {} ", t.glyph(Glyph::Arrow))).unwrap_or_default();
                    let to = c.to.clone().unwrap_or_default();
                    let fw = from.chars().count();
                    spans.push(Span::styled(from, t.ink2()));
                    spans.push(Span::styled(
                        pad(&to, 19usize.saturating_sub(fw).max(to.chars().count() + 1)),
                        word_style(&to),
                    ));
                }
            }
            if !c.numbers.is_empty() {
                let used: usize = spans.iter().map(|s| s.width()).sum();
                spans.push(Span::styled(fit(&c.numbers, (width as usize).saturating_sub(used)), t.ink2()));
            }
            if wide && !c.track.is_empty() && c.what != "written" {
                let used: usize = spans.iter().map(|s| s.width()).sum();
                let room = (width as usize).saturating_sub(used + 4);
                if room > 8 {
                    spans.push(Span::styled(format!("   {}", fit(&c.track, room)), t.dim()));
                }
            }
            Line::from(spans)
        }
        (_, _, run) => {
            let head = run.as_ref().map_or("condition", |r| if r.auto { "auto-condition" } else { "condition" });
            let mut facts = Vec::new();
            if let Some(r) = run {
                if let (Some(read), Some(used)) = (r.read, r.used) {
                    facts.push(format!("{used}/{read} traces used"));
                }
                if let Some(ms) = r.duration_ms {
                    facts.push(learnlog::duration(ms));
                }
                match r.changes {
                    Some(0) => facts.push("no reflex changed".into()),
                    Some(1) => facts.push("1 change".into()),
                    Some(n) => facts.push(format!("{n} changes")),
                    None => {}
                }
                if wide {
                    if let Some(g) = &r.generation {
                        facts.push(format!("gen {g}"));
                    }
                }
            }
            let repo = match (&k.repo, wide) {
                (Some(r), true) => format!(" {r}"),
                _ => String::new(),
            };
            let tail = if facts.is_empty() { String::new() } else { format!(" · {}", facts.join(" · ")) };
            let lead = format!(" {} {} ", t.glyph(Glyph::Active), clock(k.ts, ctx.tz, false));
            let room = (width as usize).saturating_sub(lead.chars().count() + head.len() + repo.len());
            Line::from(vec![
                Span::styled(format!(" {} ", t.glyph(Glyph::Active)), t.accent()),
                Span::styled(format!("{} ", clock(k.ts, ctx.tz, false)), t.dim()),
                Span::raw(format!("{head}{repo}")),
                Span::styled(fit(&tail, room), t.ink2()),
            ])
        }
    }
}

/// The health lamps, each with its fixing command, wrapped to `width` (a lamp never splits).
pub fn lights_lines(h: Option<&Health>, t: &Theme, width: u16) -> Vec<Line<'static>> {
    let Some(h) = h else { return vec![Line::from(Span::styled(" checking health…", t.dim()))] };
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut cur: Vec<Span<'static>> = vec![Span::raw(" ")];
    for l in &h.lights {
        let mut one = vec![
            Span::styled(format!("{} ", t.lamp_mark(l.lamp)), t.lamp(l.lamp)),
            Span::styled(l.label.clone(), t.ink2()),
        ];
        if let Some(cmd) = &l.hint {
            one.push(Span::styled(" · ", t.dim()));
            one.push(Span::styled(cmd.clone(), t.command()));
        }
        let used: usize = cur.iter().map(|s| s.width()).sum();
        let w: usize = one.iter().map(|s| s.width()).sum();
        if cur.len() > 1 && used + 4 + w > width as usize {
            out.push(Line::from(std::mem::take(&mut cur)));
            cur.push(Span::raw(" "));
        } else if cur.len() > 1 {
            cur.push(Span::raw("    "));
        }
        cur.extend(one);
    }
    out.push(Line::from(cur));
    out
}

/// Word-wrap `lines` to `width` columns. Unlike ratatui's own wrapping, a continuation keeps its
/// line's indent (its leading spaces), so wrapped text stays aligned under the text it continues.
/// A word longer than the room is broken at the edge.
pub fn wrap_lines(lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    let width = width as usize;
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        if width < 2 || line.width() <= width {
            out.push(line);
            continue;
        }
        let base = line.style;
        let cells: Vec<(char, Style)> = line
            .spans
            .iter()
            .flat_map(|s| {
                let st = base.patch(s.style);
                s.content.chars().map(move |c| (c, st))
            })
            .collect();
        let indent = cells.iter().take_while(|(c, _)| *c == ' ').count().min(width / 2);
        let mut rows: Vec<Vec<(char, Style)>> = vec![];
        let mut cur: Vec<(char, Style)> = vec![];
        let mut i = 0;
        while i < cells.len() {
            // A token: the spaces before a word, then the word.
            let mut j = i;
            while j < cells.len() && cells[j].0 == ' ' {
                j += 1;
            }
            let word_at = j;
            while j < cells.len() && cells[j].0 != ' ' {
                j += 1;
            }
            let mut token = &cells[i..j];
            i = j;
            if cur.len() + token.len() > width && cur.len() > indent {
                if word_at == j {
                    continue; // trailing spaces at the edge
                }
                rows.push(std::mem::take(&mut cur));
                cur = vec![(' ', Style::default()); indent];
                token = &cells[word_at..j];
            }
            for &c in token {
                if cur.len() >= width {
                    rows.push(std::mem::take(&mut cur));
                    cur = vec![(' ', Style::default()); indent];
                }
                cur.push(c);
            }
        }
        rows.push(cur);
        out.extend(rows.into_iter().map(|row| {
            let mut spans: Vec<Span<'static>> = vec![];
            let mut text = String::new();
            let mut style = row.first().map_or(Style::default(), |c| c.1);
            for (c, st) in row {
                if st != style && !text.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut text), style));
                }
                style = st;
                text.push(c);
            }
            spans.push(Span::styled(text, style));
            Line::from(spans)
        }));
    }
    out
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
    // Wrapped before sizing, so the last line (often the keys that answer the popup) is counted.
    let lines = wrap_lines(lines, w.saturating_sub(4));
    let h = (lines.len() as u16).saturating_add(2).min(area.height.saturating_sub(2).max(3));
    let r = centered(area, w, h);
    f.render_widget(Clear, r);
    let hidden = (lines.len() as u16).saturating_sub(h.saturating_sub(2));
    let scroll = scroll.min(hidden);
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(if t.ch("╭", "+") == "╭" {
            ratatui::widgets::BorderType::Rounded
        } else {
            ratatui::widgets::BorderType::Plain
        })
        .border_style(t.accent())
        .title(Span::styled(format!(" {title} "), t.command().patch(t.bold())));
    if hidden > 0 {
        // More than fits: say so, and which keys reach it.
        let more = match (scroll > 0, scroll < hidden) {
            (true, true) => format!(" {} more · j/k scroll ", t.ch("↑↓", "^v")),
            (false, _) => format!(" {} more · j/k scroll ", t.ch("↓", "v")),
            (true, false) => format!(" {} more · j/k scroll ", t.ch("↑", "^")),
        };
        block = block.title_bottom(Line::from(Span::styled(more, t.dim())).right_aligned());
    }
    let inner = block.inner(r).inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 });
    f.render_widget(block, r);
    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

/// The watch pane's `?` overlay: its keys, from the keymap, and what the marks mean.
pub fn watch_help(t: &Theme) -> Vec<Line<'static>> {
    let mut v = keys::overlay(&[("adage watch", keys::WATCH)], t);
    v.push(Line::raw(""));
    v.extend(legend(t));
    v
}

/// What the feed's marks mean.
pub fn legend(t: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled("MARKS", t.dim().patch(t.bold()))),
        Line::from(vec![
            Span::styled(format!("  {} {} ", t.glyph(Glyph::Arrow), t.glyph(Glyph::Reflex)), t.accent()),
            Span::raw("a reflex fired (lamp: fire, prime, inhibit)   "),
            Span::styled(t.glyph(Glyph::Instinct).to_string(), t.accent()),
            Span::raw(" an instinct"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {} ", t.glyph(Glyph::Ok)), t.green()),
            Span::raw("followed   "),
            Span::styled(format!("{} ", t.glyph(Glyph::Fail)), t.red()),
            Span::raw("ignored   "),
            Span::styled(format!("{} ", t.glyph(Glyph::Off)), t.dim()),
            Span::styled("control arm: reflexes withheld on purpose", t.dim()),
        ]),
    ]
}

/// The inspector: what the agent saw at one event.
pub fn inspect_lines(i: &FeedItem, ctx: &Ctx, width: u16) -> Vec<Line<'static>> {
    let t = &ctx.theme;
    let mut v = vec![Line::from(vec![
        Span::styled(format!("trace {}", feed::short_session(&i.session, 12)), t.bold()),
        Span::styled(
            format!(
                " · {} · {} · {}{}",
                feed::harness_name(&i.harness),
                i.event,
                clock(i.ts, ctx.tz, true),
                if i.repo.is_empty() { String::new() } else { format!(" · {}", i.repo) }
            ),
            t.ink2(),
        ),
    ])];
    if i.holdout {
        v.push(Line::from(Span::styled("control arm: nothing was delivered to the agent in this trace", t.dim())));
    }
    v.push(Line::raw(""));
    for a in &i.acts {
        let mut spans = vec![Span::raw("  ")];
        spans.extend(act_spans(a, t, Fit::Wide, width as usize, 0));
        v.push(Line::from(spans));
    }
    if i.acts.is_empty() {
        v.push(Line::from(Span::styled("  nothing to say here", t.dim())));
    }
    for (route, text) in &i.texts {
        v.push(Line::raw(""));
        let head = if i.holdout { "WHAT THE AGENT WOULD HAVE SEEN (WITHHELD)" } else { "WHAT THE AGENT SAW" };
        v.push(Line::from(vec![
            Span::styled(head, t.dim().patch(t.bold())),
            Span::styled(format!(" · {route}"), t.dim()),
        ]));
        v.extend(directive_lines(text, t, "  "));
    }
    v.push(Line::raw(""));
    v.push(Line::from(vec![
        Span::styled("evidence and review: ", t.dim()),
        Span::styled("adage ui", t.command()),
        Span::styled(" (Reflexes) · esc closes", t.dim()),
    ]));
    v
}

/// A directive's text as the agent sees it: the wrapper dim, commands in the command colour, the
/// provenance line secondary.
pub fn directive_lines(text: &str, t: &Theme, indent: &str) -> Vec<Line<'static>> {
    text.lines()
        .map(|l| {
            let trimmed = l.trim_start();
            let style = if trimmed.starts_with('<') {
                t.dim()
            } else if l.starts_with("  ") {
                t.command()
            } else if trimmed.starts_with("Learned by Adage") || trimmed.starts_with("Adage default") {
                t.ink2()
            } else {
                Style::default()
            };
            Line::from(vec![Span::raw(indent.to_string()), Span::styled(l.to_string(), style)])
        })
        .collect()
}

fn map_hint_lines(t: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::raw("The map shows every stimulus → reflex → response, with evidence."),
        Line::raw(""),
        Line::from(vec![Span::styled("  adage map", t.command()), Span::styled("   in a browser", t.dim())]),
        Line::from(vec![Span::styled("  adage ui", t.command()), Span::styled("    tab 3, in the terminal", t.dim())]),
        Line::raw(""),
        Line::from(Span::styled("esc closes", t.dim())),
    ]
}

// ---------------------------------------------------------------------------------------------
// Drawing: the live view

fn header(live: &Live, health: Option<&Health>, key: Option<&str>, ctx: &Ctx, width: u16) -> Line<'static> {
    let t = &ctx.theme;
    let gen = health.and_then(|h| h.generation.as_deref()).map(|g| format!("gen {}", &g[..g.len().min(8)]));
    let harnesses = health.map(|h| h.harnesses.join(", ")).filter(|h| !h.is_empty());
    let state: Vec<Span<'static>> = if live.paused {
        vec![Span::styled(format!("{} paused +{}", t.ch("‖", "="), live.held.len()), t.yellow().patch(t.bold()))]
    } else if live.scroll > 0 {
        vec![Span::styled(format!("{} scrolled {}", t.ch("↑", "^"), live.scroll), t.yellow())]
    } else if t.color {
        vec![Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.green()), Span::raw("live")]
    } else {
        vec![Span::styled("LIVE", t.bold())]
    };
    let (fire, prime, inhibit) = live.counts(key);
    let counter = |a: Aspect, n: usize, long: bool| -> Vec<Span<'static>> {
        let word = Theme::strength_word(a);
        match (t.color, long) {
            (true, true) => vec![
                Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.aspect(a)),
                Span::styled(format!("{word} {n}"), t.ink2()),
            ],
            (true, false) => vec![
                Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.aspect(a)),
                Span::styled(n.to_string(), t.ink2()),
            ],
            (false, _) => vec![Span::styled(format!("{} {n}", word.to_ascii_uppercase()), t.ink2())],
        }
    };
    let right = |long: bool, counters: bool| -> Vec<Span<'static>> {
        let mut v = state.clone();
        if counters {
            for (a, n) in [(Aspect::Green, fire), (Aspect::Yellow, prime), (Aspect::Red, inhibit)] {
                v.push(Span::raw(if long { "   " } else { "  " }));
                v.extend(counter(a, n, long));
            }
        }
        v.push(Span::raw(" "));
        v
    };
    let left = |facts: &[&Option<String>]| -> Vec<Span<'static>> {
        let mut v = vec![Span::styled(" adage", t.title()), Span::styled(" watch", t.bold())];
        let mut parts = vec![ctx.label.clone()];
        parts.extend(facts.iter().filter_map(|f| (*f).clone()));
        v.push(Span::styled(format!("  {}", parts.join(" · ")), t.ink2()));
        if ctx.demo {
            v.push(Span::styled(" · demo", t.holdout()));
        }
        v
    };
    let w = |s: &[Span]| s.iter().map(|x| x.width()).sum::<usize>();
    let options = [
        (left(&[&gen, &harnesses]), right(true, true)),
        (left(&[&gen]), right(true, true)),
        (left(&[&gen, &harnesses]), right(false, true)),
        (left(&[]), right(false, true)),
        (left(&[]), right(false, false)),
    ];
    let (l, r) = options.iter().find(|(l, r)| w(l) + w(r) < width as usize).cloned().unwrap_or_else(|| {
        (vec![Span::styled(" adage", t.title()), Span::styled(" watch", t.bold())], right(false, false))
    });
    let gap = (width as usize).saturating_sub(w(&l) + w(&r));
    let mut spans = l;
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(r);
    Line::from(spans)
}

fn footer(live: &Live, t: &Theme, width: u16) -> Line<'static> {
    if live.filtering {
        return Line::from(vec![
            Span::styled(" / ", t.key()),
            Span::raw(live.filter.clone()),
            Span::styled(t.ch("▌", "_"), t.accent()),
            Span::styled("   enter keep · esc clear", t.dim()),
        ]);
    }
    let quit = [Span::styled("q", t.key()), Span::styled(" quit ", t.dim())];
    let room = (width as usize).saturating_sub(9);
    let mut spans = vec![Span::raw(" ")];
    spans.extend(keys::bar(&[keys::WATCH], t, room));
    let used: usize = spans.iter().map(|s| s.width()).sum();
    spans.push(Span::raw(" ".repeat((width as usize).saturating_sub(used + 7))));
    spans.extend(quit);
    Line::from(spans)
}

/// The feed, newest at the bottom, in `area`.
fn draw_feed(f: &mut Frame, area: Rect, live: &Live, key: Option<&str>, ctx: &Ctx) {
    let t = &ctx.theme;
    let items = live.visible(key);
    if items.is_empty() {
        let lines = if !live.filter.is_empty() {
            vec![Line::from(Span::styled(
                format!(" Nothing matches “{}”. esc clears the filter.", live.filter),
                t.dim(),
            ))]
        } else {
            let scope = if live.all_repos || key.is_none() { "any repository".to_string() } else { ctx.label.clone() };
            vec![
                Line::from(Span::styled(format!(" No traces yet for {scope}."), t.dim())),
                Line::from(vec![
                    Span::styled(
                        " Start an agent there: each hook call appears here as it happens. No hooks yet? ",
                        t.dim(),
                    ),
                    Span::styled("adage init", t.command()),
                ]),
            ]
        };
        f.render_widget(Paragraph::new(wrap_lines(lines, area.width)), area);
        return;
    }
    let lines = feed_lines(&items, ctx, area.width, &live.pulse);
    let h = area.height as usize;
    let scroll = live.scroll.min(lines.len().saturating_sub(h));
    let end = lines.len() - scroll;
    let mut start = end.saturating_sub(h);
    // Never start in the middle of an event: its first line carries the time and the trace.
    while start > 0 && start < end && lines[start].0 == lines[start - 1].0 {
        start += 1;
    }
    // The newest stays at the bottom edge.
    let mut shown: Vec<Line> = vec![Line::raw(""); h.saturating_sub(end - start)];
    shown.extend(lines[start..end].iter().map(|(_, l)| l.clone()));
    f.render_widget(Paragraph::new(shown), area);
}

fn draw_ticker(f: &mut Frame, area: Rect, live: &Live, key: Option<&str>, ctx: &Ctx) {
    let t = &ctx.theme;
    let ticks = live.visible_ticks(key);
    let mut tail: Vec<Line> = Vec::new();
    if let Some(n) = live.waiting.filter(|n| *n > 0) {
        tail.push(Line::from(vec![
            Span::styled(format!("   {} ", t.glyph(Glyph::Off)), t.dim()),
            Span::styled(
                format!(
                    "{n} reflex{} wait{} for review · ",
                    if n == 1 { "" } else { "es" },
                    if n == 1 { "s" } else { "" }
                ),
                t.ink2(),
            ),
            Span::styled("adage review", t.command()),
        ]));
    }
    if ticks.is_empty() {
        let mut lines = vec![Line::from(vec![
            Span::styled(" Nothing conditioned yet. Runs appear here as they finish · ", t.dim()),
            Span::styled("adage condition", t.command()),
        ])];
        lines.extend(tail);
        f.render_widget(Paragraph::new(wrap_lines(lines, area.width)), area);
        return;
    }
    let h = (area.height as usize).saturating_sub(tail.len());
    let mut lines: Vec<Line> =
        ticks[ticks.len().saturating_sub(h)..].iter().map(|k| tick_line(k, ctx, area.width)).collect();
    lines.extend(tail);
    f.render_widget(Paragraph::new(lines), area);
}

/// The live view: now, conditioning, health. `chrome` adds the watch pane's own header and key
/// line (the full app has its own tab bar and status bar).
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
    let lights = lights_lines(health, t, area.width);
    let lights_h = (lights.len() as u16).clamp(1, 3);
    let ticker_h = match area.height {
        0..=17 => 2,
        18..=26 => 3,
        27..=40 => 4,
        _ => 8,
    } + u16::from(live.waiting.is_some_and(|n| n > 0));
    let roomy = area.height >= 24;
    let gap = u16::from(roomy);
    let (header_h, keys_h) = if chrome { (1, 2) } else { (0, 0) };
    let [header_a, _, now_rule, feed_area, _, cond_rule, ticker, _, health_rule, lamps, keys_a] = Layout::vertical([
        Constraint::Length(header_h),
        Constraint::Length(if chrome { gap } else { 0 }),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(gap),
        Constraint::Length(1),
        Constraint::Length(ticker_h),
        Constraint::Length(gap),
        Constraint::Length(1),
        Constraint::Length(lights_h),
        Constraint::Length(keys_h),
    ])
    .areas(area);

    if chrome {
        f.render_widget(Paragraph::new(header(live, health, key, ctx, area.width)), header_a);
    }
    let mut notes = Vec::new();
    if live.all_repos || key.is_none() {
        notes.push("all repositories".to_string());
    }
    if !live.filter.is_empty() {
        notes.push(format!("filter “{}”", live.filter));
    }
    if live.hide_quiet {
        notes.push("quiet hidden".to_string());
    }
    let note = (!notes.is_empty()).then(|| Span::styled(notes.join(" · "), t.dim()));
    f.render_widget(Paragraph::new(rule("now", note, area.width, t)), now_rule);
    draw_feed(f, feed_area, live, key, ctx);

    let running = (!live.learning.is_empty()).then(|| {
        let what = if live.learning.len() == 1 {
            "running now…".to_string()
        } else {
            format!("{} running…", live.learning.len())
        };
        Span::styled(what, t.yellow().patch(t.bold()))
    });
    f.render_widget(Paragraph::new(rule("conditioning", running, area.width, t)), cond_rule);
    draw_ticker(f, ticker, live, key, ctx);

    f.render_widget(Paragraph::new(rule("health", None, area.width, t)), health_rule);
    f.render_widget(Paragraph::new(lights), lamps);

    if chrome {
        let [r, k] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(keys_a);
        f.render_widget(Paragraph::new(rule("", None, area.width, t)), r);
        f.render_widget(Paragraph::new(footer(live, t, area.width)), k);
    }
    if live.help {
        popup(f, area, "keys", watch_help(t), t, live.popup_scroll);
    } else if live.map_hint {
        popup(f, area, "the map", map_hint_lines(t), t, live.popup_scroll);
    } else if live.inspect {
        match live.current(key, ctx, feed_area.width) {
            Some(i) => {
                popup(f, area, "inspect", inspect_lines(i, ctx, area.width.saturating_sub(8)), t, live.popup_scroll)
            }
            None => popup(f, area, "inspect", vec![Line::from(Span::styled("Nothing to inspect yet.", t.dim()))], t, 0),
        }
    }
}

/// The lamp of a health status.
pub fn lamp_of(s: crate::doctor::Status) -> Lamp {
    match s {
        crate::doctor::Status::Pass => Lamp::Ok,
        crate::doctor::Status::Warn => Lamp::Warn,
        crate::doctor::Status::Fail => Lamp::Fail,
    }
}
