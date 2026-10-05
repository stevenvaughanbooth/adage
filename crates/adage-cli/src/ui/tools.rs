//! Palette tools reuse the command implementations without writing into the alternate screen.
//! Arguments are passed directly to the current executable, never through a shell. Paths are
//! resolved before dispatch so a folder named `generate` or `--trust` remains a folder.

use super::jobs::{Job, JobMsg};
use adage_store::Paths;
use anyhow::{bail, Context, Result};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

const MAX_OUTPUT: usize = 512 << 10;
const MAX_LINES: usize = 12_000;
const REPORT_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub enum Tool {
    Status,
    Survey,
    Report,
    Graph,
    Eval(PathBuf),
    InstallModel(PathBuf),
    SetHistorySource { harness: String, directory: PathBuf },
    ExportOpenCode,
    AddTrack { directory: PathBuf, trust: bool, expected_digest: Option<String> },
    ShareTrack { track: String, output: PathBuf, key: Option<PathBuf> },
}

impl Tool {
    pub fn title(&self) -> &'static str {
        match self {
            Tool::Status => "Installed reflex collections and evidence",
            Tool::Survey => "Survey session history",
            Tool::Report => "How agents followed guidance",
            Tool::Graph => "Situations, reflexes, and responses",
            Tool::Eval(_) => "Evaluate a dataset",
            Tool::InstallModel(_) => "Install a model from a folder",
            Tool::SetHistorySource { .. } => "Configure history",
            Tool::ExportOpenCode => "Import OpenCode history",
            Tool::AddTrack { .. } => "Import a signed pack",
            Tool::ShareTrack { .. } => "Export a signed pack",
        }
    }

    pub fn mutates(&self) -> bool {
        matches!(
            self,
            Tool::InstallModel(_)
                | Tool::AddTrack { .. }
                | Tool::ShareTrack { .. }
                | Tool::SetHistorySource { .. }
                | Tool::ExportOpenCode
        )
    }

    fn person_command(&self) -> Option<&'static str> {
        match self {
            Tool::AddTrack { .. } => Some("add"),
            Tool::ShareTrack { .. } => Some("share"),
            Tool::SetHistorySource { .. } => Some("config set"),
            Tool::ExportOpenCode => Some("import history"),
            _ => None,
        }
    }

    fn args(&self, paths: &Paths, base: &Path) -> Result<Vec<OsString>> {
        let words = |args: &[&str]| args.iter().map(OsString::from).collect::<Vec<_>>();
        Ok(match self {
            Tool::Status => words(&["status"]),
            Tool::Report => words(&["report"]),
            Tool::Graph => words(&["graph"]),
            Tool::Survey => {
                let mut args = words(&["survey"]);
                if let Some(root) = adage_core::repo::find_root(base) {
                    args.push("--repo".into());
                    args.push(adage_core::repo::repo_key_cached(&root, &paths.repo_keys()).into());
                }
                args
            }
            Tool::Eval(dir) => vec!["eval".into(), directory(dir, base)?.into_os_string()],
            Tool::InstallModel(dir) => {
                vec!["models".into(), "install".into(), directory(dir, base)?.into_os_string()]
            }
            Tool::SetHistorySource { .. } | Tool::ExportOpenCode => bail!("This action runs inside the app."),
            Tool::AddTrack { directory: dir, trust, expected_digest } => {
                let mut args = vec!["add".into(), directory(dir, base)?.into_os_string()];
                if *trust {
                    args.push("--trust".into());
                }
                if let Some(digest) = expected_digest {
                    args.push("--expected-digest".into());
                    args.push(digest.into());
                }
                args
            }
            Tool::ShareTrack { track, output, key } => {
                if track.trim().is_empty() || track.starts_with("--") {
                    bail!("Choose an installed collection name from Reflexes.");
                }
                // Routes selects a directory reference, which distinguishes an imported track
                // from a learned one of the same name. Preserve that selection for `share`.
                let installed = paths.tracks().join(track);
                let source = if installed.join("track.toml").is_file() {
                    installed.into_os_string()
                } else {
                    OsString::from(track)
                };
                let mut args =
                    vec!["share".into(), source, "--out".into(), resolve_path(output, base)?.into_os_string()];
                if let Some(key) = key {
                    args.push("--key".into());
                    args.push(resolve_path(key, base)?.into_os_string());
                }
                args
            }
        })
    }
}

