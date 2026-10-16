//! Declared facts: what a repository's own files say about how it is built, tested and generated.
//!
//! Everything here comes from bounded reads of repository files ([`crate::files`]); nothing is
//! executed. Every declared command, and the body it expands to, is classified by adage-core's
//! normaliser ([`crate::safety`]) and carries its verdict, so a detector can only bind to entries
//! whose bodies are plain and classify at `verify` or `regenerate`.

use crate::catalogue::catalogue;
use crate::files::{safe_rel, RepoFiles};
use crate::safety::{self, Line};
use adage_core::{CommandTable, Effect};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// At most this many `.go` files are scanned for `//go:generate`.
pub const MAX_GO_FILES: usize = 200;
/// At most this many files are scanned (head only) for generated-file markers.
pub const MAX_MARKER_FILES: usize = 400;
/// At most this many workflow files are read.
pub const MAX_WORKFLOWS: usize = 32;
/// At most this many CI commands are kept.
pub const MAX_CI_COMMANDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Makefile,
    PackageJson,
    Justfile,
    Taskfile,
    Tox,
    Nox,
}

/// Whether an entry's body may be directed, and at what effect.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "verdict", rename_all = "lowercase")]
pub enum Verdict {
    /// Every line is plain; `effect` is the strongest classified effect in the body.
    Plain { effect: Effect },
    /// It may never bind, and why.
    Refused { reason: String },
}

/// One declared entry point: a Makefile target, a package.json script, a just recipe, …
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub source: Source,
    pub name: String,
    /// The command an agent runs to invoke it, if Adage could ever direct it (`make test`,
    /// `npm run check`). `None` for runners adage-core cannot classify (just, task, tox, nox).
    pub command: Option<String>,
    /// The effect adage-core gives `command` (what track validation will re-derive).
    pub command_effect: Option<Effect>,
    /// The command lines it expands to, in order (Makefile prerequisites' recipes first; npm
    /// `pre`/`post` scripts included; nested `npm run x` expanded).
    pub body: Vec<String>,
    /// Makefile prerequisites, expanded.
    pub prereqs: Vec<String>,
    /// The entry this one only runs (`"test": "npm run check"`, `test: check` with no recipe).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias_of: Option<String>,
    pub verdict: Verdict,
    #[serde(skip)]
    pub lines: Vec<Line>,
}

impl Entry {
    /// The body effect, if the body is plain.
    pub fn body_effect(&self) -> Option<Effect> {
        match self.verdict {
            Verdict::Plain { effect } => Some(effect),
            Verdict::Refused { .. } => None,
        }
    }

    /// Everything Adage would direct by running this entry: the stronger of the command's own
    /// class and its body's. `None` if it cannot be directed or its body is refused.
    pub fn effect(&self) -> Option<Effect> {
        Some(safety::max_effect(self.command_effect?, self.body_effect()?))
    }

