//! Recorded hook sessions and the latest conditioning counts are different inventories.
use crate::ui::app::{AppState, Cmd, WantKind};
use crate::ui::tools::Tool;
use crate::ui::view::{self, Ctx};
use crate::ui::{feed, scope};
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use serde_json::Value;

#[derive(Default)]
pub struct State {
    pub data: Option<Result<Value, String>>,
    pub selected: usize,
}

pub fn sessions(s: &AppState) -> Vec<&Value> {
    let Some(Ok(data)) = &s.traces.data else { return vec![] };
    data["trace_sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| s.scope_key().is_none_or(|repo| row["repo"].as_str() == Some(repo)))
        .collect()
}

pub fn key(s: &mut AppState, code: KeyCode) -> Option<Cmd> {
    let count = sessions(s).len();
    match code {
        KeyCode::Up | KeyCode::Char('k') => s.traces.selected = s.traces.selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => s.traces.selected = (s.traces.selected + 1).min(count.saturating_sub(1)),
        KeyCode::Home => s.traces.selected = 0,
        KeyCode::End => s.traces.selected = count.saturating_sub(1),
        KeyCode::Enter => {
            let rows = sessions(s);
            let row = rows.get(s.traces.selected.min(count.saturating_sub(1)))?;
            return Some(Cmd::OpenTrace {
                session: row["session"].as_str()?.into(),
                repo: row["repo"].as_str()?.into(),
            });
        }
        KeyCode::Char('f') => s.open_repos(),
        KeyCode::Char('R') => return Some(Cmd::Want(WantKind::Traces)),
        KeyCode::Char('h') => return Some(Cmd::Do(super::super::app::Action::Tool(Tool::HistorySources))),
        KeyCode::Char('c') => {
            if s.scope_key() != s.here() {
                s.message("Open this repository", vec!["Condition now works in the open checkout. Choose Open repository from the palette to condition another repository.".into()], false);
            } else {
                return Some(Cmd::PreviewLearning);
            }
        }
        _ => {}
    }
    None
}

fn number(v: &Value) -> String {
    v.as_u64().map_or_else(|| "not recorded".into(), |n| n.to_string())
}

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let data = match &s.traces.data {
        None => {
            f.render_widget(Paragraph::new(" Loading recorded sessions and conditioning history…"), area);
            return;
        }
        Some(Err(error)) => {
            f.render_widget(
                Paragraph::new(view::wrap_lines(
                    vec![Line::from(Span::styled(format!(" Could not load traces: {error}"), t.red()))],
                    area.width,
                )),
                area,
            );
            return;
        }
        Some(Ok(data)) => data,
    };
    let rows = sessions(s);
    let mut summary = vec![
        Line::from(Span::styled(
            format!(" Recorded activity: {}", crate::helpers::plural(rows.len(), "session", "sessions")),
            t.bold(),
        )),
        Line::from(Span::styled(" Hook observations and Adage decisions, not all agent transcripts.", t.ink2())),
        Line::raw(""),
    ];
    let histories: Vec<&Value> = data["learning_history"]["repositories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| s.scope_key().is_none_or(|repo| row["repo"].as_str() == Some(repo)))
        .collect();
    if histories.is_empty() {
        summary.push(Line::raw(" Last conditioning: no retained successful run for this scope."));
    } else {
        for row in histories {
            let repo = row["repo"].as_str().unwrap_or("");
            let prefix = if s.scope_key().is_none() { format!("{} · ", scope::friendly(repo)) } else { String::new() };
            summary.push(Line::raw(format!(
                " {prefix}Last conditioning: {} sessions used / {} files read",
                number(&row["sessions_used"]),
                number(&row["files_read"])
            )));
        }
    }
    summary.push(Line::from(Span::styled(
        " Counts describe each repository's latest retained run, not a lifetime total.",
        t.dim(),
    )));
    summary.push(Line::from(Span::styled(" h History sources · c Condition now (open checkout)", t.accent())));
    let summary = view::wrap_lines(t.lines(summary), area.width);
    let head_height = (summary.len() as u16).min(area.height.saturating_sub(4));
    let [head, rule, list] =
        Layout::vertical([Constraint::Length(head_height), Constraint::Length(1), Constraint::Min(1)]).areas(area);
    f.render_widget(Paragraph::new(summary), head);
    f.render_widget(
        Paragraph::new(view::rule("RECORDED SESSIONS · enter opens all recorded events", None, rule.width, t)),
        rule,
    );
    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(view::wrap_lines(
                vec![
                    Line::raw(" No hook sessions recorded here yet."),
                    Line::raw(" Past agent history can still be used for conditioning; press h to inspect sources."),
                ],
                list.width,
            )),
            list,
        );
        return;
    }
    let chosen = s.traces.selected.min(rows.len() - 1);
    let offset = chosen.saturating_sub(list.height.saturating_sub(1) as usize);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(offset)
        .map(|(i, row)| {
            let session = row["session"].as_str().unwrap_or("");
            let harness = row["harnesses"]
                .as_array()
                .map(|h| h.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            let label = format!(
                " {} {}  {}  {} events  {}  {}",
                if i == chosen { ">" } else { " " },
                view::clock(row["end_ts"].as_u64().unwrap_or(0), ctx.tz, true),
                feed::short_session(session, 10),
                number(&row["events_total"]),
                harness,
                scope::friendly(row["repo"].as_str().unwrap_or(""))
            );
            Line::from(Span::styled(
                t.fit(&label, list.width as usize),
                if i == chosen { t.selected() } else { t.ink2() },
            ))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), list);
}

