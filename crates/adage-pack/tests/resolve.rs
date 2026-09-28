//! Resolution against a repository (Track C §2.3): applicability, requirements, L-ref pins, the
//! runtime form, determinism, and the core instincts pack.

mod common;

use adage_pack::bind::{applies, bind, Applies, BindEnv, Status, Target, SHAPE_NOT_SUPPORTED};
use adage_pack::{Level, Os, Pack};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A PATH holding (fake, never run) executables with these names.
fn fake_path(dir: &Path, tools: &[&str]) -> std::ffi::OsString {
    let bin = dir.join(format!("bin-{}", tools.join("-")));
    std::fs::create_dir_all(&bin).unwrap();
    for t in tools {
        let p = bin.join(t);
        std::fs::write(&p, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    bin.into_os_string()
}

fn env(dir: &Path, tools: &[&str]) -> BindEnv {
    BindEnv { os: Os::Linux, path: Some(fake_path(dir, tools)), cas: None }
}

/// A checkout with an origin, so its `repo_key` is the origin form.
fn checkout(dir: &Path, origin: &str) -> PathBuf {
    let root = dir.join("repo");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/config"), format!("[remote \"origin\"]\n\turl = https://{origin}.git\n")).unwrap();
    root
}

fn status<'a>(b: &'a adage_pack::Binding, id: &str) -> &'a Status {
    &b.routes.iter().find(|r| r.id == id).unwrap_or_else(|| panic!("no route {id}")).status
}

fn reason(b: &adage_pack::Binding, id: &str) -> String {
    status(b, id).reason().unwrap_or_else(|| panic!("{id} is live")).to_string()
}

const LOOKUP_SCRIPT: &str = "#!/bin/sh\necho '{\"files\": []}'\n";

/// A pack with one L-ref lookup, a finish line and a dead end, for `github.com/acme/billing`.
fn lref_pack(applicability: &str) -> Pack {
    let sha = adage_pack::digest::sha256_hex(LOOKUP_SCRIPT.as_bytes());
    let mut f = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        f.insert(k.to_string(), v.into_bytes());
    };
    put(
        "pack.toml",
        format!(
            r#"format = "adage-pack/1"

[pack]
name = "acme/billing"
version = "1.0.0"
origin = "reflex"

[applicability]
{applicability}

[requires]
tools = [{{ name = "make" }}]
templates = ["avoid@1", "obligation@1", "run@1"]
repo_files = [{{ path = "scripts/wi-lookup", sha256 = "{sha}" }}]

[exec]
requested = "E1"
"#
        ),
    );
    put(
        "routes/lookup.yaml",
        format!(
            r#"id: lookup
shape: run
trigger: {{ on: prompt, span: ticket }}
inputs:
  item: {{ type: work-item-id, from: {{ prompt: {{ span: ticket }} }} }}
steps:
  - id: lookup
    run: {{ argv: ["./scripts/wi-lookup", "{{item}}", "--json"] }}
    effect: observe
    script: {{ path: scripts/wi-lookup, closure: ref, sha256: "{sha}" }}
strength: fire
exec: {{ level: E1 }}
"#
        ),
    );
    put("routes/finish-line-tests.yaml", common::FINISH_LINE.into());
    put(
        "routes/dead-end-pytest.yaml",
        common::DEAD_END.replace("instead: [\"make\", \"test\"]", "instead: [\"make\", \"check\"]"),
    );
    put("types/work-item-id.yaml", common::WORK_ITEM_ID.into());
    Pack::from_files(&common::with_closure(f)).unwrap()
}

