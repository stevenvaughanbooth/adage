//! Binding instincts against fixture repositories: what binds, what never may, and that the
//! result is deterministic and valid as a track.

use adage_core::Effect;
use adage_instincts::{bind, bind_with, BindOptions, BindResult};
use adage_tracks::{
    Evidence, Exit, Gauge, Index, Manifest, Route, RouteState, Scope, Step, Strength, Track, TrackInfo, Trigger,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A fixture repository by name. `acme` is the shared acme-billing sandbox
/// (`evals/sandboxes/acme-billing`), the one copy every crate's tests read.
fn fixture(name: &str) -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    match name {
        "acme" => here.join("../../evals/sandboxes/acme-billing"),
        _ => here.join("tests/fixtures").join(name),
    }
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

/// Whether any route directs `command` after `key` fails, whatever the signature.
fn has_any(r: &BindResult, key: &str, command: &str) -> bool {
    r.runs().iter().any(|x| {
        matches!(&x.trigger, Trigger::AfterFailure { command: k, .. } if k == key)
            && x.step == Step::Run { command: command.into() }
    })
}

#[test]
fn acme_recovers_its_dead_end_and_finish_line_on_day_zero() {
    let r = bound("acme");
    // U1: `python3 -m unittest` fails without ACME_ENV; `make test` sets it. The id is the one the
    // learner derived for this working command in the demo (adage-eval gates), so the
    // learned route replaces the instinct as soon as it exists.
    let d = route(&r, "dead-end-d41b1e");
    assert_eq!(
        d.trigger,
        Trigger::AfterFailure { command: "exec python3 -m unittest".into(), signature: "ACME_ENV".into() }
    );
    assert_eq!(d.step, Step::Run { command: "make test".into() });
    assert_eq!(r.origins["dead-end-d41b1e"], "U1");
    // U3: the other runners an agent tries first (round 1 of the demo: pytest, 1.44 times a session),
    // on the wrapper's signature or the repository's message naming `make test`, never on any
    // failure.
    assert!(has(&r, "fail exec pytest [ACME_ENV]", "make test"), "{:#?}", shape(&r));
    assert!(has(&r, "fail exec pytest [make test]", "make test"));
    assert!(has(&r, "fail exec python3 -m pytest [ACME_ENV]", "make test"));
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
    // A missing loader may also fail in the repository's own words, naming the wrapper; `npm test`
    // is not bound on that (npm echoes its scripts).
    assert!(has(&r, "fail exec node --test [npm run check]", "npm run check"));
    assert!(!has(&r, "fail exec npm test [npm run check]", "npm run check"));
    assert_eq!(r.runs().len(), 4, "{:#?}", shape(&r));
    // The failing `npm test` is never itself directed.
    assert!(!shape(&r).iter().any(|(_, _, c)| c == "npm test"));
}

#[test]
fn pnpm_repository_gets_package_manager_discipline() {
    let r = bound("pnpm");
    let install: Vec<&Route> =
        r.routes.iter().filter(|x| x.step == Step::Run { command: "pnpm install".into() }).collect();
    for x in &install {
        // An environment step waits for a person (Design O11).
        assert_eq!((x.effect, x.state), (Effect::Environment, RouteState::Nascent));
    }
    // Each on npm's (or yarn's) own refusal, never on any failure: `workspace:*` in the manifest,
    // `npm ci` with no package-lock.json, yarn missing or refused by corepack.
    assert!(has(&r, "fail exec npm install [Unsupported URL Type]", "pnpm install"), "{:#?}", shape(&r));
    assert!(has(&r, "fail exec npm ci [Unsupported URL Type]", "pnpm install"));
    assert!(has(&r, "fail exec npm ci [an existing package-lock.json]", "pnpm install"));
    assert!(has(&r, "fail exec yarn install [configured to use pnpm]", "pnpm install"));
    assert!(has(&r, "fail exec yarn test [yarn: command not found]", "pnpm test"));
    assert_eq!(install.len(), 6, "{:#?}", shape(&r));
    // `npm test` runs the same `vitest run` as `pnpm test` and fails the same way: nothing binds
    // (RepoBench: vue-core's N1 fired on genuine test failures).
    assert!(!has_any(&r, "exec npm test", "pnpm test"), "{:#?}", shape(&r));
    assert!(r.skipped.iter().any(|(i, w)| i == "N1" && w.starts_with("npm test runs the same script")));
    assert_eq!(route(&r, "dead-end-68b3e9").state, RouteState::Forming);
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
    assert_eq!(gen.slots["path"], "strength/strength_string.go");
    assert_eq!(gen.slots["command"], "go generate ./...");
    assert!(r.advisories().iter().any(|a| a.instinct == "U6" && a.slots["path"] == "go.sum"));
    let facts = adage_instincts::extract(&fixture("go"));
    let go = facts.go.unwrap();
    assert_eq!(go.module, "example.com/signals");
    assert_eq!(go.generate, vec![("strength/strength.go".to_string(), "stringer -type=Strength".to_string())]);
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
fn tox_wraps_pytest_when_its_test_environment_runs_plain_commands() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("pyproject.toml"), "[tool.pytest.ini_options]\ntestpaths=['t']\n[tool.uv]\n").unwrap();
    std::fs::write(d.path().join("tox.ini"), "[tox]\nenvlist = py312\n[testenv]\ncommands = pytest {posargs}\n")
        .unwrap();
    std::fs::create_dir_all(d.path().join("src/tidal")).unwrap();
    std::fs::write(d.path().join("src/tidal/__init__.py"), "").unwrap();
    let r = bind(d.path(), &[], &BTreeSet::new());
    // No dependency group has pytest, so `uv run pytest` would not find it: U11 says why.
    assert!(
        r.skipped.iter().any(|(i, w)| i == "U11" && w.contains("no dependency group has pytest")),
        "{:?}",
        r.skipped
    );
    // P1: tox is the declared wrapper, on the signatures of a missing environment only.
    for t in [
        "fail exec pytest [pytest: command not found]",
        "fail exec pytest [command not found: pytest]",
        "fail exec python -m pytest [No module named pytest]",
        "fail exec python3 -m pytest [No module named 'tidal']",
    ] {
        assert!(has(&r, t, "tox -e py"), "{t}: {:#?}", shape(&r));
    }
    assert!(r.origins.values().any(|o| o == "P1"));
    // U3: the one declared test entry point, `tox -e py`, where the output names it.
    assert!(has(&r, "fail exec pytest [tox -e py]", "tox -e py"), "{:#?}", shape(&r));
    assert!(r.runs().iter().all(|x| x.step == Step::Run { command: "tox -e py".into() }));

    // Factor-conditional commands (`tests: pytest`) are not what `tox -e py` runs: reported.
    std::fs::write(
        d.path().join("tox.ini"),
        "[tox]\nenv_list = py3{11,12}-tests\n[testenv]\ncommands =\n    tests: pytest\n",
    )
    .unwrap();
    let r = bind(d.path(), &[], &BTreeSet::new());
    assert!(r.runs().is_empty(), "{:#?}", shape(&r));
    assert!(r.skipped.iter().any(|(i, w)| i == "P1" && w.contains("factors")), "{:?}", r.skipped);

    // nox sessions are code: reported, never bound.
    std::fs::remove_file(d.path().join("tox.ini")).unwrap();
    std::fs::write(
        d.path().join("noxfile.py"),
        "import nox\n\n@nox.session\ndef tests(session):\n    session.run('pytest')\n",
    )
    .unwrap();
    let r = bind(d.path(), &[], &BTreeSet::new());
    assert!(r.runs().is_empty(), "{:#?}", shape(&r));
    assert!(r.skipped.iter().any(|(i, w)| i == "P1" && w.contains("nox")), "{:?}", r.skipped);
}

