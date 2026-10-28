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
    /// The harness can read the person's connectors for the world (the mod's `$.mcp.call` on the
    /// MCP servers the session has connected): calendar, pull requests, tickets arrive.
    #[serde(default)]
    pub world_pull: bool,
    /// A row can be appended to the running conversation, or a prompt submitted when idle (the
    /// mod's `$.session.append`, `$.prompt.submit`): interrupts and autonomy messages.
    #[serde(default)]
    pub world_push: bool,
    /// Context can be placed after the prompt as typed, where it never touches the cached prefix
    /// (the mod's `prompt.submit` `context`): hints and deltas.
    #[serde(default)]
    pub prompt_tail: bool,
    /// A block of the first message that the harness sends once and again after a compaction
    /// (the mod's `prompt.context`): the world snapshot.
    #[serde(default)]
    pub first_message_block: bool,
    /// Per-request cache figures from the harness (the mod's `turn.step` `usage`): the cache
    /// instrument.
    #[serde(default)]
    pub cache_figures: bool,
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
        world_pull: false,
        world_push: false,
        prompt_tail: false,
        first_message_block: false,
        cache_figures: false,
    };
}

/// What a native hook event is, whichever harness sends it and under whichever name: the value of
/// the one table of native event names ([`Harness::events`]). Everything that asks what an event
/// name means (the hook's dispatch and its adapters' translation, the press, a rebuild reading
/// records written before they carried a `kind`) reads it from there, so a new event is one row
/// per harness and one variant here, and the compiler lists every `match` that must decide it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// A session starts (or resumes, or is cleared or compacted).
    SessionStart,
    /// The person submitted a prompt.
    Prompt,
    /// Several tool calls completed together (Claude Code's `PostToolBatch`).
    ToolBatch,
    /// One tool call completed (it may report a failure the harness signals no other way).
    ToolUse,
    /// One tool call failed.
    Failure,
    /// The agent is about to end its turn.
    Stop,
    /// The session ended.
    SessionEnd,
    /// The mod's step boundary before a model request (`turn.step`).
    TurnStep,
    /// A shell call is about to run (Claude Code's `PreToolUse`: the command hook, or the mod's
    /// dry run and verdict).
    PreToolUse,
    /// The mod asks for a press token (the person's own answer, `adage_hook::press`).
    Press,
    /// The mod asks for the standing text (its cached system-prompt section).
    Standing,
    /// The mod asks whether a skill's text is to be annotated (`skill.prompt`).
    SkillPrompt,
}

impl EventKind {
    /// The runtime event this is decided as ([`crate::Event::name`], the `kind` of its spool
    /// record), or `None` for an ask the hook answers itself. Both tool-call kinds are a `batch`:
    /// a translation may still read a failure from a `ToolUse`'s payload.
    pub const fn decided(self) -> Option<&'static str> {
        match self {
            EventKind::SessionStart => Some("session_start"),
            EventKind::Prompt => Some("prompt"),
            EventKind::ToolBatch | EventKind::ToolUse => Some("batch"),
            EventKind::Failure => Some("failure"),
            EventKind::Stop => Some("stop"),
            EventKind::SessionEnd => Some("session_end"),
            EventKind::TurnStep => Some("turn_step"),
            EventKind::PreToolUse | EventKind::Press | EventKind::Standing | EventKind::SkillPrompt => None,
        }
    }

    /// Only a mod sends it: no command hook is ever registered for it.
    pub const fn mod_only(self) -> bool {
        match self {
            EventKind::TurnStep | EventKind::Standing | EventKind::SkillPrompt => true,
            EventKind::SessionStart
            | EventKind::Prompt
            | EventKind::ToolBatch
            | EventKind::ToolUse
            | EventKind::Failure
            | EventKind::Stop
            | EventKind::SessionEnd
            | EventKind::PreToolUse
            | EventKind::Press => false,
        }
    }
}

/// Claude Code: its command hooks' names (code.claude.com/docs/en/hooks), then the mod's.
const CLAUDE_CODE_EVENTS: &[(&str, EventKind)] = &[
    ("SessionStart", EventKind::SessionStart),
    ("UserPromptSubmit", EventKind::Prompt),
    ("PreToolUse", EventKind::PreToolUse),
    ("PostToolUse", EventKind::ToolUse),
    ("PostToolBatch", EventKind::ToolBatch),
    ("PostToolUseFailure", EventKind::Failure),
    ("Stop", EventKind::Stop),
    ("SessionEnd", EventKind::SessionEnd),
    ("TurnStep", EventKind::TurnStep),
    ("Standing", EventKind::Standing),
    ("SkillPrompt", EventKind::SkillPrompt),
    ("Press", EventKind::Press),
];

/// Codex: its command hooks' names (developers.openai.com/codex/hooks). `PostToolUseFailure` is in
/// its vocabulary but reported never to fire; it is accepted in case it ever does.
const CODEX_EVENTS: &[(&str, EventKind)] = &[
    ("SessionStart", EventKind::SessionStart),
    ("UserPromptSubmit", EventKind::Prompt),
    ("PostToolUse", EventKind::ToolUse),
    ("PostToolUseFailure", EventKind::Failure),
    ("Stop", EventKind::Stop),
    ("SessionEnd", EventKind::SessionEnd),
];

