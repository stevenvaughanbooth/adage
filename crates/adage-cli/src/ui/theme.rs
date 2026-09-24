//! Colours and glyphs. With `NO_COLOR` set (no-color.org) nothing is coloured: aspects and lamps
//! are spelled out in text instead, and only bold, dim, italic and reverse remain.

use adage_tracks::Aspect;
use ratatui::style::{Color, Modifier, Style};

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
    pub color: bool,
}

impl Theme {
    pub fn from_env() -> Theme {
        Theme { color: std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) }
    }

    fn fg(&self, c: Color) -> Style {
        if self.color {
            Style::default().fg(c)
        } else {
            Style::default()
        }
    }

    pub fn green(&self) -> Style {
        self.fg(Color::Green)
    }
    pub fn yellow(&self) -> Style {
        self.fg(Color::Yellow)
    }
    pub fn red(&self) -> Style {
        self.fg(Color::Red)
    }
    pub fn accent(&self) -> Style {
        self.fg(Color::Cyan)
    }
    pub fn holdout(&self) -> Style {
        self.fg(Color::Magenta).add_modifier(Modifier::ITALIC)
    }
    pub fn dim(&self) -> Style {
        if self.color {
            Style::default().fg(Color::DarkGray)
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

    /// The aspect's lamp: a coloured dot, or its name without colour.
    pub fn aspect_mark(&self, a: Aspect) -> &'static str {
        match (self.color, a) {
            (true, _) => "●",
            (false, Aspect::Green) => "[G]",
            (false, Aspect::Yellow) => "[Y]",
            (false, Aspect::Red) => "[R]",
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
