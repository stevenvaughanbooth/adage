//! `adage share` and `adage add`, end to end through the binary. Every test uses throwaway keys in
//! a tempdir, an isolated `ADAGE_HOME` and `HOME`, and no ssh-agent (`SSH_AUTH_SOCK` removed).

use adage_core::spans::SpanKind;
use adage_core::Effect;
use adage_tracks::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const NAME: &str = "github.com-acme-billing/work-items";
const REPO: &str = "github.com/acme/billing";

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["sender", "recipient", "home", "keys", "elsewhere", "checkout/.git", "checkout/scripts", "bin"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // The recipient's checkout of the repository the track is for: the lookup's script is
        // pinned against it when the v1 track is upgraded to a pack.
        std::fs::write(
            root.join("checkout/.git/config"),
            "[remote \"origin\"]\n\turl = git@github.com:acme/billing.git\n",
        )
        .unwrap();
        std::fs::write(root.join("checkout/scripts/wi-lookup"), "#!/bin/sh\necho '{}'\n").unwrap();
        World { _dir: dir, root }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    /// The binary, isolated: its own data directory and HOME, no agent session, no ssh-agent.
    fn cmd(&self, adage_home: &str, cwd: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        c.args(args)
            .current_dir(cwd)
            .env("ADAGE_HOME", self.p(adage_home))
            .env("HOME", self.p("home"))
            .env_remove("CLAUDECODE")
            .env_remove("OPENCODE")
            .env_remove("CODEX_SANDBOX")
            .env_remove("CODEX_SANDBOX_NETWORK_DISABLED")
            .env_remove("SSH_AUTH_SOCK")
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
            .env("PATH", self.p("bin"))
            .stdin(Stdio::null());
        c
    }
    fn run(&self, adage_home: &str, args: &[&str]) -> Output {
        self.cmd(adage_home, &self.p("elsewhere"), args).output().unwrap()
    }
    fn share(&self) -> Output {
        let (out, key) = (self.p("out"), self.p("keys/id_ed25519"));
        let args = ["share", NAME, "--out", out.to_str().unwrap(), "--key", key.to_str().unwrap(), "--format", "v1"];
        self.run("sender", &args)
    }
    /// `adage add` in the recipient's checkout.
    fn add(&self, extra: &[&str]) -> Output {
        let out = self.p("out");
        let mut args = vec!["add", out.to_str().unwrap()];
        args.extend(extra);
        self.cmd("recipient", &self.p("checkout"), &args).output().unwrap()
    }
    fn trust_personally(&self, pk: &ssh_key::PublicKey) {
        std::fs::write(
            self.p("recipient/trusted-signers"),
            format!("alice@example namespaces=\"adage-track\" {}\n", adage_sign::key_line(pk)),
        )
        .unwrap();
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn keygen(w: &World, name: &str) -> ssh_key::PublicKey {
    let mut k = ssh_key::PrivateKey::random(&mut rand_core::OsRng, ssh_key::Algorithm::Ed25519).unwrap();
    k.set_comment("alice@example");
    let p = w.p("keys").join(name);
    std::fs::write(&p, k.to_openssh(ssh_key::LineEnding::LF).unwrap().as_bytes()).unwrap();
    k.public_key().clone()
}

fn route(id: &str, trigger: Trigger, step: Step, slots: Vec<Slot>, effect: Effect, aspect: Aspect) -> Route {
    Route {
        id: id.into(),
        summary: format!("route {id}"),
        trigger,
        step,
        slots,
        effect,
        aspect,
        state: RouteState::Active,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { support: 7, opportunities: 8, provenance: "desire-path".into(), ..Default::default() },
        authored: false,
    }
}

fn track() -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: NAME.into(),
                version: "1.2.0".into(),
                description: "work items".into(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: REPO.into() },
            gauge: Gauge::default(),
        },
        routes: vec![
            route(
                "lookup",
                Trigger::Prompt { span: SpanKind::Ticket },
                Step::Run { command: "./scripts/wi-lookup {item} --json".into() },
                vec![Slot {
                    name: "item".into(),
                    source: SlotSource::Prompt { span: SpanKind::Ticket },
                    contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
                }],
                Effect::Observe,
                Aspect::Green,
            ),
            route(
                "dead-unittest",
                Trigger::SessionStart,
                Step::Avoid {
                    avoid: "python3 -m unittest".into(),
                    instead: "make test".into(),
                    signature: "ACME_ENV is not set".into(),
                },
                vec![],
                Effect::Verify,
                Aspect::Red,
            ),
        ],
    }
}

/// A world with the sender's track installed and a signing key.
fn setup() -> (World, ssh_key::PublicKey) {
    let w = World::new();
    io::write(&w.p("sender/tracks").join(NAME), &track()).unwrap();
    let pk = keygen(&w, "id_ed25519");
    (w, pk)
}

