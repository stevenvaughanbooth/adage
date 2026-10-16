//! The marketplace built into Adage (`.adage/marketplace.toml` and `packs/` in this repository):
//! the binary's copy is the repository's, every implemented instinct ships as a pinned pack, the
//! packs bind exactly what the instinct binder binds, a pack decides what its detector may bind,
//! templates are checked as strictly as routes, and installing them is `adage add`'s install.

use adage_pack::builtin;
use adage_pack::instincts::{self, Source};
use adage_pack::marketplace::{self, Marketplace};
use adage_pack::Pack;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn base() -> Vec<Pack> {
    builtin::marketplace().unwrap().packs.iter().map(|(_, p)| p.clone()).collect()
}

fn base_pack(name: &str) -> Pack {
    base().into_iter().find(|p| p.manifest.pack.name == name).unwrap_or_else(|| panic!("no {name}"))
}

/// Every file under `dir`, relative to `root`, `/`-separated.
fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(root, &p, out);
        } else {
            out.insert(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read(&p).unwrap());
        }
    }
}

#[test]
fn the_built_in_marketplace_is_this_repository_s() {
    let root = repo();
    let mut on_disk = BTreeMap::new();
    walk(&root, &root.join("packs"), &mut on_disk);
    on_disk.insert(marketplace::MANIFEST.into(), std::fs::read(root.join(marketplace::MANIFEST)).unwrap());
    let embedded: BTreeMap<String, Vec<u8>> =
        builtin::FILES.iter().map(|(p, t)| (p.to_string(), t.as_bytes().to_vec())).collect();
    assert_eq!(
        embedded.keys().collect::<Vec<_>>(),
        on_disk.keys().collect::<Vec<_>>(),
        "builtin::FILES must list exactly the marketplace's files (add or remove the include_str! lines)"
    );
    assert_eq!(embedded, on_disk);
    // Read from the checkout as any marketplace is: every pin holds, and its home is this
    // repository.
    let m = marketplace::load_dir(&root).unwrap();
    assert_eq!(m.name(), builtin::NAME);
    assert_eq!(m.manifest.marketplace.repository.as_deref(), Some("github.com/stevenvaughanbooth/adage"));
    assert_eq!(m.packs.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(), base());
}

#[test]
fn every_implemented_instinct_ships_in_a_base_pack() {
    let cat = adage_instincts::catalogue();
    let enabled: BTreeSet<String> = cat.instincts.iter().filter(|i| i.enabled).map(|i| i.id.clone()).collect();
    let mut shipped = BTreeSet::new();
    for p in base() {
        let adage_pack::Applicability::Detector { detector, catalogue } = &p.manifest.applicability else {
            panic!("{} is not an instinct pack", p.manifest.pack.name)
        };
        assert_eq!(catalogue, &adage_pack::bind::catalogue_id());
        assert!(!p.manifest.pack.description.is_empty(), "{}", p.manifest.pack.name);
        assert!(p.routes.iter().all(|r| r.detector.as_deref() == Some(detector.as_str())));
        assert!(shipped.insert(detector.clone()), "{detector} ships twice");
    }
    assert_eq!(shipped, enabled);
    let m = &builtin::marketplace().unwrap().manifest;
    for e in &m.packs {
        assert_eq!(
            e.default,
            if matches!(e.name.as_str(), "adage/verification-discovery" | "adage/verification-recovery") {
                marketplace::Initially::Off
            } else {
                marketplace::Initially::Detected
            }
        );
    }
}

#[test]
fn recovery_pack_requires_explicit_selection_and_binds_only_advice() {
    let m = builtin::marketplace().unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("Makefile"), "test:\n\tpython3 -m pytest\n").unwrap();
    let detected: Vec<_> =
        m.packs.iter().filter(|(e, _)| e.default == marketplace::Initially::Detected).map(|(_, p)| p).collect();
    let none = BTreeSet::new();
    let ordinary = instincts::bind(&detected, repo.path(), &[], &none, &none);
    assert!(!ordinary.origins.values().any(|id| id == "U13"));
    let recovery = &m.packs.iter().find(|(e, _)| e.name == "adage/verification-recovery").unwrap().1;
    let mut selected = detected.clone();
    selected.push(recovery);
    let enabled = instincts::bind(&selected, repo.path(), &[], &none, &none);
    let routes: Vec<_> = enabled.of_pack("adage/verification-recovery").collect();
    assert_eq!(routes.len(), 1);
    assert!(
        matches!(&routes[0].step, adage_tracks::Step::Advise { template, .. } if template == "verification-unexplained-recovery")
    );
    assert!(recovery.routes.iter().all(|r| r.exec.is_none()));
    assert_eq!(recovery.manifest.exec.requested, adage_pack::Level::E0);
    assert!(instincts::bind(&detected, repo.path(), &[], &none, &none)
        .of_pack("adage/verification-recovery")
        .next()
        .is_none());
}

