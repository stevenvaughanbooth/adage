//! E3 staging (Track C §3.6) and gate G4 (§6): zero writes outside the declared outputs across
//! 500 staged runs of randomised hostile regenerate commands.
//!
//! Each run gets a fresh repository (sources, declared outputs under `gen/`, a real `.git`), a
//! world-readable layout (root runs sandboxed commands as nobody), and a script of one to four
//! hostile or legitimate steps, run as an approved L-ref script (`sh tools/act.sh`). After each
//! run every path of the repository, `.git` included, is compared with its state before: any
//! difference outside `gen/**` is a write outside the declared outputs. A directory outside the
//! repository and the repository's parent are checked too.
//!
//! `cargo test -p adage-exec --test stage -- --nocapture` prints the tally. Without a staging
//! backend (no user namespaces, no bubblewrap with overlays) the runs are skipped, and the test
//! asserts that nothing runs.

#![cfg(unix)]

use adage_core::{CommandTable, Effect};
use adage_exec::approvals::{Approval, ApprovalKey, Store};
use adage_exec::policy::Policy;
use adage_exec::request::{ActionKind, ActionSpec, ExecRequest, Labels, Level, Origin, OutputSpec, ScriptRef};
use adage_exec::run::Context;
use adage_exec::sandbox::{self, Profile};
use adage_exec::stage::{self, Outcome, StageBackend};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::process::Command;

const OUTPUTS: &str = "gen/**";

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// A template repository: sources, outputs, a committed `.git`.
fn template(dir: &Path) {
    for (p, body) in [
        ("src/main.py", "print('main')\n"),
        ("src/util.py", "def f():\n    return 1\n"),
        ("gen/out.py", "# generated\nX = 1\n"),
        ("gen/old.py", "# generated, stale\n"),
        ("README.md", "# fixture\n"),
        ("tools/placeholder", "x\n"),
    ] {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, body).unwrap();
    }
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "fixture"]);
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let t = e.file_type().unwrap();
        let dest = to.join(e.file_name());
        if t.is_dir() {
            copy_tree(&e.path(), &dest);
        } else if t.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(e.path()).unwrap(), dest).unwrap();
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn world_readable(p: &Path) {
    let m = std::fs::symlink_metadata(p).unwrap();
    if m.file_type().is_symlink() {
        return;
    }
    let mode = m.permissions().mode();
    let add = if m.is_dir() || mode & 0o100 != 0 { 0o555 } else { 0o444 };
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode | add)).unwrap();
    if m.is_dir() {
        for e in std::fs::read_dir(p).unwrap().flatten() {
            world_readable(&e.path());
        }
    }
}

/// Every path under `root` with what it holds: file bytes and mode, a symlink's target, or a
/// directory.
fn snapshot(root: &Path) -> BTreeMap<String, String> {
    fn go(dir: &Path, rel: &str, out: &mut BTreeMap<String, String>) {
        let mut es: Vec<_> = std::fs::read_dir(dir).map(|r| r.flatten().collect()).unwrap_or_default();
        es.sort_by_key(|e| e.file_name());
        for e in es {
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() { name } else { format!("{rel}/{name}") };
            let m = std::fs::symlink_metadata(e.path()).unwrap();
            let t = m.file_type();
            if t.is_symlink() {
                out.insert(r, format!("link {}", std::fs::read_link(e.path()).unwrap().display()));
            } else if t.is_dir() {
                out.insert(r.clone(), format!("dir {:o}", m.permissions().mode() & 0o7777));
                go(&e.path(), &r, out);
            } else if t.is_file() {
                let b = std::fs::read(e.path()).unwrap();
                out.insert(r, format!("file {:o} {}", m.permissions().mode() & 0o7777, adage_exec::sha256_hex(&b)));
            } else {
                out.insert(r, "special".into());
            }
        }
    }
    let mut out = BTreeMap::new();
    go(root, "", &mut out);
    out
}

fn in_outputs(rel: &str) -> bool {
    stage::matches_any(&[OUTPUTS.to_string()], rel) || rel == "gen"
}

