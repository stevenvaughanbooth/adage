//! `adage init` wires every harness (onboarding plan O4) and the card's not-connected rows (O4,
//! O10), through the binary, with an isolated `HOME`, `ADAGE_HOME`, `XDG_CONFIG_HOME`,
//! `CODEX_HOME`, `CLAUDE_CONFIG_DIR` and a `PATH` holding only what each test puts there. No test
//! here reaches the network (`ADAGE_OFFLINE=1`, or a `file://` release) or runs the real `claude`
//! (a fake on `PATH` records its calls), except `live_init_installs_the_plugin_into_claude_code`,
//! which is ignored unless asked for.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

struct World {
    w: common::TempWorld,
}

impl World {
    fn new() -> World {
        let w = common::TempWorld::new(&["home", "bin", "xdg", "data", "cfg"]);
        w.repo("work");
        World { w }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.w.p(rel)
    }
    /// `adage` (this build, or `exe`) in the world's repository.
    fn cmd_with(&self, exe: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(exe);
        common::scrub(&mut c);
        c.args(args)
            .current_dir(self.p("work"))
            .env("ADAGE_HOME", self.p("data"))
            .env("HOME", self.p("home"))
            .env("XDG_CONFIG_HOME", self.p("xdg"))
            .env("CODEX_HOME", self.p("codex"))
            .env("CLAUDE_CONFIG_DIR", self.p("cfg"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.p("bin").display()))
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
            .env("ADAGE_OFFLINE", "1")
            .env("FAKE_CLAUDE_LOG", self.p("claude.log"))
            .stdin(Stdio::null());
        c
    }
    fn cmd(&self, args: &[&str]) -> Command {
        self.cmd_with(Path::new(env!("CARGO_BIN_EXE_adage")), args)
    }
    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(o.status.success(), "adage {args:?}:\n{}", text(&o));
        text(&o)
    }
    /// Claude Code (its configuration directory exists), Codex (a used `CODEX_HOME`) and OpenCode
    /// (its configuration directory) installed.
    fn every_harness(&self) {
        std::fs::create_dir_all(self.p("codex")).unwrap();
        std::fs::write(self.p("codex/config.toml"), "").unwrap();
        std::fs::create_dir_all(self.p("xdg/opencode")).unwrap();
    }
    /// A copy of this build's `adage` with nothing beside it (a hard link where the file system
    /// allows), so a test stages and fetches as an installed binary would.
    fn installed_adage(&self, rel: &str) -> PathBuf {
        let to = self.p(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        let from = Path::new(env!("CARGO_BIN_EXE_adage"));
        if std::fs::hard_link(from, &to).is_err() {
            std::fs::copy(from, &to).unwrap();
        }
        to
    }
    /// A fake `claude` on `PATH` that answers `claude plugin …` as Claude Code 2.1.292 does and
    /// records each call in `claude.log`; `FAKE_CLAUDE_REFUSE=1` makes it refuse the marketplace.
    #[cfg(unix)]
    fn fake_claude(&self) {
        self.w.executable("bin/claude", FAKE_CLAUDE);
    }
    fn claude_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.p("claude.log")).unwrap_or_default().lines().map(str::to_string).collect()
    }
    fn json(&self, rel: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.p(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))).unwrap()
    }
}

