//! The Settings tab: `config.toml`, as `adage config` shows it, edited in place.

use crate::ui::app::{Action, AppState, Cmd, Modal};
use crate::ui::view::{self, Ctx};
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn key(s: &mut AppState, code: KeyCode) -> Option<Cmd> {
    let n = s.settings.len();
    match code {
        KeyCode::Up | KeyCode::Char('k') => s.setting_sel = s.setting_sel.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => s.setting_sel = (s.setting_sel + 1).min(n.saturating_sub(1)),
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('e') => {
            let row = s.settings.get(s.setting_sel)?;
            // Auto-condition starts or stops background work: it asks first, as Health's fix does.
            if row.key == "learn.auto" {
                let on = row.value != "true";
                s.confirm_auto_condition(on);
                return None;
            }
            if row.boolean() {
                let next = if row.value == "true" { "false" } else { "true" };
                return Some(Cmd::Do(Action::SetConfig { key: row.key, value: next.into() }));
            }
            if row.key == "learn.thresholds" && code == KeyCode::Char(' ') {
                let next = if row.value == "demo" { "default" } else { "demo" };
                return Some(Cmd::Do(Action::SetConfig { key: row.key, value: next.into() }));
            }
            if row.key == "ui.notices" && code == KeyCode::Char(' ') {
                let next = if row.value == "off" { "daily" } else { "off" };
                return Some(Cmd::Do(Action::SetConfig { key: row.key, value: next.into() }));
            }
            s.modal = Some(Modal::Edit { key: row.key, buffer: row.value.clone(), error: None });
        }
        _ => {}
    }
    None
}

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let mut lines = vec![
        Line::from(vec![Span::styled(" config  ", t.dim()), Span::raw(crate::detect::tilde(&s.config_path))]),
        Line::from(vec![
            Span::styled(" the same settings as ", t.dim()),
            Span::styled("adage config", t.command()),
            Span::styled("; environment variables win over the file", t.dim()),
        ]),
        Line::raw(""),
    ];
    for p in &s.settings_problems {
        lines.push(Line::from(Span::styled(format!(" ! {p}"), t.yellow())));
    }
    if !s.settings_problems.is_empty() {
        lines.push(Line::raw(""));
    }
    let key_w = s.settings.iter().map(|r| r.key.len()).max().unwrap_or(10);
    let val_w = s.settings.iter().map(|r| r.value.chars().count().max(2)).max().unwrap_or(8).min(40);
    let mut sel_line = 0;
    for (i, row) in s.settings.iter().enumerate() {
        let sel = i == s.setting_sel;
        if sel {
            sel_line = lines.len();
        }
        let shown = if row.value.is_empty() { "\"\"".to_string() } else { row.value.clone() };
        let source_style = if row.source == "default" { t.dim() } else { t.accent() };
        lines.push(Line::from(vec![
            Span::styled(if sel { t.ch("▌", ">") } else { " " }, t.accent()),
            Span::styled(format!("{:<key_w$}", row.key), if sel { t.bold() } else { Style::default() }),
            Span::raw("  "),
            Span::styled(format!("{shown:<val_w$}"), t.bold()),
            Span::raw("  "),
            Span::styled(format!("({})", row.source), source_style),
        ]));
        lines.push(Line::from(Span::styled(format!("   {}", row.doc), t.dim())));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " enter edits (space toggles on/off and switches thresholds); every value is validated before it is saved.",
        t.dim(),
    )));
    // Wrapped here, each continuation under its own text, so the scroll counts screen rows.
    let mut rows = Vec::new();
    let mut sel_row = 0;
    for (i, l) in lines.into_iter().enumerate() {
        if i == sel_line {
            sel_row = rows.len();
        }
        rows.extend(view::wrap_lines(t.lines(vec![l]), area.width));
    }
    let offset = sel_row.saturating_sub((area.height as usize).saturating_sub(3));
    f.render_widget(Paragraph::new(rows).scroll((offset as u16, 0)), area);
}
