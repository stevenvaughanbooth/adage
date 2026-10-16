//! `cargo xtask bump <version> [--date <YYYY-MM-DD>] [--no-changelog]` and
//! `cargo xtask release-check [--require-dist] [--dist <path>]`: one command writes a release's
//! version everywhere it is kept, and one check fails while those places disagree or the release
//! workflow is not what dist generates (onboarding plan O2).
//!
//! The version lives in [`VERSIONED`]: the workspace manifest (the source of truth), the lockfile's
//! entries for the members that inherit it, the Claude Code plugin's manifest, the marketplace
//! (its metadata and every plugin entry), and the npm package templates. `bump` rewrites each in
//! place (only the version strings, so every other byte stays), renames CHANGELOG.md's Unreleased
//! heading to `## <version> - <date>` (dist reads that section as the GitHub Release notes), then
//! runs the version check and `node scripts/check-plugin.mjs`. It computes every file before it
//! writes any, so a refusal changes nothing.
//!
//! `release-check` runs the version check, then the release workflow's static facts (Homebrew is
//! off the release until macOS signing exists; a pull request only plans; every custom publish job
//! dist-workspace.toml names is a job and a file; the documented SBOM patch is applied), then, when
//! dist is found (`--dist`, or `dist` on `PATH`; `--require-dist` makes it mandatory, as CI does),
//! regenerates `release.yml` in a scratch tree of links to this one and compares it, after the
//! one documented patch, with the committed file, and plans the release to confirm no Homebrew
//! formula is built.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const CARGO_TOML: &str = "Cargo.toml";
pub const CARGO_LOCK: &str = "Cargo.lock";
pub const CHANGELOG: &str = "CHANGELOG.md";
pub const RELEASE_YML: &str = ".github/workflows/release.yml";
pub const DIST_CONFIG: &str = "dist-workspace.toml";

/// The JSON files that carry the release version, and the JSON pointers of every `version` key in
/// each: a `version` key the list does not name fails the check, so a new one is a decision here.
/// `*` stands for every element of an array.
pub const VERSIONED: &[(&str, &[&str])] = &[
    ("plugin/claude-code/.claude-plugin/plugin.json", &["/version"]),
    (".claude-plugin/marketplace.json", &["/metadata/version", "/plugins/*/version"]),
    ("npm/cli/package.json", &["/version"]),
    ("npm/platform/package.json", &["/version"]),
];

/// dist's template reads the SBOM step's output as `output.paths`, which does not exist; the
/// committed workflow reads `outputs.paths` (dist-workspace.toml documents it).
const SBOM_UNPATCHED: &str = "${{ steps.cargo-cyclonedx.output.paths }}";
const SBOM_PATCHED: &str = "${{ steps.cargo-cyclonedx.outputs.paths }}";

/// The jobs of the release workflow with Homebrew off: plan, build, host, the two custom publish
/// jobs, announce.
const RELEASE_JOBS: &[&str] = &[
    "plan",
    "build-local-artifacts",
    "build-global-artifacts",
    "host",
    "custom-publish-npm",
    "custom-publish-marketplace",
    "announce",
];

// ---------------------------------------------------------------------------------------------
// The commands

pub fn run_bump(root: &Path, args: &[String]) -> Result<()> {
    let usage = "usage: cargo xtask bump <version> [--date <YYYY-MM-DD>] [--no-changelog]";
    let mut version = None;
    let mut date = None;
    let mut changelog = true;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--date" => date = Some(it.next().ok_or_else(|| anyhow!("--date needs a value\n{usage}"))?.clone()),
            "--no-changelog" => changelog = false,
            s if s.starts_with('-') => bail!("unknown option {s}\n{usage}"),
            s if version.is_none() => version = Some(s.to_string()),
            s => bail!("unexpected argument {s}\n{usage}"),
        }
    }
    let version = version.ok_or_else(|| anyhow!(usage))?;
    let date = match date {
        Some(d) => d,
        None => today(),
    };
    let changed = bump(root, &version, &date, changelog)?;
    for f in &changed {
        println!("wrote {f}");
    }
    let problems = version_problems(root)?;
    if !problems.is_empty() {
        bail!("after the bump the versions still disagree:\n  {}", problems.join("\n  "));
    }
    match Command::new("node").arg("scripts/check-plugin.mjs").current_dir(root).status() {
        Ok(s) if s.success() => {}
        Ok(_) => bail!("node scripts/check-plugin.mjs failed after the bump"),
        Err(_) => println!("node not found: run `node scripts/check-plugin.mjs` before tagging"),
    }
    println!(
        "adage {version}: next, commit (git commit -s), `git tag v{version}`, and `git push origin HEAD v{version}` (docs/release.md)"
    );
    Ok(())
}

