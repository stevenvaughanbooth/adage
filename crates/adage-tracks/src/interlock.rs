//! Interlocking (Design §10.6): the rules that stop two installed routes issuing conflicting
//! directives.
//!
//! The conflict matrix is computed per repository over the routes an index compiles, with the
//! one command-pattern algebra ([`adage_core::pattern`]):
//!
//! 1. **Polarity.** A route that avoids a runner (a dead end, inhibit) and a route whose step runs a
//!    command that runner matches (fire or prime). Inhibit beats fire, whatever either's
//!    provenance: running a command this repository's sessions saw fail is never directed beside
//!    the warning against it. If the directing route speaks at a prompt (a prompt, intent or
//!    session-start trigger) and the dead end is announced at the first prompt, the directing
//!    route is **suppressed** at compile time: it would only ever fire beside, or after, the
//!    warning. Otherwise it stays in the index and the runtime filters it (**approach locking**):
//!    no directive is emitted that conflicts with an inhibit directive already emitted in the session,
//!    or emitted in the same decision (`adage_runtime::decide`).
//! 2. **Two fires.** Two fire routes at the same decision point (the same trigger kind, with
//!    triggers that overlap) directing different commands. One stays fire and the other is
//!    **weakened** to prime: learned beats imported or person-authored, then more supporting
//!    sessions, then a higher lower bound, then the route id. At most one fire per decision
//!    point.
//! 3. **Instincts last.** An instinct (a prebuilt route, `adage_instincts`) has the lowest
//!    precedence: where a route from any other source directs a different command at the same
//!    decision point, the instinct is **suppressed**. (It never wins two fires either: an
//!    instinct is prime.)
//!
//! Conflicts are reported ([`Conflict`]) for `adage condition` and `adage doctor` to show.

