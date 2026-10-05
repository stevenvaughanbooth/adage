//! Track C C5 end to end: E2 actions from hooks on an `evals/gen` finish-line sandbox (gate G4's
//! outcome verification), vendored-script packs from `adage share --vendor` to an action on
//! another checkout, and attestations from `adage share --attest` to `adage review`.
//!
//! Approvals go through the real `adage review delegate`; hook events through the real
//! `adage-exec hook claude <event>` (the binary built beside `adage`; without it, as in a
//! `cargo test -p adage-cli` that did not build it, the hook legs are skipped and say so).

#![cfg(unix)]

use adage_core::Effect;
use adage_tracks::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn have(prog: &str, arg: &str) -> bool {
    Command::new(prog).arg(arg).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn ok(o: Output) -> String {
    assert!(o.status.success(), "{}", text(&o));
    text(&o)
}

fn refused(o: Output, why: &str) -> String {
    let t = text(&o);
    assert!(!o.status.success(), "accepted (expected: {why}):\n{t}");
    assert!(t.contains(why), "expected {why:?} in:\n{t}");
    t
}

/// `chmod -R a+rX` (as root, sandboxed commands run as nobody).
fn open_up(p: &Path) {
    let m = std::fs::symlink_metadata(p).unwrap();
    if m.file_type().is_symlink() {
        return;
    }
    let mode = m.permissions().mode();
    let add = if m.is_dir() || mode & 0o100 != 0 { 0o555 } else { 0o444 };
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode | add)).unwrap();
    if m.is_dir() {
        for e in std::fs::read_dir(p).unwrap().flatten() {
            open_up(&e.path());
        }
    }
}

fn env_for(c: &mut Command, home: &Path) {
    // Exercise Apple's system Python launcher even when the developer's PATH has another Python.
    #[cfg(target_os = "macos")]
    c.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    c.env("ADAGE_HOME", home)
        .env("ADAGE_POLICY", home.join("no-managed-policy.toml"))
        .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
        .env("ADAGE_AUTO_LEARN", "0")
        .env_remove("CLAUDECODE")
        .env_remove("OPENCODE")
        .env_remove("CODEX_SANDBOX")
        .env_remove("CODEX_SANDBOX_NETWORK_DISABLED")
        .env_remove("ADAGE_ARM")
        .env_remove("SSH_AUTH_SOCK")
        .env_remove("ADAGE_DISABLE")
        .env_remove("ADAGE_EXEC_FORCE_BACKEND");
}

fn adage(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
    c.args(args).current_dir(cwd).stdin(Stdio::null());
    env_for(&mut c, home);
    c.output().unwrap()
}

/// `adage-exec`, built beside `adage` by a workspace build.
fn adage_exec() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_BIN_EXE_adage")).with_file_name("adage-exec");
    p.is_file().then_some(p)
}

fn sandbox_live() -> bool {
    adage_exec::stage::backend(&adage_exec::sandbox::detect()).available()
}

/// One hook event through `adage-exec hook claude <event>`, in the treated arm: its stdout.
fn hook_child(home: &Path, repo: &Path, event: &str, payload: Value) -> std::process::Child {
    let mut c = Command::new(adage_exec().unwrap());
    c.args(["hook", "claude", event])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    env_for(&mut c, home);
    c.env("ADAGE_ARM", "treated");
    let mut child = c.spawn().unwrap();
    child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    child
}