#[cfg(unix)]
const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo "$*" >> "$FAKE_CLAUDE_LOG"
C="$CLAUDE_CONFIG_DIR"
case "$*" in
  "plugin list --json")
    if [ -f "$C/fake-installed" ]; then
      printf '[{"id":"adage@adage-local","version":"0.1.0","scope":"%s","enabled":true,"readFromFolder":"%s"}]\n' "$(cat "$C/fake-scope")" "$(cat "$C/fake-installed")"
    else
      echo '[]'
    fi ;;
  "plugin marketplace add "*)
    if [ -n "$FAKE_CLAUDE_REFUSE" ]; then
      echo '{"command":"marketplace-add","outcome":"error","message":"Marketplace sources are restricted by policy"}'
      exit 1
    fi
    printf '%s' "$4" > "$C/fake-marketplace"
    echo '{"command":"marketplace-add","outcome":"ok","marketplace":"adage-local","message":"Successfully added marketplace: adage-local (declared in user settings)"}' ;;
  "plugin install adage@adage-local --scope "*)
    printf '%s/adage' "$(cat "$C/fake-marketplace")" > "$C/fake-installed"
    printf '%s' "$5" > "$C/fake-scope"
    mkdir -p "$C/plugins"
    printf '{"version":2,"plugins":{"adage@adage-local":[{"scope":"%s","version":"0.1.0"}]}}' "$5" > "$C/plugins/installed_plugins.json"
    printf '{"enabledPlugins":{"adage@adage-local":true}}' > "$C/settings.local.json"
    echo '{"command":"install","outcome":"ok","plugin":"adage@adage-local","scope":"user","message":"Successfully installed plugin: adage@adage-local (scope: user)"}' ;;
  "plugin marketplace update adage-local --json")
    echo '{"command":"marketplace-update","outcome":"ok","marketplace":"adage-local","message":"Updated marketplace adage-local"}' ;;
  *) echo "fake claude: unexpected $*" >&2; exit 2 ;;
esac
"#;

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn check<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["check"] == name)
        .unwrap_or_else(|| panic!("no check {name}: {v:#}"))
}

/// Adage's own hook entries in a settings document, by event.
fn adage_events(doc: &Value) -> Vec<String> {
    let mut out: Vec<String> = doc["hooks"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| {
            let t = v.to_string();
            t.contains(r#"["hook","claude","#) || t.contains(" hook codex ")
        })
        .map(|(k, _)| k.clone())
        .collect();
    out.sort();
    out
}

/// T-O4-01, T-O4-02: without `claude` on `PATH`, `init --yes --hooks user` writes the three user
/// files (each diffed, an existing one backed up), doctor is green for every harness, and a
/// second run writes nothing and says where each is registered.
#[test]
fn init_yes_hooks_user_connects_every_harness() {
    let w = World::new();
    w.every_harness();
    std::fs::create_dir_all(w.p("cfg")).unwrap();
    // The person's own Codex hook stays; the file is backed up before Adage's entries join it.
    let mine = json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "notify-send done" }] }] } });
    std::fs::write(w.p("codex/hooks.json"), serde_json::to_string_pretty(&mine).unwrap()).unwrap();
    let t = w.ok(&["init", "--yes", "--hooks", "user"]);
    for s in [
        "3  Hooks",
        "`claude` is not on PATH, so Adage cannot install its plugin for you",
        "/plugin marketplace add stevenvaughanbooth/adage",
        "/plugin install adage@adage",
        "settings.json would be created:",
        "hooks.json would change:",
        "+ ",
        "plugins/adage.js would be created",
        "open /hooks in Codex to approve them",
        "next  claude · start a session",
        "codex · open /hooks in Codex to approve the new hooks",
        "adage update",
    ] {
        assert!(t.contains(s), "init output lacks {s:?}:\n{t}");
    }
    assert!(!t.contains("// Adage for OpenCode"), "the OpenCode plugin source is not dumped:\n{t}");
    let claude = w.json("cfg/settings.json");
    assert_eq!(
        adage_events(&claude),
        ["PostToolBatch", "PostToolUseFailure", "PreToolUse", "SessionEnd", "SessionStart", "Stop", "UserPromptSubmit"],
        "{claude:#}"
    );
    let codex = w.json("codex/hooks.json");
    assert!(codex.to_string().contains("notify-send done") && codex.to_string().contains(" hook codex "), "{codex:#}");
    assert_eq!(
        std::fs::read_to_string(w.p("codex/hooks.json.adage-bak")).unwrap(),
        serde_json::to_string_pretty(&mine).unwrap()
    );
    assert!(std::fs::read_to_string(w.p("xdg/opencode/plugins/adage.js"))
        .unwrap()
        .starts_with("// Adage for OpenCode"));
    // Doctor is green for every harness.
    let o = w.run(&["doctor", "--json"]);
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", text(&o)));
    for h in ["claude-code", "codex", "opencode"] {
        assert_eq!(check(&v, &format!("hooks: {h}"))["status"], "pass", "{h}: {v:#}");
    }
    // T-O4-02: again, nothing is written, and each says where it is registered.
    let files = || {
        ["cfg/settings.json", "codex/hooks.json", "xdg/opencode/plugins/adage.js"]
            .map(|f| std::fs::read(w.p(f)).unwrap())
    };
    let before = files();
    let t = w.ok(&["init", "--yes", "--hooks", "user"]);
    assert_eq!(files(), before, "a second run writes nothing:\n{t}");
    for f in ["cfg/settings.json", "codex/hooks.json", "xdg/opencode/plugins/adage.js"] {
        let s = format!("registered in {} · nothing to write", w.p(f).display());
        assert!(t.contains(&s), "lacks {s:?}:\n{t}");
    }
}

