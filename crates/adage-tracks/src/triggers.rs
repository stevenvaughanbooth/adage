//! What can make a repository's routes act, precomputed when the index is compiled (Design §13.2:
//! "no match — rejected in the hook client from a precomputed key set").
//!
//! A hook asks these before it loads the command table, parses any command or takes the state
//! lock (`adage_runtime::fast`). They are deliberately coarse: they say "this event might
//! matter" far more often than it does, and must never say "cannot matter" when it can.

use crate::model::{CtxItem, Route, Step, Trigger};
use adage_core::spans::SpanKind;
use serde::{Deserialize, Serialize};

/// Interpreters whose script is part of a command's identity (as in `keys::template_command`).
const INTERPRETERS: [&str; 9] = ["python", "python3", "node", "bash", "sh", "zsh", "ruby", "perl", "deno"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Triggers {
    /// Span kinds that prompt routes bind from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_spans: Vec<SpanKind>,
    /// Learned value types that prompt routes bind from (`SlotSource::PromptValue`): a prompt
    /// with no token any of them accepts cannot bind one ([`Triggers::prompt_might_bind`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_types: Vec<adage_core::vtype::Runs>,
    /// Intent routes: every prompt must be embedded.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub intent: bool,
    /// Routes announced at the first prompt of a session (dead ends).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub first_prompt: bool,
    /// Words one of which a tool call's input must contain for it to match an after-command or
    /// after-failure trigger or any command item of a context, or to run a command a dead end
    /// avoids (recorded for measurement).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<String>,
    /// Globs whose edits make finish-line obligations, or are an edit item of a context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edit_globs: Vec<String>,
    /// A trigger no word can stand for (not a plain command key): every call takes the full path.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub any_call: bool,
}

fn basename(t: &str) -> &str {
    t.rsplit(['/', '\\']).next().unwrap_or(t)
}

/// The word a command's input must contain: the script's name for an interpreter running a
/// script, else the program's name (`python3 scripts/wi-lookup {item}` → `wi-lookup`,
/// `make gen` → `make`). `None` when there is no such word.
pub fn command_word(argv: &[&str]) -> Option<String> {
    let program = argv.iter().find(|t| !t.contains('='))?;
    let at = argv.iter().position(|t| t == program)?;
    let mut word = basename(program);
    if INTERPRETERS.contains(&word) {
        if let Some(script) = argv[at + 1..].iter().find(|t| !t.starts_with('-')) {
            if !script.starts_with('{') {
                word = basename(script);
            }
        }
    }
    (!word.is_empty() && !word.starts_with('{')).then(|| word.to_string())
}

/// The word for a canonical command key (`exec …`); `None` for any other kind of key.
pub fn key_word(key: &str) -> Option<String> {
    let rest = key.strip_prefix("exec ")?;
    command_word(&rest.split_whitespace().collect::<Vec<_>>())
}

/// The literal pieces a path matching `glob` must contain, split at wildcards, bracket
/// expressions and path separators: `schema/*.json` → `schema`, `.json`. Empty when the glob has
/// no literal piece (`*`), so any path could match.
pub fn glob_fragments(glob: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = glob.chars();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '*' | '?' | '/' | '\\' => flush(&mut cur, &mut out),
            '[' => {
                flush(&mut cur, &mut out);
                // Skip the bracket expression: it stands for one character, not its contents.
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                }
            }
            c => cur.push(c),
        }
    }
    flush(&mut cur, &mut out);
    out
}

