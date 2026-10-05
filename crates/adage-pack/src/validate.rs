//! Strict reading and validation of a pack as untrusted input (Track C §1.2, §2.2).
//!
//! Three layers, each refusing the whole pack on the first failure:
//!
//! 1. **Files** ([`read_dir_files`], [`check_files`]): no symlinks or special files, only the
//!    allowlisted layout, at most [`MAX_FILES`] files of at most [`MAX_FILE_BYTES`] each, scripts at
//!    most [`MAX_SCRIPT_BYTES`] each and [`MAX_SCRIPTS_TOTAL`] together; every file UTF-8 with no
//!    NUL, no control character but tab and line breaks, and nothing invisible or
//!    direction-changing.
//! 2. **Schema** ([`parse`]): every struct refuses unknown fields; file names match ids; vendored
//!    scripts are named by their own SHA-256.
//! 3. **Semantics** ([`validate`]): safe names and ids, strict contracts, plain argv, repository-
//!    relative paths and globs, effects re-classified locally, shape consistency, exit values that
//!    pass their types, execution requests within the pack's ceiling.
//!
//! The closure ([`crate::closure`]) runs after these.

use crate::model::*;
use crate::types::{self, Resolved};
use adage_core::{CommandTable, Effect};
use adage_tracks::io::{classify, invisible, safe_text};
use anyhow::{bail, Context, Result};
use once_cell_like::SLOT_RE;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const MAX_FILES: usize = 256;
pub const MAX_FILE_BYTES: u64 = 1 << 20;
pub const MAX_SCRIPT_BYTES: usize = 64 << 10;
pub const MAX_SCRIPTS_TOTAL: usize = 256 << 10;
pub const MAX_ROUTES: usize = 200;
pub const MAX_STEPS: usize = 4;
pub const MAX_ARGV: usize = 32;
pub const MAX_INTENT_ROUTES: usize = adage_tracks::MAX_INTENT_ROUTES;

pub const MANIFEST: &str = "pack.toml";
pub const CLOSURE: &str = "CLOSURE";
pub const EVIDENCE: &str = "evidence.json";
pub const SIG: &str = "pack.sig";

/// Interpreters: the first operand is the script they run (as `adage_core::keys`).
pub const INTERPRETERS: [&str; 9] = ["python", "python3", "node", "bash", "sh", "zsh", "ruby", "perl", "deno"];
/// Interpreters that may run a vendored script (Track C §1.7).
pub const VENDOR_INTERPRETERS: [&str; 3] = ["python3", "node", "sh"];
/// Flags that make an interpreter run inline code: never a plain argv.
const CODE_FLAGS: [&str; 5] = ["-c", "-e", "--eval", "-p", "--print"];
/// An interpreter operand with one of these extensions is a script path.
const SCRIPT_EXTS: [&str; 10] = [".py", ".js", ".mjs", ".cjs", ".ts", ".sh", ".bash", ".rb", ".pl", ".php"];

mod once_cell_like {
    use regex::Regex;
    use std::sync::LazyLock;
    pub static SLOT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([a-z_][a-z0-9_]{0,31})\}").unwrap());
}

// ---------------------------------------------------------------------------------------------
// Small predicates
// ---------------------------------------------------------------------------------------------

/// Lowercase letters, digits and `-`, starting alphanumeric, at most 64 bytes (as
/// `adage_tracks::io::safe_id`).
pub fn safe_id(id: &str) -> bool {
    adage_tracks::io::safe_id(id)
}

/// `owner/name`, each part lowercase `[a-z0-9._-]` starting alphanumeric (as
/// `adage_tracks::io::safe_name`, but exactly two parts: a pack is one track namespace).
pub fn safe_pack_name(n: &str) -> bool {
    n.len() <= 128
        && n.split('/').count() == 2
        && n.split('/').all(|p| {
            !p.is_empty()
                && p.as_bytes()[0].is_ascii_alphanumeric()
                && p.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
        })
}

/// A detector's catalogue id: an uppercase letter and one to three digits (`U5`, `N1`, `U11`).
pub fn detector_id_ok(d: &str) -> bool {
    (2..=4).contains(&d.len()) && d.as_bytes()[0].is_ascii_uppercase() && d.bytes().skip(1).all(|c| c.is_ascii_digit())
}

/// A short token: ASCII alphanumeric first, then `[A-Za-z0-9._-]`.
pub fn safe_token(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}

/// An input, output or slot name: `[a-z_][a-z0-9_]{0,31}`.
pub fn safe_slot(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && !s.as_bytes()[0].is_ascii_digit()
        && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// A repository-relative path: no absolute path, no `.` or `..` segment, no empty segment, no
/// leading `-`, nothing but `[A-Za-z0-9_.@+-/]`.
pub fn safe_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 200
        && !p.starts_with('/')
        && !p.starts_with('-')
        && p.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
        && p.chars().all(|c| c.is_ascii_alphanumeric() || "_.@+-/".contains(c))
}

/// A repository-relative glob (as `adage_tracks::advice::safe_glob`).
pub fn safe_glob(g: &str) -> bool {
    adage_tracks::advice::safe_glob(g)
}

/// An E2 action's output or cache glob: a safe repository-relative glob that cannot name `.git`
/// (its first segment is neither `.git` nor a wildcard that could match it).
pub fn action_glob_ok(g: &str) -> bool {
    let first = g.split('/').next().unwrap_or_default();
    safe_glob(g)
        && g.len() <= 200
        && first != ".git"
        && !first.starts_with('.')
        && !first.starts_with('*')
        && !first.starts_with('?')
        && !first.starts_with('[')
}

/// Cache declarations additionally support pytest's conventional hidden cache directory.
/// This exception never applies to outputs or to arbitrary hidden paths.
pub fn cache_glob_ok(g: &str) -> bool {
    action_glob_ok(g) || matches!(g, ".pytest_cache/**" | ".pytest_cache/.gitignore")
}

#[cfg(test)]
mod cache_glob_tests {
    use super::{action_glob_ok, cache_glob_ok};

    #[test]
    fn pytest_cache_exception_does_not_authorize_hidden_outputs_or_other_hidden_paths() {
        assert!(cache_glob_ok(".pytest_cache/**"));
        assert!(cache_glob_ok(".pytest_cache/.gitignore"));
        assert!(!action_glob_ok(".pytest_cache/**"));
        assert!(!action_glob_ok(".pytest_cache/.gitignore"));
        assert!(cache_glob_ok("tests/__pycache__/**"));
        for glob in
            [".env", ".codex/**", ".git/**", ".GIT/**", ".pytest_cache/../.git/**", ".*/**", "**/__pycache__/**"]
        {
            assert!(!cache_glob_ok(glob), "{glob}");
        }
    }
}