/// Shell-free `~` support for guided path fields. Nothing else (`$VAR`, globs, substitutions)
/// is expanded: those characters can legally belong to a file's name.
fn resolve_path(path: &Path, base: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("Enter a folder path.");
    }
    let expanded = match path.strip_prefix("~") {
        Ok(rest) => {
            adage_store::home_dir().context("Your home folder could not be found; use a full path.")?.join(rest)
        }
        Err(_) => path.to_path_buf(),
    };
    Ok(if expanded.is_absolute() { expanded } else { base.join(expanded) })
}

fn directory(path: &Path, base: &Path) -> Result<PathBuf> {
    let path = resolve_path(path, base)?;
    if !path.is_dir() {
        bail!("{} is not an existing folder. Check the path and try again.", path.display());
    }
    Ok(path)
}

/// Resolve the exact folder shown in the source confirmation before saving it.
pub fn history_source_directory(path: &Path, root: Option<&Path>) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    directory(path, root.unwrap_or(&cwd))
}

/// Show the actual signed identity before the import confirmation can grant trust. Signature
/// verification is read-only and uses the same canonical digest and track validation as `add`.
pub fn import_summary(directory: &Path, root: Option<&Path>) -> Result<(Vec<String>, String)> {
    let cwd = std::env::current_dir().context("Finding the current folder")?;
    let dir = self::directory(directory, root.unwrap_or(&cwd))?;
    let listing = crate::share::list_files(&dir)?;
    if let Some((name, why)) = listing.other.iter().find(|(name, why)| name != crate::share::SIG && *why != "hidden") {
        bail!("{name} does not belong in a shared collection ({why}).");
    }
    let signature = dir.join(crate::share::SIG);
    if std::fs::metadata(&signature).context("This folder has no track.sig signature")?.len() > 64 << 10 {
        bail!("The collection signature is unexpectedly large.");
    }
    let pem = std::fs::read_to_string(&signature).context("Reading the collection signature")?;
    let digest = adage_tracks::io::digest(&dir).context("Reading the signed collection")?;
    let signer = adage_sign::verify_embedded(&adage_sign::message(&digest)?, adage_sign::NAMESPACE, &pem)
        .context("The signature does not match this collection; it may have changed after signing")?;
    let track = adage_tracks::io::read(&dir).context("Validating the shared collection")?;
    let reviewed = crate::add::confirmation_digest(&digest, &pem);
    Ok((
        vec![
            format!("Collection: {} @ {}", track.manifest.track.name, track.manifest.track.version),
            format!("Repository: {}", track.manifest.scope.repo),
            format!("Reflexes: {}", track.routes.len()),
            format!("Signer: {}", adage_sign::fingerprint(&signer)),
            "Signature verified. New reflexes start on probation.".into(),
            "If trusting a new signer, check this fingerprint with the collection's author before continuing.".into(),
        ],
        reviewed,
    ))
}

/// Launch a palette action. The TUI confirms mutating operations before calling this function;
/// sharing and trust changes still enforce the existing person-at-a-terminal guard here and in
/// the child. No agent-session marker is removed.
pub fn run(paths: Paths, root: Option<PathBuf>, tool: Tool) -> Job {
    let cancelable = !tool.mutates();
    let mut job = Job::spawn(tool.title(), move |tx| {
        if let Some(command) = tool.person_command() {
            crate::share::require_person(command)?;
        }
        let cwd = std::env::current_dir().context("Finding the current folder")?;
        let base = root.map(|p| if p.is_absolute() { p } else { cwd.join(p) }).unwrap_or_else(|| cwd.clone());
        let paths = Paths::at(if paths.root.is_absolute() { paths.root } else { cwd.join(paths.root) });
        if let Tool::SetHistorySource { harness, directory: folder } = &tool {
            let key = match harness.as_str() {
                "claude-code" => "learn.claude_projects",
                "codex" => "learn.codex_sessions",
                "opencode" => "learn.opencode_exports",
                _ => bail!("Choose Claude Code, Codex, or OpenCode."),
            };
            let folder = directory(folder, &base)?;
            adage_store::config::set(&paths, key, &folder.to_string_lossy()).map_err(anyhow::Error::msg)?;
            let mut lines =
                vec![format!("Saved {key}. Survey, Learn now, and automatic learning will use this folder.")];
            lines.extend(crate::detect::history_summary(&adage_store::config::load(&paths).config));
            return Ok(lines);
        }
        if matches!(tool, Tool::ExportOpenCode) {
            return export_opencode(&paths, &base, tx);
        }
        let args = tool.args(&paths, &base)?;
        let mut cmd = Command::new(std::env::current_exe().context("Finding the Adage executable")?);
        cmd.args(args).current_dir(base).env("ADAGE_HOME", &paths.root);
        if tool.person_command().is_some() {
            // Parent stdin has already passed require_person. The child has captured pipes, so
            // lift only its terminal check; it independently retains every agent-session check.
            cmd.env("ADAGE_ALLOW_NONINTERACTIVE", "1");
        }
        let _ = tx.send(JobMsg::Progress(0.05, format!("{}…", tool.title())));
        let mut lines = capture(&mut cmd, tx, !tool.mutates())?;
        if lines.is_empty() {
            lines.push("Completed successfully.".into());
        }
        Ok(lines)
    });
    job.cancelable = cancelable;
    job
}

