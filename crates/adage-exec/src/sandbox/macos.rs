//! macOS: `sandbox-exec -p <profile>` with a generated SBPL profile (Track C §3.3), the approach
//! Codex CLI uses. `(deny default)`, then: process exec and fork, sysctl reads, reads of the
//! repository, system, toolchain and CAS paths, writes to the scratch directory and `/dev/null`
//! only, and no network.
//!
//! The generator is plain string building and is unit-tested on every OS; running it needs macOS
//! and is covered by the macOS CI runner.

use super::Resolved;

/// Apple tool shims in /usr/bin load libxcrun and tools from the selected developer directory.
/// Ask the fixed system selector with no inherited DEVELOPER_DIR or loader environment. Grant
/// only its canonical Developer/CommandLineTools directory, never /Applications as a whole.
#[cfg(target_os = "macos")]
pub(crate) fn developer_dir() -> Option<std::path::PathBuf> {
    static DIR: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let output = std::process::Command::new("/usr/bin/xcode-select")
            .arg("-p")
            .env_clear()
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() || output.stdout.len() > 4096 {
            return None;
        }
        let raw = std::str::from_utf8(&output.stdout).ok()?.trim();
        let path = std::path::Path::new(raw);
        if !path.is_absolute() {
            return None;
        }
        let path = path.canonicalize().ok()?;
        if !matches!(path.file_name()?.to_str()?, "Developer" | "CommandLineTools") || !path.join("usr").is_dir() {
            return None;
        }
        Some(path)
    })
    .clone()
}

/// Apple's /usr/bin/python3 is a libxcrun launcher, not the interpreter. With a fresh scratch
/// HOME its toolchain discovery can launch xcodebuild repeatedly and exhaust E1's wall budget
/// before Python starts. Resolve that one system shim to the selected developer interpreter;
/// the same canonical developer directory is already included in the sandbox's read grants.
/// User-installed interpreters and all other commands retain their ordinary PATH resolution.
#[cfg(target_os = "macos")]
pub(crate) fn interpreter(program: std::path::PathBuf) -> std::path::PathBuf {
    if program != std::path::Path::new("/usr/bin/python3") {
        return program;
    }
    let Some(developer) = developer_dir() else { return program };
    let Ok(real) = developer.join("usr/bin/python3").canonicalize() else { return program };
    use std::os::unix::fs::PermissionsExt;
    if real.starts_with(&developer) && real.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    {
        real
    } else {
        program
    }
}

/// Count the real UID's existing processes without invoking a tool under the limit we are
/// calculating. RLIMIT_NPROC is per UID on macOS too, not per sandbox. Snapshot races remain
/// possible, but unrelated existing processes must not consume the run's entire allowance.
#[cfg(target_os = "macos")]
pub(crate) fn processes_of(uid: u32) -> std::io::Result<u64> {
    use std::io;
    unsafe extern "C" {
        fn proc_listpids(kind: u32, info: u32, buffer: *mut libc::c_void, bytes: libc::c_int) -> libc::c_int;
    }
    const PROC_RUID_ONLY: u32 = 5;
    let mut slots = 1024usize;
    for _ in 0..8 {
        let mut pids = vec![0i32; slots];
        let bytes = (pids.len() * std::mem::size_of::<i32>()) as libc::c_int;
        // SAFETY: the allocated buffer has exactly `bytes` writable bytes. The selector and
        // real UID follow libproc.h/sys/proc_info.h's proc_listpids contract.
        let got = unsafe { proc_listpids(PROC_RUID_ONLY, uid, pids.as_mut_ptr().cast(), bytes) };
        if got < 0 {
            return Err(io::Error::last_os_error());
        }
        if got < bytes {
            return Ok(pids[..got as usize / std::mem::size_of::<i32>()].iter().filter(|&&p| p > 0).count() as u64);
        }
        slots *= 2;
    }
    Err(io::Error::other("the UID process census exceeds its bound"))
}