/// The full stored hook session, preserving quiet observations and exact delivered text.
pub fn detail_lines(trace: &Value) -> Vec<String> {
    let mut lines = vec![
        format!("Session {}", trace["session"].as_str().unwrap_or("")),
        format!("Repository: {}", trace["repo"].as_str().unwrap_or("")),
        format!(
            "{} recorded hook events. This is hook activity, not the full agent transcript.",
            number(&trace["events_total"])
        ),
    ];
    for event in trace["events"].as_array().into_iter().flatten() {
        lines.push(String::new());
        lines.push(format!(
            "{} · {} · {} · {}",
            view::clock(event["ts"].as_u64().unwrap_or(0), view::local_offset(), true),
            event["harness"].as_str().unwrap_or("unknown"),
            event["event"].as_str().unwrap_or("event"),
            event["arm"].as_str().unwrap_or("unknown arm")
        ));
        for observed in event["observed"].as_array().into_iter().flatten() {
            lines.push(format!(
                "Observed: {}",
                observed.as_str().map(String::from).unwrap_or_else(|| observed.to_string())
            ));
        }
        let control = event["arm"] == "control";
        if let Some(pretool) = event["pretool"].as_object() {
            let withheld = control || event["withheld"] == true;
            let mode = if withheld { "Withheld" } else { "Applied" };
            for (field, label) in [("rewrite", "command rewrite"), ("advise", "tool advice")] {
                if let Some(value) = pretool.get(field).and_then(Value::as_str) {
                    lines.push(format!("{mode} {label}: {value}"));
                }
            }
            if let Some(interpreter) = pretool.get("interpreter").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                lines.push(format!("Interpreter: {interpreter}"));
            }
        }
        for (field, label) in [
            ("predicted", "Chose next step"),
            ("would_predict", "Would choose next step"),
            ("substituted", "Replaced dead end"),
            ("would_substitute", "Would replace dead end"),
            ("answered", "Answered from recorded result"),
        ] {
            if let Some(value) = event[field].as_str() {
                lines.push(format!("{label}: {value}"));
            }
        }
        for candidate in event["noticed"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            lines.push(format!("Noticed candidate (not delivered): {candidate}"));
        }
        if let Some(reason) = event["continue_reason"].as_str() {
            lines.push(format!("Asked agent to continue: {reason}"));
        }
        if control {
            if let Some(reason) = event["would_continue"].as_str() {
                lines.push(format!("Withheld continuation: {reason}"));
            }
        }
        for (field, label) in [("directives", "Delivered"), ("would_have", "Withheld (control)")] {
            if field == "would_have" && !control {
                continue;
            }
            for directive in event[field].as_array().into_iter().flatten() {
                lines.push(format!("{label}: {}", directive["route"].as_str().unwrap_or("reflex")));
                lines.extend(directive["text"].as_str().unwrap_or("").lines().map(String::from));
            }
        }
        for judgment in event["judgements"].as_array().into_iter().flatten() {
            lines.push(format!(
                "Outcome: {} · {}",
                judgment["route"].as_str().unwrap_or(""),
                judgment["kind"].as_str().unwrap_or("")
            ));
        }
    }
    lines
}
