//! The terminal UI's styles, from the shared design system (`adage_term`, whose palette is
//! generated from `design/tokens/adage.tokens.json`). Views ask for a meaning (`green()` is
//! strength fire, `dim()` is tertiary text), never a colour.
//!
//! With `NO_COLOR` set (no-color.org) nothing is coloured: strengths and lamps are spelled out
//! in text instead, and only bold, dim, italic and reverse remain. `ADAGE_ASCII=1` swaps every
//! glyph for its ASCII fallback, and `ADAGE_REDUCED_MOTION=1` turns the one-shot pulses off.

use adage_term::{Caps, Glyph, Level, Role};
use adage_tracks::Aspect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

/// A health lamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lamp {
    Ok,
    Warn,
    Fail,
    /// Not applicable, not configured, or not known yet.
    Off,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Whether anything is coloured (false under `NO_COLOR`).
    pub color: bool,
    /// One-shot pulses (a firing reflex); off with `ADAGE_REDUCED_MOTION=1`.
    pub motion: bool,
    term: adage_term::Theme,
}

impl Theme {
    /// The terminal the UI owns: its colour depth, ground and glyphs from the environment.
    pub fn from_env() -> Theme {
        // A full-screen view only runs at a terminal, and bold and dim are always available.
        let caps = Caps { effects: true, ..Caps::from_env(|k| std::env::var(k).ok(), true) };
        let motion = std::env::var("ADAGE_REDUCED_MOTION").map_or(true, |v| v.is_empty() || v == "0");
        Theme { color: caps.level > Level::None, motion, term: adage_term::Theme::new(caps) }
    }

    /// The 16-colour theme (`true`) or no colour (`false`): what tests and `--once` use.
    pub fn with_color(color: bool) -> Theme {
        let level = if color { Level::Ansi16 } else { Level::None };
        Theme { color, motion: true, term: adage_term::Theme::new(Caps { level, effects: true, ..Caps::plain() }) }
    }

    /// The same, with ASCII glyphs (`ADAGE_ASCII=1`).
    #[cfg(test)]
    pub fn ascii(mut self) -> Theme {
        self.term.caps.unicode = false;
        self
    }

    /// No colour, with this theme's glyphs: `--once` frames honour `ADAGE_ASCII` too.
    pub fn uncoloured(&self) -> Theme {
        let mut t = Theme::with_color(false);
        t.term.caps.unicode = self.term.caps.unicode;
        t
    }

    fn role(&self, r: Role) -> Style {
        self.term.tui(r)
    }

