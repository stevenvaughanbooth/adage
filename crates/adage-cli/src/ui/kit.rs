//! The design kit of `adage ui`: the few shapes every screen is built from, so the tabs read as one
//! product instead of a wall of text. Each piece is a pure function (or a small builder) from data
//! and a [`Theme`] to ratatui lines, spans or cells; nothing here reads files or holds state.
//!
//! Every piece honours the theme exactly as the rest of the UI does: the colour depth (truecolor,
//! 256, 16 or none under `NO_COLOR`), `ADAGE_ASCII` glyphs, and `ADAGE_REDUCED_MOTION`. Nothing
//! paints a background: emphasis (a chip, a keycap) is the terminal's own reverse video, so the
//! user's palette stays in charge.
//!
//! The spacing rules, so screens line up with each other:
//!
//! * a tab's content starts [`PAD`] column in; a card's content one column inside its border;
//! * one blank line between sections, none between a section's header and its first row;
//! * a list row is `lamp  label  short value`; the full text belongs to the selected row's
//!   detail card, wrapped, never clipped there;
//! * the plain words come first and the detail on demand (a card, `enter`); ids and hashes never
//!   lead a row.
//!
//! ```ignore
//! use crate::ui::kit::{self, Card, KeyValue, Section, Tone};
//! let mut lines = vec![Section::new("Hooks & delivery").count(4).line(width, t)];
//! lines.push(kit::row(Lamp::Ok, "Claude Code", "hooks received", width, false, t));
//! let kv = KeyValue::new().row("why", "Codex has not sent anything yet").lines(inner_w, t);
//! Card::new("Codex").tone(Tone::Warn).hints(&[("enter", "fix")]).render(f, area, kv, t);
//! ```

use super::theme::{Lamp, Theme};
use adage_term::Glyph;
use adage_tracks::Strength;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Columns between the screen's edge and a tab's content.
pub const PAD: usize = 1;

// ---------------------------------------------------------------------------------------------
// Tone

/// What a chip, lamp, meter or card border means. Pick by meaning; the theme picks the colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// The terminal's own ink.
    Plain,
    /// Tertiary: labels, rules, the quiet.
    Muted,
    /// The one accent: focus, the active thing.
    Accent,
    /// Fine, done, fire.
    Ok,
    /// Worth a look, prime.
    Warn,
    /// Broken, inhibit.
    Fail,
}

impl Tone {
    /// The tone's style (`Plain` is the default style).
    ///
    /// ```ignore
    /// Span::styled("3 passed", Tone::Ok.style(t))
    /// ```
    pub fn style(self, t: &Theme) -> Style {
        match self {
            Tone::Plain => Style::default(),
            Tone::Muted => t.dim(),
            Tone::Accent => t.accent(),
            Tone::Ok => t.green(),
            Tone::Warn => t.yellow(),
            Tone::Fail => t.red(),
        }
    }

    /// The tone of a health lamp.
    pub fn of_lamp(l: Lamp) -> Tone {
        match l {
            Lamp::Ok => Tone::Ok,
            Lamp::Warn => Tone::Warn,
            Lamp::Fail => Tone::Fail,
            Lamp::Off => Tone::Muted,
        }
    }

