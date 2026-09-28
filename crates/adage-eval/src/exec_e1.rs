//! An offline (L1-style) estimate of E1 prefetch (Track C §6): on replayed held-out sessions,
//! how often a prefetch would have replaced the lookup call the agent made itself, and how many
//! model turns that saves **beyond E0**.
//!
//! E0 already tells the agent which lookup to run; the agent still spends a call, and usually a
//! turn, running it. With E1 the result is already beside the directive. For every directive a
//! prompt-triggered run route would have given (the shape `adage-exec hook` prefetches), the
//! agent's first later call that meets the directed command is the call E1 would have replaced.
//! The turn it was made in is saved when every call in that turn was the lookup; when the agent
//! batched it with other calls, only the call is saved. Only the first matching call per
//! directive counts. It is an estimate: it assumes the agent would have used the prefetched
//! result exactly as it used its own, and says nothing about output the typed projection would
//! have withheld.

use crate::replay::Replayed;
use adage_core::CommandTable;
use adage_tracks::{Index, Step, Trigger};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize)]
pub struct ExecE1 {
    /// Scored sessions.
    pub sessions: usize,
    /// Sessions with at least one directive E1 could have prefetched.
    pub triggered_sessions: usize,
    /// Sessions where the agent then ran the lookup itself (a call E1 would have replaced).
    pub replaced_sessions: usize,
    /// Calls E1 would have replaced.
    pub calls_replaced: usize,
    /// Model turns saved beyond E0 (turns that held nothing but the lookup).
    pub turns_saved: u32,
    /// `turns_saved / triggered_sessions` (G3 compares this with 0.5).
    pub turns_saved_per_triggered_session: f64,
    /// Per route: (directives E1 could have prefetched, calls replaced).
    pub by_route: BTreeMap<String, (usize, usize)>,
}

/// The per-session part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionE1 {
    pub triggered: bool,
    pub calls_replaced: usize,
    pub turns_saved: u32,
    pub by_route: BTreeMap<String, (usize, usize)>,
}

/// Measure one replayed session.
pub fn measure(rep: &Replayed, index: &Index, repo_key: &str, table: &CommandTable) -> SessionE1 {
    let mut out = SessionE1::default();
    let mut used: Vec<(u32, usize)> = Vec::new();
    for f in &rep.fired {
        let Some(ir) = index.for_repo(repo_key).find(|r| r.route.id == f.directive.route) else { continue };
        let prefetchable = matches!(ir.route.step, Step::Run { .. })
            && matches!(ir.route.trigger, Trigger::Prompt { .. } | Trigger::Intent { .. });
        if !prefetchable {
            continue;
        }
        let Some(line) = f.directive.text.lines().find(|l| l.starts_with("  ")) else { continue };
        let key = adage_runtime::command_key(line.trim(), table);
        if key.is_empty() {
            continue;
        }
        out.triggered = true;
        let e = out.by_route.entry(ir.route.id.clone()).or_default();
        e.0 += 1;
        let hit = rep
            .actions
            .iter()
            .find(|a| a.turn > f.turn && !used.contains(&(a.turn, a.call)) && adage_core::pattern::meets(&key, &a.key));
        let Some(hit) = hit else { continue };
        used.push((hit.turn, hit.call));
        e.1 += 1;
        out.calls_replaced += 1;
        let same_turn: Vec<_> = rep.actions.iter().filter(|a| a.turn == hit.turn).collect();
        if same_turn.iter().all(|a| adage_core::pattern::meets(&key, &a.key)) {
            out.turns_saved += 1;
        }
    }
    out
}

impl ExecE1 {
    pub fn add(&mut self, s: &SessionE1) {
        self.sessions += 1;
        self.triggered_sessions += s.triggered as usize;
        self.replaced_sessions += (s.calls_replaced > 0) as usize;
        self.calls_replaced += s.calls_replaced;
        self.turns_saved += s.turns_saved;
        for (k, (a, b)) in &s.by_route {
            let e = self.by_route.entry(k.clone()).or_default();
            e.0 += a;
            e.1 += b;
        }
        self.turns_saved_per_triggered_session =
            if self.triggered_sessions == 0 { 0.0 } else { self.turns_saved as f64 / self.triggered_sessions as f64 };
    }

