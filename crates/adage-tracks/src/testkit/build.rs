//! Builders for the model types a fixture writes by hand: [`Route`], [`Track`] and a hand-written
//! [`Index`] (one that is not compiled from tracks, as the hook's unit tests use).

use crate::index::{ExecBinding, Index, IndexedRoute};
use crate::{
    io, wilson_lower, Evidence, Exit, Gauge, Manifest, Origin, Route, RouteState, Scope, Slot, Step, Strength, Track,
    TrackInfo, Trigger,
};
use adage_core::Effect;
use std::path::Path;

/// A route with this id, trigger and step: firm, fired, a verification, no slots, no exit and
/// default evidence until the builder says otherwise.
pub fn route(id: &str, trigger: Trigger, step: Step) -> RouteBuilder {
    RouteBuilder(Route {
        id: id.into(),
        summary: String::new(),
        trigger,
        step,
        slots: vec![],
        effect: Effect::Verify,
        strength: Strength::Fire,
        state: RouteState::Firm,
        exit: vec![],
        evidence: Evidence::default(),
        authored: false,
    })
}

/// A [`Route`] under construction ([`route`], or one of [`super::kinds`]).
#[derive(Debug, Clone)]
#[must_use]
pub struct RouteBuilder(Route);

impl RouteBuilder {
    pub fn id(mut self, id: &str) -> Self {
        self.0.id = id.into();
        self
    }
    pub fn summary(mut self, summary: &str) -> Self {
        self.0.summary = summary.into();
        self
    }
    pub fn trigger(mut self, trigger: Trigger) -> Self {
        self.0.trigger = trigger;
        self
    }
    pub fn step(mut self, step: Step) -> Self {
        self.0.step = step;
        self
    }
    /// Add a slot.
    pub fn slot(mut self, slot: Slot) -> Self {
        self.0.slots.push(slot);
        self
    }
    /// Replace the slots.
    pub fn slots(mut self, slots: Vec<Slot>) -> Self {
        self.0.slots = slots;
        self
    }
    pub fn effect(mut self, effect: Effect) -> Self {
        self.0.effect = effect;
        self
    }
    pub fn strength(mut self, strength: Strength) -> Self {
        self.0.strength = strength;
        self
    }
    pub fn state(mut self, state: RouteState) -> Self {
        self.0.state = state;
        self
    }
    pub fn exit(mut self, exit: Vec<Exit>) -> Self {
        self.0.exit = exit;
        self
    }
    /// The exit most learned routes carry: the directive lapses when the prompt's intent changes.
    pub fn until_intent_changes(self) -> Self {
        self.exit(vec![Exit::IntentChanges])
    }
    /// Replace the evidence.
    pub fn evidence(mut self, evidence: Evidence) -> Self {
        self.0.evidence = evidence;
        self
    }
    /// Seen in `support` of `opportunities` sessions (counts only: rate and bound stay as they are).
    pub fn support(mut self, support: u32, opportunities: u32) -> Self {
        self.0.evidence.support = support;
        self.0.evidence.opportunities = opportunities;
        self
    }
    /// [`Self::support`], with the rate and its Wilson lower bound the learner would write
    /// (a rate of 0 with no opportunities).
    pub fn measured(mut self, support: u32, opportunities: u32) -> Self {
        let e = &mut self.0.evidence;
        e.support = support;
        e.opportunities = opportunities;
        e.rate = if opportunities > 0 { support as f64 / opportunities as f64 } else { 0.0 };
        e.lower_bound = wilson_lower(support, opportunities);
        self
    }
    pub fn days(mut self, days: u32) -> Self {
        self.0.evidence.days = days;
        self
    }
    pub fn provenance(mut self, provenance: &str) -> Self {
        self.0.evidence.provenance = provenance.into();
        self
    }
    pub fn origin(mut self, origin: Origin) -> Self {
        self.0.evidence.origin = Some(origin);
        self
    }
    pub fn authored(mut self, authored: bool) -> Self {
        self.0.authored = authored;
        self
    }
    /// Any other change, on the route itself.
    pub fn with(mut self, f: impl FnOnce(&mut Route)) -> Self {
        f(&mut self.0);
        self
    }
    pub fn build(self) -> Route {
        self.0
    }
}

