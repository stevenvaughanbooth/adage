//! Interlocking (Design §10.6): the rules that stop two installed routes issuing conflicting
//! directives.
//!
//! The conflict matrix is computed per repository over the routes an index compiles, with the
//! one command-pattern algebra ([`adage_core::pattern`]):
//!
//! 1. **Polarity.** A route that avoids a runner (a dead end, red) and a route whose step runs a
//!    command that runner matches (green or yellow). Red beats green, whatever either's
//!    provenance: running a command this repository's sessions saw fail is never directed beside
//!    the warning against it. If the directing route speaks at a prompt (a prompt, intent or
//!    session-start trigger) and the dead end is announced at the first prompt, the directing
//!    route is **suppressed** at compile time: it would only ever fire beside, or after, the
//!    warning. Otherwise it stays in the index and the runtime filters it (**approach locking**):
//!    no directive is emitted that conflicts with a red directive already emitted in the session,
//!    or emitted in the same decision (`adage_runtime::decide`).
//! 2. **Two greens.** Two green routes at the same decision point (the same trigger kind, with
//!    triggers that overlap) directing different commands. One stays green and the other is
//!    **demoted** to yellow: learned beats imported or person-authored, then more supporting
//!    sessions, then a higher lower bound, then the route id. At most one green per decision
//!    point.
//! 3. **Instincts last.** An instinct (a prebuilt route, `adage_instincts`) has the lowest
//!    precedence: where a route from any other source directs a different command at the same
//!    decision point, the instinct is **suppressed**. (It never wins two greens either: an
//!    instinct is yellow.)
//!
//! Conflicts are reported ([`Conflict`]) for `adage condition` and `adage doctor` to show.

use crate::index::IndexedRoute;
use crate::model::{Aspect, Route, Step, Trigger};
use adage_core::pattern;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictKind {
    /// A dead end avoids what another route directs.
    Polarity,
    /// Two green routes direct different commands at one decision point.
    TwoGreens,
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
    /// winning red directive.
    RuntimeFilter,
    /// The losing route was compiled as yellow.
    Demoted,
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
            Resolution::Demoted => "demoted to yellow",
        };
        let what = match self.kind {
            ConflictKind::Polarity => "avoids what",
            ConflictKind::TwoGreens => "stays the one green beside",
            ConflictKind::Instinct => "overrides the instinct",
        };
        let (repo, winner, loser, command) = (&self.repo, &self.winner, &self.loser, &self.command);
        format!("{repo}: {winner} {what} {loser} directs ({command}); {loser}: {how}")
    }
}

/// Whether `red` says not to run what `other` directs: `red` avoids a runner that `other`'s
/// command runs. Only an avoid step says "not this"; its aspect does not matter.
pub fn avoids(red: &Route, other: &Route) -> bool {
    match (&red.step, &other.step) {
        (Step::Avoid { avoid, .. }, Step::Run { command }) => pattern::directs_avoided(avoid, command),
        _ => false,
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
        _ => false,
    }
}