    /// Repository scripts its body runs.
    pub fn scripts(&self) -> Vec<String> {
        let s: BTreeSet<String> = self.lines.iter().flat_map(|l| l.scripts.iter().cloned()).collect();
        s.into_iter().collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PythonFacts {
    pub pyproject: bool,
    /// pytest is configured: `[tool.pytest.ini_options]`, pytest.ini, setup.cfg `[tool:pytest]`,
    /// tox.ini `[pytest]`, or a conftest.py.
    pub pytest_configured: bool,
    pub tox: bool,
    /// tox is configured in tox.ini (else in pyproject.toml).
    pub tox_ini: bool,
    pub nox: bool,
    pub uv: bool,
    pub poetry: bool,
    /// The project's own virtual environment (`.venv` or `venv` holding a `pyvenv.cfg`), created
    /// but perhaps not activated: its interpreter has the project's dependencies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venv: Option<String>,
    /// For a uv project, the dependency group `uv run` needs for pytest: `""` when a default group
    /// has it, else the group to name (`tests`). `None` when no group has pytest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uv_pytest_group: Option<String>,
    /// Import packages the project itself provides (`src/click/__init__.py` → `click`): a
    /// `No module named '<package>'` names the environment, never the code under test.
    pub packages: Vec<String>,
    /// Import names of the test dependencies pyproject.toml declares (the `test`, `tests`,
    /// `testing` and `dev` dependency groups and optional dependencies, uv's legacy
    /// dev-dependencies), at most [`MAX_TEST_DEPS`]: missing from an interpreter, they name the
    /// environment too.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub test_deps: Vec<String>,
}

/// At most this many declared test dependencies are read.
pub const MAX_TEST_DEPS: usize = 12;

/// The import names of pyproject.toml's declared test dependencies ([`PythonFacts::test_deps`]).
/// A requirement's distribution name (`pytest-xdist>=3`) becomes its usual import name
/// (`pytest_xdist`); only names that are plain identifiers are kept.
pub fn test_deps(pyproject: &toml::Value) -> Vec<String> {
    const GROUPS: [&str; 4] = ["test", "tests", "testing", "dev"];
    let mut lists: Vec<&toml::Value> = Vec::new();
    for table in
        [pyproject.get("dependency-groups"), pyproject.get("project").and_then(|p| p.get("optional-dependencies"))]
            .into_iter()
            .flatten()
    {
        lists.extend(GROUPS.iter().filter_map(|g| table.get(*g)));
    }
    lists.extend(pyproject.get("tool").and_then(|t| t.get("uv")).and_then(|u| u.get("dev-dependencies")));
    let mut out: Vec<String> = Vec::new();
    for req in lists.iter().filter_map(|l| l.as_array()).flatten().filter_map(|r| r.as_str()) {
        let name: String = req.trim().chars().take_while(|c| c.is_ascii_alphanumeric() || "_.-".contains(*c)).collect();
        let import = name.to_ascii_lowercase().replace(['-', '.'], "_");
        let ok = !import.is_empty()
            && import.len() <= 40
            && import.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && import.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if ok && !out.contains(&import) && out.len() < MAX_TEST_DEPS {
            out.push(import);
        }
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CargoFacts {
    pub workspace: bool,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct GoFacts {
    pub module: String,
    /// `(file, command)` for each `//go:generate` line.
    pub generate: Vec<(String, String)>,
    /// Why `go generate` may not be directed, if a directive is not plain. A directive runs
    /// without a shell, but it is checked as if it were a body line.
    pub generate_refused: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PackageManager {
    /// npm, pnpm, yarn or bun.
    pub name: String,
    /// `package-manager-field`, a lockfile name, or `default`.
    pub declared_by: String,
}

/// A file the repository marks, or its generator shows, as generated.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Generated {
    pub path: String,
    /// `code-generated` (`Code generated … DO NOT EDIT`), `at-generated` (`@generated`), or
    /// `generator-output` (a path a declared generator script writes).
    pub marker: String,
}

/// A declared generator: an entry that regenerates, and what it reads and writes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GeneratorLink {
    pub source: Source,
    pub entry: String,
    pub command: String,
    /// Globs of the generator's inputs.
    pub inputs: Vec<String>,
    /// `declared` (Makefile prerequisites) or `script` (paths named in the generator script).
    pub inputs_from: String,
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DeclaredFacts {
    pub entries: Vec<Entry>,
    pub package_json: bool,
    pub package_manager: Option<PackageManager>,
    /// Lockfiles of more than one node package manager, or a packageManager field that disagrees
    /// with the lockfile.
    pub package_manager_conflict: bool,
    /// Root lockfiles: `(path, manager)`.
    pub lockfiles: Vec<(String, String)>,
    pub python: PythonFacts,
    pub cargo: Option<CargoFacts>,
    pub go: Option<GoFacts>,
    pub ci_commands: Vec<String>,
    /// Repository scripts that declared bodies run and that drive a test runner themselves
    /// (`tools/testkit.py` calling `unittest.TextTestRunner`): script → runner family.
    pub script_runners: BTreeMap<String, String>,
    pub generated: Vec<Generated>,
    pub generators: Vec<GeneratorLink>,
    /// Node: the package manager a `preinstall` `only-allow` guard insists on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only_allow: Option<String>,
    /// Node: the package manager `devEngines.packageManager` names (npm refuses to run for
    /// another, with `EBADDEVENGINES`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_engines_manager: Option<String>,
    /// Node: the root manifest depends through a protocol npm does not support (`workspace:`,
    /// `catalog:`, `link:`, `patch:`, `portal:`), so `npm install` fails with
    /// `Unsupported URL Type`.
    pub non_npm_protocol: bool,
    /// Files larger than the whole-file cap of which a line scan read the first 64 KiB.
    pub partial_files: Vec<String>,
    /// Files skipped, and why.
    pub skipped_files: BTreeMap<String, String>,
}

impl DeclaredFacts {
    pub fn entry(&self, source: Source, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.source == source && e.name == name)
    }
}

/// A name Adage may put into a directed command (`make <name>`, `npm run <name>`).
pub fn safe_entry_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 48
        && n.as_bytes()[0].is_ascii_alphanumeric()
        && n.bytes().all(|c| c.is_ascii_alphanumeric() || b":_.-".contains(&c))
}

/// Extract the declared facts of the repository at `root`.
pub fn extract(root: &Path) -> DeclaredFacts {
    let Some(files) = RepoFiles::open(root) else { return DeclaredFacts::default() };
    extract_from(&files)
}

pub fn extract_from(files: &RepoFiles) -> DeclaredFacts {
    let table = CommandTable::load();
    let mut f = DeclaredFacts::default();
    let generators = &catalogue().generators.known;

    // Lockfiles.
    for (name, manager) in &catalogue().lockfiles {
        if files.is_file(name) {
            f.lockfiles.push((name.clone(), manager.clone()));
        }
    }
    f.lockfiles.sort();

    // package.json (read first: the package manager decides how scripts are run).
    let pkg = files.read("package.json").and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    f.package_json = pkg.is_some();
    if let Some(pkg) = &pkg {
        let (pm, conflict) = package_manager(pkg, &f.lockfiles);
        f.package_manager = Some(pm);
        f.package_manager_conflict = conflict;
        node_guards(pkg, &mut f);
    }

    // Makefile.
    for name in ["GNUmakefile", "makefile", "Makefile"] {
        if let Some(text) = files.read(name) {
            f.entries.extend(makefile_entries(&text, files, generators, table));
            break;
        }
    }
    if let (Some(pkg), Some(pm)) = (&pkg, &f.package_manager) {
        f.entries.extend(package_entries(pkg, &pm.name, files, generators, table));
    }
    for name in ["justfile", "Justfile", ".justfile"] {
        if let Some(text) = files.read(name) {
            f.entries.extend(justfile_entries(&text, files, generators, table));
            break;
        }
    }
    for name in ["Taskfile.yml", "Taskfile.yaml", "taskfile.yml"] {
        if let Some(text) = files.read(name) {
            f.entries.extend(taskfile_entries(&text));
            break;
        }
    }

    // Python.
    let pyproject = files.read("pyproject.toml").and_then(|t| toml::from_str::<toml::Value>(&t).ok());
    f.python.pyproject = pyproject.is_some();
    let tool = |k: &str| pyproject.as_ref().and_then(|p| p.get("tool")).and_then(|t| t.get(k)).is_some();
    let pytest_ini_options = pyproject
        .as_ref()
        .and_then(|p| p.get("tool"))
        .and_then(|t| t.get("pytest"))
        .and_then(|t| t.get("ini_options"))
        .is_some();
    let tox_ini = files.read("tox.ini");
    let ini_has = |text: &Option<String>, pred: &dyn Fn(&str) -> bool| {
        text.as_deref().is_some_and(|t| {
            t.lines().any(|l| l.trim().strip_prefix('[').and_then(|x| x.strip_suffix(']')).is_some_and(pred))
        })
    };
    let setup_cfg = files.read("setup.cfg");
    f.python.pytest_configured = pytest_ini_options
        || files.is_file("pytest.ini")
        || ini_has(&setup_cfg, &|s| s == "tool:pytest")
        || ini_has(&tox_ini, &|s| s == "pytest")
        || files.is_file("conftest.py")
        || files.is_file("tests/conftest.py");
    f.python.tox = tool("tox") || ini_has(&tox_ini, &|s| s == "tox" || s.starts_with("testenv"));
    f.python.tox_ini = ini_has(&tox_ini, &|s| s == "tox" || s.starts_with("testenv"));
    f.python.nox = files.is_file("noxfile.py");
    f.python.uv = tool("uv") || files.is_file("uv.lock");
    f.python.poetry = tool("poetry") || files.is_file("poetry.lock");
    f.python.venv = [".venv", "venv"]
        .into_iter()
        .find(|d| files.is_file(&format!("{d}/pyvenv.cfg")) && files.is_dir(&format!("{d}/bin")))
        .map(str::to_string);
    if f.python.uv {
        f.python.uv_pytest_group = pyproject.as_ref().and_then(uv_pytest_group);
    }
    f.python.test_deps = pyproject.as_ref().map(test_deps).unwrap_or_default();
    if f.python.tox {
        f.entries.push(tox_entry(tox_ini.as_deref(), pyproject.as_ref(), files, generators, table));
    }
    if f.python.nox {
        match files.read("noxfile.py").as_deref().and_then(nox_entry) {
            Some(e) => f.entries.push(e),
            None => f.entries.push(unclassified(Source::Nox, "nox", "noxfile.py declares no tests session")),
        }
    }

    // Cargo.
    if let Some(cargo) = files.read("Cargo.toml").and_then(|t| toml::from_str::<toml::Value>(&t).ok()) {
        let ws = cargo.get("workspace");
        let members = ws
            .and_then(|w| w.get("members"))
            .and_then(|m| m.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).filter(|m| safe_rel(m)).map(str::to_string).collect())
            .unwrap_or_default();
        f.cargo = Some(CargoFacts { workspace: ws.is_some(), members });
    }

    // One bounded walk serves the go:generate scan and the generated-marker scan.
    let all = files.walk();

    // Go.
    if let Some(gomod) = files.read("go.mod") {
        let module = gomod
            .lines()
            .find_map(|l| l.trim().strip_prefix("module "))
            .map(|m| m.trim().to_string())
            .filter(|m| m.chars().all(|c| c.is_ascii_graphic()))
            .unwrap_or_default();
        let mut generate = Vec::new();
        for rel in all.iter().filter(|p| p.ends_with(".go")).take(MAX_GO_FILES) {
            // A directive is one line: a large file (a long `_test.go`) is scanned as far as the
            // whole-file cap, not skipped.
            if let Some((text, partial)) = files.read_lines_prefix(rel) {
                if partial {
                    f.partial_files.push(rel.clone());
                }
                for l in text.lines() {
                    if let Some(cmd) = l.strip_prefix("//go:generate ") {
                        generate.push((rel.clone(), cmd.trim().to_string()));
                    }
                }
            }
        }
        let generate_refused = generate.iter().find_map(|(file, c)| {
            safety::check_line(c, files, generators, table).err().map(|e| format!("{file}: {e}"))
        });
        f.go = Some(GoFacts { module, generate, generate_refused });
    }

    // CI.
    let workflows: Vec<&String> = all
        .iter()
        .filter(|p| {
            p.starts_with(".github/workflows/")
                && p.matches('/').count() == 2
                && (p.ends_with(".yml") || p.ends_with(".yaml"))
        })
        .take(MAX_WORKFLOWS)
        .collect();
    for w in workflows {
        if let Some(text) = files.read(w) {
            f.ci_commands.extend(run_steps(&text));
        }
    }
    f.ci_commands.truncate(MAX_CI_COMMANDS);

    // Generated-file markers.
    let code: Vec<&String> = all.iter().filter(|p| code_file(p)).take(MAX_MARKER_FILES).collect();
    for rel in code {
        if let Some(head) = files.head(rel) {
            if let Some(marker) = generated_marker(&head) {
                f.generated.push(Generated { path: rel.clone(), marker: marker.into() });
            }
        }
    }

    // The project's own import packages.
    if f.python.pyproject || f.python.pytest_configured {
        f.python.packages = project_packages(&all);
    }

    // Scripts that are test runners in disguise.
    let scripts: BTreeSet<String> = f
        .entries
        .iter()
        .filter(|e| e.body_effect().is_some() && e.command_effect.is_some_and(|x| x == Effect::Verify))
        .flat_map(|e| e.scripts())
        .collect();
    for s in scripts {
        if let Some(fam) = files.read(&s).as_deref().and_then(script_runner) {
            f.script_runners.insert(s, fam.to_string());
        }
    }

    // Generators: what each regenerating entry reads and writes.
    f.generators = generator_links(&f.entries, files, &all);
    for g in &f.generators {
        for o in &g.outputs {
            if !f.generated.iter().any(|x| &x.path == o) {
                f.generated.push(Generated { path: o.clone(), marker: "generator-output".into() });
            }
        }
    }

    f.entries.sort_by(|a, b| (a.source, &a.name).cmp(&(b.source, &b.name)));
    f.generated.sort();
    f.generators.sort_by(|a, b| (a.source, &a.entry).cmp(&(b.source, &b.entry)));
    f.skipped_files = files.notes();
    f
}

/// The test runner a Python script drives, if it is a runner script: `py:unittest` or `py:pytest`.
pub fn script_runner(text: &str) -> Option<&'static str> {
    let has = |s: &str| text.contains(s);
    if has("import unittest") && (has("unittest.main(") || has("TextTestRunner") || has("defaultTestLoader")) {
        Some("py:unittest")
    } else if has("import pytest") && has("pytest.main(") {
        Some("py:pytest")
    } else {
        None
    }
}

/// Import packages the project provides: `src/<name>/__init__.py`, else `<name>/__init__.py` at
/// the root (not tests, docs or tooling). At most three, sorted.
pub fn project_packages(all: &[String]) -> Vec<String> {
    const NOT: [&str; 10] =
        ["tests", "test", "testing", "docs", "doc", "examples", "bench", "benchmarks", "scripts", "tools"];
    let ident = |n: &str| {
        !n.is_empty()
            && n.len() <= 48
            && !n.as_bytes()[0].is_ascii_digit()
            && n.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            && !NOT.contains(&n)
    };
    let pick = |prefix: &str| -> Vec<String> {
        all.iter()
            .filter_map(|p| p.strip_prefix(prefix)?.strip_suffix("/__init__.py"))
            .filter(|n| !n.contains('/') && ident(n))
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .take(3)
            .collect()
    };
    let src = pick("src/");
    if src.is_empty() {
        pick("")
    } else {
        src
    }
}

/// A requirement's distribution name, normalised (`pytest-xdist[psutil]>=3` → `pytest-xdist`).
fn requirement_name(r: &str) -> String {
    r.trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c))
        .collect::<String>()
        .to_ascii_lowercase()
        .replace(['_', '.'], "-")
}