/// `init --yes` without `--hooks` registers nothing and says so (QA first-run #14's wording).
#[test]
fn init_yes_without_hooks_connects_nothing_and_says_so() {
    let w = World::new();
    std::fs::create_dir_all(w.p("cfg")).unwrap();
    let t = w.ok(&["init", "--yes"]);
    assert!(t.contains("--yes does not register them"), "{t}");
    assert!(t.contains("hooks       not registered: claude-code sessions will not hear from Adage"), "{t}");
    assert!(t.contains("next  adage init --yes --hooks user"), "{t}");
    assert!(!w.p("cfg/settings.json").exists());
    let t = w.ok(&["init", "--yes", "--hooks", "none"]);
    assert!(t.contains("next  adage init --yes --hooks user") && !w.p("cfg/settings.json").exists(), "{t}");
}

/// T-O4-03: the Claude Code plugin is registered already (its data directory has a staged
/// binary): no settings file is written beside it.
#[cfg(unix)]
#[test]
fn a_plugin_registration_is_not_registered_again() {
    let w = World::new();
    let data = w.p("cfg/plugins/data/adage-adage/bin");
    std::fs::create_dir_all(&data).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_adage"), data.join("adage")).unwrap();
    let t = w.ok(&["init", "--yes", "--hooks", "user"]);
    assert!(
        t.contains("the Adage plugin (adage@adage) is installed; Claude Code updates it from the adage marketplace"),
        "{t}"
    );
    assert!(t.contains("hooks       registered: claude-code (the Adage plugin)"), "{t}");
    assert!(!w.p("cfg/settings.json").exists(), "no second registration:\n{t}");
}

/// T-O4-04: `--yes --hooks` refuses inside an agent session and writes nothing.
#[test]
fn init_yes_hooks_refuses_inside_an_agent_session() {
    let w = World::new();
    w.every_harness();
    let o = w.cmd(&["init", "--yes", "--hooks", "user"]).env("CLAUDECODE", "1").output().unwrap();
    assert!(!o.status.success() && text(&o).contains("agent session"), "{}", text(&o));
    assert!(!w.p("cfg/settings.json").exists() && !w.p("codex/hooks.json").exists());
    assert!(!w.p("xdg/opencode/plugins").exists());
    // Printing is allowed there, and says what `--yes --hooks user` would do.
    let o = w.cmd(&["init", "--hooks", "user"]).env("CLAUDECODE", "1").output().unwrap();
    assert!(o.status.success() && text(&o).contains("printing only"), "{}", text(&o));
}