    /// The tone of a strength: fire is ok, prime a warning colour, inhibit a failure colour.
    pub fn of_strength(s: Strength) -> Tone {
        match s {
            Strength::Fire => Tone::Ok,
            Strength::Prime => Tone::Warn,
            Strength::Inhibit => Tone::Fail,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Measuring, clipping, wrapping

/// Display columns of `s` (wide characters count two).
///
/// ```ignore
/// assert_eq!(kit::width("enter"), 5);
/// ```
pub fn width(s: &str) -> usize {
    if s.is_ascii() {
        s.len()
    } else {
        Span::raw(s).width()
    }
}

/// Display columns of a run of spans.
pub fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.width()).sum()
}

fn char_width(c: char) -> usize {
    if c.is_ascii() {
        1
    } else {
        Span::raw(c.to_string()).width()
    }
}

/// `s` cut to `n` columns, ending in the theme's ellipsis (`…`, or `...`) when it was cut.
///
/// ```ignore
/// assert_eq!(kit::clip("embedding model", 9, t), "embeddin…");
/// ```
pub fn clip(s: &str, n: usize, t: &Theme) -> String {
    t.fit(s, n)
}

/// Styled `spans` cut to `n` columns. The span where the room runs out ends in the ellipsis;
/// trailing padding is dropped before anything is cut.
pub fn clip_spans(spans: Vec<Span<'static>>, n: usize, t: &Theme) -> Vec<Span<'static>> {
    if spans_width(&spans) <= n {
        return spans;
    }
    let mut out = Vec::new();
    let mut used = 0;
    for mut s in spans {
        let w = s.width();
        if used + w <= n {
            used += w;
            out.push(s);
            continue;
        }
        let room = n.saturating_sub(used);
        let trimmed = s.content.trim_end().to_string();
        s.content = if width(&trimmed) <= room { trimmed } else { t.fit(&trimmed, room) }.into();
        if !s.content.is_empty() {
            out.push(s);
        }
        break;
    }
    out
}

/// The words of `text` filled into lines of `width` columns. A line never breaks inside a word,
/// with one exception: a single word longer than a whole line (a long path) breaks after a `/`,
/// `-`, `_` or `.` where it can, else at the edge.
///
/// ```ignore
/// assert_eq!(kit::wrap("one two three", 7), ["one two", "three"]);
/// ```
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    wrap_spans(vec![Span::raw(text.to_string())], width, 0)
        .into_iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
        .collect()
}

/// Styled `spans` wrapped to `width` columns, each continuation line indented `hang` columns (a
/// hanging indent: `why  Codex is set up but` / `     hasn't sent anything`). The first line's own
/// leading spaces stay. Words are never broken except as [`wrap`] says; the spaces at a break are
/// dropped, and the styles of every character are kept.
///
/// ```ignore
/// let lines = kit::wrap_spans(vec![Span::raw("why  "), Span::raw("a long reason")], 12, 5);
/// // "why  a long" / "     reason"
/// ```
pub fn wrap_spans(spans: Vec<Span<'static>>, width: usize, hang: usize) -> Vec<Line<'static>> {
    let cells: Vec<(char, Style)> = spans.iter().flat_map(|s| s.content.chars().map(move |c| (c, s.style))).collect();
    if width < 2 || cells.iter().map(|(c, _)| char_width(*c)).sum::<usize>() <= width {
        return vec![Line::from(spans)];
    }
    let hang = hang.min(width / 2);
    let lead = cells.iter().take_while(|(c, _)| *c == ' ').count().min(width / 2);
    let mut rows: Vec<Vec<(char, Style)>> = vec![];
    let mut cur: Vec<(char, Style)> = cells[..lead].to_vec();
    let mut cur_w = lead;
    let mut fresh = true; // nothing but the indent on this row yet
    let mut i = lead;
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
        let spaces = &cells[i..word_at];
        let word = &cells[word_at..j];
        i = j;
        if word.is_empty() {
            break; // trailing spaces
        }
        let ww: usize = word.iter().map(|(c, _)| char_width(*c)).sum();
        let gap = if fresh { 0 } else { spaces.len() };
        if !fresh && cur_w + gap + ww > width {
            rows.push(std::mem::take(&mut cur));
            cur = vec![(' ', Style::default()); hang];
            cur_w = hang;
            fresh = true;
        }
        if !fresh {
            cur.extend_from_slice(spaces);
            cur_w += spaces.len();
        }
        if cur_w + ww <= width {
            cur.extend_from_slice(word);
            cur_w += ww;
            fresh = false;
            continue;
        }
        // Longer than a whole line: break after a separator where one fits, else at the edge.
        let mut rest = word;
        while !rest.is_empty() {
            let room = width.saturating_sub(cur_w).max(1);
            let mut fit = 0;
            let mut w = 0;
            for (c, _) in rest {
                let cw = char_width(*c);
                if w + cw > room {
                    break;
                }
                w += cw;
                fit += 1;
            }
            let fit = fit.max(1);
            let cut = if fit < rest.len() {
                rest[..fit].iter().rposition(|(c, _)| matches!(c, '/' | '-' | '_' | '.')).map_or(fit, |p| p + 1)
            } else {
                fit
            };
            cur.extend_from_slice(&rest[..cut]);
            cur_w += rest[..cut].iter().map(|(c, _)| char_width(*c)).sum::<usize>();
            rest = &rest[cut..];
            if !rest.is_empty() {
                rows.push(std::mem::take(&mut cur));
                cur = vec![(' ', Style::default()); hang];
                cur_w = hang;
            }
        }
        fresh = false;
    }
    rows.push(cur);
    rows.into_iter().map(cells_line).collect()
}

/// [`wrap_spans`] over every line, each continuing under its own leading spaces plus `hang`.
pub fn wrap_lines(lines: Vec<Line<'static>>, width: usize, hang: usize) -> Vec<Line<'static>> {
    let mut out = Vec::with_capacity(lines.len());
    for l in lines {
        let lead = l.spans.iter().flat_map(|s| s.content.chars()).take_while(|c| *c == ' ').count();
        out.extend(wrap_spans(l.spans, width, lead + hang));
    }
    out
}

/// A row of styled characters as a line of spans, one span per run of one style.
fn cells_line(row: Vec<(char, Style)>) -> Line<'static> {
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
    if !text.is_empty() {
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

/// The first clause of a sentence, for a list row's short value: up to the first `;`, `:` (not
/// in a path or a time), ` — ` or `. `, without a trailing comma. The whole sentence is the
/// detail card's.
///
/// ```ignore
/// assert_eq!(kit::first_clause("on; 0 bound, 8 packs"), "on");
/// ```
pub fn first_clause(s: &str) -> &str {
    let b = s.as_bytes();
    let mut end = s.len();
    for (i, c) in s.char_indices() {
        let next = b.get(i + 1).copied();
        let cut = match c {
            ';' => true,
            ':' | '.' => next == Some(b' '),
            '—' => true,
            _ => false,
        };
        if cut && i > 0 {
            end = i;
            break;
        }
    }
    s[..end].trim().trim_end_matches(',')
}

// ---------------------------------------------------------------------------------------------
// Headers and cards

/// A section header: a bold title, an optional count and note, and a thin rule to the edge.
///
/// ```ignore
/// lines.push(Section::new("Hooks & delivery").count(4).line(width, t));
/// //  Hooks & delivery  4 ─────────────────────────────
/// lines.push(Section::new("Learning").note("1 to look at", Tone::Warn).line(width, t));
/// //  Learning  · 1 to look at ────────────────────────
/// ```
#[derive(Debug, Clone, Default)]
pub struct Section {
    title: String,
    count: Option<String>,
    note: Option<(String, Tone)>,
    indent: usize,
}

impl Section {
    pub fn new(title: impl Into<String>) -> Section {
        Section { title: title.into(), count: None, note: None, indent: PAD }
    }

    /// A count after the title, dim: how many rows the section holds.
    pub fn count(mut self, n: usize) -> Section {
        self.count = Some(n.to_string());
        self
    }

    /// A note after the count, in its tone: `· 1 to look at`.
    pub fn note(mut self, text: impl Into<String>, tone: Tone) -> Section {
        self.note = Some((text.into(), tone));
        self
    }

    /// Columns before the title (default [`PAD`]).
    pub fn indent(mut self, n: usize) -> Section {
        self.indent = n;
        self
    }

    /// The header as one line `width` columns wide.
    pub fn line(&self, width: u16, t: &Theme) -> Line<'static> {
        let mut spans = vec![Span::raw(" ".repeat(self.indent)), Span::styled(self.title.clone(), t.bold())];
        if let Some(c) = &self.count {
            spans.push(Span::styled(format!("  {c}"), t.dim()));
        }
        if let Some((n, tone)) = &self.note {
            spans.push(Span::styled(t.sep(), t.dim()));
            spans.push(Span::styled(n.clone(), tone.style(t)));
        }
        let used = spans_width(&spans);
        let rest = (width as usize).saturating_sub(used + 2);
        if rest > 0 {
            spans.push(Span::styled(format!(" {}", t.ch("─", "-").repeat(rest)), t.rule()));
        }
        Line::from(clip_spans(spans, width as usize, t))
    }
}

/// A card: a rounded border (`+-|` in ASCII) with a title, its content one column in, and an
/// optional footer of key hints on the bottom border. Its tone colours the border and title.
///
/// ```ignore
/// let lines = KeyValue::new().row("why", why).lines(Card::inner_width(area), t);
/// Card::new("Codex").tone(Tone::Warn).hints(&[("enter", "fix")]).render(f, area, lines, t);
/// // ╭─ Codex ─────────────────────────╮
/// // │ why  Codex hasn't sent anything │
/// // ╰─────────────────── enter fix ───╯
/// ```
#[derive(Debug, Clone, Default)]
pub struct Card {
    title: String,
    tone: Option<Tone>,
    hints: Vec<(String, String)>,
    note: Option<String>,
}

impl Card {
    pub fn new(title: impl Into<String>) -> Card {
        Card { title: title.into(), ..Card::default() }
    }

    /// The border's tone (default: a dim border, a bold title).
    pub fn tone(mut self, tone: Tone) -> Card {
        self.tone = Some(tone);
        self
    }

    /// Key hints on the bottom border: `[("enter", "fix"), ("esc", "close")]`.
    pub fn hints(mut self, hints: &[(&str, &str)]) -> Card {
        self.hints = hints.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect();
        self
    }

    /// A dim note at the right of the top border (`2 of 5`, `same as adage doctor`).
    pub fn note(mut self, note: impl Into<String>) -> Card {
        self.note = Some(note.into());
        self
    }

    /// Columns for content inside a card drawn in `area`: two for the border, two for padding.
    pub fn inner_width(area: Rect) -> usize {
        (area.width as usize).saturating_sub(4)
    }

    /// The card's rows for `content_rows` of content (its two borders added).
    pub fn height(content_rows: usize) -> u16 {
        (content_rows as u16).saturating_add(2)
    }

    /// The bordered block, for drawing a card's content yourself.
    pub fn block(&self, t: &Theme) -> Block<'static> {
        let border = self.border_style(t);
        let title = self.title_style(t);
        // Titles sit one rule-dash in from the corners, as [`Card::lines`] draws them: `╭─ Title`.
        let (top, bottom) = (t.border().horizontal_top, t.border().horizontal_bottom);
        let mut b = Block::default().borders(Borders::ALL).border_set(t.border()).border_style(border);
        if !self.title.is_empty() {
            b = b.title(Line::from(vec![
                Span::styled(top, border),
                Span::styled(format!(" {} ", t.text(&self.title)), title),
            ]));
        }
        if let Some(n) = &self.note {
            b = b.title(
                Line::from(vec![Span::styled(format!(" {} ", t.text(n)), t.dim()), Span::styled(top, border)])
                    .right_aligned(),
            );
        }
        if !self.hints.is_empty() {
            let pairs: Vec<(&str, &str)> = self.hints.iter().map(|(k, l)| (k.as_str(), l.as_str())).collect();
            let mut spans = vec![Span::raw(" ")];
            spans.extend(hints(&pairs, t, usize::MAX));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(bottom, border));
            b = b.title_bottom(Line::from(spans).right_aligned());
        }
        b
    }