/// Whether the PEP 735 dependency group `group` (with its `include-group`s) has pytest.
fn group_has_pytest(groups: &toml::Value, group: &str, depth: usize) -> bool {
    if depth > 4 {
        return false;
    }
    let Some(items) = groups.get(group).and_then(|g| g.as_array()) else { return false };
    items.iter().any(|x| match x {
        toml::Value::String(r) => requirement_name(r) == "pytest",
        toml::Value::Table(t) => {
            t.get("include-group").and_then(|g| g.as_str()).is_some_and(|g| group_has_pytest(groups, g, depth + 1))
        }
        _ => false,
    })
}

/// For a uv project: `""` if `uv run` has pytest as it syncs by default (a default group, or the
/// legacy `tool.uv.dev-dependencies`), else the one group to name: `tests`, `test` or `testing`,
/// else the first group by name that has pytest. `None` if no group has it.
fn uv_pytest_group(pyproject: &toml::Value) -> Option<String> {
    let uv = pyproject.get("tool").and_then(|t| t.get("uv"));
    let legacy = uv.and_then(|u| u.get("dev-dependencies")).and_then(|d| d.as_array());
    if legacy.is_some_and(|d| d.iter().any(|r| r.as_str().is_some_and(|r| requirement_name(r) == "pytest"))) {
        return Some(String::new());
    }
    let groups = pyproject.get("dependency-groups")?;
    let names: Vec<&String> = groups.as_table()?.keys().collect();
    let defaults: Vec<String> = match uv.and_then(|u| u.get("default-groups")) {
        Some(toml::Value::Array(a)) => a.iter().filter_map(|g| g.as_str().map(str::to_string)).collect(),
        Some(toml::Value::String(s)) if s == "all" => names.iter().map(|n| n.to_string()).collect(),
        _ => vec!["dev".into()],
    };
    if defaults.iter().any(|g| group_has_pytest(groups, g, 0)) {
        return Some(String::new());
    }
    let preferred = ["tests", "test", "testing"].into_iter().find(|p| names.iter().any(|n| n == p));
    let chosen = match preferred {
        Some(p) if group_has_pytest(groups, p, 0) => Some(p.to_string()),
        _ => names.into_iter().find(|n| group_has_pytest(groups, n, 0)).cloned(),
    };
    chosen.filter(|g| safe_entry_name(g))
}

