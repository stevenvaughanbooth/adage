//! Binding instincts against fixture repositories: what binds, what never may, and that the
//! result is deterministic and valid as a track.

use adage_core::Effect;
use adage_instincts::{bind, bind_with, BindOptions, BindResult};
use adage_tracks::{
    Aspect, Evidence, Exit, Gauge, Index, Manifest, Route, RouteState, Scope, Step, Track, TrackInfo, Trigger,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn bound(name: &str) -> BindResult {
    bind(&fixture(name), &[], &BTreeSet::new())
}

const FIXTURES: [&str; 12] = [
    "acme",
    "harbor",
    "pnpm",
    "rust",
    "go",
    "pytool",
    "decoy",
    "hostile-npm",
    "hostile-hooks",
    "hostile-make",
    "hostile-ci",
    "hostile-unicode",
];

/// `(trigger command key or glob, directed command)` for each route.
fn shape(r: &BindResult) -> Vec<(String, String, String)> {
    r.runs()
        .into_iter()
        .map(|x| {
            let t = match &x.trigger {
                Trigger::AfterFailure { command, signature } => format!("fail {command} [{signature}]"),
                Trigger::AfterEdit { paths } => format!("edit {paths}"),
                other => format!("{other:?}"),
            };
            let Step::Run { command } = &x.step else { panic!("{:?}", x.step) };
            (x.id.clone(), t, command.clone())
        })
        .collect()
}

fn route<'a>(r: &'a BindResult, id: &str) -> &'a Route {
    r.routes.iter().find(|x| x.id == id).unwrap_or_else(|| panic!("no route {id} in {:#?}", shape(r)))
}

fn has(r: &BindResult, trigger: &str, command: &str) -> bool {
    shape(r).iter().any(|(_, t, c)| t == trigger && c == command)
}

#[test]
fn acme_recovers_its_dead_end_and_finish_line_on_day_zero() {
    let r = bound("acme");
    // U1: `python3 -m unittest` fails without ACME_ENV; `make test` sets it. The id is the one the
    // dead-end miner derived for this working command in the demo (adage-eval gates), so the
    // learned route replaces the instinct as soon as it exists.
    let d = route(&r, "dead-end-d41b1e");
    assert_eq!(
        d.trigger,
        Trigger::AfterFailure { command: "exec python3 -m unittest".into(), signature: "ACME_ENV".into() }
    );
    assert_eq!(d.step, Step::Run { command: "make test".into() });
    assert_eq!(r.origins["dead-end-d41b1e"], "U1");
    // U3: the other runners an agent tries first (round 1 of the demo: pytest, 1.44 times a session).
    assert!(has(&r, "fail exec pytest []", "make test"));
    assert!(has(&r, "fail exec python3 -m pytest []", "make test"));
    // U5: the generator reads schema/*.json (named in tools/gen.py); same id as the learned route.
    let f = route(&r, "finish-line-e7e809");
    assert_eq!(f.trigger, Trigger::AfterEdit { paths: "schema/*.json".into() });
    assert_eq!(f.step, Step::Run { command: "make gen".into() });
    assert_eq!(f.effect, Effect::Regenerate);
    assert_eq!(f.exit, vec![Exit::ExploratoryChange]);
    // U4: the generator's output is advised, with the command that regenerates it.
    let a = r.advisories().into_iter().find(|a| a.instinct == "U4").expect("U4 advisory");
    assert_eq!(a.slots["path"], "acme/generated/models.py");
    assert_eq!(a.slots["command"], "make gen");
    // Nothing else: `make lint` runs `python3 -m py_compile`, which classifies as environment.
    assert!(!shape(&r).iter().any(|(_, _, c)| c.contains("lint") || c.contains("ci")), "{:#?}", shape(&r));
}

#[test]
fn harbor_recovers_its_dead_end_and_finish_line_on_day_zero() {
    let r = bound("harbor");
    // `npm test` runs `node --test` bare and fails on the alias; `npm run check` adds --import.
    assert!(has(&r, "fail exec npm test [ERR_MODULE_NOT_FOUND]", "npm run check"), "{:#?}", shape(&r));
    assert!(has(&r, "fail exec node --test [ERR_MODULE_NOT_FOUND]", "npm run check"));
    let f = route(&r, "finish-line-04e5d4");
    assert_eq!(f.trigger, Trigger::AfterEdit { paths: "registry/*.csv".into() });
    assert_eq!(f.step, Step::Run { command: "npm run codegen".into() });
    let a = r.advisories().into_iter().find(|a| a.instinct == "U4").expect("U4 advisory");
    assert_eq!(a.slots["path"], "src/generated/registry.ts");
    assert_eq!(r.runs().len(), 3, "{:#?}", shape(&r));
    // The failing `npm test` is never itself directed.
    assert!(!shape(&r).iter().any(|(_, _, c)| c == "npm test"));
}

