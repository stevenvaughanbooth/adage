//! Asking an installed Adage binary which version it is (`<bin> version`, which prints `adage
//! 0.1.0` or `adage-exec 0.1.0` and exits), and handing an update over to the binary it
//! installed (`<new> update --finish <from>`), so the steps after the package manager run with the
//! new version's code: its registrations, its plugin files, its doctor.
//!
//! Only binaries that look like Adage's are asked ([`askable`]): named `adage` or `adage-exec`
//! (or the plugin's digest-named copy of one), absolute, and outside the repository `adage doctor`
//! runs in, so a repository's own settings file cannot make doctor run a program it ships. Each
//! answer is remembered for the life of the process (by path, size and modification time):
//! `adage ui` re-runs doctor every few seconds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// How long a binary may take to answer `version`.
const TIMEOUT: Duration = Duration::from_secs(3);

fn command(bin: &Path) -> Command {
    Command::new(bin)
}

/// The version in a `version` answer's first line: `adage 0.2.0`, `adage-exec 0.2.0`, or a newer
/// binary's longer line (`adage 0.2.0 · 0.3.0 is out (adage update)`).
pub(crate) fn version_line(out: &str) -> Option<String> {
    let line = out.lines().next()?.trim();
    let mut words = line.split_whitespace();
    let name = words.next()?;
    if name != "adage" && name != "adage-exec" {
        return None;
    }
    let v = words.next()?;
    adage_store::release::Version::parse(v).map(|_| v.trim_start_matches('v').to_string())
}

/// Is `name` (a file name, `.exe` dropped) one of Adage's binaries, or the plugin's digest-named
/// copy of one (`adage-<64 hex>`)?
fn adage_name(name: &str) -> bool {
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let digest = |rest: &str| rest.len() == 64 && rest.bytes().all(|b| b.is_ascii_hexdigit());
    matches!(name, "adage" | "adage-exec")
        || name.strip_prefix("adage-exec-").is_some_and(digest)
        || name.strip_prefix("adage-").is_some_and(digest)
}

/// May `bin` be asked its version from a directory inside `repo`? See the module doc.
pub(crate) fn askable(bin: &Path, repo: Option<&Path>) -> bool {
    if !bin.is_absolute() || !crate::detect::executable(bin) {
        return false;
    }
    let resolved = bin.canonicalize().unwrap_or_else(|_| bin.to_path_buf());
    // The file that runs is named as Adage's (npm's launcher is `adage.js`), wherever the path
    // asked for links from: a link named `adage` is not enough.
    let named = resolved.file_name().is_some_and(|n| {
        let n = n.to_string_lossy();
        adage_name(&n) || n == "adage.js"
    });
    let roots: Vec<PathBuf> =
        repo.into_iter().flat_map(|r| [r.to_path_buf(), r.canonicalize().unwrap_or_default()]).collect();
    let inside = |p: &Path| roots.iter().any(|r| !r.as_os_str().is_empty() && p.starts_with(r));
    named && !inside(bin) && !inside(&resolved)
}

type Key = (PathBuf, u64, Option<SystemTime>);
static ANSWERS: Mutex<Option<HashMap<Key, Option<String>>>> = Mutex::new(None);

/// The version the binary at `bin` answers, or `None` (it is not one of Adage's, failed, or took
/// longer than three seconds). Ask only what [`askable`] allows.
pub(crate) fn version_of(bin: &Path) -> Option<String> {
    let resolved = bin.canonicalize().ok()?;
    let meta = std::fs::metadata(&resolved).ok()?;
    let key = (resolved.clone(), meta.len(), meta.modified().ok());
    if let Some(v) = ANSWERS.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(&key).cloned())) {
        return v;
    }
    let v = ask(&resolved);
    if let Ok(mut m) = ANSWERS.lock() {
        m.get_or_insert_with(HashMap::new).insert(key, v.clone());
    }
    v
}

/// Run `<bin> version` with no input, its output captured, killed after [`TIMEOUT`].
fn ask(bin: &Path) -> Option<String> {
    let mut child = command(bin)
        .arg("version")
        // Its own answer, not an npm launcher's lookup or a colour code.
        .env_remove("ADAGE_NPM_WHICH")
        .env("ADAGE_COLOR", "never")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = child.wait_with_output().ok()?;
    out.status.success().then(|| version_line(&String::from_utf8_lossy(&out.stdout))).flatten()
}

/// Run `<bin> update --finish <from>` with the terminal attached: the binary an update installed
/// finishes it. Its exit status says how: 0 finished, 2 it does not know `--finish` (a binary from
/// before this command existed: the caller finishes instead, and that binary's usage error is not
/// shown), anything else failed (its error is shown once it exits). Its standard error is held
/// until then; its output and any question it asks reach the terminal as they happen.
pub(crate) fn hand_over(bin: &Path, from: &str, extra: &[&str]) -> std::io::Result<ExitStatus> {
    let child = command(bin).args(["update", "--finish", from]).args(extra).stderr(Stdio::piped()).spawn()?;
    let out = child.wait_with_output()?;
    if out.status.code() != Some(2) {
        use std::io::Write;
        let _ = std::io::stderr().write_all(&out.stderr);
    }
    Ok(out.status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_answer_is_read_from_its_first_line() {
        assert_eq!(version_line("adage 0.1.0\n").as_deref(), Some("0.1.0"));
        assert_eq!(version_line("adage-exec 0.2.0-rc.1\n").as_deref(), Some("0.2.0-rc.1"));
        assert_eq!(version_line("adage 0.2.0 · 0.3.0 is out (adage update)").as_deref(), Some("0.2.0"));
        for not in ["", "rm: cannot remove 'version'", "adage version", "something 0.1.0", "adage"] {
            assert_eq!(version_line(not), None, "{not:?}");
        }
    }

    #[test]
    fn only_adage_named_binaries_outside_the_repository_are_asked() {
        let d = tempfile::tempdir().unwrap();
        let exe = |rel: &str| {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "#!/bin/sh\necho adage 9.9.9\n").unwrap();
            crate::net::make_executable(&p).unwrap();
            p
        };
        let ok = exe("home/.cargo/bin/adage");
        let digest = exe(&format!("data/bin/adage-{}", "a".repeat(64)));
        let other = exe("usr/bin/rm");
        let shipped = exe("repo/tools/adage");
        let repo = d.path().join("repo");
        assert!(askable(&ok, Some(&repo)));
        assert!(askable(&digest, Some(&repo)), "the plugin's digest-named copy");
        assert!(!askable(&other, Some(&repo)), "not Adage's name");
        assert!(!askable(&shipped, Some(&repo)), "a binary the repository itself ships");
        assert!(!askable(Path::new("adage"), None), "relative");
        #[cfg(unix)]
        {
            // A link named adage to another program, inside the repository or not, is not asked.
            std::os::unix::fs::symlink(&other, d.path().join("home/adage")).unwrap();
            assert!(!askable(&d.path().join("home/adage"), None));
            std::os::unix::fs::symlink(&other, repo.join("adage")).unwrap();
            assert!(!askable(&repo.join("adage"), Some(&repo)));
            // The plugin's stable link to its digest-named copy is.
            std::os::unix::fs::symlink(&digest, d.path().join("data/bin/adage")).unwrap();
            assert!(askable(&d.path().join("data/bin/adage"), Some(&repo)));
            assert_eq!(version_of(&ok).as_deref(), Some("9.9.9"));
        }
    }
}
