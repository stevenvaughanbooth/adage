//! `adage eval types --end-to-end`: learned value types from learning to the hook (Track G G1
//! gate). For each bundle or dataset, `adage condition`'s plan runs (demo
//! thresholds, no instincts) on the sessions; the tracks it would write are compiled into an
//! index (a nascent is replayed as if a person approved it, which an environment-effect lookup
//! needs before it is ever delivered); then the runtime decides every session's prompts, and a
//! prompt naming each fresh control id and each near miss:
//!
//! - **recall**: sessions that looked an item up (their lookup call's value is the truth) whose
//!   prompt got the lookup directive with exactly that value; and fresh ids (in the prompt
//!   `Please work on <id>.`) bound exactly;
//! - **unsafe**: any directive binding a value that is not a plain token, or that is one of the
//!   adversarial strings (a leading `-`, shell metacharacters, the fixed hostile set). Must be 0;
//! - **controls**: sessions with no lookup call that nevertheless got a lookup directive.

use adage_learn::import::Session;
use adage_learn::LearnConfig;
use adage_runtime::{decide, Ctx, Event, SessionState};
use adage_tracks::{Index, RouteState, SlotSource, Step};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EndToEnd {
    /// The learned lookup command and its contract, if a lookup route was learned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    /// Whether its slot is a learned value type (`prompt_value`).
    pub typed: bool,
    pub sessions: u32,
    pub sessions_bound: u32,
    pub fresh: u32,
    pub fresh_bound: u32,
    pub controls: u32,
    pub controls_fired: u32,
    /// (prompt, directed text) pairs that bound an unsafe value. Must be empty.
    pub unsafe_binds: Vec<(String, String)>,
}

impl EndToEnd {
    pub fn recall(&self) -> f64 {
        let n = self.sessions + self.fresh;
        if n == 0 {
            0.0
        } else {
            (self.sessions_bound + self.fresh_bound) as f64 / n as f64
        }
    }
}

/// The value the lookup directive bound, if the lookup route fired: the word of the directed
/// command (the indented line of the fixed template) where the template has its slot, without
/// the slot's affix.
fn lookup_directive(ctx: &Ctx, template: &str, text: &str) -> Option<String> {
    let mut st = SessionState::default();
    let d = decide(ctx, &mut st, &Event::Prompt { text: text.to_string() });
    let x = d.would_have.iter().find(|x| x.route == "lookup-work-item")?;
    let cmd = x.text.lines().find(|l| l.starts_with("  "))?.trim().to_string();
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let tw: Vec<&str> = template.split_whitespace().collect();
    let i = tw.iter().position(|w| w.contains("{item}"))?;
    let (pre, suf) = tw[i].split_once("{item}")?;
    let w = words.get(i)?;
    Some(w.strip_prefix(pre)?.strip_suffix(suf)?.to_string())
}

fn plain(v: &str) -> bool {
    !v.is_empty() && !v.starts_with('-') && v.chars().all(|c| c.is_ascii_alphanumeric() || "_./:@=+,-".contains(c))
}

/// The truth for a session: the value it passed to the lookup, if it looked one up.
fn truth_of(s: &Session, lookup: &(String, u8)) -> Option<String> {
    s.steps.iter().find_map(|st| {
        let adage_core::CanonicalAction::Exec { argv, .. } = &st.norm.action else { return None };
        let shape = adage_core::pattern::Shape::of_tokens(argv)?;
        (shape.identity.join(" ") == lookup.0).then(|| shape.positionals.get(lookup.1 as usize).cloned())?
    })
}

/// Run the end-to-end check on one set of sessions. `lookup` is the lookup's identity and
/// positional index; `adversarial` the strings no directive may ever bind.
pub fn run(
    repo: &str,
    sessions: &[Session],
    lookup: &(String, u8),
    fresh: &[String],
    adversarial: &[String],
) -> Result<EndToEnd> {
    let mut out = EndToEnd::default();
    let home = tempfile::tempdir()?;
    let cfg = LearnConfig::demo();
    let store = adage_learn::store::Files::new(&home.path().join("tracks"), &home.path().join("index.json"));
    let (_, planned) = adage_learn::plan(sessions.to_vec(), None, &store, &cfg)?;
    let mut tracks: Vec<adage_tracks::Track> = planned.into_iter().map(|p| p.track).collect();
    for t in &mut tracks {
        for r in &mut t.routes {
            if r.state == RouteState::Nascent {
                r.state = RouteState::Firm;
            }
        }
    }
    let route = tracks.iter().flat_map(|t| t.routes.iter()).find(|r| r.id == "lookup-work-item");
    let mut template = String::new();
    if let Some(r) = route {
        let contract = match r.slots.first().map(|s| &s.contract) {
            Some(adage_tracks::Contract::Pattern { regex }) => regex.clone(),
            _ => String::new(),
        };
        if let Step::Run { command } = &r.step {
            out.route = Some(format!("{command} {contract}"));
            template = command.clone();
        }
        out.typed = r.slots.iter().any(|s| matches!(s.source, SlotSource::PromptValue { .. }));
    }
    let index = Index::compile(&tracks);
    let table = adage_core::CommandTable::load();
    let root = home.path().join("root");
    std::fs::create_dir_all(&root)?;
    let ctx = Ctx {
        checkout: &adage_runtime::checkout::Disk,
        index: &index,
        repo: repo.to_string(),
        root: &root,
        table: &table,
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    };
    let check_unsafe = |prompt: &str, got: &Option<String>, out: &mut EndToEnd| {
        if let Some(v) = got {
            if !plain(v) || adversarial.iter().any(|a| a == v) {
                out.unsafe_binds.push((prompt.to_string(), v.clone()));
            }
        }
    };
    for s in sessions {
        let truth = truth_of(s, lookup);
        let prompt = s.prompts.first().map(|p| p.text.clone()).unwrap_or_else(|| s.prompt.clone());
        let got = lookup_directive(&ctx, &template, &prompt);
        check_unsafe(&prompt, &got, &mut out);
        match truth {
            Some(v) => {
                out.sessions += 1;
                out.sessions_bound += u32::from(got.as_deref() == Some(v.as_str()));
            }
            None => {
                out.controls += 1;
                out.controls_fired += u32::from(got.is_some());
            }
        }
    }
    // Fresh ids are written the way the sessions' prompts wrote theirs (`#482` for `482`).
    let mention = sessions
        .iter()
        .find_map(|s| {
            let truth = truth_of(s, lookup)?;
            let text = s.prompts.first().map(|p| p.text.as_str()).unwrap_or(s.prompt.as_str());
            adage_core::tokens::with_mentions(text, adage_core::tokens::PROMPT_TOKENS)
                .into_iter()
                .find(|(t, _)| t.text == truth)
                .map(|(_, m)| m)
        })
        .unwrap_or_default();
    for v in fresh {
        let prompt = format!("Please work on {}{v}{}.", mention.prefix, mention.suffix);
        let got = lookup_directive(&ctx, &template, &prompt);
        check_unsafe(&prompt, &got, &mut out);
        out.fresh += 1;
        out.fresh_bound += u32::from(got.as_deref() == Some(v.as_str()));
    }
    for a in adversarial {
        for prompt in [format!("Please work on {a}."), a.to_string()] {
            let got = lookup_directive(&ctx, &template, &prompt);
            check_unsafe(&prompt, &got, &mut out);
        }
    }
    Ok(out)
}
