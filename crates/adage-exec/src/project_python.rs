//! Explicitly approved project Python environments for staged E2 verification. Discovery is
//! static: no interpreter is executed to decide what a person is being asked to approve.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::request::{ActionKind, ExecRequest, Level};

thread_local! {
    static DEADLINE: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

/// A cooperative deadline scoped to this thread's interpreter discovery/staging. Nested
/// discovery inherits the tighter action deadline; ordinary non-Python stages are unchanged.
pub(crate) struct Budget(Option<std::time::Instant>);
impl Drop for Budget {
    fn drop(&mut self) {
        DEADLINE.set(self.0);
    }
}
pub(crate) fn budget(ms: u64) -> Budget {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms.min(crate::limits::E2_MAX_WALL_MS));
    let old = DEADLINE.get();
    DEADLINE.set(Some(old.map_or(deadline, |old| old.min(deadline))));
    Budget(old)
}
pub(crate) fn checkpoint() -> Result<(), String> {
    if DEADLINE.get().is_some_and(|deadline| std::time::Instant::now() >= deadline) {
        Err("project Python preparation exceeded the action wall-clock budget".into())
    } else {
        Ok(())
    }
}

const MAX_FILES: usize = 40_000;
const MAX_BYTES: u64 = 1 << 30;
/// v1: private staged venv, plain editable paths rebased, -B and a fresh -X pycache_prefix.
/// v2: `__pycache__` directories are left out of the digests ([`bytecode_cache`]) and removed
/// from both private copies, and child interpreters get the private prefix too.
pub const ADAPTER_VERSION: u32 = 2;

/// Whether `rel` lies in a `__pycache__` directory: bytecode caches, left out of the environment
/// and runtime digests. Python writes them as it runs (every ordinary `python` or `pytest` run
/// compiles some), and hashing them made any run outside Adage lapse the person's approval: 126
/// stdlib `.pyc` files written into a shared runtime silently withheld every later action and
/// session contract.
///
/// This is safe only because an approved run never reads `__pycache__` in either tree:
/// - every staged project-Python run starts with `-B -X pycache_prefix=<fresh scratch dir>`
///   (`crate::stage::stage_with`), so the interpreter reads and writes bytecode caches only
///   under that private, empty prefix; child interpreters inherit `PYTHONPYCACHEPREFIX` (the same
///   directory) and `PYTHONDONTWRITEBYTECODE`;
/// - [`prepare`] removes every `__pycache__` directory from the private copies of the
///   environment and the runtime before anything runs, so nothing excluded here is present.
///
/// Project Python is valid only for an E2 verify action ([`validate_command`]), and E2 runs only
/// staged (`crate::run::execute` refuses E2): there is no other path that runs it.
///
/// Everything else stays in the digest, bytecode included: a bare `*.pyc` or `*.pyo` outside
/// `__pycache__` can be imported without any source, so it is code, and adding or changing one
/// lapses the approval, as does any source, configuration or interpreter change.
fn bytecode_cache(rel: &Path) -> bool {
    rel.components().any(|c| c.as_os_str() == "__pycache__")
}

/// The complete local dependency identity approved with one exact command. Paths to the base
/// runtime are local authority, never portable authority inherited from an imported pack.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectPython {
    pub adapter_version: u32,
    pub program: String,
    pub config_sha256: String,
    pub environment_sha256: String,
    pub executable: PathBuf,
    pub executable_sha256: String,
    pub runtime_root: PathBuf,
    pub runtime_sha256: String,
    pub version: String,
}

fn environment(program: &str) -> Option<&str> {
    let mut p = program.split('/');
    let dir = p.next()?;
    if !matches!(dir, ".venv" | "venv") || p.next()? != "bin" {
        return None;
    }
    let name = p.next()?;
    let suffix = name.strip_prefix("python")?;
    if p.next().is_some()
        || !(suffix.is_empty()
            || suffix == "3"
            || suffix.strip_prefix("3.").is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())))
    {
        return None;
    }
    Some(dir)
}

