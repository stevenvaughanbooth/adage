//! The start screen: as `adage ui` opens, Adage's brain (the site hero's, [`super::brain`]) turns
//! for about two seconds, lit in text, over the wordmark and the tagline, then dissolves into the
//! app. Any key ends it at once, and every key but the ones people press to skip (enter, space,
//! escape) goes on to the app, so nobody waits for it.
//!
//! It is drawn over the app's own frame, so the app is there underneath to dissolve into, and it
//! never paints the background. It is not shown where it would be in the way or unwanted
//! ([`skip`]): away from a terminal, in an agent's session, on a dumb or small terminal, with
//! `ADAGE_REDUCED_MOTION`, with `ui.start_screen` off (`ADAGE_START_SCREEN=0`), or when `adage
//! watch` hands over to the app. A frame is a pure function of the time since the start
//! ([`Splash::draw_at`]), which the tests snapshot.

use super::brain::{Brain, Canvas, Fit, Pose};
use super::theme::Theme;
use super::view::Ctx;
use adage_term::Level;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use std::time::{Duration, Instant};

/// The brain fades in over this long (seconds)…
const FADE_IN: f32 = 0.35;
/// …turns until this…
const DISSOLVE: f32 = 1.9;
/// …and has dissolved into the app by this.
pub const LENGTH: f32 = 2.35;

/// Time between frames while it runs: about 30 a second.
pub const FRAME: Duration = Duration::from_millis(33);

/// The smallest terminal it is drawn on; below this the brain is too small to read.
pub const MIN_COLS: u16 = 60;
pub const MIN_ROWS: u16 = 20;

/// The turn: from above and three-quarters behind, where both hemispheres show with the fissure
/// between them and the cerebellum tucked under the occipital lobes over the stem, slowly round
/// towards the side.
fn pose(t: f32) -> Pose {
    Pose { yaw: -2.75 + 0.21 * t, pitch: 0.5 - 0.025 * t }
}

/// Darkest to brightest, a glyph for each step of light.
const RAMP: [&str; 10] = [" ", ".", ":", "-", "=", "+", "*", "#", "%", "@"];

/// The wordmark and the tagline (the site's headline).
const WORDMARK: &str = "a d a g e";
const TAGLINE: &str = "Agent traces become reflexes.";

/// Why the start screen is not shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// Standard input or output is not a terminal.
    NotATerminal,
    /// An agent's session: no screen of its own, and no delay.
    AgentSession,
    /// `TERM=dumb`.
    Dumb,
    /// `ADAGE_REDUCED_MOTION=1`.
    ReducedMotion,
    /// `ui.start_screen` is off (or `ADAGE_START_SCREEN=0`).
    Off,
    /// `adage watch` handed over to the app: its person is already at work.
    FromWatch,
    /// Smaller than [`MIN_COLS`] by [`MIN_ROWS`].
    TooSmall,
}

/// What decides whether the start screen shows, gathered by the caller.
pub struct Conditions<'a> {
    /// Standard input and output are both terminals.
    pub tty: bool,
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// The theme's one-shot motion (off with `ADAGE_REDUCED_MOTION=1`).
    pub motion: bool,
    /// `ui.start_screen`.
    pub setting: bool,
    pub from_watch: bool,
    /// The terminal's size, columns by rows.
    pub size: (u16, u16),
}

/// Why the start screen should not be shown, if it should not; `None` shows it.
pub fn skip(c: &Conditions) -> Option<Skip> {
    let set = |k: &str| (c.env)(k).filter(|v| !v.is_empty());
    if !c.tty {
        Some(Skip::NotATerminal)
    } else if crate::install::AGENT_ENV.iter().any(|k| set(k).is_some()) {
        Some(Skip::AgentSession)
    } else if set("TERM").as_deref() == Some("dumb") {
        Some(Skip::Dumb)
    } else if !c.motion {
        Some(Skip::ReducedMotion)
    } else if !c.setting {
        Some(Skip::Off)
    } else if c.from_watch {
        Some(Skip::FromWatch)
    } else if c.size.0 < MIN_COLS || c.size.1 < MIN_ROWS {
        Some(Skip::TooSmall)
    } else {
        None
    }
}

