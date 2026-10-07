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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tool {
    Status,
    Condition,
    Survey,
    Report,
    Graph,
    Savings,
    Packs,
    Marketplace,
    HistorySources,
    Delegate {
        reference: String,
        route: String,
        level: String,
        outputs: String,
        cache: String,
        effect: String,
    },
    Revoke {
        reference: String,
        route: String,
    },
    Pack {
        name: String,
        enabled: bool,
    },
    Eval(PathBuf),
    InstallModel(PathBuf),
    SetHistorySource {
        harness: String,
        directory: PathBuf,
    },
    ExportOpenCode,
    AddTrack {
        directory: PathBuf,
        trust: bool,
        expected_digest: Option<String>,
    },
    ShareTrack {
        track: String,
        output: PathBuf,
        key: Option<PathBuf>,
    },
    /// `adage update`: Adage, its hooks and the plugin, the way they were installed (onboarding
    /// plan O6). A person's decision, confirmed first, its lines shown as it runs.
    Update,
}

impl Tool {
    pub fn title(&self) -> &'static str {
        match self {
            Tool::Status => "Installed reflex collections and evidence",
            Tool::Condition => "Condition this repository",
            Tool::Survey => "Sense session history",
            Tool::Report => "How agents followed guidance",
            Tool::Graph => "Situations, reflexes, and responses",
            Tool::Savings => "Measured savings",
            Tool::Packs => "Installed packs and instincts",
            Tool::Marketplace => "Available packs",
            Tool::HistorySources => "History sources",
            Tool::Delegate { .. } => "Delegate reflex",
            Tool::Revoke { .. } => "Revoke delegation",
            Tool::Pack { enabled: true, .. } => "Enable pack",
            Tool::Pack { enabled: false, .. } => "Disable pack",
            Tool::Eval(_) => "Evaluate a dataset",
            Tool::InstallModel(_) => "Install a model from a folder",
            Tool::SetHistorySource { .. } => "Configure history",
            Tool::ExportOpenCode => "Import OpenCode history",
            Tool::AddTrack { .. } => "Import a signed pack",
            Tool::ShareTrack { .. } => "Export a signed pack",
            Tool::Update => "Update Adage",
        }
    }

    pub fn mutates(&self) -> bool {
        matches!(
            self,
            Tool::Condition
                | Tool::InstallModel(_)
                | Tool::Delegate { .. }
                | Tool::Revoke { .. }
                | Tool::Pack { .. }
                | Tool::AddTrack { .. }
                | Tool::ShareTrack { .. }
                | Tool::SetHistorySource { .. }
                | Tool::ExportOpenCode
                | Tool::Update
        )
    }

    fn person_command(&self) -> Option<&'static str> {
        match self {
            Tool::AddTrack { .. } => Some("add"),
            Tool::Condition => Some("condition"),
            Tool::ShareTrack { .. } => Some("share"),
            Tool::SetHistorySource { .. } => Some("config set"),
            Tool::ExportOpenCode => Some("import history"),
            Tool::Delegate { .. } | Tool::Revoke { .. } => Some("review delegate"),
            Tool::Pack { .. } => Some("packs"),
            Tool::InstallModel(_) => Some("models install"),
            Tool::Update => Some("update"),
            _ => None,
        }
    }

    fn args(&self, paths: &Paths, base: &Path) -> Result<Vec<OsString>> {
        let words = |args: &[&str]| args.iter().map(OsString::from).collect::<Vec<_>>();
        Ok(match self {
            Tool::Status => words(&["status"]),
            Tool::Update => words(&["update"]),
            Tool::Condition => vec!["condition".into(), "--repo-root".into(), base.into()],
            Tool::Report => words(&["report"]),
            Tool::Graph => words(&["graph"]),
            Tool::Savings => words(&["savings"]),
            Tool::Packs => words(&["packs"]),
            Tool::Marketplace => words(&["marketplace"]),
            Tool::HistorySources => bail!("This action runs inside the app."),
            Tool::Delegate { reference, route, level, outputs, cache, effect } => {
                if !matches!(level.as_str(), "e1" | "e2") {
                    bail!("Execution level must be e1 or e2.");
                }
                let mut args = words(&["review", "delegate", reference, route, "--level", level]);
                if level == "e2" {
                    if !matches!(effect.as_str(), "verify" | "regenerate") {
                        bail!("E2 effect must be verify or regenerate.");
                    }
                    args.extend(words(&["--effect", effect]));
                    for (flag, globs) in [("--outputs", outputs), ("--cache", cache)] {
                        for glob in globs.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                            args.extend(words(&[flag, glob]));
                        }
                    }
                }
                args
            }
            Tool::Revoke { reference, route } => words(&["review", "revoke", reference, route]),
            Tool::Pack { name, enabled } => {
                if name.is_empty() || name.starts_with('-') {
                    bail!("Enter an installed pack name.");
                }
                words(&["packs", if *enabled { "on" } else { "off" }, name])
            }
            Tool::Survey => words(&["sense"]),
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
pub fn resolve_path(path: &Path, base: &Path) -> Result<PathBuf> {
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

pub fn directory(path: &Path, base: &Path) -> Result<PathBuf> {
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
        if matches!(tool, Tool::HistorySources) {
            return Ok(crate::history::summary(&paths, Some(&base)));
        }
        if let Tool::SetHistorySource { harness, directory: folder } = &tool {
            return crate::history::set_source(&paths, harness, &directory(folder, &base)?);
        }
        if matches!(tool, Tool::ExportOpenCode) {
            return crate::history::import_opencode(&paths, &base);
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
    fn paths_and_dataset_names_stay_literal_arguments() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("generate")).unwrap();
        assert_eq!(
            Tool::Eval("generate".into()).args(&Paths::at(d.path()), d.path()).unwrap(),
            vec![OsString::from("eval"), d.path().join("generate").into_os_string()]
        );
        assert_eq!(
            resolve_path(Path::new("$(data); with spaces"), d.path()).unwrap(),
            d.path().join("$(data); with spaces")
        );
        assert!(resolve_path(Path::new(""), d.path()).is_err());
        let args = Tool::Condition.args(&Paths::at(d.path()), d.path()).unwrap();
        assert_eq!(args, vec![OsString::from("condition"), OsString::from("--repo-root"), d.path().into()]);
        assert!(
            !args.iter().any(|a| a == "--auto"),
            "an explicit run must not silently change the background allowlist"
        );
    }
    #[test]
    fn report_output_is_bounded_drained_and_cannot_control_the_terminal() {
        let bytes = vec![b'x'; MAX_OUTPUT + 100];
        let mut reader = std::io::Cursor::new(bytes);
        let (out, truncated) = bounded_output(&mut reader).unwrap();
        assert!(truncated);
        assert_eq!(out.len(), MAX_OUTPUT);
        assert_eq!(reader.position(), (MAX_OUTPUT + 100) as u64);
        assert_eq!(clean_lines(b"\x1b[31mred\x1b[0m \x1b]8;;file:///tmp/a\x1b\\text\x1b]8;;\x1b\\"), vec!["red text"]);
    }
    #[test]
    fn imports_carry_the_reviewed_token_and_mutations_keep_person_guards() {
        let d = tempfile::tempdir().unwrap();
        let t =
            Tool::AddTrack { directory: d.path().into(), trust: true, expected_digest: Some("review-token".into()) };
        let args = t.args(&Paths::at(d.path()), d.path()).unwrap();
        assert_eq!(&args[args.len() - 2..], [OsString::from("--expected-digest"), OsString::from("review-token")]);
        for t in [
            t,
            Tool::SetHistorySource { harness: "codex".into(), directory: d.path().into() },
            Tool::ExportOpenCode,
            Tool::Condition,
            Tool::Revoke { reference: "a/b".into(), route: "test".into() },
            Tool::Update,
        ] {
            assert!(t.mutates());
            assert!(t.person_command().is_some());
        }
        // `Update Adage` runs bare `adage update`: the child's own terminal check, lifted only by
        // the override this parent sets after its own (never a press).
        assert_eq!(Tool::Update.args(&Paths::at(d.path()), d.path()).unwrap(), vec![OsString::from("update")]);
    }
}
