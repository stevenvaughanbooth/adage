//! Implementation plan §4.1, end to end through the binary: a re-run `adage add` keeps the
//! recipient's review decisions, drift on pinned routes is proposed in `adage review` and applied
//! by `approve` (or silenced by `dismiss`), and passphrase-protected keys are handled. Throwaway
//! keys, isolated `ADAGE_HOME` and `HOME`, no agent session, no ssh-agent.

use adage_core::spans::SpanKind;
use adage_core::Effect;
use adage_tracks::review::{Decision, Reviews};
use adage_tracks::*;
use serde_json::Value;
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
        for d in ["sender", "recipient", "home", "keys", "elsewhere", "repo/.git", "repo/scripts", "bin"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // The recipient's checkout of the repository: `add` runs here, so the lookup's script is
        // pinned (L-ref) when the track is converted to a pack.
        std::fs::write(root.join("repo/.git/config"), "[remote \"origin\"]\n\turl = git@github.com:acme/billing.git\n")
            .unwrap();
        std::fs::write(root.join("repo/scripts/wi-lookup"), "#!/bin/sh\necho '{}'\n").unwrap();
        // Tools the routes name, found by PATH lookup at bind (never run).
        std::fs::write(root.join("bin/make"), "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root.join("bin/make"), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        World { _dir: dir, root }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    fn run(&self, adage_home: &str, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_adage"))
            .args(args)
            .current_dir(self.p("repo"))
            .env("PATH", self.p("bin"))
            .env("ADAGE_HOME", self.p(adage_home))
            .env("HOME", self.p("home"))
            .env_remove("CLAUDECODE")
            .env_remove("OPENCODE")
            .env_remove("CODEX_SANDBOX")
            .env_remove("CODEX_SANDBOX_NETWORK_DISABLED")
            .env_remove("SSH_AUTH_SOCK")
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
    fn ok(&self, adage_home: &str, args: &[&str]) -> String {
        let o = self.run(adage_home, args);
        assert!(o.status.success(), "adage {args:?} failed:\n{}", text(&o));
        text(&o)
    }
    /// The sender publishes `t` as `out`, a pack signed with the test key.
    fn publish(&self, t: &Track, out: &str) {
        let _ = std::fs::remove_dir_all(self.p("sender/tracks"));
        io::write(&self.p("sender/tracks").join(NAME), t).unwrap();
        let (o, key) = (self.p(out), self.p("keys/id_ed25519"));
        self.ok("sender", &["share", NAME, "--out", o.to_str().unwrap(), "--key", key.to_str().unwrap()]);
    }
    fn add(&self, out: &str) -> String {
        self.ok("recipient", &["add", self.p(out).to_str().unwrap()])
    }
    /// The bound track of the imported pack, for this repository.
    fn installed(&self) -> PathBuf {
        self.p("recipient/tracks/@imported/@bound").join(NAME).join("github-com-acme-billing")
    }
    fn route(&self, id: &str) -> Route {
        io::read(&self.installed()).unwrap().routes.into_iter().find(|r| r.id == id).unwrap()
    }
    fn queue(&self) -> Vec<Value> {
        let v: Value = serde_json::from_str(&self.ok("recipient", &["review", "--json"])).unwrap();
        v["queue"].as_array().unwrap().clone()
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn keygen(w: &World) -> ssh_key::PrivateKey {
    let mut k = ssh_key::PrivateKey::random(&mut rand_core::OsRng, ssh_key::Algorithm::Ed25519).unwrap();
    k.set_comment("alice@example");
    std::fs::write(w.p("keys/id_ed25519"), k.to_openssh(ssh_key::LineEnding::LF).unwrap().as_bytes()).unwrap();
    std::fs::write(
        w.p("recipient/trusted-signers"),
        format!("alice@example namespaces=\"adage-pack\" {}\n", adage_sign::key_line(k.public_key())),
    )
    .unwrap();
    k
}

fn route(id: &str, trigger: Trigger, step: Step, effect: Effect, strength: Strength) -> Route {
    let slots = match &step {
        Step::Run { command } if command.contains("{item}") => vec![Slot {
            name: "item".into(),
            source: SlotSource::Prompt { span: SpanKind::Ticket },
            contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
        }],
        _ => vec![],
    };
    Route {
        id: id.into(),
        summary: format!("route {id}"),
        trigger,
        step,
        slots,
        effect,
        strength,
        state: RouteState::Firm,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence {
            support: 7,
            opportunities: 8,
            days: 3,
            rate: 0.875,
            lower_bound: wilson_lower(7, 8),
            provenance: "desire-path".into(),
            ..Default::default()
        },
        authored: false,
    }
}

/// The sender's track at `version`, with the lookup directing `lookup`.
fn track(version: &str, lookup: &str, extra: bool) -> Track {
    let mut routes = vec![
        route(
            "lookup",
            Trigger::Prompt { span: SpanKind::Ticket },
            Step::Run { command: lookup.into() },
            Effect::Observe,
            Strength::Fire,
        ),
        route(
            "dead-unittest",
            Trigger::SessionStart,
            Step::Avoid {
                avoid: "python3 -m unittest".into(),
                instead: "make test".into(),
                signature: "ACME_ENV is not set".into(),
            },
            Effect::Verify,
            Strength::Inhibit,
        ),
        route(
            "check",
            Trigger::SessionStart,
            Step::Run { command: "make check".into() },
            Effect::Verify,
            Strength::Prime,
        ),
    ];
    if extra {
        routes.push(route(
            "lint",
            Trigger::SessionStart,
            Step::Run { command: "make lint".into() },
            Effect::Verify,
            Strength::Prime,
        ));
    }
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: NAME.into(),
                version: version.into(),
                description: "work items".into(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: REPO.into() },
            gauge: Gauge::default(),
        },
        routes,
    }
}