pub fn run_check(root: &Path, args: &[String]) -> Result<()> {
    let usage = "usage: cargo xtask release-check [--require-dist] [--dist <path>]";
    let mut require = false;
    let mut dist: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--require-dist" => require = true,
            "--dist" => dist = Some(it.next().ok_or_else(|| anyhow!("--dist needs a path\n{usage}"))?.into()),
            s => bail!("unexpected argument {s}\n{usage}"),
        }
    }
    let mut problems = version_problems(root)?;
    problems.extend(workflow_problems(root)?);
    if !problems.is_empty() {
        bail!("release-check:\n  {}", problems.join("\n  "));
    }
    println!("release-check: versions agree ({}), and release.yml has no Homebrew job", workspace_version(root)?);
    let dist = dist.or_else(|| on_path("dist"));
    match dist {
        Some(d) => {
            dist_problems(root, &d).map(|p| problems.extend(p))?;
            if !problems.is_empty() {
                bail!("release-check:\n  {}", problems.join("\n  "));
            }
            println!(
                "release-check: {} is what {} generates, with the SBOM patch; the plan builds no Homebrew formula",
                RELEASE_YML,
                d.display()
            );
        }
        None if require => {
            bail!("release-check: dist is not on PATH (install the version {DIST_CONFIG} pins, or pass --dist)")
        }
        None => println!(
            "release-check: dist not found, so release.yml was not regenerated (CI runs it with --require-dist)"
        ),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Versions

/// `MAJOR.MINOR.PATCH` with an optional `-prerelease` (dist and npm read it; no build metadata).
pub fn valid_version(v: &str) -> bool {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let nums: Vec<&str> = core.split('.').collect();
    let number =
        |n: &&str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && (n.len() == 1 || !n.starts_with('0'));
    nums.len() == 3
        && nums.iter().all(number)
        && pre.is_none_or(|p| {
            !p.is_empty()
                && p.split('.').all(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        })
}

/// The `version = "…"` line of `[workspace.package]`: its byte range in the text, and the value.
fn workspace_version_at(toml: &str) -> Result<(std::ops::Range<usize>, String)> {
    let mut in_section = false;
    let mut at = 0;
    for line in toml.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with('[') {
            in_section = t == "[workspace.package]";
        } else if in_section {
            if let Some(rest) = t.strip_prefix("version") {
                let value = rest
                    .trim_start()
                    .strip_prefix('=')
                    .map(str::trim)
                    .and_then(|v| v.strip_prefix('"')?.strip_suffix('"'));
                let value =
                    value.ok_or_else(|| anyhow!("{CARGO_TOML}: [workspace.package] version is not a plain string"))?;
                return Ok((at..at + line.len(), value.to_string()));
            }
        }
        at += line.len();
    }
    bail!("{CARGO_TOML}: no version in [workspace.package]")
}

pub fn workspace_version(root: &Path) -> Result<String> {
    Ok(workspace_version_at(&read(root, CARGO_TOML)?)?.1)
}

/// The workspace members whose manifests say `version.workspace = true`, by package name.
fn inheriting_members(root: &Path) -> Result<BTreeSet<String>> {
    let toml = read(root, CARGO_TOML)?;
    let list = toml
        .lines()
        .find_map(|l| l.trim().strip_prefix("members").map(str::trim))
        .and_then(|r| r.strip_prefix('='))
        .ok_or_else(|| anyhow!("{CARGO_TOML}: no workspace members"))?;
    let mut dirs = Vec::new();
    for m in list.trim().trim_start_matches('[').trim_end_matches(']').split(',') {
        let m = m.trim().trim_matches('"');
        if m.is_empty() {
            continue;
        }
        match m.strip_suffix("/*") {
            Some(parent) => {
                let mut found: Vec<PathBuf> = std::fs::read_dir(root.join(parent))
                    .with_context(|| format!("reading {parent}"))?
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.join("Cargo.toml").is_file())
                    .collect();
                found.sort();
                dirs.extend(found);
            }
            None => dirs.push(root.join(m)),
        }
    }
    let mut names = BTreeSet::new();
    for d in dirs {
        let manifest =
            std::fs::read_to_string(d.join("Cargo.toml")).with_context(|| format!("reading {}", d.display()))?;
        let name = manifest
            .lines()
            .find_map(|l| l.trim().strip_prefix("name").and_then(|r| r.trim_start().strip_prefix('=')))
            .map(|v| v.trim().trim_matches('"').to_string())
            .ok_or_else(|| anyhow!("{}: no package name", d.display()))?;
        if manifest.lines().any(|l| l.trim().replace(' ', "") == "version.workspace=true") {
            names.insert(name);
        }
    }
    Ok(names)
}