/// tox's test environment as one declared entry, `tox -e py`, whose body is the base test
/// environment's commands (`[testenv] commands` in tox.ini, `[tool.tox.env_run_base] commands` in
/// pyproject.toml) with `{posargs}` removed. Refused when those commands depend on factors
/// (`tests: pytest`) or cannot be read: `tox -e py` would then not run what they say.
fn tox_entry(
    tox_ini: Option<&str>,
    pyproject: Option<&toml::Value>,
    files: &RepoFiles,
    generators: &[String],
    table: &CommandTable,
) -> Entry {
    let none = || "tox's base test environment declares no commands".to_string();
    let lines = match tox_ini {
        Some(ini) if ini.lines().any(|l| l.trim() == "[testenv]") => tox_ini_commands(ini),
        _ => pyproject
            .and_then(|p| p.get("tool")?.get("tox")?.get("env_run_base")?.get("commands").cloned())
            .map_or_else(|| Err(none()), |c| tox_toml_commands(&c)),
    };
    let lines = lines.and_then(|l| if l.is_empty() { Err(none()) } else { Ok(l) });
    entry(Source::Tox, "py", Some("tox -e py".into()), lines, vec![], files, generators, table)
}

static POSARGS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{posargs(:[^{}]*)?\}").unwrap());
static FACTOR: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Za-z0-9_.{},!-]+:\s").unwrap());

fn tox_ini_commands(ini: &str) -> Result<Vec<String>, String> {
    let (mut in_testenv, mut in_commands) = (false, false);
    let mut out = Vec::new();
    for raw in ini.lines() {
        let t = raw.trim();
        if let Some(sec) = t.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            in_testenv = sec == "testenv";
            in_commands = false;
            continue;
        }
        if !in_testenv || t.starts_with('#') || t.starts_with(';') {
            continue;
        }
        if !raw.starts_with([' ', '\t']) {
            in_commands = false;
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == "commands" {
                    in_commands = true;
                    if !v.trim().is_empty() {
                        out.push(v.trim().to_string());
                    }
                }
            }
        } else if in_commands && !t.is_empty() {
            out.push(t.to_string());
        }
    }
    if out.iter().any(|l| FACTOR.is_match(l)) {
        return Err("tox's test commands depend on factors, so no one tox command runs them".into());
    }
    Ok(out.iter().map(|l| POSARGS.replace_all(l, "").trim().to_string()).filter(|l| !l.is_empty()).collect())
}

fn tox_toml_commands(v: &toml::Value) -> Result<Vec<String>, String> {
    let Some(cmds) = v.as_array() else { return Err("tox's commands are not a list".into()) };
    let mut out = Vec::new();
    for c in cmds {
        let Some(argv) = c.as_array() else { return Err("a tox command is not a list".into()) };
        let mut words = Vec::new();
        for a in argv {
            match a {
                toml::Value::String(w) if !w.is_empty() && !w.contains(char::is_whitespace) => words.push(w.clone()),
                toml::Value::Table(t) if t.get("replace").and_then(|r| r.as_str()) == Some("posargs") => {}
                _ => return Err("a tox command has an argument Adage cannot read".into()),
            }
        }
        out.push(words.join(" "));
    }
    Ok(out)
}

/// nox's test session (`@nox.session` over `def tests(` or `def test(`), as `nox -s <name>`. A
/// nox session is Python code, so its body can never be checked plain: it is refused, and
/// reported.
fn nox_entry(text: &str) -> Option<Entry> {
    let lines: Vec<&str> = text.lines().collect();
    let name = lines.iter().enumerate().find_map(|(i, l)| {
        let n = ["tests", "test"].into_iter().find(|n| l.trim_start().starts_with(&format!("def {n}(")))?;
        lines[..i].iter().rev().take(3).any(|p| p.trim_start().starts_with("@nox.session")).then_some(n)
    })?;
    let mut e = unclassified(Source::Nox, name, "a nox session is Python code, so its body cannot be checked");
    e.command = Some(format!("nox -s {name}"));
    Some(e)
}

/// Guards a node repository puts on its package manager: a `preinstall` `only-allow`,
/// `devEngines.packageManager`, and dependencies through protocols npm does not support.
fn node_guards(pkg: &serde_json::Value, f: &mut DeclaredFacts) {
    let node = ["npm", "pnpm", "yarn", "bun"];
    if let Some(pre) = pkg.get("scripts").and_then(|s| s.get("preinstall")).and_then(|v| v.as_str()) {
        let w = safety::words(pre);
        if let Some(i) = w.iter().position(|x| x == "only-allow") {
            f.only_allow = w.get(i + 1).filter(|m| node.contains(&m.as_str())).cloned();
        }
    }
    let de = pkg.get("devEngines").and_then(|d| d.get("packageManager"));
    let de = match de {
        Some(serde_json::Value::Array(a)) => a.first(),
        other => other,
    };
    f.dev_engines_manager =
        de.and_then(|d| d.get("name")).and_then(|n| n.as_str()).filter(|n| node.contains(n)).map(str::to_string);
    let protocols = ["workspace:", "catalog:", "link:", "patch:", "portal:"];
    f.non_npm_protocol = ["dependencies", "devDependencies", "optionalDependencies"].iter().any(|k| {
        pkg.get(*k)
            .and_then(|d| d.as_object())
            .is_some_and(|d| d.values().any(|v| v.as_str().is_some_and(|v| protocols.iter().any(|p| v.starts_with(p)))))
    });
}

