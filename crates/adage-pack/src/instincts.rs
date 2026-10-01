//! Instinct packs: templates that a detector compiled into Adage fills (Track C §1.1;
//! `docs/packs.md`, "Instinct packs").
//!
//! An instinct is a reflex with blanks. Its pack holds **templates** ([`RouteV2::detector`]):
//! routes whose trigger and step say what the instinct may bind (after `{runner}` fails with
//! `{signature}`, run `{command}`), with a detector named where a learned route holds its values.
//! The detectors are Rust compiled into Adage, so a pack names them and never supplies them. For a
//! repository, the detectors its packs' templates name run over the repository's declared files
//! (`adage_instincts::bind`, once, together, in the catalogue's order, so one directive per trigger
//! holds across packs and a learned route still wins), and each route a detector binds is kept only
//! when a template of its pack admits it ([`admits`]): same trigger kind, same step kind and
//! advice template, an effect no stronger than the template's, the same exits. A kept route names
//! its pack in its origin (provenance `instinct:<pack>@<version>`).
//!
//! So a pack decides what may bind: a detector no template names stays off, and a route whose
//! shape no template admits is not bound. The template is fixed bytes, digested and signed like
//! any route; what fills it comes from the repository and is validated as a route every time.

use crate::bind::{catalogue_compatible, catalogue_id, BoundRoute, Status};
use crate::model::*;
use crate::validate::rank;
use crate::Pack;
use adage_tracks::{Exit, Origin, Route, Step, Trigger};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// What binding instinct packs to one repository produced, sorted.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstinctBinding {
    /// Every bound route, sorted by id, its origin naming the pack it came from.
    pub routes: Vec<Route>,
    /// `(who, why)` for each candidate, template or pack not bound: who is an instinct id, a pack
    /// name, or `declared` (a file the binder could not read).
    pub skipped: Vec<(String, String)>,
    /// Route id → the instinct (catalogue id) that bound it.
    pub origins: BTreeMap<String, String>,
    /// Route id → `(pack name, template id)`: the template that admitted it.
    pub templates: BTreeMap<String, (String, String)>,
}

impl InstinctBinding {
    /// As the binder reports it ([`adage_instincts::BindResult`]): routes, skipped, origins.
    pub fn into_result(self) -> adage_instincts::BindResult {
        adage_instincts::BindResult { routes: self.routes, skipped: self.skipped, origins: self.origins }
    }

    /// The bound routes of one pack.
    pub fn of_pack<'a>(&'a self, pack: &'a str) -> impl Iterator<Item = &'a Route> + 'a {
        self.routes.iter().filter(move |r| self.templates.get(&r.id).is_some_and(|(p, _)| p == pack))
    }
}

