//! Person-authorized local history configuration and bounded OpenCode export.
//! These operations return data; front ends choose how to present it.
use adage_store::Paths;
use anyhow::{bail, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct Imported {
    pub count: usize,
    pub destination: PathBuf,
}

pub fn set_source(paths: &Paths, harness: &str, directory: &Path, _person: &crate::Person) -> Result<PathBuf> {
    let key = match harness {
        "claude-code" => "learn.claude_projects",
        "codex" => "learn.codex_sessions",
        "opencode" => "learn.opencode_exports",
        _ => bail!("Choose Claude Code, Codex, or OpenCode."),
    };
    let directory = directory.canonicalize().context("Choose an accessible history folder")?;
    if !directory.is_dir() {
        bail!("Choose a history folder, not a file.");
    }
    adage_store::config::set(paths, key, &directory.to_string_lossy()).map_err(anyhow::Error::msg)?;
    Ok(directory)
}

fn bounded_output_limit(mut stream: impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut kept = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        let n = stream.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        let take = n.min(limit.saturating_sub(kept.len()));
        kept.extend_from_slice(&buffer[..take]);
        truncated |= take < n;
    }
    Ok((kept, truncated))
}

fn capture_json(cmd: &mut Command, deadline: Instant) -> Result<serde_json::Value> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Starting OpenCode; install it or choose an existing exports folder")?;
    let out = child.stdout.take().context("Reading OpenCode export")?;
    let err = child.stderr.take().context("Reading OpenCode status")?;
    let (output_tx, output_rx) = std::sync::mpsc::sync_channel(1);
    let (error_tx, error_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = output_tx.send(bounded_output_limit(out, 8 << 20));
    });
    std::thread::spawn(move || {
        let _ = error_tx.send(bounded_output_limit(err, 64 << 10));
    });
    let command_deadline = deadline.min(Instant::now() + Duration::from_secs(30));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        }
        if Instant::now() >= command_deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("OpenCode export timed out. Choose an existing exports folder or try again.");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    // A child of OpenCode may hold its pipes open after it exits; never join a blocked reader.
    let remaining = || command_deadline.saturating_duration_since(Instant::now());
    let (bytes, truncated) = output_rx.recv_timeout(remaining()).context("OpenCode output did not close in time")??;
    let _ = error_rx.recv_timeout(remaining()); // Never expose transcript or provider details.
    if !status.success() {
        bail!("OpenCode could not export its local history ({status}). Choose an existing exports folder or update OpenCode.");
    }
    if truncated {
        bail!("OpenCode returned more than 8 MiB for one response. Use a smaller existing export file instead.");
    }
    serde_json::from_slice(&bytes)
        .context("OpenCode returned an unsupported export format; choose an existing exports folder")
}

fn same_repository(directory: &str, root: &Path) -> bool {
    adage_core::repo::find_root(Path::new(directory))
        .and_then(|p| p.canonicalize().ok())
        .is_some_and(|p| root.canonicalize().is_ok_and(|r| r == p))
}

