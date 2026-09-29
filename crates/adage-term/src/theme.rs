//! Semantic roles resolved against what the terminal can do.
//!
//! Call sites name a [`Role`]; the theme picks the colour. Body text ([`Role::Ink`]) always
//! uses the terminal's own foreground, and nothing ever paints the background: the user's
//! theme stays in charge. At 16 colours each role maps to a named ANSI role, so the user's own
//! palette applies; at 256 colours and truecolor, to the token values for a dark or light
//! ground. With no colour, strength is spelled out and only bold and dim remain.

use crate::caps::{Caps, Level};
use crate::color::{Ansi16, Swatch};
use crate::tokens;

/// What a piece of text is. Pick by meaning, never by colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Body text: the terminal's default foreground.
    Ink,
    /// Secondary text: descriptions, evidence after the number.
    Ink2,
    /// Tertiary text: labels, timestamps, separators, the control arm.
    Ink3,
    /// The one accent: the command name in a header, the active step, selection.
    Accent,
    /// Accent text: commands to run, key hints.
    Command,
    /// Strength fire, and success.
    Fire,
    /// Strength prime, and warnings.
    Prime,
    /// Strength inhibit, and errors.
    Inhibit,
    /// GPU spend and cost, nothing else.
    Cost,
}

impl Role {
    fn swatch(self) -> Option<Swatch> {
        Some(match self {
            Role::Ink => return None,
            Role::Ink2 => tokens::INK_2,
            Role::Ink3 => tokens::INK_3,
            Role::Accent => tokens::ACCENT_BASE,
            Role::Command => tokens::ACCENT_SOFT,
            Role::Fire => tokens::LAMP_FIRE,
            Role::Prime => tokens::LAMP_PRIME,
            Role::Inhibit => tokens::LAMP_INHIBIT,
            Role::Cost => tokens::COST,
        })
    }
}

/// A reflex's strength (the lexicon's words for the schema's green, yellow and red).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Strength {
    /// Do this: strong evidence.
    Fire,
    /// Consider this: a suggestion.
    Prime,
    /// Don't do this: a known dead end.
    Inhibit,
}

impl Strength {
    pub fn word(self) -> &'static str {
        match self {
            Strength::Fire => "fire",
            Strength::Prime => "prime",
            Strength::Inhibit => "inhibit",
        }
    }

    pub fn role(self) -> Role {
        match self {
            Strength::Fire => Role::Fire,
            Strength::Prime => Role::Prime,
            Strength::Inhibit => Role::Inhibit,
        }
    }

    /// Parse the schema's colour names and the lexicon's words.
    pub fn parse(s: &str) -> Option<Strength> {
        match s {
            "green" | "fire" => Some(Strength::Fire),
            "yellow" | "prime" => Some(Strength::Prime),
            "red" | "inhibit" => Some(Strength::Inhibit),
            _ => None,
        }
    }
}

/// A glyph from the token table, with its ASCII fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Lamp,
    Off,
    Instinct,
    Reflex,
    Ok,
    Fail,
    Arrow,
    Active,
    Done,
    Rail,
    Pointer,
    Sep,
    /// A procedure card.
    Card,
    /// A rewrite: the command was changed before it ran.
    Rewrite,
    /// Quiet: uplift below the bar; waits for a person.
    Quiet,
    /// From a skill (`adage train`).
    Skill,
    /// A notice shown at session start.
    Notice,
}

impl Glyph {
    fn pair(self) -> (&'static str, &'static str) {
        use tokens::glyph as g;
        match self {
            Glyph::Lamp => g::LAMP,
            Glyph::Off => g::OFF,
            Glyph::Instinct => g::INSTINCT,
            Glyph::Reflex => g::REFLEX,
            Glyph::Ok => g::OK,
            Glyph::Fail => g::FAIL,
            Glyph::Arrow => g::ARROW,
            Glyph::Active => g::ACTIVE,
            Glyph::Done => g::DONE,
            Glyph::Rail => g::RAIL,
            Glyph::Pointer => g::POINTER,
            Glyph::Sep => g::SEP,
            Glyph::Card => g::CARD,
            Glyph::Rewrite => g::REWRITE,
            Glyph::Quiet => g::QUIET,
            Glyph::Skill => g::SKILL,
            Glyph::Notice => g::NOTICE,
        }
    }
}