    /// The card as text lines `width` columns wide, for a card inside a scrolling list: the same
    /// border, title, note and hints as [`Card::render`], `body` wrapped inside under its own
    /// indent and cut with an ellipsis rather than overflow.
    ///
    /// ```ignore
    /// lines.extend(Card::new("Codex").tone(Tone::Warn).lines(vec![Line::raw("hasn't sent anything")], 50, t));
    /// ```
    pub fn lines(&self, body: Vec<Line<'static>>, width: usize, t: &Theme) -> Vec<Line<'static>> {
        let b = t.border();
        let border = self.border_style(t);
        let width = width.max(8);
        let inner = width - 4;
        let edge = |l: &str, fill: &str, r: &str, mut mid: Vec<Span<'static>>, right: Vec<Span<'static>>| {
            let room = width.saturating_sub(4);
            let right_w = spans_width(&right).min(room);
            mid = clip_spans(mid, room.saturating_sub(right_w), t);
            let gap = room.saturating_sub(spans_width(&mid) + right_w);
            let mut v = vec![Span::styled(format!("{l}{fill}"), border)];
            v.extend(mid);
            v.push(Span::styled(fill.repeat(gap), border));
            v.extend(clip_spans(right, room, t));
            v.push(Span::styled(format!("{fill}{r}"), border));
            Line::from(v)
        };
        let title: Vec<Span<'static>> = if self.title.is_empty() {
            vec![]
        } else {
            vec![Span::styled(format!(" {} ", t.text(&self.title)), self.title_style(t))]
        };
        let note: Vec<Span<'static>> =
            self.note.iter().map(|n| Span::styled(format!(" {} ", t.text(n)), t.dim())).collect();
        let mut out = vec![edge(b.top_left, b.horizontal_top, b.top_right, title, note)];
        for l in wrap_lines(t.lines(body), inner, 0) {
            let mut v = vec![Span::styled(format!("{} ", b.vertical_left), border)];
            let content = clip_spans(l.spans, inner, t);
            let pad = inner.saturating_sub(spans_width(&content));
            v.extend(content);
            v.push(Span::raw(" ".repeat(pad)));
            v.push(Span::styled(format!(" {}", b.vertical_right), border));
            out.push(Line::from(v));
        }
        let pairs: Vec<(&str, &str)> = self.hints.iter().map(|(k, l)| (k.as_str(), l.as_str())).collect();
        let mut hint = hints(&pairs, t, inner.saturating_sub(2));
        if !hint.is_empty() {
            hint.insert(0, Span::raw(" "));
            hint.push(Span::raw(" "));
        }
        out.push(edge(b.bottom_left, b.horizontal_bottom, b.bottom_right, vec![], hint));
        out
    }

