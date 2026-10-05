//! Files a cloud drive has offloaded: iCloud Drive with "Optimize Mac Storage", and other macOS
//! sync apps built on File Provider. Such a file is still listed in its folder, but its contents
//! are only in the cloud, and the first read of it waits for a download. A repository kept in a
//! synced folder (`~/Documents` and `~/Desktop` under iCloud's "Desktop & Documents Folders") loses
//! files this way: old git objects, and files no one has opened for a while.
//!
//! Adage never waits on the cloud: no hook and no learner touches the network
//! (docs/zero-config.md §1), and a hook has seconds to answer. [`never_download`] makes reading an
//! offloaded file fail at once in this process, so every reader skips it like any file it cannot
//! read; [`is_offloaded`] lets `adage doctor` say which files were skipped and why.

use std::path::Path;

/// Make reads of offloaded files fail at once in this process instead of downloading them.
///
/// The policy is inherited by the processes this one starts, so a command that runs other
/// programs on the person's behalf (an agent session, an installer) must not call it. It does
/// nothing outside macOS, or where the system refuses it (the default then stands).
pub fn never_download() {
    #[cfg(target_os = "macos")]
    {
        use std::os::raw::c_int;
        // <sys/resource.h>: the dataless-file iotype (macOS 10.15 and later) at process scope.
        const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: c_int = 3;
        const IOPOL_SCOPE_PROCESS: c_int = 0;
        const IOPOL_MATERIALIZE_DATALESS_FILES_OFF: c_int = 1;
        extern "C" {
            fn setiopolicy_np(iotype: c_int, scope: c_int, policy: c_int) -> c_int;
        }
        // SAFETY: a system call that takes three integers and changes only this process's I/O
        // policy; on failure it returns -1 and nothing changes.
        unsafe {
            setiopolicy_np(
                IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
                IOPOL_SCOPE_PROCESS,
                IOPOL_MATERIALIZE_DATALESS_FILES_OFF,
            );
        }
    }
}

/// Whether `path` is an offloaded file: listed here, with its contents only in the cloud.
/// Reads the file's flags, never its contents, so asking never starts a download.
pub fn is_offloaded(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        // <sys/stat.h>: SF_DATALESS.
        const SF_DATALESS: u32 = 0x4000_0000;
        std::fs::symlink_metadata(path).is_ok_and(|m| m.st_flags() & SF_DATALESS != 0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_file_is_not_offloaded_and_still_reads_under_the_policy() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("Makefile");
        std::fs::write(&f, "test:\n\tcargo test\n").unwrap();
        assert!(!is_offloaded(&f));
        assert!(!is_offloaded(&d.path().join("missing")));
        never_download();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "test:\n\tcargo test\n");
    }
}
