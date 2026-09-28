//! Hostile packs are refused, each with a specific reason.

mod common;

use adage_pack::digest::sha256_hex;
use adage_pack::Pack;
use std::collections::BTreeMap;

type Files = BTreeMap<String, Vec<u8>>;

/// Apply `f` to the sample files, re-derive `CLOSURE` if `reclose` (so the refusal is not merely a
/// stale listing), and expect a refusal mentioning `why`.
fn refused_with(f: impl FnOnce(&mut Files), reclose: bool, why: &str) {
    let mut files = common::files_without_closure();
    f(&mut files);
    let files = if reclose {
        // A hostile author derives CLOSURE with the real algorithm; if derivation itself refuses,
        // write a stale one so loading still has to refuse.
        let parsed = adage_pack::validate::parse(&files);
        let closure = parsed
            .ok()
            .filter(|p| adage_pack::validate::validate(p).is_ok())
            .and_then(|p| adage_pack::closure::compute(&p).ok())
            .map(|c| c.render())
            .unwrap_or_default();
        let mut files = files;
        files.insert("CLOSURE".into(), closure.into_bytes());
        files
    } else {
        files
    };
    let err = match Pack::from_files(&files) {
        Ok(_) => panic!("accepted a hostile pack (expected: {why})"),
        Err(e) => format!("{e:#}"),
    };
    assert!(err.contains(why), "expected {why:?} in: {err}");
}

fn edit(files: &mut Files, rel: &str, from: &str, to: &str) {
    let s = String::from_utf8(files[rel].clone()).unwrap();
    assert!(s.contains(from), "{rel} has no {from:?}");
    files.insert(rel.into(), s.replacen(from, to, 1).into_bytes());
}

fn put(files: &mut Files, rel: &str, body: &str) {
    files.insert(rel.into(), body.as_bytes().to_vec());
}

const WIC: &str = "routes/work-item-context.yaml";

#[test]
fn the_sample_is_accepted() {
    Pack::from_files(&common::files()).unwrap();
}

#[test]
fn dangling_script_references_are_refused() {
    // argv names a repository script the step does not declare.
    refused_with(
        |f| {
            put(
                f,
                "routes/other.yaml",
                "id: other\nshape: run\ntrigger: { on: session_start }\nsteps:\n  - id: s\n    run: { argv: [\"./scripts/other\"] }\n    effect: observe\naspect: yellow\n",
            )
        },
        true,
        "without a script entry (dangling reference)",
    );
    // The same through an interpreter.
    refused_with(
        |f| {
            put(
                f,
                "routes/other.yaml",
                "id: other\nshape: run\ntrigger: { on: session_start }\nsteps:\n  - id: s\n    run: { argv: [python3, tools/x.py] }\n    effect: observe\naspect: yellow\n",
            )
        },
        true,
        "dangling reference",
    );
    // An L-ref script with no requires.repo_files entry.
    refused_with(
        |f| {
            edit(
                f,
                "pack.toml",
                "repo_files = [{ path = \"scripts/wi-lookup\"",
                "repo_files = [{ path = \"scripts/wi-lookup2\"",
            )
        },
        true,
        "has no requires.repo_files entry",
    );
    // A vendored script that is not in the pack.
    refused_with(
        |f| {
            f.retain(|k, _| !k.starts_with("scripts/"));
        },
        true,
        "is not in the pack (dangling reference)",
    );
    // A program not in the command table and no script.
    refused_with(
        |f| edit(f, "routes/finish-line-tests.yaml", "argv: [\"make\", \"test\"]", "argv: [\"frob\", \"test\"]"),
        true,
        "not in the command table",
    );
    // A type nobody defines.
    refused_with(|f| edit(f, WIC, "type: wi-state", "type: wi-status"), true, "dangling");
    // A read of an output that does not exist.
    refused_with(|f| edit(f, WIC, "from: lookup.files", "from: lookup.paths"), true, "names no output or input");
}