/// The start screen for `adage ui` opening in `ctx` on a terminal of `size`, unless [`skip`]
/// says otherwise.
pub fn start(ctx: &Ctx, size: (u16, u16), from_watch: bool) -> Option<Splash> {
    use std::io::IsTerminal;
    let env = |k: &str| std::env::var(k).ok();
    let conditions = Conditions {
        tty: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        env: &env,
        motion: ctx.theme.motion,
        setting: adage_store::config::load(&ctx.paths).config.ui_start_screen,
        from_watch,
        size,
    };
    skip(&conditions).is_none().then(Splash::new)
}

/// A key pressed during the start screen: whether it only ends it. Enter, space and escape
/// are what people press to skip, and would otherwise act on the app; every other key ends
/// the start screen and goes on to the app as usual.
pub fn swallows(code: KeyCode) -> bool {
    matches!(code, KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Esc)
}

/// The start screen while it runs.
pub struct Splash {
    brain: Brain,
    canvas: Canvas,
    /// The fit for the last brain area drawn, so a resize refits.
    fit: Option<(Rect, Fit)>,
    started: Instant,
}

impl Splash {
    /// Sample the brain, and start the clock once it is ready.
    pub fn new() -> Splash {
        let brain = Brain::new();
        Splash { brain, canvas: Canvas::new(0, 0), fit: None, started: Instant::now() }
    }

    /// Seconds since it started.
    fn elapsed(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.started).as_secs_f32()
    }

    /// It has dissolved into the app.
    pub fn done(&self, now: Instant) -> bool {
        self.elapsed(now) >= LENGTH
    }

    /// Draw the frame for `now` over the app's frame in `buf`.
    pub fn draw(&mut self, buf: &mut Buffer, area: Rect, now: Instant, t: &Theme) {
        let at = self.elapsed(now);
        self.draw_at(buf, area, at, t);
    }

    /// Draw the frame `at` seconds in over `buf`: the brain, the wordmark and the tagline, every
    /// cell of `area` overwritten, except those that have dissolved to show the app.
    pub fn draw_at(&mut self, buf: &mut Buffer, area: Rect, at: f32, t: &Theme) {
        let area = area.intersection(buf.area);
        if area.width < MIN_COLS || area.height < MIN_ROWS || at >= LENGTH {
            return;
        }
        // The brain over three rows of text: a gap, the wordmark, the tagline, and a last row
        // left clear.
        let brain = Rect { height: area.height - 4, ..area };
        let fit = match self.fit {
            Some((r, fit)) if r == brain => fit,
            _ => {
                let poses: Vec<Pose> = (0..=8).map(|i| pose(LENGTH * i as f32 / 8.0)).collect();
                let fit = self.brain.fit(&poses, brain.width as usize, brain.height as usize);
                self.canvas = Canvas::new(brain.width as usize, brain.height as usize);
                self.fit = Some((brain, fit));
                fit
            }
        };
        let p = pose(at);
        self.brain.render(p, fit, &mut self.canvas);
        let sparks = if t.color { self.brain.sparks(p, fit, at, &self.canvas) } else { vec![] };
        let shade = Shade::new(t);
        let fade_in = smooth(at / FADE_IN);
        let dissolve = smooth((at - DISSOLVE) / (LENGTH - DISSOLVE));
        let mut text = vec![];
        let (cx, ty) = (area.x + area.width / 2, area.y + area.height - 3);
        // The wordmark resolves letter by letter out of the brain's own glyphs.
        let word: Vec<char> = WORDMARK.chars().collect();
        let x0 = cx.saturating_sub(word.len() as u16 / 2);
        for (i, ch) in word.iter().enumerate() {
            let settle = 0.3 + 0.07 * i as f32;
            let s = if *ch == ' ' || at >= settle {
                ch.to_string()
            } else if at >= settle - 0.3 {
                RAMP[2 + (hash(i as u16, (at * 24.0) as u16) * 8.0) as usize].to_string()
            } else {
                continue;
            };
            text.push((x0 + i as u16, ty, s, t.title()));
        }
        // The tagline types out once the wordmark has settled.
        let typed = (((at - 0.75) / 0.45).clamp(0.0, 1.0) * TAGLINE.len() as f32) as usize;
        let tag = t.text(TAGLINE);
        let x0 = cx.saturating_sub(tag.chars().count() as u16 / 2);
        for (i, ch) in tag.chars().take(typed).enumerate() {
            text.push((x0 + i as u16, ty + 1, ch.to_string(), t.ink2()));
        }

        for row in area.top()..area.bottom() {
            for col in area.left()..area.right() {
                // Dissolving: each cell goes in its own moment, thinning first.
                let h = hash(col, row);
                if h < dissolve {
                    continue;
                }
                let thin = if h < dissolve + 0.12 { 3 } else { 0 };
                let cell = &mut buf[(col, row)];
                cell.reset();
                if !brain.contains((col, row).into()) {
                    continue;
                }
                let (c, r) = ((col - brain.x) as usize, (row - brain.y) as usize);
                if let Some(s) =
                    sparks.iter().filter(|s| (s.col, s.row) == (c, r)).max_by(|a, b| a.glow.total_cmp(&b.glow))
                {
                    let glyph = if s.glow > 0.55 {
                        "@"
                    } else if s.glow > 0.25 {
                        "*"
                    } else {
                        "+"
                    };
                    cell.set_symbol(glyph).set_style(shade.spark(s.hue, s.glow * fade_in));
                    continue;
                }
                let px = self.canvas.at(c, r);
                if !px.drawn() {
                    continue;
                }
                let (level, style) = shade.cell(px.light, px.alpha * fade_in, thin);
                cell.set_symbol(RAMP[level]).set_style(style);
            }
        }
        for (x, y, s, style) in text {
            if x < area.right() && hash(x, y) >= dissolve {
                buf[(x, y)].set_symbol(&s).set_style(style);
            }
        }
    }
}

