//! `adage update` end to end (onboarding plan O5, O7, O8): the release check against a local
//! fixture server that counts every request, the transaction with a fake package manager on
//! `PATH`, the way back, the plugin channel with a stub `claude`, and doctor's version checks.
//!
//! The "installed" adage of each world is a hard link to the test binary (a copy where links are
//! not possible) at the place its channel puts it, e.g. under `node_modules` for npm. Every update
//! replaces files by writing a new file and renaming it (as package managers do), so the test
//! binary itself is never written. The fake package manager installs a small script that answers
//! `version` with the new version and hands every other command to the test binary.

#![cfg(unix)]

mod common;

use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

const CURRENT: &str = env!("CARGO_PKG_VERSION");
const REAL: &str = env!("CARGO_BIN_EXE_adage");

/// The next minor release after this build.
fn newer() -> String {
    let mut it = CURRENT.split(['.', '-']);
    let (major, minor) = (it.next().unwrap(), it.next().unwrap().parse::<u64>().unwrap());
    format!("{major}.{}.0", minor + 1)
}

const NPM_ANSWER: &str = "/npm/@adage-sh/cli/latest";
const GITHUB_ANSWER: &str = "/github/repos/stevenvaughanbooth/adage/releases/latest";

/// A local HTTP server answering from a table and recording every request's path.
#[derive(Clone)]
struct Fixture {
    addr: String,
    routes: Arc<Mutex<HashMap<String, (u16, String)>>>,
    hits: Arc<Mutex<Vec<String>>>,
}

impl Fixture {
    fn start() -> Fixture {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let f = Fixture {
            addr: l.local_addr().unwrap().to_string(),
            routes: Arc::new(Mutex::new(HashMap::new())),
            hits: Arc::new(Mutex::new(Vec::new())),
        };
        let g = f.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let g = g.clone();
                std::thread::spawn(move || g.answer(s));
            }
        });
        f
    }
    fn answer(&self, mut s: std::net::TcpStream) {
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && s.read(&mut b).unwrap_or(0) == 1 {
            head.push(b[0]);
        }
        let head = String::from_utf8_lossy(&head).to_string();
        let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
        self.hits.lock().unwrap().push(path.clone());
        let (status, body) = self.routes.lock().unwrap().get(&path).cloned().unwrap_or((404, "{}".into()));
        let _ = write!(
            s,
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    }
    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
    fn route(&self, path: &str, status: u16, body: &str) {
        self.routes.lock().unwrap().insert(path.into(), (status, body.into()));
    }
    fn hits(&self) -> Vec<String> {
        self.hits.lock().unwrap().clone()
    }
}

struct World {
    _w: common::TempWorld,
    root: PathBuf,
    server: Fixture,
}

/// Where npm's platform package puts the binary.
const NPM_BIN: &str = "npm/lib/node_modules/@adage-sh/cli-host/bin";

fn install(at: &Path) {
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    if std::fs::hard_link(REAL, at).is_err() {
        std::fs::copy(REAL, at).unwrap();
    }
}

fn write_exe(p: &Path, body: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let tmp = p.with_extension("tmp-write");
    std::fs::write(&tmp, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&tmp, p).unwrap();
}

/// A new adage: answers `version` with `version`, hands everything else to the test binary.
fn fake_adage(version: &str) -> String {
    format!("#!/bin/sh\nif [ \"$1\" = version ]; then echo \"adage {version}\"; exit 0; fi\nexec '{REAL}' \"$@\"\n")
}

fn sha(p: &Path) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(std::fs::read(p).unwrap()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A run's output with runs of spaces as one (the key column's padding is the renderer's).
fn text(o: &Output) -> String {
    let t = common::text(o);
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        if !(c == ' ' && out.ends_with(' ')) {
            out.push(c);
        }
    }
    out
}

