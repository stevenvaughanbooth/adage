//! The agent harnesses Adage works with, and what each can do.
//!
//! Every place that behaves differently per harness matches on [`Harness`] (exhaustively) or reads
//! its [`Capabilities`], never a name string: adding a harness is adding a variant, and the
//! compiler lists every place that must handle it. What each harness documents, and what is
//! inferred, is recorded in docs/harnesses.md.

/// An agent harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Harness {
    ClaudeCode,
    Codex,
    OpenCode,
}

/// What a harness's hooks let Adage do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// A `PreToolUse` hook that may rewrite a tool call's input (Claude Code).
    pub pre_tool_rewrite: bool,
    /// The session-start contract (a context block for the model at session start). OpenCode's
    /// bridge has no session-start context channel.
    pub session_contract: bool,
    /// Adage may run approved actions inside a hook (E2 actions, the finish-line gate): the hook
    /// timeout leaves room for them. OpenCode's bridge waits one second.
    pub actions: bool,
    /// A channel for one line to the person that the model never sees (Claude Code's
    /// `systemMessage`).
    pub person_notices: bool,
}

impl Harness {
    pub const ALL: [Harness; 3] = [Harness::ClaudeCode, Harness::Codex, Harness::OpenCode];

    /// The stable identifier: what a track's `gauge.harness`, a [`crate::Session`]'s `harness`,
    /// `adage install <harness>` and the registration listings use.
    pub const fn id(self) -> &'static str {
        match self {
            Harness::ClaudeCode => "claude-code",
            Harness::Codex => "codex",
            Harness::OpenCode => "opencode",
        }
    }

    /// The name in a hook registration (`adage hook <name> <event>`) and in spool records.
    pub const fn hook_name(self) -> &'static str {
        match self {
            Harness::ClaudeCode => "claude",
            Harness::Codex => "codex",
            Harness::OpenCode => "opencode",
        }
    }

    /// The product's name, for people.
    pub const fn display(self) -> &'static str {
        match self {
            Harness::ClaudeCode => "Claude Code",
            Harness::Codex => "Codex",
            Harness::OpenCode => "OpenCode",
        }
    }

    /// A two-letter tag for narrow columns.
    pub const fn short(self) -> &'static str {
        match self {
            Harness::ClaudeCode => "cc",
            Harness::Codex => "cx",
            Harness::OpenCode => "oc",
        }
    }

    pub const fn capabilities(self) -> Capabilities {
        match self {
            Harness::ClaudeCode => {
                Capabilities { pre_tool_rewrite: true, session_contract: true, actions: true, person_notices: true }
            }
            Harness::Codex => {
                Capabilities { pre_tool_rewrite: false, session_contract: true, actions: true, person_notices: false }
            }
            Harness::OpenCode => {
                Capabilities { pre_tool_rewrite: false, session_contract: false, actions: false, person_notices: false }
            }
        }
    }

    /// A harness by its identifier or its hook name (`claude-code` or `claude`, `codex`,
    /// `opencode`).
    pub fn parse(s: &str) -> Option<Harness> {
        Harness::ALL.into_iter().find(|h| h.id() == s || h.hook_name() == s)
    }
}

impl std::fmt::Display for Harness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_parses_back_to_its_harness() {
        for h in Harness::ALL {
            assert_eq!(Harness::parse(h.id()), Some(h));
            assert_eq!(Harness::parse(h.hook_name()), Some(h));
        }
        assert_eq!(Harness::parse("cursor"), None);
        assert_eq!(Harness::parse(""), None);
    }

    #[test]
    fn names_are_distinct() {
        let mut ids: Vec<&str> = Harness::ALL.iter().map(|h| h.id()).collect();
        let mut hooks: Vec<&str> = Harness::ALL.iter().map(|h| h.hook_name()).collect();
        let mut shorts: Vec<&str> = Harness::ALL.iter().map(|h| h.short()).collect();
        for v in [&mut ids, &mut hooks, &mut shorts] {
            v.sort();
            v.dedup();
            assert_eq!(v.len(), Harness::ALL.len());
        }
    }
}
