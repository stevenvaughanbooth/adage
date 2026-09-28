//! The command palette (`:` or ctrl-k): fuzzy search over every action, every reflex and every
//! place in the app. Each action shows the `adage` command that does the same from a shell, so
//! the palette teaches the CLI.

use super::app::{AppState, Tab};
use super::fuzzy;
use super::keys;
use super::theme::Theme;
use adage_term::Glyph;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Actions,
    Reflexes,
    GoTo,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Actions => "ACTIONS",
            Section::Reflexes => "REFLEXES",
            Section::GoTo => "GO TO",
        }
    }
}

/// What choosing an entry does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// Press `key` in `tab` (a review action applies to the selected reflex).
    Key(Tab, char),
    /// Select a reflex in the Reflexes tab: `(reference, id)`.
    Reflex(String, String),
    Tab(Tab),
    /// Select a setting.
    Setting(usize),
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub section: Section,
    pub title: String,
    /// Shown after the title, dim: the `adage` command, or a reflex's standing.
    pub note: String,
    /// The key that does the same where it applies.
    pub key: Option<&'static str>,
    pub run: Run,
    /// A reflex's strength and instinct flag, for its lamp and glyph.
    pub lamp: Option<(adage_tracks::Aspect, bool)>,
}

#[derive(Debug, Clone, Default)]
pub struct Palette {
    pub query: String,
    pub sel: usize,
}

/// Every entry the palette can offer now.
pub fn entries(s: &AppState) -> Vec<Entry> {
    let action = |title: &str, note: &str, key: &'static str, run: Run| Entry {
        section: Section::Actions,
        title: title.to_string(),
        note: note.to_string(),
        key: Some(key),
        run,
        lamp: None,
    };
    let mut v = vec![
        action("Strengthen reflex…", "adage review strengthen", "s", Run::Key(Tab::Reflexes, 's')),
        action("Weaken reflex…", "adage review weaken", "w", Run::Key(Tab::Reflexes, 'w')),
        action("Extinguish reflex…", "adage review extinguish", "x", Run::Key(Tab::Reflexes, 'x')),
        action("Approve reflex…", "adage review approve", "a", Run::Key(Tab::Reflexes, 'a')),
        action("Pin reflex…", "adage review pin", "n", Run::Key(Tab::Reflexes, 'n')),
        action("Delegate reflex…", "adage review delegate", "d", Run::Key(Tab::Reflexes, 'd')),
        action("Condition this repository now", "adage condition", "c", Run::Key(Tab::Health, 'c')),
        action("Fetch the intent model", "adage models fetch", "m", Run::Key(Tab::Health, 'm')),
        action("Install hooks", "adage init", "i", Run::Key(Tab::Health, 'i')),
        action("Recheck health", "adage doctor", "R", Run::Key(Tab::Health, 'R')),
        action("Pause or resume the live feed", "", "p", Run::Key(Tab::Live, 'p')),
        action("Filter the live feed", "", "/", Run::Key(Tab::Live, '/')),
        action("Focus the map on the selection", "", "f", Run::Key(Tab::Map, 'f')),
    ];
    if let Some(Ok(r)) = &s.routes {
        for row in &r.rows {
            v.push(Entry {
                section: Section::Reflexes,
                title: row.id.clone(),
                note: format!("{} · {} · {}", Theme::strength_word(row.aspect), row.lifecycle(), row.count()),
                key: None,
                run: Run::Reflex(row.reference.clone(), row.id.clone()),
                lamp: Some((row.aspect, row.instinct)),
            });
        }
    }
    let goto = |title: String, note: &str, key: Option<&'static str>, run: Run| Entry {
        section: Section::GoTo,
        title,
        note: note.to_string(),
        key,
        run,
        lamp: None,
    };
    for (tab, name, k) in [
        (Tab::Live, "Live", "1"),
        (Tab::Reflexes, "Reflexes", "2"),
        (Tab::Map, "Map", "3"),
        (Tab::Health, "Health", "4"),
        (Tab::Settings, "Settings", "5"),
    ] {
        let note = match tab {
            Tab::Live => "adage watch",
            Tab::Health => "adage doctor",
            Tab::Settings => "adage config",
            _ => "",
        };
        v.push(goto(name.to_string(), note, Some(k), Run::Tab(tab)));
    }
    v.push(goto("Open the map in the browser".into(), "adage map", Some("o"), Run::Key(Tab::Map, 'o')));
    for (i, row) in s.settings.iter().enumerate() {
        v.push(goto(
            format!("Settings · {}", row.key),
            &format!("adage config set {}", row.key),
            None,
            Run::Setting(i),
        ));
    }
    v
}