    fn border_style(&self, t: &Theme) -> Style {
        self.tone.map_or(t.rule(), |tone| if tone == Tone::Plain { t.rule() } else { tone.style(t) })
    }

    fn title_style(&self, t: &Theme) -> Style {
        self.tone.map_or(t.bold(), |tone| tone.style(t).patch(t.bold()))
    }

    /// Draw the card over `area` (clearing it first) with `lines` inside, wrapped under their own
    /// indent. Returns the area the content took.
    pub fn render(&self, f: &mut Frame, area: Rect, lines: Vec<Line<'static>>, t: &Theme) -> Rect {
        f.render_widget(Clear, area);
        let block = self.block(t);
        let inner = block.inner(area).inner(Margin { horizontal: 1, vertical: 0 });
        f.render_widget(block, area);
        let lines = wrap_lines(t.lines(lines), inner.width as usize, 0);
        f.render_widget(Paragraph::new(lines), inner);
        inner
    }
}

/// Rows of `label  value`: labels dim in one column, values wrapped under themselves (a hanging
/// indent), so a long value never runs back under its label.
///
/// ```ignore
/// let lines = KeyValue::new()
///     .row("why", "Codex is set up but hasn't sent anything yet")
///     .row("fix", vec![Span::styled("adage init", t.command())])
///     .lines(40, t);
/// // why  Codex is set up but hasn't sent
/// //      anything yet
/// // fix  adage init
/// ```
#[derive(Debug, Clone, Default)]
pub struct KeyValue {
    rows: Vec<(String, Vec<Span<'static>>)>,
    indent: usize,
    label_w: Option<usize>,
}

impl KeyValue {
    pub fn new() -> KeyValue {
        KeyValue::default()
    }

    /// One row. The value is a string (plain ink) or styled spans.
    pub fn row(mut self, label: impl Into<String>, value: impl Into<Line<'static>>) -> KeyValue {
        self.rows.push((label.into(), value.into().spans));
        self
    }

    /// Columns before each label.
    pub fn indent(mut self, n: usize) -> KeyValue {
        self.indent = n;
        self
    }

    /// The label column's width (default: the longest label).
    pub fn label_width(mut self, n: usize) -> KeyValue {
        self.label_w = Some(n);
        self
    }

    /// The rows, wrapped to `width` columns.
    pub fn lines(&self, width: usize, t: &Theme) -> Vec<Line<'static>> {
        let lw = self.label_w.unwrap_or_else(|| self.rows.iter().map(|(l, _)| width_of(l)).max().unwrap_or(0));
        let mut out = Vec::new();
        for (label, value) in &self.rows {
            let mut spans = vec![Span::raw(" ".repeat(self.indent)), Span::styled(t.pad(label, lw + 2), t.dim())];
            spans.extend(value.iter().cloned());
            out.extend(wrap_spans(spans, width, self.indent + lw + 2));
        }
        out
    }
}