/// Bind instinct `packs` to the repository at `root`. `existing` are the repository's routes from
/// every other source and `rejections` the route ids a person rejected (as for
/// `adage_instincts::bind`); `disabled` are catalogue ids never to bind (`instincts.disabled`).
pub fn bind(
    packs: &[&Pack],
    root: &Path,
    existing: &[Route],
    rejections: &BTreeSet<String>,
    disabled: &BTreeSet<String>,
) -> InstinctBinding {
    let cat = adage_instincts::catalogue();
    let ours = catalogue_id();
    let mut out = InstinctBinding::default();
    let mut sorted: Vec<&Pack> = packs.to_vec();
    sorted.sort_by(|a, b| a.manifest.pack.name.cmp(&b.manifest.pack.name));
    sorted.dedup_by(|a, b| a.manifest.pack.name == b.manifest.pack.name);
    // The templates of every pack, by the detector that fills them.
    let mut by_detector: BTreeMap<&str, Vec<(&Pack, &RouteV2)>> = BTreeMap::new();
    for p in sorted {
        let name = &p.manifest.pack.name;
        let Applicability::Detector { catalogue, .. } = &p.manifest.applicability else {
            out.skipped.push((name.clone(), "not an instinct pack: its routes are not templates".into()));
            continue;
        };
        if !catalogue_compatible(catalogue, &ours) {
            out.skipped
                .push((name.clone(), format!("written for detector catalogue {catalogue}; this build has {ours}")));
            continue;
        }
        for t in &p.routes {
            let Some(d) = t.detector.as_deref() else { continue };
            match cat.get(d) {
                _ if disabled.contains(d) => {
                    out.skipped.push((d.to_string(), format!("{name}: turned off (instincts.disabled)")))
                }
                None => out.skipped.push((name.clone(), format!("{}: detector {d} is not in this build", t.id))),
                Some(i) if !i.enabled => {
                    out.skipped.push((name.clone(), format!("{}: detector {d} is not implemented yet", t.id)))
                }
                Some(_) => by_detector.entry(d).or_default().push((p, t)),
            }
        }
    }
    // Only the detectors a template names run; the catalogue's order decides which claims a
    // trigger first, across packs as within one.
    let opts = adage_instincts::BindOptions {
        disabled: cat
            .instincts
            .iter()
            .map(|i| i.id.clone())
            .filter(|id| !by_detector.contains_key(id.as_str()))
            .collect(),
    };
    let result = adage_instincts::bind_with(root, existing, rejections, &opts);
    out.skipped.extend(result.skipped);
    for mut r in result.routes {
        let instinct = result.origins.get(&r.id).cloned().unwrap_or_default();
        // The tightest template that admits it: the lowest effect ceiling, then by name.
        let fit = by_detector.get(instinct.as_str()).into_iter().flatten().filter(|(_, t)| admits(t, &r)).min_by(
            |(pa, ta), (pb, tb)| {
                (rank(ta.steps[0].effect), &pa.manifest.pack.name, &ta.id).cmp(&(
                    rank(tb.steps[0].effect),
                    &pb.manifest.pack.name,
                    &tb.id,
                ))
            },
        );
        let Some((p, t)) = fit else {
            out.skipped.push((instinct, format!("{}: no template of its pack admits {}", r.id, describe(&r))));
            continue;
        };
        stamp(&mut r, p);
        out.templates.insert(r.id.clone(), (p.manifest.pack.name.clone(), t.id.clone()));
        out.origins.insert(r.id.clone(), instinct);
        out.routes.push(r);
    }
    out.routes.sort_by(|a, b| a.id.cmp(&b.id));
    out.skipped.sort();
    out.skipped.dedup();
    out
}

/// Whether template `t` admits route `r` that its detector bound: the same trigger kind, the same
/// step kind (and advice template, with slots the template has), an effect no stronger than the
/// template's, the template's strength, and its exits.
pub fn admits(t: &RouteV2, r: &Route) -> bool {
    let trigger = matches!(
        (&t.trigger, &r.trigger),
        (TriggerV2::AfterFailure { .. }, Trigger::AfterFailure { .. })
            | (TriggerV2::AfterCommand { .. }, Trigger::AfterCommand { .. })
            | (TriggerV2::AfterEdit { .. }, Trigger::AfterEdit { .. })
    );
    let Some(s) = t.steps.first() else { return false };
    let step = match (s.kind(), &r.step) {
        (Some(StepKind::Run(_)), Step::Run { .. }) => true,
        (Some(StepKind::Advise(a)), Step::Advise { template, slots }) => {
            &a.template == template && slots.keys().all(|k| a.slots.contains_key(k))
        }
        _ => false,
    };
    let exits: Vec<Exit> = t
        .exit
        .iter()
        .filter_map(|e| match e {
            ExitV2::IntentChanges => Some(Exit::IntentChanges),
            ExitV2::ExploratoryChange => Some(Exit::ExploratoryChange),
            ExitV2::ResultEquals { .. } => None,
        })
        .collect();
    trigger && step && rank(r.effect) <= rank(s.effect) && r.strength == t.strength && r.exit == exits
}