/// Quote a path as an SBPL string literal.
fn lit(p: &std::path::Path) -> String {
    let s = p.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The SBPL profile for a resolved sandbox.
pub fn sbpl(res: &Resolved) -> String {
    let mut read: Vec<String> = res.read.iter().map(|p| format!("(subpath {})", lit(p))).collect();
    read.extend(res.dev.iter().map(|p| format!("(literal {})", lit(p))));
    // macOS 26's dyld cache loader needs to open the root directory itself. Without this
    // literal-node read, even /bin/sh aborts before main. Never use (subpath "/") here:
    // descendants still require one of the explicit read grants below.
    format!(
        "(version 1)\n\
         (deny default)\n\
         (allow process-exec)\n\
         (allow process-fork)\n\
         (allow sysctl-read)\n\
         (allow signal (target same-sandbox))\n\
         (allow file-read-metadata)\n\
         (allow file-read-data (literal \"/\"))\n\
         (allow file-read-data (literal \"/Library/Preferences/com.apple.dt.Xcode.plist\"))\n\
         (allow file-read* {read})\n\
         (allow file-read* file-write* (subpath {scratch}) (literal \"/dev/null\"))\n\
         (deny network*)\n",
        read = read.join(" "),
        scratch = lit(&res.scratch),
    )
}

/// The full argv: `sandbox-exec -p <profile> -- program args…`.
pub fn wrap(
    sandbox_exec: &std::path::Path,
    program: &std::path::Path,
    rest: &[String],
    res: &Resolved,
) -> (std::path::PathBuf, Vec<String>) {
    let mut a = vec!["-p".to_string(), sbpl(res), "--".into(), program.to_string_lossy().into_owned()];
    a.extend(rest.iter().cloned());
    (sandbox_exec.to_path_buf(), a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;
    use crate::request::Level;
    use std::path::PathBuf;

    #[cfg(target_os = "macos")]
    #[test]
    fn process_census_includes_the_current_users_existing_processes() {
        // SAFETY: getuid has no preconditions. This is a read-only native census; no fork
        // pressure or host-wide process limits are changed by the test.
        let count = processes_of(unsafe { libc::getuid() }).unwrap();
        assert!(count > 0, "the current test process must be counted");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_python_shim_resolves_only_within_the_selected_developer_directory() {
        for p in ["/usr/local/bin/python3", "/usr/bin/git", "/bin/sh", "/repo/python3"] {
            assert_eq!(interpreter(p.into()), std::path::Path::new(p));
        }
        let Some(developer) = developer_dir() else { return };
        let Ok(real) = developer.join("usr/bin/python3").canonicalize() else { return };
        if real.starts_with(&developer) && real.is_file() {
            assert_eq!(interpreter("/usr/bin/python3".into()), real);
        }
    }

    #[test]
    fn profile_denies_by_default_and_quotes_paths() {
        let res = Resolved {
            repo: PathBuf::from("/Users/a/repo"),
            cwd: PathBuf::from("/Users/a/repo"),
            scratch: PathBuf::from("/private/tmp/s"),
            read: vec![PathBuf::from("/Users/a/re\"po"), PathBuf::from("/usr")],
            dev: vec![PathBuf::from("/dev/null")],
            links: vec![],
            limits: Limits::for_level(Level::E1),
            drop_to: None,
            overlay: None,
            bind: None,
        };
        let p = sbpl(&res);
        assert!(p.starts_with("(version 1)\n(deny default)\n"));
        assert!(p.contains("(subpath \"/Users/a/re\\\"po\")"));
        assert!(p.contains("(allow file-read* file-write* (subpath \"/private/tmp/s\") (literal \"/dev/null\"))"));
        assert!(p.contains("(deny network*)"));
        assert!(p.contains("(allow file-read-data (literal \"/\"))"));
        assert!(!p.contains("(subpath \"/\")"), "root access must not include descendants");
        assert!(!p.contains("(allow network"));
        assert_eq!(p.matches("file-write*").count(), 1, "writes only to scratch and /dev/null");
    }
}
