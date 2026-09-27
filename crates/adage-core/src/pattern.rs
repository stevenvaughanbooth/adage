//! One command-pattern algebra: when is an observed command "the same command" as a directed one?
//!
//! Every place that compares commands uses this module: the runtime's follow judgement, a
//! finish-line obligation being met, after-command and after-failure triggers, `adage graph`,
//! interlocking (`adage_tracks::interlock`), and the eval scorer. Before it there were three
//! different answers (the exact template key, a two-token "command head", a token prefix), and each
//! was wrong somewhere: `make gen -j4` never met a `make gen` obligation, `wi-lookup W-1 --json`
//! did not trigger a route mined from `wi-lookup W-1`, and `npm run build` was credited as
//! following `npm run check` (both had the head `npm run`).
//!
//! A command is read into a [`Shape`] (from its canonical template key or its argv, after
//! [`crate::keys::template_command`]):
//!
//! - **program**: what runs (`make`, `python3`, `scripts/wi-lookup`); leading environment
//!   assignments are options, not the program;
//! - **identity**: the program and the subcommand it multiplexes to, from the table
//!   [`MULTIPLEXERS`] (`npm run build`, `make gen`, `cargo test`, `python3 -m pytest`,
//!   `docker compose up`, `uv run pytest`); an interpreter running a script is the script
//!   (`python3 scripts/wi-lookup` and `scripts/wi-lookup` are one identity);
//! - **positionals**: the operands after the identity, in order;
//! - **options**: flags with their values (`--json`, `-j4`, `-C {path}`), and `{env}`
//!   assignments, as a multiset.
//!
//! Patterns come at three [`Level`]s, from narrow to broad:
//!
//! | Level | Matches an observed command when | Used for |
//! | --- | --- | --- |
//! | `Exact` | same identity and positionals, and its options include the pattern's | directed commands (follow, obligations, triggers, scoring) |
//! | `HeadSub` | same identity (program and subcommand) | a dead end's avoided runner |
//! | `Head` | same program | a bare interpreter as a runner (`python3`) |
//!
//! So a directed command D is **met** by an observed O ([`meets`]) when O's key equals D's, or O
//! extends D with only options: `make gen -j4` meets `make gen`, `scripts/wi-lookup W-1 --json`
//! meets `scripts/wi-lookup W-1`; `npm run build` does not meet `npm run check`, `make test` does
//! not meet `make gen`, `cargo test` does not meet `cargo build`, and `pytest tests/a.py` does not
//! meet `pytest` (an extra positional selects something else). Slots match slots: `{item}` in a
//! route's command matches `{id}` in an observed key.
//!
//! Which flags take a value is data too (per program, in [`MULTIPLEXERS`]); elsewhere a flag
//! takes the next token as its value only when that token is a number (`-n 20`, `-j 4`). An
//! unknown flag followed by a word leaves the word a positional, which can only make a match
//! stricter.

/// How general a pattern is. The derived order runs from narrow to broad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Exact,
    HeadSub,
    Head,
}

/// How a program's operands name its subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sub {
    /// No subcommand: the program alone is the identity (`pytest`).
    None,
    /// The first `n` operands name the subcommand (`make gen`, `gh issue view`).
    Words(usize),
    /// A package manager: `run <script>` (and `run-script`) names a script, `test`/`start`/
    /// `stop`/`restart` are `run test` and so on, and `exec`/`x`/`dlx` run another command.
    Scripts,
    /// `<program> run <command…>` runs another command (`uv run pytest`); any other subcommand
    /// is one word.
    RunWrapper,
    /// The operands are another command (`npx prisma generate`).
    Wrapper,
    /// `docker <sub>`, and `docker compose <sub>`.
    Docker,
}

/// A program that multiplexes to subcommands, and the flags it gives a value.
#[derive(Debug, Clone, Copy)]
pub struct Multiplexer {
    pub names: &'static [&'static str],
    pub sub: Sub,
    pub value_flags: &'static [&'static str],
}

