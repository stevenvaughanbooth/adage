//! `adage world follow`: the local sources and the timeline as JSON lines, for the session's life.
//! The Claude Code mod spawns one per session, exactly as it spawns `adage watch --follow`
//! (`plugin/claude-code/hooks/follower.ts`); the child ends with its parent (`lifeline`).
//!
//! The first child to take `world/lock` polls the local sources: transcripts landing in the
//! directories `world.transcripts` names, files recently edited under `world.roots`, and, only
//! while `world.desktop` is on, the active window's title. A second child reads only. Every child
//! tails the activity timeline and prints each event as `{"cursor": …, "event": {…}}`, and the
//! sources' health as `{"cursor": …, "health": {…}}` when it changes.
//!
//! No network. Content is read for one thing only: a transcript's first 300 characters, as its
//! summary, from a directory the person named.

use super::ingest::{self, Batch};
use crate::ui::follow::lifeline::Lifeline;
use adage_store::{config, lock, spool, state, Paths};
use adage_trace::{Action, ActivityEvent, Artifact, ArtifactKind};
use adage_world::state::{ArtifactsState, HealthState};
use adage_world::Descriptor;
use anyhow::{bail, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const USAGE: &str = "usage: adage world follow [--repo <dir>] [--session <id>] [--stdin ignore|eof] [--since <cursor>]";
const POLL: Duration = Duration::from_secs(5);
/// A debug-build knob for tests: the poll period in milliseconds.
const TEST_POLL_ENV: &str = "ADAGE_WORLD_POLL_MS";
/// How deep `world.roots` are walked, and how many files one walk keeps.
const MAX_DEPTH: usize = 4;
const MAX_FILES: usize = 200;
const SKIP_DIRS: &[&str] =
    &[".git", "node_modules", "target", "dist", "build", ".venv", "venv", "__pycache__", ".cache"];

pub fn follow(args: &[String], paths: &Paths) -> Result<()> {
    for a in args.iter().filter(|a| a.starts_with("--")) {
        if !matches!(a.as_str(), "--repo" | "--session" | "--stdin" | "--since") {
            bail!("unknown flag {a}\n{USAGE}");
        }
    }
    let watch_stdin = match crate::helpers::flag(args, "--stdin").as_deref() {
        None | Some("eof") => true,
        Some("ignore") => false,
        Some(other) => bail!("--stdin takes eof or ignore, not {other:?}\n{USAGE}"),
    };
    let since: u64 = match crate::helpers::flag(args, "--since") {
        Some(s) => {
            s.parse().map_err(|_| anyhow::anyhow!("--since takes a cursor from a line of `adage world follow`"))?
        }
        None => 0,
    };
    let session = crate::helpers::flag(args, "--session").unwrap_or_default();
    run(paths, &session, since, watch_stdin)
}

fn poll_every() -> Duration {
    let knob = cfg!(debug_assertions).then(|| std::env::var(TEST_POLL_ENV).ok()?.parse::<u64>().ok()).flatten();
    knob.map_or(POLL, Duration::from_millis)
}

fn run(paths: &Paths, session: &str, since: u64, watch_stdin: bool) -> Result<()> {
    let life = Lifeline::start(watch_stdin);
    std::fs::create_dir_all(paths.world())?;
    // The poller: the first child to take the lock; a second reads only, and takes it over when
    // the first ends.
    let mut held = lock::try_exclusive(&paths.world_lock()).ok().flatten();
    let mut sources = Local::new(paths);
    let mut out = std::io::stdout().lock();
    let mut cursor = since;
    let mut last_health: Option<serde_json::Value> = None;
    let poll = poll_every();
    let mut last_poll = Instant::now() - poll;
    loop {
        if life.ended() {
            return Ok(());
        }
        if held.is_none() {
            held = lock::try_exclusive(&paths.world_lock()).ok().flatten();
        }
        let live = config::load(paths).config.world_enabled && !paths.world_paused().exists();
        if held.is_some() && live && last_poll.elapsed() >= poll {
            last_poll = Instant::now();
            sources.poll(paths);
        }
        let mut lines: Vec<serde_json::Value> = Vec::new();
        if let Ok((events, next)) = spool::read_from::<ActivityEvent>(&paths.world_activity(), cursor) {
            // Each event with its thing's title, for the pane's timeline (the event carries the id).
            let titles = if events.is_empty() {
                None
            } else {
                state::try_read::<ArtifactsState>(&paths.world_artifacts()).ok().flatten()
            };
            for e in events {
                let title = titles.as_ref().and_then(|t| t.artifacts.get(&e.artifact)).map(|a| a.title.clone());
                lines.push(json!({ "cursor": next, "event": e, "title": title, "session": session }));
            }
            cursor = next;
        }
        let health = state::try_read::<HealthState>(&paths.world_health()).ok().flatten().unwrap_or_default();
        let h = serde_json::to_value(&health).unwrap_or_default();
        if last_health.as_ref() != Some(&h) {
            last_health = Some(h.clone());
            lines.push(json!({ "cursor": cursor, "health": h, "polling": held.is_some(), "live": live }));
        }
        for l in lines {
            if writeln!(out, "{l}").and_then(|()| out.flush()).is_err() {
                return Ok(());
            }
        }
        std::thread::sleep(poll.min(Duration::from_millis(500)));
    }
}

/// The local sources, and what each saw last.
struct Local {
    descriptors: Vec<Descriptor>,
    /// Per source, when it last polled.
    polled: BTreeMap<String, Instant>,
    /// Per path, the modification time last written, so a file is written once per change.
    seen: BTreeMap<PathBuf, u64>,
    /// The desktop's last title, so a window is written once while it stays in front.
    last_title: String,
    started: u64,
}

impl Local {
    fn new(paths: &Paths) -> Local {
        let (descriptors, _) = super::descriptors_in_effect(paths);
        Local {
            descriptors: descriptors.into_iter().filter(|d| d.source.needs == adage_world::Needs::Local).collect(),
            polled: BTreeMap::new(),
            seen: BTreeMap::new(),
            last_title: String::new(),
            started: adage_core::time::now_ms(),
        }
    }

    fn poll(&mut self, paths: &Paths) {
        let cfg = config::load(paths).config;
        let now = adage_core::time::now_ms();
        let due: Vec<Descriptor> = self
            .descriptors
            .iter()
            .filter(|d| self.polled.get(d.name()).is_none_or(|t| t.elapsed() >= Duration::from_secs(d.every_secs())))
            .cloned()
            .collect();
        for d in due {
            self.polled.insert(d.name().to_string(), Instant::now());
            let batch = match d.name() {
                "meetings" => self.files(&d, &cfg.world_transcripts, now, true),
                "files" => self.files(&d, &cfg.world_roots, now, false),
                "desktop" if cfg.world_desktop => self.desktop(&d, now),
                _ => continue,
            };
            if let Some(b) = batch {
                let _ = ingest::write(paths, b);
            }
        }
    }

    /// Files under `dirs` modified since the last poll (or within the descriptor's lookback on
    /// the first), each an artifact and an event.
    fn files(&mut self, d: &Descriptor, dirs: &[PathBuf], now: u64, transcripts: bool) -> Option<Batch> {
        if dirs.is_empty() {
            return None;
        }
        let lookback = d.lookback_secs() * 1000;
        let floor = now.saturating_sub(lookback);
        let glob = d.watch.as_ref().map(|w| w.glob.clone()).unwrap_or_default();
        let mut batch = Batch { source: d.name().to_string(), via: "follow".into(), ..Batch::default() };
        let mut found = 0usize;
        for root in dirs {
            for (path, mtime) in recent_files(root, floor, &glob) {
                if found >= MAX_FILES {
                    break;
                }
                let changed = self.seen.get(&path).is_none_or(|m| *m < mtime);
                if !changed {
                    continue;
                }
                self.seen.insert(path.clone(), mtime);
                found += 1;
                let id = format!("file:{}", path.display());
                let title = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                let summary = transcripts.then(|| head(&path)).flatten();
                batch.artifacts.push(Artifact {
                    id: id.clone(),
                    kind: if transcripts { ArtifactKind::Transcript } else { ArtifactKind::File },
                    title,
                    locator: crate::detect::tilde(&path),
                    summary,
                    embedding: None,
                    first_seen: mtime,
                    last_touched: mtime,
                    related: vec![],
                    state: None,
                    people: vec![],
                });
                batch.events.push(ActivityEvent {
                    ts: mtime.max(self.started.min(now)),
                    source: d.name().to_string(),
                    kind: if transcripts { ArtifactKind::Transcript } else { ArtifactKind::File },
                    artifact: id,
                    action: if transcripts { Action::Landed } else { Action::Edited },
                    session: None,
                    detail: None,
                });
            }
        }
        (!batch.events.is_empty()).then_some(batch)
    }

    /// The active window's title, once per change: an artifact of the app, opened.
    fn desktop(&mut self, d: &Descriptor, now: u64) -> Option<Batch> {
        let (app, title) = active_window()?;
        if title.is_empty() || title == self.last_title {
            return None;
        }
        self.last_title = title.clone();
        let id = format!("desktop:{app}:{}", adage_store::paths::sanitise(&title));
        let kind = if is_browser(&app) { ArtifactKind::Url } else { ArtifactKind::Doc };
        Some(Batch {
            source: d.name().to_string(),
            via: "follow".into(),
            artifacts: vec![Artifact {
                id: id.clone(),
                kind,
                title: title.clone(),
                locator: app.clone(),
                summary: None,
                embedding: None,
                first_seen: now,
                last_touched: now,
                related: vec![],
                state: None,
                people: vec![],
            }],
            events: vec![ActivityEvent {
                ts: now,
                source: d.name().to_string(),
                kind,
                artifact: id,
                action: Action::Opened,
                session: None,
                detail: None,
            }],
            ..Batch::default()
        })
    }
}

fn is_browser(app: &str) -> bool {
    let a = app.to_ascii_lowercase();
    ["chrome", "safari", "firefox", "arc", "brave", "edge", "chromium", "vivaldi", "zen"].iter().any(|b| a.contains(b))
}

/// The first 300 characters of a transcript, as its summary; `None` when unreadable or empty.
fn head(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 4096];
    let n = f.read(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf[..n]);
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut = adage_trace::world::truncate(&text, adage_trace::world::MAX_SUMMARY).to_string();
    (!cut.is_empty()).then_some(cut)
}

