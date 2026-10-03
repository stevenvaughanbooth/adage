//! `adage review`, end to end through the binary, with an isolated `ADAGE_HOME` and `HOME`, and
//! no agent session in the child's environment.

use adage_core::spans::SpanKind;
use adage_core::Effect;
use adage_runtime::SessionState;
use adage_store::{spool, state, Paths};
use adage_tracks::review::{self, Decision, Review, Reviews};
use adage_tracks::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

const REPO: &str = "acme";
const NAME: &str = "acme/work-items";
/// The import in [`populated`] has the learned track's name (a teammate's copy); its version tells them apart.
const IMPORT_VERSION: &str = "9.0.0";

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["data", "home", "work"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        World { _dir: dir, root }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    fn paths(&self) -> Paths {
        Paths::at(self.p("data"))
    }
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        common::scrub(&mut c);
        c.args(args)
            .current_dir(self.p("work"))
            // Text assertions use UTF-8 glyphs regardless of the invoking terminal.
            .env("TERM", "xterm-256color")
            .env("LC_ALL", "C.UTF-8")
            .env("ADAGE_ASCII", "0")
            .env("NO_COLOR", "1")
            .env("ADAGE_HOME", self.p("data"))
            .env("HOME", self.p("home"))
            .env("TERM", "xterm")
            .env("LC_ALL", "en_US.UTF-8")
            .env_remove("ADAGE_ASCII")
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
            .stdin(Stdio::null());
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(o.status.success(), "adage {args:?} failed:\n{}", text(&o));
        text(&o)
    }
    fn queue(&self) -> Value {
        serde_json::from_str(&self.ok(&["review", "--json"])).unwrap()
    }
    fn track_dir(&self, reference: &str) -> PathBuf {
        self.paths().tracks().join(reference)
    }
    fn track(&self, reference: &str) -> Track {
        io::read(&self.track_dir(reference)).unwrap()
    }
    fn index(&self) -> Index {
        Index::load(&self.paths().index())
    }
    /// A session spool with judgements, attributed to `repo` through its state file.
    fn session(&self, sid: &str, repo: &str, arm: &str, judgements: &[(&str, &str)]) {
        let paths = self.paths();
        state::update::<SessionState, _>(&paths.state(sid), |s| s.repo = Some(repo.into())).unwrap();
        let js: Vec<Value> = judgements.iter().map(|(k, r)| json!({ "kind": k, "route": r })).collect();
        spool::append(&paths.spool(sid), &json!({ "ts": 1, "event": "PostToolBatch", "arm": arm, "generation": "g", "decision": { "judgements": js } })).unwrap();
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn route(id: &str, trigger: Trigger, step: Step, effect: Effect, strength: Strength, support: u32, opp: u32) -> Route {
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
            support,
            opportunities: opp,
            days: 2,
            rate: support as f64 / opp as f64,
            lower_bound: wilson_lower(support, opp),
            provenance: "desire-path".into(),
            ..Default::default()
        },
        authored: false,
    }
}

fn lookup(strength: Strength) -> Route {
    route(
        "lookup",
        Trigger::Prompt { span: SpanKind::Ticket },
        Step::Run { command: "./scripts/wi-lookup {item} --json".into() },
        Effect::Observe,
        strength,
        20,
        20,
    )
}

fn verify(id: &str, strength: Strength, support: u32) -> Route {
    route(id, Trigger::SessionStart, Step::Run { command: "make test".into() }, Effect::Verify, strength, support, 12)
}

fn make_track(name: &str, provenance: &str, routes: Vec<Route>) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: name.into(),
                version: "0.1.0".into(),
                description: "test".into(),
                provenance: provenance.into(),
            },
            scope: Scope { repo: REPO.into() },
            gauge: Gauge::default(),
        },
        routes,
    }
}

fn install(w: &World, reference: &str, t: &Track, reviews: &[(&Route, Decision)]) {
    let dir = w.track_dir(reference);
    io::write(&dir, t).unwrap();
    let mut rs = Reviews::default();
    for (r, d) in reviews {
        rs.routes.insert(
            r.id.clone(),
            Review { decision: *d, on: "2026-09-01".into(), by: None, basis: review::basis(r, 0, 0), via: None },
        );
    }
    review::save(&dir, &rs).unwrap();
    let all = io::read_all(&w.paths().tracks()).unwrap();
    Index::compile(&all).write_atomic(&w.paths().index()).unwrap();
}