/// Programs whose subcommand is part of a command's identity. Data: extend it here.
pub const MULTIPLEXERS: &[Multiplexer] = &[
    Multiplexer {
        names: &["npm", "pnpm", "yarn", "bun"],
        sub: Sub::Scripts,
        value_flags: &["--prefix", "-w", "--workspace", "-C", "--dir", "--filter", "-F", "--cwd"],
    },
    Multiplexer {
        names: &["make", "gmake"],
        sub: Sub::Words(1),
        value_flags: &["-C", "-f", "--file", "--makefile", "--directory", "-o", "-W", "-I"],
    },
    Multiplexer {
        names: &["cargo"],
        sub: Sub::Words(1),
        value_flags: &[
            "-p",
            "--package",
            "--manifest-path",
            "--features",
            "-F",
            "--target",
            "--bin",
            "--test",
            "--example",
            "--bench",
            "--config",
            "-Z",
            "--profile",
        ],
    },
    Multiplexer {
        names: &["go"],
        sub: Sub::Words(1),
        value_flags: &["-C", "-run", "-tags", "-o", "-bench", "-timeout"],
    },
    Multiplexer {
        names: &["uv"],
        sub: Sub::RunWrapper,
        value_flags: &["--python", "-p", "--with", "--directory", "--project", "--package", "--extra", "--group"],
    },
    Multiplexer { names: &["poetry"], sub: Sub::RunWrapper, value_flags: &["-C", "--directory", "-P", "--project"] },
    Multiplexer { names: &["pipenv", "pdm", "hatch"], sub: Sub::RunWrapper, value_flags: &[] },
    Multiplexer { names: &["npx", "bunx", "pnpx"], sub: Sub::Wrapper, value_flags: &["-p", "--package"] },
    Multiplexer {
        names: &["just"],
        sub: Sub::Words(1),
        value_flags: &["-f", "--justfile", "-d", "--working-directory"],
    },
    Multiplexer { names: &["task"], sub: Sub::Words(1), value_flags: &["-t", "--taskfile", "-d", "--dir"] },
    Multiplexer {
        names: &["git"],
        sub: Sub::Words(1),
        value_flags: &["-C", "-c", "--git-dir", "--work-tree", "-m", "-b", "-B"],
    },
    Multiplexer {
        names: &["docker", "podman"],
        sub: Sub::Docker,
        value_flags: &["-f", "--file", "-p", "--project-name", "--env-file", "--context", "-H"],
    },
    Multiplexer {
        names: &["docker-compose"],
        sub: Sub::Words(1),
        value_flags: &["-f", "--file", "-p", "--project-name"],
    },
    Multiplexer { names: &["gh", "glab"], sub: Sub::Words(2), value_flags: &["-R", "--repo", "--json", "-q", "--jq"] },
    Multiplexer { names: &["deno"], sub: Sub::Words(1), value_flags: &["--config", "-c"] },
    Multiplexer {
        names: &["pytest", "py.test"],
        sub: Sub::None,
        value_flags: &["-k", "-m", "-n", "-p", "-c", "-o", "--rootdir", "--maxfail", "--tb", "--junitxml"],
    },
];

/// Interpreters: running one with a script is running the script; `-m <module>` names a module,
/// and `-c`/`-e` inline code.
fn interpreter(name: &str) -> bool {
    let py = name.strip_prefix("python").is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.'));
    py || matches!(name, "node" | "bash" | "sh" | "zsh" | "ruby" | "perl")
}

const CODE_FLAGS: [&str; 5] = ["-c", "-e", "--eval", "-p", "--print"];

/// Flags that switch an interpreter into a subcommand of its own (data: extend it here).
const MODE_FLAGS: &[(&str, &[&str])] = &[("node", &["--test", "--run", "--check"])];

fn mode_flag(interpreter: &str, flag: &str) -> bool {
    let flag = flag.split('=').next().unwrap_or(flag);
    MODE_FLAGS.iter().any(|(i, fs)| *i == interpreter && fs.contains(&flag))
}

fn multiplexer(name: &str) -> Option<&'static Multiplexer> {
    MULTIPLEXERS.iter().find(|m| m.names.contains(&name))
}

fn basename(t: &str) -> &str {
    t.rsplit(['/', '\\']).next().unwrap_or(t)
}

/// A typed placeholder (`{id}`, `{path}`, a route's `{item}`): it matches any other.
pub fn is_slot(t: &str) -> bool {
    t.len() >= 2 && t.starts_with('{') && t.ends_with('}')
}

