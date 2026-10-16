//! Motion in `adage ui`: brief transitions that say where you went, and never get in the way.
//!
//! * A tab switch eases the new tab in over [`TAB_MS`]: its rows reveal top-down while the content
//!   slides in from two columns to the right, its ink coming up from dim.
//! * A popup (or the palette) grows in from its centre over [`POP_MS`], its border drawn at the
//!   edge of the growing box.
//!
//! Both work on the finished frame (`Buffer` post-processing), so every screen stays a pure
//! function of state and the tests, which draw with `app::draw`, see final frames. With
//! `ADAGE_REDUCED_MOTION=1` nothing moves. The app loop redraws at the start screen's cadence
//! (`splash::FRAME`, 33 ms) only while a transition runs, and idles otherwise.

use super::app::{AppState, Tab};
use super::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::time::{Duration, Instant};

/// How long a tab takes to ease in.
pub const TAB_MS: u64 = 180;
/// How long a popup takes to grow in.
pub const POP_MS: u64 = 140;

/// What overlays the app: which popup (by kind, not content, so a progress bar moving does not
/// grow it again), and whether the palette is open.
type Overlay = (Option<std::mem::Discriminant<super::app::Modal>>, bool);

fn overlay(s: &AppState) -> Overlay {
    (s.modal.as_ref().map(std::mem::discriminant), s.palette.is_some())
}

/// The transitions running, and what was on screen at the last frame.
pub struct Motion {
    enabled: bool,
    tab: Tab,
    over: Overlay,
    tab_at: Option<Instant>,
    pop_at: Option<Instant>,
}

impl Motion {
    /// Motion for the app as it opens on `s` (nothing eases in at the start: the start screen has
    /// its own dissolve). `enabled` is the theme's `motion`.
    pub fn new(s: &AppState, enabled: bool) -> Motion {
        Motion { enabled, tab: s.tab, over: overlay(s), tab_at: None, pop_at: None }
    }

    /// Note what is on screen for the frame about to be drawn: another tab starts the tab's
    /// transition, a popup that was not open starts the popup's.
    pub fn observe(&mut self, s: &AppState, now: Instant) {
        let over = overlay(s);
        if self.enabled {
            if s.tab != self.tab && over == (None, false) {
                self.tab_at = Some(now);
            }
            let opened = (over.0.is_some() && over.0 != self.over.0) || (over.1 && !self.over.1);
            if opened {
                self.pop_at = Some(now);
            }
        }
        self.tab = s.tab;
        self.over = over;
        let done = |at: &mut Option<Instant>, ms: u64| {
            if at.is_some_and(|a| now.saturating_duration_since(a) >= Duration::from_millis(ms)) {
                *at = None;
            }
        };
        done(&mut self.tab_at, TAB_MS);
        done(&mut self.pop_at, POP_MS);
    }

    /// Whether a transition is running (the loop then redraws at the fast cadence).
    pub fn active(&self) -> bool {
        self.tab_at.is_some() || self.pop_at.is_some()
    }

    /// Whether a popup is growing in (the loop then keeps the frame from before it is drawn).
    pub fn growing(&self) -> bool {
        self.pop_at.is_some()
    }

    /// Ease the tab's content (`area`) in, on the frame drawn without popups.
    pub fn tab_in(&self, buf: &mut Buffer, area: Rect, now: Instant) {
        if let Some(at) = self.tab_at {
            reveal(buf, area, progress(at, now, TAB_MS));
        }
    }

    /// Grow the popup in: `before` is the frame before the popup was drawn on it.
    pub fn pop_in(&self, buf: &mut Buffer, before: &Buffer, now: Instant, t: &Theme) {
        if let Some(at) = self.pop_at {
            grow(buf, before, progress(at, now, POP_MS), t);
        }
    }
}

/// How far a transition that started `at` and lasts `ms` has come: 0.0 to 1.0.
fn progress(at: Instant, now: Instant, ms: u64) -> f32 {
    (now.saturating_duration_since(at).as_secs_f32() * 1000.0 / ms as f32).clamp(0.0, 1.0)
}

/// Ease out (cubic): fast at first, settling at the end.
pub fn ease(p: f32) -> f32 {
    1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3)
}

/// The tab's ease-in at `p`: rows below the reveal line are blank, the content sits up to two
/// columns to the right of its place, and it is dim for the first half.
pub fn reveal(buf: &mut Buffer, area: Rect, p: f32) {
    let area = area.intersection(buf.area);
    if p >= 1.0 || area.is_empty() {
        return;
    }
    let e = ease(p);
    // Every row shows a little before the end, so the last frames are the content settling.
    let shown = ((e * 1.25).min(1.0) * area.height as f32).ceil() as u16;
    let shift = ((1.0 - e) * 2.0).round() as u16;
    let dim = e < 0.55;
    for y in area.top()..area.bottom() {
        if y >= area.y + shown {
            for x in area.left()..area.right() {
                buf[(x, y)].reset();
            }
            continue;
        }
        if shift > 0 {
            // Right to left, so each cell moves before the one it lands on is overwritten.
            for x in (area.left()..area.right()).rev() {
                if x >= area.x + shift {
                    let from = buf[(x - shift, y)].clone();
                    buf[(x, y)] = from;
                } else {
                    buf[(x, y)].reset();
                }
            }
        }
        if dim {
            for x in area.left()..area.right() {
                let c = &mut buf[(x, y)];
                c.modifier.insert(Modifier::DIM);
            }
        }
    }
}

