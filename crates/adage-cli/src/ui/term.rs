//! The terminal, owned for the length of a screen and restored on every exit: normal return,
//! error, or panic (a panic hook restores it before the message is printed, so the message is
//! readable and the shell is usable).

use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::crossterm::{cursor, execute};
use ratatui::Terminal;
use std::io::Stdout;
use std::sync::atomic::Ordering;
use std::sync::Once;
use std::time::Duration;

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// How long to wait for a key before redrawing: about 6 frames a second, and no busy loop.
pub const TICK: Duration = Duration::from_millis(160);

/// Owns the terminal; restores it when dropped.
pub struct Term {
    pub terminal: Tui,
}

static HOOK: Once = Once::new();

/// Put the terminal back: raw mode off, main screen, cursor shown. Idempotent.
pub fn restore() {
    if super::ACTIVE.swap(false, Ordering::SeqCst) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, cursor::Show);
    }
}

impl Term {
    pub fn enter() -> Result<Term> {
        HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                restore();
                previous(info);
            }));
        });
        terminal::enable_raw_mode()?;
        super::ACTIVE.store(true, Ordering::SeqCst);
        let mut out = std::io::stdout();
        if let Err(e) = execute!(out, EnterAlternateScreen, cursor::Hide) {
            restore();
            return Err(e.into());
        }
        match Terminal::new(CrosstermBackend::new(out)) {
            Ok(terminal) => Ok(Term { terminal }),
            Err(e) => {
                restore();
                Err(e.into())
            }
        }
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        restore();
    }
}

/// The next key press within [`TICK`], if any. Resizes are handled by the next draw; key releases
/// (reported on Windows) are ignored.
pub fn next_key() -> Result<Option<KeyEvent>> {
    next_key_within(TICK)
}

/// The next key press within `wait`, if any.
pub fn next_key_within(wait: Duration) -> Result<Option<KeyEvent>> {
    if !event::poll(wait)? {
        return Ok(None);
    }
    match event::read()? {
        Event::Key(k) if k.kind == KeyEventKind::Press => Ok(Some(k)),
        _ => Ok(None),
    }
}