fn is_env(t: &str) -> bool {
    t == "{env}" || {
        let (name, _) = t.split_once('=').unwrap_or(("", ""));
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }
}

fn tok_eq(a: &str, b: &str) -> bool {
    a == b || (is_slot(a) && is_slot(b))
}

fn seq_eq(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| tok_eq(x, y))
}

/// Every option of `want` has its own match among `have` (a multiset inclusion, slots matching
/// slots).
fn options_include(have: &[String], want: &[String]) -> bool {
    let mut used = vec![false; have.len()];
    want.iter().all(|w| {
        let wt: Vec<&str> = w.split(' ').collect();
        let hit = have.iter().enumerate().find(|(i, h)| {
            let ht: Vec<&str> = h.split(' ').collect();
            !used[*i] && ht.len() == wt.len() && ht.iter().zip(&wt).all(|(x, y)| tok_eq(x, y))
        });
        match hit {
            Some((i, _)) => {
                used[i] = true;
                true
            }
            None => false,
        }
    })
}

/// A command, read for matching (see the module documentation).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shape {
    pub program: String,
    pub identity: Vec<String>,
    pub positionals: Vec<String>,
    pub options: Vec<String>,
}

impl Shape {
    /// Read a canonical template key (`exec make gen -j4`). `None` for a key that is not a
    /// command.
    pub fn of_key(key: &str) -> Option<Shape> {
        let rest = key.strip_prefix("exec ")?;
        let toks: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
        Shape::of_tokens(&toks)
    }

    /// Read an argv (it is templated first, as keys are).
    pub fn of_argv(argv: &[String]) -> Option<Shape> {
        Shape::of_tokens(&crate::keys::template_command(argv))
    }

    /// Read a plain command line, as a track stores it (`./scripts/wi-lookup {item} --json`).
    pub fn of_command(command: &str) -> Option<Shape> {
        let argv: Vec<String> = command.split_whitespace().map(str::to_string).collect();
        Shape::of_argv(&argv)
    }

    /// Read templated tokens.
    pub fn of_tokens(toks: &[String]) -> Option<Shape> {
        read(toks, 0)
    }

    /// Whether this command, directed, is met by `observed`: the same identity and positionals,
    /// and at least this command's options.
    pub fn met_by(&self, observed: &Shape) -> bool {
        seq_eq(&self.identity, &observed.identity)
            && seq_eq(&self.positionals, &observed.positionals)
            && options_include(&observed.options, &self.options)
    }

    pub fn same_identity(&self, other: &Shape) -> bool {
        seq_eq(&self.identity, &other.identity)
    }
}

/// Split tokens after a program into options and positionals. Returns (options, positionals as
/// (index into `toks`, token)).
fn split(toks: &[String], value_flags: &[&str]) -> (Vec<String>, Vec<(usize, String)>) {
    let mut opts = Vec::new();
    let mut pos = Vec::new();
    let mut i = 0;
    let mut rest_positional = false;
    while i < toks.len() {
        let t = &toks[i];
        if rest_positional {
            pos.push((i, t.clone()));
        } else if t == "--" {
            rest_positional = true;
        } else if t == "{env}" || ((t.starts_with('-') || t.starts_with('+')) && t.len() > 1) {
            let takes = !t.contains('=')
                && toks
                    .get(i + 1)
                    .is_some_and(|n| !n.starts_with('-') && (value_flags.contains(&t.as_str()) || n == "{n}"));
            if takes {
                opts.push(format!("{t} {}", toks[i + 1]));
                i += 1;
            } else {
                opts.push(t.clone());
            }
        } else {
            pos.push((i, t.clone()));
        }
        i += 1;
    }
    (opts, pos)
}

