//! macOS: `sandbox-exec -p <profile>` with a generated SBPL profile (Track C §3.3), the approach
//! Codex CLI uses. `(deny default)`, then: process exec and fork, sysctl reads, reads of the
//! repository, system, toolchain and CAS paths, writes to the scratch directory and `/dev/null`
//! only, and no network.
//!
//! The generator is plain string building and is unit-tested on every OS; running it needs macOS
//! and is covered by the macOS CI runner.

use super::Resolved;

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
    format!(
        "(version 1)\n\
         (deny default)\n\
         (allow process-exec)\n\
         (allow process-fork)\n\
         (allow sysctl-read)\n\
         (allow signal (target same-sandbox))\n\
         (allow file-read-metadata)\n\
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
        assert!(!p.contains("(allow network"));
        assert_eq!(p.matches("file-write*").count(), 1, "writes only to scratch and /dev/null");
    }
}
