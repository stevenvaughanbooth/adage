//! `adage train <skill>` end to end on a small repository: the compile, the replay (where this
//! machine has a sandbox), the review, the staged pack, and determinism.

use adage_train::compile::Verified;
use adage_train::skill::{self, Options};
use adage_train::verify;
use std::path::Path;
use std::process::Command;

const SKILL: &str = "---
name: widget-work
description: Work a ticket (WD-NNN) in the widget repository.
---

# Working a ticket

1. **Read the ticket.** Run `scripts/ticket <ID>` (for example `scripts/ticket WD-123`). Read the
   linked files first.
2. After editing `schema/*.json`, run `make gen` to regenerate the models.
3. **Make the change** in the files under `src/`.
4. **Run the tests with `make test`.** Never run `python3 -m unittest` instead of `make test`: it
   fails with `ACME_ENV is not set`. Expect `OK` at the end.
5. After editing `src/*.py`, run `make suite` to check everything.
6. **Before you finish, run `make ci`.**

## Notes

- Do not commit; leave the working tree for review.
- Keep changes small.
";

const MAKEFILE: &str = "gen:\n\t@echo generated\ntest:\n\t@echo 'Ran 3 tests' ; echo OK\nsuite:\n\t@echo 'suite is broken' ; exit 2\nci: test\n\t@echo ci ok\n";

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@localhost", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

fn repo() -> tempfile::TempDir {
    let t = tempfile::Builder::new().prefix("widget").tempdir().unwrap();
    let d = t.path();
    for sub in ["scripts", "schema", "src", "tests"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    std::fs::write(d.join("scripts/ticket"), "#!/bin/sh\necho \"ticket $1: src/widget.py\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.join("scripts/ticket"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(d.join("Makefile"), MAKEFILE).unwrap();
    std::fs::write(d.join("schema/widget.json"), "{}\n").unwrap();
    std::fs::write(d.join("src/widget.py"), "X = 1\n").unwrap();
    std::fs::write(d.join("tests/__init__.py"), "raise RuntimeError('ACME_ENV is not set')\n").unwrap();
    std::fs::write(d.join("SKILL.md"), SKILL).unwrap();
    git(d, &["init", "-q"]);
    git(d, &["add", "."]);
    git(d, &["commit", "-q", "-m", "init"]);
    t
}

fn tools() -> bool {
    ["make", "python3", "git"]
        .iter()
        .all(|t| std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(t).is_file())))
}

fn run(repo: &Path, home: &Path, replay: bool) -> skill::Outcome {
    let o = Options {
        skill: repo.join("SKILL.md"),
        repo: Some(repo.to_path_buf()),
        replay: verify::Options { enabled: replay, wall_ms: 60_000 },
        home: home.to_path_buf(),
    };
    skill::run(&o).unwrap()
}

#[test]
fn a_skill_compiles_replays_and_stages_one_reviewed_pack() {
    if !tools() {
        eprintln!("skipped: make, python3 or git is missing");
        return;
    }
    let r = repo();
    let home = tempfile::tempdir().unwrap();
    let out = run(r.path(), home.path(), true);
    let c = &out.compiled;
    let ids: Vec<&str> = c.candidates.iter().map(|x| x.route.id.as_str()).collect();
    for want in [
        "widget-work-s1-lookup",
        "widget-work-s1-read-linked",
        "widget-work-s2-regenerate",
        "widget-work-s4-avoid",
        "widget-work-s4-verify",
        "widget-work-s6-before-finish",
        "widget-work-uncommitted",
    ] {
        assert!(ids.contains(&want), "{want} missing from {ids:?}");
    }
    assert!(c.text["s3"][0].starts_with("needs judgment"));
    let text = skill::render(&out);
    assert!(text.contains("adage train accept widget-work"), "{text}");
    let pack = out.pack.as_ref().expect("a pack");
    assert_eq!(pack.manifest.pack.origin, adage_pack::PackOrigin::Trained);
    assert_eq!(pack.manifest.provenance.skill.as_deref(), Some(format!("sha256:{}", c.graph.skill.sha256).as_str()));
    let proc_ = pack.procedure.as_ref().expect("procedure.json");
    assert_eq!(proc_.steps.len(), 6);
    // Staged: the pack as reviewed, and the review with its digest.
    let staged = adage_pack::Pack::load(&out.dir.join("pack")).unwrap();
    assert_eq!(&staged, pack);
    let review = skill::load_review(&out.dir).unwrap();
    assert_eq!(Some(review.digest.clone()), out.digest);
    // Nothing is installed.
    assert!(!home.path().join("tracks").exists());
    assert!(!home.path().join("index.json").exists());

    if adage_exec::sandbox::detect().available() {
        let v = |id: &str| &c.candidates.iter().find(|x| x.route.id == id).unwrap().verified;
        assert_eq!(v("widget-work-s1-lookup"), &Verified::Works);
        assert_eq!(v("widget-work-s4-verify"), &Verified::Works);
        assert_eq!(v("widget-work-s4-avoid"), &Verified::Works, "{:?}", c.candidates);
        // `make suite` fails on a clean checkout: stale, dropped, reported.
        assert!(c.stale.contains_key("s5"), "{:?}", c.stale);
        assert!(!ids.iter().any(|i| i.contains("s5")), "{ids:?}");
        assert!(text.contains("STALE"), "{text}");
        assert!(matches!(&proc_.steps[4].outcome, adage_pack::procedure::Outcome::Stale { .. }));
        let lookup = c.candidates.iter().find(|x| x.route.id == "widget-work-s1-lookup").unwrap();
        // `scripts/ticket` is an unlisted script whose name says nothing of what it changes, so
        // its effect is environment: prime at most, verified or not.
        assert_eq!(lookup.route.strength, adage_pack::Strength::Prime);
        // The same skill and repository give the same pack.
        let again = run(r.path(), home.path(), true);
        assert_eq!(again.digest, out.digest);
    } else {
        eprintln!("replay part skipped: no sandbox on this machine");
    }
}

#[test]
fn without_a_replay_nothing_fires() {
    if !tools() {
        return;
    }
    let r = repo();
    let home = tempfile::tempdir().unwrap();
    let out = run(r.path(), home.path(), false);
    assert!(!out.compiled.candidates.is_empty());
    for c in &out.compiled.candidates {
        assert_ne!(c.route.strength, adage_pack::Strength::Fire, "{}", c.route.id);
        assert!(matches!(c.verified, Verified::NotReplayed(_)));
    }
    assert!(skill::render(&out).contains("not replayed"));
}