fn read(toks: &[String], depth: usize) -> Option<Shape> {
    if depth > 4 {
        return None;
    }
    // Leading environment assignments are options, not the program.
    let env = toks.iter().take_while(|t| is_env(t)).count();
    let mut options: Vec<String> = toks[..env].iter().map(|_| "{env}".to_string()).collect();
    let toks = &toks[env..];
    let first = toks.first()?;
    let prog = first.strip_prefix("./").unwrap_or(first);
    let name = basename(prog);
    let rest = &toks[1..];

    if interpreter(name) {
        let (opts, pos) = split(rest, &[]);
        // The first operand: a script, or the module after `-m`, or inline code after `-c`.
        let mut i = 0;
        while i < rest.len() {
            let t = rest[i].as_str();
            if t == "-m" {
                let module = rest.get(i + 1)?;
                let mut inner = vec![module.clone()];
                inner.extend(rest[i + 2..].iter().cloned());
                let mut s = read(&inner, depth + 1)?;
                s.program = name.to_string();
                let mut id = vec![name.to_string(), "-m".to_string()];
                id.append(&mut s.identity);
                s.identity = id;
                let mut o: Vec<String> = options;
                o.extend(rest[..i].iter().cloned());
                o.append(&mut s.options);
                s.options = o;
                return Some(s);
            }
            if mode_flag(name, t) {
                // A flag that switches the interpreter into a mode of its own: `node --test`
                // (its test runner) is not `node tools/codegen.mjs`.
                let (o2, p2) = split(&rest[i + 1..], &[]);
                options.extend(rest[..i].iter().cloned());
                options.extend(o2);
                return Some(Shape {
                    program: name.to_string(),
                    identity: vec![name.to_string(), t.to_string()],
                    positionals: p2.into_iter().map(|(_, t)| t).collect(),
                    options,
                });
            }
            if CODE_FLAGS.contains(&t) {
                let (o2, p2) = split(&rest[i + 1..], &[]);
                options.extend(rest[..i].iter().cloned());
                options.extend(o2);
                return Some(Shape {
                    program: name.to_string(),
                    identity: vec![name.to_string(), t.to_string()],
                    positionals: p2.into_iter().map(|(_, t)| t).collect(),
                    options,
                });
            }
            if !t.starts_with('-') {
                break;
            }
            i += 1;
        }
        let Some((at, script)) = pos.first().cloned() else {
            options.extend(opts);
            return Some(Shape {
                program: name.to_string(),
                identity: vec![name.to_string()],
                positionals: vec![],
                options,
            });
        };
        // The script is the identity; the interpreter's own flags are options.
        let script = script.strip_prefix("./").unwrap_or(&script).to_string();
        let (o2, p2) = split(&rest[at + 1..], &[]);
        options.extend(rest[..at].iter().filter(|t| t.starts_with('-')).cloned());
        options.extend(o2);
        return Some(Shape {
            program: name.to_string(),
            identity: vec![script],
            positionals: p2.into_iter().map(|(_, t)| t).collect(),
            options,
        });
    }

    let Some(m) = multiplexer(name) else {
        let (opts, pos) = split(rest, &[]);
        options.extend(opts);
        return Some(Shape {
            program: prog.to_string(),
            identity: vec![prog.to_string()],
            positionals: pos.into_iter().map(|(_, t)| t).collect(),
            options,
        });
    };
    let (opts, pos) = split(rest, m.value_flags);
    let word = |k: usize| pos.get(k).map(|(_, t)| t.as_str());
    // (identity words taken from the operands, or a wrapped command starting at an operand)
    let (words, wrapped): (usize, Option<usize>) = match m.sub {
        Sub::None => (0, None),
        Sub::Words(n) => (n.min(pos.len()), None),
        Sub::Scripts => match word(0) {
            Some("run" | "run-script") => (2.min(pos.len()), None),
            Some("exec" | "x" | "dlx") => (1, Some(1)),
            Some(_) => (1, None),
            None => (0, None),
        },
        Sub::RunWrapper => match word(0) {
            Some("run") => (1, Some(1)),
            Some(_) => (1, None),
            None => (0, None),
        },
        Sub::Wrapper => (0, Some(0)),
        Sub::Docker => match word(0) {
            Some("compose") => (2.min(pos.len()), None),
            Some(_) => (1, None),
            None => (0, None),
        },
    };
    let mut identity = vec![name.to_string()];
    identity.extend(pos[..words].iter().map(|(_, t)| t.clone()));
    // `npm test` is `npm run test`.
    if m.sub == Sub::Scripts && words == 1 && matches!(word(0), Some("test" | "start" | "stop" | "restart")) {
        identity.insert(1, "run".to_string());
    }
    if let Some(w) = wrapped {
        if let Some(&(at, _)) = pos.get(w) {
            // Options before the wrapped command are the wrapper's; the command reads itself.
            options.extend(split(&rest[..at], m.value_flags).0);
            let mut inner = read(&rest[at..], depth + 1)?;
            identity.append(&mut inner.identity);
            options.append(&mut inner.options);
            return Some(Shape { program: name.to_string(), identity, positionals: inner.positionals, options });
        }
    }
    options.extend(opts);
    Some(Shape {
        program: name.to_string(),
        identity,
        positionals: pos[words..].iter().map(|(_, t)| t.clone()).collect(),
        options,
    })
}