fn reasons(q: &[Value], route: &str) -> Vec<String> {
    q.iter()
        .filter(|i| i["route"] == route)
        .flat_map(|i| i["reasons"].as_array().unwrap().iter().map(|r| r["kind"].as_str().unwrap().to_string()))
        .collect()
}

#[test]
fn reimport_keeps_decisions_and_proposes_changes_to_pinned_routes() {
    let w = World::new();
    keygen(&w);
    w.publish(&track("1.0.0", "./scripts/wi-lookup {item} --json", false), "out1");
    w.add("out1");
    w.ok("recipient", &["review", "extinguish", NAME, "dead-unittest"]);
    w.ok("recipient", &["review", "pin", NAME, "lookup"]);
    w.ok("recipient", &["review", "approve", NAME, "check", "--by", "bob"]);

    // The author ships 2.0.0: the lookup changed, and there is a new route.
    w.publish(&track("2.0.0", "./scripts/wi-lookup {item}", true), "out2");
    let out = w.add("out2");
    assert!(out.contains("kept        your review decisions"), "{out}");
    assert!(out.contains("proposed    lookup"), "{out}");
    let reviews: Reviews = adage_tracks::review::load(&w.installed()).unwrap();
    let decisions: Vec<(&str, Decision)> = reviews.routes.iter().map(|(k, v)| (k.as_str(), v.decision)).collect();
    assert_eq!(
        decisions,
        vec![("check", Decision::Approved), ("dead-unittest", Decision::Extinguished), ("lookup", Decision::Pinned)]
    );
    assert_eq!(reviews.routes["check"].by.as_deref(), Some("bob"));
    assert_eq!(
        w.route("dead-unittest").state,
        RouteState::Extinguished,
        "an extinguish decision survives the re-import"
    );
    let lookup = w.route("lookup");
    assert!(lookup.authored);
    assert_eq!(
        lookup.step,
        Step::Run { command: "./scripts/wi-lookup {item} --json".into() },
        "the recipient's pinned copy is kept"
    );
    assert_eq!(w.route("lint").state, RouteState::Forming);

    // The queue: the pin's drift is proposed with a diff; the new route waits as an import.
    let q = w.queue();
    assert_eq!(reasons(&q, "lookup"), vec!["proposed"]);
    assert_eq!(reasons(&q, "lint"), vec!["imported"]);
    assert!(
        reasons(&q, "check").is_empty() && reasons(&q, "dead-unittest").is_empty(),
        "decided routes stay decided: {q:?}"
    );
    let item = q.iter().find(|i| i["route"] == "lookup").unwrap();
    let diff = item["proposal"]["diff"].as_array().unwrap();
    let step = diff.iter().find(|d| d["field"] == "step").expect("the step differs");
    assert_eq!(step["pinned"], "run `./scripts/wi-lookup {item} --json`");
    assert_eq!(step["proposed"], "run `./scripts/wi-lookup {item}`");
    assert!(item["decisions"].as_array().unwrap().iter().any(|d| d == "dismiss"));
    let listing = w.ok("recipient", &["review"]);
    assert!(
        listing.contains("why         proposed:") && listing.contains("- step") && listing.contains("+ step"),
        "{listing}"
    );

    // Dismissed: the pinned route stays, and re-adding the same version proposes nothing.
    w.ok("recipient", &["review", "dismiss", NAME, "lookup"]);
    assert!(reasons(&w.queue(), "lookup").is_empty());
    w.add("out2");
    assert!(reasons(&w.queue(), "lookup").is_empty(), "a dismissal holds while the proposal is unchanged");

    // 3.0.0 changes it again: proposed again, and approving applies it, still pinned.
    w.publish(&track("3.0.0", "./scripts/wi-lookup {item} --full", true), "out3");
    w.add("out3");
    assert_eq!(reasons(&w.queue(), "lookup"), vec!["proposed"]);
    let out = w.ok("recipient", &["review", "approve", NAME, "lookup"]);
    assert!(out.contains("proposed change applied"), "{out}");
    let lookup = w.route("lookup");
    assert_eq!(lookup.step, Step::Run { command: "./scripts/wi-lookup {item} --full".into() });
    assert!(lookup.authored, "an applied proposal stays pinned");
    assert_ne!(lookup.strength, Strength::Fire, "an import never becomes fire by a proposal");
    assert!(reasons(&w.queue(), "lookup").is_empty());
    assert!(
        !w.installed().join(adage_tracks::proposals::FILE).exists()
            || adage_tracks::proposals::load(&w.installed()).unwrap().routes.is_empty()
    );
    let index = Index::load(&w.p("recipient/index.json"));
    let ir = index.for_repo(REPO).find(|r| r.route.id == "lookup").unwrap();
    assert_eq!(ir.route.step, lookup.step, "the index serves the applied version");
    // Proposals are local state: sharing the import never carries them.
    let (o, key) = (w.p("reshare"), w.p("keys/id_ed25519"));
    w.ok("recipient", &["share", NAME, "--out", o.to_str().unwrap(), "--key", key.to_str().unwrap()]);
    assert!(!o.join(adage_tracks::proposals::FILE).exists());
}

#[test]
fn a_passphrase_protected_key_without_a_terminal_is_refused_with_a_way_forward() {
    let w = World::new();
    let k = keygen(&w);
    let enc = k.encrypt(&mut rand_core::OsRng, "correct horse").unwrap();
    std::fs::write(w.p("keys/id_ed25519"), enc.to_openssh(ssh_key::LineEnding::LF).unwrap().as_bytes()).unwrap();
    io::write(&w.p("sender/tracks").join(NAME), &track("1.0.0", "./scripts/wi-lookup {item}", false)).unwrap();
    let (out, key) = (w.p("out"), w.p("keys/id_ed25519"));
    let o = w.run("sender", &["share", NAME, "--out", out.to_str().unwrap(), "--key", key.to_str().unwrap()]);
    assert!(!o.status.success());
    let t = text(&o);
    assert!(t.contains("passphrase-protected") && t.contains("no terminal") && t.contains("ssh-add"), "{t}");
    assert!(!out.exists(), "nothing was written");
    assert!(!Path::new(&format!("{}.adage", out.display())).exists());
}
