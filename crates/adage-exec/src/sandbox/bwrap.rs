//! The Linux fallback when Landlock is missing: bubblewrap (Track C §3.3). Namespaces instead of
//! an LSM: all of them unshared (network included), only the read paths bound read-only, a fresh
//! `/dev` and `/proc`, the scratch directory bound read-write, and the process tree killed with its
//! parent. No seccomp filter is added here (bwrap itself needs `mount` and `clone` with namespace
//! flags); the network namespace, not a filter, denies sockets their reach.

use std::path::Path;

use super::Resolved;

/// bwrap's arguments, up to and including `--`; the program and its arguments follow.
pub fn args(res: &Resolved) -> Vec<String> {
    let mut a: Vec<String> =
        ["--unshare-all", "--die-with-parent", "--new-session", "--cap-drop", "ALL"].map(String::from).to_vec();
    for p in &res.read {
        let s = p.to_string_lossy().into_owned();
        match &res.overlay {
            // E2: the repository is a writable overlay whose upper layer bwrap mounts from
            // outside the sandbox (bubblewrap 0.10 and later).
            Some(o) if p == &res.repo => a.extend([
                "--overlay-src".into(),
                s.clone(),
                "--overlay".into(),
                o.upper.to_string_lossy().into_owned(),
                o.work.to_string_lossy().into_owned(),
                s,
            ]),
            _ => a.extend(["--ro-bind".into(), s.clone(), s]),
        }
    }
    for (link, target) in &res.links {
        a.extend(["--symlink".into(), target.to_string_lossy().into_owned(), link.to_string_lossy().into_owned()]);
    }
    a.extend(["--dev".into(), "/dev".into(), "--proc".into(), "/proc".into()]);
    let scratch = res.scratch.to_string_lossy().into_owned();
    a.extend(["--bind".into(), scratch.clone(), scratch]);
    // Root does not reach bwrap: the spawn drops to nobody first (`run::basic_pre_exec`), so bwrap
    // runs unprivileged in its own user namespace and `RLIMIT_NPROC` applies.
    a.extend(["--chdir".into(), res.cwd.to_string_lossy().into_owned(), "--".into()]);
    a
}

/// The full argv: bwrap, its arguments, then the program and its arguments.
pub fn wrap(bwrap: &Path, program: &Path, rest: &[String], res: &Resolved) -> (std::path::PathBuf, Vec<String>) {
    let mut a = args(res);
    a.push(program.to_string_lossy().into_owned());
    a.extend(rest.iter().cloned());
    (bwrap.to_path_buf(), a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;
    use crate::request::Level;
    use std::path::PathBuf;

    #[test]
    fn binds_only_what_the_profile_reads() {
        let res = Resolved {
            repo: PathBuf::from("/r"),
            cwd: PathBuf::from("/r"),
            scratch: PathBuf::from("/tmp/s"),
            read: vec![PathBuf::from("/r"), PathBuf::from("/usr")],
            dev: vec![],
            links: vec![],
            limits: Limits::for_level(Level::E1),
            drop_to: None,
            overlay: None,
        };
        let a = args(&res).join(" ");
        assert_eq!(
            a,
            "--unshare-all --die-with-parent --new-session --cap-drop ALL --ro-bind /r /r --ro-bind /usr /usr \
             --dev /dev --proc /proc --bind /tmp/s /tmp/s --chdir /r --"
        );
        assert!(!a.contains("--ro-bind / /"), "never the whole filesystem");
        let staged = Resolved {
            overlay: Some(super::super::Overlay { upper: PathBuf::from("/st/u"), work: PathBuf::from("/st/w") }),
            ..res
        };
        let a = args(&staged).join(" ");
        assert!(a.contains("--overlay-src /r --overlay /st/u /st/w /r --ro-bind /usr /usr"), "{a}");
    }
}