fn unclassified(source: Source, name: &str, why: &str) -> Entry {
    Entry {
        source,
        name: name.into(),
        command: None,
        command_effect: None,
        body: vec![],
        prereqs: vec![],
        alias_of: None,
        verdict: Verdict::Refused { reason: why.into() },
        lines: vec![],
    }
}

/// Classify a body and build its entry.
#[allow(clippy::too_many_arguments)]
fn entry(
    source: Source,
    name: &str,
    command: Option<String>,
    body: Result<Vec<String>, String>,
    prereqs: Vec<String>,
    files: &RepoFiles,
    generators: &[String],
    table: &CommandTable,
) -> Entry {
    let command_effect = command.as_deref().map(|c| adage_tracks::io::classify(c, table));
    let (verdict, lines, body) = match body {
        Err(reason) => (Verdict::Refused { reason }, vec![], vec![]),
        Ok(body) => match safety::check_body(&body, files, generators, table) {
            Ok((effect, lines)) => (Verdict::Plain { effect }, lines, body),
            Err(reason) => (Verdict::Refused { reason }, vec![], body),
        },
    };
    Entry { source, name: name.into(), command, command_effect, body, prereqs, alias_of: None, verdict, lines }
}

// ---------------------------------------------------------------------------------------------
// Makefile

#[derive(Debug, Clone, Default)]
struct Rule {
    targets: Vec<String>,
    prereqs: Vec<String>,
    recipe: Vec<String>,
}

static ASSIGN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^([A-Za-z_][A-Za-z0-9_.-]*)\s*(\?=|::=|:=|\+=|!=|=)\s*(.*)$").unwrap());

/// Strip a Makefile comment (`#` not escaped).
fn strip_comment(l: &str) -> &str {
    let b = l.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'#' && (i == 0 || b[i - 1] != b'\\') {
            return &l[..i];
        }
    }
    l
}

