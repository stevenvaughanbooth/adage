//! `verified.json` (plan D4, test plan T-D4-01): `adage train` on the acme skill, in the acme
//! sandbox, writes one record per compiled command and what its replay found, dated, with whether
//! the skill is inert; the pack's digest and `procedure.json` do not depend on it.

use adage_pack::verified::{self, Outcome};
use adage_train::compile::Verified;
use adage_train::skill::{self, Options};
use adage_train::verify;
use std::path::{Path, PathBuf};
use std::process::Command;

fn acme() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/sandboxes/acme-billing")
}

fn acme_skill() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/lab/skills/skills/acme-work-item/SKILL.md"),
    )
    .unwrap()
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .unwrap();
    assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
}

/// The acme sandbox as a git repository, with `skill` as its SKILL.md (outside the tracked tree's
/// code, as a skill directory would hold it).
fn repo(tmp: &Path, skill: &str) -> PathBuf {
    let r = tmp.join("acme");
    copy_dir(&acme(), &r);
    std::fs::create_dir_all(r.join(".claude/skills/acme-work-item")).unwrap();
    std::fs::write(r.join(".claude/skills/acme-work-item/SKILL.md"), skill).unwrap();
    git(&r, &["init", "-q"]);
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "fixture"]);
    r
}

fn tools() -> bool {
    ["make", "python3", "git"]
        .iter()
        .all(|t| std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(t).is_file())))
}

fn train(r: &Path, home: &Path, replay: bool) -> skill::Outcome {
    skill::run(&Options {
        skill: r.join(".claude/skills/acme-work-item/SKILL.md"),
        repo: Some(r.to_path_buf()),
        replay: verify::Options { enabled: replay, wall_ms: 60_000 },
        home: home.to_path_buf(),
    })
    .unwrap()
}

#[test]
fn train_writes_verified_json_beside_the_pack_and_outside_its_digest() {
    if !tools() {
        eprintln!("skipped: make, python3 or git is missing");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path(), &acme_skill());
    let home = tmp.path().join("home");
    let out = train(&r, &home, true);
    assert_eq!(out.dir, home.join("train/skills/acme-work-item"));
    let v = verified::read(&out.dir).unwrap();
    assert_eq!(v.schema, verified::SCHEMA);
    assert_eq!(v.skill.name, "acme-work-item");
    assert_eq!(v.skill.sha256, out.compiled.graph.skill.sha256);
    assert!(v.skill.inert, "the acme skill has no dynamic block and no fork");
    assert_eq!(v.repo, out.repo_key);
    assert_eq!(Some(v.digest.clone()), out.digest);
    assert!(v.accepted_at.is_none(), "staged unaccepted");
    // One record per compiled candidate that runs a command, kept or dropped as stale.
    let runs = out
        .compiled
        .candidates
        .iter()
        .chain(&out.compiled.dropped)
        .filter(|c| c.route.steps.iter().any(|s| s.run.is_some()))
        .filter(|c| !matches!(c.route.shape, adage_pack::Shape::Avoid | adage_pack::Shape::Read))
        .count();
    assert_eq!(v.steps.len(), runs, "{:#?}", v.steps);
    let commands: Vec<&str> = v.steps.iter().map(|s| s.command.as_str()).collect();
    for want in ["scripts/wi-lookup {id}", "make gen", "make test", "make ci"] {
        assert!(commands.contains(&want), "{want} missing from {commands:?}");
    }
    let today = adage_tracks::review::today();
    for s in &v.steps {
        assert_eq!(s.date, today);
        assert_eq!(s.sandbox, out.report.sandbox);
        let kept = out.compiled.candidates.iter().find(|c| c.route.id == s.route);
        match s.result {
            Outcome::Works => assert_eq!(kept.map(|c| &c.verified), Some(&Verified::Works), "{s:?}"),
            Outcome::NotReplayed => {
                assert!(matches!(kept.map(|c| &c.verified), Some(Verified::NotReplayed(_))), "{s:?}")
            }
            Outcome::Stale => {
                assert!(s.route.is_empty() && s.signature.as_deref().is_some_and(|x| !x.is_empty()), "{s:?}")
            }
        }
    }
    if adage_exec::sandbox::detect().available() {
        let test = v.steps.iter().find(|s| s.command == "make test").unwrap();
        assert_eq!(test.result, Outcome::Works, "{v:#?}");
        // The sandbox copy has no tracker cache (it is not committed), so the lookup fails on a
        // clean checkout: stale, out of the pack, and recorded with what failed.
        let lookup = v.steps.iter().find(|s| s.command == "scripts/wi-lookup {id}").unwrap();
        assert_eq!(lookup.result, Outcome::Stale, "{v:#?}");
        assert!(lookup.signature.as_deref().is_some_and(|x| x.contains("Error")), "{lookup:?}");
    } else {
        assert!(v.steps.iter().all(|s| s.result == Outcome::NotReplayed), "{v:#?}");
    }
    // Not part of the pack: its digest is the staged pack's alone, and procedure.json says
    // nothing of it.
    assert!(!out.dir.join("pack").join(verified::VERIFIED).exists());
    assert_eq!(Some(adage_pack::digest_dir(&out.dir.join("pack")).unwrap()), out.digest);
    let procedure = std::fs::read_to_string(out.dir.join("pack/procedure.json")).unwrap();
    assert!(!procedure.contains(&today) && !procedure.contains("verified"), "{procedure}");
    // Accepted, then trained again on the same skill: the same pack, still accepted; a changed
    // skill is staged unaccepted. (Without the replay, whose failure lines can name a sandbox's
    // temporary directory, so that the same skill gives the same pack.)
    let first = train(&r, &home, false);
    assert!(verified::accept(&first.dir, first.digest.as_deref().unwrap(), "2026-10-03").unwrap());
    let again = train(&r, &home, false);
    assert_eq!(again.digest, first.digest);
    assert_eq!(verified::read(&again.dir).unwrap().accepted_at.as_deref(), Some("2026-10-03"));
    std::fs::write(
        r.join(".claude/skills/acme-work-item/SKILL.md"),
        acme_skill().replace("Keep changes minimal", "Keep changes small"),
    )
    .unwrap();
    let changed = train(&r, &home, false);
    let v = verified::read(&changed.dir).unwrap();
    assert_ne!(v.skill.sha256, out.compiled.graph.skill.sha256);
    assert!(v.accepted_at.is_none(), "a changed skill waits for a new acceptance");
}

#[test]
fn a_dynamic_block_or_a_forked_context_is_not_inert() {
    if !tools() {
        return;
    }
    let dynamic = acme_skill().replace("## Notes\n", "## Notes\n\n- The branch: !`git branch --show-current`\n");
    let fork = acme_skill().replacen("name: acme-work-item\n", "name: acme-work-item\ncontext: fork\n", 1);
    for (skill, what) in [(dynamic, "a dynamic-context block"), (fork, "context: fork")] {
        let tmp = tempfile::tempdir().unwrap();
        let r = repo(tmp.path(), &skill);
        let out = train(&r, &tmp.path().join("home"), false);
        assert!(!out.inert, "{what}");
        let v = verified::read(&out.dir).unwrap();
        assert!(!v.skill.inert, "{what}");
        assert!(v.steps.iter().all(|s| s.result == Outcome::NotReplayed), "replay off");
    }
}