#[test]
fn the_project_environment_answers_a_runner_that_ran_outside_it() {
    // RepoBench's Python sandboxes: a `.venv` made by `uv venv`, not activated; tox and uv declared.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("pyproject.toml"),
        "[project]\nname = 'click'\n[dependency-groups]\ntests = ['pytest']\ndev = ['tox', {include-group = 'tests'}]\n[tool.pytest.ini_options]\ntestpaths = ['tests']\n",
    )
    .unwrap();
    std::fs::write(d.path().join("uv.lock"), "version = 1\n").unwrap();
    std::fs::write(
        d.path().join("tox.ini"),
        "[tox]\nenv_list = py3{11,12}-tests\n[testenv]\ncommands =\n    tests: pytest\n",
    )
    .unwrap();
    std::fs::create_dir_all(d.path().join("src/click")).unwrap();
    std::fs::write(d.path().join("src/click/__init__.py"), "").unwrap();
    std::fs::create_dir_all(d.path().join("tests")).unwrap();
    std::fs::write(d.path().join("tests/test_a.py"), "import click\n").unwrap();

    // Without the virtual environment: `uv run pytest` (pytest is in the default group, `dev`).
    let r = bind(d.path(), &[], &BTreeSet::new());
    assert!(has(&r, "fail exec pytest [pytest: command not found]", "uv run pytest"), "{:#?}", shape(&r));
    assert!(has(&r, "fail exec python3 -m pytest [No module named 'click']", "uv run pytest"));
    assert!(has(&r, "fail exec tox [tox: command not found]", "uv run pytest"));

    // With it: its own interpreter, which needs no network and changes nothing.
    std::fs::create_dir_all(d.path().join(".venv/bin")).unwrap();
    std::fs::write(d.path().join(".venv/pyvenv.cfg"), "home = /usr/bin\n").unwrap();
    let r = bind(d.path(), &[], &BTreeSet::new());
    for t in [
        "fail exec pytest [pytest: command not found]",
        "fail exec pytest [command not found: pytest]",
        "fail exec pytest [No module named 'click']",
        "fail exec python -m pytest [No module named pytest]",
        "fail exec python -m pytest [python: command not found]",
        "fail exec python3 -m pytest [No module named pytest]",
        "fail exec python3 -m pytest [No module named 'click']",
        "fail exec tox [tox: command not found]",
    ] {
        assert!(has(&r, t, ".venv/bin/python -m pytest"), "{t}: {:#?}", shape(&r));
    }
    for x in r.runs() {
        assert_eq!(r.origins[&x.id], "U11");
        assert_eq!((x.effect, x.state), (Effect::Verify, RouteState::Forming));
        let adage_tracks::Origin::Instinct { declared, .. } = x.origin() else { panic!("{:?}", x.origin()) };
        assert_eq!(declared.as_deref(), Some(".venv/pyvenv.cfg"));
    }
    // Launch report, bug 5: `python -c "import click"` and `python repro.py` failing with `No
    // module named 'click'` (4 of 6 zero-config sessions began so) got nothing. Any Python run
    // that cannot import the project's package or a declared test dependency is answered: advice
    // naming the project's interpreter.
    let advice: Vec<(String, String, String)> = r
        .advisories()
        .iter()
        .filter(|a| a.template_id == "project-python")
        .map(|a| {
            let Trigger::AfterFailure { command, signature } = a.trigger else { panic!("{:?}", a.trigger) };
            (command.clone(), signature.clone(), a.slots["interpreter"].clone())
        })
        .collect();
    for (probe, module) in [("python", "click"), ("python3", "click"), ("python", "pytest"), ("python3", "tox")] {
        let want = (format!("exec {probe}"), format!("No module named '{module}'"), ".venv/bin/python".to_string());
        assert!(advice.contains(&want), "{want:?} in {advice:#?}");
    }
    assert!(r.advisories().iter().all(|a| a.text().is_some()));
    // The key keeps the interpreter's path, so the learner's id for this working command matches.
    let key = adage_instincts::safety::command_key(".venv/bin/python -m pytest", adage_core::CommandTable::load());
    assert_eq!(key.as_deref(), Some("exec .venv/bin/python -m pytest"));

    // The uv group that has pytest is named when the default groups lack it.
    std::fs::remove_dir_all(d.path().join(".venv")).unwrap();
    std::fs::write(
        d.path().join("pyproject.toml"),
        "[project]\nname = 'click'\n[dependency-groups]\ndev = ['ruff']\ntests = ['pytest-xdist', 'pytest>=8']\n[tool.pytest.ini_options]\n",
    )
    .unwrap();
    let r = bind(d.path(), &[], &BTreeSet::new());
    assert!(has(&r, "fail exec pytest [pytest: command not found]", "uv run --group tests pytest"), "{:#?}", shape(&r));
}

