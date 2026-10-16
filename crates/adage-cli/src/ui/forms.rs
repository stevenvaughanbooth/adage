//! Guided inputs for palette operations. Values stay literal; no shell interpretation.
use super::{
    app::{AppState, Cmd, Modal},
    theme::Theme,
    tools::Tool,
    view,
};
use ratatui::{
    crossterm::event::KeyCode,
    layout::Rect,
    text::{Line, Span},
    Frame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    OpenRepository,
    Evaluate,
    InstallModel,
    AddPack,
    SharePack,
    HistoryClaude,
    HistoryCodex,
    HistoryOpenCode,
    Delegate,
    PackOn,
    PackOff,
}
impl Kind {
    pub fn title(self) -> &'static str {
        match self {
            Self::OpenRepository => "Open repository",
            Self::Evaluate => "Evaluate a dataset",
            Self::InstallModel => "Install a local model",
            Self::AddPack => "Import a signed pack",
            Self::SharePack => "Export a signed pack",
            Self::HistoryClaude => "Configure Claude Code history",
            Self::HistoryCodex => "Configure Codex history",
            Self::HistoryOpenCode => "Configure OpenCode history",
            Self::Delegate => "Delegate selected reflex",
            Self::PackOn => "Enable pack",
            Self::PackOff => "Disable pack",
        }
    }
    pub fn fields(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::OpenRepository => &[("Repository folder", "Git repository to inspect and condition")],
            Self::Evaluate => &[("Dataset folder", "Folder containing evaluation sessions")],
            Self::InstallModel => &[("Model folder", "Existing local embedding model folder")],
            Self::AddPack => &[
                ("Pack folder", "Signed pack.toml pack directory"),
                ("Trust signer", "false, or true after checking the displayed fingerprint"),
            ],
            Self::SharePack => &[
                ("Source", "Installed collection reference or pack directory"),
                ("Output folder", "Destination for the signed pack; uses your SSH signing agent"),
            ],
            Self::HistoryClaude => &[("History folder", "Claude Code projects folder; replaces automatic detection")],
            Self::HistoryCodex => &[("History folder", "Codex sessions folder; replaces automatic detection")],
            Self::HistoryOpenCode => {
                &[("Exports folder", "OpenCode export JSON files; Import OpenCode history refreshes it")]
            }
            Self::Delegate => &[
                ("Level", "e1 prefetch or e2 action; execution needs this explicit grant"),
                ("Outputs", "E2: comma-separated writable globs, optional for verify"),
                ("Cache inputs", "E2: comma-separated input globs, optional"),
                ("Effect", "E2: verify or regenerate; ignored for e1"),
            ],
            Self::PackOn | Self::PackOff => &[("Pack name", "Installed pack name from Installed packs and instincts")],
        }
    }
}
#[derive(Debug)]
pub struct Form {
    pub kind: Kind,
    pub values: Vec<String>,
    pub selected: usize,
    pub error: Option<String>,
    pub reflex: Option<(String, String)>,
}
impl Form {
    pub fn new(kind: Kind) -> Self {
        let values = kind
            .fields()
            .iter()
            .map(|(name, _)| {
                match *name {
                    "Trust signer" => "false",
                    "Level" => "e1",
                    "Effect" => "verify",
                    _ => "",
                }
                .into()
            })
            .collect();
        Self { kind, values, selected: 0, error: None, reflex: None }
    }
}
pub fn key(s: &mut AppState, mut form: Form, code: KeyCode) -> Option<Cmd> {
    match code {
        KeyCode::Esc => return None,
        KeyCode::Tab | KeyCode::Down => form.selected = (form.selected + 1) % form.values.len(),
        KeyCode::BackTab | KeyCode::Up => form.selected = (form.selected + form.values.len() - 1) % form.values.len(),
        KeyCode::Backspace => {
            form.values[form.selected].pop();
            form.error = None;
        }
        KeyCode::Char(c) if !c.is_control() && form.values[form.selected].len() < 4096 => {
            form.values[form.selected].push(c);
            form.error = None;
        }
        KeyCode::Enter => {
            let required = if form.kind == Kind::Delegate { 1 } else { form.values.len() };
            if let Some(i) = form.values.iter().take(required).position(|x| x.trim().is_empty()) {
                form.selected = i;
                form.error = Some(format!("Enter {} to continue.", form.kind.fields()[i].0));
            } else {
                let v = |i: usize| form.values[i].trim().to_string();
                let tool = match form.kind {
                    Kind::OpenRepository => return Some(Cmd::OpenRepository(v(0).into())),
                    Kind::Evaluate => Tool::Eval(v(0).into()),
                    Kind::InstallModel => Tool::InstallModel(v(0).into()),
                    Kind::HistoryClaude => {
                        Tool::SetHistorySource { harness: "claude-code".into(), directory: v(0).into() }
                    }
                    Kind::HistoryCodex => Tool::SetHistorySource { harness: "codex".into(), directory: v(0).into() },
                    Kind::HistoryOpenCode => {
                        Tool::SetHistorySource { harness: "opencode".into(), directory: v(0).into() }
                    }
                    Kind::AddPack => {
                        if !matches!(v(1).as_str(), "true" | "false") {
                            form.selected = 1;
                            form.error = Some("Trust signer must be true or false.".into());
                            s.modal = Some(Modal::Form(form));
                            return None;
                        }
                        Tool::AddTrack { directory: v(0).into(), trust: v(1) == "true", expected_digest: None }
                    }
                    Kind::SharePack => Tool::ShareTrack { track: v(0), output: v(1).into(), key: None },
                    Kind::Delegate => {
                        let (reference, route) = form.reflex.clone()?;
                        Tool::Delegate { reference, route, level: v(0), outputs: v(1), cache: v(2), effect: v(3) }
                    }
                    Kind::PackOn | Kind::PackOff => Tool::Pack { name: v(0), enabled: form.kind == Kind::PackOn },
                };
                return Some(if tool.mutates() {
                    Cmd::PrepareTool(tool)
                } else {
                    Cmd::Do(super::app::Action::Tool(tool))
                });
            }
        }
        _ => {}
    }
    s.modal = Some(Modal::Form(form));
    None
}
pub fn draw(f: &mut Frame, area: Rect, form: &Form, t: &Theme) {
    let mut lines = vec![Line::raw("Tab / Shift-Tab moves between fields. Enter reviews the action."), Line::raw("")];
    for (i, (label, hint)) in form.kind.fields().iter().enumerate() {
        lines.push(Line::from(Span::styled(label.to_string(), t.bold())));
        lines.push(Line::from(Span::styled(hint.to_string(), t.dim())));
        let value = &form.values[i];
        let shown = super::words::fit_left(value, (area.width as usize).saturating_sub(12), t.more());
        lines.push(Line::from(Span::styled(
            format!(
                "{} {shown}{}",
                if i == form.selected { ">" } else { " " },
                if i == form.selected { t.ch("▏", "_") } else { "" }
            ),
            if i == form.selected { t.accent() } else { t.ink2() },
        )));
        lines.push(Line::raw(""));
    }
    if let Some(error) = &form.error {
        lines.push(Line::from(Span::styled(error.clone(), t.red())));
    }
    lines.push(Line::raw("Esc cancels. Paths support ~/; shell substitutions are never expanded."));
    // Scroll along with the field so its active input stays visible on short terminals.
    let scroll = (form.selected as u16 * 4).saturating_sub(area.height.saturating_sub(9));
    view::popup(f, area, form.kind.title(), lines, t, scroll);
}