/// Expand make variables and the few functions that matter; anything else is left as written,
/// and the body check refuses what remains (`$(`).
fn expand(s: &str, vars: &BTreeMap<String, String>, auto: &[(&str, String)], depth: usize) -> String {
    if depth > 8 {
        return s.to_string();
    }
    let mut out = String::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] != '$' {
            out.push(cs[i]);
            i += 1;
            continue;
        }
        match cs.get(i + 1) {
            Some('$') => {
                out.push('$');
                i += 2;
            }
            Some(&open @ ('(' | '{')) => {
                let close = if open == '(' { ')' } else { '}' };
                let mut j = i + 2;
                let mut level = 1;
                while j < cs.len() {
                    if cs[j] == open {
                        level += 1;
                    } else if cs[j] == close {
                        level -= 1;
                        if level == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if j >= cs.len() {
                    out.extend(&cs[i..]);
                    break;
                }
                let inner: String = cs[i + 2..j].iter().collect();
                let inner = expand(&inner, vars, auto, depth + 1);
                if inner == "MAKE" {
                    out.push_str("make");
                } else if let Some(p) = inner.strip_prefix("wildcard ") {
                    out.push_str(p.trim());
                } else if !inner.contains([' ', ',', '(']) {
                    // An undefined variable expands to nothing, as make does.
                    let v = vars.get(&inner).cloned().unwrap_or_default();
                    out.push_str(&expand(&v, vars, auto, depth + 1));
                } else {
                    out.push_str("$(");
                    out.push_str(&inner);
                    out.push(')');
                }
                i = j + 1;
            }
            Some(c) => {
                let name = c.to_string();
                match auto.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(v),
                    None => {
                        out.push('$');
                        out.push(*c);
                    }
                }
                i += 2;
            }
            None => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

fn parse_makefile(text: &str) -> (Vec<Rule>, BTreeSet<String>) {
    let mut logical: Vec<String> = Vec::new();
    let mut cur = String::new();
    for raw in text.lines() {
        if let Some(s) = raw.strip_suffix('\\') {
            cur.push_str(s);
            cur.push(' ');
        } else {
            cur.push_str(raw);
            logical.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        logical.push(cur);
    }
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    let mut raw_rules: Vec<Rule> = Vec::new();
    let mut phony = BTreeSet::new();
    let mut current: Option<usize> = None;
    let mut in_define = false;
    for line in &logical {
        if in_define {
            if line.trim() == "endef" {
                in_define = false;
            }
            continue;
        }
        if let Some(r) = line.strip_prefix('\t') {
            if let Some(i) = current {
                let r = r.trim();
                if !r.is_empty() && !r.starts_with('#') {
                    raw_rules[i].recipe.push(r.to_string());
                }
            }
            continue;
        }
        let t = strip_comment(line).trim();
        if t.is_empty() {
            continue;
        }
        current = None;
        let first = t.split_whitespace().next().unwrap_or("");
        if first == "define" {
            in_define = true;
            continue;
        }
        if [
            "include", "-include", "sinclude", "ifeq", "ifneq", "ifdef", "ifndef", "else", "endif", "override",
            "unexport", "vpath",
        ]
        .contains(&first)
        {
            continue;
        }
        let t = t.strip_prefix("export ").unwrap_or(t);
        if let Some(c) = ASSIGN.captures(t) {
            let (name, op, value) = (c[1].to_string(), &c[2], c[3].trim().to_string());
            match op {
                "?=" => {
                    vars.entry(name).or_insert(value);
                }
                "+=" => {
                    let e = vars.entry(name).or_default();
                    if !e.is_empty() {
                        e.push(' ');
                    }
                    e.push_str(&value);
                }
                // A shell assignment's value is unknowable without running it: refuse its use.
                // (The marker contains a space, so expansion never mistakes it for a variable.)
                "!=" => {
                    vars.insert(name, "$(shell !=)".into());
                }
                _ => {
                    vars.insert(name, value);
                }
            }
            continue;
        }
        let Some((lhs, rhs)) = t.split_once(':') else { continue };
        let rhs = rhs.strip_prefix(':').unwrap_or(rhs);
        let (rhs, inline) = match rhs.split_once(';') {
            Some((a, b)) => (a, Some(b.trim().to_string())),
            None => (rhs, None),
        };
        // Target-specific variables (`test: FOO = 1`) are not rules.
        if ASSIGN.is_match(rhs.trim()) {
            continue;
        }
        let targets: Vec<String> = lhs.split_whitespace().map(str::to_string).collect();
        if targets.is_empty() || targets.iter().any(|x| x.contains('%') || x.contains('=')) {
            continue;
        }
        // Kept whole until expansion: `$(wildcard $(SRC)/*.json)` contains spaces.
        let prereqs: Vec<String> = vec![rhs.replace(" | ", " ")];
        if targets.iter().any(|x| x == ".PHONY") {
            phony.extend(expand(rhs, &vars, &[], 0).split_whitespace().map(str::to_string));
            continue;
        }
        if targets.iter().any(|x| x.starts_with('.')) && !targets.iter().any(|x| x.contains('/')) {
            continue;
        }
        raw_rules.push(Rule { targets, prereqs, recipe: inline.into_iter().filter(|s| !s.is_empty()).collect() });
        current = Some(raw_rules.len() - 1);
    }
    // Expand after the whole file is read: make's recursive variables see later assignments.
    let rules = raw_rules
        .into_iter()
        .map(|r| {
            let targets: Vec<String> = r
                .targets
                .iter()
                .flat_map(|t| expand(t, &vars, &[], 0).split_whitespace().map(str::to_string).collect::<Vec<_>>())
                .collect();
            let prereqs: Vec<String> = r
                .prereqs
                .iter()
                .flat_map(|p| expand(p, &vars, &[], 0).split_whitespace().map(str::to_string).collect::<Vec<_>>())
                .collect();
            let at = targets.first().cloned().unwrap_or_default();
            let auto = [("@", at), ("<", prereqs.first().cloned().unwrap_or_default()), ("^", prereqs.join(" "))];
            let recipe = r
                .recipe
                .iter()
                .map(|l| expand(l, &vars, &auto, 0).trim().trim_start_matches(['@', '-', '+']).trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            Rule { targets, prereqs, recipe }
        })
        .collect();
    (rules, phony)
}

/// A target's full body: the recipes of its prerequisite targets (depth first, bounded), then its
/// own. `Err` on a cycle or excessive depth.
fn make_body(target: &str, rules: &[Rule], seen: &mut BTreeSet<String>, depth: usize) -> Result<Vec<String>, String> {
    if depth > 8 {
        return Err("prerequisites nest too deeply".into());
    }
    if !seen.insert(target.to_string()) {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    let mine: Vec<&Rule> = rules.iter().filter(|r| r.targets.iter().any(|t| t == target)).collect();
    for r in &mine {
        for p in &r.prereqs {
            if rules.iter().any(|x| x.targets.iter().any(|t| t == p)) {
                out.extend(make_body(p, rules, seen, depth + 1)?);
            }
        }
    }
    for r in &mine {
        out.extend(r.recipe.iter().cloned());
    }
    Ok(out)
}

fn makefile_entries(text: &str, files: &RepoFiles, generators: &[String], table: &CommandTable) -> Vec<Entry> {
    let (rules, _phony) = parse_makefile(text);
    let mut names: BTreeSet<String> = BTreeSet::new();
    for r in &rules {
        names.extend(r.targets.iter().cloned());
    }
    names
        .into_iter()
        .map(|name| {
            let prereqs: Vec<String> =
                rules.iter().filter(|r| r.targets.contains(&name)).flat_map(|r| r.prereqs.iter().cloned()).collect();
            let command = safe_entry_name(&name).then(|| format!("make {name}"));
            let body = make_body(&name, &rules, &mut BTreeSet::new(), 0);
            // `test: check` with no recipe of its own only runs `check`.
            let own_recipe = rules.iter().any(|r| r.targets.contains(&name) && !r.recipe.is_empty());
            let alias_of = match prereqs.as_slice() {
                [p] if !own_recipe && rules.iter().any(|r| r.targets.contains(p)) => Some(p.clone()),
                _ => None,
            };
            let mut e = entry(Source::Makefile, &name, command, body, prereqs, files, generators, table);
            e.alias_of = alias_of;
            e
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// package.json

fn package_manager(pkg: &serde_json::Value, lockfiles: &[(String, String)]) -> (PackageManager, bool) {
    let node = ["npm", "pnpm", "yarn", "bun"];
    let locks: BTreeSet<&str> = lockfiles.iter().map(|(_, m)| m.as_str()).filter(|m| node.contains(m)).collect();
    let field = pkg
        .get("packageManager")
        .and_then(|v| v.as_str())
        .map(|s| s.split('@').next().unwrap_or("").to_string())
        .filter(|s| node.contains(&s.as_str()));
    let lock_name = |m: &str| lockfiles.iter().find(|(_, x)| x == m).map(|(p, _)| p.clone()).unwrap_or_default();
    match field {
        Some(pm) => {
            let conflict = locks.iter().any(|l| *l != pm);
            (PackageManager { name: pm, declared_by: "package-manager-field".into() }, conflict)
        }
        None => match locks.len() {
            0 => (PackageManager { name: "npm".into(), declared_by: "default".into() }, false),
            1 => {
                let pm = locks.iter().next().unwrap().to_string();
                let by = lock_name(&pm);
                (PackageManager { name: pm, declared_by: by }, false)
            }
            _ => (PackageManager { name: "npm".into(), declared_by: "default".into() }, true),
        },
    }
}

/// How `pm` runs the script `name`.
pub fn run_script(pm: &str, name: &str) -> String {
    match (pm, name) {
        ("npm" | "pnpm" | "yarn", "test") => format!("{pm} test"),
        ("bun", _) => format!("bun run {name}"),
        _ => format!("{pm} run {name}"),
    }
}

/// The script a body segment runs through a package manager, if any (`npm run x`, `yarn x`).
fn script_ref<'a>(argv: &'a [String], scripts: &BTreeMap<String, String>) -> Option<&'a str> {
    let pm = argv.first()?.as_str();
    if !["npm", "pnpm", "yarn", "bun"].contains(&pm) {
        return None;
    }
    let rest: Vec<&str> = argv[1..].iter().map(String::as_str).filter(|a| !a.starts_with('-')).collect();
    let name = match rest.as_slice() {
        ["run" | "run-script", n, ..] => *n,
        ["test", ..] if pm != "bun" => "test",
        [n, ..] if pm == "yarn" || pm == "pnpm" => *n,
        _ => return None,
    };
    scripts.contains_key(name).then_some(name)
}

/// Expand a script into its command lines: `pre<name>`, the script (nested script references
/// expanded), `post<name>`.
fn script_body(
    name: &str,
    scripts: &BTreeMap<String, String>,
    seen: &mut BTreeSet<String>,
    depth: usize,
) -> Result<Vec<String>, String> {
    if depth > 4 {
        return Err("scripts nest too deeply".into());
    }
    if !seen.insert(name.to_string()) {
        return Err(format!("script {name:?} runs itself"));
    }
    let mut out = Vec::new();
    for part in [format!("pre{name}"), name.to_string(), format!("post{name}")] {
        let Some(text) = scripts.get(&part) else { continue };
        if part != name && seen.contains(&part) {
            continue;
        }
        // Each `&&`/`;` segment that runs another script is replaced by that script's body.
        let mut kept: Vec<String> = Vec::new();
        for seg in text.split("&&").flat_map(|p| p.split(';')) {
            let argv = safety::words(seg.trim());
            match script_ref(&argv, scripts) {
                Some(n) if n != part => {
                    if !kept.is_empty() {
                        out.push(kept.join(" && "));
                        kept.clear();
                    }
                    out.extend(script_body(n, scripts, seen, depth + 1)?);
                }
                Some(_) => return Err(format!("script {part:?} runs itself")),
                None => kept.push(seg.trim().to_string()),
            }
        }
        if !kept.is_empty() {
            out.push(kept.join(" && "));
        }
    }
    seen.remove(name);
    Ok(out)
}

fn package_entries(
    pkg: &serde_json::Value,
    pm: &str,
    files: &RepoFiles,
    generators: &[String],
    table: &CommandTable,
) -> Vec<Entry> {
    let scripts: BTreeMap<String, String> = pkg
        .get("scripts")
        .and_then(|s| s.as_object())
        .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    scripts
        .keys()
        .map(|name| {
            if !safe_entry_name(name) {
                return entry(
                    Source::PackageJson,
                    name,
                    None,
                    Err("the script name contains characters a directed command may not".into()),
                    vec![],
                    files,
                    generators,
                    table,
                );
            }
            let body = script_body(name, &scripts, &mut BTreeSet::new(), 0);
            let mut e =
                entry(Source::PackageJson, name, Some(run_script(pm, name)), body, vec![], files, generators, table);
            // `"test": "npm run check"` only runs `check`.
            let text = scripts[name].trim();
            if !text.contains("&&") && !text.contains(';') {
                e.alias_of = script_ref(&safety::words(text), &scripts).filter(|n| n != name).map(str::to_string);
            }
            e
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// justfile, Taskfile

static JUST_RECIPE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^@?([A-Za-z0-9_-]+)[^:=]*:([^=].*)?$").unwrap());

fn justfile_entries(text: &str, files: &RepoFiles, generators: &[String], table: &CommandTable) -> Vec<Entry> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = out.last_mut() {
                let l = line.trim().trim_start_matches(['@', '-']).trim();
                if !l.is_empty() && !l.starts_with('#') {
                    last.1.push(l.to_string());
                }
            }
            continue;
        }
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("set ") || t.starts_with('[') {
            continue;
        }
        match JUST_RECIPE.captures(t) {
            Some(c) if !t.contains(":=") => out.push((c[1].to_string(), vec![])),
            _ => out.push((String::new(), vec![])),
        }
    }
    out.into_iter()
        .filter(|(n, _)| !n.is_empty())
        .map(|(n, body)| {
            let mut e = entry(Source::Justfile, &n, None, Ok(body), vec![], files, generators, table);
            e.command = None;
            e
        })
        .collect()
}

fn taskfile_entries(text: &str) -> Vec<Entry> {
    let mut in_tasks = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            in_tasks = line.trim_end() == "tasks:";
            continue;
        }
        if in_tasks && line.starts_with("  ") && !line.starts_with("   ") {
            if let Some(name) = line.trim().strip_suffix(':') {
                if safe_entry_name(name) {
                    out.push(unclassified(Source::Taskfile, name, "task is not in adage-core's command table"));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// CI

/// `run:` steps of a GitHub Actions workflow, line by line (no YAML parser: a workflow is
/// untrusted input and only its commands matter).
pub fn run_steps(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let t = l.trim_start();
        let mut indent = l.len() - t.len();
        let t = match t.strip_prefix("- ") {
            Some(rest) => {
                indent += 2;
                rest.trim_start()
            }
            None => t,
        };
        i += 1;
        let Some(v) = t.strip_prefix("run:") else { continue };
        let v = v.trim();
        if v.starts_with('|') || v.starts_with('>') {
            while i < lines.len() {
                let b = lines[i];
                let bt = b.trim_start();
                if !bt.is_empty() && b.len() - bt.len() <= indent {
                    break;
                }
                out.push(bt.trim().to_string());
                i += 1;
            }
        } else {
            let v = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(v);
            let v = v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')).unwrap_or(v);
            out.push(v.to_string());
        }
    }
    out.into_iter()
        .filter(|c| {
            !c.is_empty()
                && !c.starts_with('#')
                && c.len() <= 300
                && c.chars().all(|x| x.is_ascii_graphic() || x == ' ')
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Generated files

const CODE_EXT: &[&str] = &[
    "go", "ts", "tsx", "js", "mjs", "cjs", "py", "pyi", "rs", "java", "kt", "swift", "cs", "rb", "php", "dart", "c",
    "cc", "cpp", "h", "hpp", "m", "scala", "ex", "sql",
];
const DATA_EXT: &[&str] = &[
    "json", "csv", "tsv", "yaml", "yml", "toml", "proto", "graphql", "gql", "xml", "sql", "avsc", "thrift", "fbs",
    "prisma",
];

fn ext(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p).rsplit_once('.').map(|(_, e)| e).unwrap_or("")
}

fn code_file(p: &str) -> bool {
    CODE_EXT.contains(&ext(p))
}

static CODE_GENERATED: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\bcode generated\b.*\bdo not edit\b").unwrap());

/// The generated-file marker in a file's first lines, if any. Only comment lines count.
pub fn generated_marker(head: &str) -> Option<&'static str> {
    for l in head.lines().take(40) {
        let t = l.trim_start();
        let comment = ["//", "#", "/*", "*", "--", "<!--", "\"\"\""].iter().any(|p| t.starts_with(p));
        if !comment {
            continue;
        }
        if CODE_GENERATED.is_match(t) {
            return Some("code-generated");
        }
        if t.contains("@generated") {
            return Some("at-generated");
        }
    }
    None
}

static LITERAL: Lazy<Regex> = Lazy::new(|| Regex::new(r#""([^"\\\n]{1,200})"|'([^'\\\n]{1,200})'"#).unwrap());

/// The path pattern a file stands for: `schema/invoice.json` → `schema/*.json` (as the learner's
/// `glob_for`).
pub fn glob_for(rel: &str) -> Option<String> {
    let (dir, file) = rel.rsplit_once('/').unwrap_or(("", rel));
    let e = file.rsplit_once('.').map(|(_, e)| e)?;
    Some(if dir.is_empty() { format!("*.{e}") } else { format!("{dir}/*.{e}") })
}

/// A glob Adage may put in an edit trigger: repository-relative, plain characters.
pub fn safe_glob(g: &str) -> bool {
    !g.is_empty()
        && g.len() <= 120
        && !g.starts_with('/')
        && !g.contains("..")
        && g.chars().all(|c| c.is_ascii_alphanumeric() || "_./*-".contains(c))
}

fn glob_matches(glob: &str, all: &[String]) -> Vec<String> {
    // Only the shapes built here: `dir/*.ext`, `*.ext`, or a plain path.
    match glob.rsplit_once("*.") {
        Some((dir, e)) => all
            .iter()
            .filter(|p| {
                let (d, _) = p.rsplit_once('/').unwrap_or(("", p));
                d == dir.trim_end_matches('/') && ext(p) == e
            })
            .cloned()
            .collect(),
        None => all.iter().filter(|p| *p == glob).cloned().collect(),
    }
}

/// Inputs and outputs a regenerating entry declares: Makefile prerequisites if it has any; else
/// the paths its generator script names (data files and `dir` + `"*.ext"` pairs as inputs, code
/// files as outputs).
fn generator_links(entries: &[Entry], files: &RepoFiles, all: &[String]) -> Vec<GeneratorLink> {
    let mut out = Vec::new();
    for e in entries {
        let (Some(cmd), Some(effect)) = (&e.command, e.effect()) else { continue };
        if effect != Effect::Regenerate || e.command_effect != Some(Effect::Regenerate) {
            continue;
        }
        // Declared: glob or file prerequisites that are not themselves targets.
        let targets: BTreeSet<&str> =
            entries.iter().filter(|x| x.source == e.source).map(|x| x.name.as_str()).collect();
        let mut inputs: BTreeSet<String> = BTreeSet::new();
        let mut outputs: BTreeSet<String> = BTreeSet::new();
        let mut from_rules: Vec<&str> = e.prereqs.iter().map(String::as_str).collect();
        // `gen: models.py` and `models.py: schema/*.json`: the file rule's prerequisites are
        // inputs, and the file it makes is an output.
        for p in &e.prereqs {
            if let Some(x) = entries.iter().find(|x| x.source == e.source && &x.name == p) {
                from_rules.extend(x.prereqs.iter().map(String::as_str));
                if code_file(p) && files.is_file(p) {
                    outputs.insert(p.clone());
                }
            }
        }
        for p in from_rules {
            if targets.contains(p) || !safe_glob(p) {
                continue;
            }
            if p.contains('*') {
                if !glob_matches(p, all).is_empty() {
                    inputs.insert(p.to_string());
                }
            } else if files.is_file(p) {
                let g = if DATA_EXT.contains(&ext(p)) {
                    glob_for(p).unwrap_or_else(|| p.to_string())
                } else {
                    p.to_string()
                };
                inputs.insert(g);
            }
        }
        let inputs_from = if !inputs.is_empty() {
            "declared"
        } else {
            for script in e.scripts() {
                let Some(text) = files.read(&script) else { continue };
                let lits: BTreeSet<String> = LITERAL
                    .captures_iter(&text)
                    .filter_map(|c| c.get(1).or_else(|| c.get(2)).map(|m| m.as_str().to_string()))
                    .collect();
                // `"*.json"`, or `".csv"` as in `f.endsWith(".csv")`.
                let exts: BTreeSet<&str> = lits
                    .iter()
                    .filter_map(|l| l.strip_prefix("*.").or_else(|| l.strip_prefix('.')))
                    .filter(|e| DATA_EXT.contains(e))
                    .collect();
                for l in &lits {
                    let rel = l.strip_prefix("./").unwrap_or(l);
                    if !safe_rel(rel) || rel == script {
                        continue;
                    }
                    if files.is_dir(rel) {
                        for x in &exts {
                            let g = format!("{rel}/*.{x}");
                            if safe_glob(&g) && !glob_matches(&g, all).is_empty() {
                                inputs.insert(g);
                            }
                        }
                    } else if DATA_EXT.contains(&ext(rel)) && files.is_file(rel) {
                        if let Some(g) = glob_for(rel) {
                            inputs.insert(g);
                        }
                    } else if CODE_EXT.contains(&ext(rel))
                        && rel.contains('/')
                        && rel.split('/').any(|s| s.contains("generated") || s == "gen")
                        && files.is_file(rel)
                    {
                        outputs.insert(rel.to_string());
                    }
                }
            }
            "script"
        };
        // More than three input patterns is not one generator's input: too broad to direct.
        if inputs.is_empty() || inputs.len() > 3 {
            continue;
        }
        out.push(GeneratorLink {
            source: e.source,
            entry: e.name.clone(),
            command: cmd.clone(),
            inputs: inputs.into_iter().collect(),
            inputs_from: inputs_from.into(),
            outputs: outputs.into_iter().collect(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makefile_variables_prerequisites_and_recipes() {
        let mk = "PYTHON ?= python3\nSRC := schema\n.PHONY: test gen\n\ntest: gen\n\t@ACME_ENV=test $(PYTHON) -m pytest -q\n\ngen: app/models.py\napp/models.py: $(wildcard $(SRC)/*.json)\n\t$(PYTHON) tools/gen.py --out $@\nclean:\n\trm -rf build\nshell:\n\techo $(shell whoami)\n";
        let (rules, phony) = parse_makefile(mk);
        assert!(phony.contains("test") && phony.contains("gen"));
        let model = rules.iter().find(|r| r.targets == ["app/models.py"]).unwrap();
        assert_eq!(model.prereqs, vec!["schema/*.json"]);
        assert_eq!(model.recipe, vec!["python3 tools/gen.py --out app/models.py"]);
        let body = make_body("test", &rules, &mut BTreeSet::new(), 0).unwrap();
        assert_eq!(body, vec!["python3 tools/gen.py --out app/models.py", "ACME_ENV=test python3 -m pytest -q"]);
        let shell = rules.iter().find(|r| r.targets == ["shell"]).unwrap();
        assert!(shell.recipe[0].contains("$("), "an unknown function stays unexpanded: {:?}", shell.recipe);
    }

    #[test]
    fn npm_scripts_expand_hooks_and_references() {
        let s: BTreeMap<String, String> = [
            ("pretest", "node tools/check.mjs"),
            ("test", "npm run lint && jest"),
            ("lint", "eslint ."),
            ("loop", "npm run loop"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        let b = script_body("test", &s, &mut BTreeSet::new(), 0).unwrap();
        assert_eq!(b, vec!["node tools/check.mjs", "eslint .", "jest"]);
        assert!(script_body("loop", &s, &mut BTreeSet::new(), 0).is_err());
    }

    #[test]
    fn workflow_run_steps() {
        let y = "jobs:\n  t:\n    steps:\n      - run: cargo test --workspace\n      - name: x\n        run: |\n          make lint\n          make test\n      - run: \"npm ci\"\n";
        assert_eq!(run_steps(y), vec!["cargo test --workspace", "make lint", "make test", "npm ci"]);
    }

    #[test]
    fn markers() {
        assert_eq!(generated_marker("// Code generated by stringer; DO NOT EDIT.\npackage x"), Some("code-generated"));
        assert_eq!(generated_marker("# @generated by tool\n"), Some("at-generated"));
        assert_eq!(generated_marker("const s = \"@generated\";\n"), None);
    }
}