/// Lowercase hex SHA-256.
pub fn sha256_hex_ok(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// `name@digest`: a model name and the first 12 hex digits of its table's SHA-256.
pub fn safe_model_id(m: &str) -> bool {
    let Some((name, digest)) = m.split_once('@') else { return false };
    !name.is_empty()
        && name.len() <= 48
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-".contains(&c))
        && digest.len() == 12
        && digest.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// `major.minor.patch`, digits only.
pub fn semver_ok(v: &str) -> bool {
    v.len() <= 32
        && v.split('.').count() == 3
        && v.split('.').all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}

/// A version constraint: `>=1.6`, `=2`, `<3.0.1`, or a bare version.
pub fn version_constraint_ok(v: &str) -> bool {
    let rest = v.strip_prefix(">=").or_else(|| v.strip_prefix('=')).or_else(|| v.strip_prefix('<')).unwrap_or(v);
    let parts: Vec<&str> = rest.split('.').collect();
    !rest.is_empty()
        && parts.len() <= 3
        && parts.iter().all(|p| !p.is_empty() && p.len() <= 6 && p.bytes().all(|c| c.is_ascii_digit()))
}

/// A repository key, as `adage_core::repo::repo_key` writes one: an origin URL form
/// (`github.com/acme/payments`) or `name@<root commit>` (or a legacy bare name).
pub fn safe_repo_key(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 200
        && k.as_bytes()[0].is_ascii_alphanumeric()
        && k.split('/').all(|s| !s.is_empty() && s != "..")
        && k.chars().all(|c| c.is_ascii_alphanumeric() || "._-/@:~".contains(c))
}

/// One argv token: plain (no shell syntax, whitespace or quoting), braces only in whole `{input}`
/// placeholders, and no path outside the repository, in an argument or a flag's value.
pub fn argv_token_ok(t: &str) -> bool {
    if t.is_empty() || t.len() > 200 || !t.chars().all(|c| c.is_ascii_alphanumeric() || "_./:=@,+{}-".contains(c)) {
        return false;
    }
    if SLOT_RE.replace_all(t, "S").contains(['{', '}']) {
        return false;
    }
    t.split(['=', ',']).all(|p| !p.starts_with('/') && !p.split('/').any(|s| s == ".."))
}

/// The `{input}` names an argv token references.
pub fn slot_refs(t: &str) -> Vec<String> {
    SLOT_RE.captures_iter(t).map(|c| c[1].to_string()).collect()
}

/// A canonical command key a trigger matches (`exec python3 scripts/wi-lookup {id}`).
pub fn command_key_ok(k: &str) -> bool {
    let Some(rest) = k.strip_prefix("exec ") else { return false };
    k.len() <= 200 && !rest.is_empty() && rest.split(' ').all(argv_token_ok)
}

/// A JSON field of a command's output: `name` or `a.b` (at most four segments), or `*paths`
/// (the repository paths plain-text output mentions, as the learner reads them).
pub fn json_field_ok(f: &str) -> bool {
    f == "*paths"
        || (f.len() <= 128
            && f.split('.').count() <= 4
            && f.split('.').all(|s| {
                !s.is_empty()
                    && s.len() <= 64
                    && !s.as_bytes()[0].is_ascii_digit()
                    && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            }))
}

/// Text for people (descriptions): printable, nothing invisible or direction-changing.
pub fn people_text_ok(t: &str, max: usize) -> bool {
    t.len() <= max && !t.chars().any(|c| c.is_control() || invisible(c) || c == '\u{2028}' || c == '\u{2029}')
}

pub fn rank(e: Effect) -> u8 {
    match e {
        Effect::Observe => 0,
        Effect::Verify => 1,
        Effect::Regenerate => 2,
        Effect::Environment => 3,
        Effect::Author => 4,
        Effect::Publish => 5,
    }
}

// ---------------------------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------------------------

/// The files a pack may contain (Track C §1.2). Anything else refuses the pack.
pub fn allowed(rel: &str) -> bool {
    let leaf = |dir: &str, ext: &str| {
        rel.strip_prefix(dir).and_then(|f| f.strip_suffix(ext)).is_some_and(|stem| !stem.contains('/') && safe_id(stem))
    };
    matches!(rel, MANIFEST | CLOSURE | EVIDENCE | SIG | crate::procedure::PROCEDURE)
        || leaf("routes/", ".yaml")
        || leaf("types/", ".yaml")
        || leaf("attestations/", ".json")
        || rel.strip_prefix("scripts/sha256-").is_some_and(sha256_hex_ok)
}

const DIRS: [&str; 4] = ["routes", "types", "scripts", "attestations"];

/// Read a pack directory's files without following anything. Refuses symlinks, special files,
/// unknown directories and files, and anything over the limits. `pack.sig` is included.
pub fn read_dir_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let meta = std::fs::symlink_metadata(dir).with_context(|| format!("reading {}", dir.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        bail!("{} is not a directory (a pack is a directory; symlinks are refused)", dir.display());
    }
    let mut out = BTreeMap::new();
    let mut count = 0usize;
    let mut stack = vec![(dir.to_path_buf(), 0u8)];
    while let Some((d, depth)) = stack.pop() {
        for e in std::fs::read_dir(&d).with_context(|| format!("reading {}", d.display()))? {
            let p = e?.path();
            let rel = p.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
            let meta = std::fs::symlink_metadata(&p)?;
            count += 1;
            if count > MAX_FILES + DIRS.len() {
                bail!("{} holds more than {MAX_FILES} files", dir.display());
            }
            if meta.file_type().is_symlink() {
                bail!("{rel} is a symlink; a pack may not contain symlinks");
            } else if meta.is_dir() {
                if depth == 0 && DIRS.contains(&rel.as_str()) {
                    stack.push((p, 1));
                } else {
                    bail!("{rel}/ is not part of the pack format");
                }
            } else if !meta.is_file() {
                bail!("{rel} is not a regular file");
            } else if !allowed(&rel) {
                bail!("{rel} is not part of the pack format (smuggled file)");
            } else if meta.len() > MAX_FILE_BYTES {
                bail!("{rel} is larger than {MAX_FILE_BYTES} bytes");
            } else {
                out.insert(rel, std::fs::read(&p)?);
            }
        }
    }
    check_files(&out)?;
    Ok(out)
}

/// Limits and content rules over a pack's files (paths relative to the pack, `/`-separated).
pub fn check_files(files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    if files.len() > MAX_FILES {
        bail!("the pack holds {} files; at most {MAX_FILES} are allowed", files.len());
    }
    let mut scripts = 0usize;
    for (rel, bytes) in files {
        if !allowed(rel) {
            bail!("{rel} is not part of the pack format (smuggled file)");
        }
        if bytes.len() as u64 > MAX_FILE_BYTES {
            bail!("{rel} is larger than {MAX_FILE_BYTES} bytes");
        }
        if rel.starts_with("scripts/") {
            if bytes.len() > MAX_SCRIPT_BYTES {
                bail!("{rel} is larger than {MAX_SCRIPT_BYTES} bytes (the script limit)");
            }
            scripts += bytes.len();
            if scripts > MAX_SCRIPTS_TOTAL {
                bail!("the pack's scripts total more than {MAX_SCRIPTS_TOTAL} bytes");
            }
        }
        let Ok(text) = std::str::from_utf8(bytes) else { bail!("{rel} is not UTF-8 text") };
        if text.contains('\0') {
            bail!("{rel} contains a NUL byte");
        }
        for (i, l) in text.split('\n').enumerate() {
            if let Some(c) = l.chars().find(|&c| (c.is_control() && c != '\t' && c != '\r') || invisible(c)) {
                bail!("{rel}:{} contains the invisible or control character U+{:04X}", i + 1, c as u32);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------------------------

/// What [`parse`] reads from a pack's files.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub manifest: PackManifest,
    pub routes: Vec<RouteV2>,
    pub types: BTreeMap<String, TypeDef>,
    /// Vendored script bytes by their SHA-256 (hex).
    pub scripts: BTreeMap<String, Vec<u8>>,
    pub evidence: Option<PackEvidence>,
    pub attestations: BTreeMap<String, serde_json::Value>,
    /// `procedure.json`, for a pack compiled from a skill.
    pub procedure: Option<crate::procedure::Procedure>,
    /// The `CLOSURE` file as found, if any.
    pub closure: Option<String>,
}

fn text(rel: &str, b: &[u8]) -> Result<String> {
    String::from_utf8(b.to_vec()).with_context(|| format!("{rel} is not UTF-8"))
}

/// Parse a pack's files strictly. Runs [`check_files`] first.
pub fn parse(files: &BTreeMap<String, Vec<u8>>) -> Result<Parsed> {
    check_files(files)?;
    let manifest_bytes = files.get(MANIFEST).with_context(|| format!("the pack has no {MANIFEST}"))?;
    let manifest: PackManifest = toml::from_str(&text(MANIFEST, manifest_bytes)?)
        .with_context(|| format!("{MANIFEST} is not a valid manifest"))?;
    let mut p = Parsed {
        manifest,
        routes: Vec::new(),
        types: BTreeMap::new(),
        scripts: BTreeMap::new(),
        evidence: None,
        attestations: BTreeMap::new(),
        procedure: None,
        closure: None,
    };
    for (rel, bytes) in files {
        if let Some(stem) = rel.strip_prefix("routes/").and_then(|f| f.strip_suffix(".yaml")) {
            let r: RouteV2 =
                serde_yaml::from_str(&text(rel, bytes)?).with_context(|| format!("{rel} is not a valid route"))?;
            if r.id != stem {
                bail!("{rel} holds route {:?}; a route file is named by its id", r.id);
            }
            p.routes.push(r);
        } else if let Some(stem) = rel.strip_prefix("types/").and_then(|f| f.strip_suffix(".yaml")) {
            let t: TypeDef =
                serde_yaml::from_str(&text(rel, bytes)?).with_context(|| format!("{rel} is not a valid type"))?;
            if t.id != stem {
                bail!("{rel} holds type {:?}; a type file is named by its id", t.id);
            }
            p.types.insert(t.id.clone(), t);
        } else if let Some(hex) = rel.strip_prefix("scripts/sha256-") {
            let got = crate::digest::hex(&Sha256::digest(bytes));
            if got != hex {
                bail!("{rel} is not named by its SHA-256 ({got})");
            }
            p.scripts.insert(hex.to_string(), bytes.clone());
        } else if rel == EVIDENCE {
            p.evidence = Some(
                serde_json::from_str(&text(rel, bytes)?)
                    .with_context(|| format!("{rel} is not valid pack evidence"))?,
            );
        } else if let Some(stem) = rel.strip_prefix("attestations/").and_then(|f| f.strip_suffix(".json")) {
            let v: serde_json::Value =
                serde_json::from_str(&text(rel, bytes)?).with_context(|| format!("{rel} is not JSON"))?;
            if !v.is_object() {
                bail!("{rel} is not a JSON object");
            }
            p.attestations.insert(stem.to_string(), v);
        } else if rel == CLOSURE {
            p.closure = Some(text(rel, bytes)?);
        } else if rel == crate::procedure::PROCEDURE {
            p.procedure = Some(
                serde_json::from_str(&text(rel, bytes)?).with_context(|| format!("{rel} is not a valid procedure"))?,
            );
        }
    }
    Ok(p)
}

// ---------------------------------------------------------------------------------------------
// Semantics
// ---------------------------------------------------------------------------------------------

/// What a run step's argv runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Program {
    /// A program in the command table, found on PATH (L-none).
    Tool(String),
    /// A repository or vendored script: its path (normalised, without `./`), and the
    /// interpreter named before it, if any (a tool, or an environment program's path).
    Script { path: String, interpreter: Option<String> },
    /// A program of the project's own environment (`.venv/bin/python`, `node_modules/.bin/vitest`):
    /// its repository-relative path and the command-table program it is. The environment is the
    /// project's, made by its setup, not content of the repository: it is never pinned or
    /// vendored, never executed by adage-exec, and binds only where the checkout has it.
    Env { path: String, tool: String },
}

fn looks_like_script(op: &str) -> bool {
    op.contains('/') || SCRIPT_EXTS.iter().any(|e| op.ends_with(e))
}

/// Read what an argv runs. Refuses a placeholder as the program, inline interpreter code, and a
/// program that is neither in the command table nor a path.
pub fn program(argv: &[String], table: &CommandTable) -> Result<Program> {
    let Some(a0) = argv.first() else { bail!("an empty argv runs nothing") };
    if a0.contains('{') {
        bail!("argv[0] {a0:?} is a placeholder; the program is never bound from input");
    }
    let env = env_program(a0, table);
    if a0.contains('/') && env.is_none() {
        let p = a0.strip_prefix("./").unwrap_or(a0);
        if !safe_path(p) {
            bail!("argv[0] {a0:?} is not a repository-relative path");
        }
        return Ok(Program::Script { path: p.to_string(), interpreter: None });
    }
    // The program as the command table names it (`python` for `.venv/bin/python`).
    let name = env.as_ref().map(|(_, tool)| tool.clone()).unwrap_or_else(|| a0.clone());
    if table.get(&name).is_none() {
        bail!("program {a0:?} is not in the command table, and no script covers it (dangling reference)");
    }
    if INTERPRETERS.contains(&name.as_str()) {
        if let Some(f) = argv.iter().skip(1).find(|a| CODE_FLAGS.contains(&a.as_str())) {
            bail!("{a0} {f} runs inline code; a pack step is a plain argv");
        }
        let mut prev_m = false;
        for a in argv.iter().skip(1) {
            if a.starts_with('-') {
                prev_m = a == "-m";
                continue;
            }
            if prev_m {
                break; // `python3 -m pytest`: a module, not a script
            }
            if looks_like_script(a) {
                if a.contains('{') {
                    bail!("the script {a:?} is a placeholder; the script is never bound from input");
                }
                let p = a.strip_prefix("./").unwrap_or(a);
                if !safe_path(p) {
                    bail!("script {a:?} is not a repository-relative path");
                }
                return Ok(Program::Script { path: p.to_string(), interpreter: Some(a0.clone()) });
            }
            break;
        }
    }
    Ok(match env {
        Some((path, tool)) => Program::Env { path, tool },
        None => Program::Tool(a0.clone()),
    })
}

/// A program of the project's own environment: `<dir>/bin/<tool>` (a virtualenv: `.venv/bin/python`)
/// or `node_modules/.bin/<tool>`, where `<dir>` is a repository-relative directory and `<tool>` is
/// in the command table. Returns its path (without `./`) and the tool.
pub fn env_program(a0: &str, table: &CommandTable) -> Option<(String, String)> {
    let p = a0.strip_prefix("./").unwrap_or(a0);
    if !safe_path(p) {
        return None;
    }
    let (dir, tool) = p.rsplit_once('/')?;
    let env_dir = dir.ends_with("/bin") || dir == "bin" || dir.ends_with("node_modules/.bin");
    let in_env = adage_core::action::venv_of(p).is_some() || dir.ends_with("node_modules/.bin");
    (env_dir && in_env && table.get(tool).is_some()).then(|| (p.to_string(), tool.to_string()))
}

/// Everything [`validate`] learns that the closure needs.
#[derive(Debug, Clone, Default)]
pub struct Checked {
    /// Per route: the resolved type of each input and each `step.output`.
    pub resolved: BTreeMap<String, BTreeMap<String, Resolved>>,
}

/// Semantic validation of a parsed pack.
pub fn validate(p: &Parsed) -> Result<Checked> {
    let table = CommandTable::load();
    validate_manifest(&p.manifest, table)?;
    if p.routes.is_empty() {
        bail!("the pack has no routes");
    }
    if p.routes.len() > MAX_ROUTES {
        bail!("the pack has {} routes; at most {MAX_ROUTES} are allowed", p.routes.len());
    }
    for t in p.types.values() {
        if !safe_id(&t.id) {
            bail!("type id {:?} is not allowed", t.id);
        }
        if types::is_builtin(&t.id) {
            bail!("type {} shadows a built-in type", t.id);
        }
        types::check_typedef(t)?;
        if let Some(item) = &t.item {
            types::resolve(item, None, None, &p.types, &format!("type {}", t.id))?;
            if item == &t.id {
                bail!("type {} is a list of itself", t.id);
            }
        }
    }
    // A detector pack is made of templates, each filled by a detector compiled into Adage; every
    // other pack holds routes with their values.
    match &p.manifest.applicability {
        Applicability::Detector { detector, .. } => {
            for r in &p.routes {
                match r.detector.as_deref() {
                    None => bail!("route {}: a detector pack holds templates, and this route names no detector", r.id),
                    Some(d) if detector != "*" && d != detector => {
                        bail!("route {}: detector {d} is not the pack's ({detector}, its applicability)", r.id)
                    }
                    Some(_) => {}
                }
            }
            if p.evidence.is_some() {
                bail!("a detector pack has no {EVIDENCE}: a template's evidence is each repository's own");
            }
        }
        _ => {
            if let Some(r) = p.routes.iter().find(|r| r.is_template()) {
                bail!(
                    "route {}: only a detector pack's routes are templates (applicability kind = \"detector\")",
                    r.id
                );
            }
        }
    }
    let ids: BTreeSet<&str> = p.routes.iter().map(|r| r.id.as_str()).collect();
    let mut checked = Checked::default();
    for r in &p.routes {
        let res = validate_route(r, p, &ids, table).with_context(|| format!("route {}", r.id))?;
        checked.resolved.insert(r.id.clone(), res);
    }
    let intents = p.routes.iter().filter(|r| matches!(r.trigger, TriggerV2::Intent { .. })).count();
    if intents > MAX_INTENT_ROUTES {
        bail!("the pack has {intents} intent routes; at most {MAX_INTENT_ROUTES} are allowed");
    }
    if let Some(ev) = &p.evidence {
        for k in ev.keys() {
            if !ids.contains(k.as_str()) {
                bail!("{EVIDENCE} has evidence for {k:?}, which is not a route of the pack (dangling reference)");
            }
        }
    }
    if p.attestations.len() > crate::attest::MAX_ATTESTATIONS {
        bail!(
            "the pack has {} attestations; at most {} are allowed",
            p.attestations.len(),
            crate::attest::MAX_ATTESTATIONS
        );
    }
    for (id, a) in &p.attestations {
        if serde_json::to_string(a)?.len() > 64 << 10 {
            bail!("attestation {id} is larger than 64 KiB");
        }
        if !safe_id(id) {
            bail!("attestation id {id:?} is not allowed");
        }
        crate::attest::check(a).with_context(|| format!("attestation {id} is not a bench result record"))?;
    }
    let skill = p.manifest.provenance.skill.as_deref();
    match &p.procedure {
        Some(_) if p.manifest.pack.origin != PackOrigin::Trained => {
            bail!("{} belongs to a trained pack", crate::procedure::PROCEDURE)
        }
        Some(proc_) => proc_.check(skill, &ids)?,
        None if skill.is_some() && p.manifest.pack.origin != PackOrigin::Trained => {
            bail!("[provenance] skill belongs to a trained pack")
        }
        None => {}
    }
    Ok(checked)
}

fn validate_manifest(m: &PackManifest, table: &CommandTable) -> Result<()> {
    if m.format != FORMAT {
        bail!("{MANIFEST}: format {:?} is not {FORMAT:?}", m.format);
    }
    let i = &m.pack;
    if !safe_pack_name(&i.name) {
        bail!("pack name {:?} is not allowed (owner/name, lowercase)", i.name);
    }
    if !semver_ok(&i.version) {
        bail!("pack version {:?} is not semver", i.version);
    }
    if !people_text_ok(&i.description, 1000) {
        bail!("pack description contains characters that may not be shown");
    }
    match &m.applicability {
        Applicability::Repo { repo_keys } => {
            if repo_keys.is_empty() || repo_keys.len() > 16 {
                bail!("applicability: 1..=16 repo keys");
            }
            if let Some(k) = repo_keys.iter().find(|k| !safe_repo_key(k)) {
                bail!("applicability: repo key {k:?} is not allowed");
            }
            unique(repo_keys, "applicability repo key")?;
        }
        Applicability::Fingerprint { fingerprint } => {
            if fingerprint.is_empty() || fingerprint.len() > 16 {
                bail!("applicability: 1..=16 fingerprint predicates");
            }
            for f in fingerprint {
                let ok = match f {
                    Fingerprint::MakefileTarget(t) => safe_token(t, 64),
                    Fingerprint::PackageScript(s) => {
                        !s.is_empty()
                            && s.len() <= 64
                            && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
                    }
                    Fingerprint::PathExists(g) => safe_glob(g),
                    Fingerprint::Lockfile(l) => adage_tracks::advice::LOCKFILES.iter().any(|(x, _)| x == l),
                };
                if !ok {
                    bail!("applicability: fingerprint predicate {f:?} is not allowed");
                }
            }
        }
        Applicability::Detector { detector, catalogue } => {
            if i.origin != PackOrigin::Instinct {
                bail!("applicability: a detector applies only to an instinct pack");
            }
            // `*`: any detector the pack's templates name.
            let det = detector == "*" || detector_id_ok(detector);
            let cat = catalogue.split_once('@').is_some_and(|(n, v)| safe_token(n, 32) && semver_ok(v));
            if !det || !cat {
                bail!("applicability: detector {detector:?} in catalogue {catalogue:?} is not allowed");
            }
        }
        Applicability::Any => {}
    }
    let q = &m.requires;
    unique(&q.os, "requires.os")?;
    if q.os.contains(&Os::Windows) && m.exec.requested != Level::E0 {
        bail!("requires.os: windows is allowed only for an E0 pack");
    }
    for t in &q.tools {
        if table.get(&t.name).is_none() || t.name.contains('/') {
            bail!("requires.tools: {:?} is not a program in the command table", t.name);
        }
        if let Some(v) = &t.version {
            if !version_constraint_ok(v) {
                bail!("requires.tools: version {v:?} of {} is not a version constraint", t.name);
            }
        }
    }
    unique(&q.tools.iter().map(|t| &t.name).collect::<Vec<_>>(), "requires.tools")?;
    for h in &q.harness {
        let (name, ver) = h.split_once(' ').unwrap_or((h, ""));
        let name_ok = !name.is_empty()
            && name.len() <= 32
            && name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-');
        if !name_ok || !(ver.is_empty() || version_constraint_ok(ver)) {
            bail!("requires.harness: {h:?} is not `name` or `name >=version`");
        }
    }
    unique(&q.harness, "requires.harness")?;
    if let Some(m) = q.models.iter().find(|m| !safe_model_id(m)) {
        bail!("requires.models: {m:?} is not `name@<12 hex digits>`");
    }
    unique(&q.models, "requires.models")?;
    for t in &q.templates {
        let ok = t.split_once('@').is_some_and(|(n, v)| {
            !n.is_empty()
                && n.len() <= 48
                && n.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b":-".contains(&c))
                && !v.is_empty()
                && v.len() <= 3
                && v.bytes().all(|c| c.is_ascii_digit())
        });
        if !ok {
            bail!("requires.templates: {t:?} is not `name@version`");
        }
    }
    unique(&q.templates, "requires.templates")?;
    if let Some(c) = &q.command_table {
        if !version_constraint_ok(c) {
            bail!("requires.command_table: {c:?} is not a version constraint");
        }
    }
    for f in &q.repo_files {
        if !safe_path(&f.path) || !sha256_hex_ok(&f.sha256) {
            bail!("requires.repo_files: {:?} is not a repository-relative path with a SHA-256", f.path);
        }
        if f.deps.len() > 16 || f.deps.iter().any(|g| !safe_glob(g)) {
            bail!("requires.repo_files: deps of {} must be at most 16 repository-relative globs", f.path);
        }
    }
    unique(&q.repo_files.iter().map(|f| &f.path).collect::<Vec<_>>(), "requires.repo_files")?;
    if !people_text_ok(&m.provenance.tool, 64) {
        bail!("provenance.tool contains characters that may not be shown");
    }
    if let Some(s) = &m.provenance.skill {
        if !s.strip_prefix("sha256:").is_some_and(sha256_hex_ok) {
            bail!("provenance.skill {s:?} is not `sha256:<lowercase hex>`");
        }
    }
    Ok(())
}

fn unique<T: Ord + std::fmt::Debug>(xs: &[T], what: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for x in xs {
        if !seen.insert(x) {
            bail!("{what}: {x:?} is listed twice");
        }
    }
    Ok(())
}

/// Validate one route; returns the resolved types of its inputs (`name`) and outputs
/// (`step.name`).
fn validate_route(
    r: &RouteV2,
    p: &Parsed,
    ids: &BTreeSet<&str>,
    table: &CommandTable,
) -> Result<BTreeMap<String, Resolved>> {
    if !safe_id(&r.id) {
        bail!("route id {:?} is not allowed", r.id);
    }
    if !r.summary.is_empty() && !safe_text(&r.summary) {
        bail!("summary contains characters that may not be shown");
    }
    if r.is_template() {
        validate_template(r)?;
        return Ok(BTreeMap::new());
    }
    validate_trigger(&r.trigger)?;

    // Inputs.
    let mut resolved = BTreeMap::new();
    if r.inputs.len() > 8 {
        bail!("at most 8 inputs");
    }
    for (name, i) in &r.inputs {
        if !safe_slot(name) {
            bail!("input name {name:?} is not allowed");
        }
        let t = types::resolve(&i.ty, i.values.as_deref(), i.max, &p.types, &format!("input {name}"))?;
        match (&i.from.prompt, &i.from.result, &i.from.edit_tests) {
            (Some(_), None, None) => {
                if !matches!(r.trigger, TriggerV2::Prompt { .. }) {
                    bail!("input {name} comes from the prompt, but the trigger is not a prompt");
                }
            }
            (None, Some(src), None) => {
                if !matches!(r.trigger, TriggerV2::AfterCommand { .. } | TriggerV2::AfterFailure { .. }) {
                    bail!("input {name} comes from a command's result, but the trigger follows no command");
                }
                if !json_field_ok(&src.field) {
                    bail!("input {name}: result field {:?} is not allowed", src.field);
                }
            }
            (None, None, Some(src)) => {
                if !matches!(r.trigger, TriggerV2::AfterEdit { .. }) {
                    bail!("input {name} comes from the edited file, but the trigger is not an edit");
                }
                if !t.is_repo_path_list() {
                    bail!("input {name}: the tests of an edited file are a repo-path-list");
                }
                if src.k == 0 || src.k > EDIT_TESTS_MAX_K {
                    bail!("input {name}: k must be 1..={EDIT_TESTS_MAX_K}");
                }
                if src.map.is_empty() || src.map.len() > EDIT_TESTS_MAX_ENTRIES {
                    bail!("input {name}: the map has 1..={EDIT_TESTS_MAX_ENTRIES} entries");
                }
                for (from, tests) in &src.map {
                    if !safe_path(from) || tests.is_empty() || tests.len() > src.k as usize {
                        bail!("input {name}: map entry {from:?} is not a path with 1..=k test files");
                    }
                    if let Some(t) = tests.iter().find(|t| !safe_path(t)) {
                        bail!("input {name}: map test {t:?} is not a repository-relative path");
                    }
                }
                if r.exec.as_ref().is_some_and(|x| x.level > Level::E0) {
                    bail!("input {name}: an edited file's tests are delivered at E0 only (never executed)");
                }
            }
            _ => bail!("input {name}: `from` names exactly one source (prompt, result or edit_tests)"),
        }
        resolved.insert(name.clone(), t);
    }
    if matches!(r.trigger, TriggerV2::Intent { .. }) && !r.inputs.is_empty() {
        bail!("an intent route has no inputs: nothing from the prompt is interpolated");
    }

    // Steps.
    if r.steps.is_empty() || r.steps.len() > MAX_STEPS {
        bail!("a route has 1..={MAX_STEPS} steps");
    }
    let mut used_inputs = BTreeSet::new();
    let mut step_ids = BTreeSet::new();
    let mut kinds = Vec::new();
    let mut max_effect = Effect::Observe;
    for s in &r.steps {
        let ctx = format!("step {}", s.id);
        if !safe_id(&s.id) {
            bail!("step id {:?} is not allowed", s.id);
        }
        if !step_ids.insert(s.id.as_str()) {
            bail!("step id {} is repeated", s.id);
        }
        let Some(kind) = s.kind() else { bail!("{ctx}: exactly one of run, read, avoid, advise") };
        if rank(s.effect) > rank(max_effect) {
            max_effect = s.effect;
        }
        if !matches!(kind, StepKind::Run(_)) && (s.script.is_some() || !s.outputs.is_empty()) {
            bail!("{ctx}: only a run step has a script or outputs");
        }
        match kind {
            StepKind::Run(run) => {
                validate_run(s, run, r, p, table, &mut used_inputs).with_context(|| ctx.clone())?;
                if s.outputs.len() > 16 {
                    bail!("{ctx}: at most 16 outputs");
                }
                for (name, o) in &s.outputs {
                    if !safe_slot(name) {
                        bail!("{ctx}: output name {name:?} is not allowed");
                    }
                    if !json_field_ok(&o.from.json) || o.from.json == "*paths" {
                        bail!("{ctx}: output {name}: JSON field {:?} is not allowed", o.from.json);
                    }
                    let t =
                        types::resolve(&o.ty, o.values.as_deref(), o.max, &p.types, &format!("{ctx}: output {name}"))?;
                    resolved.insert(format!("{}.{name}", s.id), t);
                }
            }
            StepKind::Read(rd) => {
                if rd.max == 0 || rd.max > 10 {
                    bail!("{ctx}: read max must be 1..=10");
                }
                let t = if let Some((step, out)) = rd.from.split_once('.') {
                    let earlier: Vec<&str> =
                        r.steps.iter().take_while(|x| x.id != s.id).map(|x| x.id.as_str()).collect();
                    if !earlier.contains(&step) {
                        bail!("{ctx}: read.from {:?} names no earlier step (dangling reference)", rd.from);
                    }
                    resolved.get(&format!("{step}.{out}"))
                } else {
                    used_inputs.insert(rd.from.clone());
                    resolved.get(&rd.from)
                };
                match t {
                    Some(t) if t.is_repo_path_list() => {}
                    Some(_) => bail!("{ctx}: read.from {:?} is not a list of repo-path", rd.from),
                    None => bail!("{ctx}: read.from {:?} names no output or input (dangling reference)", rd.from),
                }
            }
            StepKind::Avoid(a) => {
                if a.runners.is_empty() || a.runners.len() > 8 {
                    bail!("{ctx}: an avoid step names 1..=8 runners");
                }
                for argv in a.runners.iter().chain(std::iter::once(&a.instead)) {
                    if argv.is_empty() || argv.len() > MAX_ARGV || !argv.iter().all(|t| argv_token_ok(t)) {
                        bail!("{ctx}: argv {argv:?} is not a plain argv");
                    }
                    if argv.iter().any(|t| !slot_refs(t).is_empty()) {
                        bail!("{ctx}: an avoid step has placeholders nothing can bind");
                    }
                }
                if rank(classify(&a.instead.join(" "), table)) > rank(Effect::Regenerate) {
                    bail!("{ctx}: the suggested command {:?} is not a verify or regenerate step", a.instead.join(" "));
                }
                if !safe_text(&a.signature) {
                    bail!("{ctx}: the signature contains characters that may not be rendered");
                }
            }
            StepKind::Advise(a) => {
                adage_tracks::advice::check(&a.template, &a.slots, table).map_err(|e| anyhow::anyhow!("{ctx}: {e}"))?;
            }
        }
        kinds.push(kind);
    }
    if let Some(unused) = r.inputs.keys().find(|k| !used_inputs.contains(*k)) {
        bail!("input {unused} is used by no step");
    }
    validate_shape(r, &kinds)?;

    // Effects and strength.
    let by_edit = matches!(r.trigger, TriggerV2::AfterEdit { .. });
    match adage_tracks::ceiling(max_effect, by_edit) {
        None => bail!("effect {max_effect:?} may never be directed"),
        Some(max) if r.strength == Strength::Fire && max != Strength::Fire => {
            bail!("strength fire exceeds the ceiling for {max_effect:?}")
        }
        _ => {}
    }
    let instinct = p.manifest.pack.origin == PackOrigin::Instinct
        || matches!(r.evidence.as_ref().and_then(|e| e.origin.as_ref()), Some(RouteOrigin::Instinct { .. }));
    if instinct && r.strength != Strength::Prime {
        bail!("an instinct route is prime");
    }

    // Links.
    if r.links.len() > 16 {
        bail!("at most 16 links");
    }
    unique(&r.links, "links")?;
    for l in &r.links {
        let Some(target) = l.strip_prefix('#') else { bail!("link {l:?} is not `#route-id`") };
        if target == r.id {
            bail!("link {l:?} points at the route itself");
        }
        if !ids.contains(target) {
            bail!("link {l:?} names no route in the pack (dangling reference)");
        }
    }

    // Exit predicates.
    for e in &r.exit {
        if let ExitV2::ResultEquals { step, field, value } = e {
            let Some(t) = resolved.get(&format!("{step}.{field}")) else {
                bail!("exit: {step}.{field} is not an output of this route (dangling reference)");
            };
            if !t.accepts(value) {
                bail!("exit: value {value:?} does not pass the type of {step}.{field}");
            }
        }
    }
    if r.exit.len() > 8 {
        bail!("at most 8 exit predicates");
    }

    // Execution request.
    if let Some(x) = &r.exec {
        if x.level > p.manifest.exec.requested {
            bail!("exec level {:?} exceeds the pack's [exec] requested {:?}", x.level, p.manifest.exec.requested);
        }
        if x.timeout_ms == 0 || x.timeout_ms > MAX_TIMEOUT_MS {
            bail!("exec timeout_ms {} is outside 1..={MAX_TIMEOUT_MS}", x.timeout_ms);
        }
        if x.level > Level::E0 && matches!(r.shape, Shape::Avoid | Shape::Advise) {
            bail!("an {} route never executes; its exec level is E0", r.shape.as_str());
        }
        let ceiling = match x.level {
            Level::E0 => Effect::Publish,
            Level::E1 => Effect::Observe,
            Level::E2 => Effect::Environment,
        };
        if rank(max_effect) > rank(ceiling) {
            bail!("exec level {:?} can never be granted to a {max_effect:?} route", x.level);
        }
        if x.level != Level::E2 && !(x.outputs.is_empty() && x.cache.is_empty()) {
            bail!("exec outputs and cache globs belong to an E2 action only");
        }
        for (what, globs) in [("outputs", &x.outputs), ("cache", &x.cache)] {
            if globs.len() > MAX_ACTION_GLOBS {
                bail!("exec {what}: at most {MAX_ACTION_GLOBS} globs");
            }
            let valid_glob = if what == "cache" { cache_glob_ok } else { action_glob_ok };
            if let Some(g) = globs.iter().find(|g| !valid_glob(g)) {
                bail!("exec {what}: {g:?} is not a repository-relative glob outside .git");
            }
            unique(globs, &format!("exec {what}"))?;
        }
        if x.level == Level::E2 && max_effect == Effect::Regenerate && x.outputs.is_empty() {
            bail!("an E2 regenerate action declares the outputs it may change (exec outputs)");
        }
    }

    if let Some(ev) = &r.evidence {
        validate_evidence(ev)?;
    }
    Ok(resolved)
}

/// A template ([`RouteV2::detector`]): what an instinct may bind, with blanks where its values
/// go. Its detector supplies the values per repository, and each route it binds is checked
/// against this template and validated in full ([`crate::instincts`]), so here the template's
/// fixed parts are checked, and its blanks must stand exactly where a detector fills them.
fn validate_template(r: &RouteV2) -> Result<()> {
    let d = r.detector.as_deref().unwrap_or_default();
    if !detector_id_ok(d) {
        bail!("detector {d:?} is not a catalogue id (U5, N1, U11, …)");
    }
    if !r.inputs.is_empty() {
        bail!("a template has no inputs: its detector fills its blanks");
    }
    let blanks_ok = match &r.trigger {
        TriggerV2::AfterFailure { command, signature } => command == BLANK_RUNNER && signature == BLANK_SIGNATURE,
        TriggerV2::AfterCommand { command } => command == BLANK_RUNNER,
        TriggerV2::AfterEdit { paths } => paths == BLANK_PATHS,
        _ => bail!("an instinct is reactive: a template's trigger is after_failure, after_command or after_edit"),
    };
    if !blanks_ok {
        bail!(
            "a template's trigger holds blanks, not values: `{BLANK_RUNNER}` (and `{BLANK_SIGNATURE}` after a failure), or `{BLANK_PATHS}` after an edit"
        );
    }
    let [s] = r.steps.as_slice() else { bail!("a template has exactly one step") };
    if !safe_id(&s.id) {
        bail!("step id {:?} is not allowed", s.id);
    }
    if s.script.is_some() || !s.outputs.is_empty() {
        bail!("step {}: a template's step has no script or outputs", s.id);
    }
    let Some(kind) = s.kind() else { bail!("step {}: exactly one of run, read, avoid, advise", s.id) };
    match kind {
        StepKind::Run(run) => {
            if run.argv != [BLANK_COMMAND] {
                bail!(
                    "step {}: a template's run step is argv [\"{BLANK_COMMAND}\"]: its detector names the command",
                    s.id
                );
            }
            if rank(s.effect) > rank(Effect::Environment) {
                bail!("step {}: an instinct directs at most an environment step", s.id);
            }
        }
        StepKind::Advise(a) => {
            let Some((_, required, optional)) =
                adage_tracks::advice::TEMPLATES.iter().find(|(id, ..)| *id == a.template)
            else {
                bail!("step {}: advice template {:?} is not known", s.id, a.template);
            };
            if let Some(missing) = required.iter().find(|k| !a.slots.contains_key(**k)) {
                bail!("step {}: advice template {} needs the slot {missing}", s.id, a.template);
            }
            for (k, v) in &a.slots {
                if !required.contains(&k.as_str()) && !optional.contains(&k.as_str()) {
                    bail!("step {}: advice template {} has no slot {k}", s.id, a.template);
                }
                if v != &format!("{{{k}}}") {
                    bail!("step {}: slot {k} of a template is the blank {{{k}}}: its detector fills it", s.id);
                }
            }
        }
        StepKind::Read(_) | StepKind::Avoid(_) => {
            bail!("step {}: a template's step runs a command or gives advice", s.id)
        }
    }
    validate_shape(r, &[kind])?;
    if r.strength != Strength::Prime {
        bail!("an instinct route is prime");
    }
    let by_edit = matches!(r.trigger, TriggerV2::AfterEdit { .. });
    if adage_tracks::ceiling(s.effect, by_edit).is_none() {
        bail!("effect {:?} may never be directed", s.effect);
    }
    if r.exit.len() > 8 {
        bail!("at most 8 exit predicates");
    }
    unique(&r.exit.iter().map(|e| format!("{e:?}")).collect::<Vec<_>>(), "exit")?;
    if r.exit.iter().any(|e| matches!(e, ExitV2::ResultEquals { .. })) {
        bail!("exit: a template has no outputs for result_equals to read");
    }
    if !r.entry || !r.links.is_empty() {
        bail!("a template is an entry route with no links");
    }
    if r.exec.as_ref().is_some_and(|x| x.level != Level::E0 || !x.outputs.is_empty() || !x.cache.is_empty()) {
        bail!("an instinct is delivered at E0: a template requests no execution");
    }
    if r.evidence.is_some() {
        bail!("a template has no evidence: each repository's is its own");
    }
    Ok(())
}

fn validate_run(
    s: &StepV2,
    run: &RunSpec,
    r: &RouteV2,
    p: &Parsed,
    table: &CommandTable,
    used: &mut BTreeSet<String>,
) -> Result<()> {
    let argv = &run.argv;
    if argv.is_empty() || argv.len() > MAX_ARGV {
        bail!("argv has 1..={MAX_ARGV} tokens");
    }
    if let Some(t) = argv.iter().find(|t| !argv_token_ok(t)) {
        bail!("argv token {t:?} is not a plain token (argv injection)");
    }
    if argv[0].contains('{') {
        bail!("argv[0] {:?} is a placeholder; the program is never bound from input", argv[0]);
    }
    for t in argv {
        for n in slot_refs(t) {
            if !r.inputs.contains_key(&n) {
                bail!("argv references undeclared input {{{n}}}");
            }
            let from = &r.inputs[&n].from;
            // The tests of an edited file fill one whole token with their paths (E0 only).
            if from.edit_tests.is_some() {
                if t != &format!("{{{n}}}") {
                    bail!("argv input {{{n}}} (an edited file's tests) is a whole token");
                }
                used.insert(n);
                continue;
            }
            if from.prompt.is_none() {
                bail!("argv input {{{n}}} must come from the prompt or the edited file");
            }
            if types::resolve(&r.inputs[&n].ty, r.inputs[&n].values.as_deref(), None, &p.types, "input")
                .is_ok_and(|t| t.is_list())
            {
                bail!("argv input {{{n}}} is a list; an argv token takes one value");
            }
            used.insert(n);
        }
    }
    let prog = program(argv, table)?;
    match (&prog, &s.script) {
        (Program::Tool(_), Some(sc)) => bail!("script {:?} is declared, but argv does not run it", sc.path),
        (Program::Env { path, .. }, Some(_)) => {
            bail!("{path} is the project's environment, not a script: it has no script entry")
        }
        (Program::Env { .. }, None) if r.exec.as_ref().is_some_and(|x| x.level > Level::E0) => {
            bail!("a program of the project's environment is delivered at E0 only (never executed)")
        }
        (Program::Tool(_) | Program::Env { .. }, None) => {
            // Never trust a declared effect: classify the command ourselves.
            let actual = classify(&argv.join(" "), table);
            if rank(actual) > rank(s.effect) {
                bail!("argv {:?} is {actual:?}, but the step declares {:?}", argv.join(" "), s.effect);
            }
        }
        (Program::Script { path, .. }, None) => {
            bail!("argv names the repository path {path:?} without a script entry (dangling reference)")
        }
        (Program::Script { path, interpreter }, Some(sc)) => {
            if !sha256_hex_ok(&sc.sha256) {
                bail!("script {:?}: sha256 is not lowercase hex SHA-256", sc.path);
            }
            if &sc.path != path {
                bail!("script {:?} is declared, but argv runs {path:?}", sc.path);
            }
            match sc.closure {
                ClosureLevel::Ref => {
                    if !safe_path(&sc.path) || sc.path.starts_with("scripts/sha256-") {
                        bail!("script {:?} is not a repository-relative path", sc.path);
                    }
                    if sc.deps.len() > 16 || sc.deps.iter().any(|g| !safe_glob(g)) {
                        bail!("script {:?}: deps must be at most 16 repository-relative globs", sc.path);
                    }
                }
                ClosureLevel::Vendor => {
                    if sc.path != format!("scripts/sha256-{}", sc.sha256) {
                        bail!("vendored script path {:?} is not scripts/sha256-<its sha256>", sc.path);
                    }
                    if !sc.deps.is_empty() {
                        bail!("a vendored script has no deps: its bytes are its closure");
                    }
                    if !interpreter.as_deref().is_some_and(|i| VENDOR_INTERPRETERS.contains(&i)) {
                        bail!("a vendored script is run by an interpreter named in argv ({VENDOR_INTERPRETERS:?}); the shebang is ignored");
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_shape(r: &RouteV2, kinds: &[StepKind]) -> Result<()> {
    use TriggerV2 as T;
    let run = |k: &StepKind| matches!(k, StepKind::Run(_));
    let read = |k: &StepKind| matches!(k, StepKind::Read(_));
    let t = &r.trigger;
    let ok = match r.shape {
        Shape::Run => {
            matches!(t, T::Prompt { .. } | T::Intent { .. } | T::SessionStart)
                && run(&kinds[0])
                && kinds.iter().all(|k| run(k) || read(k))
        }
        Shape::After => {
            matches!(t, T::AfterCommand { .. } | T::AfterFailure { .. })
                && run(&kinds[0])
                && kinds.iter().all(|k| run(k) || read(k))
        }
        Shape::Obligation => matches!(t, T::AfterEdit { .. }) && kinds.iter().all(run),
        Shape::Read => !matches!(t, T::Intent { .. }) && kinds.iter().all(read),
        Shape::Sequence => kinds.len() >= 2 && kinds.iter().all(run),
        Shape::Avoid => {
            matches!(t, T::SessionStart | T::AfterFailure { .. } | T::Prompt { .. })
                && kinds.len() == 1
                && matches!(kinds[0], StepKind::Avoid(_))
        }
        Shape::Advise => {
            matches!(t, T::AfterEdit { .. } | T::AfterCommand { .. } | T::AfterFailure { .. })
                && kinds.len() == 1
                && matches!(kinds[0], StepKind::Advise(_))
        }
    };
    if !ok {
        bail!("shape {} does not fit its trigger and steps", r.shape.as_str());
    }
    match r.shape {
        Shape::Avoid if r.strength != Strength::Inhibit => bail!("an avoid route is inhibit"),
        Shape::Advise => {
            if r.strength != Strength::Prime || r.steps[0].effect != Effect::Observe || !r.inputs.is_empty() {
                bail!("an advise route is prime, observe, with no inputs");
            }
        }
        Shape::Avoid => {}
        _ if r.strength == Strength::Inhibit => bail!("only an avoid route is inhibit"),
        _ => {}
    }
    Ok(())
}

fn validate_trigger(t: &TriggerV2) -> Result<()> {
    match t {
        TriggerV2::Prompt { .. } | TriggerV2::SessionStart => {}
        TriggerV2::AfterCommand { command } => {
            if !command_key_ok(command) {
                bail!("trigger command key {command:?} is not a plain `exec …` key");
            }
        }
        TriggerV2::AfterFailure { command, signature } => {
            if !command_key_ok(command) {
                bail!("trigger command key {command:?} is not a plain `exec …` key");
            }
            if !safe_text(signature) {
                bail!("failure trigger signature contains unsafe text");
            }
        }
        TriggerV2::AfterEdit { paths } => {
            if !safe_glob(paths) {
                bail!("glob {paths:?} is not a repository-relative glob");
            }
        }
        TriggerV2::Intent { family, model, centroid, threshold } => {
            use adage_tracks::{INTENT_MAX_DIM, INTENT_MAX_THRESHOLD, INTENT_MIN_DIM, INTENT_MIN_THRESHOLD};
            if !safe_id(family) {
                bail!("intent family id {family:?} is not allowed");
            }
            if !safe_model_id(model) {
                bail!("intent model id {model:?} is not allowed");
            }
            if !(INTENT_MIN_DIM..=INTENT_MAX_DIM).contains(&centroid.len()) {
                bail!(
                    "intent centroid has {} dimensions; {INTENT_MIN_DIM}..={INTENT_MAX_DIM} are allowed",
                    centroid.len()
                );
            }
            if centroid.iter().any(|&x| !(-10_000..=10_000).contains(&x)) {
                bail!("intent centroid components must lie in -10000..=10000");
            }
            let norm2: f64 = centroid.iter().map(|&x| (x as f64 / 1e4).powi(2)).sum();
            if !(0.98..=1.02).contains(&norm2) {
                bail!("intent centroid is not a unit vector (|c|^2 = {norm2:.3})");
            }
            if !threshold.is_finite() || !(INTENT_MIN_THRESHOLD..=INTENT_MAX_THRESHOLD).contains(threshold) {
                bail!("intent threshold {threshold} is outside {INTENT_MIN_THRESHOLD}..={INTENT_MAX_THRESHOLD}");
            }
        }
    }
    Ok(())
}

fn validate_evidence(e: &RouteEvidence) -> Result<()> {
    if let Some(x) = &e.execution {
        if x.opportunities == 0
            || x.successful == 0
            || x.sessions == 0
            || x.successful > x.opportunities
            || x.sessions > x.successful
        {
            bail!("evidence: invalid routine execution counts");
        }
    }
    let unit = |x: f64| x.is_finite() && (0.0..=1.0).contains(&x);
    let nonneg = |x: f64| x.is_finite() && (0.0..=1e6).contains(&x);
    if !unit(e.rate) || !unit(e.lower_bound) || !nonneg(e.median_position) || !nonneg(e.turns_saved) {
        bail!("evidence: rates lie in 0..=1 and positions and savings are finite and non-negative");
    }
    if !e.provenance.is_empty() && (!safe_text(&e.provenance) || e.provenance.len() > 96) {
        bail!("evidence: provenance {:?} is not allowed", e.provenance);
    }
    if e.models.len() > 16 || e.models.iter().any(|m| !safe_token(m, 64)) {
        bail!("evidence: models must be at most 16 plain tokens");
    }
    for k in e.by_origin.keys() {
        if k.len() > 32 || !safe_id(&k.replace('_', "-")) {
            bail!("evidence origin {k:?} is not allowed");
        }
    }
    if let Some(RouteOrigin::Instinct { pack, version, instinct, ecosystem, declared }) = &e.origin {
        let token = |s: &str| s.len() <= 32 && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c));
        if !token(pack) || !token(version) || !token(instinct) {
            bail!("evidence: instinct origin {pack:?}@{version:?} ({instinct:?}) is not allowed");
        }
        if let Some(eco) = ecosystem {
            if !adage_tracks::ECOSYSTEMS.contains(&eco.as_str()) {
                bail!("evidence: ecosystem {eco:?} is not one of {:?}", adage_tracks::ECOSYSTEMS);
            }
        }
        if let Some(d) = declared {
            if !safe_path(d) {
                bail!("evidence: declared file {d:?} is not a repository-relative path");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        for ok in ["{item}", "--json", "--out=gen/x.py", "src/a.rs", "make", "-j4", "W-{id}"] {
            assert!(argv_token_ok(ok), "{ok}");
        }
        for bad in [
            "",
            "a b",
            "a;b",
            "$(x)",
            "`x`",
            "/etc/passwd",
            "--out=/tmp/x",
            "../x",
            "a/../../b",
            "{",
            "{Item}",
            "x}",
            "~/.ssh",
            "a|b",
            "a'b",
            "a\u{200B}b",
        ] {
            assert!(!argv_token_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn programs() {
        let t = CommandTable::load();
        let v = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
        assert_eq!(program(&v("make gen"), t).unwrap(), Program::Tool("make".into()));
        assert_eq!(program(&v("python3 -m pytest"), t).unwrap(), Program::Tool("python3".into()));
        assert_eq!(
            program(&v("python3 scripts/wi-lookup {item}"), t).unwrap(),
            Program::Script { path: "scripts/wi-lookup".into(), interpreter: Some("python3".into()) }
        );
        assert_eq!(
            program(&v("./scripts/x --json"), t).unwrap(),
            Program::Script { path: "scripts/x".into(), interpreter: None }
        );
        assert!(program(&v("python3 manage.py test"), t).is_ok_and(|p| matches!(p, Program::Script { .. })));
        assert!(program(&v("frobnicate"), t).is_err());
        assert!(program(&v("sh -c make"), t).is_err());
        assert!(program(&v("{prog} x"), t).is_err());
    }
}
