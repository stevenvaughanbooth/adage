//! The terminal UI's styles, from the shared design system (`adage_term`, whose palette is
//! generated from `design/tokens/adage.tokens.json`). Views ask for a meaning (`green()` is
//! strength fire, `dim()` is tertiary text), never a colour.
//!
//! With `NO_COLOR` set (no-color.org) nothing is coloured: strengths and lamps are spelled out
//! in text instead, and only bold, dim, italic and reverse remain.

use adage_term::{Caps, Level, Role};
use adage_tracks::Aspect;
use ratatui::style::{Modifier, Style};

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
    term: adage_term::Theme,
}

impl Theme {
    /// The terminal the UI owns: its colour depth, ground and glyphs from the environment.
    pub fn from_env() -> Theme {
        // A full-screen view only runs at a terminal, and bold and dim are always available.
        let caps = Caps { effects: true, ..Caps::from_env(|k| std::env::var(k).ok(), true) };
        Theme { color: caps.level > Level::None, term: adage_term::Theme::new(caps) }
    }

    /// The 16-colour theme (`true`) or no colour (`false`): what tests and `--once` use.
    pub fn with_color(color: bool) -> Theme {
        let level = if color { Level::Ansi16 } else { Level::None };
        Theme { color, term: adage_term::Theme::new(Caps { level, effects: true, ..Caps::plain() }) }
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

    /// The strength's lamp: a coloured dot, or its word without colour.
    pub fn aspect_mark(&self, a: Aspect) -> &'static str {
        match (self.color, a) {
            (true, _) => "●",
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

    pub fn lamp_mark(&self, l: Lamp) -> &'static str {
        match (self.color, l) {
            (true, Lamp::Off) => "○",
            (true, _) => "●",
            (false, Lamp::Ok) => "+",
            (false, Lamp::Warn) => "!",
            (false, Lamp::Fail) => "x",
            (false, Lamp::Off) => "-",
        }
    }
}

pub fn parse_aspect(s: &str) -> Option<Aspect> {
    match s {
        "green" => Some(Aspect::Green),
        "yellow" => Some(Aspect::Yellow),
        "red" => Some(Aspect::Red),
        _ => None,
    }
}