    /// Strength fire, success.
    pub fn green(&self) -> Style {
        self.role(Role::Fire)
    }
    /// Strength prime, warnings.
    pub fn yellow(&self) -> Style {
        self.role(Role::Prime)
    }
    /// Strength inhibit, failures.
    pub fn red(&self) -> Style {
        self.role(Role::Inhibit)
    }
    /// The one accent.
    pub fn accent(&self) -> Style {
        self.role(Role::Accent)
    }
    /// The control arm: reflexes withheld on purpose.
    pub fn holdout(&self) -> Style {
        self.role(Role::Ink3).add_modifier(Modifier::ITALIC)
    }
    /// Tertiary text.
    pub fn dim(&self) -> Style {
        if self.color {
            self.role(Role::Ink3)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        }
    }
    /// Secondary text: descriptions, evidence after the number.
    pub fn ink2(&self) -> Style {
        self.role(Role::Ink2)
    }
    /// Commands the reader can run, and key hints.
    pub fn command(&self) -> Style {
        self.role(Role::Command)
    }
    /// A key in a hint: the command colour, bold.
    pub fn key(&self) -> Style {
        self.command().add_modifier(Modifier::BOLD)
    }
    /// Rules and box lines.
    pub fn rule(&self) -> Style {
        self.dim()
    }
    /// A glyph, or its ASCII fallback.
    pub fn glyph(&self, g: Glyph) -> &'static str {
        self.term.glyph(g)
    }
    /// A drawing character (rules, boxes, bars) or its ASCII stand-in.
    pub fn ch(&self, unicode: &'static str, ascii: &'static str) -> &'static str {
        if self.term.caps.unicode {
            unicode
        } else {
            ascii
        }
    }
    /// The separator between facts, with its spaces: ` · `, or ` - `.
    pub fn sep(&self) -> String {
        format!(" {} ", self.glyph(Glyph::Sep))
    }
    /// `s` in quotation marks: “s”, or "s".
    pub fn quote(&self, s: &str) -> String {
        format!("{}{s}{}", self.ch("“", "\""), self.ch("”", "\""))
    }
    /// Arrow keys, as a hint: `↑↓`, or `^v`.
    pub fn arrows(&self) -> &'static str {
        self.ch("↑↓", "^v")
    }
    /// A popup's border: rounded, or `+`, `-` and `|`.
    pub fn border(&self) -> ratatui::symbols::border::Set<'static> {
        if self.term.caps.unicode {
            ratatui::symbols::border::ROUNDED
        } else {
            ratatui::symbols::border::Set {
                top_left: "+",
                top_right: "+",
                bottom_left: "+",
                bottom_right: "+",
                vertical_left: "|",
                vertical_right: "|",
                horizontal_top: "-",
                horizontal_bottom: "-",
            }
        }
    }
    /// A trailing ellipsis: `…`, or `...`.
    pub fn more(&self) -> &'static str {
        self.ch("…", "...")
    }
    /// Cut `s` to `n` columns with this theme's ellipsis ([`super::words::fit_with`]).
    pub fn fit(&self, s: &str, n: usize) -> String {
        super::words::fit_with(s, n, self.more())
    }
    /// `s` as a column `n` wide, with this theme's ellipsis ([`super::words::pad_with`]).
    pub fn pad(&self, s: &str, n: usize) -> String {
        super::words::pad_with(s, n, self.more())
    }
    /// Swap the typographic characters in `text` (separators, arrows, ellipses, quotes, dashes)
    /// for their ASCII fallbacks when this terminal cannot draw them. For text written once and
    /// shown on every screen: tables of keys, palette entries, messages.
    pub fn text(&self, text: &str) -> String {
        if self.term.caps.unicode {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            match c {
                '·' => out.push_str(self.glyph(Glyph::Sep)),
                '→' => out.push_str(self.glyph(Glyph::Arrow)),
                '…' => out.push_str("..."),
                '↑' => out.push('^'),
                '↓' | '▾' => out.push('v'),
                '▸' | '›' | '⏎' => out.push('>'),
                '“' | '”' => out.push('"'),
                '‘' | '’' => out.push('\''),
                '–' | '—' => out.push('-'),
                '×' => out.push('x'),
                '≤' => out.push_str("<="),
                '≥' => out.push_str(">="),
                c => out.push(c),
            }
        }
        out
    }
    /// [`Theme::text`] over every span of `lines`: for a whole popup or card at once.
    pub fn lines(&self, lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
        if self.term.caps.unicode {
            return lines;
        }
        lines
            .into_iter()
            .map(|mut l| {
                for s in &mut l.spans {
                    s.content = self.text(&s.content).into();
                }
                l
            })
            .collect()
    }
    pub fn bold(&self) -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }
    pub fn selected(&self) -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }
    pub fn title(&self) -> Style {
        self.accent().add_modifier(Modifier::BOLD)
    }

    pub fn aspect(&self, a: Aspect) -> Style {
        match a {
            Aspect::Green => self.green(),
            Aspect::Yellow => self.yellow(),
            Aspect::Red => self.red(),
        }
    }

    /// The strength's word: fire, prime, inhibit.
    pub fn strength_word(a: Aspect) -> &'static str {
        match a {
            Aspect::Green => "fire",
            Aspect::Yellow => "prime",
            Aspect::Red => "inhibit",
        }
    }

    /// The strength's lamp: a coloured dot, or its word without colour.
    pub fn aspect_mark(&self, a: Aspect) -> &'static str {
        match (self.color, a) {
            (true, _) => self.glyph(Glyph::Lamp),
            (false, Aspect::Green) => "FIRE",
            (false, Aspect::Yellow) => "PRIME",
            (false, Aspect::Red) => "INHIBIT",
        }
    }

    pub fn lamp(&self, l: Lamp) -> Style {
        match l {
            Lamp::Ok => self.green(),
            Lamp::Warn => self.yellow(),
            Lamp::Fail => self.red().add_modifier(Modifier::BOLD),
            Lamp::Off => self.dim(),
        }
    }

    /// A health lamp's mark. Its shape says the state as well as its colour does, so a
    /// colour-blind reader (or a monochrome palette) loses nothing.
    pub fn lamp_mark(&self, l: Lamp) -> &'static str {
        match (self.color, l) {
            (true, Lamp::Off) => self.glyph(Glyph::Off),
            (true, Lamp::Ok) => self.glyph(Glyph::Ok),
            (true, Lamp::Warn) => "!",
            (true, Lamp::Fail) => self.glyph(Glyph::Fail),
            (false, Lamp::Ok) => "+",
            (false, Lamp::Warn) => "!",
            (false, Lamp::Fail) => "x",
            (false, Lamp::Off) => "-",
        }
    }
}

pub fn parse_aspect(s: &str) -> Option<Aspect> {
    match s {
        "green" | "fire" => Some(Aspect::Green),
        "yellow" | "prime" => Some(Aspect::Yellow),
        "red" | "inhibit" => Some(Aspect::Red),
        _ => None,
    }
}