#[test]
fn pnpm_repository_gets_package_manager_discipline() {
    let r = bound("pnpm");
    let install: Vec<&Route> =
        r.routes.iter().filter(|x| x.step == Step::Run { command: "pnpm install".into() }).collect();
    assert_eq!(install.len(), 2, "npm install and npm ci");
    for x in install {
        // An environment step waits for a person (Design O11).
        assert_eq!((x.effect, x.state), (Effect::Environment, RouteState::Draft));
    }
    assert!(has(&r, "fail exec npm install []", "pnpm install"));
    assert!(has(&r, "fail exec npm test []", "pnpm test"));
    assert_eq!(route(&r, "dead-end-68b3e9").state, RouteState::Probation);
    let kinds: BTreeSet<&str> = r.advisories().iter().map(|a| a.template_id).collect();
    assert_eq!(kinds, ["lockfile", "package-manager"].into_iter().collect());
    let pm = r.advisories().into_iter().find(|a| a.template_id == "package-manager").unwrap();
    assert_eq!(*pm.trigger, Trigger::AfterCommand { command: "exec npm install".into() });
    assert_eq!(
        pm.text().unwrap(),
        "This repository uses pnpm (pnpm-lock.yaml), not npm: use pnpm for installs and scripts here."
    );
}

#[test]
fn rust_workspace_and_go_generate_bind_advisories_only() {
    let r = bound("rust");
    assert!(r.runs().is_empty(), "{:#?}", shape(&r));
    assert_eq!(r.advisories().len(), 1);
    assert_eq!(r.advisories()[0].slots["path"], "Cargo.lock");
    let facts = adage_instincts::extract(&fixture("rust"));
    assert_eq!(facts.cargo.unwrap().members, vec!["crates/ledger", "crates/ledger-cli"]);
    assert!(facts.ci_commands.contains(&"cargo test --workspace --locked".to_string()));

    let r = bound("go");
    assert!(r.runs().is_empty(), "G1 (go generate finish line) ships disabled: {:#?}", shape(&r));
    let gen = r.advisories().into_iter().find(|a| a.instinct == "U4").expect("U4 advisory");
    assert_eq!(gen.slots["path"], "aspect/aspect_string.go");
    assert_eq!(gen.slots["command"], "go generate ./...");
    assert!(r.advisories().iter().any(|a| a.instinct == "U6" && a.slots["path"] == "go.sum"));
    let facts = adage_instincts::extract(&fixture("go"));
    let go = facts.go.unwrap();
    assert_eq!(go.module, "example.com/signals");
    assert_eq!(go.generate, vec![("aspect/aspect.go".to_string(), "stringer -type=Aspect".to_string())]);
}

#[test]
fn python_wrapper_covers_every_spelling_of_pytest() {
    let r = bound("pytool");
    for runner in ["exec pytest", "exec python -m pytest", "exec python3 -m pytest"] {
        assert!(has(&r, &format!("fail {runner} [DJANGO_SETTINGS_MODULE]"), "make test"), "{runner}: {:#?}", shape(&r));
    }
    assert!(r.origins.values().any(|o| o == "P1"));
    // U5 from Makefile prerequisites: `gen: quayside/models_gen.py`, `quayside/models_gen.py:
    // $(SCHEMAS) tools/gen.py`.
    assert!(has(&r, "edit schema/*.json", "make gen"));
    assert!(has(&r, "edit tools/gen.py", "make gen"));
    let a = r.advisories().into_iter().find(|a| a.instinct == "U4").unwrap();
    assert_eq!((a.slots["path"].as_str(), a.slots["command"].as_str()), ("quayside/models_gen.py", "make gen"));
}