/// Ease in and out, 0 below 0 and 1 above 1.
fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A cell's own number in [0, 1), the same every time.
fn hash(a: u16, b: u16) -> f32 {
    let mut h = (a as u32).wrapping_mul(0x9E37_79B1) ^ (b as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// The palette the brain is lit in, from the shadow up to a glint: the hero's violets on a night
/// ground; on a day ground, the ramp runs the other way (shadows dense and dark, light sparse).
const NIGHT: [(f32, [u8; 3]); 5] = [
    (0.0, [0x33, 0x2c, 0x66]),
    (0.3, [0x55, 0x48, 0xb0]),
    (0.55, [0x7f, 0x70, 0xf0]),
    (0.8, [0xb8, 0xac, 0xff]),
    (1.0, [0xf4, 0xf0, 0xff]),
];
const DAY: [(f32, [u8; 3]); 4] =
    [(0.0, [0x1e, 0x17, 0x5c]), (0.45, [0x4a, 0x3b, 0xc4]), (0.8, [0x8a, 0x7e, 0xe6]), (1.0, [0xb8, 0xb0, 0xf0])];
/// Fire, prime and inhibit: night, then day.
const SPARKS: [([u8; 3], [u8; 3]); 3] = [
    ([0x4a, 0xde, 0x80], [0x16, 0xa3, 0x4a]),
    ([0xfb, 0xbf, 0x24], [0xd9, 0x77, 0x06]),
    ([0xfb, 0x71, 0x85], [0xe1, 0x1d, 0x48]),
];

/// How light becomes a glyph and a style, for one terminal.
struct Shade {
    level: Level,
    day: bool,
}

impl Shade {
    fn new(t: &Theme) -> Shade {
        Shade { level: if t.color { t.level() } else { Level::None }, day: t.day() }
    }

    /// A drawn cell's glyph (its index in [`RAMP`], never blank) and style, from its light and
    /// how much of it there is (`alpha`: less where the cord fades out and while the brain fades
    /// in), `thin` steps sparser while it dissolves.
    fn cell(&self, light: f32, alpha: f32, thin: usize) -> (usize, Style) {
        // Facing the key is about 1.1 and a glint a little more; the far side is near 0. The
        // whole ramp spans that, so the lit side is dense and the shadow side sparse.
        let x = ((light - 0.06) / 1.09).clamp(0.0, 1.0);
        // Ink on a day ground is shadow, so there the ramp runs the other way.
        let ink = if self.day { 1.0 - x * 0.92 } else { x };
        let alpha = alpha.clamp(0.0, 1.0);
        let level = ((ink * alpha * 9.0).round() as usize).clamp(1, 9);
        let level = level.saturating_sub(thin).max(1);
        // Less of it is also fainter: towards the shadow at night, towards the paper by day.
        let x = if self.day { 1.0 - (1.0 - x) * alpha } else { x * alpha };
        (level, self.style(level, x))
    }

    /// The style of a cell at `level` with light `x` (0 to 1): a colour from the palette, in as
    /// many colours as the terminal has, or bold and dim alone.
    fn style(&self, level: usize, x: f32) -> Style {
        // Colours a half-step apart: enough for the gradient, few enough that neighbouring
        // cells share one and the frame stays small.
        let q = (x * 18.0).round() / 18.0;
        let rgb = if self.day { ramp(&DAY, q) } else { ramp(&NIGHT, q) };
        match self.level {
            Level::TrueColor => Style::default().fg(Color::Rgb(rgb[0], rgb[1], rgb[2])),
            Level::Ansi256 => Style::default().fg(Color::Indexed(xterm256(rgb))),
            // The user's own colours, by light: blue in shadow up through their magentas, and
            // white for a glint only on a night ground.
            Level::Ansi16 => Style::default().fg(match x {
                x if x < 0.28 => Color::Blue,
                x if x < 0.6 => Color::Magenta,
                x if x < 0.9 || self.day => Color::LightMagenta,
                _ => Color::White,
            }),
            Level::None => match level {
                1..=2 => Style::default().add_modifier(Modifier::DIM),
                9 => Style::default().add_modifier(Modifier::BOLD),
                _ => Style::default(),
            },
        }
    }

    /// A spark's style: its strength's colour, dimmer as it fades.
    fn spark(&self, hue: usize, glow: f32) -> Style {
        let (night, day) = SPARKS[hue.min(2)];
        let base = if self.day { day } else { night };
        let k = 0.45 + 0.55 * glow.clamp(0.0, 1.0);
        let rgb = base.map(|c| (c as f32 * k).round() as u8);
        let style = Style::default().add_modifier(Modifier::BOLD);
        match self.level {
            Level::TrueColor => style.fg(Color::Rgb(rgb[0], rgb[1], rgb[2])),
            Level::Ansi256 => style.fg(Color::Indexed(xterm256(rgb))),
            _ => style.fg([Color::Green, Color::Yellow, Color::Red][hue.min(2)]),
        }
    }
}

/// The colour at `x` along a palette of stops.
fn ramp(stops: &[(f32, [u8; 3])], x: f32) -> [u8; 3] {
    let x = x.clamp(0.0, 1.0);
    let i = stops.iter().rposition(|(at, _)| *at <= x).unwrap_or(0).min(stops.len() - 2);
    let ((a, ca), (b, cb)) = (stops[i], stops[i + 1]);
    let k = ((x - a) / (b - a)).clamp(0.0, 1.0);
    std::array::from_fn(|c| (ca[c] as f32 + (cb[c] as f32 - ca[c] as f32) * k).round() as u8)
}

/// The nearest colour in the xterm-256 palette: its 6×6×6 cube or its grey ramp.
fn xterm256(rgb: [u8; 3]) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let near =
        |v: u8| LEVELS.iter().enumerate().min_by_key(|(_, l)| (**l as i32 - v as i32).abs()).map_or(0, |(i, _)| i);
    let (r, g, b) = (near(rgb[0]), near(rgb[1]), near(rgb[2]));
    let cube = [LEVELS[r], LEVELS[g], LEVELS[b]];
    let mean = (rgb.iter().map(|&v| v as u32).sum::<u32>() / 3) as u8;
    let grey_i = (((mean as i32 - 8).max(0) + 5) / 10).min(23) as u8;
    let grey = 8 + 10 * grey_i;
    let dist = |c: [u8; 3]| -> i32 { (0..3).map(|i| (c[i] as i32 - rgb[i] as i32).pow(2)).sum() };
    if dist([grey; 3]) < dist(cube) {
        232 + grey_i
    } else {
        16 + 36 * r as u8 + 6 * g as u8 + b as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::watch::buffer_text;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    fn shown(c: Conditions) -> Option<Skip> {
        skip(&c)
    }

    fn base(env: &dyn Fn(&str) -> Option<String>) -> Conditions<'_> {
        Conditions { tty: true, env, motion: true, setting: true, from_watch: false, size: (120, 40) }
    }

    #[test]
    fn it_shows_at_a_roomy_terminal_with_nothing_against_it() {
        let e = env(&[("TERM", "xterm-256color")]);
        assert_eq!(shown(base(&e)), None);
        assert_eq!(shown(Conditions { size: (MIN_COLS, MIN_ROWS), ..base(&e) }), None);
    }

    #[test]
    fn it_never_shows_where_it_would_be_in_the_way() {
        let none = env(&[]);
        assert_eq!(shown(Conditions { tty: false, ..base(&none) }), Some(Skip::NotATerminal));
        let agent = env(&[("CLAUDECODE", "1")]);
        assert_eq!(shown(base(&agent)), Some(Skip::AgentSession));
        let empty = env(&[("CLAUDECODE", "")]);
        assert_eq!(shown(base(&empty)), None, "an empty marker is no marker");
        let dumb = env(&[("TERM", "dumb")]);
        assert_eq!(shown(base(&dumb)), Some(Skip::Dumb));
        assert_eq!(shown(Conditions { motion: false, ..base(&none) }), Some(Skip::ReducedMotion));
        assert_eq!(shown(Conditions { setting: false, ..base(&none) }), Some(Skip::Off));
        assert_eq!(shown(Conditions { from_watch: true, ..base(&none) }), Some(Skip::FromWatch));
        assert_eq!(shown(Conditions { size: (MIN_COLS - 1, 40), ..base(&none) }), Some(Skip::TooSmall));
        assert_eq!(shown(Conditions { size: (200, MIN_ROWS - 1), ..base(&none) }), Some(Skip::TooSmall));
    }

    #[test]
    fn only_the_skipping_keys_are_kept_from_the_app() {
        assert!(swallows(KeyCode::Enter) && swallows(KeyCode::Char(' ')) && swallows(KeyCode::Esc));
        for k in [KeyCode::Char('q'), KeyCode::Char('2'), KeyCode::Char(':'), KeyCode::Tab, KeyCode::Down] {
            assert!(!swallows(k), "{k:?} goes on to the app");
        }
    }

    /// A frame `at` seconds in, over a buffer of `~` standing in for the app, so what the start
    /// screen leaves shows.
    fn frame(w: u16, h: u16, at: f32, t: &Theme) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::filled(area, ratatui::buffer::Cell::new("~"));
        Splash::new().draw_at(&mut buf, area, at, t);
        buf
    }

    #[test]
    fn a_frame_mid_turn() {
        let text = buffer_text(&frame(100, 34, 1.2, &Theme::with_color(false)));
        assert!(text.contains("a d a g e") && text.contains("Agent traces become reflexes."));
        assert!(!text.contains('~'), "every cell is the start screen's until it dissolves");
        insta::assert_snapshot!("splash-mid-turn", text);
    }

    #[test]
    fn frames_are_the_same_every_time() {
        let t = Theme::with_level(Level::TrueColor, false);
        assert_eq!(frame(90, 30, 0.8, &t), frame(90, 30, 0.8, &t));
    }

    #[test]
    fn it_turns() {
        let t = Theme::with_color(false);
        assert_ne!(buffer_text(&frame(90, 30, 0.6, &t)), buffer_text(&frame(90, 30, 1.6, &t)));
    }

    #[test]
    fn it_dissolves_into_the_app_and_then_leaves_it_alone() {
        let t = Theme::with_color(false);
        let count = |b: &Buffer| buffer_text(b).matches('~').count();
        let (before, during, after) = (frame(80, 24, 1.0, &t), frame(80, 24, 2.1, &t), frame(80, 24, LENGTH, &t));
        assert_eq!(count(&before), 0);
        assert!(count(&during) > 0 && count(&during) < 80 * 24, "partly dissolved");
        assert_eq!(count(&after), 80 * 24, "gone: the app's frame is untouched");
    }

    #[test]
    fn too_small_a_terminal_is_left_alone() {
        let t = Theme::with_color(false);
        let b = frame(MIN_COLS - 1, 30, 1.0, &t);
        assert_eq!(buffer_text(&b).matches('~').count(), (MIN_COLS as usize - 1) * 30);
    }

    #[test]
    fn colour_follows_the_terminal() {
        let at = |level| {
            let b = frame(90, 30, 1.0, &Theme::with_level(level, false));
            let mut fg: Vec<Color> = vec![];
            for c in b.content() {
                if !fg.contains(&c.fg) {
                    fg.push(c.fg);
                }
            }
            fg
        };
        let rgb = at(Level::TrueColor);
        assert!(rgb.iter().filter(|c| matches!(c, Color::Rgb(..))).count() > 8, "a gradient of truecolor violets");
        assert!(at(Level::Ansi256).iter().all(|c| matches!(c, Color::Indexed(_) | Color::Reset)));
        let none = frame(90, 30, 1.0, &Theme::with_color(false));
        assert!(none.content().iter().all(|c| c.fg == Color::Reset), "NO_COLOR: no colour at all");
    }

    #[test]
    fn ascii_only_terminals_get_ascii() {
        let b = frame(90, 30, 1.0, &Theme::with_color(false).ascii());
        assert!(buffer_text(&b).is_ascii());
    }

    #[test]
    fn the_xterm_palette_is_matched_to_the_nearest() {
        assert_eq!(xterm256([0, 0, 0]), 16);
        assert_eq!(xterm256([255, 255, 255]), 231);
        assert_eq!(xterm256([0x8b, 0x7c, 0xff]), 105);
        assert_eq!(xterm256([128, 128, 128]), 244);
    }

    /// Frames to look at while tuning, with timings:
    /// `SPLASH_SIZE=200x60 cargo test -p adage-cli --release splash::tests::dump -- --ignored --nocapture`
    /// prints text frames; `SPLASH_HTML=<file>` writes them as coloured HTML instead.
    #[test]
    #[ignore]
    fn dump() {
        let size = std::env::var("SPLASH_SIZE").unwrap_or_else(|_| "120x40".into());
        let (w, h) = size.split_once('x').map(|(w, h)| (w.parse().unwrap(), h.parse().unwrap())).unwrap();
        let day = std::env::var("SPLASH_DAY").is_ok();
        let t = Theme::with_level(Level::TrueColor, day);
        let start = Instant::now();
        let mut splash = Splash::new();
        eprintln!("build {:?}", start.elapsed());
        let area = Rect::new(0, 0, w, h);
        let mut html = String::new();
        let times: Vec<f32> = std::env::var("SPLASH_AT")
            .map(|s| s.split(',').map(|x| x.parse().unwrap()).collect())
            .unwrap_or_else(|_| vec![0.2, 0.8, 1.4, 2.0]);
        for at in times {
            let mut buf = Buffer::empty(area);
            let n = 30;
            let t0 = Instant::now();
            for _ in 0..n {
                splash.draw_at(&mut buf, area, at, &t);
            }
            eprintln!("{w}x{h} at {at}s: {:?} a frame", t0.elapsed() / n);
            if std::env::var("SPLASH_HTML").is_ok() {
                let bg = if day { "#f4f3f8" } else { "#0b0b10" };
                html.push_str(&format!("<pre style='font:13px/15.6px Menlo,monospace;background:{bg};color:#888;padding:8px;margin:0 0 8px'>"));
                for y in 0..h {
                    for x in 0..w {
                        let c = &buf[(x, y)];
                        let s = c.symbol().replace('&', "&amp;").replace('<', "&lt;");
                        match c.fg {
                            Color::Rgb(r, g, b) => {
                                html.push_str(&format!("<span style='color:rgb({r},{g},{b})'>{s}</span>"))
                            }
                            _ => html.push_str(&s),
                        }
                    }
                    html.push('\n');
                }
                html.push_str("</pre>");
            } else {
                println!("{}", buffer_text(&buf));
            }
        }
        if let Ok(path) = std::env::var("SPLASH_HTML") {
            std::fs::write(path, format!("<html><body style='margin:0;background:#000'>{html}</body></html>")).unwrap();
        }
        // What a terminal is sent for one frame after another, a frame's time apart.
        struct Count(std::rc::Rc<std::cell::Cell<usize>>);
        impl std::io::Write for Count {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.set(self.0.get() + b.len());
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let bytes = std::rc::Rc::new(std::cell::Cell::new(0));
        let backend = ratatui::backend::CrosstermBackend::new(Count(bytes.clone()));
        let mut term = ratatui::Terminal::with_options(
            backend,
            ratatui::TerminalOptions { viewport: ratatui::Viewport::Fixed(area) },
        )
        .unwrap();
        let mut sent = vec![];
        for i in 0..4 {
            let at = 1.0 + i as f32 * FRAME.as_secs_f32();
            term.draw(|f| splash.draw_at(f.buffer_mut(), area, at, &t)).unwrap();
            sent.push(bytes.get());
        }
        eprintln!("{w}x{h}: about {} bytes a frame", (sent[3] - sent[1]) / 2);
    }
}