/// The hostile and legitimate steps a run's script is drawn from. `{O}` is a directory outside
/// the repository, `{N}` the run number.
const STEPS: &[&str] = &[
    "echo gen{N} > gen/out.py",
    "echo x > src/main.py",
    "rm -f src/util.py",
    "echo x > new_top.txt",
    "rm -rf gen; ln -s ../src gen; echo x > gen/pwn.py",
    "ln -sf /etc/passwd gen/link.py",
    "ln -s {O} gen/esc; echo x > gen/esc/pwn",
    "echo x >> .git/config",
    "rm -f .git/HEAD",
    "mkdir -p .git/hooks; echo 'touch /tmp/pwned' > .git/hooks/pre-commit",
    "head -c 5000000 /dev/zero > big.bin",
    "head -c 3000000 /dev/zero > gen/big.bin",
    "echo x > {O}/pwn",
    "echo x > ../escape-{N}",
    "ln src/main.py gen/hard.py",
    "mv src/main.py gen/moved.py",
    "chmod +x src/util.py",
    "mkfifo gen/fifo",
    "rm -rf src",
    "mkdir -p gen/deep/a/b/c; echo x > gen/deep/a/b/c/d.py",
    "rm -f gen/old.py",
    "mkdir -p src/new; touch src/new/x",
    "echo x > $HOME/.profile",
    "ln -s ../.git gen/g; echo x > gen/g/config",
    "rm -rf gen; echo x > gen",
    "echo x > .gitignore",
    "touch gen/.hidden",
    "cp README.md gen/readme-copy.md",
    "exit 3",
    "ln -s ../src/main.py gen/alias.py",
];

/// A small deterministic generator (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn context<'a>(
    policy: &'a Policy,
    store: &'a Store,
    cap: &'a sandbox::Capability,
    table: &'a CommandTable,
) -> Context<'a> {
    Context {
        inherited_env: std::env::vars().collect(),
        policy,
        store,
        capability: cap,
        table,
        repo_key: "fixture".into(),
        cas: None,
        home: None,
        canaries: vec![],
    }
}

/// An approved E2 regenerate request running `sh tools/act.sh` in `repo`.
fn request(repo: &Path, script: &str) -> ExecRequest {
    std::fs::write(repo.join("tools/act.sh"), script).unwrap();
    std::fs::set_permissions(repo.join("tools/act.sh"), std::fs::Permissions::from_mode(0o644)).unwrap();
    ExecRequest {
        repo_root: repo.to_path_buf(),
        project_python: None,
        argv: vec!["sh".into(), "tools/act.sh".into()],
        inputs: BTreeMap::new(),
        contracts: BTreeMap::new(),
        effect: Effect::Regenerate,
        level: Level::E2,
        output: OutputSpec::default(),
        scripts: vec![ScriptRef { path: "tools/act.sh".into(), sha256: adage_exec::sha256_hex(script.as_bytes()) }],
        profile: Profile {
            limits: Profile::for_level(Level::E2).limits.with_timeout(Level::E2, 20_000),
            ..Profile::for_level(Level::E2)
        },
        labels: Labels { track: "fixture/g4@1.0.0".into(), route: "regen".into(), step: "run".into() },
        origin: Origin::Local,
        route_exec_digest: None,
        action: Some(ActionSpec { kind: ActionKind::Regenerate, outputs: vec![OUTPUTS.into()], cache: vec![] }),
    }
}

fn approved(req: &ExecRequest) -> Store {
    let mut s = Store::default();
    s.approve(Approval {
        key: ApprovalKey::for_request(req, "fixture"),
        route: adage_exec::approvals::route_label(req),
        approved_on: "2026-09-28T00:00:00Z".into(),
        by: None,
        basis: serde_json::Value::Null,
        auto: false,
        terms: None,
    });
    s
}

fn e3_policy() -> Policy {
    Policy { max_level: Level::E2, ..Policy::default() }
}

