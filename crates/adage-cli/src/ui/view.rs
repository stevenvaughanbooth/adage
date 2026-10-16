//! State shared by every screen, and the pure functions that draw it. Nothing here touches the
//! terminal: each `draw_*` renders state into a ratatui `Frame`, which tests drive with
//! `TestBackend`.
//!
//! The live view (`adage watch`, and the Live tab of `adage ui`) is a cockpit (`cockpit.rs`): a
//! pulse band of what Adage did, the session as a growing graph (`flow.rs`), and beside it (or
//! under it, on a narrow screen) the reflexes in play, the last conditioning and health. This
//! file holds its state ([`Live`]) and the pieces other screens share.

use super::feed::{self, Act, FeedItem};
use super::health::Health;
use super::keys;
use super::learnlog::{self, Tick, Tone};
use super::tail::Record;
use super::theme::{Lamp, Theme};
use adage_core::repo;
use adage_store::Paths;
use adage_term::Glyph;
use adage_tracks::Strength;
use anyhow::{bail, Context, Result};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

pub const NBSP: &str = "\u{a0}";

/// Feed items kept in memory.
pub const FEED_CAP: usize = 600;
/// Ticker lines kept in memory.
pub const TICK_CAP: usize = 200;
/// How long a firing reflex's box flashes.
pub const PULSE_MS: u128 = 900;
/// How long the heartbeat beats after an event arrives; the newest node glows as long.
pub const BEAT_MS: u128 = 800;
/// How long the newest node takes to slide into place.
pub const SLIDE_MS: u128 = 220;
/// How long a meter takes to ease to its new value.
pub const EASE_MS: u128 = 450;

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
        // The repository as the card names it (its `origin`, else its main checkout's name), not
        // the directory: a worktree is the same repository.
        let label = root
            .as_deref()
            .map(|r| repo::origin_key(r).unwrap_or_else(|| repo::checkout_name(r)))
            .unwrap_or_else(|| "(no repository)".into());
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

/// The live view's motion, worked out from the clock each frame ([`Live::tick`]). Left at its
/// default (still) by tests and `--once`, which draw final frames.
#[derive(Debug, Clone, Copy, Default)]
pub struct Anim {
    /// The heartbeat after an event arrived: 3 (just now) down to 1, then 0 (still).
    pub beat: u8,
    /// Milliseconds since the newest event arrived, while it slides in and glows.
    pub fresh_ms: Option<u128>,
    /// The follow rate as its meter shows it, easing to the true one.
    pub follow: Option<f64>,
    /// A clock for spinners, in milliseconds.
    pub spin_ms: u128,
    /// The wall clock (ms since the epoch), for `12 s ago`; without it, clock times.
    pub now_ms: Option<u64>,
}

impl Anim {
    /// Whether the newest node glows.
    pub fn glow(&self) -> bool {
        self.fresh_ms.is_some()
    }

    /// Columns the newest node sits right of its place while it slides in.
    pub fn slide(&self) -> usize {
        match self.fresh_ms {
            Some(ms) if ms < SLIDE_MS => {
                let p = ms as f32 / SLIDE_MS as f32;
                ((1.0 - super::motion::ease(p)) * 6.0).round() as usize
            }
            _ => 0,
        }
    }
}

/// The live feed and the conditioning ticker.
#[derive(Debug, Default)]
pub struct Live {
    pub feed: VecDeque<FeedItem>,
    /// Items that arrived while paused, shown on resume.
    pub held: Vec<FeedItem>,
    pub ticks: VecDeque<Tick>,
    pub paused: bool,
    /// Show every repository's activity, not only one's.
    pub all_repos: bool,
    /// The repository chosen to show (`adage ui`'s switcher); `None` follows the one the screen
    /// was opened in ([`Live::scope`]).
    pub repo: Option<String>,
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
    /// Every conditioning run in full (`l`) is open.
    pub runs: bool,
    /// Lines scrolled in the open popup.
    pub popup_scroll: u16,
    /// Reflexes waiting for review (nascent, or flagged), once known.
    pub waiting: Option<usize>,
    /// Of those: quiet ones (uplift below the bar), which are not install steps.
    pub quiet: usize,
    /// Of those: the ones that need a person (the rest are in use, worth a look).
    pub needs: usize,
    /// Each reflex's gist by id, once the reflexes are loaded: the ticker names reflexes by it.
    pub gists: std::collections::BTreeMap<String, String>,
    pub pulse: Pulse,
    pub anim: Anim,
    /// When the newest event arrived (after the history read at the start).
    beat_at: Option<Instant>,
    /// The follow meter's ease: from, to, since.
    ease: Option<(f64, f64, Instant)>,
    /// The first frame, the spinners' zero.
    born: Option<Instant>,
    next_seq: u64,
    primed: bool,
}