/// A checkout with a project `.venv` (made, not activated) holding pytest, for U11.
fn venv_repo(dir: &Path) -> PathBuf {
    let root = dir.join("venv-repo");
    std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
    std::fs::create_dir_all(root.join(".venv/lib/python3.12/site-packages/pytest")).unwrap();
    std::fs::create_dir_all(root.join("click")).unwrap();
    std::fs::write(root.join(".venv/pyvenv.cfg"), "home = /usr/bin\n").unwrap();
    std::fs::write(root.join(".venv/lib/python3.12/site-packages/pytest/__init__.py"), "").unwrap();
    std::fs::write(root.join("click/__init__.py"), "").unwrap();
    std::fs::write(root.join(".venv/bin/python"), "").unwrap();
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"click\"\n\n[tool.pytest.ini_options]\naddopts = \"-q\"\n\n[dependency-groups]\ntests = [\"pytest\"]\n",
    )
    .unwrap();
    root
}

#[test]
fn the_base_packs_bind_exactly_what_the_binder_binds_on_every_fixture() {
    let root = repo();
    let d = tempfile::tempdir().unwrap();
    let mut repos: Vec<PathBuf> = std::fs::read_dir(root.join("crates/adage-instincts/tests/fixtures"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    repos.push(root.join("evals/sandboxes/acme-billing"));
    repos.push(venv_repo(d.path()));
    repos.sort();
    let packs = base();
    let refs: Vec<&Pack> = packs.iter().collect();
    let none = BTreeSet::new();
    let mut bound = 0;
    for r in &repos {
        let direct = adage_instincts::bind(r, &[], &none);
        let via = instincts::bind(&refs, r, &[], &none, &none);
        let strip = |rs: &[adage_tracks::Route]| {
            rs.iter()
                .map(|x| {
                    let mut x = x.clone();
                    x.evidence.provenance.clear();
                    if let Some(adage_tracks::Origin::Instinct { pack, version, .. }) = &mut x.evidence.origin {
                        pack.clear();
                        version.clear();
                    }
                    x
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(strip(&via.routes), strip(&direct.routes), "{}", r.display());
        assert_eq!(via.origins, direct.origins, "{}", r.display());
        for x in &via.routes {
            let (pack, _) = &via.templates[&x.id];
            let p = base_pack(pack);
            assert_eq!(x.evidence.provenance, format!("instinct:{pack}@{}", p.manifest.pack.version));
            assert!(
                matches!(x.origin(), adage_tracks::Origin::Instinct { pack: q, .. } if &q == pack),
                "{} {x:?}",
                r.display()
            );
        }
        bound += via.routes.len();
    }
    assert!(bound > 40, "the fixtures bind every base pack's shapes ({bound} routes)");
    // Every template of every base pack admits something somewhere.
    let mut used: BTreeSet<(String, String)> = BTreeSet::new();
    for r in &repos {
        used.extend(instincts::bind(&refs, r, &[], &none, &none).templates.into_values());
    }
    for p in &packs {
        for t in &p.routes {
            assert!(
                used.contains(&(p.manifest.pack.name.clone(), t.id.clone())),
                "{} {} binds nowhere",
                p.manifest.pack.name,
                t.id
            );
        }
    }
}

#[test]
fn a_pack_decides_what_its_detector_may_bind() {
    let fixture = repo().join("crates/adage-instincts/tests/fixtures/pnpm");
    let none = BTreeSet::new();
    let all = base();
    let refs: Vec<&Pack> = all.iter().collect();
    let full = instincts::bind(&refs, &fixture, &[], &none, &none);
    let installs = |b: &instincts::InstinctBinding| {
        b.routes.iter().filter(|r| r.effect == adage_core::Effect::Environment).count()
    };
    assert!(installs(&full) > 0, "pnpm install binds, nascent: {full:?}");
    // Without the install template, N1 still answers test failures, but directs no install.
    let mut pm = base_pack("adage/package-manager");
    pm.routes.retain(|r| r.id != "install-with-declared-manager");
    pm.check().unwrap();
    let mut packs: Vec<Pack> =
        all.iter().filter(|p| p.manifest.pack.name != "adage/package-manager").cloned().collect();
    packs.push(pm);
    let refs: Vec<&Pack> = packs.iter().collect();
    let less = instincts::bind(&refs, &fixture, &[], &none, &none);
    assert_eq!(installs(&less), 0);
    assert!(less.skipped.iter().any(|(who, why)| who == "N1" && why.contains("no template")), "{:?}", less.skipped);
    assert!(less.routes.iter().any(|r| r.effect == adage_core::Effect::Verify));
    // A pack left out binds nothing; a detector turned off binds nothing.
    let only: Vec<Pack> = vec![base_pack("adage/lockfiles")];
    let refs: Vec<&Pack> = only.iter().collect();
    let b = instincts::bind(&refs, &fixture, &[], &none, &none);
    assert!(!b.routes.is_empty() && b.origins.values().all(|i| i == "U6"), "{b:?}");
    let off: BTreeSet<String> = ["U6".to_string()].into();
    assert!(instincts::bind(&refs, &fixture, &[], &none, &off).routes.is_empty());
}

/// python-env's files without its CLOSURE, with one route file replaced.
fn python_env_with(route: &str, yaml: &str) -> BTreeMap<String, Vec<u8>> {
    let mut f = builtin::marketplace().unwrap().files("adage/python-env").unwrap();
    f.remove("CLOSURE");
    f.insert(format!("routes/{route}.yaml"), yaml.as_bytes().to_vec());
    f
}

fn refused(files: &BTreeMap<String, Vec<u8>>) -> String {
    let parsed = adage_pack::validate::parse(files).unwrap();
    format!("{:#}", adage_pack::validate::validate(&parsed).expect_err("refused"))
}

#[test]
fn templates_are_checked_as_strictly_as_routes() {
    let good = std::str::from_utf8(
        &builtin::marketplace().unwrap().files("adage/python-env").unwrap()["routes/run-tests-in-project-env.yaml"],
    )
    .unwrap()
    .to_string();
    let id = "run-tests-in-project-env";
    for (from, to, why) in [
        ("command: '{runner}'", "command: exec pytest", "blanks, not values"),
        ("signature: '{signature}'", "signature: No module named pytest", "blanks, not values"),
        ("- '{command}'", "- make\n    - test", "its detector names the command"),
        ("- '{command}'", "- '{command}'\n    - --verbose", "its detector names the command"),
        ("effect: verify", "effect: publish", "at most an environment step"),
        ("detector: U11", "detector: U5", "not the pack's"),
        ("strength: prime", "strength: fire", "prime"),
        ("- when: intent_changes", "- when: result_equals\n  step: run\n  field: x\n  value: y", "result_equals"),
        ("on: after_failure", "on: session_start", "an instinct is reactive"),
    ] {
        assert!(good.contains(from), "{from}");
        let msg = refused(&python_env_with(id, &good.replacen(from, to, 1)));
        assert!(msg.contains(why), "{from} -> {to}: {msg}");
    }
    // Under `detector = "*"`, each template's own detector id is checked.
    let mut f = python_env_with(id, &good.replacen("detector: U11", "detector: u11", 1));
    let star = String::from_utf8(f["pack.toml"].clone()).unwrap().replace("detector = \"U11\"", "detector = \"*\"");
    f.insert("pack.toml".into(), star.into_bytes());
    assert!(refused(&f).contains("not a catalogue id"));
    let msg = refused(&python_env_with(id, &format!("{good}evidence:\n  support: 3\n")));
    assert!(msg.contains("no evidence"), "{msg}");
    let msg = refused(&python_env_with(id, &format!("{good}links:\n- '#name-project-interpreter'\n")));
    assert!(msg.contains("no links"), "{msg}");
    // An advice template's slots are its blanks, and only its own.
    let advice = "id: name-project-interpreter\nshape: advise\ndetector: U11\ntrigger:\n  on: after_failure\n  command: '{runner}'\n  signature: '{signature}'\nsteps:\n- id: advise\n  advise:\n    template: project-python\n    slots:\n      interpreter: SLOT\n  effect: observe\nstrength: prime\n";
    for (slot, why) in
        [(".venv/bin/python", "the blank {interpreter}"), ("'{interpreter}'\n      note: '{note}'", "has no slot note")]
    {
        let msg = refused(&python_env_with("name-project-interpreter", &advice.replace("SLOT", slot)));
        assert!(msg.contains(why), "{slot}: {msg}");
    }
    // A template belongs to a detector pack, and a detector pack holds only templates.
    let mut f = adage_pack::builtin::marketplace().unwrap().files("adage/python-env").unwrap();
    f.remove("CLOSURE");
    let toml = String::from_utf8(f["pack.toml"].clone()).unwrap();
    let repo_pack = toml.replace(
        "kind = \"detector\"\ndetector = \"U11\"\ncatalogue = \"core@0.1.0\"",
        "kind = \"repo\"\nrepo_keys = [\"github.com/acme/billing\"]",
    );
    f.insert("pack.toml".into(), repo_pack.replace("origin = \"instinct\"", "origin = \"reflex\"").into_bytes());
    assert!(refused(&f).contains("only a detector pack's routes are templates"));
}

/// The built-in marketplace's files with `edit` applied to its manifest and `drop` packs removed:
/// a newer build's marketplace.
fn newer_marketplace(drop: &[&str], edit: impl Fn(&mut Marketplace)) -> marketplace::Loaded {
    let mut files: BTreeMap<String, Vec<u8>> =
        builtin::FILES.iter().map(|(p, t)| (p.to_string(), t.as_bytes().to_vec())).collect();
    let mut m = builtin::marketplace().unwrap().manifest.clone();
    for name in drop {
        let e = m.get(name).unwrap().clone();
        files.retain(|p, _| !p.starts_with(&format!("{}/", e.source)));
        m.packs.retain(|x| x.name != *name);
    }
    edit(&mut m);
    files.insert(marketplace::MANIFEST.into(), toml::to_string_pretty(&m).unwrap().into_bytes());
    marketplace::Loaded::from_files(files).unwrap()
}

#[test]
fn syncing_a_marketplace_installs_records_repairs_and_removes() {
    use adage_pack::install::{self, Via};
    use adage_pack::marketplace::Installed;
    let d = tempfile::tempdir().unwrap();
    let tracks = d.path().join("tracks");
    let b = builtin::marketplace().unwrap();
    let names: Vec<String> = b.packs.iter().map(|(e, _)| e.name.clone()).collect();
    assert!(b.status(&tracks).iter().all(|(_, s)| *s == Installed::No));
    // First sync: every pack installed where `adage add` installs a pack, exactly as pinned, and
    // recorded as from the adage marketplace.
    let synced = builtin::install(&tracks).unwrap();
    assert_eq!((synced.installed.clone(), synced.updated.len(), synced.removed.len()), (names.clone(), 0, 0));
    let records = install::records(&tracks);
    for (e, p) in &b.packs {
        let dir = install::pack_dir(&tracks, &e.name);
        assert_eq!(adage_pack::digest_dir(&dir).unwrap(), e.digest, "installed exactly as pinned");
        assert_eq!(&Pack::load(&dir).unwrap(), p);
        assert!(builtin::reserved(&e.name));
        let r = &records[&e.name];
        assert_eq!(
            (&r.version, &r.digest, &r.via),
            (&e.version, &e.digest, &Via::Marketplace { name: "adage".into() })
        );
    }
    assert!(b.status(&tracks).iter().all(|(_, s)| *s == Installed::Current));
    assert!(builtin::install(&tracks).unwrap().is_empty(), "in sync: nothing written");
    // A damaged copy is restored; nothing else is touched.
    let lock = install::pack_dir(&tracks, "adage/lockfiles");
    std::fs::write(lock.join("routes/lockfile.yaml"), "id: lockfile\n").unwrap();
    assert!(b.status(&tracks).iter().any(|(e, s)| e.name == "adage/lockfiles" && *s == Installed::Differs));
    assert_eq!(builtin::install(&tracks).unwrap().updated, vec!["adage/lockfiles".to_string()]);
    assert!(Pack::load(&lock).is_ok());
    // A newer build that no longer lists a pack: syncing it removes that pack and its record, and
    // leaves a pack installed another way alone.
    let mut team = base_pack("adage/lockfiles");
    team.manifest.pack.name = "acme/lockfiles".into();
    team.write(&install::pack_dir(&tracks, "acme/lockfiles")).unwrap();
    let newer = newer_marketplace(&["adage/lockfiles"], |_| {});
    let synced = newer.sync(&tracks).unwrap();
    assert_eq!(
        (synced.installed.len(), synced.updated.len(), synced.removed.clone()),
        (0, 0, vec!["adage/lockfiles".into()])
    );
    assert!(!lock.exists());
    assert!(!install::records(&tracks).contains_key("adage/lockfiles"));
    assert!(install::pack_dir(&tracks, "acme/lockfiles").join("pack.toml").is_file());
}

#[test]
fn binding_reads_built_in_packs_from_the_binary_and_skips_stale_ones() {
    use adage_pack::install::{self, Record, Via};
    let d = tempfile::tempdir().unwrap();
    let tracks = d.path().join("tracks");
    builtin::install(&tracks).unwrap();
    let names: Vec<String> = builtin::marketplace().unwrap().packs.iter().map(|(e, _)| e.name.clone()).collect();
    // An instinct pack added by a person binds beside them; a reflex pack is not an instinct pack.
    let mut team = base_pack("adage/lockfiles");
    team.manifest.pack.name = "acme/lockfiles".into();
    team.write(&install::pack_dir(&tracks, "acme/lockfiles")).unwrap();
    // A pack an older build's marketplace installed, which this build's no longer lists, is stale
    // until the next sync removes it: it does not bind as if a person had added it.
    let mut old = base_pack("adage/lockfiles");
    old.manifest.pack.name = "adage/old-lockfiles".into();
    let dir = install::pack_dir(&tracks, "adage/old-lockfiles");
    old.write(&dir).unwrap();
    let digest = adage_pack::digest_dir(&dir).unwrap();
    let via = Via::Marketplace { name: "adage".into() };
    install::record(&tracks, "adage/old-lockfiles", Record { version: "0.1.0".into(), digest, via }).unwrap();
    let (avail, problems) = instincts::available(&tracks);
    assert_eq!(avail.iter().filter(|(_, s)| *s == Source::Builtin).count(), names.len());
    assert!(avail.iter().any(|(p, s)| p.manifest.pack.name == "acme/lockfiles" && matches!(s, Source::Installed(_))));
    assert!(!avail.iter().any(|(p, _)| p.manifest.pack.name == "adage/old-lockfiles"));
    assert_eq!(avail.len(), names.len() + 1);
    assert!(
        problems
            .iter()
            .any(|(who, why)| who == "adage/old-lockfiles" && why.contains("no longer in the adage marketplace")),
        "{problems:?}"
    );
    // The next sync removes it.
    assert_eq!(builtin::install(&tracks).unwrap().removed, vec!["adage/old-lockfiles".to_string()]);
    assert!(instincts::available(&tracks).1.is_empty());
}

#[test]
fn a_marketplace_pin_is_checked() {
    let root = repo();
    let text = std::fs::read_to_string(root.join(marketplace::MANIFEST)).unwrap();
    let pin = &builtin::marketplace().unwrap().manifest.packs[0].digest;
    let m = Marketplace::parse(&text.replacen(pin.as_str(), &"0".repeat(64), 1)).unwrap();
    let files: BTreeMap<String, Vec<u8>> =
        builtin::FILES.iter().map(|(p, t)| (p.to_string(), t.as_bytes().to_vec())).collect();
    let err = format!("{:#}", m.load(&files).unwrap_err());
    assert!(err.contains("not the") && err.contains("its marketplace pins"), "{err}");
}
