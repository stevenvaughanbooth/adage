//! `adage --help` and `adage <command> --help` (or `-h`): one table, grouped, in the lexicon's
//! words (`docs/lexicon.md`). A command never treats `--help` as an argument: [`wants`] is checked
//! before anything runs, so `adage condition --help` conditions nothing and `adage map --help`
//! serves nothing.

/// One command: its name, a one-line description, and its usage lines.
struct Cmd {
    name: &'static str,
    about: &'static str,
    usage: &'static [&'static str],
}

/// Groups in the order a new user meets them.
const GROUPS: &[(&str, &[Cmd])] = &[
    (
        "Start",
        &[
            Cmd {
                name: "init",
                about: "first run: detect harnesses, install hooks, sense your traces",
                usage: &["adage init [--repo <path>] [--yes | --print]"],
            },
            Cmd {
                name: "sense",
                about: "read-only: which reflexes your traces would form",
                usage: &["adage sense [--repo <key>] [--since <days>] [--claude-projects <dir>] [--codex-sessions <dir>] [--demo] [--json]"],
            },
            Cmd {
                name: "install",
                about: "print or write a harness's hook registration",
                usage: &["adage install <claude-code|opencode|codex> [--print | --write [--yes]] [--scope project|user] [--bin <path>] [--repo <dir>]"],
            },
            Cmd {
                name: "doctor",
                about: "check the binary, data directory, index, tracks, model and hooks",
                usage: &["adage doctor [--json] [--watch]"],
            },
        ],
    ),
    (
        "Condition",
        &[
            Cmd {
                name: "condition",
                about: "form reflexes from your traces, bind instincts, compile the index",
                usage: &[
                    "adage condition [--claude-projects <dir>] [--codex-sessions <dir>] [--transcripts <file>...]",
                    "                [--cwd-prefix <path>] [--repo <key>] [--outcomes <json>] [--demo] [--no-cache] [--no-instincts] [--json]",
                    "adage condition --instincts-only [--repo-path <dir>] [--repo <key>] [--json]",
                    "                  instincts for one repository from its files alone, before any trace (day 0)",
                    "adage condition --auto --repo-root <path>",
                    "                  condition one repository quietly (what hooks start)",
                ],
            },
            Cmd {
                name: "train",
                about: "compile a skill into one reviewed pack of reflexes (runs adage-train)",
                usage: &[
                    "adage train <SKILL.md> [--repo <path>] [--no-replay] [--json]   compile, replay, review; installs nothing",
                    "adage train accept <name>   install the reviewed pack",
                    "adage train plan --from <file> [--repo <path>] [--sessions N] [--json] [--out plan.json]",
                    "adage train --plan plan.json [--harness claude-code|fake] [--budget USD] …",
                    "adage train report <run-dir>",
                    "                  `adage train --help` lists every option",
                ],
            },
            Cmd {
                name: "models",
                about: "the optional embedding model for intent reflexes",
                usage: &["adage models fetch | install <dir> | status"],
            },
        ],
    ),
    (
        "Review and share",
        &[
            Cmd {
                name: "review",
                about: "reflexes waiting for a person's decision, with evidence and directive text",
                usage: &[
                    "adage review [--json]",
                    "adage review <approve|strengthen|weaken|extinguish|pin|dismiss> <track> <reflex> [--by <name>]",
                    "adage review delegate <track> <reflex> --level e1|e2 [...]   let a reflex act (see adage review delegate)",
                    "adage review revoke <track> <reflex>",
                ],
            },
            Cmd {
                name: "share",
                about: "redaction-check, build and sign a pack of your reflexes",
                usage: &["adage share <track-or-pack> --out <dir> [--key <path>] [--vendor] [--attest <results.jsonl>]... [--ack-scripts] [--repo-root <dir>]"],
            },
            Cmd {
                name: "add",
                about: "verify a signed pack, install it and bind it here, forming",
                usage: &["adage add <pack-dir> [--trust]"],
            },
            Cmd {
                name: "delegate",
                about: "let approved reflexes act: hooks call adage-exec (E1 prefetch, E2 actions)",
                usage: &["adage delegate on [--bin <adage-exec>] | off | status [--json]   [--repo <dir>]", "adage delegate rewrites on|off   rewrites to the project's environment alone"],
            },
        ],
    ),
    (
        "Watch",
        &[
            Cmd {
                name: "watch",
                about: "a live pane: health lights, hook activity, what Adage conditions",
                usage: &["adage watch [--repo <path>] [--demo] [--once [--size WxH]]"],
            },
            Cmd {
                name: "savings",
                about: "what Adage saves here, measured against sessions where it stayed quiet",
                usage: &["adage savings [--repo <path>] [--json]"],
            },
            Cmd {
                name: "ui",
                about: "the full-screen app: live feed, reflexes and review, health fixes, settings",
                usage: &["adage ui [--repo <path>] [--demo]"],
            },
            Cmd {
                name: "map",
                about: "serve the map of your reflexes at http://127.0.0.1:8765/",
                usage: &["adage map [--port <n>]"],
            },
            Cmd {
                name: "status",
                about: "installed tracks, reflexes, strengths and evidence",
                usage: &["adage status"],
            },
            Cmd {
                name: "report",
                about: "directives emitted, followed and ignored, from the spools",
                usage: &["adage report [--json]"],
            },
            Cmd {
                name: "graph",
                about: "stimuli → reflexes → steps → agent responses, with live counts",
                usage: &[
                    "adage graph [--json] [--events <n>]",
                    "adage graph --learned --dump <repo> [--transcripts <file>...] [--json]   the graph engine's trace facts",
                ],
            },
        ],
    ),
    (
        "Settings and plumbing",
        &[
            Cmd {
                name: "config",
                about: "settings in $ADAGE_HOME/config.toml",
                usage: &["adage config [get <key> | set <key> <value>] [--json]"],
            },
            Cmd {
                name: "eval",
                about: "offline replay evals and the bench (docs/evals.md)",
                usage: &["adage eval <dataset-dir> [--split <split>] [--json]   (`adage eval --help` lists the rest)"],
            },
            Cmd {
                name: "hook",
                about: "the hook client harnesses call (reads the payload on stdin)",
                usage: &["adage hook <claude|opencode|codex> <Event>"],
            },
            Cmd {
                name: "hooks-config",
                about: "print a Claude Code hooks settings block",
                usage: &["adage hooks-config [<adage-path>]"],
            },
            Cmd { name: "version", about: "print the version", usage: &["adage version"] },
        ],
    ),
];