/// Run explicitly with ADAGE_TEST_PROJECT_PYTHON_FIXTURE naming a prepared, offline uv project
/// whose .venv contains pytest. The fixture is read only; every execution uses a disposable copy.
#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires a prepared offline uv/pytest fixture; exercises real project-Python staging"]
fn approved_project_python_runs_pytest_against_staged_source() {
    let source = PathBuf::from(std::env::var_os("ADAGE_TEST_PROJECT_PYTHON_FIXTURE").expect("prepared uv fixture"))
        .canonicalize()
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().canonicalize().unwrap().join("repo");
    std::fs::create_dir_all(repo.join("src/cpu_probe")).unwrap();
    std::fs::create_dir_all(repo.join("tests")).unwrap();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    copy_tree(&source.join(".venv"), &repo.join(".venv"));
    let site = repo.join(".venv/lib/python3.12/site-packages");
    assert!(site.join("pytest").is_dir(), "fixture must contain real pytest");
    for e in std::fs::read_dir(&site).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "pth") {
            let s = std::fs::read_to_string(e.path()).unwrap();
            std::fs::write(e.path(), s.replace(source.to_str().unwrap(), repo.to_str().unwrap())).unwrap();
        }
    }
    std::fs::write(repo.join("src/cpu_probe/__init__.py"), "answer = 42\n").unwrap();
    std::fs::write(repo.join("tests/test_probe.py"), format!("import cpu_probe\ndef test_staged_import():\n    assert cpu_probe.answer == 42\n    assert not cpu_probe.__file__.startswith({:?})\n    assert '/tree/src/' in cpu_probe.__file__\n", repo.to_str().unwrap())).unwrap();
    let table = CommandTable::load();
    let command = ".venv/bin/python -m pytest -q -p no:cacheprovider tests/test_probe.py";
    let ir = adage_tracks::index::IndexedRoute {
        repo: "fixture".into(),
        track: "fixture/learned".into(),
        version: "0.1.0".into(),
        exec: None,
        route: adage_tracks::Route {
            id: "verify".into(),
            summary: String::new(),
            trigger: adage_tracks::Trigger::AfterEdit { paths: "tests/*.py".into() },
            step: adage_tracks::Step::Run { command: command.into() },
            slots: vec![],
            effect: adage_tracks::io::classify(command, table),
            strength: adage_tracks::Strength::Fire,
            state: adage_tracks::RouteState::Firm,
            exit: vec![],
            evidence: Default::default(),
            authored: false,
        },
    };
    let index = adage_tracks::Index { routes: vec![ir.clone()], ..Default::default() };
    let planned = || adage_exec::route::plan(&ir, &index, &repo, &repo, Level::E2, None, table).unwrap().request;
    let original_request = planned();
    assert!(original_request.scripts.is_empty(), "pytest selections must remain staged inputs");
    let store = approved(&original_request);
    // Editing the selected test after approval keeps this exact dependency/command grant valid.
    std::fs::write(repo.join("src/cpu_probe/__init__.py"), "answer = 43\n").unwrap();
    std::fs::write(repo.join("tests/test_probe.py"), format!("import cpu_probe\ndef test_staged_import():\n    assert cpu_probe.answer == 43\n    assert not cpu_probe.__file__.startswith({:?})\n    assert '/tree/src/' in cpu_probe.__file__\n    assert '/tree/tests/' in __file__\n", repo.to_str().unwrap())).unwrap();
    let req = planned();
    assert_eq!(ApprovalKey::for_request(&original_request, "fixture"), ApprovalKey::for_request(&req, "fixture"));
    let before = snapshot(&repo);
    let cap = sandbox::detect();
    assert!(cap.available(), "native proof requires an enforced sandbox");
    let policy = e3_policy();
    let start = std::time::Instant::now();
    let staged = stage::stage(&req, &context(&policy, &store, &cap, table)).unwrap();
    assert_eq!(staged.exit, Some(0), "{}", String::from_utf8_lossy(&staged.stderr));
    assert!(staged.plan.refused.is_empty(), "{:?}", staged.plan);
    assert_eq!(snapshot(&repo), before, "verification must never modify original sources or environment");
    eprintln!(
        "real uv/pytest staged verification passed in {:?} including identity checks and copies; interpreter run {}ms",
        start.elapsed(),
        staged.duration_ms
    );
}