/// A command pattern at a [`Level`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommandPattern {
    pub level: Level,
    pub shape: Shape,
}

impl CommandPattern {
    /// The exact pattern of a canonical key.
    pub fn exact(key: &str) -> Option<CommandPattern> {
        Shape::of_key(key).map(|shape| CommandPattern { level: Level::Exact, shape })
    }

    /// The canonical pattern of an argv at `level`.
    pub fn of_argv(argv: &[String], level: Level) -> Option<CommandPattern> {
        Shape::of_argv(argv).map(|shape| CommandPattern { level, shape })
    }

    /// A runner a dead end avoids (`python3 -m pytest`, `npm test`): any run of the same program
    /// and subcommand, whatever its operands. A bare interpreter (`python3`) is every run of it.
    pub fn runner(text: &str) -> Option<CommandPattern> {
        let shape = Shape::of_command(text)?;
        let bare = shape.identity.len() == 1 && shape.identity[0] == shape.program && interpreter(&shape.program);
        Some(CommandPattern { level: if bare { Level::Head } else { Level::HeadSub }, shape })
    }

    /// The same pattern, no narrower than `level`.
    pub fn at_least(mut self, level: Level) -> CommandPattern {
        self.level = self.level.max(level);
        self
    }

    pub fn matches(&self, observed: &Shape) -> bool {
        match self.level {
            Level::Exact => self.shape.met_by(observed),
            Level::HeadSub => self.shape.same_identity(observed),
            Level::Head => self.shape.program == observed.program,
        }
    }

    /// Whether this pattern matches an observed canonical key.
    pub fn matches_key(&self, key: &str) -> bool {
        Shape::of_key(key).is_some_and(|o| self.matches(&o))
    }

    /// Whether every command `other` matches, this matches too.
    pub fn subsumes(&self, other: &CommandPattern) -> bool {
        if other.level > self.level {
            return false;
        }
        match self.level {
            // A pattern on the program alone: `other`'s matches must all run this program, which
            // an identity pins only when it starts with the program (not a script run by an
            // interpreter, which may be run directly).
            Level::Head => {
                other.shape.program == self.shape.program
                    && (other.level == Level::Head || other.shape.identity.first() == Some(&other.shape.program))
            }
            Level::HeadSub => self.shape.same_identity(&other.shape),
            Level::Exact => self.shape.met_by(&other.shape),
        }
    }
}

/// Whether a directed command (canonical key `directed`) is met by an observed one: equal keys,
/// or the observed command extends the directed one with options only. Keys that are not
/// commands meet only when equal.
pub fn meets(directed: &str, observed: &str) -> bool {
    directed == observed
        || match (Shape::of_key(directed), Shape::of_key(observed)) {
            (Some(d), Some(o)) => d.met_by(&o),
            _ => false,
        }
}

/// Whether an observed canonical key runs one of a dead end's avoided runners (`a / b`).
pub fn runs_avoided(avoid: &str, observed: &str) -> bool {
    let Some(o) = Shape::of_key(observed) else { return false };
    avoid.split(" / ").filter_map(|r| CommandPattern::runner(r.trim())).any(|p| p.matches(&o))
}

/// Whether a command a route directs (as the track stores it) runs one of the avoided runners.
pub fn directs_avoided(avoid: &str, command: &str) -> bool {
    let Some(c) = Shape::of_command(command) else { return false };
    avoid.split(" / ").filter_map(|r| CommandPattern::runner(r.trim())).any(|p| p.matches(&c))
}