/// The plugin route first: with `claude` on `PATH`, `init` stages the plugin from this binary as
/// a local marketplace, adds it, installs it and checks the list; the settings-file registration
/// `adage install` wrote earlier is taken out (diff shown, backup kept), so every event is wired
/// once. A second run stages again (nothing changed) and installs nothing.
#[cfg(unix)]
#[test]
fn the_plugin_route_installs_verifies_and_leaves_one_registration() {
    let w = World::new();
    w.fake_claude();
    let exe = w.installed_adage("inst/adage");
    // Registered through the settings file before, beside the person's own setting.
    let o = w.cmd_with(&exe, &["install", "claude-code", "--write", "--scope", "user"]).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let mut doc = w.json("cfg/settings.json");
    doc["model"] = json!("opus");
    std::fs::write(w.p("cfg/settings.json"), serde_json::to_string_pretty(&doc).unwrap()).unwrap();

    let o = w.cmd_with(&exe, &["init", "--yes", "--hooks", "user"]).output().unwrap();
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    let root = w.p("data/plugin");
    assert_eq!(
        w.claude_calls(),
        [
            "plugin list --json".to_string(),
            "plugin list --json".into(),
            format!("plugin marketplace add {} --scope user --json", root.display()),
            "plugin install adage@adage-local --scope user --json".into(),
            "plugin list --json".into(),
        ],
        "{t}"
    );
    for s in [
        "the Adage plugin, staged from this adage in",
        "ran claude plugin marketplace add",
        "ran claude plugin install adage@adage-local --scope user",
        "installed and enabled · Claude Code reads it from",
        "adage-exec and adage-train not staged",
        "registers Adage's hooks too, so each would run twice; taking them out:",
        "- ",
        "removed Adage's hooks from",
        "hooks       registered: claude-code (the Adage plugin)",
    ] {
        assert!(t.contains(s), "init output lacks {s:?}:\n{t}");
    }
    // The marketplace: one plugin from `./adage`, the module files, this binary and its checksum.
    let market = w.json("data/plugin/.claude-plugin/marketplace.json");
    assert_eq!(
        (market["name"].as_str(), market["plugins"][0]["source"].as_str()),
        (Some("adage-local"), Some("./adage"))
    );
    let plugin = root.join("adage");
    for f in ["hooks/hooks.json", "hooks/register.ts", "scripts/session-start", ".claude-plugin/plugin.json"] {
        assert!(plugin.join(f).is_file(), "{f} not staged");
    }
    let sums = std::fs::read_to_string(plugin.join("dist/SHA256SUMS")).unwrap();
    let (sha, rel) = sums.trim().split_once("  ").unwrap();
    assert!(rel.ends_with("/adage") && !sums.contains("adage-exec"), "{sums}");
    let staged = plugin.join("dist").join(rel);
    assert_eq!(std::fs::metadata(&staged).unwrap().len(), std::fs::metadata(&exe).unwrap().len());
    assert_eq!(sha.len(), 64);
    // One registration left: the settings file keeps the person's setting, not Adage's hooks.
    let after = w.json("cfg/settings.json");
    assert!(adage_events(&after).is_empty() && after["model"] == "opus", "{after:#}");
    assert!(w.json("cfg/settings.json.adage-bak")["hooks"]["Stop"].is_array());
    let o = w.cmd_with(&exe, &["doctor", "--json"]).output().unwrap();
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", text(&o)));
    let cc = check(&v, "hooks: claude-code");
    assert_eq!(
        (cc["status"].as_str(), cc["detail"].as_str()),
        (Some("pass"), Some("registered by the Adage plugin")),
        "{v:#}"
    );

    // Again: staged again with nothing to change, the marketplace refreshed, nothing installed.
    std::fs::remove_file(w.p("claude.log")).unwrap();
    let o = w.cmd_with(&exe, &["init", "--yes", "--hooks", "user"]).output().unwrap();
    let t = text(&o);
    assert!(
        o.status.success()
            && t.contains("the Adage plugin (adage@adage-local) is installed and up to date with this adage"),
        "{t}"
    );
    let calls = w.claude_calls();
    assert!(calls.contains(&"plugin marketplace update adage-local --json".to_string()), "{calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("plugin install")), "{calls:?}");
}

/// `--print` (and `--hooks` without `--yes`) runs nothing and writes nothing, and says what
/// `--yes --hooks user` would do.
#[cfg(unix)]
#[test]
fn print_runs_nothing_and_writes_nothing() {
    let w = World::new();
    w.every_harness();
    w.fake_claude();
    let snapshot = || {
        let mut out = Vec::new();
        let mut stack = vec![w.p("")];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push((p.clone(), std::fs::read(&p).unwrap_or_default()));
                }
            }
        }
        out.sort();
        out
    };
    let before = snapshot();
    for args in [&["init", "--print"][..], &["init", "--print", "--hooks", "user"], &["init", "--hooks", "user"]] {
        let t = w.ok(args);
        assert!(t.contains("printing only") && t.contains("adage init --yes --hooks user"), "{t}");
        assert!(t.contains("stages it from this adage in") && t.contains("claude plugin install"), "{t}");
    }
    assert!(w.claude_calls().is_empty(), "claude was run: {:?}", w.claude_calls());
    assert_eq!(snapshot(), before, "--print writes nothing");
}