/// One `[[package]]` entry of the lockfile: its name, its version, the byte range of its
/// `version = "…"` line, and whether it names a `source` (a registry or git dependency).
#[derive(Default)]
struct LockEntry {
    name: String,
    version: Option<(String, std::ops::Range<usize>)>,
    sourced: bool,
}

/// The lockfile's `[[package]]` entries without a `source` (this workspace's members): each
/// name, its version, and the byte range of its `version = "…"` line.
fn lock_members(lock: &str) -> Vec<(String, String, std::ops::Range<usize>)> {
    let mut entries: Vec<LockEntry> = Vec::new();
    let mut open = false;
    let mut at = 0;
    for line in lock.split_inclusive('\n') {
        let t = line.trim();
        let value = |key: &str| {
            t.strip_prefix(key)
                .and_then(|r| r.trim_start().strip_prefix('='))
                .map(|v| v.trim().trim_matches('"').to_string())
        };
        if t == "[[package]]" {
            entries.push(LockEntry::default());
            open = true;
        } else if t.starts_with('[') {
            open = false;
        } else if let Some(e) = entries.last_mut().filter(|_| open) {
            if let Some(n) = value("name") {
                e.name = n;
            } else if let Some(v) = value("version") {
                e.version = Some((v, at..at + line.len()));
            } else if t.starts_with("source") {
                e.sourced = true;
            }
        }
        at += line.len();
    }
    entries.into_iter().filter(|e| !e.sourced).filter_map(|e| e.version.map(|(v, r)| (e.name, v, r))).collect()
}

/// Every `"version": "…"` pair in a JSON text: the byte range of the value (quotes included).
fn json_version_spans(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let key = "\"version\"";
    let mut from = 0;
    while let Some(k) = text[from..].find(key) {
        let after = from + k + key.len();
        from = after;
        let rest = &text[after..];
        let colon = rest.len() - rest.trim_start().len();
        let Some(r) = rest[colon..].strip_prefix(':') else { continue };
        let start = after + colon + 1 + (r.len() - r.trim_start().len());
        let r = &text[start..];
        if let Some(body) = r.strip_prefix('"') {
            if let Some(end) = body.find('"') {
                out.push(start..start + end + 2);
            }
        }
    }
    out
}

/// The values at the version pointers of one JSON document (`*` expands an array).
fn json_versions(doc: &Value, pointers: &[&str]) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for p in pointers {
        match p.split_once("/*") {
            Some((array, rest)) => {
                for (i, item) in doc.pointer(array).and_then(Value::as_array).into_iter().flatten().enumerate() {
                    out.push((
                        format!("{array}/{i}{rest}"),
                        item.pointer(rest).and_then(Value::as_str).map(str::to_string),
                    ));
                }
            }
            None => out.push((p.to_string(), doc.pointer(p).and_then(Value::as_str).map(str::to_string))),
        }
    }
    out
}

/// Where the versions disagree with `[workspace.package]`, one line each; empty when they agree.
pub fn version_problems(root: &Path) -> Result<Vec<String>> {
    let version = workspace_version(root)?;
    let mut problems = Vec::new();
    if !valid_version(&version) {
        problems.push(format!("{CARGO_TOML}: version {version:?} is not MAJOR.MINOR.PATCH[-pre]"));
    }
    let members = inheriting_members(root)?;
    let lock = lock_members(&read(root, CARGO_LOCK)?);
    for name in &members {
        match lock.iter().find(|(n, _, _)| n == name) {
            Some((_, v, _)) if *v == version => {}
            Some((_, v, _)) => problems.push(format!("{CARGO_LOCK}: {name} is {v}, the workspace is {version}")),
            None => problems.push(format!("{CARGO_LOCK}: no entry for the workspace member {name}")),
        }
    }
    for (file, pointers) in VERSIONED {
        let text = read(root, file)?;
        let doc: Value = serde_json::from_str(&text).with_context(|| format!("parsing {file}"))?;
        let found = json_versions(&doc, pointers);
        for (at, v) in &found {
            match v {
                Some(v) if *v == version => {}
                Some(v) => problems.push(format!("{file}: {at} is {v}, the workspace is {version}")),
                None => problems.push(format!("{file}: no version at {at}")),
            }
        }
        let keys = json_version_spans(&text).len();
        if keys != found.len() {
            problems.push(format!(
                "{file}: {keys} \"version\" keys, but the release keeps {} ({}); name the new one in xtask/src/release.rs VERSIONED",
                found.len(),
                pointers.join(", ")
            ));
        }
    }
    Ok(problems)
}

// ---------------------------------------------------------------------------------------------
// The bump

