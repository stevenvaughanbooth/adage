//! A compact narrative and the next useful actions, while existing tab shortcuts stay stable.
use crate::ui::{
    app::{AppState, Cmd, Tab},
    forms::Kind,
    palette::Run,
    tools::Tool,
    view::{self, Ctx},
};
use ratatui::{
    crossterm::event::KeyCode,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
fn steps() -> Vec<(&'static str, &'static str, Run)> {
    vec![
        ("Open repository", "Choose the project you want Adage to understand.", Run::Form(Kind::OpenRepository)),
        (
            "Connect your agent",
            "adage init connects every agent found: Claude Code through the Adage plugin. Or search connect in the palette.",
            Run::Tab(Tab::Health),
        ),
        ("History sources", "See exactly which local traces are available.", Run::Tool(Tool::HistorySources)),
        ("Condition now", "Preview sources; learn reflexes and bind applicable instincts.", Run::Key(Tab::Health, 'c')),
        ("Review reflexes", "Inspect responses and evidence before approving changes.", Run::Tab(Tab::Reflexes)),
        ("Explore the web map", "Follow complete traces and their measured outcomes.", Run::Web { traces: false }),
    ]
}
pub fn key(s: &mut AppState, code: KeyCode) -> Option<Cmd> {
    match code {
        // `u: update` (the first row, while a newer Adage is out): `adage update` as a job, after
        // its confirmation says what it will do.
        KeyCode::Char('u') => return Some(Cmd::PrepareTool(Tool::Update)),
        KeyCode::Up | KeyCode::Char('k') => s.home_sel = s.home_sel.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => s.home_sel = (s.home_sel + 1).min(steps().len() - 1),
        KeyCode::Enter => {
            if s.home_sel == 1 {
                s.palette = Some(crate::ui::palette::Palette { query: "connect".into(), sel: 0 });
                return None;
            }
            return s.run_entry(steps()[s.home_sel].2.clone());
        }
        _ => {}
    }
    None
}
pub fn draw(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let mut lines = vec![
        Line::from(Span::styled(" Agent traces become reflexes.", t.title())),
        Line::raw(" Adage reads local history and gives your agent the right response at the right moment."),
        Line::raw(" Reflexes come from observed work. Instincts come from packs and bind to this repository."),
        Line::raw(""),
    ];
    if ctx.demo {
        lines.push(Line::from(Span::styled(
            " DEMO · temporary sample data. Open repository to start with your own project.",
            t.yellow(),
        )));
    }
    // A newer Adage, from the release check's cache: information, said where people look first,
    // in plain ink (never a warning colour), with the key that runs `adage update`.
    let update = crate::update::newer(&ctx.paths);
    if let Some(r) = &update {
        lines.push(Line::from(vec![
            Span::styled(format!(" adage {}", env!("CARGO_PKG_VERSION")), t.ink2()),
            Span::styled(format!(" · {} is out · ", r.version), t.ink2()),
            Span::styled("u", t.accent()),
            Span::styled(" update", t.ink2()),
            Span::styled(" (adage update: Adage, its hooks and the plugin together)", t.dim()),
        ]));
    }
    lines.push(Line::from(Span::styled(format!(" Repository: {}", ctx.label), t.ink2())));
    lines.push(Line::raw(""));
    for (i, (title, why, _)) in steps().iter().enumerate() {
        lines.push(Line::from(vec![
            Span::styled(if i == s.home_sel { " > " } else { "   " }, t.accent()),
            Span::styled(title.to_string(), if i == s.home_sel { t.bold() } else { t.ink2() }),
        ]));
        if i == s.home_sel {
            lines.push(Line::from(Span::styled(format!("   {why}"), t.dim())));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::raw(" : or Ctrl-K opens every action. Tab navigates; ? explains the current screen."));
    let lines = view::wrap_lines(t.lines(lines), area.width);
    let selected_line = 7 + usize::from(update.is_some()) + s.home_sel;
    let scroll = selected_line.saturating_sub(area.height.saturating_sub(3) as usize);
    f.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), area);
}
