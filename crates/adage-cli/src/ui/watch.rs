//! `adage watch`: the compact live pane.

use super::health::{self, Health};
use super::jobs::{Loaded, Loader};
use super::tail::{LineTail, SpoolTail};
use super::term::{self, Term};
use super::view::{self, Ctx, Live};
use anyhow::Result;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Terminal;

/// The files the live view follows.
pub struct Sources {
    pub spool: SpoolTail,
    pub log: LineTail,
}

impl Sources {
    pub fn new(ctx: &Ctx) -> Sources {
        Sources { spool: SpoolTail::new(ctx.paths.sessions()), log: LineTail::new(ctx.paths.learn_log()) }
    }

    pub fn poll(&mut self, live: &mut Live) {
        live.push_records(self.spool.poll());
        live.push_log(self.log.poll());
    }
}

/// Map a key to the live view's keys; `true` to quit.
pub fn live_key(live: &mut Live, code: KeyCode, mods: KeyModifiers) -> bool {
    if live.help {
        live.help = false;
        return matches!(code, KeyCode::Char('q'));
    }
    match code {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Char('?') | KeyCode::Char('h') => live.help = true,
        KeyCode::Up => {
            live.key('k');
        }
        KeyCode::Down => {
            live.key('j');
        }
        KeyCode::PageUp => live.scroll = live.scroll.saturating_add(10),
        KeyCode::PageDown => live.scroll = live.scroll.saturating_sub(10),
        KeyCode::End => live.scroll = 0,
        KeyCode::Char(c) => {
            live.key(c);
        }
        _ => {}
    }
    false
}

/// Run the pane until `q`. `before_tick` runs each frame (the demo uses it to keep playing).
pub fn run(ctx: Ctx, mut before_tick: Option<Box<dyn FnMut()>>) -> Result<()> {
    let loader = Loader::spawn(ctx.paths.clone(), ctx.root.clone());
    let mut sources = Sources::new(&ctx);
    let mut live = Live::default();
    let mut health: Option<Health> = None;
    let mut ctx = ctx;
    let mut t = Term::enter()?;
    loop {
        if let Some(f) = before_tick.as_mut() {
            f();
        }
        while let Ok(m) = loader.rx.try_recv() {
            if let Loaded::Health(h) = m {
                health = Some(*h);
                ctx.tz = view::local_offset();
            }
        }
        sources.poll(&mut live);
        t.terminal.draw(|f| view::draw_live(f, f.area(), &live, health.as_ref(), &ctx, true))?;
        if let Some(k) = term::next_key()? {
            if live_key(&mut live, k.code, k.modifiers) {
                break;
            }
        }
    }
    drop(t);
    for n in super::take_notices() {
        eprintln!("{n}");
    }
    Ok(())
}

/// A buffer as plain text, one line per row, trailing spaces trimmed.
pub fn buffer_text(b: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..b.area.height {
        let mut line = String::new();
        for x in 0..b.area.width {
            line.push_str(b[(x, y)].symbol());
        }
        out.push_str(line.replace(view::NBSP, " ").trim_end());
        out.push('\n');
    }
    out
}

/// One frame of the live pane, as text: everything the spools and log hold now.
pub fn snapshot(ctx: &Ctx, size: Option<(u16, u16)>) -> String {
    let (w, h) = size.unwrap_or((100, 30));
    let mut live = Live::default();
    Sources::new(ctx).poll(&mut live);
    let health = health::gather(&ctx.paths, ctx.root.as_deref());
    let mut term = match Terminal::new(TestBackend::new(w, h)) {
        Ok(t) => t,
        Err(e) => return format!("adage watch: {e}\n"),
    };
    let plain = Ctx { theme: super::theme::Theme { color: false }, ..ctx.clone() };
    let _ = term.draw(|f| view::draw_live(f, f.area(), &live, Some(&health), &plain, true));
    buffer_text(term.backend().buffer())
}