/// Whether two directed commands (as tracks store them) are the same command: either meets the
/// other.
pub fn same_command(a: &str, b: &str) -> bool {
    match (Shape::of_command(a), Shape::of_command(b)) {
        (Some(x), Some(y)) => x.met_by(&y) || y.met_by(&x),
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(cmd: &str) -> String {
        let argv: Vec<String> = cmd.split_whitespace().map(str::to_string).collect();
        format!("exec {}", crate::keys::template_command(&argv).join(" "))
    }

    fn met(directed: &str, observed: &str) -> bool {
        meets(&key(directed), &key(observed))
    }

    #[test]
    fn directed_commands_are_met_by_option_only_extensions() {
        let yes = [
            ("make gen", "make gen"),
            ("make gen", "make gen -j4"),
            ("make gen", "make -j 4 gen"),
            ("make gen", "make -C sub gen"),
            ("make gen", "ACME_ENV=ci make gen"),
            ("scripts/wi-lookup W-1", "scripts/wi-lookup W-1 --json"),
            ("scripts/wi-lookup W-1", "./scripts/wi-lookup W-2042 --json"),
            ("scripts/wi-lookup W-1", "python3 scripts/wi-lookup W-7"),
            ("python3 scripts/wi-lookup {item}", "./scripts/wi-lookup W-2042 --json"),
            ("./scripts/wi-lookup {item} --json", "python3 scripts/wi-lookup W-2042 --json"),
            ("npm run build", "npm run build --silent"),
            ("npm test", "npm run test"),
            ("npm run test", "npm test"),
            ("cargo test", "cargo test --workspace --locked"),
            ("cargo test", "cargo test -p adage-core"),
            ("python3 -m pytest", "python3 -m pytest -q"),
            ("python3 -m pytest", "python3 -m pytest -k billing"),
            ("uv run pytest", "uv run --with x pytest -x"),
            ("docker compose up", "docker compose up -d"),
            ("npx prisma generate", "npx --yes prisma generate"),
            ("git status", "git -C {path} status --short"),
            ("gh issue view W-1", "gh issue view W-2 --json title"),
            ("head -n 20 {item}", "head -n 50 acme/a.py"),
        ];
        for (d, o) in yes {
            assert!(met(d, o), "{o:?} should meet {d:?}");
        }
        let no = [
            ("npm run check", "npm run build"),
            ("make gen", "make test"),
            ("cargo build", "cargo test"),
            ("python3 -m pytest", "python3 -m unittest"),
            ("python3 -m pytest", "python -m pytest"),
            ("python3 -m pytest", "pytest"),
            ("pytest", "pytest tests/a.py"),
            ("scripts/wi-lookup W-1 --json", "scripts/wi-lookup W-1"),
            ("scripts/wi-lookup W-1", "scripts/other W-1"),
            ("npx prisma generate", "npx prisma migrate"),
            ("uv run pytest", "uv run ruff"),
            ("docker compose up", "docker compose down"),
            ("gh issue view W-1", "gh issue close W-1"),
            ("git push", "git push --force origin"),
            ("make gen", "make gen test"),
            ("make", "make gen"),
            ("go test", "go vet"),
            ("just build", "just test"),
            ("task lint", "task test"),
            ("yarn run lint", "yarn run fix"),
        ];
        for (d, o) in no {
            assert!(!met(d, o), "{o:?} must not meet {d:?}");
        }
        // Keys that are not commands meet only when equal.
        assert!(meets("read {path}", "read {path}"));
        assert!(!meets("read {path}", "read {path} [range]"));
        assert!(!meets("exec make gen", "read {path}"));
    }

    #[test]
    fn identities_come_from_the_multiplexer_table() {
        let id = |c: &str| Shape::of_command(c).unwrap().identity.join(" ");
        assert_eq!(id("npm run build -- --watch"), "npm run build");
        assert_eq!(id("pnpm --filter web run lint"), "pnpm run lint");
        assert_eq!(id("make -j4 gen"), "make gen");
        assert_eq!(id("cargo +nightly test"), "cargo test");
        assert_eq!(id("python3 -m pytest tests/a.py"), "python3 -m pytest");
        assert_eq!(id("python3.12 -m pytest"), "python3.12 -m pytest");
        assert_eq!(id("python3 ./scripts/wi-lookup W-1"), "scripts/wi-lookup");
        assert_eq!(id("uv run python -m pytest"), "uv run python -m pytest");
        assert_eq!(id("poetry run pytest -q"), "poetry run pytest");
        assert_eq!(id("npx prisma generate"), "npx prisma");
        assert_eq!(id("just build"), "just build");
        assert_eq!(id("task test"), "task test");
        assert_eq!(id("git -C x status"), "git status");
        assert_eq!(id("docker compose -f a.yml up"), "docker compose up");
        assert_eq!(id("go test ./..."), "go test");
        assert_eq!(id("/usr/bin/make gen"), "make gen");
        assert_eq!(id("ACME_ENV=test python3 -m unittest"), "python3 -m unittest");
        assert_eq!(id("python3 -c print(1)"), "python3 -c");
        assert_eq!(id("tool list"), "tool");
    }

    #[test]
    fn runners_match_any_run_of_their_identity() {
        let r = CommandPattern::runner("python3 -m pytest").unwrap();
        assert_eq!(r.level, Level::HeadSub);
        assert!(r.matches_key(&key("python3 -m pytest tests/test_a.py -v")));
        assert!(!r.matches_key(&key("python -m pytest")));
        assert!(runs_avoided("python3 -m pytest / python -m pytest", &key("python -m pytest -q")));
        assert!(runs_avoided("npm test", &key("npm run test -- --watch")));
        assert!(!runs_avoided("npm test", &key("npm run build")));
        let bare = CommandPattern::runner("python3").unwrap();
        assert_eq!(bare.level, Level::Head);
        assert!(bare.matches_key(&key("python3 tools/gen.py")));
        assert!(!bare.matches_key(&key("tools/gen.py")));
        // A flag-mode subcommand: `node --test` failing says nothing about `node <script>`.
        let node_test = CommandPattern::runner("node --test").unwrap();
        assert_eq!((node_test.level, node_test.shape.identity.join(" ")), (Level::HeadSub, "node --test".to_string()));
        assert!(node_test.matches_key(&key("node --test src/")));
        assert!(!node_test.matches_key(&key("node tools/codegen.mjs")));
        assert!(!runs_avoided("node --test", &key("node tools/codegen.mjs --out gen")));
        assert!(directs_avoided("npm test", "npm test"));
        assert!(directs_avoided("python3 -m pytest", "python3 -m pytest {item}"));
        assert!(!directs_avoided("python3 -m pytest", "make test"));
    }

    #[test]
    fn the_levels_form_a_lattice() {
        let exact = CommandPattern::exact(&key("make gen")).unwrap();
        let flagged = CommandPattern::exact(&key("make gen -j4")).unwrap();
        let sub = exact.clone().at_least(Level::HeadSub);
        let head = exact.clone().at_least(Level::Head);
        assert!(Level::Exact < Level::HeadSub && Level::HeadSub < Level::Head);
        for (a, b) in [(&exact, &flagged), (&sub, &exact), (&head, &sub), (&head, &exact), (&sub, &sub)] {
            assert!(a.subsumes(b), "{a:?} ⊇ {b:?}");
        }
        assert!(!flagged.subsumes(&exact));
        assert!(!exact.subsumes(&sub));
        assert!(!sub.subsumes(&head));
        let test = CommandPattern::exact(&key("make test")).unwrap().at_least(Level::HeadSub);
        assert!(!sub.subsumes(&test) && head.subsumes(&test));
        // A script run by an interpreter can be run directly: `python3` does not cover it.
        let py = CommandPattern::runner("python3").unwrap();
        let script = CommandPattern::runner("python3 scripts/gen").unwrap();
        assert!(!py.subsumes(&script));
        assert!(py.subsumes(&CommandPattern::runner("python3 -m pytest").unwrap()));
        // What a pattern subsumes, it matches.
        let o = Shape::of_key(&key("make -j8 gen")).unwrap();
        assert!(exact.matches(&o) && sub.matches(&o) && head.matches(&o) && !flagged.matches(&o));
    }

    #[test]
    fn same_command_is_symmetric_meeting() {
        assert!(same_command("make gen", "make gen -j4"));
        assert!(same_command("make gen -j4", "make gen"));
        assert!(!same_command("npm run build", "npm run check"));
    }
}