impl Live {
    pub fn push_records(&mut self, recs: Vec<Record>) {
        let mut fired = None;
        let arrived = !recs.is_empty();
        for r in recs {
            let mut item = feed::from_record(&r.session, &r.value);
            self.next_seq += 1;
            item.seq = self.next_seq;
            if item.fired() && !item.control {
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
        // Anything new beats the heart, and the newest node slides in and glows.
        if arrived && self.primed && !self.paused {
            self.beat_at = Some(Instant::now());
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

    /// Dim the pulse and the heartbeat with time; `motion` false (reduced motion) turns them off.
    pub fn tick_pulse(&mut self, now: Instant, motion: bool) {
        let born = *self.born.get_or_insert(now);
        self.anim.spin_ms = now.saturating_duration_since(born).as_millis();
        if let Some(at) = self.beat_at {
            let ms = now.saturating_duration_since(at).as_millis();
            if motion && ms < BEAT_MS {
                self.anim.fresh_ms = Some(ms);
                self.anim.beat = match ms {
                    0..150 => 3,
                    150..400 => 2,
                    _ => 1,
                };
            } else {
                self.beat_at = None;
                self.anim.fresh_ms = None;
                self.anim.beat = 0;
            }
        }
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

    /// A frame's clock work: the pulse and heartbeat, the wall clock (`now_ms`), and the follow
    /// meter easing to the rate of what `here`'s scope shows.
    pub fn tick(&mut self, now: Instant, motion: bool, here: Option<&str>, now_ms: u64) {
        self.tick_pulse(now, motion);
        self.anim.now_ms = Some(now_ms);
        let target = super::cockpit::Stats::of(&self.visible(here)).follow_rate().unwrap_or(0.0);
        match self.anim.follow {
            Some(shown) if motion && (shown - target).abs() > 1e-6 => {
                let (from, to, at) = match self.ease {
                    Some(e) if (e.1 - target).abs() <= 1e-9 => e,
                    _ => {
                        let e = (shown, target, now);
                        self.ease = Some(e);
                        e
                    }
                };
                let p = now.saturating_duration_since(at).as_millis() as f32 / EASE_MS as f32;
                if p >= 1.0 {
                    self.anim.follow = Some(to);
                    self.ease = None;
                } else {
                    self.anim.follow = Some(from + (to - from) * f64::from(super::motion::ease(p)));
                }
            }
            _ => {
                self.anim.follow = Some(target);
                self.ease = None;
            }
        }
    }

    /// Whether anything moves (the screen then redraws faster): a flash, the heartbeat, a meter.
    pub fn pulsing(&self) -> bool {
        self.pulse.at.is_some() || self.beat_at.is_some() || self.ease.is_some()
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

    /// The repository shown: `None` for every repository. The one chosen, else `here` (the one
    /// the screen was opened in), else every one.
    pub fn scope<'a>(&'a self, here: Option<&'a str>) -> Option<&'a str> {
        if self.all_repos {
            None
        } else {
            self.repo.as_deref().or(here)
        }
    }

    /// Opened outside any repository (`here` is `None`), the screens settle on the most recently
    /// active one, once there is one: a scope that moved with every new event would be no scope.
    pub fn settle(&mut self, here: Option<&str>) {
        if self.all_repos || self.repo.is_some() || here.is_some() {
            return;
        }
        let newest = self.feed.iter().chain(&self.held).filter(|i| !i.repo.is_empty()).max_by_key(|i| (i.ts, i.seq));
        self.repo = newest.map(|i| i.repo.clone());
    }

    /// The items shown, oldest first.
    pub fn visible<'a>(&'a self, repo_key: Option<&'a str>) -> Vec<&'a FeedItem> {
        let needle = self.filter.to_lowercase();
        let scope = self.scope(repo_key);
        self.feed
            .iter()
            .filter(|i| scope.is_none_or(|k| i.repo == k))
            .filter(|i| !self.hide_quiet || !i.quiet())
            .filter(|i| needle.is_empty() || i.haystack().to_lowercase().contains(&needle))
            .collect()
    }

    pub fn visible_ticks<'a>(&'a self, repo_key: Option<&'a str>) -> Vec<&'a Tick> {
        let scope = self.scope(repo_key);
        self.ticks.iter().filter(|t| scope.is_none_or(|k| t.repo.as_deref().is_none_or(|r| r == k))).collect()
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
            'l' => self.runs = true,
            _ => return false,
        }
        true
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
        spans.push(Span::styled(t.sep(), t.dim()));
        spans.push(n);
    }
    let used: usize = spans.iter().map(|s| s.width()).sum();
    let rest = (width as usize).saturating_sub(used + 1);
    spans.push(Span::styled(format!(" {}", dash.repeat(rest)), t.rule()));
    Line::from(spans)
}

/// The lamp and word of a strength, padded to one column width: `● fire     ` / `FIRE       `.
fn strength_cell(a: Strength, t: &Theme, w: usize, bold: bool) -> Vec<Span<'static>> {
    let style = if bold { t.strength(a).patch(t.bold()) } else { t.strength(a) };
    if t.color {
        vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.strength(a)),
            Span::styled(t.pad(Theme::strength_word(a), w.saturating_sub(2)), style),
        ]
    } else {
        vec![Span::styled(t.pad(&Theme::strength_word(a).to_ascii_uppercase(), w), style)]
    }
}

/// How roomy an act's words are.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Fit {
    /// Every word.
    Wide,
    /// A narrow pane: no verbs the glyphs already carry.
    Narrow,
}

/// One act, as spans, within `room` columns.
pub(super) fn act_spans(
    a: &Act,
    t: &Theme,
    fit_: Fit,
    room: usize,
    hot: u8,
    name: &dyn Fn(&str) -> String,
) -> Vec<Span<'static>> {
    let narrow = fit_ == Fit::Narrow;
    let verb = |v: &str| if narrow { String::new() } else { format!("{v} ") };
    let off = t.glyph(Glyph::Off);
    let mut spans = match a {
        Act::Directed { route, strength, detail, instinct } => {
            let glyph = if detail == "card" || detail.starts_with("card · ") {
                Glyph::Card
            } else if *instinct {
                Glyph::Instinct
            } else {
                Glyph::Reflex
            };
            let mut v = vec![
                Span::styled(format!("{} ", t.glyph(Glyph::Arrow)), t.accent()),
                Span::styled(format!("{} ", t.glyph(glyph)), t.accent()),
            ];
            let cell_w = if narrow { Theme::strength_word(*strength).len() + 3 } else { 11 };
            let mut cell = strength_cell(*strength, t, cell_w, hot > 0);
            if hot == 3 {
                if let Some(first) = cell.first_mut() {
                    first.style = first.style.add_modifier(Modifier::REVERSED);
                }
            }
            v.extend(cell);
            // What it said, in the pulse's emphasis while it is new; the id only when it said
            // nothing that can be shown.
            let what = if detail.is_empty() { route.as_str() } else { detail.as_str() };
            let style = match hot {
                0 => t.ink2(),
                1 => t.bold(),
                _ => t.accent().patch(t.bold()),
            };
            let used: usize = v.iter().map(|s| s.width()).sum();
            v.push(Span::styled(t.fit(&t.text(what), room.saturating_sub(used)), style));
            v
        }
        Act::Withheld { route, strength } => {
            vec![Span::styled(
                format!(
                    "{off} would {} {} (withheld{}control session)",
                    Theme::strength_word(*strength),
                    name(route),
                    t.sep()
                ),
                t.control(),
            )]
        }
        Act::Followed(r) => vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Ok)), t.green()),
            Span::styled(verb("followed"), t.ink2()),
            Span::raw(name(r)),
        ],
        Act::Ignored(r) => vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Fail)), t.red()),
            Span::styled(verb("not observed"), t.ink2()),
            Span::raw(name(r)),
        ],
        Act::Misfired(r) => vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Fail)), t.red()),
            Span::styled(verb("misfired"), t.ink2()),
            Span::raw(name(r)),
        ],
        Act::Met(r) => vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Ok)), t.green()),
            Span::styled("finish line met: ".to_string(), t.ink2()),
            Span::raw(name(r)),
        ],
        Act::Unaided(r) => vec![Span::styled(format!("{off} found {} unaided", name(r)), t.control())],
        Act::NotReached(r) => vec![Span::styled(format!("{off} missed {}", name(r)), t.control())],
        Act::StopRequest => vec![
            Span::styled(format!("{} ", t.ch("■", "#")), t.yellow().patch(t.bold())),
            Span::raw("finish line: asked the agent to continue".to_string()),
        ],
        Act::StopWithheld => {
            vec![Span::styled(
                format!("{off} would ask the agent to continue (withheld{}control session)", t.sep()),
                t.control(),
            )]
        }
        Act::Pretool { rewrite, kind, interpreter, withheld } => {
            let what = pretool_words(*rewrite, kind, interpreter);
            if *withheld {
                vec![Span::styled(format!("{off} would have {what} (withheld{}control session)", t.sep()), t.control())]
            } else {
                // A rewrite changed the command before it ran: its own mark, always shown.
                let mark = if *rewrite { Glyph::Rewrite } else { Glyph::Arrow };
                vec![
                    Span::styled(format!("{} ", t.glyph(mark)), t.accent()),
                    Span::styled(what, if *rewrite { t.accent().patch(t.bold()) } else { t.ink2() }),
                ]
            }
        }
        Act::Noticed { candidate, support, review, .. } => {
            let seen =
                if *support == 1 { "1 earlier session".to_string() } else { format!("{support} earlier sessions") };
            let mut v = vec![
                Span::styled(format!("{} ", t.ch("○", "o")), t.dim()),
                Span::styled(verb("noticed"), t.ink2()),
                // A candidate is not a reflex yet, so it often has no gist: say what it is, not
                // its id (`enter` shows the id).
                Span::raw(match name(candidate) {
                    n if n == *candidate => "a pattern forming".to_string(),
                    n => n,
                }),
                Span::styled(format!(" · seen in {seen}"), t.dim()),
            ];
            if *review {
                v.push(Span::styled(" · waits for your review".to_string(), t.dim()));
            }
            v
        }
        Act::Answered { command, ago, .. } => {
            let when = if ago.is_empty() { String::new() } else { format!(" · ran {ago}") };
            vec![
                Span::styled(format!("{} ", t.glyph(Glyph::Ok)), t.green()),
                Span::styled(verb("answered from its run of"), t.ink2()),
                Span::raw(command.clone()),
                Span::styled(when, t.dim()),
            ]
        }
        Act::Recovery { operation } => {
            let text = match operation {
                Some(id) => format!(
                    "an action was interrupted; inspect the worktree, then run adage-exec recover --id {id} --acknowledge in a terminal"
                ),
                None => "the execution journal cannot be read; inspect the worktree and repair the journal".to_string(),
            };
            vec![Span::styled(format!("{} ", t.ch("⚠", "!")), t.yellow().patch(t.bold())), Span::raw(text)]
        }
        Act::Other(kind, r) => vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Sep)), t.dim()),
            Span::styled(format!("{kind} "), t.ink2()),
            Span::raw(name(r)),
        ],
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
                if trimmed.chars().count() <= room - used { trimmed } else { t.fit(&trimmed, room - used) }.into();
        }
        used += w;
        true
    });
    spans
}