/// When `claude` refuses, the settings file is the route, and the typed `/plugin` commands are
/// named.
#[cfg(unix)]
#[test]
fn when_claude_refuses_the_settings_file_is_the_route() {
    let w = World::new();
    w.fake_claude();
    let exe = w.installed_adage("inst/adage");
    let o = w.cmd_with(&exe, &["init", "--yes", "--hooks", "user"]).env("FAKE_CLAUDE_REFUSE", "1").output().unwrap();
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    for s in [
        "the plugin route did not finish",
        "restricted by policy",
        "in Claude Code instead: /plugin marketplace add stevenvaughanbooth/adage, then /plugin install adage@adage",
        "settings.json would be created:",
        "hooks       registered: claude-code (",
    ] {
        assert!(t.contains(s), "lacks {s:?}:\n{t}");
    }
    assert_eq!(adage_events(&w.json("cfg/settings.json")).len(), 7);
}

/// The release target this machine's companions come from (`net::release_target`).
#[cfg(unix)]
fn release_target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        _ => "x86_64-unknown-linux-musl",
    }
}

/// A release directory as dist lays it out: `v<version>/<app>-<target>.tar.xz` holding
/// `<app>-<target>/<app>`, with `<archive>.sha256` beside it.
#[cfg(unix)]
fn release(w: &World, names: &[&str]) -> PathBuf {
    use sha2::Digest;
    let t = release_target();
    let dir = w.p(&format!("release/v{}", env!("CARGO_PKG_VERSION")));
    for n in names {
        let stage = w.p(&format!("release-src/{n}-{t}"));
        std::fs::create_dir_all(&stage).unwrap();
        w.w.executable(
            &format!("release-src/{n}-{t}/{n}"),
            format!("#!/bin/sh\necho {n} {}\n", env!("CARGO_PKG_VERSION")),
        );
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join(format!("{n}-{t}.tar.xz"));
        let ok = Command::new("tar")
            .arg("-cJf")
            .arg(&archive)
            .arg("-C")
            .arg(w.p("release-src"))
            .arg(format!("{n}-{t}"))
            .status()
            .expect("tar builds the fixture archive");
        assert!(ok.success());
        let sum: String =
            sha2::Sha256::digest(std::fs::read(&archive).unwrap()).iter().map(|b| format!("{b:02x}")).collect();
        std::fs::write(dir.join(format!("{n}-{t}.tar.xz.sha256")), format!("{sum}  {n}-{t}.tar.xz\n")).unwrap();
    }
    w.p("release")
}