#[test]
fn a_legitimate_regenerate_applies_only_its_outputs() {
    let cap = sandbox::detect();
    let (policy, table) = (e3_policy(), CommandTable::load());
    let all = backends(&cap);
    if all.is_empty() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("tools")).unwrap();
        let req = request(d.path(), "echo x > gen/out.py\n");
        let store = approved(&req);
        let r = stage::stage(&req, &context(&policy, &store, &cap, table)).unwrap_err();
        assert!(matches!(r.stage, "stage" | "sandbox"), "{r}");
        return;
    }
    for b in all {
        let base = tempfile::tempdir().unwrap();
        let repo = base.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        template(&repo);
        world_readable(base.path());
        if matches!(b, StageBackend::UsernsOverlay | StageBackend::BwrapOverlay { .. }) {
            owned_by_nobody(&repo);
        }
        let req = request(&repo, "echo regenerated > gen/out.py\nrm -f gen/old.py\necho new > gen/new.py\n");
        let store = approved(&req);
        let ctx = context(&policy, &store, &cap, table);
        let before = snapshot(&repo);
        let staged = stage::stage_with(&req, &ctx, b.clone()).unwrap_or_else(|r| panic!("{}: {r}", b.line()));
        assert_eq!(snapshot(&repo), before, "{}: staging alone changes nothing", b.line());
        let outcome = stage::finish(&staged);
        let Outcome::Applied { changed, .. } = &outcome else {
            panic!("{}: {outcome:?}\n{}", b.line(), String::from_utf8_lossy(&staged.stderr))
        };
        let paths: Vec<&str> = changed.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, vec!["gen/new.py", "gen/old.py", "gen/out.py"], "{}", b.line());
        assert_eq!(std::fs::read_to_string(repo.join("gen/out.py")).unwrap(), "regenerated\n");
        assert!(!repo.join("gen/old.py").exists());
        let after = snapshot(&repo);
        for (k, v) in &before {
            if !in_outputs(k) {
                assert_eq!(after.get(k), Some(v), "{}: {k} changed outside the outputs", b.line());
            }
        }
        // Not approved: nothing runs.
        let other = request(&repo, "echo y > gen/out.py\n");
        let r = stage::stage_with(&other, &ctx, b.clone()).unwrap_err();
        assert_eq!(r.stage, "approval", "{r}");
        // An E3 request never runs through run::execute.
        assert!(adage_exec::run::execute(&req, &ctx).is_err());
    }
}

#[test]
fn three_refusals_revoke_the_approval() {
    let home = tempfile::tempdir().unwrap();
    let req = request(
        &{
            let d = home.path().join("repo");
            std::fs::create_dir_all(d.join("tools")).unwrap();
            d
        },
        "echo x > src/a\n",
    );
    let store = approved(&req);
    store.save(&adage_exec::approvals::path(home.path())).unwrap();
    let on = "2026-09-28T00:00:00Z";
    for n in 0..3 {
        let staged = stage::Staged {
            repo: req.repo_root.canonicalize().unwrap(),
            action: req.action.clone().unwrap(),
            exit: Some(0),
            signal: None,
            duration_ms: 1,
            pre: stage::PreState::default(),
            inputs: BTreeMap::new(),
            fields: None,
            backend: StageBackend::Copy,
            stderr: vec![],
            rewritten: vec![],
            plan: stage::Plan { refused: vec![("src/a".into(), "outside")], ..Default::default() },
        };
        let done = adage_exec::journal::finish(
            &staged,
            &adage_exec::journal::Commit {
                home: home.path(),
                repo: "fixture",
                session: "s",
                occurrence: &n.to_string(),
                approved_on: on,
                request: &req,
                origin: adage_exec::journal::RunOrigin::Manual,
            },
        );
        assert_eq!(done.revoked, n == 2, "third counted refusal revokes: {:?}", done.outcome);
    }
    let s = Store::load(&adage_exec::approvals::path(home.path())).unwrap();
    assert!(s.approvals.is_empty());
    assert_eq!(s.revoked.len(), 1);
    assert!(s.revoked[0].cause.contains("refused 3 times"));
    let recs: Vec<serde_json::Value> = adage_store::spool::read_all(&stage::spool_path(home.path())).unwrap();
    assert_eq!(recs.len(), 6, "each refusal has a prepare and a completion");
    assert_eq!(recs[5]["outcome"]["outcome"], "refused");
    assert!(!serde_json::to_string(&recs).unwrap().contains("echo"), "never the command's text or output");
}

