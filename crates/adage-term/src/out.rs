//! The output grammar every command prints through.
//!
//! ```text
//! adage status · acme-billing · gen 8dd5ec01 · hooks claude, codex      ← header
//!
//! instincts   1 track · 1 instinct · adage/finish-line@0.1.0             ← kv (one key column)
//! reflexes    2 tracks · 4 reflexes from 57 traces
//!
//!   ● fire     ◠ lookup-work-item        23/33  lb 0.53 · 8 turns saved   ← rows (Line + pad_to)
//!   ● inhibit  ◠ dead-end-d41b1e         11/43  "No module named pytest" → make test
//!
//! next  adage review · 1 nascent reflex waits for you                     ← next
//! ```
//!
//! Lines are built from spans that name a [`Role`], never a colour, and rendered against the
//! [`Theme`] of the stream they are written to. Columns are aligned with [`Line::pad_to`], by
//! display width, after the theme has chosen glyphs and strength labels.

use crate::theme::{Glyph, Role, Strength, Theme};
use std::io::{self, Write};
use unicode_width::UnicodeWidthStr;

/// The width of the key column in [`Out::kv`] rows: one rule for every command.
pub const KEY_WIDTH: usize = 12;

#[derive(Debug, Clone, PartialEq)]
enum Span {
    Text {
        role: Role,
        bold: bool,
        text: String,
    },
    Glyph {
        role: Role,
        glyph: Glyph,
    },
    Strength(Strength),
    /// Spaces up to this display column (at least one space if already past it).
    PadTo(usize),
}

/// One line of output, as spans with roles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Line {
    spans: Vec<Span>,
}

impl Line {
    pub fn new() -> Line {
        Line::default()
    }

    /// Text in a role.
    pub fn push(mut self, role: Role, text: impl Into<String>) -> Line {
        self.spans.push(Span::Text { role, bold: false, text: text.into() });
        self
    }

    /// Bold text in a role.
    pub fn bold(mut self, role: Role, text: impl Into<String>) -> Line {
        self.spans.push(Span::Text { role, bold: true, text: text.into() });
        self
    }

    /// Body text.
    pub fn text(self, text: impl Into<String>) -> Line {
        self.push(Role::Ink, text)
    }

    /// Secondary text.
    pub fn quiet(self, text: impl Into<String>) -> Line {
        self.push(Role::Ink2, text)
    }

    /// Tertiary text: labels and units.
    pub fn faint(self, text: impl Into<String>) -> Line {
        self.push(Role::Ink3, text)
    }

    /// A command the reader can run.
    pub fn cmd(self, text: impl Into<String>) -> Line {
        self.push(Role::Command, text)
    }

    /// ` · ` between facts.
    pub fn sep(mut self) -> Line {
        self.spans.push(Span::Text { role: Role::Ink3, bold: false, text: " ".into() });
        self.spans.push(Span::Glyph { role: Role::Ink3, glyph: Glyph::Sep });
        self.spans.push(Span::Text { role: Role::Ink3, bold: false, text: " ".into() });
        self
    }

    /// A glyph from the token table (ASCII on terminals without UTF-8).
    pub fn glyph(mut self, role: Role, glyph: Glyph) -> Line {
        self.spans.push(Span::Glyph { role, glyph });
        self
    }

    /// A strength: `● fire` in colour, `FIRE` without.
    pub fn strength(mut self, s: Strength) -> Line {
        self.spans.push(Span::Strength(s));
        self
    }

    /// Pad with spaces to a display column.
    pub fn pad_to(mut self, column: usize) -> Line {
        self.spans.push(Span::PadTo(column));
        self
    }

