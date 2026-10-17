//! A focused review of a frozen reflex revision. Navigation never records a decision.
use super::app::{Action, AppState, Cmd, Modal};
use super::routes::{RouteRow, ACTIONS};
use super::tabs::reflexes;
use super::theme::Theme;
use super::view::{self, Ctx};
use super::{kit, words};
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use std::cell::Cell;

pub struct State {
    pub row: RouteRow,
    pub scroll: u16,
    pub action: usize,
    confirm: Option<Confirmation>,
    // Rendering records only viewport geometry, so End then Up moves immediately.
    scroll_limit: Cell<u16>,
}

struct Confirmation {
    title: String,
    lines: Vec<Line<'static>>,
    action: Action,
}

pub fn open(s: &mut AppState) {
    let Some(row) = s.selected_route().cloned() else { return };
    s.reflexes.details = true;
    let action = row.verbs.iter().position(|v| *v == "approve").unwrap_or(0);
    s.modal = Some(Modal::ReflexReview(Box::new(State {
        row,
        scroll: 0,
        action,
        confirm: None,
        scroll_limit: Cell::new(0),
    })));
}

/// Explain eligibility separately from strength and execution authority.
pub(super) fn service(row: &RouteRow) -> &'static str {
    if row.more.execution.is_some() {
        "Execution evidence: reviewed separately from permission to run."
    } else if row.nascent() || row.quiet() {
        "Waiting for approval before advisory delivery."
    } else if row.extinguished() {
        "Out of service: this reflex is extinguished."
    } else if !row.in_index || !matches!(row.lifecycle(), "firm" | "forming" | "dormant") {
        "Not currently available for advice; check Health before expecting delivery."
    } else if row.approved {
        "Approved · active advice. Strength is separate from approval."
    } else if row.lifecycle() == "forming" {
        "Active advice: forming means it is in service while evidence grows."
    } else {
        "Active advice: the runtime delivers it when its conditions match."
    }
}

fn decision_effect(verb: &str, row: &RouteRow) -> String {
    if verb == "approve" && row.instinct && !row.nascent() {
        let mut message = format!(
            "Approve records acceptance. Current strength: {}. {}",
            Theme::strength_word(row.strength),
            service(row)
        );
        if row.strength == adage_tracks::Strength::Prime && row.ceiling == "prime" {
            message.push_str(" This reflex's ceiling is Prime.");
        }
        message
    } else {
        words::verb_effect(verb).into()
    }
}

pub fn key(s: &mut AppState, mut review: Box<State>, code: KeyCode) -> Option<Cmd> {
    review.scroll = review.scroll.min(review.scroll_limit.get());
    if let Some(confirm) = review.confirm.take() {
        match code {
            KeyCode::Enter | KeyCode::Char('y') => return Some(Cmd::Do(confirm.action)),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => review.scroll = 0,
            _ => {
                scroll(&mut review.scroll, code);
                review.confirm = Some(confirm);
            }
        }
        s.modal = Some(Modal::ReflexReview(review));
        return None;
    }
    match code {
        KeyCode::Esc | KeyCode::Char('q') => return None,
        KeyCode::Tab | KeyCode::BackTab => {
            let n = review.row.verbs.len();
            if n > 0 {
                review.action =
                    if code == KeyCode::Tab { (review.action + 1) % n } else { (review.action + n - 1) % n };
            }
        }
        KeyCode::Char('r') => s.reflexes.raw = !s.reflexes.raw,
        KeyCode::Char('v') => s.reflexes.details = !s.reflexes.details,
        KeyCode::Char('t') => {
            reflexes::show_traces(s, &review.row);
            return None;
        }
        KeyCode::Char('o') => {
            let row = &review.row;
            let cmd = Cmd::OpenWeb {
                focus: Some(serde_json::json!([row.repo, row.reference, row.id]).to_string()),
                traces: false,
            };
            s.modal = Some(Modal::ReflexReview(review));
            return Some(cmd);
        }
        KeyCode::Char('d') => {
            reflexes::select(s, &review.row.reference, &review.row.id);
            return s.run_entry(super::palette::Run::Form(super::forms::Kind::Delegate));
        }
        KeyCode::Enter | KeyCode::Char('a' | 's' | 'w' | 'x' | 'n' | 'm') => {
            let verb = if code == KeyCode::Enter {
                review.row.verbs.get(review.action).copied()
            } else {
                ACTIONS.iter().find(|(k, _)| code == KeyCode::Char(*k)).map(|(_, v)| *v)
            };
            if let Some(verb) = verb {
                reflexes::confirm_review(s, &review.row, verb);
                match s.modal.take() {
                    Some(Modal::Confirm { title, lines, action, .. }) => {
                        review.confirm = Some(Confirmation { title, lines, action });
                        review.scroll = 0;
                    }
                    message => {
                        // An unavailable shortcut explains itself, without applying a decision.
                        s.modal = message;
                        return None;
                    }
                }
            }
        }
        _ => scroll(&mut review.scroll, code),
    }
    s.modal = Some(Modal::ReflexReview(review));
    None
}