fn reasons(q: &Value) -> BTreeMap<(String, String), Vec<String>> {
    q["queue"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            let kinds = i["review"]["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["kind"].as_str().unwrap().to_string())
                .collect();
            ((i["reference"].as_str().unwrap().to_string(), i["id"].as_str().unwrap().to_string()), kinds)
        })
        .collect()
}

fn key(t: &str, r: &str) -> (String, String) {
    (t.to_string(), r.to_string())
}

/// A learned track with one route per queue reason (and some that must not be queued), plus an
/// import at forming and runtime judgements.
fn populated() -> World {
    let w = World::new();
    let same = verify("approved-same", Strength::Prime, 10);
    let changed_now = verify("approved-changed", Strength::Prime, 10);
    let changed_then = verify("approved-changed", Strength::Prime, 4);
    let mut kept = verify("rejected-kept", Strength::Prime, 10);
    kept.state = RouteState::Extinguished;
    let mut back = verify("rejected-back", Strength::Prime, 12);
    back.state = RouteState::Forming;
    let back_then = verify("rejected-back", Strength::Prime, 5);
    let ignores = verify("ignores", Strength::Fire, 12);
    let mut pinned = verify("pinned", Strength::Prime, 10);
    pinned.authored = true;
    let t = make_track(
        NAME,
        "desire-path",
        vec![lookup(Strength::Fire), same.clone(), changed_now, kept.clone(), back, ignores.clone(), pinned.clone()],
    );
    install(
        &w,
        NAME,
        &t,
        &[
            (&same, Decision::Approved),
            (&changed_then, Decision::Approved),
            (&kept, Decision::Extinguished),
            (&back_then, Decision::Extinguished),
            (&ignores, Decision::Approved),
            (&pinned, Decision::Pinned),
        ],
    );
    let mut imp = lookup(Strength::Prime);
    imp.state = RouteState::Forming;
    let mut it = make_track(NAME, "imported:desire-path", vec![imp]);
    it.manifest.track.version = IMPORT_VERSION.into();
    install(&w, "@imported/acme/work-items", &it, &[]);
    // Six treated sessions leave `ignores`; control sessions and another repository do not count.
    for i in 0..6 {
        w.session(&format!("t{i}"), REPO, "treated", &[("ignored", "ignores"), ("followed", "lookup")]);
    }
    for i in 0..4 {
        w.session(&format!("h{i}"), REPO, "control", &[("ignored", "approved-same")]);
        w.session(
            &format!("o{i}"),
            "elsewhere",
            "treated",
            &[("ignored", "approved-same"), ("ignored", "approved-same")],
        );
    }
    w
}