    /// Append another line's spans.
    pub fn extend(mut self, other: Line) -> Line {
        self.spans.extend(other.spans);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The line as text styled for `theme`, and its display width.
    pub fn render(&self, theme: &Theme) -> (String, usize) {
        // Runs of text in the same style are written with one pair of escape codes.
        let mut runs: Vec<(anstyle::Style, String)> = Vec::new();
        let mut col = 0usize;
        let mut paint = |style: anstyle::Style, text: &str, col: &mut usize| {
            *col += UnicodeWidthStr::width(text);
            match runs.last_mut() {
                Some((s, t)) if *s == style => t.push_str(text),
                _ => runs.push((style, text.to_string())),
            }
        };
        let style_of = |role: Role, bold: bool| {
            let s = theme.style(role);
            if bold && theme.caps.effects {
                s.bold()
            } else {
                s
            }
        };
        for span in &self.spans {
            match span {
                Span::Text { role, bold, text } => paint(style_of(*role, *bold), text, &mut col),
                Span::Glyph { role, glyph } => paint(style_of(*role, false), theme.glyph(*glyph), &mut col),
                Span::Strength(s) => paint(style_of(s.role(), false), &theme.strength_label(*s), &mut col),
                Span::PadTo(c) => {
                    let n = if *c > col { c - col } else { 1 };
                    paint(anstyle::Style::new(), &" ".repeat(n), &mut col);
                }
            }
        }
        let mut out = String::new();
        for (style, text) in runs {
            if style == anstyle::Style::new() {
                out.push_str(&text);
            } else {
                out.push_str(&format!("{style}{text}{style:#}"));
            }
        }
        (out, col)
    }

    /// The line without styling (for tests and width checks).
    pub fn plain(&self) -> String {
        self.render(&Theme::plain()).0
    }
}

/// A stream with a theme: standard output for results, standard error for errors.
pub struct Out<W: Write> {
    w: W,
    pub theme: Theme,
}

impl Out<anstream::AutoStream<io::Stdout>> {
    /// Standard output, styled for what it is (plain when piped or under `NO_COLOR`).
    pub fn stdout() -> Self {
        Out::stdout_with(Theme::stdout())
    }

    /// Standard output with an explicit theme (`--plain`, or tests of a command's styling).
    /// `anstream` strips styling when the theme has no colour and enables escape processing on
    /// older Windows consoles.
    pub fn stdout_with(theme: Theme) -> Self {
        let choice = if theme.color() { anstream::ColorChoice::Always } else { anstream::ColorChoice::Never };
        Out { w: anstream::AutoStream::new(io::stdout(), choice), theme }
    }
}

impl Out<anstream::AutoStream<io::Stderr>> {
    /// Standard error, styled for what it is.
    pub fn stderr() -> Self {
        let theme = Theme::stderr();
        let choice = if theme.color() { anstream::ColorChoice::Always } else { anstream::ColorChoice::Never };
        Out { w: anstream::AutoStream::new(io::stderr(), choice), theme }
    }
}

impl<W: Write> Out<W> {
    /// Any writer (a `Vec<u8>` in tests) with a theme; styling is written as is.
    pub fn raw(w: W, theme: Theme) -> Self {
        Out { w, theme }
    }

    pub fn into_inner(self) -> W {
        self.w
    }

    /// Write one line.
    pub fn line(&mut self, line: &Line) -> io::Result<()> {
        let (text, _) = line.render(&self.theme);
        writeln!(self.w, "{}", text.trim_end_matches(' '))
    }

    pub fn blank(&mut self) -> io::Result<()> {
        writeln!(self.w)
    }

    /// `adage <command> · fact · fact`: the first line of every command's output.
    pub fn header(&mut self, command: &str, facts: &[&str]) -> io::Result<()> {
        let mut l = Line::new().bold(Role::Accent, format!("adage {command}"));
        for f in facts.iter().filter(|f| !f.is_empty()) {
            l = l.sep().quiet(*f);
        }
        self.line(&l)
    }

    /// A key and its value, the key in the one key column.
    pub fn kv(&mut self, key: &str, value: Line) -> io::Result<()> {
        self.line(&Line::new().faint(key).pad_to(KEY_WIDTH).extend(value))
    }

    /// A section label.
    pub fn section(&mut self, title: &str) -> io::Result<()> {
        self.line(&Line::new().bold(Role::Ink, title))
    }

    /// `next  adage review · why`: how every command ends when there is something to do.
    pub fn next(&mut self, command: &str, why: &str) -> io::Result<()> {
        let mut l = Line::new().bold(Role::Accent, "next").pad_to(6).cmd(command);
        if !why.is_empty() {
            l = l.sep().quiet(why);
        }
        self.line(&l)
    }

    /// Further `next` suggestions, aligned under the first.
    pub fn also(&mut self, command: &str, why: &str) -> io::Result<()> {
        let mut l = Line::new().pad_to(6).cmd(command);
        if !why.is_empty() {
            l = l.sep().quiet(why);
        }
        self.line(&l)
    }

    /// A finished step: `✓ what  detail`.
    pub fn done(&mut self, what: &str, detail: &str) -> io::Result<()> {
        self.line(&Line::new().text("  ").glyph(Role::Fire, Glyph::Ok).text(" ").text(what).pad_to(28).quiet(detail))
    }

    /// Secondary information.
    pub fn note(&mut self, text: &str) -> io::Result<()> {
        self.line(&Line::new().quiet(text))
    }

    /// `warning  message`.
    pub fn warn(&mut self, message: &str) -> io::Result<()> {
        self.line(&Line::new().bold(Role::Prime, "warning").pad_to(9).text(message))
    }