#[test]
fn the_sample_binds_with_reasons() {
    let d = tempfile::tempdir().unwrap();
    let pack = Pack::from_files(&common::files()).unwrap();
    let t = Target { repo: "github.com/acme/payments".into(), root: None };
    let b = bind(&pack, &t, &env(d.path(), &["python3", "make"]));
    assert_eq!(b.applies, Applies::Yes);
    // An L-ref script with no checkout to check it against.
    assert!(reason(&b, "work-item-context").contains("no checkout"), "{}", reason(&b, "work-item-context"));
    // With the checkout, its script matches, but two steps have no runtime form yet.
    let root = checkout(d.path(), "github.com/acme/payments");
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(root.join("scripts/wi-lookup"), common::LOOKUP).unwrap();
    let here = bind(&pack, &Target::checkout(&root), &env(d.path(), &["python3", "make"]));
    assert_eq!(reason(&here, "work-item-context"), SHAPE_NOT_SUPPORTED);
    // A vendored script not in the cas.
    assert!(reason(&b, "count-items").contains("not in the cas"), "{}", reason(&b, "count-items"));
    assert!(status(&b, "dead-end-pytest").is_live());
    assert!(status(&b, "read-linked-files").is_live());
    assert_eq!(status(&b, "finish-line-tests"), &Status::Live { max_level: Level::E0 });
    // Without make on PATH, the finish line is inert (the dead end only names make).
    let b = bind(&pack, &t, &env(d.path(), &["python3"]));
    assert_eq!(reason(&b, "finish-line-tests"), "the tool make is not on PATH");
    assert!(status(&b, "dead-end-pytest").is_live());
    // Another repository: nothing binds.
    let other = Target { repo: "github.com/acme/other".into(), root: None };
    let b = bind(&pack, &other, &env(d.path(), &["python3", "make"]));
    assert!(matches!(b.applies, Applies::No(_)) && b.routes.is_empty());
    // The OS requirement.
    let mut e = env(d.path(), &["python3", "make"]);
    e.os = Os::Windows;
    let b = bind(&pack, &t, &e);
    assert!(b.routes.iter().all(|r| r.status.reason().is_some_and(|w| w.contains("requires linux or macos"))));
}

#[test]
fn an_lref_script_binds_only_while_its_hash_matches() {
    let d = tempfile::tempdir().unwrap();
    let root = checkout(d.path(), "github.com/acme/billing");
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(root.join("scripts/wi-lookup"), LOOKUP_SCRIPT).unwrap();
    let pack = lref_pack("kind = \"repo\"\nrepo_keys = [\"github.com/acme/billing\"]");
    let e = env(d.path(), &["make"]);
    let b = bind(&pack, &Target::checkout(&root), &e);
    assert_eq!(b.repo, "github.com/acme/billing");
    assert_eq!(status(&b, "lookup"), &Status::Live { max_level: Level::E1 });
    let lookup = b.routes.iter().find(|r| r.id == "lookup").unwrap();
    let sha = adage_pack::digest::sha256_hex(LOOKUP_SCRIPT.as_bytes());
    assert_eq!(lookup.scripts, vec![("scripts/wi-lookup".to_string(), sha.clone())]);
    assert_eq!(lookup.route_exec_digest, pack.route_exec_digest("lookup").unwrap());
    let exec = lookup.exec().unwrap();
    assert_eq!((exec.max_level, exec.scripts.len()), (Level::E1, 1));
    // The runtime form: today's route model, at forming, the command table's effect.
    let rt = lookup.route.as_ref().unwrap();
    assert_eq!(rt.step, adage_tracks::Step::Run { command: "./scripts/wi-lookup {item} --json".into() });
    assert_eq!(rt.state, adage_tracks::RouteState::Forming);
    assert_eq!(rt.slots.len(), 1);
    // Deterministic.
    assert_eq!(bind(&pack, &Target::checkout(&root), &e), b);

    // The script changes: that route is inert, the others stay live.
    std::fs::write(root.join("scripts/wi-lookup"), format!("{LOOKUP_SCRIPT}curl evil\n")).unwrap();
    let b = bind(&pack, &Target::checkout(&root), &e);
    let why = reason(&b, "lookup");
    assert!(why.contains("scripts/wi-lookup has changed") && why.contains(&sha[..12]), "{why}");
    assert!(status(&b, "finish-line-tests").is_live() && status(&b, "dead-end-pytest").is_live());
    // A symlink in its place is refused, never followed.
    #[cfg(unix)]
    {
        std::fs::remove_file(root.join("scripts/wi-lookup")).unwrap();
        let outside = d.path().join("outside");
        std::fs::write(&outside, LOOKUP_SCRIPT).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("scripts/wi-lookup")).unwrap();
        let b = bind(&pack, &Target::checkout(&root), &e);
        assert!(reason(&b, "lookup").contains("symlink"), "{}", reason(&b, "lookup"));
    }
    // No checkout on this machine: inert, with the reason.
    let b = bind(&pack, &Target { repo: "github.com/acme/billing".into(), root: None }, &e);
    assert!(reason(&b, "lookup").contains("no checkout"), "{}", reason(&b, "lookup"));
}