#[test]
fn queue_lists_each_reason_with_evidence_and_directive() {
    let w = populated();
    let q = w.queue();
    let got = reasons(&q);
    let expect: BTreeMap<_, Vec<String>> = [
        (key("@imported/acme/work-items", "lookup"), vec!["imported".to_string()]),
        (key(NAME, "approved-changed"), vec!["changed".into()]),
        (key(NAME, "ignores"), vec!["ignoring".into()]),
        (key(NAME, "lookup"), vec!["new".into()]),
        (key(NAME, "rejected-back"), vec!["resurfaced".into()]),
    ]
    .into_iter()
    .collect();
    assert_eq!(got, expect, "{q:#}");
    assert_eq!(q["count"], 5);

    let item = |t: &str, r: &str| {
        q["queue"].as_array().unwrap().iter().find(|i| i["reference"] == t && i["id"] == r).unwrap().clone()
    };
    let lk = item(NAME, "lookup");
    assert_eq!(lk["strength"], "fire");
    assert_eq!(lk["ceiling"], "fire");
    assert_eq!(lk["effect"], "observe");
    assert_eq!(lk["review"]["verbs"], json!(["approve", "extinguish", "weaken", "pin"]));
    assert_eq!(lk["evidence"]["support"], 20);
    assert_eq!(lk["evidence"]["opportunities"], 20);
    assert!(lk["evidence"]["lower_bound"].as_f64().unwrap() > 0.8);
    assert_eq!(lk["live"]["followed"], 6);
    assert_eq!(lk["example_values"]["item"], "W-1234");
    let d = lk["directive"].as_str().unwrap();
    assert!(d.starts_with("<adage-reflex track=\"acme/work-items@0.1.0\" route=\"lookup\" strength=\"fire\">"), "{d}");
    assert!(d.contains("./scripts/wi-lookup W-1234 --json"), "{d}");
    assert!(d.contains("not requested by the user"), "{d}");

    let dr = item(NAME, "ignores");
    assert_eq!(dr["live"]["ignored"], 6, "control and other repositories are not counted");
    assert!(dr["review"]["reasons"][0]["why"].as_str().unwrap().contains("weakening"));
    assert!(item(NAME, "approved-changed")["review"]["reasons"][0]["why"]
        .as_str()
        .unwrap()
        .starts_with("approved on 2026-09-01; since then lower bound moved"));
    assert_eq!(item("@imported/acme/work-items", "lookup")["imported"], true);

    let t = w.ok(&["review"]);
    for s in [
        "5 to look over",
        "why         new:",
        "seen in 20 of 20 traces",
        "example values: item = W-1234",
        "adage review <approve|extinguish|weaken|pin> acme/work-items lookup",
        " · imported",
        "adage review --verbose",
    ] {
        assert!(t.contains(s), "listing lacks {s:?}:\n{t}");
    }
    // The statistics are for `--verbose` (QA first-run #13): plain words by default.
    assert!(!t.contains("Wilson lower bound") && !t.contains("backoff depth"), "{t}");
    let v = w.ok(&["review", "--verbose"]);
    assert!(v.contains("Wilson lower bound"), "{v}");
    // Deterministic: the same inputs give the same bytes.
    assert_eq!(w.ok(&["review", "--json"]), w.ok(&["review", "--json"]));
    assert_eq!(t, w.ok(&["review"]));
}

#[test]
fn decisions_change_track_files_and_index() {
    let w = populated();
    let dir = w.track_dir(NAME);
    let lookup_in_index = |w: &World| {
        w.index()
            .routes
            .iter()
            .find(|r| r.track == NAME && r.version != IMPORT_VERSION && r.route.id == "lookup")
            .map(|r| r.route.clone())
    };

    // Approve: recorded, nothing else changes, and it leaves the queue.
    let before = std::fs::read(dir.join("routes/lookup.yaml")).unwrap();
    w.ok(&["review", "approve", NAME, "lookup", "--by", "steven"]);
    assert_eq!(std::fs::read(dir.join("routes/lookup.yaml")).unwrap(), before);
    let rs = review::load(&dir).unwrap();
    assert_eq!(rs.routes["lookup"].decision, Decision::Approved);
    assert_eq!(rs.routes["lookup"].by.as_deref(), Some("steven"));
    assert_eq!(rs.routes["lookup"].on.len(), 10);
    assert_eq!(w.track(NAME).manifest.track.version, "0.1.0", "approval alone changes no behaviour");
    assert!(!reasons(&w.queue()).contains_key(&key(NAME, "lookup")));

    // Weaken: fire → prime in the file and the index; a major bump.
    w.ok(&["review", "weaken", NAME, "lookup"]);
    let t = w.track(NAME);
    assert_eq!(t.routes.iter().find(|r| r.id == "lookup").unwrap().strength, Strength::Prime);
    assert_eq!(t.manifest.track.version, "1.0.0");
    assert_eq!(lookup_in_index(&w).unwrap().strength, Strength::Prime);
    let rs = review::load(&dir).unwrap();
    assert_eq!(rs.routes["lookup"].decision, Decision::Weakened);
    assert_eq!(rs.routes["lookup"].by, None, "no name unless the person gives one");
    assert_eq!(rs.routes["lookup"].basis.followed, 6, "judgements so far are the new baseline");
    let o = w.run(&["review", "weaken", NAME, "lookup"]);
    assert!(!o.status.success() && text(&o).contains("only a fire reflex"), "{}", text(&o));

    // Strengthen: back to fire; a minor bump.
    w.ok(&["review", "strengthen", NAME, "lookup"]);
    assert_eq!(lookup_in_index(&w).unwrap().strength, Strength::Fire);
    assert_eq!(w.track(NAME).manifest.track.version, "1.1.0");

    // Reject: extinguished, kept in the file, gone from the index.
    w.ok(&["review", "extinguish", NAME, "lookup"]);
    assert_eq!(w.track(NAME).routes.iter().find(|r| r.id == "lookup").unwrap().state, RouteState::Extinguished);
    assert!(lookup_in_index(&w).is_none());
    // A teammate's import of the same route does not slip back in behind the rejection.
    assert!(!w.index().routes.iter().any(|r| r.route.id == "lookup"));

    // Approve an extinguished route: back in service at forming, never fire.
    w.ok(&["review", "approve", NAME, "lookup"]);
    let r = lookup_in_index(&w).unwrap();
    assert_eq!((r.state, r.strength), (RouteState::Forming, Strength::Prime));

    // Pin: edit-locked.
    w.ok(&["review", "pin", NAME, "lookup"]);
    assert!(w.track(NAME).routes.iter().find(|r| r.id == "lookup").unwrap().authored);
    assert_eq!(review::load(&dir).unwrap().routes["lookup"].decision, Decision::Pinned);

    // The index on disk is exactly what compiling the tracks gives.
    let all = io::read_all(&w.paths().tracks()).unwrap();
    assert_eq!(serde_json::to_string(&Index::compile(&all)).unwrap(), serde_json::to_string(&w.index()).unwrap());

    // Unknown routes, tracks, and verbs are errors, and --by is checked.
    assert!(!w.run(&["review", "approve", NAME, "nope"]).status.success());
    assert!(!w.run(&["review", "approve", "nope/nope", "lookup"]).status.success());
    assert!(!w.run(&["review", "bless", NAME, "lookup"]).status.success());
    assert!(!w.run(&["review", "approve", NAME, "lookup", "--by", &"x".repeat(65)]).status.success());
}