/// A resolved foreground: none (the terminal's own), a 16-colour index, a 256 index, or RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fg {
    Default,
    Ansi(u8),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// A role resolved for one terminal: a foreground and two effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub fg: Fg,
    pub bold: bool,
    pub dim: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub caps: Caps,
}

impl Theme {
    pub const fn new(caps: Caps) -> Theme {
        Theme { caps }
    }

    /// No colour, UTF-8 glyphs: pipes, `--plain`, snapshots.
    pub const fn plain() -> Theme {
        Theme { caps: Caps::plain() }
    }

    /// For standard output.
    pub fn stdout() -> Theme {
        Theme { caps: Caps::stdout() }
    }

    /// For standard error.
    pub fn stderr() -> Theme {
        Theme { caps: Caps::stderr() }
    }

    pub fn color(&self) -> bool {
        self.caps.level > Level::None
    }

    /// A role for this terminal.
    pub fn resolve(&self, role: Role) -> Resolved {
        let plain = Resolved { fg: Fg::Default, bold: false, dim: false };
        let Some(sw) = role.swatch() else { return plain };
        if !self.caps.effects {
            return plain;
        }
        match self.caps.level {
            // Meaning must survive without colour: tertiary text dims, the accent is bold;
            // strength is spelled out by the caller (`Theme::strength_label`).
            Level::None => match role {
                Role::Ink2 | Role::Ink3 => Resolved { dim: true, ..plain },
                Role::Accent => Resolved { bold: true, ..plain },
                _ => plain,
            },
            Level::Ansi16 => match sw.ansi16 {
                Ansi16::Default => plain,
                Ansi16::Dim => Resolved { dim: true, ..plain },
                other => Resolved { fg: Fg::Ansi(other.index().unwrap_or(0)), ..plain },
            },
            Level::Ansi256 => {
                Resolved { fg: Fg::Indexed(if self.caps.day { sw.day_256 } else { sw.night_256 }), ..plain }
            }
            Level::TrueColor => {
                let c = if self.caps.day { sw.day } else { sw.night };
                Resolved { fg: Fg::Rgb(c.0, c.1, c.2), ..plain }
            }
        }
    }

    /// A role as an `anstyle` style, for printed output.
    pub fn style(&self, role: Role) -> anstyle::Style {
        let r = self.resolve(role);
        let mut s = anstyle::Style::new();
        s = match r.fg {
            Fg::Default => s,
            Fg::Ansi(i) => s.fg_color(Some(anstyle::Color::Ansi(ansi_color(i)))),
            Fg::Indexed(i) => s.fg_color(Some(anstyle::Color::Ansi256(anstyle::Ansi256Color(i)))),
            Fg::Rgb(r, g, b) => s.fg_color(Some(anstyle::Color::Rgb(anstyle::RgbColor(r, g, b)))),
        };
        if r.bold {
            s = s.bold();
        }
        if r.dim {
            s = s.dimmed();
        }
        s
    }

    /// A role as a ratatui style, for the full-screen views.
    #[cfg(feature = "ratatui")]
    pub fn tui(&self, role: Role) -> ratatui_core::style::Style {
        use ratatui_core::style::{Color, Modifier, Style};
        let r = self.resolve(role);
        let mut s = Style::default();
        s = match r.fg {
            Fg::Default => s,
            Fg::Ansi(i) => s.fg(match i {
                0 => Color::Black,
                1 => Color::Red,
                2 => Color::Green,
                3 => Color::Yellow,
                4 => Color::Blue,
                5 => Color::Magenta,
                6 => Color::Cyan,
                7 => Color::Gray,
                8 => Color::DarkGray,
                9 => Color::LightRed,
                10 => Color::LightGreen,
                11 => Color::LightYellow,
                12 => Color::LightBlue,
                13 => Color::LightMagenta,
                14 => Color::LightCyan,
                _ => Color::White,
            }),
            Fg::Indexed(i) => s.fg(Color::Indexed(i)),
            Fg::Rgb(r, g, b) => s.fg(Color::Rgb(r, g, b)),
        };
        if r.bold {
            s = s.add_modifier(Modifier::BOLD);
        }
        if r.dim {
            s = s.add_modifier(Modifier::DIM);
        }
        s
    }

