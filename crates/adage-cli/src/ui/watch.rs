//! `adage watch`: the compact live pane.

use super::health::{self, Health};
use super::jobs::{HealthCadence, Loaded, Loader, Want};
use super::tail::{LineTail, SpoolTail};
use super::term::{self, Term};
use super::view::{self, Ctx, Live};
use anyhow::Result;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Terminal;
use std::time::{Duration, Instant};

/// The files the live view follows, and the conditioners' locks.
pub struct Sources {
    pub spool: SpoolTail,
    pub log: LineTail,
    paths: adage_store::Paths,
    polls: u64,
}

/// Conditioners' locks are probed every this many polls (about twice a second).
const LOCK_EVERY: u64 = 3;

impl Sources {
    pub fn new(ctx: &Ctx) -> Sources {
        Sources {
            spool: SpoolTail::new(ctx.paths.sessions()),
            log: LineTail::new(ctx.paths.learn_log()),
            paths: ctx.paths.clone(),
            polls: 0,
        }
    }

    /// New records and log lines; which repositories are conditioning (`repo`, or all).
    pub fn poll(&mut self, live: &mut Live, repo: Option<&str>) {
        live.push_records(self.spool.poll());
        live.push_log(self.log.poll());
        if self.polls.is_multiple_of(LOCK_EVERY) {
            live.learning = health::learning_now(&self.paths, live.scope(repo));
        }
        self.polls += 1;
    }
}

/// Map a key to the live view's keys; `true` to quit.
pub fn live_key(live: &mut Live, code: KeyCode, mods: KeyModifiers) -> bool {
    if code == KeyCode::Char('c') && mods.contains(KeyModifiers::CONTROL) {
        return true;
    }
    if live.filtering {
        match code {
            KeyCode::Esc => {
                live.filter.clear();
                live.filtering = false;
            }
            KeyCode::Enter => live.filtering = false,
            KeyCode::Backspace => {
                live.filter.pop();
            }
            KeyCode::Char(c) if live.filter.chars().count() < 60 => {
                live.filter.push(c);
                live.scroll = 0;
            }
            _ => {}
        }
        return false;
    }
    if live.help || live.inspect || live.map_hint || live.runs {
        // A popup scrolls when it is taller than the pane; any other key closes it.
        match code {
            KeyCode::Up | KeyCode::Char('k') => live.popup_scroll = live.popup_scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => live.popup_scroll = live.popup_scroll.saturating_add(1),
            KeyCode::PageUp => live.popup_scroll = live.popup_scroll.saturating_sub(10),
            KeyCode::PageDown => live.popup_scroll = live.popup_scroll.saturating_add(10),
            _ => {
                live.help = false;
                live.inspect = false;
                live.map_hint = false;
                live.runs = false;
                live.popup_scroll = 0;
                return matches!(code, KeyCode::Char('q'));
            }
        }
        return false;
    }
    match code {
        KeyCode::Char('q') => return true,
        KeyCode::Esc if !live.filter.is_empty() => live.filter.clear(),
        KeyCode::Esc => return true,
        KeyCode::Char('?') | KeyCode::Char('h') => live.help = true,
        KeyCode::Enter => live.inspect = true,
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

/// How long to wait for a key: shorter while a pulse runs, so it fades smoothly.
pub fn wait(live: &Live) -> Duration {
    if live.pulsing() {
        Duration::from_millis(60)
    } else {
        term::TICK
    }
}

/// Run the pane until `q`. `before_tick` runs each frame (the demo uses it to keep playing).
pub fn run(ctx: Ctx, mut before_tick: Option<Box<dyn FnMut()>>) -> Result<()> {
    let loader = Loader::spawn(ctx.paths.clone(), ctx.root.clone(), HealthCadence::Terminal);
    loader.want(Want::Routes);
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
            match m {
                Loaded::Health(h) => {
                    health = Some(*h);
                    ctx.tz = view::local_offset();
                }
                Loaded::Routes(Ok(r)) => {
                    live.waiting = Some(r.waiting());
                    live.quiet = r.quiet();
                    live.needs = r.needs();
                    live.gists = r.gists();
                }
                Loaded::Routes(Err(_)) | Loaded::Traces(_) => {}
            }
        }
        sources.poll(&mut live, health.as_ref().and_then(|h| h.repo_key.as_deref()));
        // Outside a repository, the pane settles on the most recently active one (once health has
        // said where it was opened).
        if let Some(h) = &health {
            live.settle(h.repo_key.as_deref());
        }
        let here = health.as_ref().and_then(|h| h.repo_key.clone());
        live.tick(Instant::now(), ctx.theme.motion, here.as_deref(), adage_core::time::now_ms());
        t.terminal.draw(|f| view::draw_live(f, f.area(), &live, health.as_ref(), &ctx, true))?;
        if let Some(k) = term::next_key_within(wait(&live))? {
            if !live.filtering
                && !live.help
                && !live.inspect
                && !live.map_hint
                && !live.runs
                && (matches!(k.code, KeyCode::Char(':' | 'm'))
                    || (k.code == KeyCode::Char('k') && k.modifiers.contains(KeyModifiers::CONTROL)))
            {
                drop(t);
                drop(loader);
                return super::app::from_watch(ctx, before_tick, k.code == KeyCode::Char('m'));
            }
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
    let health = health::gather(&ctx.paths, ctx.root.as_deref());
    Sources::new(ctx).poll(&mut live, health.repo_key.as_deref());
    if let Ok(r) = super::routes::load(&ctx.paths, ctx.root.as_deref()) {
        live.waiting = Some(r.waiting());
        live.quiet = r.quiet();
        live.needs = r.needs();
        live.gists = r.gists();
    }
    let mut term = match Terminal::new(TestBackend::new(w, h)) {
        Ok(t) => t,
        Err(e) => return format!("adage watch: {e}\n"),
    };
    let plain = Ctx { theme: ctx.theme.uncoloured(), ..ctx.clone() };
    let _ = term.draw(|f| view::draw_live(f, f.area(), &live, Some(&health), &plain, true));
    buffer_text(term.backend().buffer())
}
