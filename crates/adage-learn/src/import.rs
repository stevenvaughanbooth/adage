//! Sessions as the learning engine reads them.
//!
//! Decoding traces is ingestion's job (`adage_ingest`), and the session is the canonical model's
//! (`adage_trace`); both are re-exported here so existing paths keep working. What stays is what
//! only learning needs of a session: which commands Adage's own directives named, so a session is
//! never counted as evidence for what it was told to do (Track G §3.2, steering by influence).

pub use adage_ingest::claude::{decode, discover, discover_project, parse_transcript};
pub use adage_ingest::signals::{asks_for_help, call_steps, Signals};
pub use adage_trace::session::*;

/// The HeadSub identities a directive named, and the directive as the graph engine reads it.
pub trait DirectiveIdentities {
    /// HeadSub identities of every command and runner it named.
    fn identities(&self) -> Vec<String>;
    /// As the graph engine reads it.
    fn influence(&self) -> adage_graph::events::Influence;
}

impl DirectiveIdentities for Directed {
    fn identities(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .commands
            .iter()
            .chain(&self.avoided)
            .map(|c| adage_graph::events::act_identity(&c.split_whitespace().map(str::to_string).collect::<Vec<_>>()))
            .filter(|x| !x.is_empty())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    fn influence(&self) -> adage_graph::events::Influence {
        adage_graph::events::Influence {
            at_step: self.at_step,
            identities: self.identities(),
            paths: self.files.clone(),
            all: self.all,
        }
    }
}

/// What a session was exposed to: the directives it received, as evidence rules read them.
pub trait Exposure {
    /// Whether a directive this session received named any of `commands` (by HeadSub identity)
    /// or, with `reads`, any file: the session is then no evidence for a route directing them
    /// (Track G §3.2).
    fn exposed_to(&self, commands: &[&str], reads: bool) -> bool;
    /// Whether a directive this session received named a command of HeadSub identity `identity`.
    fn exposed_identity(&self, identity: &str) -> bool;
    /// The directives as the graph engine reads them.
    fn graph_influence(&self) -> Vec<adage_graph::events::Influence>;
}

impl Exposure for Session {
    fn exposed_to(&self, commands: &[&str], reads: bool) -> bool {
        let ids: Vec<String> = commands
            .iter()
            .map(|c| adage_graph::events::act_identity(&c.split_whitespace().map(str::to_string).collect::<Vec<_>>()))
            .collect();
        self.influence.iter().any(|d| {
            d.all || (reads && !d.files.is_empty()) || d.identities().iter().any(|i| ids.iter().any(|x| x == i))
        })
    }

    fn exposed_identity(&self, identity: &str) -> bool {
        self.influence.iter().any(|d| d.all || d.identities().iter().any(|i| i == identity))
    }

    fn graph_influence(&self) -> Vec<adage_graph::events::Influence> {
        self.influence.iter().map(DirectiveIdentities::influence).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_session_is_exposed_only_to_what_its_directives_named() {
        let mut s = Session {
            id: "s".into(),
            root: PathBuf::from("/r"),
            repo: "r".into(),
            model: None,
            day: String::new(),
            prompt: "p".into(),
            steps: vec![],
            steered: true,
            directed: ["dead-end-1".to_string()].into(),
            influence: vec![Directed {
                at_step: 0,
                route: "dead-end-1".into(),
                commands: v(&["make test"]),
                avoided: v(&["python3 -m pytest"]),
                ..Default::default()
            }],
            outcome: None,
            harness: CLAUDE_CODE.into(),
            prompts: vec![PromptObs { text: "p".into(), at_step: 0 }],
            ending: Default::default(),
        };
        assert!(s.exposed_to(&["make test"], false) && s.exposed_to(&["python3 -m pytest tests"], false));
        assert!(!s.exposed_to(&["make gen"], false) && !s.exposed_to(&["./scripts/wi-lookup {item}"], true));
        s.influence[0].all = true;
        assert!(s.exposed_to(&["make gen"], false));
    }
}