/// The entries matching `query`, best first within each section, with matched positions.
pub fn matches(s: &AppState, query: &str) -> Vec<(Entry, Vec<usize>)> {
    let mut scored: Vec<(Entry, i64, Vec<usize>)> = entries(s)
        .into_iter()
        .filter_map(|e| {
            let m = fuzzy::score(query, &e.title).or_else(|| {
                fuzzy::score(query, &e.note).map(|m| fuzzy::Match { score: m.score - 50, positions: vec![] })
            })?;
            Some((e, m.score, m.positions))
        })
        .collect();
    // Stable: entries keep their order among equals, so an empty query lists them as written.
    scored.sort_by(|a, b| a.0.section.cmp(&b.0.section).then(b.1.cmp(&a.1)));
    if query.is_empty() {
        // Without a query, a few reflexes only: the palette is for jumping, the tab for browsing.
        let mut n = 0;
        scored.retain(|(e, _, _)| {
            e.section != Section::Reflexes || {
                n += 1;
                n <= 5
            }
        });
    }
    scored.into_iter().map(|(e, _, p)| (e, p)).collect()
}

/// A title with its matched characters in the command colour.
fn highlighted(title: &str, positions: &[usize], base: Style, t: &Theme, w: usize) -> Vec<Span<'static>> {
    let text = t.pad(&t.text(title), w);
    let mut spans = Vec::new();
    for (i, c) in text.chars().enumerate() {
        let style = if positions.contains(&i) { t.key() } else { base };
        spans.push(Span::styled(c.to_string(), style));
    }
    spans
}