    /// A glyph, or its ASCII fallback.
    pub fn glyph(&self, g: Glyph) -> &'static str {
        let (u, a) = g.pair();
        if self.caps.unicode {
            u
        } else {
            a
        }
    }

    /// The lamp and word for a strength, as a row shows them: `● fire` in colour, `FIRE`
    /// without (the word carries the meaning the colour cannot).
    pub fn strength_label(&self, s: Strength) -> String {
        if self.color() {
            format!("{} {}", self.glyph(Glyph::Lamp), s.word())
        } else {
            s.word().to_ascii_uppercase()
        }
    }
}

fn ansi_color(i: u8) -> anstyle::AnsiColor {
    use anstyle::AnsiColor::*;
    match i {
        0 => Black,
        1 => Red,
        2 => Green,
        3 => Yellow,
        4 => Blue,
        5 => Magenta,
        6 => Cyan,
        7 => White,
        8 => BrightBlack,
        9 => BrightRed,
        10 => BrightGreen,
        11 => BrightYellow,
        12 => BrightBlue,
        13 => BrightMagenta,
        14 => BrightCyan,
        _ => BrightWhite,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(level: Level) -> Theme {
        Theme::new(Caps { level, effects: true, ..Caps::plain() })
    }

    #[test]
    fn body_text_never_gets_a_colour() {
        for level in [Level::None, Level::Ansi16, Level::Ansi256, Level::TrueColor] {
            assert_eq!(at(level).resolve(Role::Ink).fg, Fg::Default);
        }
    }

    #[test]
    fn sixteen_colours_use_the_users_theme_roles() {
        let t = at(Level::Ansi16);
        assert_eq!(t.resolve(Role::Fire).fg, Fg::Ansi(2));
        assert_eq!(t.resolve(Role::Inhibit).fg, Fg::Ansi(1));
        assert_eq!(t.resolve(Role::Accent).fg, Fg::Ansi(5));
        assert!(t.resolve(Role::Ink2).dim);
    }

    #[test]
    fn truecolor_follows_the_ground() {
        let night = at(Level::TrueColor);
        let day = Theme::new(Caps { level: Level::TrueColor, day: true, effects: true, ..Caps::plain() });
        assert_eq!(night.resolve(Role::Accent).fg, Fg::Rgb(0x8b, 0x7c, 0xff));
        assert_eq!(day.resolve(Role::Accent).fg, Fg::Rgb(0x5b, 0x4b, 0xd6));
    }

    #[test]
    fn without_colour_strength_is_spelled_out() {
        assert_eq!(at(Level::None).strength_label(Strength::Inhibit), "INHIBIT");
        assert_eq!(at(Level::Ansi16).strength_label(Strength::Fire), "● fire");
        assert_eq!(at(Level::None).style(Role::Fire), anstyle::Style::new());
        assert_eq!(Theme::plain().style(Role::Accent), anstyle::Style::new(), "pipes get no escape codes");
    }

    #[test]
    fn ascii_fallbacks() {
        let t = Theme::new(Caps { unicode: false, ..Caps::plain() });
        assert_eq!(t.glyph(Glyph::Instinct), "(o)");
        assert_eq!(t.glyph(Glyph::Arrow), "->");
        assert_eq!(Theme::plain().glyph(Glyph::Reflex), "◠");
    }

    #[test]
    fn newer_concepts_have_one_column_glyphs_and_ascii_fallbacks() {
        let ascii = Theme::new(Caps { unicode: false, ..Caps::plain() });
        let want = [
            (Glyph::Card, "▤", "[=]"),
            (Glyph::Rewrite, "↪", "=>"),
            (Glyph::Quiet, "◌", "(.)"),
            (Glyph::Skill, "◈", "<>"),
            (Glyph::Notice, "»", ">>"),
        ];
        for (g, utf8, fallback) in want {
            assert_eq!(Theme::plain().glyph(g), utf8);
            assert_eq!(ascii.glyph(g), fallback);
            assert_eq!(unicode_width::UnicodeWidthStr::width(utf8), 1, "{utf8} is one column");
        }
    }
}