/// Files under `root` (to [`MAX_DEPTH`], skipping [`SKIP_DIRS`] and dotfiles) matching `glob`
/// (`*.{md,txt}`; empty for any) and modified at or after `floor`, with their mtimes, newest first.
fn recent_files(root: &Path, floor: u64, glob: &str) -> Vec<(PathBuf, u64)> {
    let exts: Vec<String> = if glob.is_empty() || glob == "*" {
        Vec::new()
    } else {
        let inner = glob.trim_start_matches("*.").trim_matches(|c| c == '{' || c == '}');
        inner.split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect()
    };
    let mut out = Vec::new();
    walk(root, 0, &mut |path, mtime| {
        if mtime < floor {
            return;
        }
        if !exts.is_empty() {
            let ext = path.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if !exts.contains(&ext) {
                return;
            }
        }
        if out.len() < MAX_FILES * 4 {
            out.push((path.to_path_buf(), mtime));
        }
    });
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out.truncate(MAX_FILES);
    out
}

/// Walk `dir` to [`MAX_DEPTH`], skipping dotfiles and [`SKIP_DIRS`], never following links,
/// calling `f` with each file and its modification time in milliseconds.
fn walk(dir: &Path, depth: usize, f: &mut dyn FnMut(&Path, u64)) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let path = e.path();
        if ft.is_dir() {
            if depth + 1 < MAX_DEPTH {
                walk(&path, depth + 1, f);
            }
        } else if ft.is_file() {
            let Ok(meta) = e.metadata() else { continue };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            f(&path, mtime);
        }
    }
}

