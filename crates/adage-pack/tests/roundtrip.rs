//! A pack survives writing and reading; its closure, digests and signature behave as specified.

mod common;

use adage_pack::{digest_dir, sign_dir, verify_dir, Level, Pack, Shape, NAMESPACE};
use ssh_key::{getrandom::SysRng, rand_core::UnwrapErr};
use ssh_key::{LineEnding, PrivateKey};
use std::time::SystemTime;

#[test]
fn sample_pack_loads_and_round_trips() {
    let p = Pack::from_files(&common::files()).unwrap();
    assert_eq!(p.manifest.pack.name, "acme/work-items");
    assert_eq!(p.manifest.exec.requested, Level::E1);
    assert_eq!(p.routes.len(), 5);
    let wic = p.routes.iter().find(|r| r.id == "work-item-context").unwrap();
    assert_eq!(wic.shape, Shape::Run);
    assert_eq!(wic.steps.len(), 2);

    let d = tempfile::tempdir().unwrap();
    p.write(d.path()).unwrap();
    let back = Pack::load(d.path()).unwrap();
    assert_eq!(back, p);
    assert_eq!(digest_dir(d.path()).unwrap(), p.digest().unwrap());
    // Writing again changes nothing.
    back.write(d.path()).unwrap();
    assert_eq!(digest_dir(d.path()).unwrap(), p.digest().unwrap());
}

#[test]
fn closure_lists_every_node_and_the_implicit_chains() {
    let p = Pack::from_files(&common::files()).unwrap();
    let text = p.closure().unwrap().render();
    let has = |l: &str| text.lines().any(|x| x == l);
    let lookup = common::lookup_sha();
    let v = common::vendored_sha();
    assert!(has(&format!("repo-file scripts/wi-lookup {lookup}")), "{text}");
    assert!(has(&format!("script scripts/sha256-{v} {v}")), "{text}");
    assert!(has("builtin repo-path-list -"), "{text}");
    assert!(has("tool python3 -"), "{text}");
    assert!(has("template run@1 -"), "{text}");
    // The lookup → read-linked pair works through an implicit after edge.
    assert!(has("edge:after route:work-item-context->route:read-linked-files -"), "{text}");
    // The dead end's `instead` is the finish line's command.
    assert!(has("edge:instead route:dead-end-pytest->route:finish-line-tests -"), "{text}");
    let route_line = format!("route work-item-context {}", p.route_exec_digest("work-item-context").unwrap());
    assert!(has(&route_line), "{text}");
    // Sorted, one line each.
    let lines: Vec<&str> = text.lines().collect();
    let mut sorted = lines.clone();
    sorted.sort();
    assert_eq!(lines, sorted);
    assert!(lines.iter().all(|l| l.split(' ').count() == 3), "{text}");
}

#[test]
fn write_mirrors_the_pack_and_drops_a_stale_signature() {
    let p = Pack::from_files(&common::files()).unwrap();
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("routes")).unwrap();
    std::fs::write(d.path().join("routes/gone.yaml"), "id: gone\n").unwrap();
    std::fs::write(d.path().join("pack.sig"), "stale").unwrap();
    p.write(d.path()).unwrap();
    assert!(!d.path().join("routes/gone.yaml").exists());
    assert!(!d.path().join("pack.sig").exists());
    Pack::load(d.path()).unwrap();
}

fn key(dir: &std::path::Path) -> (std::path::PathBuf, ssh_key::PublicKey) {
    let k = PrivateKey::random(&mut UnwrapErr(SysRng), ssh_key::Algorithm::Ed25519).unwrap();
    let p = dir.join("id_test");
    std::fs::write(&p, k.to_openssh(LineEnding::LF).unwrap().as_bytes()).unwrap();
    (p, k.public_key().clone())
}

#[test]
fn signs_and_verifies_in_the_pack_namespace() {
    let keys = tempfile::tempdir().unwrap();
    let (priv_key, pk) = key(keys.path());
    let d = tempfile::tempdir().unwrap();
    Pack::from_files(&common::files()).unwrap().write(d.path()).unwrap();
    sign_dir(d.path(), None, Some(&priv_key)).unwrap();
    // The signature does not change the digest, and the signed pack still loads.
    Pack::load(d.path()).unwrap();
    let line = adage_sign::key_line(&pk);
    let open = adage_sign::allowed_signers::parse(&format!("t {line}")).unwrap();
    let pack_only = adage_sign::allowed_signers::parse(&format!("t namespaces=\"{NAMESPACE}\" {line}")).unwrap();
    let other_only = adage_sign::allowed_signers::parse(&format!("t namespaces=\"other\" {line}")).unwrap();
    assert!(verify_dir(d.path(), &open, SystemTime::now()).is_ok());
    assert!(verify_dir(d.path(), &pack_only, SystemTime::now()).is_ok());
    // An entry for another namespace does not authorise packs, which can request execution.
    let e = verify_dir(d.path(), &other_only, SystemTime::now()).unwrap_err();
    assert!(e.to_string().contains("namespaces"), "{e}");
    // A signature in another namespace over the same digest is not a pack signature.
    let digest = digest_dir(d.path()).unwrap();
    let other_sig = adage_sign::sign(&adage_sign::message(&digest).unwrap(), "other", None, Some(&priv_key)).unwrap();
    std::fs::write(d.path().join("pack.sig"), &other_sig.pem).unwrap();
    assert!(verify_dir(d.path(), &open, SystemTime::now()).is_err());
    // Any change to a signed file breaks the signature.
    sign_dir(d.path(), None, Some(&priv_key)).unwrap();
    let r = d.path().join("routes/finish-line-tests.yaml");
    let text = std::fs::read_to_string(&r).unwrap().replace("src/*.py", "lib/*.py");
    std::fs::write(&r, text).unwrap();
    assert!(verify_dir(d.path(), &open, SystemTime::now()).is_err());
}