fn width_of(s: &str) -> usize {
    width(s)
}

// ---------------------------------------------------------------------------------------------
// Chips, badges, lamps, keycaps

/// A chip: a short word in a pill of its tone (reverse video and bold), ` new `. Without colour
/// it is the same pill in the terminal's own reverse.
///
/// ```ignore
/// spans.push(kit::chip("fix", Tone::Warn, t));
/// ```
pub fn chip(text: &str, tone: Tone, t: &Theme) -> Span<'static> {
    Span::styled(format!(" {} ", t.text(text)), tone.style(t).add_modifier(Modifier::REVERSED | Modifier::BOLD))
}

/// A count badge, as the tab bar shows what waits on a tab: ` 2 ` in a pill of its tone.
///
/// ```ignore
/// if waiting > 0 { spans.push(kit::badge(waiting, Tone::Warn, t)); }
/// ```
pub fn badge(n: usize, tone: Tone, t: &Theme) -> Span<'static> {
    chip(&n.to_string(), tone, t)
}

/// A health lamp's mark in its colour: `✓`, `!`, `✗`, `○` (`+ ! x -` without colour). The
/// shape carries the state as well as the colour, so nothing is lost without colour.
pub fn lamp(l: Lamp, t: &Theme) -> Span<'static> {
    Span::styled(t.lamp_mark(l).to_string(), t.lamp(l))
}

/// A strength's lamp and word: `● fire` in colour, `FIRE` without.
pub fn strength(s: Strength, t: &Theme) -> Vec<Span<'static>> {
    if t.color {
        vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.strength(s)),
            Span::styled(Theme::strength_word(s).to_string(), t.strength(s)),
        ]
    } else {
        vec![Span::styled(Theme::strength_word(s).to_ascii_uppercase(), t.bold())]
    }
}

/// A keycap: the key in an inverted chip, ` enter `.
pub fn keycap(key: &str, t: &Theme) -> Span<'static> {
    let base = if t.color { t.ink2() } else { Style::default() };
    Span::styled(format!(" {} ", t.text(key)), base.add_modifier(Modifier::REVERSED))
}

/// Key hints: each key's cap and what it does, ` enter  fix   R  recheck`, as many as fit in
/// `width` columns (whole hints only).
///
/// ```ignore
/// spans.extend(kit::hints(&[("enter", "fix"), ("R", "recheck")], t, 60));
/// ```
pub fn hints(pairs: &[(&str, &str)], t: &Theme, width: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0;
    for (key, label) in pairs {
        let gap = if out.is_empty() { 0 } else { 2 };
        let w = width_of(&t.text(key)) + 2 + 1 + width_of(label);
        if used + gap + w > width {
            break;
        }
        if gap > 0 {
            out.push(Span::raw("  "));
        }
        out.push(keycap(key, t));
        out.push(Span::styled(format!(" {label}"), t.dim()));
        used += gap + w;
    }
    out
}

/// A list row: pointer, lamp, label in a column `label_w` wide, then a short value, cut with an
/// ellipsis at `width`. The selected row is bold, with the accent bar at its left.
///
/// ```ignore
/// lines.push(kit::row(Lamp::Warn, "Codex", "set up, not heard from yet", 20, 80, true, t));
/// // ▌ ! Codex               set up, not heard from yet
/// ```
pub fn row(
    l: Lamp,
    label: &str,
    value: &str,
    label_w: usize,
    width: usize,
    selected: bool,
    t: &Theme,
) -> Line<'static> {
    let label_style = if selected { t.bold() } else { Style::default() };
    let spans = vec![
        Span::styled(if selected { t.ch("▌", ">") } else { " " }, t.accent()),
        Span::raw(" "),
        lamp(l, t),
        Span::raw(" "),
        Span::styled(t.pad(&t.text(label), label_w), label_style),
        Span::styled(t.text(value), t.ink2()),
    ];
    Line::from(clip_spans(spans, width, t))
}

// ---------------------------------------------------------------------------------------------
// Meters, stats, tiles, the spinner

/// The cells of a meter `width` columns wide filled to `ratio` (clamped to 0..=1; NaN is 0):
/// full cells, then one half cell where the fill ends mid-cell, then the track. In ASCII the half
/// cell rounds.
pub fn meter_cells(ratio: f64, width: usize, unicode: bool) -> (usize, bool, usize) {
    let r = if ratio.is_nan() { 0.0 } else { ratio.clamp(0.0, 1.0) };
    let halves = (r * width as f64 * 2.0).round() as usize;
    let (full, half) = if unicode { (halves / 2, halves % 2 == 1) } else { (halves.div_ceil(2).min(width), false) };
    let track = width.saturating_sub(full + usize::from(half));
    (full, half, track)
}