/// Every staging mechanism this machine can run: the one [`stage::backend`] picks, and the others
/// available (a copy always; an overlay in a user namespace under Landlock; bubblewrap with
/// overlays). As root an overlay needs a fixture owned by nobody (see [`owned_by_nobody`]).
fn backends(cap: &sandbox::Capability) -> Vec<StageBackend> {
    let mut v = Vec::new();
    let chosen = stage::backend(cap);
    if chosen.available() {
        v.push(chosen);
    }
    if !cap.available() {
        return v;
    }
    let mut more = vec![StageBackend::Copy];
    if matches!(cap.backend, sandbox::Backend::Landlock { .. }) && cap.user_namespaces {
        more.push(StageBackend::UsernsOverlay);
    }
    if let Some(b) = cap.bwrap.as_ref().filter(|b| stage::bwrap_has_overlay(b)) {
        more.push(StageBackend::BwrapOverlay { path: b.clone() });
    }
    for b in more {
        if !v.contains(&b) {
            v.push(b);
        }
    }
    v
}

/// As root, sandboxed commands run as nobody, who can write through an overlay only what nobody
/// owns: give the fixture to nobody.
fn owned_by_nobody(p: &Path) {
    if !sandbox::is_root() {
        return;
    }
    std::os::unix::fs::lchown(p, Some(65534), Some(65534)).unwrap();
    if std::fs::symlink_metadata(p).unwrap().is_dir() {
        for e in std::fs::read_dir(p).unwrap().flatten() {
            owned_by_nobody(&e.path());
        }
    }
}

struct G4 {
    tally: BTreeMap<&'static str, usize>,
    outside_writes: usize,
    applied_changes: usize,
}

fn g4(backend: &StageBackend, runs: usize, seed: u64) -> G4 {
    let cap = sandbox::detect();
    let (policy, table) = (e3_policy(), CommandTable::load());
    let tpl = tempfile::tempdir().unwrap();
    template(tpl.path());
    let mut rng = Rng(seed);
    let mut out = G4 { tally: BTreeMap::new(), outside_writes: 0, applied_changes: 0 };
    let overlay = matches!(backend, StageBackend::UsernsOverlay | StageBackend::BwrapOverlay { .. });
    for n in 0..runs {
        let base = tempfile::tempdir().unwrap();
        let repo = base.path().join("repo");
        let outside = base.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        copy_tree(tpl.path(), &repo);
        world_readable(base.path());
        if overlay {
            owned_by_nobody(&repo);
        }
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o777)).unwrap();
        std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let k = 1 + rng.below(4);
        let mut steps: Vec<String> = Vec::new();
        for _ in 0..k {
            let s = STEPS[rng.below(STEPS.len())];
            steps.push(s.replace("{O}", &outside.display().to_string()).replace("{N}", &n.to_string()));
        }
        // Keep going past failures, as a hostile script would.
        let script = steps.join(" 2>/dev/null\n") + " 2>/dev/null\n";
        let req = request(&repo, &script);
        let before = snapshot(&repo);
        let listing = |d: &Path| -> Vec<String> {
            let mut v: Vec<String> =
                std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            v.sort();
            v
        };
        let parent_before = listing(base.path());
        let store = approved(&req);
        let ctx = context(&policy, &store, &cap, table);
        let outcome = match stage::stage_with(&req, &ctx, backend.clone()) {
            Ok(s) => stage::finish(&s),
            Err(r) => panic!("run {n}: {r}\n{script}"),
        };
        *out.tally.entry(outcome.name()).or_default() += 1;
        if let Outcome::Applied { changed, .. } = &outcome {
            out.applied_changes += changed.len();
        }
        let after = snapshot(&repo);
        let mut bad = Vec::new();
        let keys: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
        for k in keys {
            if !in_outputs(k) && before.get(k) != after.get(k) {
                bad.push(format!("{k}: {:?} -> {:?}", before.get(k), after.get(k)));
            }
            if in_outputs(k) && before.get(k) != after.get(k) {
                let ok = after.get(k).is_none_or(|v| v.starts_with("file ") || v.starts_with("dir "));
                if !ok {
                    bad.push(format!("{k}: applied as {:?} (only regular files are applied)", after.get(k)));
                }
            }
        }
        if std::fs::read_dir(&outside).unwrap().count() != 0 {
            bad.push("the outside directory was written".into());
        }
        let parent_after = listing(base.path());
        if parent_after != parent_before {
            bad.push(format!("the repository's parent changed: {parent_after:?}"));
        }
        if !bad.is_empty() {
            out.outside_writes += bad.len();
            eprintln!("run {n}: {outcome:?}\n  script: {script}\n  {}", bad.join("\n  "));
        }
    }
    out
}

