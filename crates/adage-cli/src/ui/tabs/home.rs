//! The Home tab: a welcoming dashboard. The repository and what Adage is in one line; four stat
//! tiles from data the app already holds (the reflexes in effect, how often they fired this week,
//! when this repository was last conditioned, and health); then the next useful actions as
//! buttons, each with an icon and a one-line description. The existing keys stay as they were.

use crate::doctor::Status;
use crate::ui::{
    app::{AppState, Cmd, Tab},
    feed::Act,
    forms::Kind,
    kit::{self, Card, Section, Tile, Tone},
    palette::Run,
    theme::Lamp,
    tools::Tool,
    view::Ctx,
};
use adage_term::Glyph;
use ratatui::{
    crossterm::event::KeyCode,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// One action: its icon (and ASCII stand-in), title, one-line description, and what it runs.
struct Step {
    icon: (&'static str, &'static str),
    title: &'static str,
    about: &'static str,
    run: Run,
}

fn steps() -> Vec<Step> {
    let step = |icon, title, about, run| Step { icon, title, about, run };
    vec![
        step(
            ("⌂", "[]"),
            "Open repository",
            "Choose the project you want Adage to understand.",
            Run::Form(Kind::OpenRepository),
        ),
        step(
            ("⇄", "<>"),
            "Connect your agent",
            "Hook Adage into Claude Code, Codex or OpenCode.",
            Run::Tab(Tab::Health),
        ),
        step(
            ("≡", "=="),
            "History sources",
            "See exactly which local traces Adage can read.",
            Run::Tool(Tool::HistorySources),
        ),
        step(
            ("◠", "~"),
            "Condition now",
            "Learn reflexes from those traces; bind instincts.",
            Run::Key(Tab::Health, 'c'),
        ),
        step(("✓", "+"), "Review reflexes", "Check each reflex and its evidence.", Run::Tab(Tab::Reflexes)),
        step(
            ("↗", "^"),
            "Explore the web map",
            "Follow whole traces and outcomes in your browser.",
            Run::Web { traces: false },
        ),
    ]
}

pub fn key(s: &mut AppState, code: KeyCode) -> Option<Cmd> {
    let last = steps().len() - 1;
    match code {
        // `u: update` (the banner, while a newer Adage is out): `adage update` as a job, after its
        // confirmation says what it will do.
        KeyCode::Char('u') => return Some(Cmd::PrepareTool(Tool::Update)),
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Left | KeyCode::Char('h') => {
            s.home_sel = s.home_sel.saturating_sub(1)
        }
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Right | KeyCode::Char('l') => {
            s.home_sel = (s.home_sel + 1).min(last)
        }
        KeyCode::Enter => {
            if s.home_sel == 1 {
                s.palette = Some(crate::ui::palette::Palette { query: "connect".into(), sel: 0 });
                return None;
            }
            return s.run_entry(steps().swap_remove(s.home_sel.min(last)).run);
        }
        _ => {}
    }
    None
}

// ---------------------------------------------------------------------------------------------
// What the tiles say

const DAY_MS: u64 = 86_400_000;

/// A moment as a person reads it: `today 12:04`, or the date (`Sep 23`).
fn when(ms: u64, now: u64, tz: i32) -> String {
    let day = |ms: u64| ((ms / 1000) as i64 + tz as i64).div_euclid(86_400);
    if day(ms) == day(now) {
        return format!("today {}", crate::ui::view::clock(ms, tz, false));
    }
    let date = adage_tracks::review::civil_date(day(ms));
    let (m, d) = (date.get(5..7).and_then(|m| m.parse::<usize>().ok()), date.get(8..10));
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    match (m, d) {
        (Some(m @ 1..=12), Some(d)) => format!("{} {}", MONTHS[m - 1], d.trim_start_matches('0')),
        _ => date,
    }
}

/// Firings delivered (never the control arm's withheld ones) in the repository the app was opened
/// in, from the feed the Live tab already holds: per day for the last seven days (oldest first),
/// and the newest firing's time.
fn firings(s: &AppState, now: u64) -> ([u64; 7], Option<u64>) {
    let here = s.here();
    let mut days = [0u64; 7];
    let mut last = None;
    for i in s.live.feed.iter().filter(|i| !i.control && here.is_none_or(|k| i.repo == k)) {
        let n = i.acts.iter().filter(|a| matches!(a, Act::Directed { .. })).count() as u64;
        if n == 0 {
            continue;
        }
        last = last.max(Some(i.ts));
        let age = now.saturating_sub(i.ts) / DAY_MS;
        if age < 7 && i.ts <= now {
            days[6 - age as usize] += n;
        }
    }
    (days, last)
}

fn tiles(s: &AppState, ctx: &Ctx, now: u64) -> Vec<Tile> {
    let t = &ctx.theme;
    let here = s.here();
    let rows: Vec<_> =
        s.route_rows().iter().filter(|r| here.is_none_or(|k| r.repo == k || r.repo.is_empty())).collect();
    let in_effect = rows.iter().filter(|r| !r.nascent() && !r.extinguished() && r.lifecycle() != "dormant").count();
    let waiting = s.live.waiting.unwrap_or(0);
    let reflexes = match &s.routes {
        None => Tile::new("Reflexes", t.more()).note("loading"),
        Some(Err(_)) => Tile::new("Reflexes", "?").note("could not be read"),
        Some(Ok(_)) => Tile::new("Reflexes", in_effect)
            .note(if in_effect == 0 { "none in effect yet" } else { "in effect here" })
            .more(if waiting > 0 {
                vec![Line::from(Span::styled(format!("{waiting} to review"), t.yellow()))]
            } else {
                vec![]
            })
            .tone(Tone::Accent),
    }
    .icon(t.glyph(Glyph::Reflex));

    let (days, last) = firings(s, now);
    let week: u64 = days.iter().sum();
    let fired = Tile::new("Fired", week)
        .icon(t.glyph(Glyph::Arrow))
        .tone(if week > 0 { Tone::Ok } else { Tone::Plain })
        .note(match (week, days[6], last) {
            (0, _, Some(ts)) => format!("last {}", when(ts, now, ctx.tz)),
            (0, _, None) => "not yet".to_string(),
            (_, 0, _) => "this week".to_string(),
            (_, n, _) => format!("this week, {n} today"),
        })
        .more(vec![Line::from(vec![Span::styled(kit::spark(&days, t), t.green()), Span::styled(" 7 days", t.dim())])]);

    // The newest conditioning run for this repository, from learn.log.
    let run = s
        .live
        .ticks
        .iter()
        .filter(|k| k.run.is_some() && here.is_none_or(|h| k.repo.as_deref().is_none_or(|r| r == h)))
        .max_by_key(|k| k.ts);
    let conditioned = match run {
        Some(k) => {
            let r = k.run.as_ref().expect("filtered");
            let changes = match r.changes {
                Some(0) => "no reflex changed".to_string(),
                Some(n) => crate::helpers::plural(n, "change", "changes"),
                None => "finished".to_string(),
            };
            Tile::new("Conditioned", when(k.ts, now, ctx.tz))
                .note(changes)
                .more(vec![Line::from(Span::styled(if r.auto { "auto-condition" } else { "by hand" }, t.dim()))])
        }
        None => Tile::new("Conditioned", "Not yet").note("Condition now, below"),
    }
    .icon(t.glyph(Glyph::Active));

    let health = match &s.health {
        None => Tile::new("Health", format!("Checking{}", t.more())).tone(Tone::Muted),
        Some(h) => {
            let (pass, warn, fail) = (h.count(Status::Pass), h.count(Status::Warn), h.count(Status::Fail));
            let total = h.checks.len().max(1);
            let (value, tone) = match (fail, warn) {
                (0, 0) => ("Healthy", Tone::Ok),
                (0, _) => ("Healthy", Tone::Warn),
                _ => ("Needs a fix", Tone::Fail),
            };
            let note = match (fail, warn) {
                (0, 0) => format!("{pass} checks passed"),
                (0, w) => format!("{w} to look at"),
                (f, _) => format!("{f} failing"),
            };
            Tile::new("Health", value).tone(tone).note(note).more(vec![Line::from(kit::meter(
                pass as f64 / total as f64,
                12,
                Tone::Ok,
                t,
            ))])
        }
    }
    .icon(t.lamp_mark(match &s.health {
        Some(h) if h.count(Status::Fail) > 0 => Lamp::Fail,
        Some(h) if h.count(Status::Warn) > 0 => Lamp::Warn,
        Some(_) => Lamp::Ok,
        None => Lamp::Off,
    }));
    vec![reflexes, fired, conditioned, health]
}

// ---------------------------------------------------------------------------------------------
// Drawing

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let now = adage_core::time::now_ms();
    let w = area.width as usize;

    // The header: the repository and what Adage is; its facts (where, which agents, whether it
    // conditions by itself); then the banners (a newer release, the demo).
    let mut facts: Vec<String> = Vec::new();
    if let Some(root) = &ctx.root {
        facts.push(crate::detect::tilde(root));
    }
    if let Some(h) = &s.health {
        let names: Vec<&str> =
            h.harnesses.iter().map(|n| adage_trace::Harness::parse(n).map_or(n.as_str(), |h| h.display())).collect();
        facts.push(if names.is_empty() {
            "no agent connected yet".into()
        } else {
            format!("{} connected", names.join(", "))
        });
        if let Some(l) = h.lights.iter().find(|l| l.label.starts_with("auto-condition")) {
            facts.push(if l.lamp == Lamp::Off { "auto-condition off".into() } else { "auto-condition on".into() });
        }
    }
    let mut head = vec![
        Line::from(vec![
            Span::styled(format!(" {} ", t.glyph(Glyph::Active)), t.accent()),
            Span::styled(ctx.label.clone(), t.title()),
            Span::styled(format!("{}your agent's reflexes, learned from its own traces", t.sep()), t.ink2()),
        ]),
        Line::from(Span::styled(format!("   {}", facts.join(&t.sep())), t.dim())),
    ];
    // A newer Adage, from the release check's cache: information, said where people look first,
    // in plain ink (never a warning colour), with the key that runs `adage update`.
    if let Some(r) = crate::update::newer(&ctx.paths) {
        head.push(Line::from(vec![
            Span::styled(format!(" {}", t.glyph(Glyph::Notice)), t.accent()),
            Span::styled(format!(" adage {}", env!("CARGO_PKG_VERSION")), t.ink2()),
            Span::styled(format!(" · {} is out · ", r.version), t.ink2()),
            Span::styled("u", t.accent().patch(t.bold())),
            Span::styled(" update", t.ink2()),
            Span::styled(" (adage update: Adage, its hooks and the plugin together)", t.dim()),
        ]));
    }
    if ctx.demo {
        head.push(Line::from(Span::styled(
            " DEMO · temporary sample data. Open repository to start with your own project.",
            t.yellow(),
        )));
    }
    let head: Vec<Line> =
        head.into_iter().map(|l| Line::from(kit::clip_spans(t.lines(vec![l]).remove(0).spans, w, t))).collect();

    // Room decides the shapes: tall tiles with big digits and button cards where there is room,
    // compact tiles and one-line buttons where there is not.
    let steps = steps();
    let fixed = head.len() as u16 + 1 + 1 + 1;
    let tiles = tiles(s, ctx, now);
    let cols = if area.width >= 72 { 4 } else { 2 };
    let tile_rows = tiles.len().div_ceil(cols) as u16;
    let tall = area.height > fixed + Tile::TALL * tile_rows + steps.len() as u16;
    let tile_h = if tall { Tile::TALL } else { Tile::SHORT };
    let grid = area.width >= 90 && area.height > fixed + tile_h * tile_rows + 1 + 4 * 3;
    let [head_a, _, tiles_a, _, section_a, actions_a, hint_a] = Layout::vertical([
        Constraint::Length(head.len() as u16),
        Constraint::Length(1),
        Constraint::Length(tile_h * tile_rows),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    f.render_widget(Paragraph::new(head), head_a);

    // The tiles, side by side (two rows of two on a narrow screen), one column in.
    let inner = Rect { x: tiles_a.x + 1, width: tiles_a.width.saturating_sub(2), ..tiles_a };
    let tile_w = inner.width.saturating_sub(cols as u16 - 1) / cols as u16;
    for (row, chunk) in tiles.chunks(cols).enumerate() {
        for (col, tile) in chunk.iter().enumerate() {
            let x = inner.x + col as u16 * (tile_w + 1);
            tile.render(f, Rect { x, y: inner.y + row as u16 * tile_h, width: tile_w, height: tile_h }, t);
        }
    }

    f.render_widget(
        Paragraph::new(
            Section::new("Get started").note("enter opens the one selected", Tone::Muted).line(area.width, t),
        ),
        section_a,
    );
    let sel = s.home_sel.min(steps.len() - 1);
    if grid {
        // Buttons as cards, two across: the selected one in the accent, with its key.
        let inner = Rect { x: actions_a.x + 1, width: actions_a.width.saturating_sub(2), ..actions_a };
        for (i, step) in steps.iter().enumerate() {
            let (row, col) = ((i / 2) as u16, (i % 2) as u16);
            let half = inner.width.saturating_sub(1) / 2;
            let r = Rect { x: inner.x + col * (half + 1), y: inner.y + row * 4, width: half, height: 4 };
            if r.bottom() > actions_a.bottom() {
                break;
            }
            let chosen = i == sel;
            let mut card = Card::new("");
            if chosen {
                card = card.tone(Tone::Accent).hints(&[("enter", "open")]);
            }
            let room = Card::inner_width(r);
            let title_style = if chosen { t.accent().patch(t.bold()) } else { t.bold() };
            let lines = vec![
                Line::from(vec![
                    Span::styled(format!("{}  ", t.ch(step.icon.0, step.icon.1)), t.accent()),
                    Span::styled(step.title, title_style),
                ]),
                Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        kit::clip(step.about, room.saturating_sub(3), t),
                        if chosen { t.ink2() } else { t.dim() },
                    ),
                ]),
            ];
            card.render(f, r, lines, t);
        }
    } else {
        // One line each: the selected one as a pill, every description on its own row's right.
        let title_w = steps.iter().map(|s| s.title.len()).max().unwrap_or(16) + 6;
        let mut lines: Vec<Line> = Vec::new();
        for (i, step) in steps.iter().enumerate() {
            let chosen = i == sel;
            let icon = t.ch(step.icon.0, step.icon.1);
            let button = format!(" {icon}  {} ", step.title);
            let pad = title_w.saturating_sub(kit::width(&button));
            let mut spans = vec![Span::styled(if chosen { t.ch("▌", ">") } else { " " }, t.accent()), Span::raw(" ")];
            if chosen {
                spans.push(Span::styled(button, t.accent().patch(t.selected()).patch(t.bold())));
            } else {
                spans.push(Span::styled(format!(" {icon}  "), t.accent()));
                spans.push(Span::styled(format!("{} ", step.title), t.ink2()));
            }
            spans.push(Span::raw(" ".repeat(pad + 1)));
            spans.push(Span::styled(step.about, if chosen { t.ink2() } else { t.dim() }));
            lines.push(Line::from(kit::clip_spans(spans, w, t)));
        }
        // Scrolled so the selection shows.
        let h = actions_a.height as usize;
        let scroll = (sel + 1).saturating_sub(h);
        f.render_widget(Paragraph::new(t.lines(lines)).scroll((scroll as u16, 0)), actions_a);
    }
    let hint = vec![Span::styled(" : or ctrl-k opens every action; ? explains this screen", t.dim())];
    f.render_widget(Paragraph::new(Line::from(kit::clip_spans(hint, w, t))), hint_a);
}

#[cfg(test)]
mod tests {
    use super::when;

    #[test]
    fn moments_read_as_today_or_a_date() {
        // 2026-09-23 12:04:00 UTC.
        let t0 = 1_790_165_040_000;
        assert_eq!(when(t0, t0 + 60_000, 0), "today 12:04");
        assert_eq!(when(t0, t0 + 2 * 86_400_000, 0), "Sep 23");
        assert_eq!(when(t0, t0, 13 * 3600), "today 01:04");
    }

    #[test]
    fn every_step_has_an_ascii_icon() {
        for s in super::steps() {
            assert!(s.icon.1.is_ascii() && !s.icon.1.is_empty(), "{}", s.title);
        }
    }
}