#[test]
fn strengthening_never_exceeds_the_effect_ceiling() {
    let w = World::new();
    let gen = route(
        "gen-at-start",
        Trigger::SessionStart,
        Step::Run { command: "make gen".into() },
        Effect::Regenerate,
        Strength::Prime,
        10,
        10,
    );
    let gen_edit = route(
        "gen-after-edit",
        Trigger::AfterEdit { paths: "schema/*.json".into() },
        Step::Run { command: "make gen".into() },
        Effect::Regenerate,
        Strength::Prime,
        10,
        10,
    );
    let env = route(
        "env",
        Trigger::SessionStart,
        Step::Run { command: "make install".into() },
        Effect::Environment,
        Strength::Prime,
        10,
        10,
    );
    let inhibit = route(
        "dead",
        Trigger::SessionStart,
        Step::Avoid { avoid: "pytest".into(), instead: "make test".into(), signature: "ACME_ENV is not set".into() },
        Effect::Verify,
        Strength::Inhibit,
        10,
        10,
    );
    install(&w, NAME, &make_track(NAME, "desire-path", vec![gen, gen_edit, env, inhibit]), &[]);
    let files = |w: &World| {
        let d = w.track_dir(NAME);
        (
            std::fs::read(d.join("routes/gen-at-start.yaml")).unwrap(),
            std::fs::read(d.join("track.toml")).unwrap(),
            std::fs::read(w.paths().index()).unwrap(),
        )
    };
    let untouched = files(&w);
    for (id, why) in
        [("gen-at-start", "ceiling is prime"), ("env", "ceiling is prime"), ("dead", "only a prime reflex")]
    {
        let o = w.run(&["review", "strengthen", NAME, id]);
        assert!(!o.status.success(), "{id} must not be strengthened");
        assert!(text(&o).contains(why), "{id}: {}", text(&o));
    }
    assert_eq!(files(&w), untouched, "a refused decision writes nothing");
    assert!(!w.track_dir(NAME).join(review::FILE).exists());
    // A regenerate step triggered by an edit may be fire.
    w.ok(&["review", "strengthen", NAME, "gen-after-edit"]);
    assert_eq!(w.track(NAME).routes.iter().find(|r| r.id == "gen-after-edit").unwrap().strength, Strength::Fire);

    // Design O2: an import cannot be fire without local first-hand evidence.
    let imp_ref = "@imported/acme/imported";
    let mut imp = lookup(Strength::Prime);
    imp.state = RouteState::Forming;
    install(&w, imp_ref, &make_track("acme/imported", "imported:desire-path", vec![imp]), &[]);
    let o = w.run(&["review", "strengthen", imp_ref, "lookup"]);
    assert!(!o.status.success() && text(&o).contains("first-hand"), "{}", text(&o));
    // Weak local evidence is not enough.
    let mut weak = lookup(Strength::Prime);
    weak.evidence.support = 3;
    weak.evidence.opportunities = 6;
    weak.evidence.lower_bound = wilson_lower(3, 6);
    install(&w, "acme/local", &make_track("acme/local", "desire-path", vec![weak]), &[]);
    assert!(!w.run(&["review", "strengthen", imp_ref, "lookup"]).status.success());
    // Learned here with strong evidence: allowed, and the import keeps its author's version.
    install(&w, "acme/local", &make_track("acme/local", "desire-path", vec![lookup(Strength::Fire)]), &[]);
    w.ok(&["review", "strengthen", imp_ref, "lookup"]);
    let t = w.track(imp_ref);
    assert_eq!(t.routes[0].strength, Strength::Fire);
    assert_eq!(t.manifest.track.version, "0.1.0");
}