#[test]
fn wrappers_adage_cannot_direct_are_reported_not_bound() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("pyproject.toml"), "[tool.pytest.ini_options]\ntestpaths=['t']\n[tool.uv]\n").unwrap();
    std::fs::write(d.path().join("tox.ini"), "[tox]\nenvlist = py312\n[testenv]\ncommands = pytest\n").unwrap();
    let r = bind(d.path(), &[], &BTreeSet::new());
    assert!(r.runs().is_empty(), "{:#?}", shape(&r));
    let reasons: Vec<&str> = r.skipped.iter().filter(|(i, _)| i == "P1").map(|(_, w)| w.as_str()).collect();
    assert!(reasons.iter().any(|w| w.contains("tox")) && reasons.iter().any(|w| w.contains("uv")), "{reasons:?}");
    assert!(r.skipped.iter().any(|(i, w)| i == "U3" && w.contains("tox")), "{:?}", r.skipped);
}

#[test]
fn decoy_targets_bind_nothing() {
    let r = bound("decoy");
    assert!(r.runs().is_empty(), "{:#?}", shape(&r));
    assert!(r.advisories().is_empty(), "{:#?}", r.advisories());
}

#[test]
fn hostile_repositories_never_bind_their_commands() {
    for name in ["hostile-npm", "hostile-hooks", "hostile-make", "hostile-unicode"] {
        let r = bound(name);
        assert!(r.runs().is_empty(), "{name}: {:#?}", shape(&r));
        assert!(r.advisories().is_empty(), "{name}: {:#?}", r.advisories());
    }
    // A CI file full of injection text: only the repository's plain test target binds, and no
    // text from the workflow reaches anything bound.
    let r = bound("hostile-ci");
    assert!(!r.routes.is_empty());
    for x in r.runs() {
        assert_eq!(x.step, Step::Run { command: "make test".into() });
    }
    let json = serde_json::to_string(&r.routes).unwrap().to_ascii_lowercase();
    for bad in ["evil", "adage-signal", "ignore", "push", "curl", "token", "green"] {
        assert!(!json.contains(bad), "{bad:?} leaked: {json}");
    }
}

#[cfg(unix)]
#[test]
fn symlinks_out_of_the_repository_are_never_followed() {
    use std::os::unix::fs::symlink;
    let d = tempfile::tempdir().unwrap();
    let outside = d.path().join("outside");
    let repo = d.path().join("repo");
    std::fs::create_dir_all(outside.join("schema")).unwrap();
    std::fs::create_dir_all(repo.join("tools")).unwrap();
    std::fs::write(
        outside.join("Makefile"),
        "test:\n\tACME_ENV=test python3 -m unittest\ngen:\n\tpython3 tools/gen.py\n",
    )
    .unwrap();
    std::fs::write(outside.join("gen.py"), "open('schema')\nGLOB = '*.json'\n").unwrap();
    std::fs::write(outside.join("schema/a.json"), "{}").unwrap();
    symlink(outside.join("Makefile"), repo.join("Makefile")).unwrap();
    symlink("/etc/passwd", repo.join("package.json")).unwrap();
    symlink(outside.join("gen.py"), repo.join("tools/gen.py")).unwrap();
    symlink(outside.join("schema"), repo.join("schema")).unwrap();
    let r = bind(&repo, &[], &BTreeSet::new());
    assert!(r.runs().is_empty(), "{:#?}", shape(&r));
    assert!(r.advisories().is_empty());
    assert!(
        r.skipped.contains(&("declared".into(), "Makefile: symlink escapes the repository".into())),
        "{:?}",
        r.skipped
    );
    assert!(r.skipped.contains(&("declared".into(), "package.json: symlink escapes the repository".into())));

    // A symlink that stays inside the repository is read.
    let inside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(inside.path().join("build")).unwrap();
    std::fs::write(inside.path().join("build/Makefile.real"), "test:\n\tACME_ENV=test python3 -m unittest\n").unwrap();
    symlink(inside.path().join("build/Makefile.real"), inside.path().join("Makefile")).unwrap();
    let r = bind(inside.path(), &[], &BTreeSet::new());
    assert!(has(&r, "fail exec python3 -m unittest [ACME_ENV]", "make test"), "{:#?}", shape(&r));
}