/// OpenCode: the names Adage's own plugin bridge sends (`plugin/opencode/adage.js`).
const OPENCODE_EVENTS: &[(&str, EventKind)] = &[
    ("session.created", EventKind::SessionStart),
    ("chat.message", EventKind::Prompt),
    ("tool.execute.after", EventKind::ToolUse),
    ("tool.error", EventKind::Failure),
    ("session.idle", EventKind::Stop),
    ("session.deleted", EventKind::SessionEnd),
];

impl Harness {
    pub const ALL: [Harness; 3] = [Harness::ClaudeCode, Harness::Codex, Harness::OpenCode];

    /// The native event names this harness's hooks (and its mod, if it has one) send, and what
    /// each is: the one table of event names. A name a harness does not send is not an event of
    /// that harness, and the hook does nothing with it.
    pub const fn events(self) -> &'static [(&'static str, EventKind)] {
        match self {
            Harness::ClaudeCode => CLAUDE_CODE_EVENTS,
            Harness::Codex => CODEX_EVENTS,
            Harness::OpenCode => OPENCODE_EVENTS,
        }
    }

    /// What this harness means by a native event name ([`Harness::events`]).
    pub fn event_kind(self, native: &str) -> Option<EventKind> {
        self.events().iter().find(|(name, _)| *name == native).map(|(_, kind)| *kind)
    }

    /// The native names of the events only this harness's mod sends ([`EventKind::mod_only`]), in
    /// the table's order. `N` is their count: any other fails to compile.
    pub const fn mod_only_events<const N: usize>(self) -> [&'static str; N] {
        let table = self.events();
        let mut out = [""; N];
        let (mut i, mut n) = (0, 0);
        while i < table.len() {
            if table[i].1.mod_only() {
                assert!(n < N, "more mod-only events than N");
                out[n] = table[i].0;
                n += 1;
            }
            i += 1;
        }
        assert!(n == N, "fewer mod-only events than N");
        out
    }

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

    /// What the harness can do when the Claude Code mod is the one asking (its architecture §1b.3):
    /// the command hooks' set plus the twelve the mod adds, for Claude Code; every other harness has
    /// no mod and keeps its own set. A harness adapter reports this only for a payload the mod
    /// sent (`via: "mod"`, or an event only the mod sends), so nothing past the adapter asks which
    /// plugin is installed.
    pub const fn capabilities_with_mod(self) -> Capabilities {
        match self {
            Harness::ClaudeCode => Capabilities {
                predict_step: true,
                answer_call: true,
                substitute_call: true,
                tool_description: true,
                cached_section: true,
                subagent_prompt: true,
                compaction: true,
                world_pull: true,
                world_push: true,
                prompt_tail: true,
                first_message_block: true,
                cache_figures: true,
                ..self.capabilities()
            },
            Harness::Codex | Harness::OpenCode => self.capabilities(),
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

    /// T-C9-01 (per harness), T-W0-04: Claude Code with the mod has every capability, its command
    /// hooks' four included; the other harnesses have no mod, so asking for it changes nothing, and
    /// the world's five are unset for every command-hook caller.
    #[test]
    fn only_claude_code_has_a_mod_and_it_adds_the_rest() {
        let m = Harness::ClaudeCode.capabilities_with_mod();
        assert_eq!(
            m,
            Capabilities {
                pre_tool_rewrite: true,
                session_contract: true,
                actions: true,
                person_notices: true,
                predict_step: true,
                answer_call: true,
                substitute_call: true,
                tool_description: true,
                cached_section: true,
                subagent_prompt: true,
                compaction: true,
                world_pull: true,
                world_push: true,
                prompt_tail: true,
                first_message_block: true,
                cache_figures: true,
            }
        );
        for h in Harness::ALL {
            let c = h.capabilities();
            assert!(
                !(c.world_pull || c.world_push || c.prompt_tail || c.first_message_block || c.cache_figures),
                "{h}: a command hook has no world capability"
            );
        }
        for h in [Harness::Codex, Harness::OpenCode] {
            assert_eq!(h.capabilities_with_mod(), h.capabilities(), "{h}");
        }
    }

    /// The event table: a native name means the same in every harness that sends it (so a record
    /// that names only its event is read the same whichever harness wrote it), no harness lists a
    /// name twice, and only Claude Code has events no command hook sends.
    #[test]
    fn a_native_event_name_means_one_thing() {
        for h in Harness::ALL {
            let names: Vec<&str> = h.events().iter().map(|(n, _)| *n).collect();
            let mut unique = names.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), names.len(), "{h}");
            for (name, kind) in h.events() {
                assert_eq!(h.event_kind(name), Some(*kind));
                for other in Harness::ALL {
                    assert!(other.event_kind(name).is_none_or(|k| k == *kind), "{name}: {h} and {other}");
                }
            }
            assert_eq!(h.event_kind("Mystery"), None);
        }
        assert_eq!(Harness::ClaudeCode.mod_only_events::<3>(), ["TurnStep", "Standing", "SkillPrompt"]);
        for h in [Harness::Codex, Harness::OpenCode] {
            assert_eq!(h.mod_only_events::<0>(), [""; 0], "{h}");
        }
        // Every decided kind has a runtime event of the same name.
        let decided: Vec<_> = Harness::ClaudeCode.events().iter().filter_map(|(_, k)| k.decided()).collect();
        assert_eq!(
            decided,
            ["session_start", "prompt", "batch", "batch", "failure", "stop", "session_end", "turn_step"]
        );
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
