//! `adage train …`: grow reflexes from a skill (docs/training.md).
//!
//! Training drives an agent harness in sandboxed sessions, and the `adage` binary never starts
//! agent sessions itself (Q4a). So `adage train` only hands over to the separate `adage-train`
//! binary, with the same arguments: on Unix it replaces this process (`exec`), elsewhere it runs
//! it and exits with its status. It looks for `adage-train` beside this binary, then on `PATH`.

use anyhow::{Context, Result};
use std::path::PathBuf;

fn exe() -> &'static str {
    if cfg!(windows) {
        "adage-train.exe"
    } else {
        "adage-train"
    }
}

/// `adage-train` beside the running `adage`, else the bare name for a `PATH` lookup.
fn locate() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(exe())))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(exe()))
}

pub fn train(args: &[String]) -> Result<()> {
    let bin = locate();
    let mut cmd = std::process::Command::new(&bin);
    cmd.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = cmd.exec();
        Err(e).with_context(|| {
            format!("cannot run {}: install adage-train beside adage or on PATH (docs/training.md)", bin.display())
        })
    }
    #[cfg(not(unix))]
    {
        let status = cmd.status().with_context(|| {
            format!("cannot run {}: install adage-train beside adage or on PATH (docs/training.md)", bin.display())
        })?;
        std::process::exit(status.code().unwrap_or(1));
    }
}