use crate::index::IndexedRoute;
use crate::model::{CtxItem, Route, Step, Strength, Trigger};
use adage_core::pattern;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictKind {
    /// A dead end avoids what another route directs.
    Polarity,
    /// Two fire routes direct different commands at one decision point.
    TwoFires,
    /// An instinct and a route from any other source direct different commands at one decision
    /// point: the instinct gives way (instincts have the lowest precedence).
    Instinct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Resolution {
    /// The losing route was left out of the index.
    Suppressed,
    /// The losing route stays in the index; the runtime never emits it after, or beside, the
    /// winning inhibit directive.
    RuntimeFilter,
    /// The losing route was compiled as prime.
    Weakened,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Conflict {
    pub repo: String,
    pub kind: ConflictKind,
    /// Route ids: the one that keeps its say, and the one that gives way.
    pub winner: String,
    pub loser: String,
    /// The losing route's command.
    pub command: String,
    pub resolution: Resolution,
}

impl Conflict {
    pub fn describe(&self) -> String {
        let how = match self.resolution {
            Resolution::Suppressed => "left out",
            Resolution::RuntimeFilter => "never emitted after or beside it",
            Resolution::Weakened => "weakened to prime",
        };
        let what = match self.kind {
            ConflictKind::Polarity => "avoids what",
            ConflictKind::TwoFires => "stays the one fire beside",
            ConflictKind::Instinct => "overrides the instinct",
        };
        let (repo, winner, loser, command) = (&self.repo, &self.winner, &self.loser, &self.command);
        format!("{repo}: {winner} {what} {loser} directs ({command}); {loser}: {how}")
    }
}

/// Whether `inhibit` says not to run what `other` directs: `inhibit` avoids a runner that `other`'s
/// command runs. Only an avoid step says "not this"; its strength does not matter. A sequence
/// conflicts if *any* of its commands runs an avoided runner (Track G §6.3); an obligation is a
/// run step after an edit, so it is covered by the run arm.
pub fn avoids(inhibit: &Route, other: &Route) -> bool {
    match (&inhibit.step, &other.step) {
        (Step::Avoid { avoid, .. }, Step::Run { command }) => pattern::directs_avoided(avoid, command),
        (Step::Avoid { avoid, .. }, Step::Sequence { commands }) => {
            commands.iter().any(|c| pattern::directs_avoided(avoid, c))
        }
        // A skill step directs no command of its own (its use is a harness tool, never a runner a
        // dead end names), so no avoid contradicts it; reviewed for `Step::Skill`.
        _ => false,
    }
}

/// The command a route directs first: a run step's, or a sequence's first (whose follow is
/// judged on it).
fn first_command(step: &Step) -> Option<&str> {
    match step {
        Step::Run { command } => Some(command),
        Step::Sequence { commands } => commands.first().map(String::as_str),
        _ => None,
    }
}

/// Whether a route speaks when a prompt arrives.
fn at_prompt(t: &Trigger) -> bool {
    matches!(t, Trigger::Prompt { .. } | Trigger::Intent { .. } | Trigger::SessionStart)
}

fn globs_overlap(a: &str, b: &str) -> bool {
    let m = |g: &str, p: &str| glob::Pattern::new(g).is_ok_and(|g| g.matches(p));
    a == b || m(a, b) || m(b, a)
}

fn keys_overlap(a: &str, b: &str) -> bool {
    pattern::meets(a, b) || pattern::meets(b, a)
}

/// Whether two triggers are one decision point: the same kind, and an event that fires one can
/// fire the other. Intent routes of different families never fire together (a prompt matches one
/// family at most).
pub fn same_point(a: &Trigger, b: &Trigger) -> bool {
    use Trigger::*;
    match (a, b) {
        (Prompt { span: x }, Prompt { span: y }) => x == y,
        (Intent { family: x, .. }, Intent { family: y, .. }) => x == y,
        (SessionStart, SessionStart) => true,
        (AfterEdit { paths: x }, AfterEdit { paths: y }) => globs_overlap(x, y),
        (AfterCommand { command: x }, AfterCommand { command: y }) => keys_overlap(x, y),
        (AfterFailure { command: x, signature: s }, AfterFailure { command: y, signature: t }) => {
            keys_overlap(x, y) && (s.contains(t.as_str()) || t.contains(s.as_str()))
        }
        // A context fires on an event of its last item (Track G §7.2), so two contexts are one
        // decision point when one event can be the last item of both (identical lists, or one a
        // suffix of the other, are special cases), and a context and a single-event trigger are
        // when that event can fire both.
        (Context { items: x, .. }, Context { items: y, .. }) => match (x.last(), y.last()) {
            (Some(a), Some(b)) => items_overlap(a, b),
            _ => false,
        },
        (Context { items, .. }, other) | (other, Context { items, .. }) => {
            items.last().is_some_and(|last| item_fires_with(last, other))
        }
        _ => false,
    }
}

/// Whether one event can satisfy both context items.
fn items_overlap(a: &CtxItem, b: &CtxItem) -> bool {
    use CtxItem::*;
    let any_keys = |x: &[String], y: &[String]| x.iter().any(|k| y.iter().any(|l| keys_overlap(k, l)));
    match (a, b) {
        (Act { keys: x }, Act { keys: y }) => any_keys(x, y),
        (Fail { keys: x, signature: s }, Fail { keys: y, signature: t }) => {
            any_keys(x, y) && (s.contains(t.as_str()) || t.contains(s.as_str()))
        }
        (Edit { glob: x }, Edit { glob: y }) => globs_overlap(x, y),
        (PromptValue { .. }, PromptValue { .. }) => true,
        (Intent { family: x, .. }, Intent { family: y, .. }) => x == y,
        _ => false,
    }
}

/// Whether the event of a context's last item can also fire a single-event trigger.
fn item_fires_with(last: &CtxItem, t: &Trigger) -> bool {
    match (last, t) {
        (CtxItem::Act { keys }, Trigger::AfterCommand { command }) => keys.iter().any(|k| keys_overlap(k, command)),
        (CtxItem::Fail { keys, signature: s }, Trigger::AfterFailure { command, signature: t }) => {
            keys.iter().any(|k| keys_overlap(k, command)) && (s.contains(t.as_str()) || t.contains(s.as_str()))
        }
        (CtxItem::Edit { glob }, Trigger::AfterEdit { paths }) => globs_overlap(glob, paths),
        (CtxItem::Intent { family: x, .. }, Trigger::Intent { family: y, .. }) => x == y,
        _ => false,
    }
}

/// Whether two fire routes would direct different commands at one decision point.
pub fn two_fires(a: &Route, b: &Route) -> bool {
    match (first_command(&a.step), first_command(&b.step)) {
        (Some(x), Some(y)) => {
            a.strength == Strength::Fire
                && b.strength == Strength::Fire
                && same_point(&a.trigger, &b.trigger)
                && !pattern::same_command(x, y)
        }
        _ => false,
    }
}

fn command_of(r: &Route) -> String {
    match &r.step {
        Step::Run { command } => command.clone(),
        Step::Avoid { avoid, .. } => avoid.clone(),
        Step::ReadBatch { slot, .. } => format!("read {{{slot}}}"),
        Step::Advise { template, .. } => format!("advise {template}"),
        Step::Sequence { commands } => commands.join(" then "),
        Step::Card { .. } => format!("card {}", r.id),
        Step::Skill { skill, .. } => crate::model::skill_key(skill),
    }
}

/// Whether an instinct route and another route direct different things at one decision point.
fn overrides(other: &Route, instinct: &Route) -> bool {
    if other.is_instinct() || !instinct.is_instinct() || !same_point(&other.trigger, &instinct.trigger) {
        return false;
    }
    match (first_command(&other.step), first_command(&instinct.step)) {
        (Some(x), Some(y)) => !pattern::same_command(x, y),
        _ => false,
    }
}

/// Apply the interlocking rules to compiled routes (sorted by repository), returning the routes to
/// serve and the conflicts found. `imported` says which routes came from an imported track.
pub fn resolve(
    routes: Vec<IndexedRoute>,
    imported: impl Fn(&IndexedRoute) -> bool,
) -> (Vec<IndexedRoute>, Vec<Conflict>) {
    let mut conflicts = Vec::new();
    let mut suppressed: BTreeSet<usize> = BTreeSet::new();
    let mut weakened: BTreeSet<usize> = BTreeSet::new();
    let same_repo = |i: usize, j: usize| routes[i].repo == routes[j].repo;
    // 3. Instincts last: an instinct another route contradicts at its decision point is left out.
    for (i, other) in routes.iter().enumerate() {
        for (j, inst) in routes.iter().enumerate() {
            if i != j && same_repo(i, j) && overrides(&other.route, &inst.route) {
                suppressed.insert(j);
                conflicts.push(Conflict {
                    repo: other.repo.clone(),
                    kind: ConflictKind::Instinct,
                    winner: other.route.id.clone(),
                    loser: inst.route.id.clone(),
                    command: command_of(&inst.route),
                    resolution: Resolution::Suppressed,
                });
            }
        }
    }
    // 1. Polarity: inhibit beats fire.
    for (i, inhibit) in routes.iter().enumerate() {
        for (j, other) in routes.iter().enumerate() {
            if i == j || !same_repo(i, j) || !avoids(&inhibit.route, &other.route) {
                continue;
            }
            let at_compile = inhibit.route.trigger == Trigger::SessionStart && at_prompt(&other.route.trigger);
            if at_compile {
                suppressed.insert(j);
            }
            conflicts.push(Conflict {
                repo: inhibit.repo.clone(),
                kind: ConflictKind::Polarity,
                winner: inhibit.route.id.clone(),
                loser: other.route.id.clone(),
                command: command_of(&other.route),
                resolution: if at_compile { Resolution::Suppressed } else { Resolution::RuntimeFilter },
            });
        }
    }
    // 2. Two fires: the preferred route keeps fire; any other fire at its decision point with a
    //    different command is weakened.
    let learned = |i: usize| !imported(&routes[i]) && !routes[i].route.authored && !routes[i].route.is_instinct();
    let mut fires: Vec<usize> =
        (0..routes.len()).filter(|i| !suppressed.contains(i) && routes[*i].route.strength == Strength::Fire).collect();
    fires.sort_by(|&a, &b| {
        let (x, y) = (&routes[a], &routes[b]);
        (&x.repo, !learned(a), std::cmp::Reverse(x.route.evidence.support), &x.route.id)
            .cmp(&(&y.repo, !learned(b), std::cmp::Reverse(y.route.evidence.support), &y.route.id))
            .then_with(|| y.route.evidence.lower_bound.total_cmp(&x.route.evidence.lower_bound))
    });
    let mut kept: Vec<usize> = Vec::new();
    for g in fires {
        match kept.iter().find(|&&k| same_repo(k, g) && two_fires(&routes[k].route, &routes[g].route)) {
            Some(&k) => {
                weakened.insert(g);
                conflicts.push(Conflict {
                    repo: routes[g].repo.clone(),
                    kind: ConflictKind::TwoFires,
                    winner: routes[k].route.id.clone(),
                    loser: routes[g].route.id.clone(),
                    command: command_of(&routes[g].route),
                    resolution: Resolution::Weakened,
                });
            }
            None => kept.push(g),
        }
    }
    conflicts.sort();
    conflicts.dedup();
    let out = routes
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !suppressed.contains(i))
        .map(|(i, mut r)| {
            if weakened.contains(&i) {
                r.route.strength = Strength::Prime;
            }
            r
        })
        .collect();
    (out, conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use adage_core::spans::SpanKind;
    use adage_core::Effect;

    fn route(id: &str, trigger: Trigger, step: Step, strength: Strength, support: u32) -> IndexedRoute {
        IndexedRoute {
            repo: "acme".into(),
            track: "acme/t".into(),
            version: "1.0.0".into(),
            route: Route {
                id: id.into(),
                summary: String::new(),
                trigger,
                step,
                slots: vec![],
                effect: Effect::Verify,
                strength,
                state: RouteState::Firm,
                exit: vec![],
                evidence: Evidence { support, ..Default::default() },
                authored: false,
            },
            exec: None,
        }
    }

    fn run(c: &str) -> Step {
        Step::Run { command: c.into() }
    }

    fn intent(family: &str) -> Trigger {
        Trigger::Intent { family: family.into(), model: "m".into(), centroid: vec![], threshold: 0.8 }
    }

    fn dead_end(avoid: &str) -> IndexedRoute {
        let step = Step::Avoid { avoid: avoid.into(), instead: "make test".into(), signature: "x".into() };
        route("dead-end", Trigger::SessionStart, step, Strength::Inhibit, 5)
    }

    #[test]
    fn inhibit_beats_fire_at_compile_time_when_both_speak_at_the_prompt() {
        let routes = vec![
            dead_end("npm test"),
            route("intent-tests", intent("tests"), run("npm run test"), Strength::Fire, 50),
            route("finish-line", Trigger::AfterEdit { paths: "*.ts".into() }, run("npm test"), Strength::Fire, 9),
            route("intent-build", intent("build"), run("npm run build"), Strength::Fire, 9),
        ];
        let (out, conflicts) = resolve(routes, |_| false);
        let ids: Vec<&str> = out.iter().map(|r| r.route.id.as_str()).collect();
        assert_eq!(ids, ["dead-end", "finish-line", "intent-build"], "the intent route is suppressed");
        assert_eq!(conflicts.len(), 2);
        let c = conflicts.iter().find(|c| c.loser == "intent-tests").unwrap();
        assert_eq!(
            (c.kind, c.resolution, c.winner.as_str()),
            (ConflictKind::Polarity, Resolution::Suppressed, "dead-end")
        );
        let c = conflicts.iter().find(|c| c.loser == "finish-line").unwrap();
        assert_eq!(c.resolution, Resolution::RuntimeFilter, "a later trigger is filtered at runtime");
        assert_eq!(
            c.describe(),
            "acme: dead-end avoids what finish-line directs (npm test); finish-line: never emitted after or beside it"
        );
    }

    #[test]
    fn one_fire_per_decision_point() {
        let edit = |p: &str| Trigger::AfterEdit { paths: p.into() };
        let routes = vec![
            route("gen-a", edit("schema/*.json"), run("make gen"), Strength::Fire, 5),
            route("gen-b", edit("schema/**/*.json"), run("make schema"), Strength::Fire, 9),
            route("gen-c", edit("docs/*.md"), run("make docs"), Strength::Fire, 3),
            route("gen-d", edit("docs/*.md"), run("make docs -j4"), Strength::Fire, 2),
            route(
                "lookup-a",
                Trigger::Prompt { span: SpanKind::Ticket },
                run("gh issue view {item}"),
                Strength::Fire,
                5,
            ),
            route("lookup-b", Trigger::Prompt { span: SpanKind::Path }, run("cat {item}"), Strength::Fire, 5),
        ];
        let (out, conflicts) = resolve(routes, |_| false);
        let strength = |id: &str| out.iter().find(|r| r.route.id == id).unwrap().route.strength;
        assert_eq!(strength("gen-b"), Strength::Fire, "more support wins");
        assert_eq!(strength("gen-a"), Strength::Prime, "an overlapping glob is the same decision point");
        assert_eq!(strength("gen-c"), Strength::Fire, "a different decision point");
        assert_eq!(strength("gen-d"), Strength::Fire, "the same command is no conflict");
        assert_eq!(strength("lookup-a"), Strength::Fire);
        assert_eq!(strength("lookup-b"), Strength::Fire, "a different span is a different decision point");
        assert_eq!(conflicts.len(), 1, "{conflicts:?}");
        assert_eq!((conflicts[0].kind, conflicts[0].winner.as_str()), (ConflictKind::TwoFires, "gen-b"));
    }

    #[test]
    fn learned_beats_imported_on_conflict() {
        let t = || Trigger::Prompt { span: SpanKind::Ticket };
        let routes = vec![
            route("imported", t(), run("gh issue view {item}"), Strength::Fire, 90),
            route("learned", t(), run("scripts/wi-lookup {item}"), Strength::Fire, 5),
        ];
        let (out, conflicts) = resolve(routes, |r| r.route.id == "imported");
        assert_eq!(out[0].route.strength, Strength::Prime);
        assert_eq!(out[1].route.strength, Strength::Fire);
        assert_eq!(conflicts[0].winner, "learned");
    }

    /// Track G G3: a sequence conflicts with an avoid if any of its commands runs an avoided
    /// runner; two contexts are one decision point when one is a suffix of the other (G4: when
    /// their last items can be one event).
    #[test]
    fn sequences_and_contexts_interlock() {
        let seq = |c: &[&str]| Step::Sequence { commands: c.iter().map(|x| x.to_string()).collect() };
        let edit = || Trigger::AfterEdit { paths: "registry/*.csv".into() };
        let bad = route("seq-bad", edit(), seq(&["make gen", "npm test"]), Strength::Fire, 5);
        let good = route("seq-good", edit(), seq(&["make gen", "make check"]), Strength::Fire, 5);
        assert!(avoids(&dead_end("npm test").route, &bad.route));
        assert!(!avoids(&dead_end("npm test").route, &good.route));
        let (out, conflicts) = resolve(vec![dead_end("npm test"), bad, good], |_| false);
        assert_eq!(out.len(), 3, "an after-edit sequence is filtered at runtime, not left out");
        assert_eq!(conflicts.len(), 1);
        assert_eq!((conflicts[0].loser.as_str(), conflicts[0].resolution), ("seq-bad", Resolution::RuntimeFilter));
        assert_eq!(conflicts[0].command, "make gen then npm test");
        use crate::model::CtxItem;
        let a = CtxItem::Act { keys: vec!["exec make gen".into()] };
        let e = CtxItem::Edit { glob: "registry/*.csv".into() };
        let long = Trigger::Context { items: vec![e.clone(), a.clone()], within: 8 };
        let short = Trigger::Context { items: vec![a.clone(), e.clone()], within: 8 };
        assert!(same_point(&long, &Trigger::Context { items: vec![e.clone(), a.clone()], within: 4 }));
        assert!(!same_point(&long, &short));
        let three = Trigger::Context { items: vec![a.clone(), e.clone(), a.clone()], within: 8 };
        assert!(same_point(&three, &long), "a suffix is the same point");
        assert!(!same_point(&long, &edit()));
        // Track G G4: a context fires on an event of its last item, so it shares a decision point
        // with any trigger that event fires.
        let other = CtxItem::Act { keys: vec!["exec make deps".into()] };
        let ends_in_gen = Trigger::Context { items: vec![other.clone(), a.clone()], within: 8 };
        assert!(same_point(&long, &ends_in_gen), "the same last item");
        assert!(same_point(&long, &Trigger::AfterCommand { command: "exec make gen".into() }));
        assert!(same_point(&Trigger::AfterCommand { command: "exec make gen".into() }, &long));
        assert!(!same_point(&long, &Trigger::AfterCommand { command: "exec make check".into() }));
        assert!(same_point(&short, &edit()), "a context ending in an edit of an overlapping glob");
        assert!(!same_point(&short, &Trigger::AfterEdit { paths: "docs/*.md".into() }));
    }

    #[test]
    fn repositories_do_not_interlock() {
        let mut other = route("intent-tests", intent("tests"), run("npm test"), Strength::Fire, 5);
        other.repo = "other".into();
        let (out, conflicts) = resolve(vec![dead_end("npm test"), other], |_| false);
        assert_eq!(out.len(), 2);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn instincts_have_the_lowest_precedence() {
        let edit = || Trigger::AfterEdit { paths: "schema/*.json".into() };
        let mut inst = route("finish-line-i", edit(), run("make gen"), Strength::Prime, 0);
        inst.route.evidence.provenance = "instinct:core@0.1.0".into();
        let mut same = inst.clone();
        same.route.id = "finish-line-j".into();
        // An import saying something else at the same point wins; one saying the same does not
        // conflict.
        let other = route("imported-gen", edit(), run("make schema"), Strength::Prime, 0);
        let (out, conflicts) = resolve(vec![inst.clone(), other.clone()], |r| r.route.id == "imported-gen");
        assert_eq!(out.iter().map(|r| r.route.id.as_str()).collect::<Vec<_>>(), ["imported-gen"]);
        assert_eq!((conflicts[0].kind, conflicts[0].resolution), (ConflictKind::Instinct, Resolution::Suppressed));
        let agree = route("learned-gen", edit(), run("make gen"), Strength::Fire, 5);
        let (out, conflicts) = resolve(vec![inst, agree, same], |_| false);
        assert_eq!(out.len(), 3);
        assert!(conflicts.is_empty(), "{conflicts:?}");
    }
}