#[test]
fn smuggled_and_unreferenced_bytes_are_refused() {
    refused_with(|f| put(f, "notes.txt", "hello"), true, "not part of the pack format");
    refused_with(|f| put(f, "routes/x.sh", "echo"), true, "not part of the pack format");
    refused_with(|f| put(f, ".hidden", "x"), true, "not part of the pack format");
    refused_with(|f| put(f, "review.yaml", "x"), true, "not part of the pack format");
    let extra = "print('extra')\n";
    refused_with(
        |f| put(f, &format!("scripts/sha256-{}", sha256_hex(extra.as_bytes())), extra),
        true,
        "smuggled bytes",
    );
    refused_with(|f| put(f, "scripts/sha256-00", "x"), true, "not part of the pack format");
    // A script whose name is not its hash.
    refused_with(
        |f| {
            let k = f.keys().find(|k| k.starts_with("scripts/")).unwrap().clone();
            f.insert(k, b"print('changed')\n".to_vec());
        },
        true,
        "is not named by its SHA-256",
    );
    refused_with(|f| put(f, "types/unused.yaml", "id: unused\nbase: enum\nvalues: [a]\n"), true, "unreferenced bytes");
    refused_with(
        |f| edit(f, "pack.toml", "{ name = \"make\" }", "{ name = \"make\" }, { name = \"jq\" }"),
        true,
        "tool:jq is used by no route",
    );
    // A route that is not an entry and that nothing links to.
    refused_with(
        |f| edit(f, "routes/count-items.yaml", "shape: run\n", "shape: run\nentry: false\n"),
        true,
        "no link reaches it",
    );
    // Reached through the dead end's `instead` edge, a non-entry route is fine.
    let mut files = common::files_without_closure();
    edit(&mut files, "routes/finish-line-tests.yaml", "shape: obligation\n", "shape: obligation\nentry: false\n");
    Pack::from_files(&common::with_closure(files)).unwrap();
}

#[test]
fn a_stale_or_forged_closure_is_refused() {
    let mut files = common::files();
    let closure = String::from_utf8(files["CLOSURE"].clone()).unwrap();
    let forged: String = closure.lines().filter(|l| !l.starts_with("edge:after")).map(|l| format!("{l}\n")).collect();
    files.insert("CLOSURE".into(), forged.into_bytes());
    let e = format!("{:#}", Pack::from_files(&files).unwrap_err());
    assert!(e.contains("CLOSURE does not match") && e.contains("derived, not declared: edge:after"), "{e}");
    refused_with(|_| {}, false, "has no CLOSURE");
    // Changing a route without re-deriving the closure changes its exec digest line.
    let mut files = common::files();
    edit(&mut files, WIC, "\"--json\"", "\"--full\"");
    assert!(format!("{:#}", Pack::from_files(&files).unwrap_err()).contains("CLOSURE does not match"));
}

#[test]
fn oversize_packs_are_refused() {
    refused_with(
        |f| put(f, "attestations/big.json", &format!("{{\"a\":\"{}\"}}", "x".repeat(1 << 20))),
        true,
        "larger than 1048576",
    );
    let big = format!("# {}\n", "x".repeat(70 << 10));
    refused_with(|f| put(f, &format!("scripts/sha256-{}", sha256_hex(big.as_bytes())), &big), true, "the script limit");
    refused_with(
        |f| {
            for i in 0..260 {
                put(f, &format!("attestations/a{i}.json"), "{}");
            }
        },
        true,
        "at most 256 are allowed",
    );
}

#[test]
fn lax_contracts_are_refused() {
    for lax in [
        "^.*$",
        "W-\\\\d{4}",
        "^[A-Z]+-\\\\d+$",
        "^\\\\S{1,20}$",
        "^a$|^.{1,9}",
        "^[a-z\\\\-]{1,20}$",
        "^[a-z]{1,300}$",
    ] {
        refused_with(
            |f| put(f, "types/work-item-id.yaml", &format!("id: work-item-id\nbase: string\npattern: \"{lax}\"\n")),
            true,
            "is not a strict contract",
        );
    }
    // Fields a base does not take.
    refused_with(
        |f| put(f, "types/wi-state.yaml", "id: wi-state\nbase: enum\nvalues: [Active, Closed]\npattern: \"^a$\"\n"),
        true,
        "only `values`",
    );
    refused_with(
        |f| put(f, "types/wi-state.yaml", "id: wi-state\nbase: enum\nvalues: [\"a b\"]\n"),
        true,
        "not a plain token",
    );
    refused_with(
        |f| put(f, "types/wi-state.yaml", "id: repo-path\nbase: enum\nvalues: [a]\n"),
        true,
        "named by its id",
    );
    // Exit values must pass their field's type.
    refused_with(|f| edit(f, WIC, "value: Closed", "value: Open"), true, "does not pass the type");
}