/// Whether two green routes would direct different commands at one decision point.
pub fn two_greens(a: &Route, b: &Route) -> bool {
    match (&a.step, &b.step) {
        (Step::Run { command: x }, Step::Run { command: y }) => {
            a.aspect == Aspect::Green
                && b.aspect == Aspect::Green
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
    }
}

/// Whether an instinct route and another route direct different things at one decision point.
fn overrides(other: &Route, instinct: &Route) -> bool {
    if other.is_instinct() || !instinct.is_instinct() || !same_point(&other.trigger, &instinct.trigger) {
        return false;
    }
    match (&other.step, &instinct.step) {
        (Step::Run { command: x }, Step::Run { command: y }) => !pattern::same_command(x, y),
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
    let mut demoted: BTreeSet<usize> = BTreeSet::new();
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
    // 1. Polarity: red beats green.
    for (i, red) in routes.iter().enumerate() {
        for (j, other) in routes.iter().enumerate() {
            if i == j || !same_repo(i, j) || !avoids(&red.route, &other.route) {
                continue;
            }
            let at_compile = red.route.trigger == Trigger::SessionStart && at_prompt(&other.route.trigger);
            if at_compile {
                suppressed.insert(j);
            }
            conflicts.push(Conflict {
                repo: red.repo.clone(),
                kind: ConflictKind::Polarity,
                winner: red.route.id.clone(),
                loser: other.route.id.clone(),
                command: command_of(&other.route),
                resolution: if at_compile { Resolution::Suppressed } else { Resolution::RuntimeFilter },
            });
        }
    }
    // 2. Two greens: the preferred route keeps green; any other green at its decision point with a
    //    different command is demoted.
    let learned = |i: usize| !imported(&routes[i]) && !routes[i].route.authored && !routes[i].route.is_instinct();
    let mut greens: Vec<usize> =
        (0..routes.len()).filter(|i| !suppressed.contains(i) && routes[*i].route.aspect == Aspect::Green).collect();
    greens.sort_by(|&a, &b| {
        let (x, y) = (&routes[a], &routes[b]);
        (&x.repo, !learned(a), std::cmp::Reverse(x.route.evidence.support), &x.route.id)
            .cmp(&(&y.repo, !learned(b), std::cmp::Reverse(y.route.evidence.support), &y.route.id))
            .then_with(|| y.route.evidence.lower_bound.total_cmp(&x.route.evidence.lower_bound))
    });
    let mut kept: Vec<usize> = Vec::new();
    for g in greens {
        match kept.iter().find(|&&k| same_repo(k, g) && two_greens(&routes[k].route, &routes[g].route)) {
            Some(&k) => {
                demoted.insert(g);
                conflicts.push(Conflict {
                    repo: routes[g].repo.clone(),
                    kind: ConflictKind::TwoGreens,
                    winner: routes[k].route.id.clone(),
                    loser: routes[g].route.id.clone(),
                    command: command_of(&routes[g].route),
                    resolution: Resolution::Demoted,
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
            if demoted.contains(&i) {
                r.route.aspect = Aspect::Yellow;
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

    fn route(id: &str, trigger: Trigger, step: Step, aspect: Aspect, support: u32) -> IndexedRoute {
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
                aspect,
                state: RouteState::Active,
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
        route("dead-end", Trigger::SessionStart, step, Aspect::Red, 5)
    }

    #[test]
    fn red_beats_green_at_compile_time_when_both_speak_at_the_prompt() {
        let routes = vec![
            dead_end("npm test"),
            route("intent-tests", intent("tests"), run("npm run test"), Aspect::Green, 50),
            route("finish-line", Trigger::AfterEdit { paths: "*.ts".into() }, run("npm test"), Aspect::Green, 9),
            route("intent-build", intent("build"), run("npm run build"), Aspect::Green, 9),
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
    fn one_green_per_decision_point() {
        let edit = |p: &str| Trigger::AfterEdit { paths: p.into() };
        let routes = vec![
            route("gen-a", edit("schema/*.json"), run("make gen"), Aspect::Green, 5),
            route("gen-b", edit("schema/**/*.json"), run("make schema"), Aspect::Green, 9),
            route("gen-c", edit("docs/*.md"), run("make docs"), Aspect::Green, 3),
            route("gen-d", edit("docs/*.md"), run("make docs -j4"), Aspect::Green, 2),
            route(
                "lookup-a",
                Trigger::Prompt { span: SpanKind::Ticket },
                run("gh issue view {item}"),
                Aspect::Green,
                5,
            ),
            route("lookup-b", Trigger::Prompt { span: SpanKind::Path }, run("cat {item}"), Aspect::Green, 5),
        ];
        let (out, conflicts) = resolve(routes, |_| false);
        let aspect = |id: &str| out.iter().find(|r| r.route.id == id).unwrap().route.aspect;
        assert_eq!(aspect("gen-b"), Aspect::Green, "more support wins");
        assert_eq!(aspect("gen-a"), Aspect::Yellow, "an overlapping glob is the same decision point");
        assert_eq!(aspect("gen-c"), Aspect::Green, "a different decision point");
        assert_eq!(aspect("gen-d"), Aspect::Green, "the same command is no conflict");
        assert_eq!(aspect("lookup-a"), Aspect::Green);
        assert_eq!(aspect("lookup-b"), Aspect::Green, "a different span is a different decision point");
        assert_eq!(conflicts.len(), 1, "{conflicts:?}");
        assert_eq!((conflicts[0].kind, conflicts[0].winner.as_str()), (ConflictKind::TwoGreens, "gen-b"));
    }

    #[test]
    fn learned_beats_imported_on_conflict() {
        let t = || Trigger::Prompt { span: SpanKind::Ticket };
        let routes = vec![
            route("imported", t(), run("gh issue view {item}"), Aspect::Green, 90),
            route("learned", t(), run("scripts/wi-lookup {item}"), Aspect::Green, 5),
        ];
        let (out, conflicts) = resolve(routes, |r| r.route.id == "imported");
        assert_eq!(out[0].route.aspect, Aspect::Yellow);
        assert_eq!(out[1].route.aspect, Aspect::Green);
        assert_eq!(conflicts[0].winner, "learned");
    }

    #[test]
    fn repositories_do_not_interlock() {
        let mut other = route("intent-tests", intent("tests"), run("npm test"), Aspect::Green, 5);
        other.repo = "other".into();
        let (out, conflicts) = resolve(vec![dead_end("npm test"), other], |_| false);
        assert_eq!(out.len(), 2);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn instincts_have_the_lowest_precedence() {
        let edit = || Trigger::AfterEdit { paths: "schema/*.json".into() };
        let mut inst = route("finish-line-i", edit(), run("make gen"), Aspect::Yellow, 0);
        inst.route.evidence.provenance = "instinct:core@0.1.0".into();
        let mut same = inst.clone();
        same.route.id = "finish-line-j".into();
        // An import saying something else at the same point wins; one saying the same does not
        // conflict.
        let other = route("imported-gen", edit(), run("make schema"), Aspect::Yellow, 0);
        let (out, conflicts) = resolve(vec![inst.clone(), other.clone()], |r| r.route.id == "imported-gen");
        assert_eq!(out.iter().map(|r| r.route.id.as_str()).collect::<Vec<_>>(), ["imported-gen"]);
        assert_eq!((conflicts[0].kind, conflicts[0].resolution), (ConflictKind::Instinct, Resolution::Suppressed));
        let agree = route("learned-gen", edit(), run("make gen"), Aspect::Green, 5);
        let (out, conflicts) = resolve(vec![inst, agree, same], |_| false);
        assert_eq!(out.len(), 3);
        assert!(conflicts.is_empty(), "{conflicts:?}");
    }
}