/// Output of runs that failed on their assertions, or on a bug in the code under test: no instinct
/// may fire on any of them (RepoBench: N1 fired on vitest failures in vue-core).
const GENUINE_FAILURES: [&str; 6] = [
    "FAILED tests/test_formatting.py::test_removing_multiline_marker - AssertionError: assert '\\x08' not in 'Usage: cli'\n1 failed, 12 passed in 0.31s",
    "E   ModuleNotFoundError: No module named 'click.nonexistent'\nERROR collecting tests/test_x.py",
    " FAIL  packages/runtime-core/__tests__/apiWatch.spec.ts > watch > basic\nAssertionError: expected 1 to be 2\n Test Files  1 failed (1)",
    "--- FAIL: TestCommand_Run (0.00s)\n    command_test.go:42: expected true\nFAIL\nexit status 1\nFAIL\tgithub.com/urfave/cli/v3\t0.4s",
    "FAIL: test_invoice_total (tests.test_billing.TestBilling)\nAssertionError: 3 != 4\nFAILED (failures=1)",
    "npm error Lifecycle script `test` failed with error:\nnpm error code 1\nnpm error command failed\nnpm error command sh -c vitest run",
];

#[test]
fn no_instinct_fires_on_a_genuine_test_failure() {
    let mut routes: Vec<Route> = Vec::new();
    for name in FIXTURES {
        routes.extend(bound(name).routes);
    }
    let mut failures = 0;
    for x in &routes {
        let Trigger::AfterFailure { signature, .. } = &x.trigger else { continue };
        failures += 1;
        assert!(!signature.trim().is_empty(), "{}: an instinct fires on its signature, never on any failure", x.id);
        for out in GENUINE_FAILURES {
            assert!(!out.contains(signature.as_str()), "{} ({signature:?}) would fire on {out:?}", x.id);
        }
    }
    assert!(failures >= 20, "{failures}");
}