#[test]
fn argv_injection_is_refused() {
    for bad in ["{item};rm", "$(id)", "a b", "`id`", "--x=$(id)", "{item}|sh", "{ITEM}", "~/.ssh/id_rsa"] {
        refused_with(|f| edit(f, WIC, "\"--json\"", &format!("\"{bad}\"")), true, "not a plain token");
    }
    refused_with(|f| edit(f, WIC, "\"--json\"", "\"{other}\""), true, "undeclared input {other}");
    refused_with(
        |f| {
            edit(
                f,
                "routes/finish-line-tests.yaml",
                "argv: [\"make\", \"test\"]",
                "argv: [\"python3\", \"-c\", \"print\"]",
            )
        },
        true,
        "inline code",
    );
    refused_with(
        |f| edit(f, "routes/finish-line-tests.yaml", "argv: [\"make\", \"test\"]", "argv: [\"{prog}\", \"test\"]"),
        true,
        "placeholder",
    );
    // A declared effect lower than the local classification.
    refused_with(
        |f| edit(f, "routes/finish-line-tests.yaml", "argv: [\"make\", \"test\"]", "argv: [\"rm\", \"test\"]"),
        true,
        "but the step declares",
    );
    // A vendored script run directly (its shebang would choose the interpreter).
    refused_with(
        |f| {
            let v = common::vendored_sha();
            edit(
                f,
                "routes/count-items.yaml",
                &format!("argv: [\"python3\", \"scripts/sha256-{v}\"]"),
                &format!("argv: [\"./scripts/sha256-{v}\"]"),
            )
        },
        true,
        "run by an interpreter named in argv",
    );
}

#[test]
fn absolute_and_escaping_paths_are_refused() {
    refused_with(|f| edit(f, WIC, "\"./scripts/wi-lookup\"", "\"/usr/bin/wi-lookup\""), true, "not a plain token");
    refused_with(|f| edit(f, WIC, "\"--json\"", "\"--out=/etc/passwd\""), true, "not a plain token");
    refused_with(|f| edit(f, WIC, "\"--json\"", "\"../../x\""), true, "not a plain token");
    refused_with(
        |f| edit(f, "routes/finish-line-tests.yaml", "src/*.py", "/etc/*"),
        true,
        "not a repository-relative glob",
    );
    refused_with(
        |f| edit(f, "routes/finish-line-tests.yaml", "src/*.py", "../*.py"),
        true,
        "not a repository-relative glob",
    );
    refused_with(
        |f| {
            edit(
                f,
                "pack.toml",
                "repo_files = [{ path = \"scripts/wi-lookup\"",
                "repo_files = [{ path = \"/etc/wi-lookup\"",
            )
        },
        true,
        "not a repository-relative path",
    );
    refused_with(|f| edit(f, "pack.toml", "\"payments@1a2b3c4d5e6f\"", "\"/home/me/payments\""), true, "repo key");
}

#[test]
fn invisible_and_control_characters_are_refused() {
    refused_with(
        |f| edit(f, WIC, "Look up the work item", "Look up the \u{202E}work item"),
        true,
        "invisible or control character U+202E",
    );
    refused_with(|f| edit(f, WIC, "Look up the work item", "Look up the\u{200B} work item"), true, "U+200B");
    refused_with(|f| edit(f, "pack.toml", "Work item context", "Work\u{7} item context"), true, "U+0007");
    refused_with(
        |f| {
            let k = f.keys().find(|k| k.starts_with("scripts/")).unwrap().clone();
            let body = "print('a')\u{2066}\n";
            f.remove(&k);
            let v = sha256_hex(body.as_bytes());
            put(f, &format!("scripts/sha256-{v}"), body);
        },
        true,
        "U+2066",
    );
    refused_with(
        |f| f.insert("routes/finish-line-tests.yaml".into(), vec![0xff, 0xfe]).map(|_| ()).unwrap_or(()),
        true,
        "not UTF-8",
    );
    refused_with(|f| edit(f, WIC, "Look up", "Look\0 up"), true, "NUL");
}

#[test]
fn unknown_fields_are_refused() {
    refused_with(|f| edit(f, WIC, "aspect: green\n", "aspect: green\nstate: active\n"), true, "unknown field `state`");
    refused_with(
        |f| edit(f, "pack.toml", "requested = \"E1\"", "requested = \"E1\"\ngranted = \"E3\""),
        true,
        "unknown field `granted`",
    );
    refused_with(|f| edit(f, WIC, "closure: ref,", "closure: ref, trusted: true,"), true, "unknown field `trusted`");
    refused_with(
        |f| edit(f, "pack.toml", "format = \"adage-pack/1\"", "format = \"adage-pack/2\""),
        true,
        "is not \"adage-pack/1\"",
    );
    refused_with(
        |f| edit(f, "pack.toml", "kind = \"repo\"", "kind = \"everywhere\""),
        true,
        "pack.toml is not a valid manifest",
    );
    refused_with(
        |f| edit(f, "evidence.json", "\"organic\":9", "\"organic\":9,\"approved\":1"),
        true,
        "unknown field `approved`",
    );
}