impl World {
    fn new() -> World {
        let w = common::TempWorld::new(&["home/.claude", "data", "bin", "xdg", "codex", "repo/.git", "projects"]);
        let root = w.root.clone();
        let server = Fixture::start();
        server.route(NPM_ANSWER, 200, &format!(r#"{{"name":"@adage-sh/cli","version":"{}"}}"#, newer()));
        let world = World { _w: w, root, server };
        install(&world.npm_adage());
        std::fs::write(
            world.p("data/config.toml"),
            format!("[learn]\nclaude_projects = \"{}\"\n", world.p("projects").display()),
        )
        .unwrap();
        world
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    fn npm_adage(&self) -> PathBuf {
        self.p(NPM_BIN).join("adage")
    }
    fn cmd(&self, exe: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(exe);
        common::scrub(&mut c);
        c.args(args)
            .current_dir(self.p("repo"))
            .env("ADAGE_HOME", self.p("data"))
            .env("HOME", self.p("home"))
            .env("CLAUDE_CONFIG_DIR", self.p("home/.claude"))
            .env("XDG_CONFIG_HOME", self.p("xdg"))
            .env("CODEX_HOME", self.p("codex"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.p("bin").display()))
            .env("ADAGE_RELEASE_URL", self.server.url())
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
            .env("ADAGE_COLOR", "never")
            .env("ADAGE_AUTO_LEARN", "0")
            .stdin(Stdio::null());
        // The fixture server is on this machine: no proxy between.
        for v in ["http_proxy", "https_proxy", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "all_proxy"] {
            c.env_remove(v);
        }
        c
    }
    fn run(&self, exe: &Path, args: &[&str]) -> Output {
        self.cmd(exe, args).output().unwrap()
    }
    /// A fake `npm` on `PATH`: logs its arguments, then installs a new adage (`good`), one that
    /// answers the wrong version (`wrong`), or fails (`fail`).
    fn npm(&self, mode: &str) {
        let target = self.npm_adage();
        let log = self.p("npm.log");
        let body = match mode {
            "fail" => "echo 'npm ERR! code E500' >&2\nexit 1\n".to_string(),
            m => {
                let v = if m == "wrong" { "0.0.1".to_string() } else { newer() };
                let t = target.display();
                format!("rm -f '{t}'\ncat > '{t}.tmp' <<'EOF_ADAGE'\n{}EOF_ADAGE\nchmod 755 '{t}.tmp'\nmv '{t}.tmp' '{t}'\n", fake_adage(&v))
            }
        };
        write_exe(
            &self.p("bin/npm"),
            &format!(
                "#!/bin/sh\necho \"$*\" >> '{}'\n[ \"$1 $2\" = \"install --global\" ] || exit 3\n{body}",
                log.display()
            ),
        );
    }
    fn npm_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.p("npm.log")).unwrap_or_default().lines().map(str::to_string).collect()
    }
    fn cache(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.p("data/cache/latest.json")).expect("the cache")).unwrap()
    }
}

/// T-O5-01: `--check --now` asks the npm registry, writes the cache, and says the pair.
#[test]
fn a_check_writes_the_cache() {
    let w = World::new();
    let o = w.run(Path::new(REAL), &["update", "--check", "--now"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains(&format!("adage {CURRENT} · {} is out", newer())), "{}", text(&o));
    let c = w.cache();
    assert_eq!(c["latest"]["version"], newer().as_str(), "{c}");
    assert_eq!(
        (c["have"].as_str(), c["latest"]["source"].as_str(), &c["failure"]),
        (Some(CURRENT), Some("npm"), &Value::Null)
    );
    assert_eq!(w.server.hits(), vec![NPM_ANSWER.to_string()], "one request, npm's");
    // As data, for the surfaces.
    let o = w.run(Path::new(REAL), &["update", "--check", "--json"]);
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        (v["current"].as_str(), v["latest"].as_str(), v["newer"].as_bool()),
        (Some(CURRENT), Some(newer().as_str()), Some(true))
    );
    assert_eq!(w.server.hits().len(), 1, "the cache answers within a day");
}

