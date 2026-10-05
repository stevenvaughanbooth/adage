//! The command menu is data, so searching and choosing an action can be tested without a TTY.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Home,
    Live,
    Routes,
    Health,
    Settings,
    Web,
    WebDemo,
    Learn,
    Model,
    Hooks(&'static str),
    AutoLearn,
    Errors,
    LearningLog,
    Refresh,
    Pause,
    Scope,
    Quiet,
    Review(char),
    Route(usize),
    Setting(&'static str),
    Status,
    Survey,
    HistorySources,
    ImportOpenCode,
    Report,
    Graph,
    Form(FormKind),
    Help,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormKind {
    OpenRepository,
    Evaluate,
    InstallModel,
    AddTrack,
    ShareTrack,
    HistoryClaude,
    HistoryCodex,
    HistoryOpenCode,
}

impl FormKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::OpenRepository => "Open a repository",
            Self::Evaluate => "Evaluate a dataset",
            Self::InstallModel => "Install a local model",
            Self::AddTrack => "Import a signed pack",
            Self::ShareTrack => "Export a signed pack",
            Self::HistoryClaude => "Configure Claude Code history",
            Self::HistoryCodex => "Configure Codex history",
            Self::HistoryOpenCode => "Configure OpenCode history",
        }
    }

    pub fn fields(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::OpenRepository => &[("Repository folder", "Choose a Git repository to learn from and inspect")],
            Self::Evaluate => &[("Dataset folder", "Folder containing the evaluation sessions")],
            Self::InstallModel => &[("Model folder", "Local folder containing the embedding model files")],
            Self::HistoryClaude => &[("History folder", "Claude Code projects folder; overrides automatic detection")],
            Self::HistoryCodex => &[("History folder", "Codex sessions folder; overrides automatic detection")],
            Self::HistoryOpenCode => {
                &[("Exports folder", "Folder of OpenCode export JSON files; use Import OpenCode history to refresh")]
            }
            Self::AddTrack => &[
                ("Pack folder", "Folder containing track.toml and its signature"),
                ("Trust signer", "false verifies an existing trust; true explicitly trusts this signer"),
            ],
            Self::ShareTrack => &[
                ("Collection name", "Installed collection name, such as my-repo/my-collection"),
                ("Output folder", "Destination for the signed copy; an SSH signing agent is required"),
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub group: &'static str,
    pub title: String,
    pub detail: String,
    pub command: Command,
}

impl Entry {
    pub fn new(group: &'static str, title: impl Into<String>, detail: impl Into<String>, command: Command) -> Self {
        Self { group, title: title.into(), detail: detail.into(), command }
    }
}

/// Every query word must occur in the title, category or explanation. This lets people use
/// either the product's names or their own goal ("download model", "install codex", "log").
pub fn matches(entries: &[Entry], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    let words: Vec<_> = query.split_whitespace().collect();
    let mut found: Vec<_> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            let haystack = format!("{} {} {}", e.group, e.title, e.detail).to_lowercase();
            words.iter().all(|word| haystack.contains(word))
        })
        .map(|(i, _)| i)
        .collect();
    if !words.is_empty() {
        found.sort_by_key(|i| !entries[*i].title.to_lowercase().starts_with(query.trim()));
    }
    found
}

#[derive(Debug, Default)]
pub struct Palette {
    pub query: String,
    pub selected: usize,
}

pub struct Form {
    pub kind: FormKind,
    pub values: Vec<String>,
    pub selected: usize,
    pub error: Option<String>,
}

impl Form {
    pub fn new(kind: FormKind) -> Self {
        let values = kind
            .fields()
            .iter()
            .map(|(label, _)| if *label == "Trust signer" { "false".into() } else { String::new() })
            .collect();
        Self { kind, values, selected: 0, error: None }
    }
}
