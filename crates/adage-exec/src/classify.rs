//! Local re-classification of what a request runs (Track C §2.2, §4.2): its effect, from the
//! command table, and whether it is *inert* (repository files cannot make it run code).

use adage_core::{shell, CommandTable, Effect};

/// Interpreters: running one is running whatever script it is given.
const INTERPRETERS: &[&str] =
    &["python", "python3", "node", "bash", "sh", "zsh", "dash", "ruby", "perl", "deno", "bun", "php", "lua"];

fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// Quote one argv element for the shell normaliser (which parses shell text).
fn quote(a: &str) -> String {
    if !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || "_./:=@,+-{}".contains(c)) {
        a.to_string()
    } else {
        format!("'{}'", a.replace('\'', "'\\''"))
    }
}

/// The effect of running `argv`, as this machine's command table classifies it. A path as the
/// program (a repository or vendored script) or an interpreter is **Environment**, whatever it is
/// named: naming a script `wi-lookup` makes it observe for *directing*, never for *executing*.
/// Unknown programs are Environment (Design §8.6).
pub fn effect(argv: &[String], table: &CommandTable) -> Effect {
    let Some(prog) = argv.first() else { return Effect::Environment };
    if prog.contains('/') || INTERPRETERS.contains(&basename(prog)) || table.get(prog).is_none() {
        return Effect::Environment;
    }
    let text: Vec<String> = argv.iter().map(|a| quote(a)).collect();
    let normalized = shell::normalize_shell(&text.join(" "), "/", table);
    if normalized.is_empty() || normalized.iter().any(|n| n.contains_unknown()) {
        return Effect::Environment;
    }
    normalized.iter().map(|n| n.effect).max().unwrap_or(Effect::Environment)
}

/// Flags that make an otherwise inert program run another program or write files; their presence
/// removes inert status.
const EXEC_FLAGS: &[&str] = &[
    "--pre",
    "--pre-glob",
    "--search-zip",
    "--ext-diff",
    "--textconv",
    "--output",
    "--exec",
    "--upload-pack",
    "--receive-pack",
    "--open-files-in-pager",
    "--run",
];

/// Why `argv` (a template or a bound argv) is not inert, or `Ok` if it is. `git_hazards` are the
/// repository's hazardous git config keys ([`crate::git::hazards`]): any removes git's status.
pub fn inert(argv: &[String], table: &CommandTable, git_hazards: &[String]) -> Result<(), String> {
    let prog = argv.first().ok_or("empty argv")?;
    if prog.contains('/') {
        return Err(format!("{prog} is a script, never inert"));
    }
    let spec = table.get(prog).ok_or_else(|| format!("{prog} is not in the command table"))?;
    if prog == "git" {
        let sub = argv.get(1).ok_or("git without a subcommand")?;
        let inert_sub = spec.subcommands.get(sub).is_some_and(|s| s.inert);
        if !inert_sub {
            return Err(format!("git {sub} is not inert (only status, log, diff, show and rev-parse are, as argv[1])"));
        }
        if !git_hazards.is_empty() {
            return Err(format!("this repository's git config sets {}", git_hazards.join(", ")));
        }
    } else if !spec.inert {
        return Err(format!("{prog} is not inert (repository files can change what it runs)"));
    }
    for a in &argv[1..] {
        let flag = a.split('=').next().unwrap_or(a);
        if EXEC_FLAGS.contains(&flag) || (prog == "rg" && (a == "-z" || a.starts_with("-z"))) {
            return Err(format!("{prog} {a} can run another program or write files"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Vec<String> {
        s.split(' ').map(String::from).collect()
    }

    #[test]
    fn scripts_and_unknowns_are_environment() {
        let t = CommandTable::load();
        assert_eq!(effect(&v("cat README.md"), t), Effect::Observe);
        assert_eq!(effect(&v("git log -1"), t), Effect::Observe);
        assert_eq!(effect(&v("git push"), t), Effect::Publish);
        assert_eq!(effect(&v("./scripts/wi-lookup W-1 --json"), t), Effect::Environment);
        assert_eq!(effect(&v("python3 scripts/show-config.py"), t), Effect::Environment);
        assert_eq!(effect(&v("wi-lookup W-1"), t), Effect::Environment);
        assert_eq!(effect(&["sed".into(), "-i".into(), "s/a/b/".into(), "f".into()], t), Effect::Author);
    }

    #[test]
    fn inert_class() {
        let t = CommandTable::load();
        for ok in [
            "cat README.md",
            "rg -n TODO src",
            "git log -1 --format=%H",
            "git rev-parse HEAD",
            "jq .x data.json",
            "wc -l {f}",
        ] {
            assert!(inert(&v(ok), t, &[]).is_ok(), "{ok}");
        }
        for bad in [
            "make test",
            "npm run build",
            "cargo build",
            "pytest",
            "python3 x.py",
            "sh -c x",
            "./scripts/x",
            "git push",
            "git -C x log",
            "git blame f",
            "rg --pre ./evil TODO",
            "rg -z x",
            "git diff --ext-diff",
            "git log --output=x",
            "find . -name x",
        ] {
            assert!(inert(&v(bad), t, &[]).is_err(), "{bad}");
        }
        assert!(inert(&v("git status"), t, &["core.fsmonitor".into()]).is_err());
        assert!(inert(&v("cat x"), t, &["core.fsmonitor".into()]).is_ok(), "hazards only affect git");
    }
}