impl Triggers {
    pub fn of<'a>(routes: impl IntoIterator<Item = &'a Route>) -> Triggers {
        let mut t = Triggers::default();
        for r in routes {
            match &r.trigger {
                Trigger::Prompt { span } => {
                    // A typed slot binds from its own type, not from the (legacy) span kind.
                    let typed: Vec<&adage_core::vtype::Runs> = r
                        .slots
                        .iter()
                        .filter_map(|s| match &s.source {
                            crate::model::SlotSource::PromptValue { vtype, .. } => Some(&vtype.runs),
                            _ => None,
                        })
                        .collect();
                    if typed.is_empty() || r.slots.iter().any(|s| s.source.vtype().is_none()) {
                        t.prompt_spans.push(*span);
                    }
                    t.prompt_types.extend(typed.into_iter().cloned());
                }
                Trigger::Intent { .. } => t.intent = true,
                Trigger::SessionStart => t.first_prompt = true,
                Trigger::AfterCommand { command } | Trigger::AfterFailure { command, .. } => match key_word(command) {
                    Some(w) => t.words.push(w),
                    None => t.any_call = true,
                },
                Trigger::AfterEdit { paths } => t.edit_globs.push(paths.clone()),
                // Track G §7.3: context matching reads alphabet events only, so what can matter is
                // an event of *any* item of the context, not only its last: each one moves the
                // session's context state. The words of every command, the literal pieces of every
                // glob, every learned value type, and any intent.
                Trigger::Context { items, .. } => t.add_items(items),
            }
            // A dead end's avoided runners: their runs are recorded (`adage graph` counts them).
            if let Step::Avoid { avoid, .. } = &r.step {
                for runner in avoid.split(" / ") {
                    match command_word(&runner.split_whitespace().collect::<Vec<_>>()) {
                        Some(w) => t.words.push(w),
                        None => t.any_call = true,
                    }
                }
            }
        }
        t.normalize();
        t
    }

    /// These triggers with a repository's candidates folded in (`crate::candidates`, plan D1):
    /// a candidate is walked like a route's context, so every event of any of its items must
    /// reach the full path, or the fast path would skip an event that notices one.
    pub fn with_candidates<'a>(
        mut self,
        candidates: impl IntoIterator<Item = &'a crate::candidates::Candidate>,
    ) -> Triggers {
        for c in candidates {
            if let Trigger::Context { items, .. } = &c.trigger {
                self.add_items(items);
            }
        }
        self.normalize();
        self
    }

    /// The words of every command, the literal pieces of every glob, every learned value type,
    /// and any intent, of these context items.
    fn add_items(&mut self, items: &[CtxItem]) {
        for it in items {
            match it {
                CtxItem::Act { keys } | CtxItem::Fail { keys, .. } => {
                    for k in keys {
                        match key_word(k) {
                            Some(w) => self.words.push(w),
                            None => self.any_call = true,
                        }
                    }
                }
                CtxItem::Edit { glob } => self.edit_globs.push(glob.clone()),
                CtxItem::PromptValue { vtype } => self.prompt_types.push(vtype.runs.clone()),
                CtxItem::Intent { .. } => self.intent = true,
            }
        }
    }

    fn normalize(&mut self) {
        self.prompt_spans.sort();
        self.prompt_spans.dedup();
        self.prompt_types.sort();
        self.prompt_types.dedup();
        self.words.sort();
        self.words.dedup();
        self.edit_globs.sort();
        self.edit_globs.dedup();
    }

    /// Whether a tool call with this input (the tool's name, its input as JSON, and the working
    /// directory) might match a trigger: it contains one of the words, or every literal piece of
    /// an edit glob. A call that might not is still decided in full whenever the session has a
    /// pending judgement or an obligation (the caller checks those).
    pub fn call_might_match(&self, haystack: &str) -> bool {
        self.any_call
            || self.words.iter().any(|w| haystack.contains(w.as_str()))
            || self.edit_globs.iter().any(|g| glob_fragments(g).iter().all(|f| haystack.contains(f.as_str())))
    }

    /// Whether a prompt might bind a learned value type: one of its tokens (as the runtime
    /// tokenises it, `adage_core::tokens`) is accepted by one of the types. Exact, not coarse:
    /// the runtime binds only such a token.
    pub fn prompt_might_bind(&self, text: &str) -> bool {
        !self.prompt_types.is_empty()
            && adage_core::tokens::tokens(text, adage_core::tokens::PROMPT_TOKENS)
                .iter()
                .any(|tok| self.prompt_types.iter().any(|t| adage_core::vtype::matches(t, &tok.text)))
    }

    /// Whether any call at all could matter (no word, glob or opaque trigger).
    pub fn calls_matter(&self) -> bool {
        self.any_call || !self.words.is_empty() || !self.edit_globs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_stand_for_the_command() {
        assert_eq!(key_word("exec python3 scripts/wi-lookup {arg}").as_deref(), Some("wi-lookup"));
        assert_eq!(key_word("exec scripts/wi-lookup {arg}").as_deref(), Some("wi-lookup"));
        assert_eq!(key_word("exec python3 -m pytest").as_deref(), Some("pytest"));
        assert_eq!(key_word("exec make gen").as_deref(), Some("make"));
        assert_eq!(key_word("exec ACME_ENV=test make test").as_deref(), Some("make"));
        assert_eq!(key_word("exec python3 {path}").as_deref(), Some("python3"));
        assert_eq!(key_word(r"exec C:\x\scripts\run.cmd").as_deref(), Some("run.cmd"));
        assert_eq!(key_word("read {path}"), None);
        assert_eq!(command_word(&["python", "-m", "pytest"]).as_deref(), Some("pytest"));
    }

    #[test]
    fn glob_fragments_are_the_literal_pieces() {
        assert_eq!(glob_fragments("schema/*.json"), vec!["schema", ".json"]);
        assert_eq!(glob_fragments("*.py"), vec![".py"]);
        assert_eq!(glob_fragments("src/[ab]x/**/*.rs"), vec!["src", "x", ".rs"]);
        assert!(glob_fragments("*").is_empty());
    }

    /// Track G §7.3: every item of a context can move the session's context state, so each one's
    /// words, globs and value types are in the triggers, not only the last item's.
    #[test]
    fn every_context_item_is_in_the_triggers() {
        use crate::model::*;
        let runs = adage_core::vtype::Runs::default();
        let vtype =
            ValueType { id: "vt-0".into(), runs: runs.clone(), regex: String::new(), samples_hash: String::new() };
        let r = Route {
            id: "ctx".into(),
            summary: String::new(),
            trigger: Trigger::Context {
                items: vec![
                    CtxItem::Fail { keys: vec!["exec python3 -m pytest".into()], signature: "boom".into() },
                    CtxItem::Edit { glob: "schema/*.json".into() },
                    CtxItem::PromptValue { vtype },
                    CtxItem::Act { keys: vec!["exec make gen".into(), "exec {path}".into()] },
                ],
                within: 8,
            },
            step: Step::Run { command: "make check".into() },
            slots: vec![],
            effect: adage_core::Effect::Verify,
            strength: crate::model::Strength::Prime,
            state: RouteState::Firm,
            exit: vec![],
            evidence: Evidence::default(),
            authored: false,
        };
        let t = Triggers::of([&r]);
        assert_eq!(t.words, ["make", "pytest"]);
        assert_eq!(t.edit_globs, ["schema/*.json"]);
        assert_eq!(t.prompt_types, [runs]);
        assert!(t.any_call, "a key no word stands for");
        assert!(!t.intent);
    }

    #[test]
    fn calls_that_might_match() {
        let t = Triggers {
            words: vec!["wi-lookup".into()],
            edit_globs: vec!["schema/*.json".into()],
            ..Default::default()
        };
        assert!(t.call_might_match(r#"/r Bash {"command":"./scripts/wi-lookup W-1"}"#));
        assert!(t.call_might_match(r#"/r/schema Edit {"file_path":"a.json"}"#), "a piece in the cwd");
        assert!(!t.call_might_match(r#"/r Bash {"command":"ls -la"}"#));
        assert!(!t.call_might_match(r#"/r Edit {"file_path":"/r/acme/a.json"}"#));
        assert!(Triggers { any_call: true, ..Default::default() }.call_might_match("anything"));
        let star = Triggers { edit_globs: vec!["*".into()], ..Default::default() };
        assert!(star.call_might_match("anything"), "a glob with no literal piece matches any path");
    }
}