/// T-O5-02: an unreachable host is recorded as `offline`, and no second request is made that day.
#[test]
fn a_failed_check_is_cached_for_a_day() {
    let w = World::new();
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let o = w
        .cmd(Path::new(REAL), &["update", "--check"])
        .env("ADAGE_RELEASE_URL", format!("http://{closed}"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("cannot check now: offline (127.0.0.1)"), "{}", text(&o));
    assert_eq!(w.cache()["failure"]["class"], "offline");
    // The server is up now; the day is not over.
    let o = w.run(Path::new(REAL), &["update", "--check"]);
    assert!(text(&o).contains("cannot check now: offline"), "{}", text(&o));
    assert!(w.server.hits().is_empty(), "no request within the day: {:?}", w.server.hits());
    // `--now` asks again.
    w.run(Path::new(REAL), &["update", "--check", "--now"]);
    assert_eq!(w.server.hits().len(), 1);
    // ADAGE_OFFLINE=1 fetches nothing at all; the release known from before is still said.
    let o = w.cmd(Path::new(REAL), &["update", "--check", "--now"]).env("ADAGE_OFFLINE", "1").output().unwrap();
    assert!(text(&o).contains(&format!("{} is out", newer())), "{}", text(&o));
    assert_eq!(w.cache()["failure"]["class"], "offline");
    assert_eq!(w.server.hits().len(), 1);
}

/// T-O5-03: npm's 404 (not published) falls through to GitHub's release; npm blocked does not.
#[test]
fn npm_404_falls_through_to_github_and_a_block_does_not() {
    let w = World::new();
    w.server.route(NPM_ANSWER, 404, r#"{"error":"Not found"}"#);
    w.server.route(
        GITHUB_ANSWER,
        200,
        &format!(r#"{{"tag_name":"v{}","published_at":"2026-10-03T12:00:00Z","html_url":"https://github.com/stevenvaughanbooth/adage/releases/tag/v{0}","body":"- faster hooks\n- a way back"}}"#, newer()),
    );
    let o = w.run(Path::new(REAL), &["update", "--check", "--now"]);
    assert!(text(&o).contains(&format!("{} is out (released", newer())), "{}", text(&o));
    assert!(text(&o).contains("· faster hooks"), "the notes' first lines: {}", text(&o));
    let c = w.cache();
    assert_eq!(
        (c["latest"]["source"].as_str(), c["latest"]["notes"].as_str()),
        (Some("github"), Some("faster hooks\na way back"))
    );
    assert_eq!(w.server.hits(), vec![NPM_ANSWER.to_string(), GITHUB_ANSWER.to_string()]);

    let w = World::new();
    w.server.route(NPM_ANSWER, 403, "{}");
    let o = w.run(Path::new(REAL), &["update", "--check", "--now"]);
    assert!(text(&o).contains("cannot check now: blocked (127.0.0.1)"), "{}", text(&o));
    assert_eq!(w.server.hits(), vec![NPM_ANSWER.to_string()], "GitHub is not asked after a block");

    // Neither has a release: unavailable, not a problem anywhere.
    let w = World::new();
    w.server.route(NPM_ANSWER, 404, "{}");
    let o = w.run(Path::new(REAL), &["update", "--check", "--now"]);
    assert!(text(&o).contains("cannot check now: unavailable"), "{}", text(&o));
}

/// T-O5-04: the learner checks once a day with `update.check = daily`, never with `off`.
#[test]
fn the_learner_checks_daily_and_never_when_off() {
    let w = World::new();
    let learn = |check: &str| {
        let o = w
            .cmd(Path::new(REAL), &["condition", "--auto", "--repo-root", &w.p("repo").to_string_lossy()])
            .env("ADAGE_UPDATE_CHECK", check)
            .env("ADAGE_TEST_NO_SPAWN", "record")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", text(&o));
    };
    learn("0");
    assert!(w.server.hits().is_empty(), "update.check = off makes no request");
    assert!(!w.p("data/cache/latest.json").exists());
    std::fs::write(
        w.p("data/config.toml"),
        format!("[learn]\nclaude_projects = \"{}\"\n[update]\ncheck = \"off\"\n", w.p("projects").display()),
    )
    .unwrap();
    let o = w
        .cmd(Path::new(REAL), &["condition", "--auto", "--repo-root", &w.p("repo").to_string_lossy()])
        .env_remove("ADAGE_UPDATE_CHECK")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(w.server.hits().is_empty(), "the config's off too");
    learn("1");
    assert_eq!(w.server.hits(), vec![NPM_ANSWER.to_string()], "one request after the learn");
    assert_eq!(w.cache()["latest"]["version"], newer().as_str());
    learn("1");
    assert_eq!(w.server.hits().len(), 1, "at most once a day per data directory");
}

/// T-O7-02: `--print` names each channel's command with the version pinned and runs nothing.
#[test]
fn the_plan_is_pinned_per_channel_and_print_runs_nothing() {
    let w = World::new();
    w.npm("good");
    let v = newer();
    let o = w.run(&w.npm_adage(), &["update", "--print"]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains(&format!("adage update · {CURRENT} → {v} · npm")), "{t}");
    assert!(t.contains(&format!("runs npm install --global @adage-sh/cli@{v}")), "{t}");
    assert!(!t.contains("@latest"), "{t}");
    assert!(t.contains("keeps") && t.contains("previous/"), "{t}");
    assert!(w.npm_calls().is_empty() && !w.p("data/previous").exists(), "--print runs and keeps nothing");

    // A source install builds the release's tag (and offers the release's binaries without cargo).
    let cargo = w.p("home/.cargo/bin/adage");
    install(&cargo);
    let o = w.run(&cargo, &["update", "--print"]);
    let t = text(&o);
    assert!(t.contains(&format!("--tag v{v} adage-cli adage-exec adage-train")), "{t}");
    assert!(t.contains("cargo is not on PATH") && t.contains("adage update --from release"), "{t}");
    // The release installer's install (channel.json from `adage init`'s Binaries step).
    std::fs::write(w.p("data/channel.json"), r#"{"channel":"release","version":"0.1.0","at":1}"#).unwrap();
    let o = w.run(&cargo, &["update", "--print"]);
    let t = text(&o);
    assert!(
        t.contains("· release") && t.contains(&format!("{}/v{v}/adage-cli-installer.sh | sh", w.server.url())),
        "{t}"
    );
    // A checkout pulls and rebuilds in place, whatever the release.
    let co = w.p("co/target/debug/adage");
    install(&co);
    std::fs::create_dir_all(w.p("co/.git")).unwrap();
    std::fs::write(w.p("co/Cargo.toml"), "[workspace.package]\nversion = \"0.1.0\"\n").unwrap();
    let o = w.run(&co, &["update", "--print"]);
    let t = text(&o);
    assert!(
        t.contains("· checkout") && t.contains("pull --ff-only") && t.contains("cargo build --locked --manifest-path"),
        "{t}"
    );
}

/// T-O7-03: nothing newer: nothing kept, nothing run, exit 0.
#[test]
fn up_to_date_runs_nothing() {
    let w = World::new();
    w.npm("good");
    w.server.route(NPM_ANSWER, 200, &format!(r#"{{"version":"{CURRENT}"}}"#));
    let o = w.run(&w.npm_adage(), &["update"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains(&format!("adage {CURRENT} is the latest · nothing to do")), "{}", text(&o));
    assert!(w.npm_calls().is_empty() && !w.p("data/previous").exists());
}

/// T-O7-04: the package manager installs the new adage: the old bytes are kept, the version is
/// verified, the new binary finishes (doctor), done; then `--rollback` puts the old bytes back.
#[test]
fn an_update_keeps_verifies_finishes_and_rolls_back() {
    let w = World::new();
    w.npm("good");
    let v = newer();
    // The installed bytes, read now: the shared target directory may rebuild the test binary
    // while this runs.
    let old = sha(&w.npm_adage());
    let o = w.run(&w.npm_adage(), &["update"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert_eq!(w.npm_calls(), vec![format!("install --global @adage-sh/cli@{v}")]);
    let kept = w.p(&format!("data/previous/{CURRENT}"));
    assert_eq!(sha(&kept.join("adage")), old, "the old bytes are kept");
    let prev: Value = serde_json::from_slice(&std::fs::read(kept.join("previous.json")).unwrap()).unwrap();
    assert_eq!(
        (prev["from"].as_str(), prev["to"].as_str(), prev["channel"].as_str()),
        (Some(CURRENT), Some(v.as_str()), Some("npm"))
    );
    assert!(t.contains(&format!("version adage {v} ✓")), "{t}");
    assert!(t.contains("doctor") && t.contains(&format!("done adage {v}")), "the new binary finished: {t}");
    assert!(t.contains("next adage update --rollback"), "{t}");
    assert_ne!(sha(&w.npm_adage()), old);

    let o = w.run(&w.npm_adage(), &["update", "--rollback"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert_eq!(sha(&w.npm_adage()), old, "byte for byte as kept");
    assert!(t.contains(&format!("done adage {CURRENT} restored")), "{t}");
}

/// T-O7-05: the binary installed answers the wrong version: an error naming both, the kept set
/// intact, and `--rollback` restores byte for byte.
#[test]
fn a_wrong_version_is_an_error_and_the_way_back_works() {
    let w = World::new();
    w.npm("wrong");
    let old = sha(&w.npm_adage());
    let o = w.run(&w.npm_adage(), &["update"]);
    let t = text(&o);
    assert!(!o.status.success(), "{t}");
    assert!(t.contains(&format!("is adage 0.0.1, not {}", newer())), "{t}");
    assert!(t.contains("adage update --rollback"), "{t}");
    assert!(!t.contains("done "), "nothing after the failed step: {t}");
    assert_eq!(sha(&w.p(&format!("data/previous/{CURRENT}/adage"))), old);
    let o = w.run(&w.npm_adage(), &["update", "--rollback"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(sha(&w.npm_adage()), old);
}

/// T-O7-06: registrations pinning a dead path are re-pointed at a terminal (diff, `.adage-bak`);
/// inside an agent session without a press `adage update` is a person's decision and refuses
/// (T-O9-03), so nothing is rewritten there (under a press: `a_press_updates_a_fetch_channel_only`).
#[test]
fn dead_registrations_are_repointed_at_a_terminal_only() {
    for agent in [false, true] {
        let w = World::new();
        w.npm("good");
        for h in ["codex", "opencode"] {
            let o = w.run(
                Path::new(REAL),
                &["install", h, "--write", "--scope", "user", "--bin", "/dead/bin/adage", "--yes"],
            );
            assert!(o.status.success(), "{}", text(&o));
        }
        let codex = w.p("codex/hooks.json");
        let opencode = w.p("xdg/opencode/plugins/adage.js");
        // A second write keeps the first as `.adage-bak`.
        let before = std::fs::read_to_string(&codex).unwrap();
        assert!(before.contains("/dead/bin/adage"));
        let mut c = w.cmd(&w.npm_adage(), &["update"]);
        if agent {
            c.env("CLAUDECODE", "1");
        }
        let o = c.output().unwrap();
        let t = text(&o);
        let new = w.npm_adage().display().to_string();
        if agent {
            assert!(!o.status.success(), "{t}");
            assert!(t.contains("`adage update` is a decision for a person") && t.contains("CLAUDECODE is set"), "{t}");
            assert_eq!(std::fs::read_to_string(&codex).unwrap(), before, "nothing rewritten in an agent session");
            assert!(w.npm_calls().is_empty() && !w.p("data/previous").exists(), "nothing kept or run: {t}");
            assert!(!w.p("codex/hooks.json.adage-bak").exists());
        } else {
            assert!(o.status.success(), "{t}");
            assert!(std::fs::read_to_string(&codex).unwrap().contains(&new), "{t}");
            assert!(std::fs::read_to_string(&opencode).unwrap().contains(&new), "{t}");
            assert_eq!(std::fs::read_to_string(w.p("codex/hooks.json.adage-bak")).unwrap(), before);
            assert!(t.contains("rewrote") && t.contains("hooks.json"), "{t}");
        }
    }
}

/// T-O7-07: the package manager fails: nothing else runs, the kept set stays, exit non-zero.
#[test]
fn a_failed_package_manager_stops_the_update() {
    let w = World::new();
    w.npm("fail");
    let old = sha(&w.npm_adage());
    let o = w.run(&w.npm_adage(), &["update"]);
    let t = text(&o);
    assert!(!o.status.success(), "{t}");
    assert!(t.contains("npm failed") && t.contains("adage update --rollback"), "{t}");
    assert!(!t.contains("doctor"), "no step after the failure: {t}");
    assert!(w.p(&format!("data/previous/{CURRENT}/previous.json")).is_file());
    assert_eq!(sha(&w.npm_adage()), old, "nothing replaced");
}

/// T-O7-08: the plugin channel: Claude Code updates it (`claude plugin update`) when `claude` is on
/// `PATH`; without it the `/plugin` steps and nothing run.
#[test]
fn the_plugin_channel_asks_claude_code() {
    let w = World::new();
    let bin = w.p(&format!("home/.claude/plugins/data/adage-adage/bin/adage-{}", "a".repeat(64)));
    install(&bin);
    let o = w.run(&bin, &["update"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("· plugin") && t.contains("next /plugin"), "{t}");

    let calls = w.p("claude.log");
    let v = newer();
    write_exe(
        &w.p("bin/claude"),
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$*\" in\n  'plugin list --json') echo '[{{\"id\":\"adage@adage\",\"version\":\"{CURRENT}\",\"scope\":\"user\",\"enabled\":true}}]' ;;\n  'plugin marketplace update adage --json') echo '{{\"outcome\":\"ok\"}}' ;;\n  'plugin update adage@adage --json') echo '{{\"outcome\":\"ok\",\"updateOutcome\":\"updated\",\"oldVersion\":\"{CURRENT}\",\"newVersion\":\"{v}\"}}' ;;\n  *) exit 9 ;;\nesac\n",
            calls.display()
        ),
    );
    let o = w.run(&bin, &["update"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("effect next session"), "{t}");
    assert!(t.contains(&format!("adage {CURRENT} → {v}")), "{t}");
    assert_eq!(
        std::fs::read_to_string(&calls).unwrap().lines().collect::<Vec<_>>(),
        ["plugin list --json", "plugin marketplace update adage --json", "plugin update adage@adage --json"]
    );
    assert!(!w.p("data/previous").exists(), "Claude Code keeps its own copies");
}

/// T-O8-03: doctor names two installed versions that disagree, with `adage update` when this
/// binary is the older one; an index from a newer adage fails with the same fix; the plugin's
/// binary is compared by version before digest, and only a `--plugin-dir` checkout is sent to
/// `plugin/build.sh`.
#[test]
fn doctor_names_versions_that_disagree() {
    let w = World::new();
    let doctor = |w: &World| -> Value {
        let o = w.run(Path::new(REAL), &["doctor", "--json"]);
        serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
    };
    let check = |v: &Value, name: &str| v["checks"].as_array().unwrap().iter().find(|c| c["check"] == name).cloned();
    let v = doctor(&w);
    assert_eq!(check(&v, "versions").unwrap()["status"], "pass", "{v:#}");

    // A newer adage on PATH: this one is older.
    write_exe(&w.p("bin/adage"), "#!/bin/sh\necho 'adage 9.9.9'\n");
    let v = doctor(&w);
    let c = check(&v, "versions").unwrap();
    assert_eq!(c["status"], "warn", "{c}");
    assert!(c["detail"].as_str().unwrap().contains("adage 9.9.9 at"), "{c}");
    assert_eq!(c["action"], serde_json::json!({ "kind": "run", "command": "adage update" }), "{c}");
    std::fs::remove_file(w.p("bin/adage")).unwrap();

    // The plugin's binary, another version.
    let data = w.p("home/.claude/plugins/data/adage-adage/bin");
    let digest = data.join(format!("adage-{}", "b".repeat(64)));
    write_exe(&digest, "#!/bin/sh\necho 'adage 9.9.9'\n");
    std::os::unix::fs::symlink(&digest, data.join("adage")).unwrap();
    let v = doctor(&w);
    let c = check(&v, "claude-code plugin binary").unwrap();
    assert_eq!(
        (c["status"].as_str(), &c["action"]),
        (Some("warn"), &serde_json::json!({ "kind": "run", "command": "adage update" })),
        "{c}"
    );

    // The same version, another build: a --plugin-dir checkout is told to rebuild; an installed
    // plugin is fine.
    write_exe(&digest, &format!("#!/bin/sh\necho 'adage {CURRENT}'\n"));
    let v = doctor(&w);
    let c = check(&v, "claude-code plugin binary").unwrap();
    assert_eq!(c["status"], "warn", "{c}");
    assert!(c["fix"].as_str().unwrap().contains("plugin/build.sh"), "{c}");
    std::fs::write(
        w.p("home/.claude/plugins/installed_plugins.json"),
        r#"{"version":2,"plugins":{"adage@adage":[{"scope":"user"}]}}"#,
    )
    .unwrap();
    let v = doctor(&w);
    let c = check(&v, "claude-code plugin binary").unwrap();
    assert_eq!(c["status"], "pass", "{c}");
    assert!(c["detail"].as_str().unwrap().contains("another build"), "{c}");
    std::fs::remove_file(data.join("adage")).unwrap();

    // An index a newer adage wrote.
    std::fs::write(
        w.p("data/index.json"),
        r#"{"format": 999, "generation": "x", "routes": [{"repo": "r", "track": "t", "version": "1.0.0", "route": {"step": {"do": "teleport"}}}]}"#,
    )
    .unwrap();
    let v = doctor(&w);
    let c = check(&v, "index").unwrap();
    assert_eq!(
        (c["status"].as_str(), &c["action"]),
        (Some("fail"), &serde_json::json!({ "kind": "run", "command": "adage update" })),
        "{c}"
    );
}

/// The status card's `update` field (docs/claude-mod/impl/O5-O7-O8-notes.md): absent without a
/// newer release, the pair and the command with one.
#[test]
fn the_card_carries_the_update() {
    let w = World::new();
    let card = |w: &World, args: &[&str]| -> Value {
        let o = w.run(Path::new(REAL), args);
        serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
    };
    assert_eq!(card(&w, &["--json"])["update"], Value::Null, "no cache: nothing to say");
    w.run(Path::new(REAL), &["update", "--check", "--now"]);
    let v = card(&w, &["--json"]);
    let u = &v["update"];
    assert_eq!(
        (u["current"].as_str(), u["latest"].as_str(), u["next"].as_str()),
        (Some(CURRENT), Some(newer().as_str()), Some("adage update")),
        "{u}"
    );
    assert_eq!(u["text"], format!("{} is out (you have {CURRENT})", newer()));
    assert_eq!(card(&w, &["--json", "--for-session"])["update"]["next"], "/adage update");
    // T-O6-02: absent when the cache names this version, an older one, or something malformed.
    let now = adage_core::time::now_ms();
    for latest in [CURRENT, "0.0.1", "not-a-version"] {
        let cache = serde_json::json!({ "checked_at": now, "have": CURRENT, "latest": { "version": latest, "source": "npm" }, "failure": null });
        std::fs::write(w.p("data/cache/latest.json"), cache.to_string()).unwrap();
        assert_eq!(card(&w, &["--json"])["update"], Value::Null, "{latest}");
        assert_eq!(
            String::from_utf8_lossy(&w.run(Path::new(REAL), &["version"]).stdout),
            format!("adage {CURRENT}\n"),
            "{latest}"
        );
    }
    std::fs::write(w.p("data/cache/latest.json"), "{ not json").unwrap();
    assert_eq!(card(&w, &["--json"])["update"], Value::Null, "a cache that is not JSON");
}

/// Strips ANSI escape sequences (a colour run's lines, compared with a plain run's).
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            for d in it.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// T-O6-01, T-O6-03, T-O6-07: with a newer release in the cache every terminal surface says the
/// same pair and the same command, and none turns a problem for it: the card's row (plain, ASCII,
/// colour) with `adage update` first in `next` and the header's lamp unchanged; `adage version`;
/// doctor's `binary` a pass naming `adage update`, its counts and exit code as before.
#[test]
fn every_surface_says_a_newer_release_and_none_turns_amber() {
    let w = World::new();
    let v = newer();
    let out = |args: &[&str], env: &[(&str, &str)]| -> (bool, String) {
        let mut c = w.cmd(Path::new(REAL), args);
        for (k, val) in env {
            c.env(k, val);
        }
        let o = c.output().unwrap();
        (o.status.success(), String::from_utf8_lossy(&o.stdout).to_string())
    };
    let doctor = || -> Value { serde_json::from_str(&out(&["doctor", "--json"], &[]).1).unwrap() };
    let binary = |d: &Value| d["checks"].as_array().unwrap().iter().find(|c| c["check"] == "binary").cloned().unwrap();
    let card_before = out(&[], &[]).1;
    let doctor_before = doctor();
    let doctor_ok = out(&["doctor"], &[]).0;
    assert_eq!(out(&["version"], &[]).1, format!("adage {CURRENT}\n"), "no cache: the version alone");
    assert!(!card_before.contains("is out"), "{card_before}");
    assert_eq!(binary(&doctor_before)["action"], Value::Null);

    assert!(w.run(Path::new(REAL), &["update", "--check", "--now"]).status.success());
    assert_eq!(out(&["version"], &[]).1, format!("adage {CURRENT} · {v} is out (adage update)\n"));

    let row = |card: &str| card.lines().find(|l| l.starts_with("update ")).map(str::to_string);
    let card = out(&[], &[]).1;
    assert_eq!(
        row(&card).as_deref(),
        Some(format!("update      {v} is out (you have {CURRENT}) → adage update").as_str()),
        "{card}"
    );
    assert_eq!(card.lines().next(), card_before.lines().next(), "the header and its lamp do not move");
    assert!(card.lines().any(|l| l.starts_with("next  adage init")), "not connected: connecting stays first: {card}");
    let ascii = out(&[], &[("ADAGE_ASCII", "1")]).1;
    assert_eq!(
        row(&ascii).as_deref(),
        Some(format!("update      {v} is out (you have {CURRENT}) -> adage update").as_str()),
        "{ascii}"
    );
    let colour = out(&[], &[("ADAGE_COLOR", "always")]).1;
    assert_eq!(row(&plain(&colour)), row(&card), "{colour}");
    // Connected (a hook has run here): `adage update` is the first `next`.
    let key: Value = serde_json::from_str(&out(&["savings", "--json"], &[]).1).unwrap();
    let rec = serde_json::json!({ "ts": 1_790_000_000_000u64, "event": "SessionStart", "kind": "session_start",
                                  "arm": "treated", "repo": key["repo"], "generation": "g", "harness": "claude", "decision": {} });
    adage_store::spool::append(&adage_store::Paths::at(w.p("data")).spool("S"), &rec).unwrap();
    let card = out(&[], &[]).1;
    assert_eq!(
        row(&card).as_deref(),
        Some(format!("update      {v} is out (you have {CURRENT}) → adage update").as_str()),
        "{card}"
    );
    assert!(card.contains(&format!("next  adage update · {v} is out")), "{card}");

    let after = doctor();
    let b = binary(&after);
    assert_eq!(b["status"], "pass", "{b}");
    assert!(b["detail"].as_str().unwrap().ends_with(&format!(" · {v} is out")), "{b}");
    assert_eq!(b["action"], serde_json::json!({ "kind": "run", "command": "adage update" }), "{b}");
    assert_eq!(after["summary"], doctor_before["summary"], "the counts do not move");
    let (ok, text) = out(&["doctor"], &[]);
    assert_eq!(ok, doctor_ok, "nor the exit code: {text}");
    assert!(text.lines().any(|l| l.split_whitespace().collect::<Vec<_>>() == ["update", "adage", "update"]), "{text}");
}

/// Issues a press for session `id` in this world's repository, as the mod asks for one after the
/// command hooks' session start, then closes the window with a prompt: the token.
fn press_for(w: &World, id: &str) -> String {
    let repo = w.p("repo");
    let hook = |event: &str, payload: Value| -> String {
        let mut c = w.cmd(Path::new(REAL), &["hook", "claude", event]);
        c.env("ADAGE_ARM", "treated").stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).trim().to_string()
    };
    let transcript = format!("/home/dev/.claude/projects/repo/{id}.jsonl");
    hook(
        "SessionStart",
        serde_json::json!({ "session_id": id, "cwd": repo, "hook_event_name": "SessionStart", "source": "startup", "transcript_path": transcript }),
    );
    let t = hook(
        "Press",
        serde_json::json!({ "session_id": id, "cwd": repo, "hook_event_name": "Press", "source": "startup" }),
    );
    let t: Value = serde_json::from_str(&t).unwrap_or_else(|e| panic!("no press ({e}): {t}"));
    hook(
        "UserPromptSubmit",
        serde_json::json!({ "session_id": id, "cwd": repo, "hook_event_name": "UserPromptSubmit", "prompt": "hello" }),
    );
    t["press"].as_str().unwrap().to_string()
}

/// `adage update` as the mod's press runs it: every agent marker the mod's child carries, the
/// press on stdin, no override (unless `env` adds it).
fn pressed_update(w: &World, exe: &Path, token: &str, env: &[(&str, &str)]) -> Output {
    let mut c = w.cmd(exe, &["update"]);
    c.env("CLAUDECODE", "1").env_remove("ADAGE_ALLOW_NONINTERACTIVE");
    for (k, v) in env {
        c.env(k, v);
    }
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let stdin = serde_json::json!({ "press": token, "session": "s" }).to_string();
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().unwrap()
}

fn stdout_lines(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect()
}

/// T-O9-02: under a press, `adage update` updates a fetch channel (npm): the next token first on
/// stdout and `{updated}` last, `via: press` in `previous.json`, the package manager's output on
/// standard error, and no settings file rewritten even where one pins a dead path (one line says
/// a terminal re-points it). A source install answers `manual` and runs nothing. The same press
/// beside `ADAGE_ALLOW_NONINTERACTIVE=1` refuses.
#[test]
fn a_press_updates_a_fetch_channel_only() {
    let w = World::new();
    w.npm("good");
    let v = newer();
    let o = w
        .run(Path::new(REAL), &["install", "codex", "--write", "--scope", "user", "--bin", "/dead/bin/adage", "--yes"]);
    assert!(o.status.success(), "{}", text(&o));
    let codex = std::fs::read_to_string(w.p("codex/hooks.json")).unwrap();

    // The override beside a press: refused, nothing spent, nothing run.
    let t = press_for(&w, "s");
    let o = pressed_update(&w, &w.npm_adage(), &t, &[("ADAGE_ALLOW_NONINTERACTIVE", "1")]);
    assert!(!o.status.success() && text(&o).contains("will not run inside an agent session"), "{}", text(&o));
    assert!(!String::from_utf8_lossy(&o.stdout).contains("\"next\""), "{}", text(&o));
    assert!(w.npm_calls().is_empty() && !w.p("data/previous").exists());

    let o = pressed_update(&w, &w.npm_adage(), &t, &[]);
    let lines = stdout_lines(&o);
    assert!(o.status.success(), "{}", text(&o));
    let first: Value = serde_json::from_str(&lines[0]).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
    assert!(first["next"].as_str().is_some_and(|n| n.len() == 64), "{}", text(&o));
    let last: Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(last, serde_json::json!({ "updated": { "from": CURRENT, "to": v, "effect": "next session" } }));
    assert_eq!(w.npm_calls(), vec![format!("install --global @adage-sh/cli@{v}")]);
    let prev: Value =
        serde_json::from_slice(&std::fs::read(w.p(&format!("data/previous/{CURRENT}/previous.json"))).unwrap())
            .unwrap();
    assert_eq!(prev["via"], "press");
    assert_eq!(std::fs::read_to_string(w.p("codex/hooks.json")).unwrap(), codex, "no settings file rewritten");
    assert!(!w.p("codex/hooks.json.adage-bak").exists());
    assert!(text(&o).contains("run adage update in a terminal"), "{}", text(&o));

    // A source install: `manual`, nothing run; the press spent, the next token first.
    let cargo = w.p("home/.cargo/bin/adage");
    install(&cargo);
    write_exe(&w.p("bin/cargo"), &format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 1\n", w.p("cargo.log").display()));
    let t = first["next"].as_str().unwrap();
    let o = pressed_update(&w, &cargo, t, &[]);
    assert!(o.status.success(), "{}", text(&o));
    let lines = stdout_lines(&o);
    assert!(serde_json::from_str::<Value>(&lines[0]).unwrap()["next"].is_string(), "{}", text(&o));
    let last: Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(last["manual"]["command"], "adage update", "{last}");
    assert!(last["manual"]["say"].as_str().unwrap().contains("in a terminal"), "{last}");
    assert!(!w.p("cargo.log").exists(), "a build never runs under a press");
}

/// T-O9-02's flags: only bare `adage update` is a press site. Every flag keeps the terminal's
/// check inside an agent session, with a valid press on stdin or none, and spends nothing.
#[test]
fn only_bare_update_takes_a_press() {
    let w = World::new();
    w.npm("good");
    let t = press_for(&w, "s");
    for args in [
        &["update", "--yes"][..],
        &["update", "--from", "npm"],
        &["update", "--rollback"],
        &["update", "--branch", "main"],
    ] {
        let mut c = w.cmd(&w.npm_adage(), args);
        c.env("CLAUDECODE", "1").env_remove("ADAGE_ALLOW_NONINTERACTIVE");
        let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let _ = child
            .stdin
            .take()
            .unwrap()
            .write_all(serde_json::json!({ "press": t, "session": "s" }).to_string().as_bytes());
        let o = child.wait_with_output().unwrap();
        assert!(!o.status.success(), "{args:?}: {}", text(&o));
        assert!(text(&o).contains("will not run inside an agent session"), "{args:?}: {}", text(&o));
        assert!(!String::from_utf8_lossy(&o.stdout).contains("\"next\""), "{args:?}");
    }
    assert!(w.npm_calls().is_empty());
    // Still valid: none of them spent it.
    let o = pressed_update(&w, &w.npm_adage(), &t, &[]);
    assert!(o.status.success(), "{}", text(&o));
}