#[test]
fn a_failure_route_without_a_signature_is_refused() {
    let r = bound("acme");
    let mut x = route(&r, "dead-end-d41b1e").clone();
    x.trigger = Trigger::AfterFailure { command: "exec python3 -m unittest".into(), signature: String::new() };
    let err = adage_instincts::bind::check_route(&x, adage_instincts::catalogue(), adage_core::CommandTable::load())
        .unwrap_err();
    assert!(err.contains("without a signature"), "{err}");
}

#[test]
fn large_go_test_files_are_scanned_not_skipped() {
    // urfave/cli: command_test.go and flag_test.go are over 64 KiB. A go:generate scan reads a
    // line at a time, so it reads their first 64 KiB instead of skipping them.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("go.mod"), "module example.com/big\n\ngo 1.24\n").unwrap();
    let mut big = String::from("package big\n\n//go:generate stringer -type=Mode\n");
    while big.len() < 170 * 1024 {
        big.push_str("func TestX(t *testing.T) { if 1 != 1 { t.Fatal(\"x\") } }\n");
    }
    std::fs::write(d.path().join("command_test.go"), &big).unwrap();
    let facts = adage_instincts::extract(d.path());
    assert!(facts.skipped_files.is_empty(), "{:?}", facts.skipped_files);
    assert_eq!(facts.partial_files, vec!["command_test.go".to_string()]);
    assert_eq!(facts.go.unwrap().generate, vec![("command_test.go".to_string(), "stringer -type=Mode".to_string())]);
    let r = bind(d.path(), &[], &BTreeSet::new());
    assert!(!r.skipped.iter().any(|(_, w)| w.contains("64 KiB")), "{:?}", r.skipped);
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
    for bad in ["evil", "adage-reflex", "ignore", "push", "curl", "token", "fire"] {
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
            assert_eq!(x.strength, Strength::Prime, "{name} {}", x.id);
            assert!(!x.authored && x.slots.is_empty());
            assert!(
                matches!(x.step, Step::Run { .. } | Step::Advise { .. }),
                "never an inhibit avoid step: {name} {}",
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
                Effect::Observe | Effect::Verify | Effect::Regenerate => assert_eq!(x.state, RouteState::Forming),
                Effect::Environment => assert_eq!(x.state, RouteState::Nascent),
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
        strength: Strength::Inhibit,
        state: RouteState::Firm,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { provenance: "desire-path".into(), support: 7, ..Default::default() },
        authored: false,
    }
}

#[test]
fn learned_routes_and_review_rejections_override_instincts() {
    let base = bound("acme");
    // 1. The learner learned the same dead end: same id, learned wins.
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
    fl.strength = Strength::Fire;
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
    assert!(!has_any(&r, "exec pytest", "make test"), "a learned route directs make ci after pytest");
    assert!(!ids.contains("finish-line-e7e809"), "a learned finish line covers schema/*.json");
    assert!(!ids.contains("dead-end-e8eb22"), "rejected in review");
    assert!(!has_any(&r, "exec python -m unittest", "make test"), "learned route names python -m unittest");
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
fn route_ids_line_up_with_delivery_shapes() {
    // An instinct shares its id with the reflex it stands in for, and has that reflex's shape.
    for name in FIXTURES {
        for x in bound(name).routes {
            let shape = adage_tracks::shape(&x);
            if x.id.starts_with("finish-line-") {
                assert_eq!(shape, "obligation", "{}", x.id);
            } else if x.id.starts_with("dead-end-") {
                assert_eq!(shape, "avoid", "{}", x.id);
            } else {
                assert!(x.id.starts_with("instinct-"), "{}", x.id);
                let advice = matches!(x.step, Step::Advise { .. });
                assert_eq!(shape, if advice { "advise" } else { "avoid" }, "{}", x.id);
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
    let ctx = adage_instincts::detect::Ctx { facts: &facts, table, cat };
    for i in cat.instincts.iter().filter(|i| !i.enabled) {
        let d = adage_instincts::detect::detect(&i.id, &ctx);
        assert!(d.routes.is_empty() && d.advisories.is_empty() && d.skipped.is_empty(), "{}", i.id);
    }
    let order: BTreeSet<&str> = adage_instincts::detect::BIND_ORDER.into_iter().collect();
    let ids: BTreeSet<&str> = cat.instincts.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(order, ids, "every catalogue entry has a place in the bind order");
}