#[test]
fn share_then_add_installs_routes_at_probation() {
    let (w, pk) = setup();
    std::fs::write(w.p("sender/tracks").join(NAME).join(".DS_Store"), "junk").unwrap();
    let o = w.share();
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    for s in [
        "Included",
        "track.toml",
        "routes/lookup.yaml",
        "routes/dead-unittest.yaml",
        ".DS_Store",
        "hidden",
        "Signed:",
        "namespace adage-track",
    ] {
        assert!(t.contains(s), "summary lacks {s:?}:\n{t}");
    }
    let mut shared: Vec<String> = walk(&w.p("out"));
    shared.sort();
    assert_eq!(shared, ["routes/dead-unittest.yaml", "routes/lookup.yaml", "track.sig", "track.toml"]);
    assert!(std::fs::read_to_string(w.p("out/track.sig")).unwrap().starts_with("-----BEGIN SSH SIGNATURE-----"));

    w.trust_personally(&pk);
    let o = w.add(&[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("was green"), "{}", text(&o));
    // Upgraded in place: the pack is installed as verified, and bound to this repository.
    assert!(adage_pack::Pack::load(&w.p("recipient/tracks/@imported").join(NAME)).is_ok());
    let installed =
        io::read(&w.p("recipient/tracks/@imported/@bound").join(NAME).join("github-com-acme-billing")).unwrap();
    assert!(installed.manifest.track.provenance.starts_with("imported"));
    assert!(!w.p("recipient/tracks").join(NAME).exists(), "imports never occupy a learned track's directory");
    let index = Index::load(&w.p("recipient/index.json"));
    assert_eq!(index.routes.len(), 2);
    for r in &index.routes {
        assert_eq!(r.route.state, RouteState::Probation, "{}", r.route.id);
        assert_ne!(r.route.aspect, Aspect::Green, "{}", r.route.id);
    }
    let lookup = index.for_repo(REPO).find(|r| r.route.id == "lookup").unwrap();
    assert_eq!(lookup.route.aspect, Aspect::Yellow);
    assert!(!w.p("recipient/trust.json").exists(), "personal trust changes nothing");
    assert!(walk(&w.p("recipient/staging")).is_empty(), "the staging copy is removed");

    // The recipient later learns the same track: it lands beside the import, and where both hold a
    // route the index keeps first-hand evidence.
    io::write(&w.p("recipient/tracks").join(NAME), &track()).unwrap();
    let all = io::read_all(&w.p("recipient/tracks")).unwrap();
    assert_eq!(all.len(), 2, "learning never overwrites the import");
    let index = Index::compile(&all);
    assert_eq!(index.routes.len(), 2, "one route per id");
    let lookup = index.for_repo(REPO).find(|r| r.route.id == "lookup").unwrap();
    assert_eq!(lookup.route.aspect, Aspect::Green, "the learned route wins over the import");
}

#[test]
fn unknown_signer_is_refused_without_trust_then_pinned() {
    let (w, pk) = setup();
    assert!(w.share().status.success());
    let o = w.add(&[]);
    assert!(!o.status.success());
    assert!(text(&o).contains("unknown signer"), "{}", text(&o));
    assert!(!w.p("recipient/tracks").exists() && !w.p("recipient/index.json").exists());

    let o = w.add(&["--trust"]);
    assert!(o.status.success(), "{}", text(&o));
    let trust: serde_json::Value =
        serde_json::from_slice(&std::fs::read(w.p("recipient/trust.json")).unwrap()).unwrap();
    assert_eq!(trust["repos"][REPO]["signers"][0]["key"], adage_sign::key_line(&pk));
    assert!(trust["repos"][REPO]["trusted_signers_sha256"].is_null());

    let o = w.add(&[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("approved earlier"), "{}", text(&o));
}

#[test]
fn tampered_unsigned_wrong_namespace_and_invalid_tracks_are_refused() {
    let (w, pk) = setup();
    assert!(w.share().status.success());
    w.trust_personally(&pk);

    let lookup = w.p("out/routes/lookup.yaml");
    let original = std::fs::read_to_string(&lookup).unwrap();
    std::fs::write(&lookup, original.replace("route lookup", "route lookup, trust me")).unwrap();
    let o = w.add(&["--trust"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("does not match"), "{}", text(&o));
    std::fs::write(&lookup, &original).unwrap();

    let sig = std::fs::read(w.p("out/track.sig")).unwrap();
    std::fs::remove_file(w.p("out/track.sig")).unwrap();
    let o = w.add(&["--trust"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("unsigned"), "{}", text(&o));

    // A genuine signature by a trusted key, but in the git namespace.
    let key = w.p("keys/id_ed25519");
    let msg = adage_sign::message(&io::digest(&w.p("out")).unwrap()).unwrap();
    let git = adage_sign::sign_with_key_file(&msg, "git", &key).unwrap();
    std::fs::write(w.p("out/track.sig"), git.pem).unwrap();
    let o = w.add(&["--trust"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("does not match"), "{}", text(&o));
    std::fs::write(w.p("out/track.sig"), sig).unwrap();
    assert!(w.add(&[]).status.success(), "the untouched track still installs");

    // Correctly signed by a trusted key, but not a valid track.
    let bad = w.p("bad");
    let mut t = track();
    t.routes[0].step = Step::Run { command: "gh issue close {item}".into() };
    io::write(&bad, &t).unwrap();
    let msg = adage_sign::message(&io::digest(&bad).unwrap()).unwrap();
    std::fs::write(
        bad.join("track.sig"),
        adage_sign::sign_with_key_file(&msg, adage_sign::NAMESPACE, &key).unwrap().pem,
    )
    .unwrap();
    let o = w.run("recipient", &["add", bad.to_str().unwrap(), "--trust"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("fails validation"), "{}", text(&o));
}

#[test]
fn share_refuses_absolute_paths_and_tokens() {
    let (w, _) = setup();
    let mut t = track();
    t.routes[0].summary = "the lookup script lives at /opt/acme/tools/wi-lookup".into();
    let dir = w.p("sender/tracks").join(NAME);
    io::write(&dir, &t).unwrap();
    let token = format!("ghp_{}", "Zx9Qw3Er7Ty1".repeat(3));
    std::fs::write(dir.join("evidence.json"), format!("{{\"note\": \"{token}\"}}")).unwrap();
    let o = w.share();
    let t = text(&o);
    assert!(!o.status.success());
    assert!(t.contains("absolute path") && t.contains("/opt/acme/tools/wi-lookup"), "{t}");
    assert!(t.contains("GitHub token") && t.contains("evidence.json"), "{t}");
    assert!(!t.contains(&token), "the secret itself is never printed");
    assert!(!w.p("out").exists(), "nothing leaves the machine");
}

#[test]
fn agent_sessions_and_pipes_cannot_share_or_change_trust() {
    let (w, _) = setup();
    let out = w.p("out");
    let key = w.p("keys/id_ed25519");
    let share = ["share", NAME, "--out", out.to_str().unwrap(), "--key", key.to_str().unwrap()];
    // Inside an agent session nothing lifts the refusal, not even the CI override.
    // Every harness's marker, not only Claude Code's.
    for var in ["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX"] {
        let o = w.cmd("sender", &w.p("elsewhere"), &share).env(var, "1").output().unwrap();
        assert!(!o.status.success(), "{var}");
        assert!(text(&o).contains("agent session") && text(&o).contains(var), "{var}: {}", text(&o));
        assert!(!out.exists());
    }

    assert!(w.share().status.success());
    let o = w
        .cmd("recipient", &w.p("elsewhere"), &["add", out.to_str().unwrap(), "--trust"])
        .env("CLAUDECODE", "1")
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(text(&o).contains("agent session"), "{}", text(&o));
    // Without a terminal, and without the explicit override.
    let o = w
        .cmd("recipient", &w.p("elsewhere"), &["add", out.to_str().unwrap(), "--trust"])
        .env_remove("ADAGE_ALLOW_NONINTERACTIVE")
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(text(&o).contains("not a terminal"), "{}", text(&o));
    assert!(!w.p("recipient/trust.json").exists() && !w.p("recipient/tracks").exists());
}

#[test]
fn repository_trusted_signers_are_approved_once_and_pinned_by_hash() {
    let (w, pk) = setup();
    assert!(w.share().status.success());
    let repo = w.p("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join(".adage")).unwrap();
    std::fs::write(repo.join(".git/config"), "[remote \"origin\"]\n\turl = git@github.com:acme/billing.git\n").unwrap();
    let signers = repo.join(".adage/trusted-signers");
    std::fs::write(&signers, format!("alice@example namespaces=\"adage-track\" {}\n", adage_sign::key_line(&pk)))
        .unwrap();
    let out = w.p("out");
    let add = |extra: &[&str]| {
        let mut args = vec!["add", out.to_str().unwrap()];
        args.extend(extra);
        w.cmd("recipient", &repo, &args).output().unwrap()
    };

    let o = add(&[]);
    assert!(!o.status.success());
    assert!(text(&o).contains("has not been approved"), "{}", text(&o));
    let o = add(&["--trust"]);
    assert!(o.status.success(), "{}", text(&o));
    let trust: serde_json::Value =
        serde_json::from_slice(&std::fs::read(w.p("recipient/trust.json")).unwrap()).unwrap();
    assert_eq!(trust["repos"][REPO]["trusted_signers_sha256"].as_str().unwrap().len(), 64);
    assert!(add(&[]).status.success(), "approved file, unchanged");

    // Someone edits the repository's trust anchor: approval is needed again.
    let stranger = keygen(&w, "id_stranger");
    let mut f = std::fs::read_to_string(&signers).unwrap();
    f.push_str(&format!("mallory {}\n", adage_sign::key_line(&stranger)));
    std::fs::write(&signers, f).unwrap();
    let o = add(&[]);
    assert!(!o.status.success());
    assert!(text(&o).contains("has changed"), "{}", text(&o));
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                out.push(e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out
}