#[test]
fn huge_files_are_skipped_and_marker_scans_read_only_the_head() {
    let d = tempfile::tempdir().unwrap();
    let pad = "x".repeat(70 * 1024);
    std::fs::write(
        d.path().join("package.json"),
        format!(r#"{{"scripts":{{"test":"node --test","check":"node --import ./a.mjs --test"}},"pad":"{pad}"}}"#),
    )
    .unwrap();
    std::fs::write(d.path().join("a.mjs"), "").unwrap();
    std::fs::write(d.path().join("Makefile"), format!("test:\n\tACME_ENV=1 python3 -m unittest\n# {pad}\n")).unwrap();
    // A large generated file: only its first bytes are read, which is where the marker is.
    let mut big = String::from("// Code generated by protoc-gen-go. DO NOT EDIT.\npackage pb\n");
    big.push_str(&"var _ = 0\n".repeat(200_000));
    std::fs::create_dir_all(d.path().join("pb")).unwrap();
    std::fs::write(d.path().join("pb/api.pb.go"), big).unwrap();
    let r = bind(d.path(), &[], &BTreeSet::new());
    assert!(r.runs().is_empty(), "{:#?}", shape(&r));
    assert!(r.skipped.contains(&("declared".into(), "package.json: larger than 64 KiB".into())), "{:?}", r.skipped);
    assert!(r.skipped.contains(&("declared".into(), "Makefile: larger than 64 KiB".into())));
    assert!(r.advisories().iter().any(|a| a.slots["path"] == "pb/api.pb.go"), "{:?}", r.advisories());
}

#[test]
fn invisible_characters_in_makefile_targets_never_bind() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("Makefile"),
        "te\u{200b}st:\n\tACME_ENV=test python3 -m unittest\n\u{202e}check:\n\tACME_ENV=test python3 -m unittest\nci:\n\tACME_ENV=te\u{200b}st python3 -m unittest\n",
    )
    .unwrap();
    let r = bind(d.path(), &[], &BTreeSet::new());
    assert!(r.runs().is_empty(), "{:#?}", shape(&r));
}

#[test]
fn pathological_manifests_do_not_crash_the_binder() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("pyproject.toml"), "a = ".to_string() + &"[".repeat(60_000)).unwrap();
    std::fs::write(d.path().join("package.json"), "[".repeat(60_000)).unwrap();
    std::fs::write(d.path().join("Cargo.toml"), "x = ".to_string() + &"{a=".repeat(15_000)).unwrap();
    std::fs::write(d.path().join("Makefile"), "A = $(A)$(A)\nB = $(B\ntest:\n\t$(A) $(B) pytest\n".repeat(200))
        .unwrap();
    let r = bind(d.path(), &[], &BTreeSet::new());
    assert!(r.runs().is_empty(), "{:#?}", shape(&r));
}

#[test]
fn binding_is_deterministic_and_independent_of_where_the_repository_is() {
    for name in FIXTURES {
        let a = serde_json::to_vec(&bound(name)).unwrap();
        let b = serde_json::to_vec(&bound(name)).unwrap();
        assert_eq!(a, b, "{name}");
        // The same files elsewhere bind the same bytes.
        let d = tempfile::tempdir().unwrap();
        copy_dir(&fixture(name), &d.path().join("elsewhere"));
        let c = serde_json::to_vec(&bind(&d.path().join("elsewhere"), &[], &BTreeSet::new())).unwrap();
        assert_eq!(a, c, "{name} moved");
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

#[test]
fn every_bound_route_is_a_valid_capped_instinct() {
    let mut all: Vec<Route> = Vec::new();
    for name in FIXTURES {
        let r = bound(name);
        for x in &r.routes {
            assert_eq!(x.evidence.provenance, "instinct:core@0.1.0", "{name} {}", x.id);
            assert_eq!(x.evidence.support, 0);
            assert_eq!(x.aspect, Aspect::Yellow, "{name} {}", x.id);
            assert!(!x.authored && x.slots.is_empty());
            assert!(
                matches!(x.step, Step::Run { .. } | Step::Advise { .. }),
                "never a red avoid step: {name} {}",
                x.id
            );
            assert!(
                matches!(
                    x.trigger,
                    Trigger::AfterFailure { .. } | Trigger::AfterEdit { .. } | Trigger::AfterCommand { .. }
                ),
                "reactive only: {name} {}",
                x.id
            );
            assert!(x.is_instinct() && matches!(x.origin(), adage_tracks::Origin::Instinct { .. }));
            assert_eq!(x.evidence.by_origin.get("declared"), Some(&1));
            match x.effect {
                Effect::Observe | Effect::Verify | Effect::Regenerate => assert_eq!(x.state, RouteState::Probation),
                Effect::Environment => assert_eq!(x.state, RouteState::Draft),
                e => panic!("{name} {}: {e:?}", x.id),
            }
            assert!(r.origins.contains_key(&x.id));
        }
        for a in r.advisories() {
            let text = a.text().expect("renders");
            assert!(text.is_ascii() && !text.contains(['`', '<', '>', '"', '$', '|', ';']), "{text}");
        }
        // One route set per repository: validate each as one track, as learn would write it.
        let t = track(&format!("{}/instincts", name.replace('-', "")), r.routes.clone());
        adage_tracks::io::validate(&t).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        all.extend(r.routes);
    }
    assert!(all.len() >= 20, "{}", all.len());
}

fn track(name: &str, routes: Vec<Route>) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: name.into(),
                version: "0.1.0".into(),
                description: "d".into(),
                provenance: "instinct:core@0.1.0".into(),
            },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes,
    }
}