fn write_private_export(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let temp = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).context("Creating a private local history export")?;
    let result = file.write_all(bytes).and_then(|_| file.sync_all()).and_then(|_| {
        drop(file);
        std::fs::rename(&temp, path)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.context("Saving the OpenCode history export")
}

pub fn import_opencode(paths: &Paths, root: &Path, bin: &Path, _person: &crate::Person) -> Result<Imported> {
    let deadline = Instant::now() + Duration::from_secs(120);
    let list = capture_json(
        Command::new(bin).args(["session", "list", "--format", "json", "--max-count", "500"]).current_dir(root),
        deadline,
    )?;
    let sessions = list.as_array().context(
        "OpenCode returned an unsupported session list; use Configure OpenCode history for existing exports.",
    )?;
    let selected: Vec<&str> = sessions
        .iter()
        .filter(|s| s["directory"].as_str().is_some_and(|d| same_repository(d, root)))
        .filter_map(|s| s["id"].as_str())
        .filter(|id| {
            id.len() <= 128 && id.starts_with("ses_") && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        .take(50)
        .collect();
    if selected.is_empty() {
        bail!("No sessions for this repository in OpenCode's 500 most recent sessions. Finish an OpenCode session here, or select an existing exports folder.");
    }
    let cfg = adage_store::config::load(paths).config;
    let destination = cfg.learn_opencode_exports.unwrap_or_else(|| paths.opencode_exports());
    std::fs::create_dir_all(&destination).context("Creating the OpenCode exports folder")?;
    let mut count = 0;
    for id in selected {
        if Instant::now() >= deadline {
            bail!("History import reached its two-minute limit. {count} valid exports are in {}; select this folder or try again.", destination.display());
        }
        let export = capture_json(Command::new(bin).args(["export", id]).current_dir(root), deadline)?;
        if !export["info"]["directory"].as_str().is_some_and(|d| same_repository(d, root))
            || !export["messages"].is_array()
        {
            bail!("An OpenCode export did not match this repository or its expected format. No source setting was changed; {count} valid export(s) are in {}.", destination.display());
        }
        write_private_export(&destination.join(format!("{id}.json")), &serde_json::to_vec(&export)?)?;
        count += 1;
    }
    adage_store::config::set(paths, "learn.opencode_exports", &destination.to_string_lossy())
        .map_err(anyhow::Error::msg)?;
    Ok(Imported { count, destination })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn opencode_import_reads_only_selected_repository_and_refuses_a_changed_export() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let other = temp.path().join("other");
        for path in [&root, &other] {
            std::fs::create_dir_all(path.join(".git")).unwrap();
        }
        let paths = Paths::at(temp.path().join("data"));
        let list = serde_json::json!([
            {"id":"ses_good", "directory":root},
            {"id":"ses_foreign", "directory":other},
            {"id":"ses_../escape", "directory":root}
        ]);
        let export_path = temp.path().join("export.json");
        let export = serde_json::json!({"info":{"id":"ses_good","directory":root},"messages":[]});
        std::fs::write(&export_path, serde_json::to_vec(&export).unwrap()).unwrap();
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
        let bin = temp.path().join("opencode");
        let script = format!("#!/bin/sh\ncase \"$1\" in\nsession) printf '%s\\n' {} ;;\nexport) [ \"$2\" = ses_good ] || exit 2; cat {} ;;\n*) exit 3 ;;\nesac\n", quote(&list.to_string()), quote(&export_path.to_string_lossy()));
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let person = crate::Person::for_tests();
        let imported = import_opencode(&paths, &root, &bin, &person).unwrap();
        assert_eq!(imported.count, 1);
        assert_eq!(std::fs::read_dir(&imported.destination).unwrap().count(), 1);
        let before = std::fs::read(imported.destination.join("ses_good.json")).unwrap();
        let config = std::fs::read(paths.config()).unwrap();
        std::fs::write(
            &export_path,
            serde_json::to_vec(&serde_json::json!({"info":{"directory":other},"messages":[]})).unwrap(),
        )
        .unwrap();
        assert!(import_opencode(&paths, &root, &bin, &person).is_err());
        assert_eq!(std::fs::read(paths.config()).unwrap(), config);
        assert_eq!(std::fs::read(imported.destination.join("ses_good.json")).unwrap(), before);
    }

    #[test]
    fn invalid_sources_preserve_config() {
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths::at(temp.path().join("data"));
        let person = crate::Person::for_tests();
        let folder = temp.path().join("history");
        std::fs::create_dir(&folder).unwrap();
        let chosen = set_source(&paths, "codex", &folder, &person).unwrap();
        assert_eq!(adage_store::config::load(&paths).config.learn_codex_sessions, Some(chosen));
        let before = std::fs::read(paths.config()).unwrap();
        assert!(set_source(&paths, "unknown", &folder, &person).is_err());
        assert!(set_source(&paths, "codex", &folder.join("missing"), &person).is_err());
        assert!(set_source(&paths, "codex", &paths.config(), &person).is_err());
        assert_eq!(std::fs::read(paths.config()).unwrap(), before);
    }

    #[test]
    fn capture_drains_but_never_keeps_unbounded_output() {
        let (bytes, truncated) = bounded_output_limit(std::io::Cursor::new(b"abcdef"), 3).unwrap();
        assert_eq!(bytes, b"abc");
        assert!(truncated);
    }

    #[test]
    fn exports_are_private_and_replaced_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ses_example.json");
        write_private_export(&path, b"first").unwrap();
        write_private_export(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