/// What a PreToolUse record says Adage did: `pointed the test run at .venv/bin/python`.
fn pretool_words(rewrite: bool, kind: &str, interpreter: &str) -> String {
    let at = if interpreter.is_empty() { String::new() } else { format!(" at {interpreter}") };
    match (rewrite, kind) {
        (true, "test") => format!("pointed the test run{at}"),
        (true, "repro") => format!("pointed the Python run{at}"),
        (true, k) => format!("rewrote the command ({k}){at}"),
        (false, "pip-install") => "advised on pip install (the project's environment)".to_string(),
        (false, "permissions") => "advised a rewrite your permission rules would treat differently".to_string(),
        (false, k) => format!("advised ({k})"),
    }
}

/// Whether an act is something Adage itself did (said, rewrote, asked, answered): the feed tags
/// those `adage`, so attribution is visible where it happened.
pub(super) fn adage_did(a: &Act) -> bool {
    match a {
        Act::Directed { .. } | Act::StopRequest | Act::Answered { .. } | Act::Recovery { .. } => true,
        Act::Pretool { withheld, .. } => !withheld,
        _ => false,
    }
}

/// One conditioning line.
pub fn tick_line(k: &Tick, ctx: &Ctx, width: u16, gists: &std::collections::BTreeMap<String, String>) -> Line<'static> {
    let t = &ctx.theme;
    let wide = width >= 90;
    match (k.tone, &k.change, &k.run) {
        (Tone::Error, _, _) => Line::from(vec![
            Span::styled(format!(" {} ", t.glyph(Glyph::Fail)), t.red()),
            Span::styled(format!("{} ", clock(k.ts, ctx.tz, false)), t.dim()),
            Span::styled(
                t.fit(&t.text(if wide { &k.text } else { &k.short }), (width as usize).saturating_sub(9)),
                t.red(),
            ),
        ]),
        (Tone::Change, Some(c), _) if c.what == "forming" => {
            // Candidates, counted on one line: patterns seen, not yet reflexes.
            let mut spans = vec![
                Span::raw("   "),
                Span::styled(format!("{} ", t.glyph(Glyph::Off)), t.dim()),
                Span::styled(c.name().to_string(), t.ink2()),
            ];
            if !c.numbers.is_empty() {
                let used: usize = spans.iter().map(|s| s.width()).sum::<usize>() + 3;
                spans.push(Span::styled(
                    format!("{}{}", t.sep(), t.fit(&t.text(&c.numbers), (width as usize).saturating_sub(used))),
                    t.dim(),
                ));
            }
            Line::from(spans)
        }
        (Tone::Change, Some(c), _) => {
            // The reflex by what it tells the agent (its id in lines written before gists).
            let id_w = if wide { 44 } else { 26 };
            let glyph = if c.what == "written" { t.glyph(Glyph::Done) } else { t.glyph(Glyph::Reflex) };
            let mut spans = vec![
                Span::raw("   "),
                Span::styled(format!("{glyph} "), t.accent()),
                Span::styled(
                    t.pad(&t.fit(gists.get(&c.route).map_or(c.name(), String::as_str), id_w - 1), id_w),
                    t.bold(),
                ),
            ];
            let word_style = |w: &str| match w {
                "fire" | "firm" => t.green().patch(t.bold()),
                "prime" => t.yellow().patch(t.bold()),
                "inhibit" => t.red().patch(t.bold()),
                "forming" => t.accent().patch(t.bold()),
                _ => t.dim(),
            };
            match c.what {
                "evidence" => spans.push(Span::styled(t.pad("evidence", 19), t.ink2())),
                "proposal" => spans.push(Span::styled(t.pad("change proposed", 19), t.ink2())),
                "new" => {
                    spans.push(Span::styled(format!("new{}", t.sep()), t.ink2()));
                    let to = c.to.clone().unwrap_or_default();
                    spans.push(Span::styled(t.pad(&to, 13), word_style(&to)));
                }
                "written" => spans.push(Span::styled(t.pad(&c.to.clone().unwrap_or_default(), 19), t.ink2())),
                _ => {
                    let from = c.from.as_deref().map(|f| format!("{f} {} ", t.glyph(Glyph::Arrow))).unwrap_or_default();
                    let to = c.to.clone().unwrap_or_default();
                    let fw = from.chars().count();
                    spans.push(Span::styled(from, t.ink2()));
                    spans.push(Span::styled(
                        t.pad(&to, 19usize.saturating_sub(fw).max(to.chars().count() + 1)),
                        word_style(&to),
                    ));
                }
            }
            if !c.numbers.is_empty() {
                let used: usize = spans.iter().map(|s| s.width()).sum();
                spans.push(Span::styled(t.fit(&t.text(&c.numbers), (width as usize).saturating_sub(used)), t.ink2()));
            }
            // Which collection, in words, when it is not the reflexes learned here (the usual one).
            let collection = super::scope::collection(&c.track, k.repo.as_deref().unwrap_or(""), false, true);
            if wide && !c.track.is_empty() && c.what != "written" && collection != "Learned here" {
                let used: usize = spans.iter().map(|s| s.width()).sum();
                let room = (width as usize).saturating_sub(used + 4);
                if room > 8 {
                    spans.push(Span::styled(format!("   {}", t.fit(&collection, room)), t.dim()));
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
                if r.forming > 0 {
                    facts.push(learnlog::forming_words(r.forming));
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
            let tail = if facts.is_empty() { String::new() } else { format!("{}{}", t.sep(), facts.join(&t.sep())) };
            let lead = format!(" {} {} ", t.glyph(Glyph::Active), clock(k.ts, ctx.tz, false));
            let room = (width as usize).saturating_sub(lead.chars().count() + head.len() + repo.len());
            Line::from(vec![
                Span::styled(format!(" {} ", t.glyph(Glyph::Active)), t.accent()),
                Span::styled(format!("{} ", clock(k.ts, ctx.tz, false)), t.dim()),
                Span::raw(format!("{head}{repo}")),
                Span::styled(t.fit(&tail, room), t.ink2()),
            ])
        }
    }
}

/// The health lamps, each with its fixing command, wrapped to `width` (a lamp never splits).
pub fn lights_lines(h: Option<&Health>, t: &Theme, width: u16) -> Vec<Line<'static>> {
    let Some(h) = h else { return vec![Line::from(Span::styled(format!(" checking health{}", t.more()), t.dim()))] };
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut cur: Vec<Span<'static>> = vec![Span::raw(" ")];
    for l in &h.lights {
        let mut one = vec![
            Span::styled(format!("{} ", t.lamp_mark(l.lamp)), t.lamp(l.lamp)),
            Span::styled(t.text(&l.label), t.ink2()),
        ];
        if let Some(cmd) = &l.hint {
            one.push(Span::styled(t.sep(), t.dim()));
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

/// The savings estimate in one line (`crate::savings::brief`): the verdict's mark (none without
/// colour: the words alone), then the figure in the default ink. Dollars only past the gate.
pub fn savings_line(s: &crate::savings::Summary, t: &Theme, width: u16) -> Line<'static> {
    use crate::savings::Verdict;
    let mut spans = vec![Span::raw(" ")];
    if t.color {
        let (g, style) = match s.verdict {
            Verdict::Saved => (Glyph::Ok, t.green()),
            Verdict::AddsCost => (Glyph::Fail, t.red()),
            Verdict::Measuring | Verdict::NoDifference => (Glyph::Off, t.dim()),
        };
        spans.push(Span::styled(format!("{} ", t.glyph(g)), style));
    }
    spans.push(Span::styled("savings ", t.ink2()));
    let used: usize = spans.iter().map(|s| s.width()).sum();
    spans.push(Span::raw(t.fit(&t.text(&crate::savings::brief(s)), (width as usize).saturating_sub(used + 1))));
    Line::from(spans)
}

/// Word-wrap `lines` to `width` columns. Unlike ratatui's own wrapping, a continuation keeps its
/// line's indent (its leading spaces), so wrapped text stays aligned under the text it continues.
/// A word longer than the room is broken at the edge.
pub fn wrap_lines(lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    wrap_hanging(lines, width, false)
}

/// [`wrap_lines`]; with `hang`, a line of two or more spans continues under its second span
/// (a key column's description, say), not under its first.
pub fn wrap_hanging(lines: Vec<Line<'static>>, width: u16, hang: bool) -> Vec<Line<'static>> {
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
        let lead = cells.iter().take_while(|(c, _)| *c == ' ').count();
        let first = line.spans.first().map_or(0, |s| s.width());
        let indent = if hang && line.spans.len() > 1 { first.max(lead) } else { lead }.min(width / 2);
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
    popup_with(f, area, title, lines, t, scroll, false)
}

/// The keys overlay: a popup whose wrapped descriptions stay under the description column.
pub fn popup_keys(f: &mut Frame, area: Rect, lines: Vec<Line<'static>>, t: &Theme, scroll: u16) {
    popup_with(f, area, "keys", lines, t, scroll, true)
}

fn popup_with(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>, t: &Theme, scroll: u16, hang: bool) {
    let lines = t.lines(lines);
    let w = lines
        .iter()
        .map(|l| l.width() as u16)
        .max()
        .unwrap_or(20)
        .saturating_add(4)
        .clamp(30, area.width.saturating_sub(4).max(10));
    // Wrapped before sizing, so the last line (often the keys that answer the popup) is counted.
    let lines = wrap_hanging(t.lines(lines), w.saturating_sub(4), hang);
    let h = (lines.len() as u16).saturating_add(2).min(area.height.saturating_sub(2).max(3));
    let r = centered(area, w, h);
    f.render_widget(Clear, r);
    let hidden = (lines.len() as u16).saturating_sub(h.saturating_sub(2));
    let scroll = scroll.min(hidden);
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_set(t.border())
        .border_style(t.accent())
        .title(Span::styled(format!(" {} ", t.text(title)), t.command().patch(t.bold())));
    if hidden > 0 {
        // More than fits: say so, and which keys reach it.
        let more = match (scroll > 0, scroll < hidden) {
            (true, true) => format!(" {} more{}j/k scroll ", t.arrows(), t.sep()),
            (false, _) => format!(" {} more{}j/k scroll ", t.ch("↓", "v"), t.sep()),
            (true, false) => format!(" {} more{}j/k scroll ", t.ch("↑", "^"), t.sep()),
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

/// What the live view's marks mean.
pub fn legend(t: &Theme) -> Vec<Line<'static>> {
    let a = |s: String| Span::styled(s, t.accent());
    vec![
        Line::from(Span::styled("MARKS", t.dim().patch(t.bold()))),
        Line::from(vec![
            Span::styled(format!("  {} adage ", t.glyph(Glyph::Active)), t.accent().patch(t.bold())),
            Span::raw("something Adage did, where it happened; in a box, a reflex it delivered"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {} ", t.glyph(Glyph::Lamp)), t.green()),
            Span::raw("fire   "),
            Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.yellow()),
            Span::raw("prime   "),
            Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.red()),
            Span::raw("inhibit: how firmly it said so"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}{} ", t.ch("───", "---"), t.ch("▶", ">")), t.green()),
            Span::styled(format!("{} followed", t.glyph(Glyph::Ok)), t.green()),
            Span::raw(": the agent took it   "),
            Span::styled(format!("{} ", t.glyph(Glyph::Fail)), t.red()),
            Span::raw("not followed, or it failed"),
        ]),
        Line::from(vec![
            a(format!("  {} ", t.ch("┊", ":"))),
            Span::raw("quiet steps, folded   "),
            a(format!("{} ", t.glyph(Glyph::Done))),
            Span::raw("a turn that ended with nothing to say"),
        ]),
        Line::from(vec![
            a(format!("  {} ", t.glyph(Glyph::Rewrite))),
            Span::raw("a command rewritten before it ran   "),
            a(format!("{} ", t.glyph(Glyph::Card))),
            Span::raw("a procedure card   "),
            Span::styled(format!("{} ", t.glyph(Glyph::Off)), t.dim()),
            Span::styled("control arm: withheld on purpose", t.dim()),
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
                "{sep}{}{sep}{}{sep}{}{}",
                feed::harness_name(&i.harness),
                i.event,
                clock(i.ts, ctx.tz, true),
                if i.repo.is_empty() { String::new() } else { format!("{}{}", t.sep(), i.repo) },
                sep = t.sep()
            ),
            t.ink2(),
        ),
    ])];
    if i.control {
        v.push(Line::from(Span::styled("control arm: nothing was delivered to the agent in this trace", t.dim())));
    }
    v.push(Line::raw(""));
    // The inspector names reflexes by id: it is where the id is for (`adage review <track> <id>`).
    for a in &i.acts {
        let mut spans = vec![Span::raw("  ")];
        spans.extend(act_spans(a, t, Fit::Wide, width as usize, 0, &|r: &str| r.to_string()));
        v.push(Line::from(spans));
        if let Act::Directed { route, .. } = a {
            v.push(Line::from(Span::styled(format!("      reflex {route}"), t.dim())));
        }
    }
    if i.acts.is_empty() {
        v.push(Line::from(Span::styled("  nothing to say here", t.dim())));
    }
    for (route, text) in &i.texts {
        v.push(Line::raw(""));
        let head = if i.control { "WHAT THE AGENT WOULD HAVE SEEN (WITHHELD)" } else { "WHAT THE AGENT SAW" };
        v.push(Line::from(vec![
            Span::styled(head, t.dim().patch(t.bold())),
            Span::styled(format!("{}{route}", t.sep()), t.dim()),
        ]));
        v.extend(directive_lines(text, t, "  "));
    }
    v.push(Line::raw(""));
    v.push(Line::from(vec![
        Span::styled("evidence and review: ", t.dim()),
        Span::styled("adage ui", t.command()),
        Span::styled(format!(" (Reflexes){}esc closes", t.sep()), t.dim()),
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
            Line::from(vec![Span::raw(indent.to_string()), Span::styled(t.text(l), style)])
        })
        .collect()
}

pub(super) fn map_hint_lines(t: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::raw(t.text("The map shows every stimulus → reflex → response, with evidence.")),
        Line::raw(""),
        Line::from(vec![Span::styled("  adage map", t.command()), Span::styled("   in a browser", t.dim())]),
        Line::from(vec![Span::styled("  adage ui", t.command()), Span::styled("    tab 3, in the terminal", t.dim())]),
        Line::raw(""),
        Line::from(Span::styled("esc closes", t.dim())),
    ]
}

// ---------------------------------------------------------------------------------------------
// Drawing: the live view

/// The watch pane's key line (or the filter being typed).
pub(super) fn footer(live: &Live, t: &Theme, width: u16) -> Line<'static> {
    if live.filtering {
        let mut v = vec![
            Span::styled(" / ", t.key()),
            Span::raw(live.filter.clone()),
            Span::styled(t.ch("▌", "_"), t.accent()),
            Span::raw("   "),
        ];
        v.extend(super::kit::hints(&[("enter", "keep"), ("esc", "clear")], t, width as usize));
        return Line::from(v);
    }
    let room = (width as usize).saturating_sub(11);
    let mut spans = vec![Span::raw(" ")];
    spans.extend(keys::bar(&[keys::WATCH], t, room));
    let used: usize = spans.iter().map(|s| s.width()).sum();
    spans.push(Span::raw(" ".repeat((width as usize).saturating_sub(used + 9))));
    spans.extend(super::kit::hints(&[("q", "quit")], t, 8));
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// The live view: the pulse band, the session's graph, and what Adage learned and how it is.
/// `chrome` adds the watch pane's own header and key line (the full app has its own tab bar and
/// status bar).
pub fn draw_live(f: &mut Frame, area: Rect, live: &Live, health: Option<&Health>, ctx: &Ctx, chrome: bool) {
    draw_live_in(f, area, live, health, health.and_then(|h| h.repo_key.as_deref()), ctx, chrome)
}

/// [`draw_live`], with `here` the repository the screen was opened in (the scope, unless another
/// one, or every one, was chosen: [`Live::scope`]).
pub fn draw_live_in(
    f: &mut Frame,
    area: Rect,
    live: &Live,
    health: Option<&Health>,
    here: Option<&str>,
    ctx: &Ctx,
    chrome: bool,
) {
    super::cockpit::draw(f, area, live, health, here, ctx, chrome);
}

/// The lamp of a health status.
pub fn lamp_of(s: crate::doctor::Status) -> Lamp {
    match s {
        crate::doctor::Status::Pass => Lamp::Ok,
        crate::doctor::Status::Warn => Lamp::Warn,
        crate::doctor::Status::Fail => Lamp::Fail,
    }
}