#[test]
fn agents_can_list_but_not_decide() {
    let w = populated();
    let snapshot = |w: &World| {
        let mut v = Vec::new();
        for e in walk(&w.p("data/tracks")) {
            v.push((e.clone(), std::fs::read(w.p("data/tracks").join(&e)).unwrap()));
        }
        v.sort();
        (v, std::fs::read(w.paths().index()).unwrap())
    };
    let before = snapshot(&w);
    let o = w.cmd(&["review", "--json"]).env("CLAUDECODE", "1").output().unwrap();
    assert!(o.status.success(), "listing is fine for agents: {}", text(&o));
    for verb in ["approve", "extinguish", "weaken", "strengthen", "pin"] {
        let o = w.cmd(&["review", verb, NAME, "lookup"]).env("CLAUDECODE", "1").output().unwrap();
        assert!(!o.status.success(), "{verb} ran inside an agent session");
        assert!(text(&o).contains("agent session"), "{verb}: {}", text(&o));
    }
    let o = w.cmd(&["review", "approve", NAME, "lookup"]).env_remove("ADAGE_ALLOW_NONINTERACTIVE").output().unwrap();
    assert!(!o.status.success() && text(&o).contains("not a terminal"), "{}", text(&o));
    assert_eq!(snapshot(&w), before, "nothing was written");
}

// ---- Re-learning, over synthetic transcripts in Claude Code's on-disk format ----

struct Tx {
    lines: Vec<Value>,
    sid: String,
    cwd: String,
    n: usize,
}

