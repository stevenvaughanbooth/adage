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
///
/// The first four are what a harness's command hooks (or bridge) offer. The rest are what the
/// Claude Code mod adds (its architecture §1b.3): a harness reports them only when its adapter
/// knows the mod is asking, and every `Harness` reports them unset ([`Harness::capabilities`]).
/// A field added here is `#[serde(default)]`, so a recorded set from before reads as unset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
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
    /// A step boundary before each model request, where Adage may make the call the agent would
    /// have made (the mod's `turn.step`, the `TurnStep` event).
    #[serde(default)]
    pub predict_step: bool,
    /// A tool call may be answered without running it (the mod's `tool.call` returning a
    /// `result`: a receipt as the result).
    #[serde(default)]
    pub answer_call: bool,
    /// A tool call may run another command in its place before it runs (a learned dead end
    /// substituted, on the engine's own permission verdict).
    #[serde(default)]
    pub substitute_call: bool,
    /// A tool's description, sent once per session, may carry a convention (`tool.describe`).
    #[serde(default)]
    pub tool_description: bool,
    /// A session-scoped section of the system prompt that the prompt cache keeps
    /// (`prompt.compose`): the standing text, once.
    #[serde(default)]
    pub cached_section: bool,
    /// A subagent's prompt may be told the conventions before it starts (`agent.spawn`).
    #[serde(default)]
    pub subagent_prompt: bool,
    /// Compaction may be told what to keep (`session.compact`).
    #[serde(default)]
    pub compaction: bool,
}

impl Capabilities {
    /// Nothing at all: the base every harness's set is spelled from.
    pub const NONE: Capabilities = Capabilities {
        pre_tool_rewrite: false,
        session_contract: false,
        actions: false,
        person_notices: false,
        predict_step: false,
        answer_call: false,
        substitute_call: false,
        tool_description: false,
        cached_section: false,
        subagent_prompt: false,
        compaction: false,
    };
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
            Harness::ClaudeCode => Capabilities {
                pre_tool_rewrite: true,
                session_contract: true,
                actions: true,
                person_notices: true,
                ..Capabilities::NONE
            },
            Harness::Codex => Capabilities { session_contract: true, actions: true, ..Capabilities::NONE },
            Harness::OpenCode => Capabilities::NONE,
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

    /// T-C9-01 (the type): the mod's capabilities are fields of their own, unset for every harness
    /// (its command hooks have today's four), and a set recorded before they existed reads as unset.
    #[test]
    fn the_mods_capabilities_are_unset_for_every_harness_and_round_trip() {
        let cc = Harness::ClaudeCode.capabilities();
        assert!(cc.pre_tool_rewrite && cc.session_contract && cc.actions && cc.person_notices);
        for h in Harness::ALL {
            let c = h.capabilities();
            let extra = [
                c.predict_step,
                c.answer_call,
                c.substitute_call,
                c.tool_description,
                c.cached_section,
                c.subagent_prompt,
                c.compaction,
            ];
            assert!(extra.iter().all(|x| !x), "{h}: {c:?}");
        }
        let all = Capabilities {
            predict_step: true,
            answer_call: true,
            substitute_call: true,
            tool_description: true,
            cached_section: true,
            subagent_prompt: true,
            compaction: true,
            ..cc
        };
        let back: Capabilities = serde_json::from_str(&serde_json::to_string(&all).unwrap()).unwrap();
        assert_eq!(back, all);
        let before = r#"{"pre_tool_rewrite":true,"session_contract":true,"actions":true,"person_notices":true}"#;
        assert_eq!(serde_json::from_str::<Capabilities>(before).unwrap(), cc);
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
