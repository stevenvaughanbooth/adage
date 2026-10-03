//! The interpreter plan (U11, the project's environment): whether a Python call is pointed at the
//! checkout's verified environment, advised instead, or left alone, and the person's own Bash
//! allow rules a rewrite must keep. Entry: [`interpreter_plan`], from the command hook's path and
//! the mod's two phases.

use super::BashCall;
use adage_store::Paths;
use adage_tracks::Index;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The marker `adage delegate rewrites off` (and `delegate off`) writes: no `PreToolUse` rewrite on
/// this machine (docs/execution.md, "Rewrites to the project's environment").
pub fn rewrite_off(paths: &Paths) -> PathBuf {
    paths.root.join("exec").join("rewrite-off")
}

/// The environment's Python version, major.minor, from its `pyvenv.cfg` (`version = 3.12.4`, or
/// uv's `version_info = 3.12.4`).
fn venv_version(venv: &Path) -> Option<String> {
    let cfg = std::fs::read_to_string(venv.join("pyvenv.cfg")).ok()?;
    cfg.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        matches!(k.trim(), "version" | "version_info").then(|| {
            let mut parts = v.trim().split('.');
            format!("{}.{}", parts.next().unwrap_or(""), parts.next().unwrap_or(""))
        })
    })
}

/// Whether the environment at `venv` belongs to the checkout at `root`, not to the checkout the
/// instincts were bound in (`bound`): it is not a link into another directory, and no `.pth` file
/// or editable finder in it points into `bound` outside `root` (a worktree whose `.venv` is a link
/// to, or a copy of, the main checkout's would test the main checkout's code).
fn env_of_checkout(venv: &Path, root: &Path, bound: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (croot, cbound) = (canon(root), canon(bound));
    if !canon(venv).starts_with(&croot) {
        return false;
    }
    let elsewhere = |text: &str| {
        text.split(|c: char| c == '\n' || c == '"' || c == '\'' || c.is_whitespace()).any(|t| {
            let p = Path::new(t.trim_end_matches([',', ')', ']', '}']));
            p.is_absolute()
                && (p.starts_with(bound) || p.starts_with(&cbound))
                && !(p.starts_with(root) || p.starts_with(&croot))
        })
    };
    let libs = std::fs::read_dir(venv.join("lib")).into_iter().flatten().flatten();
    for lib in libs {
        let site = lib.path().join("site-packages");
        for f in std::fs::read_dir(&site).into_iter().flatten().flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            let finder = name.starts_with("__editable__") && name.ends_with(".py");
            if !(name.ends_with(".pth") || finder) || f.metadata().map_or(true, |m| m.len() > 64 << 10) {
                continue;
            }
            if std::fs::read_to_string(f.path()).is_ok_and(|t| elsewhere(&t)) {
                return false;
            }
        }
    }
    true
}

/// The Bash allow rules of the person's Claude Code settings that apply in `root`: user
/// (`$CLAUDE_CONFIG_DIR` or `~/.claude`), project and local settings. Unreadable files add
/// nothing. Read only when a rewrite is planned.
pub(super) fn claude_allow_rules(root: &Path) -> Vec<String> {
    let user = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude")));
    let files = user
        .map(|u| u.join("settings.json"))
        .into_iter()
        .chain([root.join(".claude/settings.json"), root.join(".claude/settings.local.json")]);
    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        let rules = v.pointer("/permissions/allow").and_then(Value::as_array).into_iter().flatten();
        out.extend(rules.filter_map(Value::as_str).filter(|r| r.starts_with("Bash")).map(str::to_string));
    }
    out
}