#[test]
fn after_cycles_and_long_chains_are_refused() {
    let after = |id: &str, on: &str, run: &str| {
        format!("id: {id}\nshape: after\ntrigger: {{ on: after_command, command: \"exec make {on}\" }}\nsteps:\n  - id: s\n    run: {{ argv: [make, {run}] }}\n    effect: environment\naspect: yellow\n")
    };
    refused_with(
        |f| {
            put(f, "routes/ping.yaml", &after("ping", "pong", "ping"));
            put(f, "routes/pong.yaml", &after("pong", "ping", "pong"));
            edit(f, "pack.toml", "\"avoid@1\"", "\"after@1\", \"avoid@1\"");
        },
        true,
        "after chain cycle",
    );
    refused_with(
        |f| {
            put(f, "routes/me.yaml", &after("me", "me", "me"));
            edit(f, "pack.toml", "\"avoid@1\"", "\"after@1\", \"avoid@1\"");
        },
        true,
        "after chain cycle: me -> me",
    );
    // a → b → c → d → e: five routes.
    let chain = |f: &mut Files| {
        let names = ["a", "b", "c", "d", "e"];
        put(f, "routes/s0.yaml", "id: s0\nshape: run\ntrigger: { on: session_start }\nsteps:\n  - id: s\n    run: { argv: [make, a] }\n    effect: environment\naspect: yellow\n");
        for w in names.windows(2) {
            put(f, &format!("routes/{}.yaml", w[1]), &after(w[1], w[0], w[1]));
        }
        edit(f, "pack.toml", "\"avoid@1\"", "\"after@1\", \"avoid@1\"");
    };
    refused_with(chain, true, "at most 4 are allowed");
}

#[test]
fn requests_beyond_the_ceiling_are_refused() {
    refused_with(|f| edit(f, WIC, "level: E1", "level: E2"), true, "exceeds the pack's [exec] requested");
    refused_with(|f| edit(f, WIC, "timeout_ms: 1500", "timeout_ms: 600001"), true, "timeout_ms");
    refused_with(
        |f| edit(f, "pack.toml", "os = [\"linux\", \"macos\"]", "os = [\"linux\", \"windows\"]"),
        true,
        "windows is allowed only for an E0 pack",
    );
    refused_with(
        |f| edit(f, "routes/finish-line-tests.yaml", "effect: verify", "effect: author"),
        true,
        "may never be directed",
    );
    refused_with(
        |f| edit(f, "routes/dead-end-pytest.yaml", "aspect: red", "aspect: green"),
        true,
        "an avoid route is red",
    );
    refused_with(
        |f| edit(f, "pack.toml", "origin = \"reflex\"", "origin = \"instinct\""),
        true,
        "an instinct route is yellow",
    );
}

#[cfg(unix)]
#[test]
fn symlinks_and_special_files_are_refused_on_disk() {
    let d = tempfile::tempdir().unwrap();
    common::write_dir(d.path(), &common::files());
    Pack::load(d.path()).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", d.path().join("types/passwd.yaml")).unwrap();
    let e = format!("{:#}", Pack::load(d.path()).unwrap_err());
    assert!(e.contains("symlink"), "{e}");
    std::fs::remove_file(d.path().join("types/passwd.yaml")).unwrap();
    // A symlinked directory, and the pack directory itself as a symlink.
    std::os::unix::fs::symlink("/etc", d.path().join("attestations")).unwrap();
    assert!(format!("{:#}", Pack::load(d.path()).unwrap_err()).contains("symlink"));
    std::fs::remove_file(d.path().join("attestations")).unwrap();
    let link = d.path().with_extension("link");
    std::os::unix::fs::symlink(d.path(), &link).unwrap();
    assert!(format!("{:#}", Pack::load(&link).unwrap_err()).contains("symlinks are refused"));
    std::fs::remove_file(&link).unwrap();
    // An unknown directory.
    std::fs::create_dir(d.path().join("bin")).unwrap();
    assert!(format!("{:#}", Pack::load(d.path()).unwrap_err()).contains("bin/ is not part of the pack format"));
}