const ENV: &str = "Environment: ADAGE_HOME (data directory), ADAGE_DISABLE=1 (kill switch), ADAGE_ARM=treated|control,
ADAGE_CONTROL (control share), ADAGE_AUTO_LEARN=0|1, ADAGE_ALLOW_NONINTERACTIVE=1 (share and add in CI).";

fn find(name: &str) -> Option<&'static Cmd> {
    GROUPS.iter().flat_map(|(_, cmds)| cmds.iter()).find(|c| c.name == name)
}

/// Whether `args` (after the command) ask for help. `eval` and `train` answer their own.
pub fn wants(cmd: &str, rest: &[String]) -> bool {
    if matches!(cmd, "eval" | "train") {
        return false;
    }
    // `adage hook` reads a payload; only its first argument can ask for help.
    if cmd == "hook" {
        return matches!(rest.first().map(String::as_str), Some("--help" | "-h"));
    }
    rest.iter().any(|a| a == "--help" || a == "-h")
}

/// Help for one command; `None` for an unknown one.
pub fn command(name: &str) -> Option<String> {
    let c = find(name)?;
    let mut s = format!("adage {}: {}\n\nusage:\n", c.name, c.about);
    for u in c.usage {
        s.push_str(&format!("  {u}\n"));
    }
    Some(s)
}

/// `adage --help`: every command, grouped, one line each.
pub fn all() -> String {
    let mut s = String::from("adage: agent traces become reflexes.\n\nusage: adage <command> [options]   (adage <command> --help for its options)\n");
    for (group, cmds) in GROUPS {
        s.push_str(&format!("\n{group}:\n"));
        for c in *cmds {
            s.push_str(&format!("  {:<14}{}\n", c.name, c.about));
        }
    }
    s.push_str(&format!("\nBare `adage` prints the status card; `adage ui` opens the app.\n{ENV}\n"));
    s
}

/// Every command `adage` dispatches, for tests.
#[cfg(test)]
pub fn names() -> Vec<&'static str> {
    GROUPS.iter().flat_map(|(_, cmds)| cmds.iter().map(|c| c.name)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_help_and_one_line() {
        for n in names() {
            let h = command(n).unwrap();
            assert!(h.starts_with(&format!("adage {n}: ")), "{h}");
            assert!(all().contains(&format!("  {n:<14}")), "{n}");
        }
        assert!(wants("condition", &["--help".into()]));
        assert!(wants("map", &["--port".into(), "1".into(), "-h".into()]));
        assert!(!wants("eval", &["--help".into()]), "eval answers its own");
        assert!(!wants("hook", &["claude".into(), "--help".into()]));
    }

    #[test]
    fn help_uses_the_lexicon() {
        let text = all() + &names().iter().filter_map(|n| command(n)).collect::<String>();
        for old in ["survey", " learn ", "promote", "demote", "allow-exec", "signal box", "holdout", "route"] {
            assert!(!text.contains(old), "help says {old:?}");
        }
    }
}