/// A meter: `━━━━━╸────`, the fill in its tone and the track dim; `=====-----` in ASCII.
///
/// ```ignore
/// spans.extend(kit::meter(0.55, 10, Tone::Ok, t)); // ━━━━━╸────
/// ```
pub fn meter(ratio: f64, width: usize, tone: Tone, t: &Theme) -> Vec<Span<'static>> {
    let unicode = t.unicode();
    let (full, half, track) = meter_cells(ratio, width, unicode);
    let fill = if tone == Tone::Plain { t.accent() } else { tone.style(t) };
    let mut v = vec![Span::styled(t.ch("━", "=").repeat(full), fill)];
    if half {
        v.push(Span::styled("╸", fill));
    }
    v.push(Span::styled(t.ch("─", "-").repeat(track), t.rule()));
    v.retain(|s| !s.content.is_empty());
    v
}

/// A sparkline of `values`, one cell each: `▁▂▄█`, or ` .:#` in ASCII. All zeros draw the floor.
///
/// ```ignore
/// assert_eq!(kit::spark(&[0, 1, 2, 4], t), "▁▃▅█");
/// ```
pub fn spark(values: &[u64], t: &Theme) -> String {
    let unicode = t.unicode();
    let bars: &[char] =
        if unicode { &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'] } else { &['_', '.', ':', '|', '#'] };
    let max = values.iter().copied().max().unwrap_or(0);
    values
        .iter()
        .map(|&v| {
            if max == 0 || v == 0 {
                bars[0]
            } else {
                let i = ((v as f64 / max as f64) * (bars.len() - 1) as f64).round() as usize;
                bars[i.clamp(1, bars.len() - 1)]
            }
        })
        .collect()
}

/// One stat: a value and what it counts, in a tone.
#[derive(Debug, Clone)]
pub struct Stat {
    pub value: String,
    pub label: String,
    pub tone: Tone,
}

impl Stat {
    pub fn new(value: impl ToString, label: impl Into<String>, tone: Tone) -> Stat {
        Stat { value: value.to_string(), label: label.into(), tone }
    }
}

/// Stats in a row: the value bold in its tone, its label dim, `·` between: `24 passed · 1 to look at`.
///
/// ```ignore
/// spans.extend(kit::stat_chips(&[Stat::new(24, "passed", Tone::Ok)], t));
/// ```
pub fn stat_chips(stats: &[Stat], t: &Theme) -> Vec<Span<'static>> {
    let mut v = Vec::new();
    for (i, s) in stats.iter().enumerate() {
        if i > 0 {
            v.push(Span::styled(t.sep(), t.dim()));
        }
        let style = if s.tone == Tone::Plain { t.bold() } else { s.tone.style(t).patch(t.bold()) };
        v.push(Span::styled(s.value.clone(), style));
        v.push(Span::styled(format!(" {}", s.label), t.ink2()));
    }
    v
}

/// The braille spinner's frame at `ms` milliseconds (`|/-\` in ASCII); a still dot with reduced
/// motion.
///
/// ```ignore
/// Span::styled(kit::spinner(elapsed.as_millis(), t), t.accent())
/// ```
pub fn spinner(ms: u128, t: &Theme) -> &'static str {
    if !t.motion {
        return t.glyph(Glyph::Active);
    }
    const BRAILLE: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    const ASCII: [&str; 4] = ["|", "/", "-", "\\"];
    if t.unicode() {
        BRAILLE[(ms / 80) as usize % BRAILLE.len()]
    } else {
        ASCII[(ms / 120) as usize % ASCII.len()]
    }
}

/// Big digits, three rows tall, for a stat tile's number: `0`–`9`, `.`, `-`, `%`, `/` and space
/// (any other character keeps its own shape on the middle row). `None` in ASCII, which has no
/// blocks to draw them with.
///
/// ```ignore
/// if let Some(rows) = kit::big(&n.to_string(), t) { /* three lines */ }
/// ```
pub fn big(s: &str, t: &Theme) -> Option<[String; 3]> {
    if !t.unicode() {
        return None;
    }
    let mut rows = [String::new(), String::new(), String::new()];
    for (i, c) in s.chars().enumerate() {
        let g: [&str; 3] = match c {
            '0' => ["█▀█", "█ █", "▀▀▀"],
            '1' => ["▀█ ", " █ ", "▀▀▀"],
            '2' => ["▀▀█", "█▀▀", "▀▀▀"],
            '3' => ["▀▀█", " ▀█", "▀▀▀"],
            '4' => ["█ █", "▀▀█", "  ▀"],
            '5' => ["█▀▀", "▀▀█", "▀▀▀"],
            '6' => ["█▀▀", "█▀█", "▀▀▀"],
            '7' => ["▀▀█", "  █", "  ▀"],
            '8' => ["█▀█", "█▀█", "▀▀▀"],
            '9' => ["█▀█", "▀▀█", "▀▀▀"],
            '.' => [" ", " ", "▀"],
            '-' => ["   ", "▀▀▀", "   "],
            '%' => ["▀ █", " █ ", "█ ▄"],
            '/' => ["  █", " █ ", "█  "],
            ' ' => [" ", " ", " "],
            _ => return None,
        };
        for (r, part) in rows.iter_mut().zip(g) {
            if i > 0 {
                r.push(' ');
            }
            r.push_str(part);
        }
    }
    Some(rows)
}