#[test]
fn gate_g4_no_write_outside_declared_outputs_in_500_staged_runs() {
    let cap = sandbox::detect();
    let runs: usize = std::env::var("ADAGE_G4_RUNS").ok().and_then(|n| n.parse().ok()).unwrap_or(500);
    let all = backends(&cap);
    if all.is_empty() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("tools")).unwrap();
        let req = request(d.path(), "echo x > gen/out.py\n");
        let (store, policy, table) = (approved(&req), e3_policy(), CommandTable::load());
        let r = stage::stage(&req, &context(&policy, &store, &cap, table)).unwrap_err();
        assert!(matches!(r.stage, "stage" | "sandbox"), "{r}");
        eprintln!("G4: skipped, no staging backend ({})", stage::backend(&cap).line());
        return;
    }
    for (i, b) in all.iter().enumerate() {
        let r = g4(b, runs, 0x9e37_79b9_7f4a_7c15 ^ i as u64);
        eprintln!(
            "G4: {runs} staged runs, {}: {:?}; {} output change(s) applied; {} write(s) outside the declared outputs",
            b.line(),
            r.tally,
            r.applied_changes,
            r.outside_writes
        );
        assert_eq!(r.outside_writes, 0, "{}: writes outside the declared outputs", b.line());
        for o in ["applied", "refused", "failed"].iter().filter(|_| runs >= 100) {
            assert!(
                r.tally.get(o).copied().unwrap_or(0) > 0,
                "{}: no run ended {o}; the generator lost coverage",
                b.line()
            );
        }
    }
}

