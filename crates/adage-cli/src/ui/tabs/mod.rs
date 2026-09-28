//! The tabs of `adage ui`, one module each. Every tab has the same two entry points:
//!
//! * `key(&mut AppState, KeyCode) -> Option<Cmd>`: a key, as a state change or a command for the
//!   runtime;
//! * `draw(&mut Frame, Rect, &AppState, &Ctx)`: a pure function from state to the tab's area.
//!
//! The Live tab is the watch view itself (`view::draw_live`, `watch::live_key`).

pub mod health;
pub mod map;
pub mod reflexes;
pub mod settings;

use super::theme::Theme;
use ratatui::text::{Line, Span};

/// A section heading in a detail pane: `WHAT THE AGENT SEES`.
pub fn head(title: &str, t: &Theme) -> Line<'static> {
    Line::from(Span::styled(title.to_string(), t.dim().patch(t.bold())))
}

/// An empty state: what is missing, and the command that fills it.
pub fn empty(what: &str, next: &str, command: &str, t: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::raw(""),
        Line::from(Span::raw(format!(" {what}"))),
        Line::from(vec![Span::styled(format!(" {next} "), t.dim()), Span::styled(command.to_string(), t.command())]),
    ]
}