    pub fn text(&self) -> String {
        format!(
            "exec E1 (offline estimate): {} of {} sessions triggered a prefetchable lookup; the agent ran it itself in {} ({} calls E1 would have replaced); turns saved beyond E0: {} ({:.2} per triggered session)\n",
            self.triggered_sessions,
            self.sessions,
            self.replaced_sessions,
            self.calls_replaced,
            self.turns_saved,
            self.turns_saved_per_triggered_session
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{Action, Fired};
    use adage_core::spans::SpanKind;
    use adage_core::Effect;
    use adage_tracks::*;

    fn action(turn: u32, call: usize, command: &str, table: &CommandTable) -> Action {
        Action {
            turn,
            call,
            key: adage_runtime::command_key(command, table),
            argv: None,
            effect: Effect::Observe,
            read: false,
            author: false,
            rel_path: None,
            failed: false,
            output: String::new(),
        }
    }

    #[test]
    fn a_lookup_the_agent_ran_alone_in_its_turn_is_a_turn_saved() {
        let table = CommandTable::load();
        let t = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "acme/w".into(),
                    version: "0.1.0".into(),
                    description: String::new(),
                    provenance: "desire-path".into(),
                },
                scope: Scope { repo: "acme".into() },
                gauge: Gauge::default(),
            },
            routes: vec![Route {
                id: "lookup-work-item".into(),
                summary: String::new(),
                trigger: Trigger::Prompt { span: SpanKind::Ticket },
                step: Step::Run { command: "python3 scripts/wi-lookup {id} --json".into() },
                slots: vec![Slot {
                    name: "id".into(),
                    source: SlotSource::Prompt { span: SpanKind::Ticket },
                    contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
                }],
                effect: Effect::Observe,
                aspect: Aspect::Green,
                state: RouteState::Active,
                exit: vec![],
                evidence: Evidence::default(),
                authored: false,
            }],
        };
        let index = Index::compile(&[t]);
        let fired = Fired {
            directive: adage_runtime::Directive {
                route: "lookup-work-item".into(),
                track: "acme/w".into(),
                version: "0.1.0".into(),
                aspect: Aspect::Green,
                text: "x\n  python3 scripts/wi-lookup W-2042 --json\ny".into(),
                kind: "work-items".into(),
                origin: None,
            },
            event: "UserPromptSubmit",
            turn: 0,
        };
        let alone = Replayed {
            fired: vec![fired.clone()],
            actions: vec![
                action(1, 0, "python3 scripts/wi-lookup W-2042 --json", &table),
                action(2, 0, "cat acme/billing.py", &table),
            ],
            ..Default::default()
        };
        let m = measure(&alone, &index, "acme", &table);
        assert_eq!((m.triggered, m.calls_replaced, m.turns_saved), (true, 1, 1));
        let batched = Replayed {
            fired: vec![fired.clone()],
            actions: vec![
                action(1, 0, "python3 scripts/wi-lookup W-2042 --json", &table),
                action(1, 1, "cat acme/billing.py", &table),
            ],
            ..Default::default()
        };
        let m = measure(&batched, &index, "acme", &table);
        assert_eq!((m.calls_replaced, m.turns_saved), (1, 0), "batched: a call saved, not a turn");
        let never = Replayed { fired: vec![fired], actions: vec![], ..Default::default() };
        let m = measure(&never, &index, "acme", &table);
        assert_eq!((m.triggered, m.calls_replaced), (true, 0));
        let mut all = ExecE1::default();
        all.add(&measure(&alone, &index, "acme", &table));
        all.add(&m);
        assert_eq!((all.triggered_sessions, all.turns_saved, all.turns_saved_per_triggered_session), (2, 1, 0.5));
    }
}