/// The active application and its window title, from the platform's own tool (`osascript` on
/// macOS, `xdotool` on Linux); `None` where neither answers. The only commands this module runs
/// (`crates/adage-exec/tests/no_spawn.rs`).
fn active_window() -> Option<(String, String)> {
    use std::process::Command;
    let out = if cfg!(target_os = "macos") {
        Command::new("osascript")
            .args([
                "-e",
                "tell application \"System Events\" to set p to first application process whose frontmost is true",
                "-e",
                "set t to \"\"",
                "-e",
                "try",
                "-e",
                "set t to name of front window of p",
                "-e",
                "end try",
                "-e",
                "return (name of p) & linefeed & t",
            ])
            .output()
            .ok()?
    } else if cfg!(target_os = "linux") {
        Command::new("xdotool").args(["getactivewindow", "getwindowclassname", "getwindowname"]).output().ok()?
    } else {
        return None;
    };
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let app = lines.next().unwrap_or("").trim().to_string();
    let title = lines.next().unwrap_or("").trim().to_string();
    (!app.is_empty()).then_some((app, adage_trace::world::truncate(&title, 256).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_filter_by_extension_time_and_skip_dirs() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(root.join("notes/a.md"), "x").unwrap();
        std::fs::write(root.join("notes/b.txt"), "x").unwrap();
        std::fs::write(root.join("notes/c.py"), "x").unwrap();
        std::fs::write(root.join("node_modules/d.md"), "x").unwrap();
        std::fs::write(root.join(".hidden.md"), "x").unwrap();
        let got: Vec<String> = recent_files(root, 0, "*.{md,txt}")
            .into_iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        let mut sorted = got.clone();
        sorted.sort();
        assert_eq!(sorted, ["a.md", "b.txt"]);
        assert!(recent_files(root, u64::MAX, "*").is_empty(), "nothing is that new");
        assert_eq!(head(&root.join("notes/a.md")).as_deref(), Some("x"));
    }
}