/// The popup's grow-in at `p`: the cells that changed from `before` (the popup) show only inside a
/// box that grows from 60% of the popup's size to all of it, with the border at its edge; outside
/// it, the frame from before.
pub fn grow(buf: &mut Buffer, before: &Buffer, p: f32, t: &Theme) {
    if p >= 1.0 || before.area != buf.area {
        return;
    }
    // The popup: the box around every cell it changed.
    let (mut x0, mut y0, mut x1, mut y1) = (u16::MAX, u16::MAX, 0u16, 0u16);
    let a = buf.area;
    for y in a.top()..a.bottom() {
        for x in a.left()..a.right() {
            if buf[(x, y)] != before[(x, y)] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x0 > x1 || y0 > y1 {
        return;
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let s = 0.6 + 0.4 * ease(p);
    let gw = ((w as f32 * s).round() as u16).clamp(2.min(w), w);
    let gh = ((h as f32 * s).round() as u16).clamp(2.min(h), h);
    let g = Rect { x: x0 + (w - gw) / 2, y: y0 + (h - gh) / 2, width: gw, height: gh };
    for y in y0..=y1 {
        for x in x0..=x1 {
            let inside = x >= g.left() && x < g.right() && y >= g.top() && y < g.bottom();
            if !inside {
                buf[(x, y)] = before[(x, y)].clone();
            }
        }
    }
    if gw < 2 || gh < 2 {
        return;
    }
    let b = t.border();
    let style = t.accent();
    let (l, r, top, bot) = (g.left(), g.right() - 1, g.top(), g.bottom() - 1);
    for x in l..=r {
        buf[(x, top)].set_symbol(b.horizontal_top).set_style(style);
        buf[(x, bot)].set_symbol(b.horizontal_bottom).set_style(style);
    }
    for y in top..=bot {
        buf[(l, y)].set_symbol(b.vertical_left).set_style(style);
        buf[(r, y)].set_symbol(b.vertical_right).set_style(style);
    }
    buf[(l, top)].set_symbol(b.top_left);
    buf[(r, top)].set_symbol(b.top_right);
    buf[(l, bot)].set_symbol(b.bottom_left);
    buf[(r, bot)].set_symbol(b.bottom_right);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    fn text(b: &Buffer) -> Vec<String> {
        (0..b.area.height).map(|y| (0..b.area.width).map(|x| b[(x, y)].symbol()).collect()).collect()
    }

    #[test]
    fn easing_runs_from_zero_to_one_and_settles() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!(ease(0.5) > 0.5, "fast first");
        assert!(ease(2.0) <= 1.0 && ease(-1.0) >= 0.0);
    }

    #[test]
    fn a_tab_reveals_top_down_and_lands_in_place() {
        let area = Rect::new(0, 0, 10, 4);
        let mut full = Buffer::empty(area);
        for y in 0..4 {
            full.set_string(0, y, format!("row{y}abcde"), Style::default());
        }
        let mut start = full.clone();
        reveal(&mut start, area, 0.0);
        assert!(text(&start).iter().all(|r| r.trim().is_empty()), "nothing yet: {:?}", text(&start));
        let mut mid = full.clone();
        reveal(&mut mid, area, 0.05);
        let rows = text(&mid);
        assert!(rows[0].starts_with("  row0"), "shifted right: {rows:?}");
        assert!(rows[3].trim().is_empty(), "lower rows wait: {rows:?}");
        assert!(mid[(3, 0)].modifier.contains(Modifier::DIM), "ink comes up from dim");
        let mut end = full.clone();
        reveal(&mut end, area, 1.0);
        assert_eq!(end, full, "the last frame is the frame");
    }

    #[test]
    fn a_popup_grows_from_its_centre_and_ends_whole() {
        let area = Rect::new(0, 0, 20, 10);
        let mut before = Buffer::empty(area);
        before.set_string(0, 0, "x".repeat(20), Style::default());
        let mut after = before.clone();
        for y in 2..8 {
            after.set_string(4, y, "#".repeat(12), Style::default());
        }
        let t = Theme::with_color(false);
        let mut small = after.clone();
        grow(&mut small, &before, 0.0, &t);
        let rows = text(&small);
        assert!(rows[2].trim().is_empty() || !rows[2].contains("############"), "{rows:?}");
        assert!(rows.iter().any(|r| r.contains('╭')), "the growing box has its border: {rows:?}");
        let mut whole = after.clone();
        grow(&mut whole, &before, 1.0, &t);
        assert_eq!(whole, after);
    }
}