fn hook(home: &Path, repo: &Path, event: &str, payload: Value) -> String {
    let o = hook_child(home, repo, event, payload).wait_with_output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// A PostToolBatch in which the agent edited `rel`.
fn edited(session: &str, repo: &Path, rel: &str) -> Value {
    let file = repo.join(rel).to_string_lossy().into_owned();
    json!({
        "session_id": session, "cwd": repo, "hook_event_name": "PostToolBatch",
        "tool_calls": [{ "tool_name": "Edit", "tool_input": { "file_path": file, "old_string": "a", "new_string": "b" }, "tool_response": { "filePath": file } }]
    })
}

fn stop(session: &str, repo: &Path) -> Value {
    json!({ "session_id": session, "cwd": repo, "hook_event_name": "Stop", "stop_hook_active": false })
}

fn context(out: &str) -> String {
    let v: Value = serde_json::from_str(out).unwrap_or_else(|e| panic!("not JSON ({e}): {out}"));
    v["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_else(|| panic!("no context: {out}")).to_string()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn exec_records(home: &Path, session: &str) -> Vec<Value> {
    let recs: Vec<Value> =
        adage_store::spool::read_all(&adage_store::Paths::at(home).spool(session)).unwrap_or_default();
    recs.into_iter()
        .filter_map(|r| r.get("exec").cloned())
        .flat_map(|e| e.as_array().cloned().unwrap_or_default())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// (b) An evals/gen finish-line sandbox: `make gen` as an E2 action
// ---------------------------------------------------------------------------------------------

const REGEN: &str = "regenerate-registry";
const TRACK: &str = "tallyhouse/finish-lines";

fn finish_line_track(repo: &str) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: TRACK.into(),
                version: "0.2.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: repo.into() },
            gauge: Gauge::default(),
        },
        routes: vec![Route {
            id: REGEN.into(),
            summary: String::new(),
            trigger: Trigger::AfterEdit { paths: "registry/*.csv".into() },
            step: Step::Run { command: "make gen".into() },
            slots: vec![],
            effect: Effect::Regenerate,
            strength: Strength::Fire,
            state: RouteState::Firm,
            exit: vec![Exit::ExploratoryChange],
            evidence: Evidence { support: 7, opportunities: 8, days: 3, ..Default::default() },
            authored: false,
        }],
    }
}

fn add_row(csv: &Path, row: &str) {
    let mut rows = std::fs::read_to_string(csv).unwrap();
    rows.push_str(row);
    rows.push('\n');
    std::fs::write(csv, rows).unwrap();
}

#[test]
fn native_event_redelivery_and_recovery_use_the_real_hook() {
    if adage_exec().is_none() || !sandbox_live() {
        eprintln!("skipped: built executor and live staging backend required");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let home = base.join("adage");
    let repo = base.join("repo");
    for path in [home.join("exec"), repo.join("registry"), repo.join("gen")] {
        std::fs::create_dir_all(path).unwrap();
    }
    std::fs::write(repo.join("registry/items.csv"), "one\n").unwrap();
    std::fs::write(repo.join("gen/out"), "").unwrap();
    std::fs::write(repo.join("Makefile"), "gen:\n\tprintf x >> gen/out\n.PHONY: gen\n").unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "."]);
    git(&repo, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.com", "commit", "-qm", "fixture"]);
    open_up(&base);
    let key = adage_core::repo::repo_key(&repo);
    Index::compile(&[finish_line_track(&key)]).write_atomic(&home.join("index.json")).unwrap();
    std::fs::write(home.join("policy.toml"), "[exec]\nmax_level = \"E2\"\n").unwrap();
    ok(adage(&home, &repo, &["review", "delegate", TRACK, REGEN, "--level", "e2", "--outputs", "gen/**"]));
    let mut payload = edited("native-identity", &repo, "registry/items.csv");
    payload["tool_calls"][0]["tool_use_id"] = json!("toolu_first");
    for _ in 0..2 {
        let out = hook(&home, &repo, "PostToolBatch", payload.clone());
        assert!(out.contains("Adage ran"), "{out}");
        assert_eq!(std::fs::read_to_string(repo.join("gen/out")).unwrap(), "x");
        assert_eq!(adage_exec::journal::list(&home).unwrap().len(), 1);
    }
    // Identical content with a new native ID is a new occurrence, not a duplicate.
    payload["tool_calls"][0]["tool_use_id"] = json!("toolu_later");
    let out = hook(&home, &repo, "PostToolBatch", payload.clone());
    assert!(out.contains("Adage ran"), "{out}");
    assert_eq!(std::fs::read_to_string(repo.join("gen/out")).unwrap(), "xx");
    let mut receipts = adage_exec::journal::list(&home).unwrap();
    assert_eq!(receipts.len(), 2);

    // A durable reservation without completion blocks retry advice even after grant removal.
    let mut pending = receipts.pop().unwrap();
    pending.id = "interrupted-fixture".into();
    pending.phase = adage_exec::journal::Phase::Prepared;
    pending.outcome = adage_exec::stage::Outcome::Unknown { reason: "fixture interrupted".into() };
    adage_store::spool::append_durable(&adage_exec::stage::spool_path(&home), &pending).unwrap();
    std::fs::remove_file(home.join("approvals.json")).unwrap();
    payload["tool_calls"][0]["tool_use_id"] = json!("toolu_recovery");
    let out = hook(&home, &repo, "PostToolBatch", payload.clone());
    assert!(out.contains("needs recovery") && out.contains("do not retry automatically"), "{out}");
    assert!(!out.contains("must be followed by running"), "{out}");
    assert_eq!(std::fs::read_to_string(repo.join("gen/out")).unwrap(), "xx");

    // Corruption is also uncertainty, never an empty journal or plain retry instruction.
    std::fs::OpenOptions::new()
        .append(true)
        .open(adage_exec::stage::spool_path(&home))
        .unwrap()
        .write_all(b"torn")
        .unwrap();
    let out = hook(&home, &repo, "PostToolBatch", payload);
    assert!(out.contains("cannot verify") && out.contains("do not retry automatically"), "{out}");
    assert!(!out.contains("must be followed by running"), "{out}");
    assert_eq!(std::fs::read_to_string(repo.join("gen/out")).unwrap(), "xx");
}