/// Write `version` into every place [`version_problems`] reads, and (with `changelog`) rename the
/// Unreleased heading. Returns the files written, in order. Writes nothing when it refuses.
pub fn bump(root: &Path, version: &str, date: &str, changelog: bool) -> Result<Vec<String>> {
    if !valid_version(version) {
        bail!("{version:?} is not a version (MAJOR.MINOR.PATCH, optionally -pre; no leading v)");
    }
    if !valid_date(date) {
        bail!("{date:?} is not a date (YYYY-MM-DD)");
    }
    let mut writes: Vec<(String, String)> = Vec::new();

    let toml = read(root, CARGO_TOML)?;
    let (range, _) = workspace_version_at(&toml)?;
    let line = &toml[range.clone()];
    let indent = &line[..line.len() - line.trim_start().len()];
    let eol = if line.ends_with('\n') { "\n" } else { "" };
    writes.push((
        CARGO_TOML.into(),
        format!("{}{indent}version = \"{version}\"{eol}{}", &toml[..range.start], &toml[range.end..]),
    ));

    let members = inheriting_members(root)?;
    let lock = read(root, CARGO_LOCK)?;
    let entries = lock_members(&lock);
    let mut spans: Vec<std::ops::Range<usize>> = Vec::new();
    for name in &members {
        let (_, _, r) = entries
            .iter()
            .find(|(n, _, _)| n == name)
            .ok_or_else(|| anyhow!("{CARGO_LOCK}: no entry for the workspace member {name}; run cargo build first"))?;
        spans.push(r.clone());
    }
    let replace_lines = |text: &str, mut spans: Vec<std::ops::Range<usize>>, line: &dyn Fn(&str) -> String| {
        spans.sort_by_key(|r| r.start);
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for r in spans {
            out.push_str(&text[at..r.start]);
            out.push_str(&line(&text[r.clone()]));
            at = r.end;
        }
        out.push_str(&text[at..]);
        out
    };
    let set_lock = |old: &str| format!("version = \"{version}\"{}", if old.ends_with('\n') { "\n" } else { "" });
    writes.push((CARGO_LOCK.into(), replace_lines(&lock, spans, &set_lock)));

    for (file, pointers) in VERSIONED {
        let text = read(root, file)?;
        let doc: Value = serde_json::from_str(&text).with_context(|| format!("parsing {file}"))?;
        let expected = json_versions(&doc, pointers).len();
        let spans = json_version_spans(&text);
        if spans.len() != expected {
            bail!("{file}: {} \"version\" keys, but the release keeps {expected}; name the new one in xtask/src/release.rs VERSIONED", spans.len());
        }
        let quoted = format!("\"{version}\"");
        writes.push((file.to_string(), replace_lines(&text, spans, &|_| quoted.clone())));
    }

    if changelog {
        let text = read(root, CHANGELOG)?;
        let mut at = 0;
        let mut found = None;
        for line in text.split_inclusive('\n') {
            let t = line.trim_end();
            if let Some(h) = t.strip_prefix("## ") {
                if h == format!("{version} - {date}") || h.starts_with(&format!("{version} - 2")) {
                    bail!("{CHANGELOG}: {version} is already released (`{t}`)");
                }
                let unreleased = h == "Unreleased" || h == "[Unreleased]" || h.ends_with(" - Unreleased");
                if unreleased && found.is_none() {
                    found = Some((at, t.len()));
                }
            }
            at += line.len();
        }
        let Some((start, heading_len)) = found else {
            bail!("{CHANGELOG}: no Unreleased section; write what {version} changes there first (it becomes the GitHub Release notes), or pass --no-changelog");
        };
        let heading = format!("## {version} - {date}");
        writes.push((CHANGELOG.into(), format!("{}{heading}{}", &text[..start], &text[start + heading_len..])));
    }

    let mut written = Vec::new();
    for (file, text) in writes {
        if read(root, &file)? != text {
            std::fs::write(root.join(&file), text).with_context(|| format!("writing {file}"))?;
            written.push(file);
        }
    }
    Ok(written)
}

fn valid_date(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && d.bytes().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && (1..=12).contains(&d[5..7].parse::<u32>().unwrap_or(0))
        && (1..=31).contains(&d[8..10].parse::<u32>().unwrap_or(0))
}

/// Today's date in UTC, `YYYY-MM-DD` (Howard Hinnant's civil-from-days).
fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

// ---------------------------------------------------------------------------------------------
// The release workflow