fn learned(id: &str, trigger: Trigger, step: Step) -> Route {
    Route {
        id: id.into(),
        summary: "learned".into(),
        trigger,
        step,
        slots: vec![],
        effect: Effect::Verify,
        aspect: Aspect::Red,
        state: RouteState::Active,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { provenance: "desire-path".into(), support: 7, ..Default::default() },
        authored: false,
    }
}

#[test]
fn learned_routes_and_review_rejections_override_instincts() {
    let base = bound("acme");
    // 1. The miner learned the same dead end: same id, learned wins.
    let same = learned(
        "dead-end-d41b1e",
        Trigger::SessionStart,
        Step::Avoid { avoid: "python3 -m unittest".into(), instead: "make test".into(), signature: "x".into() },
    );
    // 2. A learned route says something different after pytest fails.
    let other = learned(
        "dead-end-000001",
        Trigger::SessionStart,
        Step::Avoid { avoid: "pytest".into(), instead: "make ci".into(), signature: "x".into() },
    );
    // 3. A learned finish line for the same glob.
    let mut fl = learned(
        "finish-line-abcdef",
        Trigger::AfterEdit { paths: "schema/*.json".into() },
        Step::Run { command: "make gen".into() },
    );
    fl.effect = Effect::Regenerate;
    fl.aspect = Aspect::Green;
    // 4. A learned dead end that avoids the command an instinct would direct.
    let avoid_it = learned(
        "dead-end-000002",
        Trigger::SessionStart,
        Step::Avoid { avoid: "python -m unittest / make gen".into(), instead: "make ci".into(), signature: "x".into() },
    );
    let rejected: BTreeSet<String> = ["dead-end-e8eb22".to_string()].into_iter().collect();
    let r = bind(&fixture("acme"), &[same, other, fl, avoid_it], &rejected);
    let ids: BTreeSet<&str> = r.routes.iter().map(|x| x.id.as_str()).collect();
    assert!(!ids.contains("dead-end-d41b1e"), "learned wins on the same id");
    assert!(!has(&r, "fail exec pytest []", "make test"), "a learned route directs make ci after pytest");
    assert!(!ids.contains("finish-line-e7e809"), "a learned finish line covers schema/*.json");
    assert!(!ids.contains("dead-end-e8eb22"), "rejected in review");
    assert!(!has(&r, "fail exec python -m unittest []", "make test"), "learned route names python -m unittest");
    let why = |needle: &str| r.skipped.iter().any(|(_, w)| w.contains(needle));
    assert!(
        why("has this id (learned wins)") && why("directs make ci") && why("rejected in review"),
        "{:#?}",
        r.skipped
    );
    assert!(why("directs make gen after editing schema/*.json"));
    // What is left still binds, and is a subset of the unopposed bind.
    assert!(!r.routes.is_empty() && r.routes.len() < base.routes.len());

    // Earlier instinct routes are not "learned": re-binding over them changes nothing.
    let again = bind(&fixture("acme"), &base.routes, &BTreeSet::new());
    assert_eq!(serde_json::to_vec(&again).unwrap(), serde_json::to_vec(&base).unwrap());
}

