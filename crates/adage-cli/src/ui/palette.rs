//! The command palette (`:` or ctrl-k): fuzzy search over every action, every reflex and every
//! place in the app. Each action shows the `adage` command that does the same from a shell, so
//! the palette teaches the CLI.

use super::app::{AppState, Tab};
use super::forms::Kind as FormKind;
use super::fuzzy;
use super::keys;
use super::theme::Theme;
use super::tools::Tool;
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
    Form(FormKind),
    Connect(&'static str),
    Tool(Tool),
    Web {
        traces: bool,
    },
    Revoke,
    /// Press `key` in `tab` (a review action applies to the selected reflex).
    Key(Tab, char),
    /// Select a reflex in the Reflexes tab: `(reference, id)`.
    Reflex(String, String),
    Tab(Tab),
    /// Open the repository switcher.
    Repos,
    /// Show one repository (`Some(key)`) or every one (`None`) in Live, Reflexes and Map.
    Scope(Option<String>),
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
    pub lamp: Option<(adage_tracks::Strength, bool)>,
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
        action("Open repository", "Choose a Git folder; leave the demo", "", Run::Form(FormKind::OpenRepository)),
        action("Connect Claude Code", "Register Claude hooks; preview exact changes", "", Run::Connect("claude-code")),
        action("Connect Codex", "Register Codex hooks; then accept in /hooks", "", Run::Connect("codex")),
        action("Connect OpenCode", "Register OpenCode hooks", "", Run::Connect("opencode")),
        action("History sources", "Detected and configured trace folders", "", Run::Tool(Tool::HistorySources)),
        action(
            "Configure Claude Code history",
            "Choose Claude projects folder",
            "",
            Run::Form(FormKind::HistoryClaude),
        ),
        action("Configure Codex history", "Choose Codex sessions folder", "", Run::Form(FormKind::HistoryCodex)),
        action("Configure OpenCode history", "Choose exports folder", "", Run::Form(FormKind::HistoryOpenCode)),
        action(
            "Import OpenCode history",
            "Copy local sessions for this repository",
            "",
            Run::Tool(Tool::ExportOpenCode),
        ),
        action("Condition now", "Learn from history; preview sources first", "c", Run::Key(Tab::Health, 'c')),
        action("Sense session history", "Read-only survey of agent traces", "", Run::Tool(Tool::Survey)),
        action("Installed reflexes and evidence", "Status report", "", Run::Tool(Tool::Status)),
        action("Measured savings", "With Adage versus control", "", Run::Tool(Tool::Savings)),
        action("Guidance outcomes", "Report observed agent responses", "", Run::Tool(Tool::Report)),
        action("Graph report", "Situations, reflexes, and responses", "", Run::Tool(Tool::Graph)),
        action("Installed packs and instincts", "List active pack bindings", "", Run::Tool(Tool::Packs)),
        action("Available packs", "Browse marketplace", "", Run::Tool(Tool::Marketplace)),
        action("Enable pack", "Bind an installed instinct pack", "", Run::Form(FormKind::PackOn)),
        action("Disable pack", "Stop binding a pack in this repository", "", Run::Form(FormKind::PackOff)),
        action("Import a signed pack", "Verify identity, then install", "", Run::Form(FormKind::AddPack)),
        action("Export a signed pack", "Sign and share a collection", "", Run::Form(FormKind::SharePack)),
        action("Evaluate a dataset", "Offline replay evaluation", "", Run::Form(FormKind::Evaluate)),
        action("Install a local model", "Validate a model folder", "", Run::Form(FormKind::InstallModel)),
        action("Explore complete traces in browser", "Open live session graphs", "", Run::Web { traces: true }),
        action("Strengthen reflex…", "adage review strengthen", "s", Run::Key(Tab::Reflexes, 's')),
        action("Revoke selected delegation", "Withdraw permission to execute", "", Run::Revoke),
        action("Weaken reflex…", "adage review weaken", "w", Run::Key(Tab::Reflexes, 'w')),
        action("Extinguish reflex…", "adage review extinguish", "x", Run::Key(Tab::Reflexes, 'x')),
        action("Approve reflex…", "adage review approve", "a", Run::Key(Tab::Reflexes, 'a')),
        action("Pin reflex…", "adage review pin", "n", Run::Key(Tab::Reflexes, 'n')),
        action("Delegate reflex…", "adage review delegate", "d", Run::Key(Tab::Reflexes, 'd')),
        action("Fetch the intent model", "adage models fetch", "m", Run::Key(Tab::Health, 'm')),
        action("Install hooks", "adage init", "i", Run::Key(Tab::Health, 'i')),
        action("Recheck health", "adage doctor", "R", Run::Key(Tab::Health, 'R')),
        action("Update Adage", "adage update", "", Run::Tool(Tool::Update)),
        action("Pause or resume the live feed", "", "p", Run::Key(Tab::Live, 'p')),
        action("Filter the live feed", "", "/", Run::Key(Tab::Live, '/')),
        action("Conditioning runs", "Every run and what it changed (Live)", "l", Run::Key(Tab::Live, 'l')),
        action("Focus the map on the selection", "", "f", Run::Key(Tab::Map, 'f')),
        action("Choose a repository", "Which repository Live, Reflexes and Map show", "f", Run::Repos),
    ];
    if let Some(Ok(r)) = &s.routes {
        // The reflexes of the repository shown, as in the Reflexes tab.
        for row in r.rows.iter().filter(|r| s.in_scope(r)) {
            v.push(Entry {
                section: Section::Reflexes,
                title: row.id.clone(),
                note: format!("{} · {} · {}", Theme::strength_word(row.strength), row.lifecycle(), row.count()),
                key: None,
                run: Run::Reflex(row.reference.clone(), row.id.clone()),
                lamp: Some((row.strength, row.instinct)),
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
        (Tab::Home, "Home", "6"),
    ] {
        let note = match tab {
            Tab::Live => "adage watch",
            Tab::Health => "adage doctor",
            Tab::Settings => "adage config",
            _ => "",
        };
        v.push(goto(name.to_string(), note, Some(k), Run::Tab(tab)));
    }
    v.push(goto("Open the map in the browser".into(), "adage map", Some("o"), Run::Web { traces: false }));
    // Each repository, to show it in Live, Reflexes and Map; and every repository.
    let shown = s.scope_key().map(String::from);
    for k in s.known_repos() {
        let note = if shown.as_deref() == Some(k.key.as_str()) { "shown now" } else { "show in Live, Reflexes, Map" };
        v.push(goto(format!("Repository · {}", k.name), note, None, Run::Scope(Some(k.key))));
    }
    let note = if shown.is_none() { "shown now" } else { "show every repository's activity" };
    v.push(goto("Repository · all repositories".into(), note, None, Run::Scope(None)));
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
            // Treat words independently, so “connect codex” cannot greedily consume the
            // leading C of Codex while matching “connect”. A compact query stays fuzzy.
            let words: Vec<_> = query.split_whitespace().collect();
            let mut m = fuzzy::Match { score: 0, positions: vec![] };
            for word in &words {
                let part = fuzzy::score(word, &e.title).or_else(|| {
                    fuzzy::score(word, &e.note).map(|part| fuzzy::Match { score: part.score - 50, positions: vec![] })
                })?;
                m.score += part.score;
                m.positions.extend(part.positions);
            }
            if !words.is_empty() {
                m.score /= words.len() as i64;
            }
            m.positions.sort_unstable();
            m.positions.dedup();
            Some((e, m.score, m.positions))
        })
        .collect();
    // Stable: entries keep their order among equals, so an empty query lists them as written.
    scored.sort_by(|a, b| {
        if query.is_empty() {
            a.0.section.cmp(&b.0.section).then(b.1.cmp(&a.1))
        } else {
            b.1.cmp(&a.1).then(a.0.section.cmp(&b.0.section))
        }
    });
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
    // Titles are what you choose by, so they get the room first (up to 60% of the width, as wide
    // as the longest one needs); the notes take what is left.
    let longest = found.iter().map(|(e, _)| e.title.chars().count() + 2 + if e.lamp.is_some() { 4 } else { 0 }).max();
    let title_w = longest.unwrap_or(16).clamp(16, (inner_w * 3 / 5).max(16));
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
        if let Some((a, instinct)) = e.lamp {
            spans.push(Span::styled(format!("{} ", t.strength_mark(a)), t.strength(a)));
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
        t.fit("Choose an action here; no terminal command copying needed", inner_w),
        t.dim(),
    )]));
    lines.push(edge(bl, br));
    f.render_widget(Paragraph::new(lines), r);
}