#[test]
fn finish_line_sandbox_make_gen_as_an_e2_action_from_the_hook() {
    if !(have("python3", "--version") && have("make", "--version") && have("git", "--version")) {
        eprintln!("skipped: python3, make and git are needed to generate the sandbox");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let (home, repo) = (base.join("adage"), base.join("repo"));
    std::fs::create_dir_all(home.join("exec")).unwrap();
    let gen = Command::new("python3")
        .arg(repo_root().join("evals/gen/gen.py"))
        .arg("--spec")
        .arg(repo_root().join("evals/gen/specs/example.json"))
        .args(["--seed", "7", "--repo-only"])
        .arg(&repo)
        .output()
        .unwrap();
    assert!(gen.status.success(), "{}", text(&gen));
    open_up(&base);
    let generated = git(&repo, &["ls-files", "*/generated/registry.py"]).trim().to_string();
    assert!(!generated.is_empty(), "the sandbox has a generated registry module");
    let key = adage_core::repo::repo_key(&repo);
    Index::compile(&[finish_line_track(&key)]).write_atomic(&home.join("index.json")).unwrap();
    let allow = ["review", "delegate", TRACK, REGEN, "--level", "e2", "--outputs", generated.as_str()];

    // E2 is a beta: the default policy caps execution at E1.
    refused(adage(&home, &repo, &allow), "max_level = \"E2\"");
    std::fs::write(home.join("policy.toml"), "[exec]\nmax_level = \"E2\"\n").unwrap();
    // A regenerate action declares its outputs.
    refused(adage(&home, &repo, &allow[..6]), "declares the outputs");
    if !sandbox_live() {
        refused(adage(&home, &repo, &allow), "nothing was approved");
        eprintln!("skipped the rest: no staging backend here");
        return;
    }

    // A stale registry: the preview shows the full diff of the declared output, and the approval
    // is recorded. An approval whose outputs do not cover the change records nothing.
    let csv = repo.join("registry/currencies.csv");
    add_row(&csv, "ZZQ,Adage Test Unit,3");
    let wrong = ["review", "delegate", TRACK, REGEN, "--level", "e2", "--outputs", "docs/*.md"];
    refused(adage(&home, &repo, &wrong), "the approval would not cover");
    assert!(!home.join("approvals.json").exists());
    let out = ok(adage(&home, &repo, &allow));
    for want in [
        "level       E2",
        "argv        make gen",
        "effect      regenerate, approved (the classifier says regenerate)",
        "runs        from the hook, when an edit matches registry/*.csv",
        "action      regenerate; staged in a writable overlay",
        &format!("outputs     {generated} (only these may change"),
        &format!("diff        {generated} (Modified)"),
        "+         'ZZQ': {'minor_units': 3, 'name': 'Adage Test Unit'},",
        "recorded    ",
    ] {
        assert!(out.contains(want), "delegate output lacks {want:?}:\n{out}");
    }
    assert!(!std::fs::read_to_string(repo.join(&generated)).unwrap().contains("ZZQ"), "the preview applies nothing");
    let store = adage_exec::approvals::Store::load(&home.join("approvals.json")).unwrap();
    assert_eq!(store.approvals.len(), 1);
    assert!(!store.approvals[0].auto, "an action is never approved automatically");

    let Some(_) = adage_exec() else {
        eprintln!("skipped the hook legs: adage-exec is not built beside adage (cargo test --workspace builds it)");
        return;
    };

    // The agent edits the registry: the hook regenerates exactly the declared output, says so,
    // and the finish line is met, so Stop asks for nothing.
    assert_eq!(git(&repo, &["status", "--porcelain"]).trim(), "M registry/currencies.csv");
    let out = hook(&home, &repo, "PostToolBatch", edited("s1", &repo, "registry/currencies.csv"));
    let ctx = context(&out);
    assert!(
        ctx.contains(&format!(
            "Adage ran `make gen` after your edit to registry/currencies.csv: updated 1 file ({generated}). Nothing else changed."
        )),
        "{ctx}"
    );
    assert!(ctx.contains("exec=\"e2\""), "the wrapper marks the session steered: {ctx}");
    assert!(std::fs::read_to_string(repo.join(&generated)).unwrap().contains("'ZZQ'"));
    let status = git(&repo, &["status", "--porcelain"]);
    let mut changed: Vec<&str> = status.lines().map(|l| l[3..].trim()).collect();
    changed.sort();
    let mut want = vec!["registry/currencies.csv", generated.as_str()];
    want.sort();
    assert_eq!(changed, want, "nothing else changed:\n{status}");
    let check = Command::new("python3").args(["tools/gen.py", "--check"]).current_dir(&repo).output().unwrap();
    assert!(check.status.success(), "outcome verification: {}", text(&check));
    assert_eq!(hook(&home, &repo, "Stop", stop("s1", &repo)), "", "the obligation is met: no Stop request");
    let recs = exec_records(&home, "s1");
    assert_eq!(recs[0]["outcome"], "applied", "{recs:?}");
    assert!(!serde_json::to_string(&recs).unwrap().contains("ZZQ"), "the spool never holds output");

    // A concurrent edit of the output while the action runs: refused, the agent's edit kept, the
    // plain directive and the reason delivered instead. At Stop the action runs again and applies.
    let gen_py = repo.join("tools/gen.py");
    let original = std::fs::read_to_string(&gen_py).unwrap();
    std::fs::write(&gen_py, original.replacen("import sys\n", "import sys\nimport time\ntime.sleep(2)\n", 1)).unwrap();
    open_up(&repo);
    add_row(&csv, "ZZR,Second Test Unit,1");
    let child = hook_child(&home, &repo, "PostToolBatch", edited("s2", &repo, "registry/currencies.csv"));
    std::thread::sleep(std::time::Duration::from_millis(1000));
    std::fs::write(repo.join(&generated), "# the agent's own edit\n").unwrap();
    let out = String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).trim().to_string();
    let ctx = context(&out);
    assert!(ctx.contains("must be followed by running:\n  make gen"), "the E0 directive: {ctx}");
    assert!(
        ctx.contains("Adage did not run it:") && ctx.contains("changed in the worktree while the action ran"),
        "{ctx}"
    );
    assert!(!ctx.contains("exec=\"e2\""), "{ctx}");
    assert_eq!(std::fs::read_to_string(repo.join(&generated)).unwrap(), "# the agent's own edit\n");
    assert_eq!(hook(&home, &repo, "Stop", stop("s2", &repo)), "", "applied at Stop: the request is withdrawn");
    assert!(std::fs::read_to_string(repo.join(&generated)).unwrap().contains("'ZZR'"));
    let outcomes: Vec<Value> = exec_records(&home, "s2").iter().filter_map(|r| r.get("outcome").cloned()).collect();
    assert_eq!(outcomes, vec![json!("refused"), json!("applied")]);

    // A change outside the declared outputs refuses the application; three such refusals revoke
    // the approval, and `adage review` lists it as exec-revoked.
    std::fs::write(
        &gen_py,
        original.replace(
            "target.write_text(text)",
            "target.write_text(text); (root / 'README.md').write_text('rewritten')",
        ),
    )
    .unwrap();
    open_up(&repo);
    let readme = std::fs::read_to_string(repo.join("README.md")).unwrap();
    for i in 1..=3 {
        add_row(&csv, &format!("ZZ{},Unit {i},1", ["A", "B", "C"][i - 1]));
        let ctx = context(&hook(&home, &repo, "PostToolBatch", edited("s3", &repo, "registry/currencies.csv")));
        assert!(
            ctx.contains("Adage did not run it:") && ctx.contains("README.md (outside the declared outputs)"),
            "call {i}: {ctx}"
        );
        assert_eq!(ctx.contains("its approval is now revoked"), i == 3, "call {i}: {ctx}");
        assert_eq!(std::fs::read_to_string(repo.join("README.md")).unwrap(), readme, "never applied");
    }
    let store = adage_exec::approvals::Store::load(&home.join("approvals.json")).unwrap();
    assert!(store.approvals.is_empty(), "revoked after three refusals");
    let out = ok(adage(&home, &repo, &["review"]));
    assert!(out.contains("exec-revoked: its E2 action was refused 3 times"), "{out}");
    // Revoked: the next edit gets the plain directive's obligation back, and Stop asks again.
    add_row(&csv, "ZZD,Unit 4,1");
    assert_eq!(hook(&home, &repo, "PostToolBatch", edited("s3", &repo, "registry/currencies.csv")), "");
    let out = hook(&home, &repo, "Stop", stop("s3", &repo));
    assert!(out.contains("Adage finish-line check") && out.contains("\"decision\":\"block\""), "{out}");
}