/// The big lamp of a summary: a round light three rows tall in its tone, with the lamp's mark
/// cut out of its middle (`(+)` in ASCII).
///
/// ```ignore
/// for (i, l) in kit::big_lamp(Lamp::Ok, t).into_iter().enumerate() { /* row i */ }
/// ```
pub fn big_lamp(l: Lamp, t: &Theme) -> [Line<'static>; 3] {
    let st = t.lamp(l).remove_modifier(Modifier::BOLD);
    let mark = t.lamp_mark(l).to_string();
    if !t.unicode() {
        return [
            Line::from(Span::styled(" .-. ", st)),
            Line::from(Span::styled(format!("( {mark} )"), st.patch(t.bold()))),
            Line::from(Span::styled(" '-' ", st)),
        ];
    }
    [
        Line::from(Span::styled("▄███▄", st)),
        Line::from(vec![
            Span::styled("██", st),
            Span::styled(mark, st.add_modifier(Modifier::REVERSED | Modifier::BOLD)),
            Span::styled("██", st),
        ]),
        Line::from(Span::styled("▀███▀", st)),
    ]
}

/// A stat tile: a small card with an icon and a label on its border, the value large (big
/// digits where there are four rows inside and the theme can draw them), and a note under it.
///
/// ```ignore
/// Tile::new("Reflexes", "12").icon("◠").note("in effect").tone(Tone::Ok).render(f, area, t);
/// ```
#[derive(Debug, Clone)]
pub struct Tile {
    pub label: String,
    pub value: String,
    pub icon: String,
    pub note: String,
    pub tone: Tone,
    /// Lines under the note (a meter, a sparkline), where the tile has rows for them.
    pub more: Vec<Line<'static>>,
}

impl Tile {
    /// Rows a compact tile takes: borders, the value, the note.
    pub const SHORT: u16 = 4;
    /// Rows a tile with big digits takes (and a line under its note).
    pub const TALL: u16 = 7;

    pub fn new(label: impl Into<String>, value: impl ToString) -> Tile {
        Tile {
            label: label.into(),
            value: value.to_string(),
            icon: String::new(),
            note: String::new(),
            tone: Tone::Plain,
            more: Vec::new(),
        }
    }

    pub fn more(mut self, lines: Vec<Line<'static>>) -> Tile {
        self.more = lines;
        self
    }

    pub fn icon(mut self, icon: impl Into<String>) -> Tile {
        self.icon = icon.into();
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Tile {
        self.note = note.into();
        self
    }

    pub fn tone(mut self, tone: Tone) -> Tile {
        self.tone = tone;
        self
    }

    /// Draw the tile in `area` (its height picks big digits or one bold line).
    pub fn render(&self, f: &mut Frame, area: Rect, t: &Theme) {
        let title = if self.icon.is_empty() { self.label.clone() } else { format!("{} {}", self.icon, self.label) };
        let block = Block::default().borders(Borders::ALL).border_set(t.border()).border_style(t.rule()).title(
            Line::from(vec![
                Span::styled(t.border().horizontal_top, t.rule()),
                Span::styled(format!(" {} ", t.text(&title)), t.ink2()),
            ]),
        );
        let inner = block.inner(area).inner(Margin { horizontal: 1, vertical: 0 });
        f.render_widget(block, area);
        let value_style = if self.tone == Tone::Plain { t.bold() } else { self.tone.style(t).patch(t.bold()) };
        let room = inner.width as usize;
        let mut lines: Vec<Line<'static>> = Vec::new();
        let big = big(&self.value, t).filter(|r| inner.height >= 4 && width(&r[0]) <= room);
        match big {
            Some(rows) => lines.extend(
                rows.into_iter().map(|r| Line::from(Span::styled(r, value_style.remove_modifier(Modifier::BOLD)))),
            ),
            None => lines.push(Line::from(Span::styled(clip(&self.value, room, t), value_style))),
        }
        if !self.note.is_empty() {
            lines.push(Line::from(Span::styled(clip(&t.text(&self.note), room, t), t.ink2())));
        }
        lines.extend(
            self.more.iter().map(|l| Line::from(clip_spans(t.lines(vec![l.clone()]).remove(0).spans, room, t))),
        );
        f.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    #[test]
    fn wrap_never_breaks_inside_a_word() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("one two three", 80), ["one two three"]);
        assert_eq!(wrap("aaaa bbbb cccc", 9), ["aaaa bbbb", "cccc"]);
        // Every line fits, and every word survives whole.
        let s = "Codex is set up but hasn't sent anything from this repository yet";
        for w in 10..40 {
            let rows = wrap(s, w);
            assert!(rows.iter().all(|r| width(r) <= w), "{w}: {rows:?}");
            assert_eq!(rows.join(" "), s, "{w}");
        }
    }

    #[test]
    fn a_word_longer_than_the_line_breaks_after_a_separator() {
        let rows = wrap("see ~/work/harbor-dispatch/.claude/settings.local.json now", 20);
        assert!(rows.iter().all(|r| width(r) <= 20), "{rows:?}");
        assert_eq!(rows[0], "see");
        assert!(rows[1].ends_with('/') || rows[1].ends_with('-') || rows[1].ends_with('.'), "{rows:?}");
        assert_eq!(rows.concat().replace(' ', ""), "see~/work/harbor-dispatch/.claude/settings.local.jsonnow");
    }