fn scroll(value: &mut u16, code: KeyCode) {
    *value = match code {
        KeyCode::Up | KeyCode::Char('k') => value.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => value.saturating_add(1),
        KeyCode::PageUp => value.saturating_sub(10),
        KeyCode::PageDown => value.saturating_add(10),
        KeyCode::Home | KeyCode::Char('g') => 0,
        KeyCode::End | KeyCode::Char('G') => u16::MAX,
        _ => *value,
    };
}

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, review: &State, ctx: &Ctx) {
    let t = &ctx.theme;
    let r = view::centered(area, area.width.saturating_sub(2).min(110), area.height.saturating_sub(2));
    f.render_widget(Clear, r);
    let title = review.confirm.as_ref().map_or("Review reflex", |c| c.title.as_str());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(t.border())
        .border_style(t.accent())
        .title(Span::styled(format!(" {title} "), t.bold()));
    let inner = block.inner(r).inner(Margin { horizontal: 1, vertical: 0 });
    f.render_widget(block, r);
    let width = inner.width as usize;
    let (mut lines, mut footer) = if let Some(c) = &review.confirm {
        (
            c.lines.clone(),
            view::wrap_lines(
                vec![Line::from(kit::hints(&[("enter/y", "confirm"), ("esc/n", "back"), ("↑↓", "scroll")], t, width))],
                inner.width,
            ),
        )
    } else {
        let mut lines = vec![Line::from(Span::styled(service(&review.row), t.accent())), Line::raw("")];
        lines.extend(reflexes::card(&review.row, s, ctx, width, inner.height > 30, 1.0));
        let mut footer = Vec::new();
        let mut actions = Vec::new();
        for (i, verb) in review.row.verbs.iter().enumerate() {
            let shortcut = ACTIONS.iter().find(|(_, v)| v == verb).map_or(' ', |(k, _)| *k);
            let style = if i == review.action { t.selected().patch(t.bold()) } else { t.ink2() };
            actions.push(Span::styled(format!(" {shortcut} {} ", words::verb(verb)), style));
            actions.push(Span::raw(" "));
        }
        if actions.is_empty() {
            actions.push(Span::styled("No review decisions available", t.dim()));
        }
        footer.extend(view::wrap_lines(vec![Line::from(actions)], inner.width));
        if let Some(verb) = review.row.verbs.get(review.action) {
            // Keep the selected decision's consequence next to its control.
            footer.extend(view::wrap_lines(
                vec![Line::from(Span::styled(decision_effect(verb, &review.row), t.ink2()))],
                inner.width,
            ));
        }
        footer.push(Line::from(kit::hints(
            &[("esc", "close"), ("↑↓", "scroll"), ("tab", "decision"), ("enter", "choose")],
            t,
            width,
        )));
        (lines, footer)
    };
    lines = view::wrap_lines(t.lines(lines), inner.width);
    let footer_height = (footer.len() as u16).min(inner.height.saturating_sub(2));
    if footer.len() > footer_height as usize && footer_height > 0 {
        let navigation = footer.pop().unwrap();
        footer.truncate(footer_height.saturating_sub(1) as usize);
        footer.push(navigation);
    }
    let [body, rule, controls] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(footer_height)]).areas(inner);
    let max_scroll = lines.len().saturating_sub(body.height as usize).min(u16::MAX as usize) as u16;
    review.scroll_limit.set(max_scroll);
    let offset = review.scroll.min(max_scroll);
    f.render_widget(Paragraph::new(lines).scroll((offset, 0)), body);
    let hint = if max_scroll > 0 {
        format!(
            "{}–{} of {} · PgUp/PgDn scroll",
            offset + 1,
            offset.saturating_add(body.height).min(max_scroll.saturating_add(body.height)),
            max_scroll.saturating_add(body.height)
        )
    } else {
        String::new()
    };
    f.render_widget(Paragraph::new(view::rule(&hint, None, rule.width, t)), rule);
    f.render_widget(Paragraph::new(t.lines(footer)), controls);
}

pub fn outcome_lines(outcome: &adage_app::review::Outcome) -> Vec<String> {
    let after = &outcome.after;
    let strength = Theme::strength_word(after.strength);
    let state = adage_app::words::lifecycle(after.state);
    let mut lines = vec![format!("{} · {}", outcome.route, outcome.reference)];
    lines.push(if outcome.proposal_applied {
        "Proposed change applied.".into()
    } else if outcome.already.is_some() {
        "This decision was already recorded; the reflex is unchanged.".into()
    } else {
        format!("{} recorded.", words::verb(outcome.verb.as_str()))
    });
    lines.push(format!("Strength: {strength}. Lifecycle: {state}."));
    if after.evidence.execution.is_some() {
        lines.push("Execution evidence reviewed; a valid local execution grant is still required.".into());
    } else if after.state.is_live() {
        lines.push("Advice remains subject to the current index and runtime conditions.".into());
        if after.strength == adage_tracks::Strength::Prime
            && adage_app::review::route_ceiling(after) == Some(adage_tracks::Strength::Prime)
        {
            lines.push("This reflex's ceiling is Prime; approval does not raise its strength.".into());
        }
    } else {
        lines.push("This reflex is not in advisory service.".into());
    }
    lines.push("Execution permissions were not changed by this decision.".into());
    if !outcome.pack_approved.is_empty() {
        lines.push(format!("Also approved: {}", outcome.pack_approved.join(", ")));
    }
    lines.extend(outcome.warnings.clone());
    lines
}
