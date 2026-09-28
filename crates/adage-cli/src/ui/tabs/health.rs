//! The Health tab: every `adage doctor` check with its lamp, and one key to fix it.

use crate::doctor::Status;
use crate::ui::app::{AppState, Cmd, WantKind};
use crate::ui::health::Fix;
use crate::ui::view::{self, Ctx};
use crate::ui::words::{self, pad};
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn key(s: &mut AppState, code: KeyCode) -> Option<Cmd> {
    let n = s.checks_len();
    let fix_of = |s: &AppState, want: &dyn Fn(&Fix) -> bool| -> Option<Fix> {
        let h = s.health.as_ref()?;
        let sel = h.fixes.get(s.check_sel).cloned().flatten().filter(|f| want(f));
        sel.or_else(|| h.fixes.iter().flatten().find(|f| want(f)).cloned())
    };
    let fix = match code {
        KeyCode::Up | KeyCode::Char('k') => {
            s.check_sel = s.check_sel.saturating_sub(1);
            return None;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            s.check_sel = (s.check_sel + 1).min(n.saturating_sub(1));
            return None;
        }
        KeyCode::Char('R') => {
            s.toast("running the checks again", true);
            return Some(Cmd::Want(WantKind::Health));
        }
        KeyCode::Enter => {
            let h = s.health.as_ref()?;
            let found = h.fixes.get(s.check_sel).cloned().flatten();
            if found.is_none() {
                // Say why nothing happened, rather than nothing.
                let c = h.checks.get(s.check_sel)?;
                let why = match (c.status, &c.fix) {
                    (Status::Pass, _) => format!("{}: passes, nothing to fix", c.label),
                    (_, Some(fx)) => format!("{}: no one-key fix; run {fx}", c.label),
                    (_, None) => format!("{}: no one-key fix", c.label),
                };
                s.toast(why, c.status == Status::Pass);
                return None;
            }
            found
        }
        KeyCode::Char('i') => fix_of(s, &|f| matches!(f, Fix::InstallHooks(_))),
        KeyCode::Char('m') => Some(Fix::FetchModel),
        KeyCode::Char('c') => Some(Fix::LearnNow),
        KeyCode::Char('s') => Some(Fix::OpenSettings),
        KeyCode::Char('o') => Some(Fix::OpenFailOpen),
        _ => None,
    }?;
    s.start_fix(fix)
}

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let Some(h) = &s.health else {
        f.render_widget(Paragraph::new(Span::styled(" running checks…", t.dim())), area);
        return;
    };
    let [summary, list, _, detail] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(3), Constraint::Length(1), Constraint::Length(5)])
            .areas(area);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {} passed", h.count(Status::Pass)), t.ink2()),
            Span::styled(" · ", t.dim()),
            Span::styled(
                format!("{} warnings", h.count(Status::Warn)),
                if h.count(Status::Warn) > 0 { t.yellow() } else { t.ink2() },
            ),
            Span::styled(" · ", t.dim()),
            Span::styled(
                format!("{} failures", h.count(Status::Fail)),
                if h.count(Status::Fail) > 0 { t.red() } else { t.ink2() },
            ),
            Span::styled("   same checks as ", t.dim()),
            Span::styled("adage doctor", t.command()),
        ])),
        summary,
    );
    let name_w = h.checks.iter().map(|c| c.label.chars().count()).max().unwrap_or(10).min(28);
    let mut lines: Vec<Line> = Vec::new();
    for (i, c) in h.checks.iter().enumerate() {
        let lamp = view::lamp_of(c.status);
        let sel = i == s.check_sel;
        let fixable = h.fixes.get(i).is_some_and(|f| f.is_some());
        lines.push(Line::from(vec![
            Span::styled(if sel { t.ch("▌", ">") } else { " " }, t.accent()),
            Span::raw(" "),
            Span::styled(format!("{} ", t.lamp_mark(lamp)), t.lamp(lamp)),
            Span::styled(pad(&c.label, name_w), if sel { t.bold() } else { ratatui::style::Style::default() }),
            Span::styled(if fixable { t.ch(" ⏎ ", " > ") } else { "   " }, t.command()),
            // Cut with an ellipsis, not the pane's edge: the whole text is in the detail below.
            Span::styled(words::fit(&c.detail, (list.width as usize).saturating_sub(name_w + 7)), t.ink2()),
        ]));
    }
    for n in s.notices.iter().rev().take(3) {
        lines.push(Line::from(Span::styled(format!("   {n}"), t.yellow())));
    }
    let hgt = list.height as usize;
    let offset = s.check_sel.saturating_sub(hgt.saturating_sub(1));
    f.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), list);

    let mut d: Vec<Line> = vec![view::rule("check", None, area.width, t)];
    if let Some(c) = h.checks.get(s.check_sel) {
        d.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(c.label.clone(), t.bold()),
            Span::styled(format!("  {}", c.detail), t.ink2()),
        ]));
        if let Some(fx) = &c.fix {
            d.push(Line::from(vec![Span::styled(" fix  ", t.dim()), Span::styled(fx.clone(), t.command())]));
        }
        if let Some(Some(fix)) = h.fixes.get(s.check_sel) {
            d.push(Line::from(vec![Span::styled(" enter", t.key()), Span::raw(format!(" {}", fix.label()))]));
        }
    }
    f.render_widget(Paragraph::new(view::wrap_lines(d, detail.width)), detail);
}