/// Offline compatibility gate over pinned, full upstream checkouts. This deliberately constructs
/// an authored fixture route and an in-memory fixture approval; it is not learning, a real-user
/// grant, or evidence that an agent action/inference was eliminated.
#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires both prepared pinned real repositories and offline uv dependencies"]
fn approved_project_python_runs_full_pinned_baseline_suites() {
    let root = PathBuf::from(std::env::var_os("ADAGE_TEST_REAL_PROJECT_ROOT").expect("prepared repository root"))
        .canonicalize()
        .unwrap();
    let output = PathBuf::from(std::env::var_os("ADAGE_TEST_REAL_PROJECT_RESULTS").expect("separate result directory"));
    std::fs::create_dir_all(&output).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    let table = CommandTable::load();
    let policy = e3_policy();
    let cap = sandbox::detect();
    assert!(cap.available(), "this compatibility gate requires the native production sandbox");
    let command = ".venv/bin/python -m pytest -q -p no:cacheprovider --junitxml=adage-baseline-result.xml tests";
    let mut rows = Vec::new();
    for (family, pinned) in [
        ("mm-validator", "7f0792bd7a06f72e393ec866ac1e89e1502ccfe1"),
        ("click-type", "06b2a678741131fd577ce170e23e5ca0aeba0309"),
    ] {
        let plan = manifest["families"].as_array().unwrap().iter().find(|p| p["family"] == family).unwrap();
        assert_eq!(plan["base_full"], pinned, "explicitly review a different prepared base");
        let original = root.join("bases").join(family);
        let original_before = snapshot(&original);
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().canonicalize().unwrap().join("repo");
        copy_tree(&original, &repo);
        // Fixture relocation only: production subsequently rebases this copied environment
        // once more into its stage. The original prepared repository is never modified.
        for entry in std::fs::read_dir(repo.join(".venv/lib/python3.12/site-packages")).unwrap().flatten() {
            if entry.path().extension().is_some_and(|s| s == "pth") {
                let text = std::fs::read_to_string(entry.path()).unwrap();
                std::fs::write(entry.path(), text.replace(original.to_str().unwrap(), repo.to_str().unwrap())).unwrap();
            }
        }
        let ir = adage_tracks::index::IndexedRoute {
            repo: "fixture".into(),
            track: "fixture/compatibility".into(),
            version: "0.1.0".into(),
            exec: None,
            route: adage_tracks::Route {
                id: "verify".into(),
                summary: "Offline compatibility fixture, not a learned route".into(),
                trigger: adage_tracks::Trigger::AfterEdit { paths: "src/**/*.py".into() },
                step: adage_tracks::Step::Run { command: command.into() },
                slots: vec![],
                effect: adage_tracks::io::classify(command, table),
                strength: adage_tracks::Strength::Fire,
                state: adage_tracks::RouteState::Firm,
                exit: vec![],
                evidence: Default::default(),
                authored: true,
            },
        };
        let index = adage_tracks::Index { routes: vec![ir.clone()], ..Default::default() };
        let terms = adage_exec::route::Terms { cache: vec!["adage-baseline-result.xml".into()], ..Default::default() };
        let req = adage_exec::route::plan(&ir, &index, &repo, &repo, Level::E2, Some(&terms), table).unwrap().request;
        assert!(req.scripts.is_empty());
        let store = approved(&req);
        let before = snapshot(&repo);
        let start = std::time::Instant::now();
        let result = stage::stage(&req, &context(&policy, &store, &cap, table));
        let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
        let unchanged = snapshot(&repo) == before && snapshot(&original) == original_before;
        let row = match result {
            Err(refusal) => serde_json::json!({"family":family,"pinned_base":pinned,"command":command,
                "wall_ms":wall_ms,"passed":false,"refusal":refusal.to_string(),"source_unchanged":unchanged}),
            Ok(staged) => {
                let report = staged.plan.apply.iter().find_map(|change| {
                    if change.path != "adage-baseline-result.xml" {
                        return None;
                    }
                    match &change.after {
                        stage::After::File { bytes, .. } => Some(bytes),
                        _ => None,
                    }
                });
                if let Some(bytes) = report {
                    std::fs::write(output.join(format!("{family}.xml")), bytes).unwrap();
                }
                let refused: Vec<_> = staged
                    .plan
                    .refused
                    .iter()
                    .map(|(path, reason)| serde_json::json!({"path":path,"reason":reason}))
                    .collect();
                serde_json::json!({"family":family,"pinned_base":pinned,"command":command,
                    "wall_ms":wall_ms,"child_ms":staged.duration_ms,"exit":staged.exit,"signal":staged.signal,
                    "passed":staged.exit==Some(0) && refused.is_empty() && report.is_some() && unchanged,
                    "refused_changes":refused,"stderr":String::from_utf8_lossy(&staged.stderr),
                    "junit_present":report.is_some(),"source_unchanged":unchanged,
                    "project_python":req.project_python})
            }
        };
        eprintln!("full pinned suite: {row}");
        rows.push(row);
    }
    std::fs::write(output.join("outcomes.json"), serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
    assert!(
        rows.iter().all(|row| row["passed"] == true),
        "full-suite compatibility failure; preserve outcomes and JUnit reports before changing policy"
    );
}