impl From<RouteBuilder> for Route {
    fn from(b: RouteBuilder) -> Route {
        b.0
    }
}

/// A track named `name` for repository `repo`: version `0.1.0`, learned as a desire path, no
/// description, no gauge, no routes yet.
pub fn track(name: &str, repo: &str) -> TrackBuilder {
    TrackBuilder(Track {
        manifest: Manifest {
            track: TrackInfo {
                name: name.into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: repo.into() },
            gauge: Gauge::default(),
        },
        routes: vec![],
    })
}

/// A [`Track`] under construction ([`track`]).
#[derive(Debug, Clone)]
#[must_use]
pub struct TrackBuilder(Track);

impl TrackBuilder {
    pub fn version(mut self, version: &str) -> Self {
        self.0.manifest.track.version = version.into();
        self
    }
    pub fn description(mut self, description: &str) -> Self {
        self.0.manifest.track.description = description.into();
        self
    }
    pub fn provenance(mut self, provenance: &str) -> Self {
        self.0.manifest.track.provenance = provenance.into();
        self
    }
    /// Add a route.
    pub fn route(mut self, route: impl Into<Route>) -> Self {
        self.0.routes.push(route.into());
        self
    }
    /// Add routes, in order.
    pub fn routes<R: Into<Route>>(mut self, routes: impl IntoIterator<Item = R>) -> Self {
        self.0.routes.extend(routes.into_iter().map(Into::into));
        self
    }
    pub fn build(self) -> Track {
        self.0
    }
    /// The track, after checking it passes [`io::validate`] (every source's safety boundary), so
    /// a fixture cannot test what no installed track could be.
    pub fn validated(self) -> Track {
        if let Err(e) = io::validate(&self.0) {
            panic!("fixture track {} is not valid: {e:#}", self.0.manifest.track.name);
        }
        self.0
    }
}

impl From<TrackBuilder> for Track {
    fn from(b: TrackBuilder) -> Track {
        b.0
    }
}

/// An index written by hand, route by route, rather than compiled from tracks: it keeps the
/// routes in the order given and the generation it is told. Its triggers and context nets are
/// left out, so the index derives them on the fly, as for an index written before they existed.
#[derive(Debug, Clone)]
#[must_use]
pub struct IndexBuilder(Index);

impl IndexBuilder {
    pub fn new(generation: &str) -> Self {
        IndexBuilder(Index { generation: generation.into(), ..Default::default() })
    }
    /// Add `route` for repository `repo`, in track `track` at version `0.1.0`.
    pub fn route(self, repo: &str, track: &str, route: impl Into<Route>) -> Self {
        self.versioned(repo, track, "0.1.0", route)
    }
    /// Add `route` for `repo` in `track` at `version`.
    pub fn versioned(self, repo: &str, track: &str, version: &str, route: impl Into<Route>) -> Self {
        self.indexed(IndexedRoute {
            repo: repo.into(),
            track: track.into(),
            version: version.into(),
            route: route.into(),
            exec: None,
        })
    }
    /// Give the last route added a pack's execution binding.
    pub fn exec(mut self, binding: ExecBinding) -> Self {
        let last = self.0.routes.last_mut().expect("a route to bind");
        last.exec = Some(binding);
        self
    }
    /// Add an indexed route as it is.
    pub fn indexed(mut self, route: IndexedRoute) -> Self {
        self.0.routes.push(route);
        self
    }
    pub fn build(self) -> Index {
        self.0
    }
    /// Write the index to `path` atomically, as the learner does, and return it.
    pub fn write(self, path: &Path) -> Index {
        self.0.write_atomic(path).expect("writing the fixture index");
        self.0
    }
}

impl From<IndexBuilder> for Index {
    fn from(b: IndexBuilder) -> Index {
        b.0
    }
}