fn regular(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    checkpoint()?;
    let m = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !m.is_file() || m.len() > max {
        return Err("expected a bounded regular file".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    checkpoint()?;
    Ok(bytes)
}

fn entries(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut dirs = vec![root.to_path_buf()];
    let mut out = Vec::new();
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            checkpoint()?;
            let e = e.map_err(|e| e.to_string())?;
            let p = e.path();
            let dir = e.file_type().map_err(|e| e.to_string())?.is_dir();
            // Bytecode caches: never read by a staged run, never in a digest ([`bytecode_cache`]).
            if dir && e.file_name() == "__pycache__" {
                continue;
            }
            if dir {
                dirs.push(p.clone());
            }
            out.push(p);
            if out.len() > MAX_FILES {
                return Err("Python dependency tree exceeds the file bound".into());
            }
        }
    }
    out.sort();
    Ok(out)
}

fn tree_hash(root: &Path, executable: Option<&Path>) -> Result<String, String> {
    if !std::fs::symlink_metadata(root).map_err(|e| e.to_string())?.is_dir() {
        return Err("Python dependency root must be a real directory, not a symlink".into());
    }
    let mut hash = Sha256::new();
    let mut remaining = MAX_BYTES;
    for p in entries(root)? {
        checkpoint()?;
        let rel = p.strip_prefix(root).map_err(|e| e.to_string())?;
        // `entries` already skips `__pycache__`; this keeps the rule in one place.
        if bytecode_cache(rel) {
            continue;
        }
        hash.update(rel.as_os_str().as_encoded_bytes());
        hash.update([0]);
        let m = std::fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
        if m.is_dir() {
            hash.update(b"dir\0");
        } else if m.file_type().is_symlink() {
            let real = p.canonicalize().map_err(|e| e.to_string())?;
            let interpreter_alias = rel.parent() == Some(Path::new("bin"))
                && rel
                    .file_name()
                    .is_some_and(|s| environment(&format!(".venv/bin/{}", s.to_string_lossy())).is_some());
            if !real.starts_with(root) && !(interpreter_alias && executable == Some(real.as_path())) {
                return Err(format!("Python dependency symlink escapes its approved tree: {}", rel.display()));
            }
            let target = std::fs::read_link(&p).map_err(|e| e.to_string())?;
            if target.is_absolute() && !(interpreter_alias && executable == Some(real.as_path())) {
                return Err("absolute dependency symlinks cannot be relocated safely".into());
            }
            hash.update(b"link\0");
            hash.update(target.as_os_str().as_encoded_bytes());
        } else if m.is_file() {
            remaining = remaining.checked_sub(m.len()).ok_or("Python dependencies exceed the byte bound")?;
            let bytes = regular(&p, MAX_BYTES)?;
            hash.update(b"file\0");
            hash.update(crate::sha256_hex(&bytes));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                hash.update((m.permissions().mode() & 0o111).to_le_bytes());
            }
        } else {
            return Err("special files are not Python dependencies".into());
        }
        hash.update([0]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn pth_paths(env: &Path, repo: &Path, replacement: Option<&Path>) -> Result<(), String> {
    for p in entries(env)?.into_iter().filter(|p| p.extension().is_some_and(|e| e == "pth")) {
        let bytes = regular(&p, 64 << 10)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| "Python path file is not UTF-8")?;
        let mut lines = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            let path = Path::new(trimmed);
            if trimmed.starts_with("import ") || trimmed.starts_with("import\t") {
                // Local import hooks (including uv's) are approved code in the dependency
                // digest. An embedded old-checkout path cannot be safely relocated as code.
                if line.contains(repo.to_string_lossy().as_ref()) {
                    return Err("executable .pth code embeds the original checkout path".into());
                }
                lines.push(line.to_string());
            } else if trimmed.is_empty() || trimmed.starts_with('#') {
                lines.push(line.to_string());
            } else if path.is_absolute() {
                let real = path.canonicalize().map_err(|_| "Python path file names a missing path")?;
                let rel = real.strip_prefix(repo).map_err(|_| "Python path file points outside the repository")?;
                lines.push(
                    replacement.map_or_else(|| line.to_string(), |root| root.join(rel).to_string_lossy().into_owned()),
                );
            } else {
                let real = p
                    .parent()
                    .unwrap()
                    .join(path)
                    .canonicalize()
                    .map_err(|_| "Python path file names a missing path")?;
                if !real.starts_with(env) {
                    return Err("relative Python path file escapes the environment".into());
                }
                lines.push(line.to_string());
            }
        }
        if replacement.is_some() {
            std::fs::write(p, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Only an explicitly named local venv is considered. Bare Python and pytest are never replaced.
pub fn identify(repo: &Path, program: &str) -> Result<Option<ProjectPython>, String> {
    let Some(dir) = environment(program) else { return Ok(None) };
    let _budget = budget(crate::limits::E2_MAX_WALL_MS);
    let repo = repo.canonicalize().map_err(|e| e.to_string())?;
    let env = repo.join(dir);
    if std::fs::symlink_metadata(&env).map_err(|e| e.to_string())?.file_type().is_symlink() {
        return Err("the project Python environment must not be a symlink".into());
    }
    let cfg = regular(&env.join("pyvenv.cfg"), 64 << 10)?;
    let text = std::str::from_utf8(&cfg).map_err(|_| "pyvenv.cfg is not UTF-8")?;
    let value = |key: &str| {
        text.lines().find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == key).map(|(_, v)| v.trim()))
    };
    if value("include-system-site-packages") != Some("false") {
        return Err("project Python must exclude system site packages".into());
    }
    let version = value("version_info").or_else(|| value("version")).ok_or("pyvenv.cfg has no version")?;
    let mut parts = version.split('.');
    let (major, minor) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
    if major != "3" || minor.parse::<u32>().ok().is_none_or(|n| n < 8) {
        return Err("only a declared Python 3.8 or newer runtime is supported".into());
    }
    let version = format!("{major}.{minor}");
    let home = Path::new(value("home").ok_or("pyvenv.cfg has no home")?).canonicalize().map_err(|e| e.to_string())?;
    if home.file_name() != Some(std::ffi::OsStr::new("bin")) {
        return Err("project Python runtime must declare a bin directory".into());
    }
    let runtime_root = home.parent().ok_or("Python runtime has no root")?.to_path_buf();
    if runtime_root.parent().is_none() || repo.starts_with(&runtime_root) || runtime_root.starts_with(&repo) {
        return Err("Python runtime root is not a separate narrow installation".into());
    }
    if std::env::var_os("HOME")
        .and_then(|p| PathBuf::from(p).canonicalize().ok())
        .is_some_and(|p| p.starts_with(&runtime_root))
    {
        return Err("Python runtime cannot be the real home or its ancestor".into());
    }
    let stdlib = runtime_root.join(format!("lib/python{version}"));
    if !stdlib.join("encodings/__init__.py").is_file() || !stdlib.join("os.py").is_file() {
        return Err("the declared Python runtime has no matching standard library".into());
    }
    let executable = repo.join(program).canonicalize().map_err(|e| e.to_string())?;
    if executable.parent() != Some(home.as_path())
        || !executable.file_name().is_some_and(|n| n.to_string_lossy() == format!("python{version}"))
    {
        return Err("the environment interpreter does not resolve to its declared runtime".into());
    }
    let binary = regular(&executable, 128 << 20)?;
    if binary.starts_with(b"#!") || binary.len() < 4 {
        return Err("the Python interpreter must be a native executable, not a wrapper script".into());
    }
    pth_paths(&env, &repo, None)?;
    Ok(Some(ProjectPython {
        adapter_version: ADAPTER_VERSION,
        program: program.into(),
        config_sha256: crate::sha256_hex(&cfg),
        environment_sha256: tree_hash(&env, Some(&executable))?,
        executable_sha256: crate::sha256_hex(&binary),
        executable,
        runtime_sha256: tree_hash(&runtime_root.join("lib"), None)?,
        runtime_root,
        version,
    }))
}

pub(crate) fn validate_command(req: &ExecRequest) -> Result<(), String> {
    let Some(identity) = &req.project_python else { return Ok(()) };
    if req.level != Level::E2
        || req.effect != adage_core::Effect::Verify
        || req.action.as_ref().is_none_or(|a| a.kind != ActionKind::Verify)
        || req.argv.first() != Some(&identity.program)
    {
        return Err("project Python is only valid for its exact E2 verify command".into());
    }
    // A closed interpreter-prefix grammar prevents request flags from overriding the adapter's
    // bytecode policy. Arguments after pytest/the script belong to that approved application.
    let module = req.argv.get(1).is_some_and(|s| s == "-m") && req.argv.get(2).is_some_and(|s| s == "pytest");
    let script = req.argv.get(1).is_some_and(|s| s.ends_with(".py") && adage_core::repo::is_safe_relative(s));
    if !module && !script {
        return Err("project Python supports -m pytest or a repository .py script, with no interpreter flags".into());
    }
    if script && !req.scripts.iter().any(|s| req.argv.get(1) == Some(&s.path)) {
        return Err("a project Python script entrypoint requires its exact approved ScriptRef".into());
    }
    Ok(())
}

pub fn validate(req: &ExecRequest) -> Result<(), String> {
    validate_command(req)?;
    let Some(identity) = &req.project_python else { return Ok(()) };
    if identify(&req.repo_root, &identity.program)?.as_ref() != Some(identity) {
        return Err("project Python identity changed since approval".into());
    }
    Ok(())
}

/// A private copy preserves the staged input snapshot: rebasing editable .pth paths is runtime
/// preparation, never an allowed source mutation. Approved dependencies run under the sandbox.
pub(crate) fn prepare(req: &ExecRequest, repo: &Path, scratch: &Path, home: Option<&Path>) -> Result<PathBuf, String> {
    // run::check already validated the full live identity. Below we hash each copied snapshot
    // against that identity, so another complete scan of mutable originals adds no assurance.
    validate_command(req)?;
    let identity = req.project_python.as_ref().ok_or("no project Python identity")?;
    let dir = environment(&identity.program).ok_or("invalid project Python program")?;
    if home.and_then(|p| p.canonicalize().ok()).is_some_and(|p| p.starts_with(&identity.runtime_root)) {
        return Err("Python runtime grant would expose the real home directory".into());
    }
    let env = repo.join(dir);
    if tree_hash(&env, Some(&identity.executable))? != identity.environment_sha256 {
        return Err("staged Python dependencies differ from the approved environment".into());
    }
    let dest = scratch.join("project-python");
    crate::stage::copy_tree(&env, &dest, &mut MAX_FILES.clone(), &mut MAX_BYTES.clone())?;
    if tree_hash(&dest, Some(&identity.executable))? != identity.environment_sha256 {
        return Err("copied Python dependencies differ from the approved environment".into());
    }
    // Execute only a verified private runtime snapshot. Reading the original runtime after
    // hashing would permit an update to race imports or the interpreter's own execution.
    let runtime = scratch.join("python-runtime");
    std::fs::create_dir_all(runtime.join("bin")).map_err(|e| e.to_string())?;
    crate::stage::copy_tree(
        &identity.runtime_root.join("lib"),
        &runtime.join("lib"),
        &mut MAX_FILES.clone(),
        &mut MAX_BYTES.clone(),
    )?;
    if tree_hash(&runtime.join("lib"), None)? != identity.runtime_sha256 {
        return Err("copied Python runtime differs from the approved runtime".into());
    }
    // Caches the digest leaves out are not executed either (`-X pycache_prefix` already keeps the
    // interpreter from reading them; this is the second wall).
    remove_bytecode(&runtime.join("lib"), false)?;
    let binary = runtime.join("bin").join(identity.executable.file_name().ok_or("invalid interpreter name")?);
    std::fs::copy(&identity.executable, &binary).map_err(|e| e.to_string())?;
    if crate::sha256_hex(&regular(&binary, 128 << 20)?) != identity.executable_sha256 {
        return Err("copied Python interpreter differs from the approved interpreter".into());
    }
    for path in entries(&dest)? {
        if std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?.file_type().is_symlink()
            && path.canonicalize().map_err(|e| e.to_string())? == identity.executable
        {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&binary, &path).map_err(|e| e.to_string())?;
            #[cfg(not(unix))]
            return Err("project Python execution requires a Unix sandbox".into());
        }
    }
    let config_path = dest.join("pyvenv.cfg");
    let config = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let config = config
        .lines()
        .map(|line| {
            if line.split_once('=').is_some_and(|(k, _)| k.trim() == "home") {
                format!("home = {}", runtime.join("bin").display())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(config_path, config).map_err(|e| e.to_string())?;
    // Remove bytecode from the private environment rather than use cached dependencies.
    remove_bytecode(&dest, true)?;
    let original = req.repo_root.canonicalize().map_err(|e| e.to_string())?;
    pth_paths(&dest, &original, Some(repo))?;
    Ok(dest.join("bin").join(Path::new(&identity.program).file_name().ok_or("invalid interpreter name")?))
}

/// Remove every `__pycache__` directory under `root` (a private copy), and with `all_pyc` every
/// other `.pyc` file too.
fn remove_bytecode(root: &Path, all_pyc: bool) -> Result<(), String> {
    let mut dirs = vec![root.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(d).map_err(|e| e.to_string())? {
            checkpoint()?;
            let e = e.map_err(|e| e.to_string())?;
            let p = e.path();
            let dir = e.file_type().map_err(|e| e.to_string())?.is_dir();
            if dir && e.file_name() == "__pycache__" {
                std::fs::remove_dir_all(p).map_err(|e| e.to_string())?;
            } else if dir {
                dirs.push(p);
            } else if all_pyc && p.extension().is_some_and(|x| x == "pyc") {
                std::fs::remove_file(p).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::request::{ActionSpec, OutputSpec};
    use crate::sandbox::Profile;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let repo = root.join("repo");
        for p in [
            "repo/.venv/bin",
            "repo/.venv/lib/python3.12/site-packages",
            "repo/src",
            "runtime/bin",
            "runtime/lib/python3.12/encodings",
        ] {
            std::fs::create_dir_all(root.join(p)).unwrap();
        }
        std::fs::write(root.join("runtime/bin/python3.12"), b"\x7fELFfixture-only").unwrap();
        std::fs::write(root.join("runtime/lib/python3.12/os.py"), "# fixture\n").unwrap();
        std::fs::write(root.join("runtime/lib/python3.12/encodings/__init__.py"), "# fixture\n").unwrap();
        std::fs::write(
            repo.join(".venv/pyvenv.cfg"),
            format!(
                "home = {}/runtime/bin\nversion_info = 3.12\ninclude-system-site-packages = false\n",
                root.display()
            ),
        )
        .unwrap();
        std::os::unix::fs::symlink(root.join("runtime/bin/python3.12"), repo.join(".venv/bin/python")).unwrap();
        std::os::unix::fs::symlink("python", repo.join(".venv/bin/python3")).unwrap();
        std::fs::write(repo.join(".venv/lib/python3.12/site-packages/_virtualenv.pth"), "import _virtualenv\n")
            .unwrap();
        std::fs::write(repo.join(".venv/lib/python3.12/site-packages/_virtualenv.py"), "# approved dependency code\n")
            .unwrap();
        std::fs::write(
            repo.join(".venv/lib/python3.12/site-packages/project.pth"),
            format!("{}/src\n", repo.display()),
        )
        .unwrap();
        (dir, repo)
    }

    fn request(repo: &Path) -> ExecRequest {
        let mut req = crate::request::tests::req(&[".venv/bin/python", "-m", "pytest"]);
        req.repo_root = repo.to_path_buf();
        req.level = Level::E2;
        req.effect = adage_core::Effect::Verify;
        req.profile = Profile::for_level(Level::E2);
        req.output = OutputSpec::default();
        req.action = Some(ActionSpec { kind: ActionKind::Verify, outputs: vec![], cache: vec![] });
        req.project_python = identify(repo, &req.argv[0]).unwrap();
        req
    }

    #[test]
    fn only_explicit_project_python_binds_and_absent_identity_preserves_old_digest() {
        let (_d, repo) = fixture();
        for bare in ["python", "python3", "pytest", "../.venv/bin/python", ".venv/bin/sh"] {
            assert!(identify(&repo, bare).unwrap().is_none(), "{bare}");
        }
        let mut ordinary = crate::request::tests::req(&["git", "status"]);
        let old = serde_json::json!({ "argv": ordinary.argv, "contracts": ordinary.contracts, "output": ordinary.output, "scripts": ordinary.scripts, "effect": ordinary.effect });
        assert_eq!(ordinary.exec_digest(), crate::sha256_hex(crate::canonical_json(&old).as_bytes()));
        ordinary.route_exec_digest = Some("existing-pack-digest".into());
        assert_eq!(ordinary.exec_digest(), "existing-pack-digest");
        ordinary.project_python = identify(&repo, ".venv/bin/python").unwrap();
        assert_ne!(ordinary.exec_digest(), "existing-pack-digest");
        assert!(validate(&ordinary).is_err(), "an E1 request cannot borrow the identity");
    }

    #[test]
    fn environment_configuration_dependency_interpreter_and_runtime_changes_lapse_grants() {
        for changed in [
            "repo/.venv/pyvenv.cfg",
            "repo/.venv/lib/python3.12/site-packages/_virtualenv.py",
            "runtime/bin/python3.12",
            "runtime/lib/python3.12/os.py",
        ] {
            let (d, repo) = fixture();
            let old = request(&repo);
            validate(&old).unwrap();
            let key = crate::approvals::ApprovalKey::for_request(&old, "repo");
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .open(d.path().join(changed))
                .unwrap()
                .write_all(b"\n# changed\n")
                .unwrap();
            assert!(validate(&old).is_err(), "{changed}");
            let new = request(&repo);
            assert_ne!(key, crate::approvals::ApprovalKey::for_request(&new, "repo"), "{changed}");
        }
    }

    #[test]
    fn interpreter_options_and_adapter_changes_are_refused() {
        let (_d, repo) = fixture();
        for argv in [
            vec![".venv/bin/python", "-X", "pycache_prefix=/tmp", "-m", "pytest"],
            vec![".venv/bin/python", "-Xpycache_prefix=/tmp", "-m", "pytest"],
            vec![".venv/bin/python", "-c", "print(1)"],
            vec![".venv/bin/python", "../outside.py"],
        ] {
            let mut req = request(&repo);
            req.argv = argv.into_iter().map(str::to_owned).collect();
            assert!(validate(&req).is_err());
        }
        let mut req = request(&repo);
        let before = req.exec_digest();
        req.project_python.as_mut().unwrap().adapter_version += 1;
        assert_ne!(before, req.exec_digest());
        assert!(validate(&req).is_err());
    }

    #[test]
    fn script_entrypoints_remain_hash_bound_in_route_planning() {
        let (_d, repo) = fixture();
        for path in ["check.py", "tools/check.py"] {
            let file = repo.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "print('verify')\n").unwrap();
            let command = format!(".venv/bin/python {path}");
            let ir = adage_tracks::index::IndexedRoute {
                repo: "fixture".into(),
                track: "fixture/learned".into(),
                version: "0.1.0".into(),
                exec: None,
                route: adage_tracks::Route {
                    id: "verify".into(),
                    summary: String::new(),
                    trigger: adage_tracks::Trigger::AfterEdit { paths: "src/*.py".into() },
                    step: adage_tracks::Step::Run { command },
                    slots: vec![],
                    effect: adage_core::Effect::Verify,
                    strength: adage_tracks::Strength::Fire,
                    state: adage_tracks::RouteState::Firm,
                    exit: vec![],
                    evidence: Default::default(),
                    authored: false,
                },
            };
            let index = adage_tracks::Index { routes: vec![ir.clone()], ..Default::default() };
            let mut req =
                crate::route::plan(&ir, &index, &repo, &repo, Level::E2, None, &adage_core::CommandTable::load())
                    .unwrap()
                    .request;
            assert_eq!(req.scripts.len(), 1);
            assert_eq!(req.scripts[0].path, path);
            validate(&req).unwrap();
            req.scripts.clear();
            assert!(validate(&req).unwrap_err().contains("ScriptRef"));
        }
    }

    /// A bare `.pyc` or `.pyo` outside `__pycache__` is importable without any source: injected
    /// bytecode is code, and it lapses the approval, beside its source or not.
    #[test]
    fn dependency_and_runtime_bytecode_churn_lapses_approval() {
        for relative in [
            "runtime/lib/python3.12/injected.pyc",
            "runtime/lib/python3.12/os.pyc",
            "runtime/lib/python3.12/legacy.pyo",
            "repo/.venv/lib/python3.12/site-packages/speedups.pyc",
        ] {
            let (d, repo) = fixture();
            let old = request(&repo);
            std::fs::write(d.path().join(relative), b"unapproved bytecode").unwrap();
            assert!(validate(&old).is_err(), "{relative}");
            assert_ne!(old.exec_digest(), request(&repo).exec_digest(), "{relative}");
        }
    }

    /// Bytecode caches Python writes into `__pycache__` as it runs, in `.venv` or the runtime
    /// `lib` (a stdlib `__pycache__` filled by any ordinary run), never lapse an approval: a
    /// staged run never reads them ([`bytecode_cache`]).
    #[test]
    fn pycache_churn_never_lapses_an_approval() {
        let (d, repo) = fixture();
        let old = request(&repo);
        let key = crate::approvals::ApprovalKey::for_request(&old, "repo");
        for cache in [
            "runtime/lib/python3.12/__pycache__/os.cpython-312.pyc",
            "runtime/lib/python3.12/encodings/__pycache__/__init__.cpython-312.pyc",
            "repo/.venv/lib/python3.12/site-packages/__pycache__/_virtualenv.cpython-312.pyc",
        ] {
            let file = d.path().join(cache);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, b"bytecode written by an ordinary run").unwrap();
            validate(&old).unwrap_or_else(|e| panic!("{cache}: {e}"));
            assert_eq!(key, crate::approvals::ApprovalKey::for_request(&request(&repo), "repo"), "{cache}");
        }
    }

    #[test]
    fn forged_broad_runtime_and_escaping_dependency_paths_are_refused() {
        let (d, repo) = fixture();
        let mut req = request(&repo);
        req.project_python.as_mut().unwrap().runtime_root = "/".into();
        assert!(validate(&req).is_err());
        let outside = d.path().join("outside");
        std::fs::write(&outside, "private").unwrap();
        let link = repo.join(".venv/lib/python3.12/site-packages/escape.py");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(identify(&repo, ".venv/bin/python").is_err());
        std::fs::remove_file(link).unwrap();
        let pth = repo.join(".venv/lib/python3.12/site-packages/project.pth");
        std::fs::write(&pth, d.path().display().to_string()).unwrap();
        assert!(identify(&repo, ".venv/bin/python").is_err());
        std::fs::write(&pth, format!("import sys; sys.path.insert(0, {:?})", repo.display().to_string())).unwrap();
        assert!(identify(&repo, ".venv/bin/python").is_err());
    }

    #[test]
    fn discovery_and_copy_obey_an_expired_action_deadline() {
        let (d, repo) = fixture();
        let _budget = budget(0);
        assert!(identify(&repo, ".venv/bin/python").unwrap_err().contains("budget"));
        assert!(crate::stage::copy_tree(&repo, &d.path().join("copy"), &mut MAX_FILES.clone(), &mut MAX_BYTES.clone())
            .unwrap_err()
            .contains("budget"));
    }

    #[test]
    fn symlinked_runtime_library_root_is_refused() {
        let (d, repo) = fixture();
        let lib = d.path().join("runtime/lib");
        let outside = d.path().join("outside-library");
        std::fs::rename(&lib, &outside).unwrap();
        std::os::unix::fs::symlink(&outside, &lib).unwrap();
        assert!(identify(&repo, ".venv/bin/python").is_err());
    }

    #[test]
    fn preparation_rebases_plain_editable_paths_without_mutating_staged_inputs() {
        let (d, repo) = fixture();
        let req = request(&repo);
        // Written after approval by an ordinary run: still approved, and never copied in to run.
        let cache = d.path().join("runtime/lib/python3.12/__pycache__/os.cpython-312.pyc");
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::fs::write(&cache, b"cached").unwrap();
        let staged = d.path().join("staged");
        crate::stage::copy_tree(&repo, &staged, &mut MAX_FILES.clone(), &mut MAX_BYTES.clone()).unwrap();
        let scratch = d.path().join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        let python = prepare(&req, &staged, &scratch, None).unwrap();
        assert!(python.canonicalize().unwrap().starts_with(scratch.canonicalize().unwrap().join("python-runtime/bin")));
        assert!(!scratch.join("python-runtime/lib/python3.12/__pycache__").exists(), "caches are not executed");
        std::fs::write(d.path().join("runtime/lib/python3.12/os.py"), "changed after snapshot").unwrap();
        assert_eq!(
            std::fs::read_to_string(scratch.join("python-runtime/lib/python3.12/os.py")).unwrap(),
            "# fixture\n"
        );
        assert_eq!(python, scratch.join("project-python/bin/python"));
        let rel = ".venv/lib/python3.12/site-packages/project.pth";
        assert_eq!(std::fs::read(repo.join(rel)).unwrap(), std::fs::read(staged.join(rel)).unwrap());
        let relocated =
            std::fs::read_to_string(scratch.join("project-python/lib/python3.12/site-packages/project.pth")).unwrap();
        assert_eq!(relocated.trim(), staged.join("src").to_str().unwrap());
    }
}