#[test]
fn configuration_can_disable_instincts() {
    let opts = BindOptions { disabled: ["U3".to_string(), "U4".to_string()].into_iter().collect() };
    let r = bind_with(&fixture("acme"), &[], &BTreeSet::new(), &opts);
    assert!(r.origins.values().all(|o| o != "U3" && o != "U4"), "{:?}", r.origins);
    assert!(r.routes.iter().any(|x| x.id == "dead-end-d41b1e"));
}

#[test]
fn the_index_keeps_the_learned_route_over_an_instinct_with_the_same_id() {
    // The index de-duplicates by id and prefers first-hand routes over instincts by provenance,
    // whatever the track names sort to (an instinct track named `aaa` still loses).
    let r = bound("acme");
    let instincts = track("acme/instincts", r.routes.clone());
    let mut l = learned(
        "dead-end-d41b1e",
        Trigger::SessionStart,
        Step::Avoid { avoid: "python3 -m unittest".into(), instead: "make test".into(), signature: "x".into() },
    );
    l.evidence.provenance = "desire-path".into();
    let mut dead_ends = track("acme/dead-ends", vec![l]);
    dead_ends.manifest.track.provenance = "desire-path".into();
    let idx = Index::compile(&[instincts, dead_ends]);
    let hit: Vec<_> = idx.routes.iter().filter(|x| x.route.id == "dead-end-d41b1e").collect();
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].track, "acme/dead-ends");
    let mut early = track("acme/aaa", r.routes.clone());
    early.manifest.track.name = "acme/aaa".into();
    let mut l2 = learned(
        "dead-end-d41b1e",
        Trigger::SessionStart,
        Step::Avoid { avoid: "python3 -m unittest".into(), instead: "make test".into(), signature: "x".into() },
    );
    l2.evidence.provenance = "desire-path".into();
    let mut late = track("acme/zzz", vec![l2]);
    late.manifest.track.provenance = "desire-path".into();
    let idx = Index::compile(&[early, late]);
    let hit: Vec<_> = idx.routes.iter().filter(|x| x.route.id == "dead-end-d41b1e").collect();
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].track, "acme/zzz", "learned beats instinct by provenance, not by name");
}

#[test]
fn route_ids_line_up_with_route_kinds() {
    for name in FIXTURES {
        for x in bound(name).routes {
            let intended = adage_instincts::intended_kind(&x);
            let today = adage_tracks::route_kind(&x);
            assert_eq!(intended, today, "{}", x.id);
            if x.id.starts_with("finish-line-") {
                assert_eq!(today, adage_tracks::KIND_FINISH_LINE);
            } else if x.id.starts_with("dead-end-") {
                assert_eq!(today, adage_tracks::KIND_DEAD_ENDS, "{}", x.id);
            } else {
                assert!(x.id.starts_with("instinct-"), "{}", x.id);
                let advice = matches!(x.step, Step::Advise { .. });
                assert_eq!(
                    today,
                    if advice { adage_tracks::KIND_ADVICE } else { adage_tracks::KIND_DEAD_ENDS },
                    "{}",
                    x.id
                );
            }
        }
    }
}

#[test]
fn catalogue_and_docs_agree() {
    let docs = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/instincts.md")).unwrap();
    for i in &adage_instincts::catalogue().instincts {
        let row = docs
            .lines()
            .find(|l| l.starts_with(&format!("| {} |", i.id)))
            .unwrap_or_else(|| panic!("docs/instincts.md has no catalogue row for {}", i.id));
        let enabled = if i.enabled { "yes" } else { "no" };
        assert!(row.contains(&format!("| {} |", i.priority)), "{row}");
        assert!(row.trim_end().ends_with(&format!("| {enabled} |")), "{}: {row}", i.id);
    }
}

#[test]
fn disabled_instincts_detect_nothing() {
    let facts = adage_instincts::extract(&fixture("acme"));
    let table = adage_core::CommandTable::load();
    let cat = adage_instincts::catalogue();
    let ctx = adage_instincts::detect::Ctx { facts: &facts, table: &table, cat };
    for i in cat.instincts.iter().filter(|i| !i.enabled) {
        let d = adage_instincts::detect::detect(&i.id, &ctx);
        assert!(d.routes.is_empty() && d.advisories.is_empty() && d.skipped.is_empty(), "{}", i.id);
    }
    let order: BTreeSet<&str> = adage_instincts::detect::BIND_ORDER.into_iter().collect();
    let ids: BTreeSet<&str> = cat.instincts.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(order, ids, "every catalogue entry has a place in the bind order");
}