#[test]
fn the_root_commit_form_of_a_key_applies() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("billing");
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "a")
            .env("GIT_AUTHOR_EMAIL", "a@example")
            .env("GIT_COMMITTER_NAME", "a")
            .env("GIT_COMMITTER_EMAIL", "a@example")
            .output()
            .ok()
            .filter(|o| o.status.success())
    };
    if git(&["init", "-q"]).is_none() || git(&["commit", "-q", "--allow-empty", "-m", "root"]).is_none() {
        eprintln!("git is not available; skipping");
        return;
    }
    let key = adage_core::repo::repo_key(&root);
    assert!(key.starts_with("billing@"), "{key}");
    let pack = lref_pack(&format!("kind = \"repo\"\nrepo_keys = [\"{key}\"]"));
    assert_eq!(applies(&pack, &Target::checkout(&root)), Applies::Yes);
    // With an origin, the key is the origin form; the root-commit form still applies.
    std::fs::write(root.join(".git/config"), "[remote \"origin\"]\n\turl = https://github.com/acme/billing.git\n")
        .unwrap();
    assert_eq!(adage_core::repo::repo_key(&root), "github.com/acme/billing");
    assert_eq!(applies(&pack, &Target::checkout(&root)), Applies::Yes);
}

#[test]
fn fingerprint_applicability() {
    let d = tempfile::tempdir().unwrap();
    let root = checkout(d.path(), "github.com/acme/anything");
    std::fs::create_dir_all(root.join("proto/v1")).unwrap();
    std::fs::write(root.join("Makefile"), "gen:\n\tprotoc proto/v1/a.proto\n").unwrap();
    std::fs::write(root.join("proto/v1/a.proto"), "syntax = \"proto3\";\n").unwrap();
    std::fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();
    let fp = |preds: &str| lref_pack(&format!("kind = \"fingerprint\"\n{preds}"));
    let yes = fp("[[applicability.fingerprint]]\nmakefile_target = \"gen\"\n[[applicability.fingerprint]]\npath_exists = \"proto/**/*.proto\"\n[[applicability.fingerprint]]\nlockfile = \"pnpm-lock.yaml\"\n");
    assert_eq!(applies(&yes, &Target::checkout(&root)), Applies::Yes);
    let b = bind(&yes, &Target::checkout(&root), &env(d.path(), &["make"]));
    assert!(status(&b, "finish-line-tests").is_live());
    // All of: one predicate that does not hold refuses the repository.
    let no = fp("[[applicability.fingerprint]]\nmakefile_target = \"gen\"\n[[applicability.fingerprint]]\npackage_script = \"codegen\"\n");
    match applies(&no, &Target::checkout(&root)) {
        Applies::No(why) => assert!(why.contains("package.json script codegen"), "{why}"),
        other => panic!("{other:?}"),
    }
    let b = bind(&no, &Target::checkout(&root), &env(d.path(), &["make"]));
    assert!(b.routes.is_empty());
    let no = fp("[[applicability.fingerprint]]\npath_exists = \"schema/*.json\"\n");
    assert!(matches!(applies(&no, &Target::checkout(&root)), Applies::No(_)));
    // Without a checkout there is nothing to read.
    let t = Target { repo: "x".into(), root: None };
    assert!(matches!(applies(&yes, &t), Applies::No(_)));
}