/// A bound route names the pack it came from.
fn stamp(r: &mut Route, p: &Pack) {
    let (name, version) = (&p.manifest.pack.name, &p.manifest.pack.version);
    r.evidence.provenance = format!("instinct:{name}@{version}");
    if let Some(Origin::Instinct { pack, version: v, .. }) = &mut r.evidence.origin {
        pack.clone_from(name);
        v.clone_from(version);
    }
}

fn describe(r: &Route) -> String {
    let on = match &r.trigger {
        Trigger::AfterFailure { .. } => "after a failure",
        Trigger::AfterCommand { .. } => "after a command",
        Trigger::AfterEdit { .. } => "after an edit",
        _ => "on another trigger",
    };
    let what = match &r.step {
        Step::Run { .. } => format!("a {} run", r.effect.as_str()),
        Step::Advise { template, .. } => format!("{template} advice"),
        _ => "this step".into(),
    };
    format!("{what} {on}")
}

/// [`crate::bind`] for a pack that applies by detector: every route its templates bind in the
/// checkout at `root`, live at E0 (instincts are delivered, never executed), its route exec digest
/// the template's; or, with `inert`, every template inert with that reason.
pub fn bound_routes(pack: &Pack, root: &Path, inert: Option<String>) -> Vec<BoundRoute> {
    let digest = |id: &str| pack.route_exec_digest(id).unwrap_or_default();
    if let Some(reason) = inert {
        return pack
            .routes
            .iter()
            .map(|t| BoundRoute {
                id: t.id.clone(),
                status: Status::Inert { reason: reason.clone() },
                route_exec_digest: digest(&t.id),
                scripts: vec![],
                route: None,
            })
            .collect();
    }
    let b = bind(&[pack], root, &[], &BTreeSet::new(), &BTreeSet::new());
    b.routes
        .into_iter()
        .map(|r| BoundRoute {
            id: r.id.clone(),
            status: Status::Live { max_level: Level::E0 },
            route_exec_digest: b.templates.get(&r.id).map(|(_, t)| digest(t)).unwrap_or_default(),
            scripts: vec![],
            route: Some(r),
        })
        .collect()
}

/// A pack that could not be read, and why: `(who, why)`.
pub type Unreadable = (String, String);

/// Where an instinct pack Adage can bind comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// The marketplace built into this binary ([`crate::builtin`]).
    Builtin,
    /// Installed with `adage add`, under `tracks/@imported/<owner>/<name>/`.
    Installed(PathBuf),
}