/// A `key = [..]` or `key = ".."` line of dist-workspace.toml's `[dist]`, as its strings.
fn dist_setting(toml: &str, key: &str) -> Option<Vec<String>> {
    let line = toml
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .find(|l| l.split('=').next().is_some_and(|k| k.trim() == key))?;
    let value = line.split_once('=')?.1.trim();
    Some(
        value
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|s| s.trim().trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

/// The top-level job names of a workflow, in order.
fn jobs(yml: &str) -> Vec<String> {
    let mut in_jobs = false;
    let mut out = Vec::new();
    for line in yml.lines() {
        if !line.starts_with(' ') && !line.starts_with('#') && !line.is_empty() {
            in_jobs = line.trim_end() == "jobs:";
            continue;
        }
        if in_jobs {
            if let Some(name) = line.strip_prefix("  ").and_then(|l| l.strip_suffix(':')) {
                if !name.starts_with(' ') && !name.starts_with('#') {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

/// The release workflow's static facts, one line per problem: no Homebrew anywhere (it waits for
/// macOS signing; a missing tap token must never fail a tag), a pull request plans and builds
/// nothing, every custom publish job is a job and a file, the SBOM patch is applied, and the dist
/// version the workflow installs is the one dist-workspace.toml pins.
pub fn workflow_problems(root: &Path) -> Result<Vec<String>> {
    let yml = read(root, RELEASE_YML)?;
    let toml = read(root, DIST_CONFIG)?;
    let mut problems = Vec::new();
    if yml.to_lowercase().contains("homebrew") {
        problems.push(format!(
            "{RELEASE_YML}: names Homebrew; it is off the release until macOS signing exists (docs/release.md)"
        ));
    }
    let installers = dist_setting(&toml, "installers").unwrap_or_default();
    if installers.iter().any(|i| i == "homebrew") {
        problems.push(format!("{DIST_CONFIG}: installers include homebrew"));
    }
    if toml.lines().any(|l| l.trim_start().starts_with("tap ") || l.trim_start().starts_with("formula ")) {
        problems.push(format!("{DIST_CONFIG}: a Homebrew tap or formula is set"));
    }
    for m in member_manifests(root)? {
        let text = std::fs::read_to_string(&m)?;
        if text.lines().any(|l| l.trim_start().starts_with("formula")) {
            let rel = m.strip_prefix(root).unwrap_or(&m).display().to_string();
            problems
                .push(format!("{rel}: a per-package Homebrew formula makes dist build a .rb even with Homebrew off"));
        }
    }
    if dist_setting(&toml, "pr-run-mode").unwrap_or_default() != ["plan"] {
        problems.push(format!(
            "{DIST_CONFIG}: pr-run-mode must be \"plan\" (a pull request plans the release and builds nothing)"
        ));
    }
    let publish = dist_setting(&toml, "publish-jobs").unwrap_or_default();
    for job in &publish {
        let Some(name) = job.strip_prefix("./") else {
            problems.push(format!("{DIST_CONFIG}: publish job {job} is not a custom ./ job"));
            continue;
        };
        let uses = format!("uses: ./.github/workflows/{name}.yml");
        if !yml.contains(&format!("  custom-{name}:\n")) || !yml.contains(&uses) {
            problems.push(format!("{RELEASE_YML}: no custom-{name} job running {name}.yml (run dist generate)"));
        }
        if !root.join(format!(".github/workflows/{name}.yml")).is_file() {
            problems.push(format!(".github/workflows/{name}.yml: the publish job {job} has no workflow file"));
        }
    }
    let found = jobs(&yml);
    if found != RELEASE_JOBS {
        problems.push(format!("{RELEASE_YML}: jobs are {}, not {}", found.join(", "), RELEASE_JOBS.join(", ")));
    }
    if !yml.contains("  pull_request:\n") || !yml.contains("|| 'plan' }}") {
        problems.push(format!("{RELEASE_YML}: a pull request must run `dist plan`"));
    }
    if yml.contains(SBOM_UNPATCHED) || !yml.contains(SBOM_PATCHED) {
        problems.push(format!("{RELEASE_YML}: the SBOM patch is not applied (`{SBOM_PATCHED}`, see {DIST_CONFIG})"));
    }
    let pinned = dist_setting(&toml, "cargo-dist-version").and_then(|v| v.into_iter().next()).unwrap_or_default();
    if !yml.contains(&format!("cargo-dist/releases/download/v{pinned}/")) {
        problems.push(format!("{RELEASE_YML}: does not install dist {pinned}, the version {DIST_CONFIG} pins"));
    }
    Ok(problems)
}

fn member_manifests(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(root.join("crates")).context("reading crates/")?.flatten() {
        let m = e.path().join("Cargo.toml");
        if m.is_file() {
            out.push(m);
        }
    }
    out.push(root.join("xtask/Cargo.toml"));
    out.sort();
    Ok(out)
}

/// Regenerate the release workflow with `dist` in a scratch tree (links to this one, with
/// dist-workspace.toml's `allow-dirty` dropped so dist writes it), apply the documented patch, and
/// compare; then plan the release there and look for a Homebrew formula.
#[cfg(unix)]
fn dist_problems(root: &Path, dist: &Path) -> Result<Vec<String>> {
    let scratch = Scratch::new("release-check")?;
    for e in std::fs::read_dir(root)?.flatten() {
        let name = e.file_name();
        if [".git", ".github", "target", DIST_CONFIG].iter().any(|n| name == *n) {
            continue;
        }
        std::os::unix::fs::symlink(e.path(), scratch.0.join(&name))?;
    }
    let config: String = read(root, DIST_CONFIG)?
        .lines()
        .filter(|l| !l.trim_start().starts_with("allow-dirty"))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(scratch.0.join(DIST_CONFIG), config)?;
    let workflows = scratch.0.join(".github/workflows");
    std::fs::create_dir_all(&workflows)?;
    for e in std::fs::read_dir(root.join(".github/workflows"))?.flatten() {
        if e.file_name() != "release.yml" {
            std::fs::copy(e.path(), workflows.join(e.file_name()))?;
        }
    }
    let run = |args: &[&str]| -> Result<String> {
        let out = Command::new(dist)
            .args(args)
            .current_dir(&scratch.0)
            .output()
            .with_context(|| format!("running {}", dist.display()))?;
        if !out.status.success() {
            bail!("dist {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    run(&["generate", "--mode", "ci"])?;
    let generated = std::fs::read_to_string(workflows.join("release.yml")).context("dist wrote no release.yml")?;
    let mut problems = Vec::new();
    if generated.matches(SBOM_UNPATCHED).count() != 1 {
        problems.push(format!(
            "dist's release.yml no longer reads `{SBOM_UNPATCHED}` once; check whether the SBOM patch in {DIST_CONFIG} is still needed"
        ));
    }
    let expected = generated.replace(SBOM_UNPATCHED, SBOM_PATCHED);
    let committed = read(root, RELEASE_YML)?;
    if expected != committed {
        let at = expected
            .lines()
            .zip(committed.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(expected.lines().count().min(committed.lines().count()));
        problems.push(format!(
            "{RELEASE_YML} differs from what dist generates from {DIST_CONFIG} beyond the SBOM patch, first at line {}: generated `{}`, committed `{}` (run dist generate, re-apply the patch)",
            at + 1,
            expected.lines().nth(at).unwrap_or("<end>").trim(),
            committed.lines().nth(at).unwrap_or("<end>").trim()
        ));
    }
    let plan: Value = serde_json::from_str(&run(&["plan", "--output-format=json"])?).context("parsing dist plan")?;
    let formulas: Vec<&str> = plan["artifacts"]
        .as_object()
        .into_iter()
        .flat_map(|a| a.keys())
        .map(String::as_str)
        .filter(|k| k.ends_with(".rb"))
        .collect();
    if !formulas.is_empty() {
        problems.push(format!("dist plan builds Homebrew formulas: {}", formulas.join(", ")));
    }
    Ok(problems)
}

#[cfg(not(unix))]
fn dist_problems(_root: &Path, _dist: &Path) -> Result<Vec<String>> {
    bail!("release-check regenerates release.yml through a tree of symbolic links: run it on Linux or macOS")
}

/// A directory under the system's temporary directory, removed (never following its links) on drop.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Result<Scratch> {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("xtask-{tag}-{}-{nanos}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(name)).find(|f| f.is_file()))
}

fn read(root: &Path, rel: &str) -> Result<String> {
    std::fs::read_to_string(root.join(rel)).with_context(|| format!("reading {rel}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn repo() -> PathBuf {
        crate::root()
    }

    /// A scratch copy of the files the release keeps the version in (the member manifests too,
    /// which `bump` reads but never writes), one file it never touches, and a CHANGELOG.md of its
    /// own (the real one changes with every pull request).
    fn scratch_release_tree() -> Scratch {
        let s = Scratch::new("bump-test").unwrap();
        let root = repo();
        let mut files: Vec<String> = vec![CARGO_TOML.into(), CARGO_LOCK.into(), "README.md".into()];
        files.extend(VERSIONED.iter().map(|(f, _)| f.to_string()));
        for m in member_manifests(&root).unwrap() {
            files.push(m.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"));
        }
        for f in files {
            let to = s.0.join(&f);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(root.join(&f), to).unwrap();
        }
        std::fs::write(
            s.0.join(CHANGELOG),
            "# Changelog\n\nPreamble.\n\n## 0.1.0 - Unreleased\n\n### Installing and updating\n\n- **A change.** Words.\n\n## 0.0.9 - 2026-01-02\n\n- Older.\n",
        )
        .unwrap();
        s
    }

    fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.insert(
                        p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"),
                        std::fs::read(&p).unwrap(),
                    );
                }
            }
        }
        out
    }

    fn changed(a: &BTreeMap<String, Vec<u8>>, b: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
        a.keys().filter(|k| a.get(*k) != b.get(*k)).cloned().collect()
    }

    /// Every place the release keeps its version agrees with Cargo.toml (what `release-check`
    /// runs first; check-plugin.mjs keeps its own three equalities beside it).
    #[test]
    fn the_release_versions_agree() {
        let problems = version_problems(&repo()).unwrap();
        assert!(problems.is_empty(), "run `cargo xtask bump <version>`:\n  {}", problems.join("\n  "));
        assert!(!inheriting_members(&repo()).unwrap().is_empty());
    }

    /// T-O2-03: the release workflow on a pull request only plans, and no job, installer or
    /// formula is Homebrew's: a missing tap token cannot fail a tag.
    #[test]
    fn the_release_workflow_plans_on_a_pull_request_with_no_homebrew() {
        let problems = workflow_problems(&repo()).unwrap();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        let yml = read(&repo(), RELEASE_YML).unwrap();
        assert_eq!(yml.to_lowercase().matches("homebrew").count(), 0);
        assert_eq!(jobs(&yml), RELEASE_JOBS);
    }

    /// T-O2-01: bump, check, bump back. The bump touches exactly the version files and the
    /// CHANGELOG heading; the way back restores every byte of the version files.
    #[test]
    fn bump_round_trips_touching_the_named_files_only() {
        let s = scratch_release_tree();
        let before = snapshot(&s.0);
        let from = workspace_version(&s.0).unwrap();
        assert!(version_problems(&s.0).unwrap().is_empty());

        // The next patch version, whatever the workspace is at.
        let (core, _) = from.split_once('-').unwrap_or((&from, ""));
        let parts: Vec<u64> = core.split('.').map(|n| n.parse().unwrap()).collect();
        let to = format!("{}.{}.{}", parts[0], parts[1], parts[2] + 1);
        let written = bump(&s.0, &to, "2026-10-06", true).unwrap();
        let after = snapshot(&s.0);
        let mut want: Vec<String> = vec![CARGO_TOML.into(), CARGO_LOCK.into(), CHANGELOG.into()];
        want.extend(VERSIONED.iter().map(|(f, _)| f.to_string()));
        want.sort();
        assert_eq!(changed(&before, &after), want);
        let mut w = written.clone();
        w.sort();
        assert_eq!(w, want);
        assert_eq!(workspace_version(&s.0).unwrap(), to);
        assert!(version_problems(&s.0).unwrap().is_empty(), "{:?}", version_problems(&s.0));
        // Only version strings changed: every other line of every file is as it was.
        for f in &want {
            let (a, b) =
                (String::from_utf8_lossy(&before[f]).into_owned(), String::from_utf8_lossy(&after[f]).into_owned());
            assert_eq!(a.lines().count(), b.lines().count(), "{f}");
            for (x, y) in a.lines().zip(b.lines()) {
                if x != y {
                    let ok = (x.contains(&from) && y == x.replace(&from, &to))
                        || (x == "## 0.1.0 - Unreleased" && y == format!("## {to} - 2026-10-06"));
                    assert!(ok, "{f}: `{x}` became `{y}`");
                }
            }
        }
        let lock = String::from_utf8_lossy(&after[CARGO_LOCK]).into_owned();
        assert!(lock.contains(&format!("name = \"adage-cli\"\nversion = \"{to}\"\n")), "a member's lock entry moves");

        // Back: every version file is byte-identical; the CHANGELOG keeps its released heading.
        let back = bump(&s.0, &from, "2026-10-06", false).unwrap();
        assert!(!back.contains(&CHANGELOG.to_string()));
        let restored = snapshot(&s.0);
        assert_eq!(changed(&before, &restored), vec![CHANGELOG.to_string()]);
        assert!(version_problems(&s.0).unwrap().is_empty());
    }

    /// A bump that cannot be made writes nothing: a malformed version, a CHANGELOG with no
    /// Unreleased section, a version already released.
    #[test]
    fn bump_refuses_without_writing() {
        let s = scratch_release_tree();
        let before = snapshot(&s.0);
        for bad in ["v0.1.1", "0.1", "0.1.1+build", "01.1.1", "0.1.1-"] {
            assert!(bump(&s.0, bad, "2026-10-06", true).is_err(), "{bad}");
        }
        assert!(bump(&s.0, "0.1.1", "2026-13-01", true).is_err());
        std::fs::write(s.0.join(CHANGELOG), "# Changelog\n\n## 0.0.9 - 2026-01-02\n").unwrap();
        let e = bump(&s.0, "0.1.1", "2026-10-06", true).unwrap_err().to_string();
        assert!(e.contains("no Unreleased section"), "{e}");
        std::fs::write(s.0.join(CHANGELOG), "# Changelog\n\n## Unreleased\n\n## 0.1.1 - 2026-01-02\n").unwrap();
        let e = bump(&s.0, "0.1.1", "2026-10-06", true).unwrap_err().to_string();
        assert!(e.contains("already released"), "{e}");
        let mut now = snapshot(&s.0);
        now.remove(CHANGELOG);
        let mut was = before.clone();
        was.remove(CHANGELOG);
        assert_eq!(changed(&was, &now), Vec::<String>::new());
        // `## Unreleased` and `## [Unreleased]` are Unreleased too.
        std::fs::write(s.0.join(CHANGELOG), "# Changelog\n\n## [Unreleased]\n\n- x\n").unwrap();
        bump(&s.0, "0.2.0-rc.1", "2026-10-06", true).unwrap();
        assert_eq!(read(&s.0, CHANGELOG).unwrap(), "# Changelog\n\n## 0.2.0-rc.1 - 2026-10-06\n\n- x\n");
    }

    /// T-O2-02: the check fails on a drifted plugin.json version, naming the file and both
    /// versions; and on each other place the version is kept.
    #[test]
    fn release_check_fails_on_a_drifted_version() {
        let s = scratch_release_tree();
        let plugin = s.0.join(VERSIONED[0].0);
        let text = std::fs::read_to_string(&plugin).unwrap();
        let v = workspace_version(&s.0).unwrap();
        std::fs::write(&plugin, text.replacen(&format!("\"version\": \"{v}\""), "\"version\": \"9.9.9\"", 1)).unwrap();
        let p = version_problems(&s.0).unwrap();
        assert_eq!(p, vec![format!("{}: /version is 9.9.9, the workspace is {v}", VERSIONED[0].0)]);
        std::fs::write(&plugin, &text).unwrap();

        let market = s.0.join(VERSIONED[1].0);
        let m = std::fs::read_to_string(&market).unwrap();
        let mut doc: Value = serde_json::from_str(&m).unwrap();
        doc["plugins"][0]["version"] = "0.0.1".into();
        std::fs::write(&market, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        assert!(version_problems(&s.0).unwrap().iter().any(|p| p.contains("/plugins/0/version is 0.0.1")));
        doc["plugins"][0]["version"] = v.clone().into();
        doc["plugins"][0]["extra"] = serde_json::json!({ "version": v });
        std::fs::write(&market, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        assert!(
            version_problems(&s.0).unwrap().iter().any(|p| p.contains("name the new one")),
            "an unlisted version key"
        );
        std::fs::write(&market, &m).unwrap();

        let npm = s.0.join(VERSIONED[2].0);
        let n = std::fs::read_to_string(&npm).unwrap();
        std::fs::write(&npm, n.replacen(&format!("\"{v}\""), "\"0.0.0-dev\"", 1)).unwrap();
        assert!(version_problems(&s.0)
            .unwrap()
            .iter()
            .any(|p| p.starts_with("npm/cli/package.json: /version is 0.0.0-dev")));
        std::fs::write(&npm, &n).unwrap();

        let lock = read(&s.0, CARGO_LOCK).unwrap();
        std::fs::write(
            s.0.join(CARGO_LOCK),
            lock.replacen(
                &format!("name = \"adage-train\"\nversion = \"{v}\""),
                "name = \"adage-train\"\nversion = \"0.0.2\"",
                1,
            ),
        )
        .unwrap();
        assert_eq!(
            version_problems(&s.0).unwrap(),
            vec![format!("{CARGO_LOCK}: adage-train is 0.0.2, the workspace is {v}")]
        );
        std::fs::write(s.0.join(CARGO_LOCK), &lock).unwrap();
        assert!(version_problems(&s.0).unwrap().is_empty());
    }

    #[test]
    fn versions_dates_and_spans() {
        for ok in ["0.1.0", "1.20.300", "0.2.0-rc.1", "1.0.0-alpha-2"] {
            assert!(valid_version(ok), "{ok}");
        }
        assert!(valid_date("2026-10-06") && !valid_date("2026-1-06") && !valid_date("2026-10-32"));
        assert_eq!(today().len(), 10);
        assert!(valid_date(&today()));
        let text = "{\"version\" : \"1.0.0\", \"x\": {\"version\":\"2\"}, \"versions\": 3}";
        let spans = json_version_spans(text);
        assert_eq!(spans.iter().map(|r| &text[r.clone()]).collect::<Vec<_>>(), vec!["\"1.0.0\"", "\"2\""]);
    }
}