impl Tx {
    fn new(sid: &str, cwd: &str, prompt: &str) -> Tx {
        let mut t = Tx { lines: vec![], sid: sid.into(), cwd: cwd.into(), n: 0 };
        t.lines.push(json!({"type":"user","sessionId":sid,"cwd":cwd,"timestamp":"2026-09-20T10:00:00Z","message":{"role":"user","content":prompt}}));
        t
    }
    fn call(&mut self, name: &str, input: Value, output: &str, is_error: bool) -> &mut Self {
        self.n += 1;
        let id = format!("toolu_{}", self.n);
        self.lines.push(json!({"type":"assistant","sessionId":self.sid,"cwd":self.cwd,"message":{"id":format!("msg_{}", self.n),"model":"claude-test","content":[{"type":"tool_use","id":id,"name":name,"input":input}]}}));
        self.lines.push(json!({"type":"user","sessionId":self.sid,"cwd":self.cwd,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":output,"is_error":is_error}]}}));
        self
    }
    fn bash(&mut self, cmd: &str, out: &str, err: bool) -> &mut Self {
        self.call("Bash", json!({"command": cmd}), out, err)
    }
    fn write(&self, dir: &Path) -> PathBuf {
        let p = dir.join(format!("{}.jsonl", self.sid));
        std::fs::write(&p, self.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        p
    }
}

/// adage-learn's golden corpus: a lookup, linked files, a finish-line step, and a dead end.
fn corpus(dir: &Path, n: usize) -> Vec<PathBuf> {
    let cwd = "/fixture/acme-billing";
    (0..n)
        .map(|i| {
            let id = format!("W-10{:02}", 40 + i);
            let mut t = Tx::new(&format!("s{i:02}"), cwd, &format!("Pick up {id} and get it done."));
            t.bash("ls", "README.md\nMakefile\nacme\nscripts", false)
                .bash(
                    &format!("python3 scripts/wi-lookup {id}"),
                    &format!("ID: {id}\nLinked files: acme/billing.py\nAcceptance: ok"),
                    false,
                )
                .call("Read", json!({"file_path": format!("{cwd}/acme/billing.py")}), "code", false);
            if i % 3 != 2 {
                t.call("Edit", json!({"file_path": format!("{cwd}/schema/invoice.json")}), "ok", false).bash(
                    "make gen",
                    "generated",
                    false,
                );
            } else {
                t.call("Edit", json!({"file_path": format!("{cwd}/acme/billing.py")}), "ok", false);
            }
            let runner =
                ["python3 -m unittest discover -s tests", "python3 -m pytest tests", "python -m pytest -v"][i % 3];
            t.bash(runner, "Traceback\nRuntimeError: ACME_ENV is not set — run tests with 'make test'", true).bash(
                "make test",
                "OK",
                false,
            );
            t.write(dir)
        })
        .collect()
}

fn learn(w: &World, files: &[PathBuf]) {
    let mut args = vec!["condition", "--repo", "acme-billing", "--demo", "--transcripts"];
    args.extend(files.iter().map(|p| p.to_str().unwrap()));
    w.ok(&args);
}

#[test]
fn decisions_survive_relearning() {
    let w = World::new();
    std::fs::create_dir_all(w.p("tx")).unwrap();
    let six = corpus(&w.p("tx"), 6);
    learn(&w, &six);
    // One learned track: every reflex of the repository.
    let dead_ends = "acme-billing/learned";
    let work = "acme-billing/learned";
    let dead = w.track(dead_ends).routes.iter().find(|r| r.id.starts_with("dead-end-")).unwrap().clone();
    assert_eq!(dead.strength, Strength::Inhibit);
    let strength =
        |w: &World, t: &str, id: &str| w.track(t).routes.iter().find(|r| r.id == id).map(|r| (r.strength, r.state));
    assert_eq!(strength(&w, work, "lookup-work-item").unwrap().0, Strength::Fire);
    assert_eq!(strength(&w, work, "read-linked-files").unwrap().0, Strength::Prime);
    let q = reasons(&w.queue());
    assert_eq!(q.values().filter(|k| *k == &vec!["new".to_string()]).count(), 4, "every learned route is new: {q:?}");
    let mut untouched = q;
    for id in [&dead.id, "lookup-work-item", "read-linked-files"] {
        assert!(untouched.remove(&key(work, id)).is_some());
    }

    w.ok(&["review", "extinguish", dead_ends, &dead.id]);
    w.ok(&["review", "weaken", work, "lookup-work-item"]);
    w.ok(&["review", "strengthen", work, "read-linked-files"]);
    let versions = |w: &World| w.track(work).manifest.track.version;
    let decided = versions(&w);

    // Re-learning the same sessions changes nothing a person decided, and bumps nothing.
    learn(&w, &six);
    assert_eq!(strength(&w, dead_ends, &dead.id).unwrap().1, RouteState::Extinguished);
    assert!(!w.index().routes.iter().any(|r| r.route.id == dead.id), "a rejected route is never delivered");
    assert_eq!(strength(&w, work, "lookup-work-item").unwrap().0, Strength::Prime, "a weakening is not reverted");
    assert_eq!(strength(&w, work, "read-linked-files").unwrap().0, Strength::Fire, "a strengthening is not reverted");
    assert_eq!(versions(&w), decided);
    let q = reasons(&w.queue());
    assert_eq!(q, untouched, "untouched routes, including execution candidates, keep their review state");
    // Deterministic: another re-learn gives identical bytes.
    let index = std::fs::read(w.paths().index()).unwrap();
    let review_file = std::fs::read(w.track_dir(work).join(review::FILE)).unwrap();
    learn(&w, &six);
    assert_eq!(std::fs::read(w.paths().index()).unwrap(), index);
    assert_eq!(std::fs::read(w.track_dir(work).join(review::FILE)).unwrap(), review_file);

    // Twice the sessions: the rejected dead end has strengthened materially and comes back at
    // forming; the strengthening is released to what the evidence says; both wait in the queue.
    let twelve = corpus(&w.p("tx"), 12);
    learn(&w, &twelve);
    assert_eq!(strength(&w, dead_ends, &dead.id).unwrap(), (Strength::Inhibit, RouteState::Forming));
    assert!(w.index().routes.iter().any(|r| r.route.id == dead.id));
    assert_eq!(
        strength(&w, work, "lookup-work-item").unwrap().0,
        Strength::Prime,
        "a weakening holds even as evidence grows"
    );
    assert_eq!(strength(&w, work, "read-linked-files").unwrap().0, Strength::Prime);
    let q = reasons(&w.queue());
    assert_eq!(q[&key(dead_ends, &dead.id)], vec!["resurfaced".to_string()]);
    assert_eq!(q[&key(work, "lookup-work-item")], vec!["changed".to_string()]);
    assert_eq!(q[&key(work, "read-linked-files")], vec!["changed".to_string()]);

    // Rejected again, it stays rejected while the evidence holds.
    w.ok(&["review", "extinguish", dead_ends, &dead.id]);
    learn(&w, &twelve);
    assert_eq!(strength(&w, dead_ends, &dead.id).unwrap().1, RouteState::Extinguished);
}

/// Design O11: an environment step is delivered only on a route a person approved. `acme-tracker`
/// is not in the command table, so it is classified as environment (as unknown commands are).
#[test]
fn an_environment_step_is_delivered_only_after_approval() {
    let w = World::new();
    std::fs::create_dir_all(w.p("tx")).unwrap();
    let files: Vec<PathBuf> = (0..6)
        .map(|i| {
            let id = format!("W-10{:02}", 40 + i);
            let mut t = Tx::new(&format!("e{i:02}"), "/fixture/acme-billing", &format!("Pick up {id}."));
            t.bash(&format!("acme-tracker fetch {id}"), "ok", false);
            t.write(&w.p("tx"))
        })
        .collect();
    learn(&w, &files);
    let work = "acme-billing/learned";
    let lookup = || w.track(work).routes.into_iter().find(|r| r.id == "lookup-work-item").unwrap();
    let delivered = || w.index().routes.iter().any(|r| r.route.id == "lookup-work-item");
    let l = lookup();
    assert_eq!((l.effect, l.strength, l.state), (Effect::Environment, Strength::Prime, RouteState::Nascent));
    assert!(!delivered(), "a nascent is never delivered");
    let q = w.queue();
    assert_eq!(reasons(&q)[&key(work, "lookup-work-item")], vec!["approval".to_string()], "{q:#}");

    w.ok(&["review", "approve", work, "lookup-work-item"]);
    assert_eq!((lookup().strength, lookup().state), (Strength::Prime, RouteState::Firm));
    assert!(delivered(), "approved, it is delivered (prime, its ceiling)");
    assert!(!reasons(&w.queue()).contains_key(&key(work, "lookup-work-item")));

    // Re-learning keeps the approval while the step is the one approved.
    learn(&w, &files);
    assert_eq!(lookup().state, RouteState::Firm);
    assert!(delivered());

    // A different command is a different step: it waits for approval again.
    let other: Vec<PathBuf> = (0..6)
        .map(|i| {
            let id = format!("W-10{:02}", 40 + i);
            let mut t = Tx::new(&format!("e{i:02}"), "/fixture/acme-billing", &format!("Pick up {id}."));
            t.bash(&format!("acme-tracker fetch --full {id}"), "ok", false);
            t.write(&w.p("tx"))
        })
        .collect();
    learn(&w, &other);
    assert_eq!(lookup().state, RouteState::Nascent);
    assert!(!delivered());
    assert_eq!(reasons(&w.queue())[&key(work, "lookup-work-item")], vec!["changed".to_string()]);
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                out.push(e.path().strip_prefix(dir).unwrap().to_string_lossy().to_string());
            }
        }
    }
    out
}

#[test]
fn routine_candidate_stays_in_execution_review_after_evidence_approval() {
    let w = World::new();
    assert!(Command::new("git").args(["init", "-q"]).current_dir(w.p("work")).status().unwrap().success());
    let mut r = route(
        "routine",
        Trigger::AfterEdit { paths: "src/*.rs".into() },
        Step::Run { command: "cargo test".into() },
        Effect::Verify,
        Strength::Prime,
        5,
        5,
    );
    r.evidence.execution = Some(ExecutionEvidence { opportunities: 5, successful: 5, sessions: 5 });
    let mut t = make_track(NAME, "desire-path", vec![r]);
    t.manifest.scope.repo = adage_core::repo::repo_key(&w.p("work"));
    install(&w, NAME, &t, &[]);
    let first = w.queue();
    let row = &first["queue"][0];
    assert_eq!(row["execution_only"], true);
    assert_eq!(row["evidence"]["execution"]["successful"], 5);
    assert!(row["directive"].is_null());
    assert!(row["review"]["next"].as_str().unwrap().ends_with("routine --level e2"));
    // What it does, truthfully (QA first-run #6): silent without a grant, nothing at Stop; no
    // finish line is promised for a reflex never said to the agent.
    let shown = w.ok(&["review"]);
    assert!(
        shown.contains("no grant    silent: the agent is told nothing, nothing runs, and nothing happens at Stop"),
        "{shown}"
    );
    assert!(shown.contains("with grant  after an edit to src/*.rs, Adage runs `cargo test` itself"), "{shown}");
    assert!(!shown.contains("enforced") && !shown.contains("asks it once"), "{shown}");
    assert!(shown.contains("next  adage review approve"), "{shown}");
    let approved = w.ok(&["review", "approve", NAME, "routine"]);
    assert!(approved.contains("execution still requires a valid local E2 grant"), "{approved}");
    assert!(approved.contains("next  adage status"), "approving dequeues it (QA first-run #3): {approved}");
    let again = w.ok(&["review", "approve", NAME, "routine"]);
    assert!(again.contains("already approved; nothing changed"), "{again}");
    // Approved, it no longer waits; it is listed as ready to delegate until it is.
    let q = w.queue();
    assert_eq!(reasons(&q)[&key(NAME, "routine")], vec!["delegable"]);
    assert_eq!((q["waiting"].as_u64(), q["queue"][0]["waits"].as_bool()), (Some(0), Some(false)), "{q:#}");
    let shown = w.ok(&["review"]);
    assert!(
        shown.contains("nothing waits for a decision") && shown.contains("Approved, and run only once delegated"),
        "{shown}"
    );
    assert!(shown.contains("Execution only") && shown.contains("5 / 5"), "{shown}");
    assert!(shown.contains("next  adage review delegate"), "{shown}");
    assert!(!shown.contains("\n  directive"), "{shown}");

    // A fixture grant is valid only for this exact derived request; no command is executed.
    let index = w.index();
    let ir = &index.routes[0];
    let terms = adage_exec::route::Terms::default();
    let plan = adage_exec::route::plan(
        ir,
        &index,
        &w.p("work"),
        &w.paths().tracks(),
        adage_exec::Level::E2,
        Some(&terms),
        &adage_core::CommandTable::load(),
    )
    .unwrap();
    let mut store = adage_exec::approvals::Store::default();
    store.approvals.push(adage_exec::approvals::Approval {
        key: adage_exec::approvals::ApprovalKey::for_request(&plan.request, &ir.repo),
        route: format!("{NAME}/routine"),
        approved_on: "2026-09-29".into(),
        by: None,
        basis: json!({}),
        auto: false,
        terms: Some(terms),
        via: None,
    });
    store.save(&adage_exec::approvals::path(&w.paths().root)).unwrap();
    assert!(!reasons(&w.queue()).contains_key(&key(NAME, "routine")));
    let mut changed = w.track(NAME);
    changed.routes[0].step = Step::Run { command: "cargo test --locked".into() };
    io::write(&w.track_dir(NAME), &changed).unwrap();
    assert!(
        reasons(&w.queue())[&key(NAME, "routine")].contains(&"execution".into()),
        "stale execution approval must return to review"
    );
}