/// A repository several instincts bind in: a Makefile test target, pytest, a generated file, a
/// lockfile.
fn instinct_repo(dir: &Path) -> PathBuf {
    let root = checkout(dir, "github.com/acme/instinctive");
    std::fs::write(root.join("Makefile"), "test:\n\tACME_ENV=test python3 -m pytest\n\nlint:\n\truff check .\n")
        .unwrap();
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"x\"\n\n[tool.pytest.ini_options]\naddopts = \"-q\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/models_pb2.py"),
        "# Generated by the protocol buffer compiler.  DO NOT EDIT!\nx = 1\n",
    )
    .unwrap();
    std::fs::write(root.join("package.json"), "{\"name\": \"x\", \"scripts\": {\"test\": \"jest\"}}\n").unwrap();
    std::fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();
    root
}

#[test]
fn the_core_instincts_pack_binds_exactly_what_the_instinct_binder_binds() {
    let d = tempfile::tempdir().unwrap();
    let root = instinct_repo(d.path());
    let none = BTreeSet::new();
    let direct = adage_instincts::bind(&root, &[], &none);
    assert!(direct.routes.len() >= 2, "the fixture binds several instincts: {direct:?}");
    let up = adage_pack::instincts::core_instincts(&root, &[], &none).unwrap().expect("instincts bind");
    let (pack, skipped) = (up.pack, up.skipped);
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(pack.manifest.pack.name, adage_pack::instincts::CORE_PACK);
    assert_eq!(pack.manifest.pack.origin, adage_pack::PackOrigin::Instinct);
    assert!(
        matches!(&pack.manifest.applicability, adage_pack::Applicability::Detector { detector, .. } if detector == "*")
    );
    // It survives its own serialisation, CLOSURE included.
    assert_eq!(Pack::from_files(&pack.files().unwrap()).unwrap(), pack);

    let tools: Vec<&str> = pack.manifest.requires.tools.iter().map(|t| t.name.as_str()).collect();
    let b = bind(&pack, &Target::checkout(&root), &env(d.path(), &tools));
    assert_eq!(b.applies, Applies::Yes);
    assert!(b.inert().next().is_none(), "{:?}", b.inert().collect::<Vec<_>>());
    let through_pack: Vec<adage_tracks::Route> = b.live().map(|r| r.route.clone().unwrap()).collect();
    assert_eq!(through_pack, direct.routes, "the pack path binds the binder's routes, byte for byte");
    for r in b.live() {
        assert!(r.exec().is_some_and(|e| e.max_level == Level::E0), "instincts request nothing");
    }
}

#[test]
fn detector_packs_apply_where_their_detector_fires() {
    let d = tempfile::tempdir().unwrap();
    let root = instinct_repo(d.path());
    let empty = checkout(&d.path().join("empty"), "github.com/acme/empty");
    let det = |detector: &str, catalogue: &str| {
        let mut p = lref_pack("kind = \"any\"");
        p.manifest.pack.origin = adage_pack::PackOrigin::Instinct;
        for r in &mut p.routes {
            r.strength = adage_tracks::Strength::Prime;
            if r.shape == adage_pack::Shape::Avoid {
                r.strength = adage_tracks::Strength::Inhibit;
            }
        }
        p.routes.retain(|r| r.shape != adage_pack::Shape::Avoid);
        p.manifest.applicability =
            adage_pack::Applicability::Detector { detector: detector.into(), catalogue: catalogue.into() };
        p
    };
    let cat = adage_pack::bind::catalogue_id();
    // U1 (a Makefile test target that wraps the runner) fires in the fixture, not in an empty repo.
    let p = det("*", &cat);
    assert_eq!(applies(&p, &Target::checkout(&root)), Applies::Yes);
    assert!(matches!(applies(&p, &Target::checkout(&empty)), Applies::No(_)));
    // An unknown catalogue or detector: every route inert, with the reason.
    let p = det("U5", "core@9.9.9");
    let b = bind(&p, &Target::checkout(&root), &env(d.path(), &["make"]));
    assert!(b.routes.iter().all(|r| r.status.reason().is_some_and(|w| w.contains("core@9.9.9"))), "{b:?}");
    let p = det("Z9", &cat);
    let b = bind(&p, &Target::checkout(&root), &env(d.path(), &["make"]));
    assert!(b.routes.iter().all(|r| r.status.reason().is_some_and(|w| w.contains("Z9"))), "{b:?}");
}