/// Every instinct pack this machine can bind: the built-in marketplace's packs (always from the
/// verified copy in the binary; their installed copies mirror it), then the instinct packs
/// installed with `adage add`, by name. A pack that fails to load is left out, with why, and so is
/// one recorded as from a marketplace that no longer lists it (the next sync removes it).
pub fn available(tracks_root: &Path) -> (Vec<(Pack, Source)>, Vec<Unreadable>) {
    let mut out: Vec<(Pack, Source)> = Vec::new();
    let mut problems = Vec::new();
    match crate::builtin::marketplace() {
        Ok(m) => out.extend(m.packs.iter().map(|(_, p)| (p.clone(), Source::Builtin))),
        Err(e) => problems.push(("adage".to_string(), format!("the built-in marketplace cannot be read: {e}"))),
    }
    let imported = tracks_root.join(adage_tracks::index::IMPORTED_DIR);
    let mut dirs: Vec<PathBuf> = Vec::new();
    for owner in std::fs::read_dir(&imported).into_iter().flatten().flatten() {
        let name = owner.file_name().to_string_lossy().to_string();
        if name.starts_with('@') || !owner.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        for pack in std::fs::read_dir(owner.path()).into_iter().flatten().flatten() {
            if pack.path().join(crate::validate::MANIFEST).is_file() {
                dirs.push(pack.path());
            }
        }
    }
    dirs.sort();
    let records = crate::install::records(tracks_root);
    let from_marketplace = |name: &str| {
        records.get(name).and_then(|r| match &r.via {
            crate::install::Via::Marketplace { name } => Some(name.clone()),
            _ => None,
        })
    };
    for dir in dirs {
        match Pack::load(&dir) {
            Ok(p) if !matches!(p.manifest.applicability, Applicability::Detector { .. }) => {}
            Ok(p) if out.iter().any(|(q, _)| q.manifest.pack.name == p.manifest.pack.name) => {}
            Ok(p) if from_marketplace(&p.manifest.pack.name).is_some() => {
                let m = from_marketplace(&p.manifest.pack.name).unwrap_or_default();
                let why = format!("no longer in the {m} marketplace: the next sync removes it");
                problems.push((p.manifest.pack.name.clone(), why));
            }
            Ok(p) => out.push((p, Source::Installed(dir))),
            Err(e) => problems.push((dir.display().to_string(), format!("{e:#}"))),
        }
    }
    (out, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(on: TriggerV2, step: StepV2, exit: Vec<ExitV2>) -> RouteV2 {
        RouteV2 {
            id: "t".into(),
            summary: String::new(),
            shape: Shape::After,
            detector: Some("U1".into()),
            trigger: on,
            entry: true,
            inputs: BTreeMap::new(),
            steps: vec![step],
            links: vec![],
            strength: Strength::Prime,
            exit,
            exec: None,
            evidence: None,
        }
    }

    fn run(effect: adage_core::Effect) -> StepV2 {
        StepV2 {
            id: "run".into(),
            run: Some(RunSpec { argv: vec![BLANK_COMMAND.into()] }),
            read: None,
            avoid: None,
            advise: None,
            effect,
            script: None,
            outputs: BTreeMap::new(),
        }
    }

    fn route(effect: adage_core::Effect, exit: Vec<Exit>) -> Route {
        Route {
            id: "dead-end-1".into(),
            summary: String::new(),
            trigger: Trigger::AfterFailure {
                command: "exec pytest".into(),
                signature: "No module named pytest".into(),
            },
            step: Step::Run { command: "make test".into() },
            slots: vec![],
            effect,
            strength: Strength::Prime,
            state: adage_tracks::RouteState::Forming,
            exit,
            evidence: Default::default(),
            authored: false,
        }
    }

    #[test]
    fn a_template_admits_only_routes_of_its_shape_within_its_ceiling() {
        use adage_core::Effect::*;
        let failure = TriggerV2::AfterFailure { command: BLANK_RUNNER.into(), signature: BLANK_SIGNATURE.into() };
        let t = template(failure.clone(), run(Verify), vec![ExitV2::IntentChanges]);
        assert!(admits(&t, &route(Verify, vec![Exit::IntentChanges])));
        assert!(admits(&t, &route(Observe, vec![Exit::IntentChanges])), "a weaker effect fits under the ceiling");
        assert!(!admits(&t, &route(Environment, vec![Exit::IntentChanges])), "an install is above a verify ceiling");
        assert!(!admits(&t, &route(Verify, vec![])), "its exits are the template's");
        let edit =
            template(TriggerV2::AfterEdit { paths: BLANK_PATHS.into() }, run(Verify), vec![ExitV2::IntentChanges]);
        assert!(!admits(&edit, &route(Verify, vec![Exit::IntentChanges])), "another trigger kind");
        let mut advise = run(Observe);
        advise.run = None;
        advise.advise = Some(AdviseSpec {
            template: "project-python".into(),
            slots: [("interpreter".to_string(), "{interpreter}".to_string())].into_iter().collect(),
        });
        let t = template(failure, advise, vec![]);
        assert!(!admits(&t, &route(Observe, vec![])), "advice does not admit a run");
        let mut r = route(Observe, vec![]);
        r.step = Step::Advise {
            template: "project-python".into(),
            slots: [("interpreter".to_string(), ".venv/bin/python".to_string())].into_iter().collect(),
        };
        assert!(admits(&t, &r));
        r.step = Step::Advise { template: "lockfile".into(), slots: BTreeMap::new() };
        assert!(!admits(&t, &r), "another advice template");
    }
}