/// T-O4-06: the Binaries step fetches the missing companions from the release of this version
/// (here a `file://` one), verifies each archive against its `.sha256`, places the binary beside
/// `adage` and records the channel; one altered byte is refused and nothing is placed.
#[cfg(unix)]
#[test]
fn the_binaries_step_fetches_verified_companions() {
    let w = World::new();
    let exe = w.installed_adage("inst/adage");
    let rel = release(&w, &["adage-exec", "adage-train"]);
    let url = format!("file://{}", rel.display());
    let o = w
        .cmd_with(&exe, &["init", "--yes", "--hooks", "none"])
        .env_remove("ADAGE_OFFLINE")
        .env("ADAGE_RELEASE_URL", &url)
        .output()
        .unwrap();
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(
        t.contains("adage-exec and adage-train are not beside adage")
            && t.contains("adage-exec fetched into")
            && t.contains("adage-train fetched into"),
        "{t}"
    );
    for n in ["adage-exec", "adage-train"] {
        let placed = w.p(&format!("inst/{n}"));
        assert!(std::fs::read_to_string(&placed).unwrap().contains(&format!("echo {n}")), "{n}");
        assert!(std::fs::metadata(&placed).unwrap().permissions().mode() & 0o111 != 0, "{n} is executable");
    }
    let channel = w.json("data/channel.json");
    assert_eq!(channel["channel"], "release");
    assert_eq!(channel["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(channel["bin_dir"], w.p("inst").display().to_string());
    assert!(t.contains("binaries    adage, adage-exec, adage-train in"), "{t}");

    // One byte altered in an archive: refused, nothing placed.
    std::fs::remove_file(w.p("inst/adage-train")).unwrap();
    let archive = rel.join(format!("v{}/adage-train-{}.tar.xz", env!("CARGO_PKG_VERSION"), release_target()));
    let mut bytes = std::fs::read(&archive).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    std::fs::write(&archive, bytes).unwrap();
    let o = w
        .cmd_with(&exe, &["init", "--yes", "--hooks", "none"])
        .env_remove("ADAGE_OFFLINE")
        .env("ADAGE_RELEASE_URL", &url)
        .output()
        .unwrap();
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("adage-train: mismatch") && t.contains("nothing written"), "{t}");
    assert!(!w.p("inst/adage-train").exists(), "an unverified binary is never placed");
    assert!(t.contains("adage-train missing") && t.contains("cargo install --locked --git"), "{t}");
    // Offline: one line, and setup goes on.
    let o = w.cmd_with(&exe, &["init", "--yes", "--hooks", "none"]).output().unwrap();
    let t = text(&o);
    assert!(o.status.success() && t.contains("adage-train: cannot fetch it now: offline"), "{t}");
}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// T-O4-08: a registration written by the plugin's digest-named binary names the plugin's stable
/// link (`<data>/bin/adage`), so a plugin update carries it.
#[cfg(unix)]
#[test]
fn a_registration_written_by_the_plugins_binary_names_its_stable_link() {
    let w = World::new();
    w.every_harness();
    let bin = w.p("cfg/plugins/data/adage-adage/bin");
    let digest = w.installed_adage("cfg/plugins/data/adage-adage/bin/adage-0123456789abcdef");
    std::os::unix::fs::symlink("adage-0123456789abcdef", bin.join("adage")).unwrap();
    let o = w.cmd_with(&bin.join("adage"), &["install", "codex", "--write", "--scope", "user"]).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let codex = std::fs::read_to_string(w.p("codex/hooks.json")).unwrap();
    let stable = bin.join("adage").display().to_string();
    assert!(codex.contains(&format!("{stable} hook codex ")), "{codex}");
    assert!(!codex.contains(&digest.display().to_string()), "not the digest-named copy: {codex}");
    // A binary that is not the plugin's names itself.
    let o = w.run(&["install", "opencode", "--write", "--scope", "user"]);
    assert!(o.status.success(), "{}", text(&o));
    let oc = std::fs::read_to_string(w.p("xdg/opencode/plugins/adage.js")).unwrap();
    let here = Path::new(env!("CARGO_BIN_EXE_adage")).canonicalize().unwrap();
    assert!(oc.contains(&format!("{:?}", here.display().to_string())), "{oc}");
}

/// The closing screen quotes the line the next Claude Code session here opens with: the one the
/// session-start hook then prints.
#[test]
fn the_closing_screen_quotes_the_next_sessions_first_line() {
    let w = World::new();
    std::fs::create_dir_all(w.p("cfg")).unwrap();
    let t = w.ok(&["init", "--yes", "--hooks", "user"]);
    let first = t
        .lines()
        .skip_while(|l| !l.starts_with("first line"))
        .nth(1)
        .unwrap_or_else(|| panic!("{t}"))
        .trim()
        .to_string();
    assert!(first.starts_with("adage: "), "{t}");
    let payload =
        json!({ "session_id": "s1", "cwd": w.p("work"), "hook_event_name": "SessionStart", "source": "startup" });
    let mut c = w.cmd(&["hook", "claude", "SessionStart"]);
    c.env("ADAGE_TEST_NO_SPAWN", "record").stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut child = c.spawn().unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    let said: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", text(&o)));
    assert_eq!(said["systemMessage"].as_str(), Some(first.as_str()), "{t}");
}

/// T-O4-09, O10: the card before any agent is connected names the plugin's two slash commands
/// and `adage init`, and each other harness found with `adage init connects it`.
#[test]
fn the_card_before_anything_is_connected() {
    let w = World::new();
    w.every_harness();
    std::fs::create_dir_all(w.p("cfg")).unwrap();
    let card = w.ok(&[]);
    let card = card.replace(env!("CARGO_PKG_VERSION"), "{VERSION}");
    insta::assert_snapshot!("card-not-connected", card);
    let v: Value = serde_json::from_slice(&w.run(&["--json"]).stdout).unwrap();
    assert_eq!(v["connected"], false);
    assert_eq!(v["next"][0]["command"], "adage init", "{v:#}");
    assert!(v["next"].as_array().unwrap().len() <= 2, "{v:#}");
}

/// The live check of the plugin route, against the `claude` on this machine, in a fresh
/// `CLAUDE_CONFIG_DIR`: `adage init --yes --hooks user` stages and installs the plugin;
/// `claude plugin list` lists it enabled, read from the staged directory; a headless session
/// (one model request) runs each of the plugin's hooks once. Ignored by default: it needs
/// `claude` on `PATH` and a Claude account. `cargo test -p adage-cli --test onboarding --
/// --ignored live_`.
#[cfg(unix)]
#[test]
#[ignore = "live: runs the real `claude` and one model request"]
fn live_init_installs_the_plugin_into_claude_code() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let claude = std::env::split_paths(&path)
        .map(|d| d.join("claude"))
        .find(|p| p.is_file())
        .expect("the live check needs `claude` on PATH");
    let w = World::new();
    // The real `claude`, and only it, beside the test's tools.
    std::os::unix::fs::symlink(&claude, w.p("bin/claude")).unwrap();
    // `claude` is a node script: node sits beside it.
    let node = claude.parent().map(Path::to_path_buf).unwrap_or_default();
    let path = format!("{}:{}:/usr/bin:/bin", w.p("bin").display(), node.display());
    let o = w.cmd(&["init", "--yes", "--hooks", "user"]).env("PATH", &path).output().unwrap();
    let t = text(&o);
    assert!(o.status.success() && t.contains("installed and enabled"), "{t}");
    // Claude Code's own view, as a person would ask it.
    let real = |args: &[&str]| {
        let mut c = Command::new(&claude);
        for v in ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_CHILD_SESSION"] {
            c.env_remove(v);
        }
        c.args(args)
            .current_dir(w.p("work"))
            .env("HOME", w.p("home"))
            .env("CLAUDE_CONFIG_DIR", w.p("cfg"))
            .env("ADAGE_HOME", w.p("data"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let o = real(&["plugin", "list", "--json"]);
    let list: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", text(&o)));
    let ours =
        list.as_array().unwrap().iter().find(|p| p["id"] == "adage@adage-local").unwrap_or_else(|| panic!("{list:#}"));
    assert_eq!(ours["enabled"], true, "{list:#}");
    assert_eq!(ours["readFromFolder"].as_str(), Some(w.p("data/plugin/adage").to_str().unwrap()), "{list:#}");
    let o = real(&["plugin", "validate", "--strict", w.p("data/plugin").to_str().unwrap()]);
    assert!(o.status.success(), "the staged marketplace validates:\n{}", text(&o));
    // One headless session: the plugin's hooks each fire once.
    let log = w.p("debug.log");
    let o = real(&["-p", "Reply OK", "--debug-file", log.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let debug = std::fs::read_to_string(&log).unwrap();
    for (what, needle) in [
        ("SessionStart", "Hook SessionStart:startup (SessionStart) success"),
        ("UserPromptSubmit", "adage@adage-local classic.UserPromptSubmit settled"),
        ("Stop", "adage@adage-local classic.Stop settled"),
        (
            "SessionEnd",
            "SessionEnd:other [${CLAUDE_PLUGIN_DATA}/bin/hook hook claude SessionEnd] completed with status 0",
        ),
    ] {
        assert_eq!(debug.matches(needle).count(), 1, "{what} fired once:\n{debug}");
    }
    assert!(debug.contains("adage: "), "the opening line reached the person:\n{debug}");
    assert!(w.p("cfg/plugins/data/adage-adage-local/bin/adage").exists(), "the shim linked the staged binary");
}