// ---------------------------------------------------------------------------------------------
// (c) Vendored scripts: share --vendor → add on another checkout → approve → run from the hook
// (d) Attestations: share --attest → add → review
// ---------------------------------------------------------------------------------------------

const COUNT: &str = "import pathlib\nn = sum(len(p.read_text().splitlines()) for p in sorted(pathlib.Path('data').glob('*.txt')))\npathlib.Path('out').mkdir(exist_ok=True)\npathlib.Path('out/count.txt').write_text(f'{n}\\n')\n";
const PACK: &str = "acme/billing";

fn keygen(dir: &Path) -> ssh_key::PublicKey {
    let mut k = ssh_key::PrivateKey::random(&mut rand_core::OsRng, ssh_key::Algorithm::Ed25519).unwrap();
    k.set_comment("alice@example");
    std::fs::write(dir.join("id_ed25519"), k.to_openssh(ssh_key::LineEnding::LF).unwrap().as_bytes()).unwrap();
    k.public_key().clone()
}

fn checkout(dir: &Path) {
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::write(dir.join(".git/config"), "[remote \"origin\"]\n\turl = git@github.com:acme/billing.git\n").unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("data/a.txt"), "one\ntwo\n").unwrap();
}

/// An authored pack whose finish line runs a repository script (L-ref) as an E2 action.
fn authored_pack(dir: &Path) {
    let sha = adage_pack::digest::sha256_hex(COUNT.as_bytes());
    let mut f: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    f.insert(
        "pack.toml".into(),
        format!(
            r#"format = "adage-pack/1"

[pack]
name = "{PACK}"
version = "1.0.0"
origin = "reflex"
description = "Keeps the line count current"

[applicability]
kind = "repo"
repo_keys = ["github.com/acme/billing"]

[requires]
tools = [{{ name = "python3" }}]
templates = ["obligation@1"]
repo_files = [{{ path = "scripts/count.py", sha256 = "{sha}" }}]

[exec]
requested = "E2"
"#
        )
        .into_bytes(),
    );
    f.insert(
        "routes/count-lines.yaml".into(),
        format!(
            "id: count-lines\nshape: obligation\ntrigger: {{ on: after_edit, paths: \"data/*.txt\" }}\nsteps:\n  - id: count\n    run: {{ argv: [python3, scripts/count.py] }}\n    effect: regenerate\n    script: {{ path: scripts/count.py, closure: ref, sha256: \"{sha}\" }}\nstrength: prime\nexec: {{ level: E2, outputs: [\"out/count.txt\"] }}\nevidence: {{ opportunities: 6, support: 5, days: 2 }}\n"
        )
        .into_bytes(),
    );
    let p = adage_pack::validate::parse(&f).unwrap();
    adage_pack::validate::validate(&p).unwrap();
    f.insert("CLOSURE".into(), adage_pack::closure::compute(&p).unwrap().render().into_bytes());
    for (rel, bytes) in &f {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}

/// A bench results file with one arms record (schema v2) and a trial line that is not attested.
fn results(path: &Path, extra: Option<(&str, Value)>) {
    let mut arms = json!({
        "schema_version": 2, "record": "arms", "run_id": "0123456789abcdef", "tier": "l2",
        "suite": "gen-smoke", "adage_version": "0.5.0",
        "params": { "runtime": {}, "learn": {} }, "params_hash": "ab".repeat(32),
        "split": { "kind": "suite", "name": "all", "seed": null }, "exec_level": "e2",
        "arms": { "e0": { "sessions": 40 }, "e2": { "sessions": 40 } },
        "deltas": { "turns_saved": 0.62 },
        "harm": { "canary_actions": 0 }
    });
    if let Some((k, v)) = extra {
        arms[k] = v;
    }
    let trial = json!({ "schema_version": 1, "record": "trial" });
    std::fs::write(path, format!("{arms}\n{trial}\n")).unwrap();
}

#[test]
fn vendored_scripts_and_attestations_round_trip() {
    if !have("python3", "--version") {
        eprintln!("skipped: python3 is needed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    for d in ["sender", "recipient/exec", "keys"] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    let (sender_repo, recipient_repo) = (base.join("sender-checkout"), base.join("recipient-checkout"));
    checkout(&sender_repo);
    checkout(&recipient_repo);
    std::fs::create_dir_all(sender_repo.join("scripts")).unwrap();
    std::fs::write(sender_repo.join("scripts/count.py"), COUNT).unwrap();
    let pk = keygen(&base.join("keys"));
    let key = base.join("keys/id_ed25519");
    authored_pack(&base.join("authored"));
    let (sender, recipient) = (base.join("sender"), base.join("recipient"));
    let share = |extra: &[&str], out: &str| {
        let authored = base.join("authored");
        let out = base.join(out);
        let mut args =
            vec!["share", authored.to_str().unwrap(), "--out", out.to_str().unwrap(), "--key", key.to_str().unwrap()];
        args.extend(extra);
        adage(&sender, &sender_repo, &args)
    };

    // A hostile attestation is refused at share time.
    let bad = base.join("bad.jsonl");
    results(&bad, Some(("grants", json!("fire"))));
    refused(share(&["--vendor", "--attest", bad.to_str().unwrap()], "x"), "is not a bench result record");

    // share --vendor --attest, in the checkout that holds the script.
    let good = base.join("gen-smoke.jsonl");
    results(&good, None);
    let out = ok(share(&["--vendor", "--attest", good.to_str().unwrap()], "out"));
    let sha = adage_pack::digest::sha256_hex(COUNT.as_bytes());
    assert!(out.contains("scripts/count.py (reflex count-lines)"), "{out}");
    assert!(out.contains("gen-smoke-1"), "{out}");
    let pack = adage_pack::Pack::load(&base.join("out")).unwrap();
    assert_eq!(pack.scripts.keys().collect::<Vec<_>>(), vec![&sha]);
    assert!(pack.manifest.requires.repo_files.is_empty(), "no repository file is required any more");
    assert_eq!(pack.attestations.len(), 1, "only the arms record is attested");

    // add, on a checkout that does not have the script: the bytes travel.
    std::fs::write(
        recipient.join("trusted-signers"),
        format!("alice@example namespaces=\"adage-pack\" {}\n", adage_sign::key_line(&pk)),
    )
    .unwrap();
    let out = ok(adage(&recipient, &recipient_repo, &["add", base.join("out").to_str().unwrap()]));
    assert!(out.contains("1 vendored script "), "{out}");
    let cas = recipient.join("cas").join(&sha);
    assert_eq!(std::fs::read_to_string(&cas).unwrap(), COUNT);
    assert_eq!(std::fs::metadata(&cas).unwrap().permissions().mode() & 0o777, 0o444, "read-only");
    let index = Index::load(&recipient.join("index.json"));
    let ir = index.routes.iter().find(|r| r.route.id == "count-lines").expect("live in the index");
    assert!(ir.exec_only(), "E0 never directs a vendored script");
    assert_eq!(ir.route.strength, Strength::Prime, "an attestation grants no strength");
    assert_eq!(ir.exec.as_ref().unwrap().max_level, adage_tracks::index::Level::E2);

    // (d) review shows the attestation as self-reported by the signer.
    let fp = adage_sign::fingerprint(&pk);
    let out = ok(adage(&recipient, &recipient_repo, &["review"]));
    assert!(out.contains("acme/billing carries 1 attestation, self-reported by its signer"), "{out}");
    assert!(out.contains(&format!("self-reported by signer {fp}")), "{out}");
    assert!(out.contains("arms record · tier l2 · suite gen-smoke · run 0123456789abcdef"), "{out}");
    assert!(out.contains("deltas turns_saved +0.620 · harm canary_actions 0"), "{out}");
    assert!(out.contains("never set a reflex's strength or an execution level"), "{out}");
    let o = adage(&recipient, &recipient_repo, &["review", "--json"]);
    assert!(o.status.success());
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["attestations"][0]["pack"], PACK);
    assert_eq!(v["attestations"][0]["note"], "self-reported by signer; informational only");

    // Approval: never automatic; policy must allow vendored scripts; the full text is shown.
    let allow = ["review", "delegate", PACK, "count-lines", "--level", "e2", "--effect", "regenerate"];
    std::fs::write(recipient.join("policy.toml"), "[exec]\nmax_level = \"E2\"\n").unwrap();
    refused(adage(&recipient, &recipient_repo, &allow), "allow_vendor_scripts");
    std::fs::write(recipient.join("policy.toml"), "[exec]\nmax_level = \"E2\"\nallow_vendor_scripts = true\n").unwrap();
    open_up(&base);
    if !sandbox_live() {
        refused(adage(&recipient, &recipient_repo, &allow), "nothing was approved");
        eprintln!("skipped the run: no staging backend here");
        return;
    }
    let out = ok(adage(&recipient, &recipient_repo, &allow));
    for want in [
        "argv        python3 scripts/sha256-",
        &format!("script      vendored (L-vendor) sha256 {sha}"),
        "its shebang is ignored); never approved automatically. Its full text:",
        "    | import pathlib",
        "    | pathlib.Path('out/count.txt').write_text(f'{n}\\n')",
        "automatic   no (",
        "the route runs scripts (L-ref or L-vendor)",
        "attested    gen-smoke-1: arms record · tier l2",
        "A vendored script is code from the pack's signer",
        "diff        out/count.txt (Created)",
        "    + 2",
    ] {
        assert!(out.contains(want), "delegate output lacks {want:?}:\n{out}");
    }
    assert!(!recipient_repo.join("out").exists(), "the preview applies nothing");

    let Some(_) = adage_exec() else {
        eprintln!("skipped the hook leg: adage-exec is not built beside adage");
        return;
    };
    // Run: the edit fires the action, which runs the CAS bytes with the interpreter in argv.
    std::fs::write(recipient_repo.join("data/a.txt"), "one\ntwo\nthree\n").unwrap();
    open_up(&recipient_repo);
    let ctx = context(&hook(&recipient, &recipient_repo, "PostToolBatch", edited("v1", &recipient_repo, "data/a.txt")));
    assert!(
        ctx.contains("after your edit to data/a.txt: updated 1 file (out/count.txt). Nothing else changed."),
        "{ctx}"
    );
    assert_eq!(std::fs::read_to_string(recipient_repo.join("out/count.txt")).unwrap(), "3\n");
    // A tampered CAS entry no longer runs: the reason is delivered, nothing changes.
    std::fs::set_permissions(&cas, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::write(&cas, "import pathlib\npathlib.Path('out/count.txt').write_text('99\\n')\n").unwrap();
    std::fs::write(recipient_repo.join("data/a.txt"), "one\n").unwrap();
    let ctx = context(&hook(&recipient, &recipient_repo, "PostToolBatch", edited("v2", &recipient_repo, "data/a.txt")));
    assert!(ctx.contains("does not match its hash"), "{ctx}");
    assert_eq!(std::fs::read_to_string(recipient_repo.join("out/count.txt")).unwrap(), "3\n");
    // The agent cannot run a vendored script itself, so Stop does not ask it to.
    assert_eq!(hook(&recipient, &recipient_repo, "Stop", stop("v2", &recipient_repo)), "");
}