/// The export tool only reads OpenCode's local session database, using its documented CLI. It
/// never runs an agent or a prompt. JSON is bounded before parsing and never printed as a report.
fn capture_json(cmd: &mut Command, tx: &Sender<JobMsg>) -> Result<serde_json::Value> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Starting OpenCode; install it or choose an existing exports folder")?;
    let out = child.stdout.take().context("Reading OpenCode export")?;
    let err = child.stderr.take().context("Reading OpenCode status")?;
    let output = std::thread::spawn(move || bounded_output_limit(out, 8 << 20));
    let errors = std::thread::spawn(move || bounded_output_limit(err, 64 << 10));
    let started = Instant::now();
    let mut last = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if last.elapsed() >= Duration::from_secs(1) {
            if tx.send(JobMsg::Progress(0.2, "Reading OpenCode's local history…".into())).is_err()
                || started.elapsed() > Duration::from_secs(30)
            {
                let _ = child.kill();
                let _ = child.wait();
                let _ = output.join();
                let _ = errors.join();
                bail!("OpenCode export did not finish in 30 seconds. Choose an existing exports folder or try again.");
            }
            last = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let (bytes, truncated) = output.join().map_err(|_| anyhow::anyhow!("Reading OpenCode output failed"))??;
    let _ = errors.join(); // Drain errors without exposing transcript or provider details.
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

fn export_opencode(paths: &Paths, base: &Path, tx: &Sender<JobMsg>) -> Result<Vec<String>> {
    let root = adage_core::repo::find_root(base).context("Open a Git repository before importing OpenCode history.")?;
    let bin = crate::detect::on_path("opencode").context("OpenCode was not found on PATH. Install OpenCode, or use Configure OpenCode history to choose existing OpenCode export JSON files.")?;
    export_opencode_with(paths, &root, &bin, tx)
}

fn export_opencode_with(paths: &Paths, root: &Path, bin: &Path, tx: &Sender<JobMsg>) -> Result<Vec<String>> {
    let list = capture_json(
        Command::new(bin).args(["session", "list", "--format", "json", "--max-count", "500"]).current_dir(root),
        tx,
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
    let destination = cfg.learn_opencode_exports.unwrap_or_else(|| paths.root.join("history/opencode"));
    std::fs::create_dir_all(&destination).context("Creating the OpenCode exports folder")?;
    let mut count = 0;
    for id in selected {
        let export = capture_json(Command::new(bin).args(["export", id]).current_dir(root), tx)?;
        if !export["info"]["directory"].as_str().is_some_and(|d| same_repository(d, root))
            || !export["messages"].is_array()
        {
            bail!("An OpenCode export did not match this repository or its expected format. No source setting was changed; {count} valid export(s) are in {}.", crate::detect::tilde(&destination));
        }
        write_private_export(&destination.join(format!("{id}.json")), &serde_json::to_vec(&export)?)?;
        count += 1;
        let _ = tx.send(JobMsg::Progress(0.5, format!("Imported {count} OpenCode session(s)…")));
    }
    adage_store::config::set(paths, "learn.opencode_exports", &destination.to_string_lossy())
        .map_err(anyhow::Error::msg)?;
    Ok(vec![format!("Imported {count} OpenCode session(s) for this repository into {}.", crate::detect::tilde(&destination)),
        "Survey and Learn now use these exports. Up to 50 matching sessions were imported from the 500 most recent sessions.".into(),
        "Run Import OpenCode history again to refresh them after later sessions; its hooks do not export history automatically.".into()])
}

/// Drain both pipes concurrently and keep a bounded prefix of each. A large report never fills
/// the pipe and deadlocks the child, nor can it exhaust the TUI's memory.
fn capture(cmd: &mut Command, tx: &Sender<JobMsg>, readonly: bool) -> Result<Vec<String>> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Starting the Adage tool")?;
    let stdout = child.stdout.take().context("Capturing tool output")?;
    let stderr = child.stderr.take().context("Capturing tool errors")?;
    let output = std::thread::spawn(move || bounded_output(stdout));
    let errors = std::thread::spawn(move || bounded_output(stderr));
    let started = Instant::now();
    let mut last_progress = Instant::now();
    let mut stopped = None;
    let status = loop {
        if let Some(status) = child.try_wait().context("Waiting for the tool")? {
            break status;
        }
        if last_progress.elapsed() >= Duration::from_secs(1) {
            let disconnected =
                tx.send(JobMsg::Progress(0.1, format!("Working… {}s elapsed", started.elapsed().as_secs()))).is_err();
            last_progress = Instant::now();
            if readonly && (disconnected || started.elapsed() >= REPORT_TIMEOUT) {
                stopped = Some(if disconnected {
                    "The report was closed."
                } else {
                    "This report took longer than five minutes. Try a smaller dataset."
                });
                let _ = child.kill();
                break child.wait().context("Stopping the report")?;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let (out, out_truncated) = output.join().map_err(|_| anyhow::anyhow!("Reading tool output failed"))??;
    let (err, err_truncated) = errors.join().map_err(|_| anyhow::anyhow!("Reading tool errors failed"))??;
    if let Some(why) = stopped {
        bail!(why);
    }
    let mut lines = clean_lines(&out);
    if out_truncated {
        lines.push("… Output was shortened to keep the viewer responsive.".into());
    }
    let err = clean_lines(&err);
    if !status.success() {
        lines.extend(err);
        if err_truncated {
            lines.push("… Additional error output was shortened.".into());
        }
        if lines.is_empty() {
            bail!("The tool exited with {status}. Please try again.");
        }
        bail!("{}", lines.join("\n"));
    }
    if !err.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Notes".into());
        lines.extend(err);
        if err_truncated {
            lines.push("… Additional notes were shortened.".into());
        }
    }
    if lines.len() > MAX_LINES {
        lines.truncate(MAX_LINES);
        lines.push("… Additional lines were shortened to keep the viewer responsive.".into());
    }
    Ok(lines)
}

fn bounded_output(mut reader: impl Read) -> std::io::Result<(Vec<u8>, bool)> {
    bounded_output_limit(&mut reader, MAX_OUTPUT)
}

fn bounded_output_limit(mut reader: impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut out = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let n = match reader.read(&mut buffer) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if n == 0 {
            return Ok((out, truncated));
        }
        let keep = n.min(limit.saturating_sub(out.len()));
        out.extend_from_slice(&buffer[..keep]);
        truncated |= keep < n;
    }
}

/// Transcript text may contain terminal escapes. Never allow a report to control the screen.
fn clean_lines(bytes: &[u8]) -> Vec<String> {
    // CSI styles/cursor sequences and OSC hyperlinks/titles are stripped as complete
    // sequences, including their visible payload. Reports never acquire clickable shell links.
    let mut state = 0;
    let mut text = String::new();
    for c in String::from_utf8_lossy(bytes).chars() {
        match (state, c) {
            (0, '\x1b') => state = 1,
            (0, c) if !c.is_control() || matches!(c, '\n' | '\t') => text.push(c),
            (0, _) => {}
            (1, '[') => state = 2,
            (1, ']') => state = 3,
            (1, _) => state = 0,
            (2, '\x40'..='\x7e') => state = 0,
            (2, _) => {}
            (3, '\x07') => state = 0,
            (3, '\x1b') => state = 4,
            (3, _) => {}
            (4, '\\') => state = 0,
            (4, _) => state = 3,
            _ => unreachable!(),
        }
    }
    text.lines().map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_actions_keep_the_agent_session_guard() {
        const PROBE: &str = "ADAGE_TEST_HISTORY_GUARD_ROOT";
        if let Some(root) = std::env::var_os(PROBE) {
            let root = PathBuf::from(root);
            let paths = Paths::at(root.join("adage"));
            for tool in
                [Tool::SetHistorySource { harness: "codex".into(), directory: root.clone() }, Tool::ExportOpenCode]
            {
                let mut job = run(paths.clone(), Some(root.clone()), tool);
                let started = Instant::now();
                let result = loop {
                    if let Some(done) = job.poll() {
                        break done;
                    }
                    assert!(started.elapsed() < Duration::from_secs(3), "guard should refuse before any subprocess");
                    std::thread::sleep(Duration::from_millis(10));
                };
                assert!(result.unwrap_err().contains("will not run inside an agent session"));
            }
            assert!(!paths.root.exists(), "guard must refuse before writing config or exports");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        // Process-local environment avoids races with every other test and proves that the CI
        // terminal override cannot lift the agent-session guard for either new mutation.
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "ui::tools::tests::history_actions_keep_the_agent_session_guard", "--nocapture"])
            .env(PROBE, temp.path())
            .env("CODEX_SANDBOX", "guard-test")
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!temp.path().join("adage").exists());
    }

    #[test]
    #[cfg(unix)]
    fn opencode_import_is_repository_scoped_and_configures_the_export_source() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let other = temp.path().join("other");
        std::fs::create_dir_all(other.join(".git")).unwrap();
        let listing = temp.path().join("sessions.json");
        std::fs::write(
            &listing,
            serde_json::json!([
                {"id":"ses_fixture","directory":root},
                {"id":"ses_elsewhere","directory":other},
                {"id":"--help","directory":root}
            ])
            .to_string(),
        )
        .unwrap();
        let export = temp.path().join("export.json");
        std::fs::write(
            &export,
            serde_json::json!({"info":{"id":"ses_fixture","directory":root},"messages":[]}).to_string(),
        )
        .unwrap();
        let bin = temp.path().join("opencode");
        let quote = |p: &Path| crate::harness::codex::sh_quote(&p.to_string_lossy());
        std::fs::write(&bin, format!("#!/bin/sh\ncase \"$1\" in\nsession) cat {};;\nexport) [ \"$2\" = ses_fixture ] || exit 9; cat {};;\n*) exit 8;;\nesac\n", quote(&listing), quote(&export))).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let paths = Paths::at(temp.path().join("data"));
        let (tx, _rx) = std::sync::mpsc::channel();
        let lines = export_opencode_with(&paths, &root, &bin, &tx).unwrap();
        assert!(lines[0].contains("Imported 1 OpenCode session"));
        let dest = adage_store::config::load(&paths).config.learn_opencode_exports.unwrap();
        assert!(dest.join("ses_fixture.json").is_file());
        assert_eq!(std::fs::metadata(dest.join("ses_fixture.json")).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(!dest.join("ses_elsewhere.json").exists());

        std::fs::write(&export, serde_json::json!({"info":{"directory":other},"messages":[]}).to_string()).unwrap();
        assert!(export_opencode_with(&paths, &root, &bin, &tx)
            .unwrap_err()
            .to_string()
            .contains("did not match this repository"));
        let kept: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dest.join("ses_fixture.json")).unwrap()).unwrap();
        assert_eq!(kept["info"]["directory"], serde_json::json!(root));
    }

    #[test]
    fn dataset_named_generate_is_a_path_not_a_subcommand() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("generate")).unwrap();
        let args = Tool::Eval("generate".into()).args(&Paths::at(temp.path()), temp.path()).unwrap();
        assert_eq!(args, vec![OsString::from("eval"), temp.path().join("generate").into_os_string()]);
    }

    #[test]
    fn imported_folder_cannot_become_a_trust_flag() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("--trust")).unwrap();
        let args = Tool::AddTrack { directory: "--trust".into(), trust: false, expected_digest: None }
            .args(&Paths::at(temp.path()), temp.path())
            .unwrap();
        assert_eq!(args, vec![OsString::from("add"), temp.path().join("--trust").into_os_string()]);
    }

    #[test]
    fn paths_preserve_spaces_and_shell_characters() {
        let base = Path::new("/work");
        assert_eq!(resolve_path(Path::new("my $(track);*.json"), base).unwrap(), base.join("my $(track);*.json"));
        assert!(resolve_path(Path::new(""), base).is_err());
        assert_eq!(resolve_path(Path::new("~someone/track"), base).unwrap(), base.join("~someone/track"));
        if let Some(home) = adage_store::home_dir() {
            assert_eq!(resolve_path(Path::new("~/track folder"), base).unwrap(), home.join("track folder"));
        }
    }

    #[test]
    fn trust_is_only_enabled_explicitly_and_mutations_are_identified() {
        let temp = tempfile::tempdir().unwrap();
        let tool = Tool::AddTrack { directory: temp.path().into(), trust: true, expected_digest: None };
        assert!(tool.mutates());
        assert_eq!(tool.person_command(), Some("add"));
        assert_eq!(tool.args(&Paths::at(temp.path()), temp.path()).unwrap().last().unwrap(), "--trust");
        assert!(!Tool::Eval(temp.path().into()).mutates());
        assert!(Tool::InstallModel(temp.path().into()).mutates());
        assert!(Tool::ShareTrack { track: "test".into(), output: "out".into(), key: None }.mutates());
    }

    #[test]
    fn output_is_bounded_but_the_pipe_is_drained() {
        let bytes = vec![b'x'; MAX_OUTPUT + 100];
        let mut input = std::io::Cursor::new(bytes);
        let (out, truncated) = bounded_output(&mut input).unwrap();
        assert_eq!(out.len(), MAX_OUTPUT);
        assert!(truncated);
        assert_eq!(input.position(), (MAX_OUTPUT + 100) as u64);
        assert_eq!(clean_lines(b"hello\x1b\x00\x08\nworld\r\n"), vec!["hello", "world"]);
        assert_eq!(clean_lines(b"\x1b[31mred\x1b[0m \x1b]8;;file:///tmp/a\x1b\\text\x1b]8;;\x1b\\"), vec!["red text"]);
    }

    #[test]
    fn child_output_and_errors_return_to_the_viewer() {
        let (tx, _rx) = std::sync::mpsc::channel();
        // The test executable's own list/help modes are portable child processes which do not
        // touch the user's Adage state, hook configuration, or agent history.
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.arg("--list");
        let lines = capture(&mut command, &tx, true).unwrap();
        assert!(lines.iter().any(|line| line.contains("ui::tools::tests::child_output_and_errors")));
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.arg("--not-a-test-runner-option");
        let error = capture(&mut command, &tx, true).unwrap_err().to_string();
        assert!(error.contains("not-a-test-runner-option"));
    }

    #[test]
    #[cfg(unix)]
    fn closing_a_read_only_report_terminates_its_child() {
        let (tx, rx) = std::sync::mpsc::channel();
        drop(rx);
        let started = Instant::now();
        let mut command = Command::new("sleep");
        command.arg("30");
        let error = capture(&mut command, &tx, true).unwrap_err().to_string();
        assert!(error.contains("report was closed"));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn export_keeps_the_selected_import_when_names_overlap() {
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths::at(temp.path());
        let reference = format!("{}/test/checks", adage_tracks::index::IMPORTED_DIR);
        let selected = paths.tracks().join(&reference);
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::write(selected.join("track.toml"), "manifest").unwrap();
        let tool = Tool::ShareTrack { track: reference, output: "out".into(), key: None };
        let args = tool.args(&paths, temp.path()).unwrap();
        assert_eq!(args[1], selected.into_os_string());
    }

    #[test]
    fn import_preview_verifies_the_signer_before_showing_a_confirmation() {
        use adage_tracks::{Gauge, Manifest, Scope, Track, TrackInfo};
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("shared");
        let track = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "test/checks".into(),
                    version: "1.0.0".into(),
                    description: "Checks".into(),
                    provenance: "authored".into(),
                },
                scope: Scope { repo: "test-repo".into() },
                gauge: Gauge::default(),
            },
            routes: vec![],
        };
        adage_tracks::io::write(&directory, &track).unwrap();
        let key = ssh_key::PrivateKey::random(&mut rand_core::OsRng, ssh_key::Algorithm::Ed25519).unwrap();
        let key_path = temp.path().join("key");
        std::fs::write(&key_path, key.to_openssh(ssh_key::LineEnding::LF).unwrap().as_bytes()).unwrap();
        let digest = adage_tracks::io::digest(&directory).unwrap();
        let signed =
            adage_sign::sign_with_key_file(&adage_sign::message(&digest).unwrap(), adage_sign::NAMESPACE, &key_path)
                .unwrap();
        std::fs::write(directory.join(crate::share::SIG), &signed.pem).unwrap();
        let (lines, token) = import_summary(&directory, None).unwrap();
        assert!(lines.iter().any(|line| line.contains("test-repo")));
        assert!(lines.iter().any(|line| line.contains(&adage_sign::fingerprint(&signed.key))));
        assert_eq!(token, crate::add::confirmation_digest(&digest, &signed.pem));
        std::fs::write(directory.join("evidence.json"), "changed").unwrap();
        assert!(import_summary(&directory, None).unwrap_err().to_string().contains("signature does not match"));
    }
}