    #[test]
    fn wrapped_spans_hang_under_their_value_and_keep_styles() {
        let t = Theme::with_color(true);
        let spans = vec![Span::styled("why  ", t.dim()), Span::styled("Codex is set up but quiet", t.yellow())];
        let lines = wrap_spans(spans, 17, 5);
        assert_eq!(text(&lines), ["why  Codex is set", "     up but quiet"]);
        assert_eq!(lines[0].spans[0].style, t.dim());
        assert_eq!(lines[1].spans.last().unwrap().style, t.yellow());
        let kv = KeyValue::new().row("why", "Codex is set up but quiet").row("fix", "adage init").lines(17, &t);
        assert_eq!(text(&kv), ["why  Codex is set", "     up but quiet", "fix  adage init"]);
    }

    #[test]
    fn clips_end_in_the_themes_ellipsis() {
        let t = Theme::with_color(false);
        assert_eq!(clip("embedding model", 9, &t), "embeddin…");
        assert_eq!(clip("embedding model", 9, &t.ascii()), "embedd...");
        let spans = vec![Span::raw("abc "), Span::raw("defghij")];
        assert_eq!(text(&[Line::from(clip_spans(spans.clone(), 7, &t))]), ["abc de…"]);
        assert_eq!(text(&[Line::from(clip_spans(spans, 20, &t))]), ["abc defghij"]);
        let padded = vec![Span::raw("abc"), Span::raw("      ")];
        assert_eq!(text(&[Line::from(clip_spans(padded, 5, &t))]), ["abc"]);
    }

    #[test]
    fn meters_fill_in_half_cells_and_clamp() {
        assert_eq!(meter_cells(0.5, 10, true), (5, false, 5));
        assert_eq!(meter_cells(0.55, 10, true), (5, true, 4));
        assert_eq!(meter_cells(0.55, 10, false), (6, false, 4));
        assert_eq!(meter_cells(2.0, 10, true), (10, false, 0));
        assert_eq!(meter_cells(-1.0, 10, true), (0, false, 10));
        assert_eq!(meter_cells(f64::NAN, 4, true), (0, false, 4));
        let t = Theme::with_color(false);
        assert_eq!(text(&[Line::from(meter(0.55, 10, Tone::Ok, &t))]), ["━━━━━╸────"]);
        assert_eq!(text(&[Line::from(meter(0.5, 6, Tone::Ok, &t.ascii()))]), ["===---"]);
        for r in [0.0, 0.13, 0.5, 0.99, 1.0] {
            for w in [1, 7, 20] {
                assert_eq!(Line::from(meter(r, w, Tone::Ok, &t)).width(), w, "{r} {w}");
            }
        }
    }

    #[test]
    fn first_clauses_stop_at_the_first_break() {
        assert_eq!(first_clause("on; github.com/x/adage: 0 bound, 8 packs"), "on");
        assert_eq!(first_clause("potion-base-8m@f65d installed; checksums ok"), "potion-base-8m@f65d installed");
        assert_eq!(first_clause("Registered; no hook has been observed"), "Registered");
        assert_eq!(first_clause("adage 0.1.0 at ~/bin/adage"), "adage 0.1.0 at ~/bin/adage");
        assert_eq!(first_clause("adage (built into Adage 0.1.0): 8 packs"), "adage (built into Adage 0.1.0)");
        assert_eq!(first_clause("last run 12:04:00, fine"), "last run 12:04:00, fine");
    }

    #[test]
    fn sparks_hints_spinner_and_big_digits() {
        let t = Theme::with_color(false);
        assert_eq!(spark(&[0, 1, 2, 4], &t), "▁▃▅█");
        assert_eq!(spark(&[0, 0], &t), "▁▁");
        assert_eq!(spark(&[0, 4], &t.ascii()), "_#");
        let h = hints(&[("enter", "fix"), ("R", "recheck")], &t, 80);
        assert_eq!(text(&[Line::from(h)]), [" enter  fix   R  recheck"]);
        assert_eq!(hints(&[("enter", "fix"), ("R", "recheck")], &t, 12).len(), 2, "whole hints only");
        let mut still = t;
        still.motion = false;
        assert_eq!(spinner(0, &still), spinner(500, &still));
        assert_ne!(spinner(0, &t), spinner(80, &t));
        let rows = big("24", &t).unwrap();
        assert!(rows.iter().all(|r| width(r) == 7), "{rows:?}");
        assert!(big("24", &t.ascii()).is_none());
        assert_eq!(text(&big_lamp(Lamp::Ok, &t)), ["▄███▄", "██+██", "▀███▀"]);
        assert_eq!(text(&big_lamp(Lamp::Warn, &t.ascii()))[1], "( ! )");
    }

    #[test]
    fn sections_rule_to_the_edge() {
        let t = Theme::with_color(false);
        let l = Section::new("Hooks & delivery").count(4).line(40, &t);
        assert_eq!(l.width(), 39);
        assert!(l.to_string().starts_with(" Hooks & delivery  4 ─"), "{l}");
        let short = Section::new("A very long section title indeed").note("1 to look at", Tone::Warn).line(20, &t);
        assert!(short.width() <= 20, "{short}");
    }
}