/// The interpreter plan for a call (U11, the project's environment), before any permission check:
/// what the command hook would do, or `None` when the session started in an environment of the
/// person's choosing, rewrites are off, no environment is bound for this checkout, or U11 is not
/// in service here.
pub(super) fn interpreter_plan(
    call: &BashCall<'_>,
    key: &str,
) -> Option<(adage_runtime::pretool::Pre, adage_runtime::pretool::Env)> {
    use adage_runtime::pretool;
    // The person started the session in an environment of their choosing (a virtualenv, a conda
    // environment): `python` and `pip` are that environment's, and Adage leaves them alone.
    if ["VIRTUAL_ENV", "CONDA_PREFIX"].iter().any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty())) {
        return None;
    }
    // Off with `runtime.rewrite = false` (`adage delegate rewrites off`), or the older marker.
    let cfg = adage_store::config::load(call.paths).config;
    if !cfg.runtime_rewrite || rewrite_off(call.paths).exists() {
        return None;
    }
    let (root, cwd) = (call.root, call.cwd);
    let binding = adage_pack::binding::binding(&call.paths.tracks(), key)?;
    let mut env = binding.env?;
    let venv = root.join(env.interpreter.strip_suffix("/bin/python").unwrap_or(".venv"));
    if Path::new(&binding.root) != root {
        // Another checkout of the repository (a per-task worktree, a fresh clone) until the
        // rebind this session start asked for lands: its own environment, by the presence of its
        // interpreter, `pyvenv.cfg` and pytest (a few `stat`s), and only while that environment
        // is this checkout's (review S3). Only test runs: a repro needs the bind-time check that
        // the project imports from this checkout.
        let pytest = std::fs::read_dir(venv.join("lib"))
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| e.path().join("site-packages/pytest/__init__.py").is_file());
        let usable = root.join(&env.interpreter).is_file() && venv.join("pyvenv.cfg").is_file() && pytest;
        if !(usable && env_of_checkout(&venv, root, Path::new(&binding.root))) {
            return None;
        }
        env.pytest = true;
        env.modules.clear();
    }
    // U11 in service here (not extinguished, rejected or turned off).
    let index = Index::try_load(&call.paths.index()).unwrap_or_default();
    if !u11_in_service(&index, key) {
        return None;
    }
    let abs = root.join(&env.interpreter);
    let e = pretool::Env {
        interpreter: pretool::shell_word(&abs.to_string_lossy()),
        shown: env.interpreter.clone(),
        // At the root, the command names the interpreter as the person (and their permission
        // rules) do: `.venv/bin/python` (first-run QA item 7).
        relative: Path::new(&env.interpreter).is_relative().then(|| pretool::shell_word(&env.interpreter)),
        pytest: env.pytest,
        modules: env.modules.clone(),
        root: root.to_path_buf(),
        cwd: cwd.to_path_buf(),
        version: venv_version(&venv),
        uv: root.join("uv.lock").is_file(),
    };
    let read = |script: &str| {
        let p = cwd.join(script);
        let small = std::fs::metadata(&p).is_ok_and(|m| m.is_file() && m.len() <= 256 << 10);
        small.then(|| std::fs::read_to_string(&p).ok()).flatten()
    };
    pretool::plan(call.command, &e, read).map(|pre| (pre, e))
}

/// Whether the U11 instinct (the project's environment) is in service for this repository.
pub(crate) fn u11_in_service(index: &Index, key: &str) -> bool {
    index.for_repo(key).any(|r| {
        matches!(&r.route.evidence.origin, Some(adage_tracks::Origin::Instinct { instinct, .. }) if instinct == "U11")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review S3: a worktree whose `.venv` is the main checkout's (a link, or a copy with the
    /// main checkout's editable install) would test the main checkout's code: no rewrite there.
    #[test]
    fn a_worktree_environment_must_be_its_own() {
        let d = tempfile::tempdir().unwrap();
        let main = d.path().join("click");
        let wt = main.join(".claude/worktrees/agent-1");
        let site = |root: &Path| root.join(".venv/lib/python3.12/site-packages");
        for r in [&main, &wt] {
            std::fs::create_dir_all(site(r)).unwrap();
            std::fs::write(r.join(".venv/pyvenv.cfg"), "home = /usr/bin\nversion_info = 3.12.4\n").unwrap();
        }
        assert_eq!(venv_version(&wt.join(".venv")).as_deref(), Some("3.12"));
        // Its own editable install: fine.
        std::fs::write(site(&wt).join("click.pth"), format!("{}\n", wt.join("src").display())).unwrap();
        assert!(env_of_checkout(&wt.join(".venv"), &wt, &main));
        // A copy of the main checkout's environment: its `.pth` points at the main checkout.
        std::fs::write(site(&wt).join("click.pth"), format!("{}\n", main.join("src").display())).unwrap();
        assert!(!env_of_checkout(&wt.join(".venv"), &wt, &main));
        // An editable finder (setuptools' `__editable__` mapping) that points there.
        std::fs::write(site(&wt).join("click.pth"), "").unwrap();
        std::fs::write(
            site(&wt).join("__editable___click_8_finder.py"),
            format!("MAPPING = {{'click': '{}'}}\n", main.join("src/click").display()),
        )
        .unwrap();
        assert!(!env_of_checkout(&wt.join(".venv"), &wt, &main));
        // A link to the main checkout's environment.
        #[cfg(unix)]
        {
            let linked = d.path().join("linked");
            std::fs::create_dir_all(&linked).unwrap();
            std::os::unix::fs::symlink(main.join(".venv"), linked.join(".venv")).unwrap();
            assert!(!env_of_checkout(&linked.join(".venv"), &linked, &main));
        }
    }
}