#[test]
fn optional_discovery_pack_is_portable_signed_advice_without_execution() {
    let marketplace = adage_pack::builtin::marketplace().unwrap();
    let (entry, original) = marketplace.packs.iter().find(|(e, _)| e.name == "adage/verification-discovery").unwrap();
    assert_eq!(entry.default, adage_pack::marketplace::Initially::Off);
    assert_eq!(original.manifest.exec.requested, Level::E0);
    assert!(original
        .routes
        .iter()
        .all(|r| r.exec.is_none() && r.steps.iter().all(|s| s.run.is_none() && s.advise.is_some())));
    let out = tempfile::tempdir().unwrap();
    original.write(out.path()).unwrap();
    let keys = tempfile::tempdir().unwrap();
    let (private, public) = key(keys.path());
    sign_dir(out.path(), None, Some(&private)).unwrap();
    let signers = adage_sign::allowed_signers::parse(&format!("fixture {}", adage_sign::key_line(&public))).unwrap();
    verify_dir(out.path(), &signers, SystemTime::now()).unwrap();
    let imported = Pack::load(out.path()).unwrap();
    assert_eq!(&imported, original);
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("package.json"), r#"{"scripts":{"test":"vitest run"}}"#).unwrap();
    let routes = adage_pack::instincts::bound_routes(&imported, repo.path(), None);
    assert_eq!(routes.len(), 2, "{routes:?}");
    for route in routes {
        assert!(matches!(route.status, adage_pack::Status::Live { max_level: Level::E0 }));
        let runtime = route.route.unwrap();
        assert!(matches!(runtime.step, adage_tracks::Step::Advise { .. }));
        assert_eq!(runtime.evidence.support, 0);
        assert_eq!(runtime.evidence.provenance, "instinct:adage/verification-discovery@0.1.0");
    }
}

#[test]
fn recovery_pack_preserves_its_stateful_template_through_signed_roundtrip() {
    let marketplace = adage_pack::builtin::marketplace().unwrap();
    let (_, original) = marketplace.packs.iter().find(|(e, _)| e.name == "adage/verification-recovery").unwrap();
    let out = tempfile::tempdir().unwrap();
    original.write(out.path()).unwrap();
    let keys = tempfile::tempdir().unwrap();
    let (private, public) = key(keys.path());
    sign_dir(out.path(), None, Some(&private)).unwrap();
    let signers = adage_sign::allowed_signers::parse(&format!("fixture {}", adage_sign::key_line(&public))).unwrap();
    verify_dir(out.path(), &signers, SystemTime::now()).unwrap();
    let imported = Pack::load(out.path()).unwrap();
    assert_eq!(&imported, original);
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("Makefile"), "test:\n\tpython3 -m pytest\n").unwrap();
    let bound = adage_pack::instincts::bound_routes(&imported, repo.path(), None);
    assert_eq!(bound.len(), 1);
    assert!(matches!(bound[0].status, adage_pack::Status::Live { max_level: Level::E0 }));
    let runtime = bound[0].route.as_ref().unwrap();
    assert!(matches!(&runtime.trigger, adage_tracks::Trigger::AfterCommand { .. }));
    assert!(
        matches!(&runtime.step, adage_tracks::Step::Advise { template, .. } if template == "verification-unexplained-recovery")
    );
    assert_eq!(runtime.evidence.support, 0);
}

#[test]
fn verification_procedure_templates_have_distinct_portable_policy_identity() {
    let original = &adage_pack::builtin::marketplace()
        .unwrap()
        .packs
        .iter()
        .find(|(e, _)| e.name == "adage/verification-discovery")
        .unwrap()
        .1;
    let mut digests = std::collections::BTreeSet::new();
    for template in ["verification-discovery", "verification-clean-state", "verification-negative-control"] {
        let mut manifest = original.manifest.clone();
        manifest.requires.templates = vec![format!("advise:{template}@1"), "advise@1".into()];
        let mut routes = original.routes.clone();
        routes[0].steps[0].advise.as_mut().unwrap().template = template.into();
        let p = Pack::new(manifest, routes, Default::default(), Default::default()).unwrap();
        assert!(digests.insert(p.route_exec_digest("investigate-empty-selection").unwrap()));
        assert_eq!(Pack::from_files(&p.files().unwrap()).unwrap(), p);
    }
    let mut after_edit = original.routes.clone();
    after_edit[0].trigger = adage_pack::TriggerV2::AfterEdit { paths: "{paths}".into() };
    let err = Pack::new(original.manifest.clone(), after_edit, Default::default(), Default::default()).unwrap_err();
    assert!(format!("{err:#}").contains("discovery advice follows a failure"), "{err:#}");
}

#[test]
fn line_endings_do_not_change_the_digest_or_the_closure() {
    let files = common::files();
    let crlf: std::collections::BTreeMap<String, Vec<u8>> = files
        .iter()
        .map(|(k, v)| {
            let v = if k.starts_with("scripts/") {
                v.clone()
            } else {
                String::from_utf8(v.clone()).unwrap().replace('\n', "\r\n").into_bytes()
            };
            (k.clone(), v)
        })
        .collect();
    assert_eq!(adage_pack::pack_digest(&files), adage_pack::pack_digest(&crlf));
    assert_eq!(Pack::from_files(&crlf).unwrap(), Pack::from_files(&files).unwrap());
}