/// The palette over `area`, the screen behind it dimmed.
pub fn draw(f: &mut Frame, area: Rect, s: &AppState, p: &Palette, t: &Theme) {
    // Dim what is behind.
    let buf = f.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].set_style(Style::reset().patch(t.dim()));
        }
    }
    let w = area.width.saturating_sub(4).min(84);
    let h = area.height.saturating_sub(2).min(22);
    if w < 30 || h < 8 {
        return;
    }
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 3, width: w, height: h };
    f.render_widget(Clear, r);
    let inner_w = (w - 4) as usize;
    let found = matches(s, &p.query);
    let sel = p.sel.min(found.len().saturating_sub(1));
    let (tl, tr, bl, br, hz, vt, lj, rj) = if t.ch("╭", "+") == "╭" {
        ("╭", "╮", "╰", "╯", "─", "│", "├", "┤")
    } else {
        ("+", "+", "+", "+", "-", "|", "+", "+")
    };
    let border = t.accent();
    let edge = |l: &'static str, r_: &'static str| {
        Line::from(vec![Span::styled(format!("{l}{}{r_}", hz.repeat(w as usize - 2)), border)])
    };
    let row = |mut spans: Vec<Span<'static>>| {
        let used: usize = spans.iter().map(|s| s.width()).sum();
        spans.insert(0, Span::styled(format!("{vt} "), border));
        spans.push(Span::raw(" ".repeat(inner_w.saturating_sub(used))));
        spans.push(Span::styled(format!(" {vt}"), border));
        Line::from(spans)
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(format!("{tl}{hz}"), border),
        Span::styled(" palette ", t.command().patch(t.bold())),
        Span::styled(format!("{}{tr}", hz.repeat((w as usize).saturating_sub(12))), border),
    ]));
    let count = format!("{} match{}", found.len(), if found.len() == 1 { "" } else { "es" });
    let q = vec![
        Span::styled(": ", t.accent().patch(t.bold())),
        Span::styled(p.query.clone(), t.bold()),
        Span::styled(t.ch("▌", "_"), t.command()),
    ];
    let qw: usize = q.iter().map(|s| s.width()).sum();
    let mut qline = q;
    qline.push(Span::raw(" ".repeat(inner_w.saturating_sub(qw + count.len()))));
    qline.push(Span::styled(count, t.dim()));
    lines.push(row(qline));
    lines.push(edge(lj, rj));
    // The list, scrolled so the selection shows.
    let list_h = (h as usize).saturating_sub(7);
    let mut body: Vec<(Option<usize>, Line<'static>)> = Vec::new();
    let mut last = None;
    for (i, (e, pos)) in found.iter().enumerate() {
        if last != Some(e.section) {
            if last.is_some() {
                body.push((None, row(vec![])));
            }
            body.push((None, row(vec![Span::styled(e.section.title(), t.dim().patch(t.bold()))])));
            last = Some(e.section);
        }
        let chosen = i == sel;
        let mut spans = vec![if chosen {
            Span::styled(format!("{} ", t.glyph(Glyph::Pointer)), t.accent().patch(t.bold()))
        } else {
            Span::raw("  ")
        }];
        let base = if chosen { t.bold() } else { Style::default() };
        let title_w = inner_w.saturating_sub(2 + 36).clamp(16, 36);
        if let Some((a, instinct)) = e.lamp {
            spans.push(Span::styled(format!("{} ", t.aspect_mark(a)), t.aspect(a)));
            spans.push(Span::styled(
                format!("{} ", t.glyph(if instinct { Glyph::Instinct } else { Glyph::Reflex })),
                t.accent(),
            ));
            let used = spans.iter().map(|s| s.width()).sum::<usize>() - 2;
            spans.extend(highlighted(&e.title, pos, base, t, title_w.saturating_sub(used)));
        } else {
            spans.extend(highlighted(&e.title, pos, base, t, title_w));
        }
        let used: usize = spans.iter().map(|s| s.width()).sum();
        let key_w = e.key.map_or(0, |k| k.len() + 1);
        spans
            .push(Span::styled(t.fit(&t.text(&format!(" {}", e.note)), inner_w.saturating_sub(used + key_w)), t.dim()));
        if let Some(k) = e.key {
            let used: usize = spans.iter().map(|s| s.width()).sum();
            spans.push(Span::raw(" ".repeat(inner_w.saturating_sub(used + k.len()))));
            spans.push(Span::styled(k, t.key()));
        }
        body.push((Some(i), row(spans)));
    }
    if found.is_empty() {
        body.push((None, row(vec![Span::styled("Nothing matches. Try a reflex id, a verb, or a tab.", t.dim())])));
    }
    let sel_line = body.iter().position(|(i, _)| *i == Some(sel)).unwrap_or(0);
    let start = sel_line.saturating_sub(list_h.saturating_sub(2)).min(body.len().saturating_sub(list_h));
    let shown: Vec<Line<'static>> = body.into_iter().skip(start).take(list_h).map(|(_, l)| l).collect();
    let pad_rows = list_h.saturating_sub(shown.len());
    lines.extend(shown);
    for _ in 0..pad_rows {
        lines.push(row(vec![]));
    }
    lines.push(edge(lj, rj));
    let mut hint = keys::bar(&[keys::PALETTE], t, inner_w);
    hint.insert(0, Span::raw(""));
    lines.push(row(hint));
    lines.push(row(vec![Span::styled(
        t.fit("every action also shows the adage command that does the same thing", inner_w),
        t.dim(),
    )]));
    lines.push(edge(bl, br));
    f.render_widget(Paragraph::new(lines), r);
}