    /// An error with its causes and, when there is one, what to do about it:
    ///
    /// ```text
    /// error  cannot listen on 127.0.0.1:80
    ///   │
    ///   ├─ cause  permission denied (os error 13)
    ///   │
    ///   ╰─ help   use another port, or let Adage pick one: adage map --port 0
    /// ```
    pub fn error(&mut self, message: &str, causes: &[String], help: Option<&str>) -> io::Result<()> {
        let mut items: Vec<(&str, &str)> = causes.iter().map(|c| ("cause", c.as_str())).collect();
        if let Some(h) = help {
            items.push(("help", h));
        }
        self.error_items(message, &items)
    }

    /// An error with labelled branches (`cause`, `tip`, `help`), in the order given.
    pub fn error_items(&mut self, message: &str, items: &[(&str, &str)]) -> io::Result<()> {
        self.line(&Line::new().bold(Role::Inhibit, "error").pad_to(7).bold(Role::Ink, message))?;
        let (bar, tee, end) = if self.theme.caps.unicode { ("│", "├─", "╰─") } else { ("|", "|-", "`-") };
        for (i, (label, text)) in items.iter().enumerate() {
            self.line(&Line::new().text("  ").faint(bar))?;
            let branch = if i + 1 == items.len() { end } else { tee };
            self.line(&Line::new().text("  ").faint(branch).text(" ").faint(*label).pad_to(12).quiet(*text))?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caps::{Caps, Level};

    fn render(theme: Theme, f: impl FnOnce(&mut Out<Vec<u8>>) -> io::Result<()>) -> String {
        let mut o = Out::raw(Vec::new(), theme);
        f(&mut o).unwrap();
        String::from_utf8(o.into_inner()).unwrap()
    }

    fn status(o: &mut Out<Vec<u8>>) -> io::Result<()> {
        o.header("status", &["acme-billing", "gen 8dd5ec01", "hooks claude, codex"])?;
        o.blank()?;
        o.kv("instincts", Line::new().text("1 track").sep().text("1 instinct"))?;
        o.kv("reflexes", Line::new().text("2 tracks").sep().text("4 reflexes from 57 traces"))?;
        o.blank()?;
        for (s, g, id, sup, note) in [
            (Strength::Fire, Glyph::Reflex, "lookup-work-item", "23/33", "lb 0.53 · 8 turns saved"),
            (Strength::Prime, Glyph::Instinct, "finish-line/regenerate", "7/7", "lb 0.65"),
            (Strength::Inhibit, Glyph::Reflex, "dead-end-d41b1e", "11/43", "\"No module named pytest\" → make test"),
        ] {
            o.line(
                &Line::new()
                    .text("  ")
                    .strength(s)
                    .pad_to(13)
                    .glyph(Role::Accent, g)
                    .text(" ")
                    .text(id)
                    .pad_to(40)
                    .quiet(sup)
                    .pad_to(47)
                    .quiet(note),
            )?;
        }
        o.blank()?;
        o.next("adage review", "1 nascent reflex waits for you")
    }

    #[test]
    fn status_plain() {
        insta::assert_snapshot!(render(Theme::plain(), status));
    }

    #[test]
    fn status_ascii() {
        let t = Theme::new(Caps { unicode: false, ..Caps::plain() });
        insta::assert_snapshot!(render(t, status));
    }

    #[test]
    fn colour_columns_line_up_with_plain_columns() {
        let color = Theme::new(Caps { level: Level::Ansi16, effects: true, ..Caps::plain() });
        let strip = |s: String| {
            let mut out = String::new();
            let mut esc = false;
            for ch in s.chars() {
                match (esc, ch) {
                    (false, '\x1b') => esc = true,
                    (true, 'm') => esc = false,
                    (true, _) => {}
                    (false, c) => out.push(c),
                }
            }
            out
        };
        let plain = render(Theme::plain(), status);
        let coloured = strip(render(color, status));
        // Same text apart from the strength labels (● fire vs FIRE), same columns.
        for (a, b) in plain.lines().zip(coloured.lines()) {
            assert_eq!(
                a.replace("FIRE   ", "● fire ").replace("PRIME  ", "● prime").replace("INHIBIT  ", "● inhibit"),
                b
            );
        }
    }

    #[test]
    fn errors_show_cause_and_help() {
        let s = render(Theme::plain(), |o| {
            o.error(
                "cannot listen on 127.0.0.1:80",
                &["permission denied (os error 13): ports below 1024 need root".into()],
                Some("use another port, or let Adage pick one: adage map --port 0"),
            )
        });
        insta::assert_snapshot!(s);
    }
}
