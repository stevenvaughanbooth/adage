#!/usr/bin/env python3
"""Self-check for the harbor-dispatch sandbox. Runs no agent and spends nothing.

  python3 selfcheck.py

Proves, on freshly generated sandboxes:
  - generation is deterministic (same seed: same bytes and commit; another seed: only the queue differs);
  - a fresh sandbox is a clean one-commit repository whose CI (`npm run check`) passes;
  - the dead end: `npm test` fails with one distinctive error, and `npm run check` passes;
  - the lookup: `bin/fetch-incident` resolves every task ID (text and --json, any case), lists exactly
    the task's files (all of which exist, and no other path-like text), and rejects unknown IDs; the
    queue file is opaque (no incident ID appears in its bytes);
  - every incident fails untouched (its hidden test runs and fails) and passes with its reference
    solution (CI green, hidden suite seen in CI, hidden test run explicitly, generated code fresh);
  - the finish-line obligation: a registry fix without `npm run codegen` fails CI and is scored stale;
  - the scorer's guard: a CI that silently skips the hidden test does not count as a pass;
  - all reference solutions together pass every hidden test (the incidents do not interfere).
"""

import hashlib
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import generate  # noqa: E402
import score as scorer  # noqa: E402
import solutions  # noqa: E402

SPEC = json.loads((HERE / "incidents.json").read_text())
TASKS = {t["id"]: t for t in SPEC["tasks"]}
# adage-core's path-span pattern (crates/adage-core/src/spans.rs): what `adage condition` treats as a
# listed file in plain-text lookup output.
PATH_SPAN = re.compile(r"(?:^|[\s`'(])((?:\.{0,2}/)?[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*\.[A-Za-z0-9]{1,6})\b", re.M)
SIGNATURE = "Error [ERR_MODULE_NOT_FOUND]: Cannot find package '@harbor/dispatch' imported from"
FAILED = []


def check(name, ok, detail=""):
    print(f"  {'ok ' if ok else 'BAD'}  {name}{('  ' + detail) if detail else ''}")
    if not ok:
        FAILED.append(name)


def fresh(tmp, name, seed=generate.DEFAULT_SEED):
    d = Path(tmp) / name
    generate.generate(d, seed)
    return d


def sh(argv, cwd):
    p = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, env=scorer.env())
    return p.returncode, p.stdout + p.stderr


def tree_digest(root, skip=(".git",)):
    h = hashlib.sha256()
    for p in sorted(Path(root).rglob("*")):
        rel = p.relative_to(root)
        if rel.parts[0] in skip or not p.is_file():
            continue
        h.update(str(rel).encode() + b"\0" + p.read_bytes() + b"\0")
    return h.hexdigest()


def signature_of(line):
    """adage-learn's failure signature (mine.rs signature_of): drop absolute-path words."""
    parts = line.split(": ")
    kept = parts
    while kept and (kept[0].lstrip().startswith("/") or "/bin/" in kept[0]):
        kept = kept[1:]
    s = ": ".join(kept) if kept else line
    return " ".join(w for w in s.split() if not w.startswith("/"))[:120]


def last_error_line(out):
    for line in reversed(out.splitlines()):
        lc = line.strip().lower()
        if "error" in lc or "not found" in lc or "not set" in lc or "no module named" in lc:
            return signature_of(line.strip())
    return None


def main():
    with tempfile.TemporaryDirectory() as tmp:
        print("generation")
        a, b, c = fresh(tmp, "a"), fresh(tmp, "b"), fresh(tmp, "c", seed=7)
        check("same seed, same files", tree_digest(a) == tree_digest(b))
        head = lambda d: sh(["git", "rev-parse", "HEAD"], d)[1].strip()
        check("same seed, same commit", head(a) == head(b), head(a)[:12])
        check("another seed changes only the ops queue",
              tree_digest(a, (".git", ".ops")) == tree_digest(c, (".git", ".ops")) and tree_digest(a) != tree_digest(c))
        rc, log = sh(["git", "log", "--oneline"], a)
        _, status = sh(["git", "status", "--porcelain"], a)
        check("one commit, clean tree", rc == 0 and len(log.strip().splitlines()) == 1 and status.strip() == "")

        print("dead end")
        rc, out = sh(["npm", "test"], a)
        sig = last_error_line(out)
        check("npm test fails", rc != 0)
        check("with one distinctive error", sig == SIGNATURE, repr(sig))
        rc, out = sh(["node", "--test"], a)
        check("node --test fails the same way", rc != 0 and last_error_line(out) == SIGNATURE)
        rc, out = sh(["npm", "run", "check"], a)
        passes = re.findall(r"ℹ pass \d+", out)
        check("npm run check passes on a fresh sandbox", rc == 0, passes[-1] if passes else "")

        print("lookup")
        pack = (a / ".ops" / "incidents.pack").read_bytes()
        check("queue file is opaque", b"INC-" not in pack and b"src/" not in pack)
        rc, out = sh(["./bin/fetch-incident", "INC-9999"], a)
        check("unknown ID is rejected", rc != 0 and "no incident" in out)
        rc, out = sh(["./bin/fetch-incident"], a)
        check("no ID prints usage", rc != 0 and "usage" in out)
        for tid, t in TASKS.items():
            rc, text = sh(["./bin/fetch-incident", tid], a)
            rcj, js = sh(["./bin/fetch-incident", tid.lower(), "--json"], a)
            try:
                j = json.loads(js)
            except ValueError:
                j = {}
            spans = sorted({m.group(1) for m in PATH_SPAN.finditer(text)})
            ok = (rc == 0 and rcj == 0 and j.get("id") == tid and j.get("files") == t["files"]
                  and t["title"] in text and all((a / f).is_file() for f in t["files"])
                  and spans == sorted(t["files"]) and "split" not in j and "kind" not in j)
            check(f"{tid:<8} lookup", ok, ", ".join(t["files"]) if ok else f"spans={spans} json={j.get('files')}")

        print("incidents: untouched fails, reference solution passes")
        for tid in TASKS:
            d = fresh(tmp, f"t-{tid}")
            before = scorer.score(d, tid)
            solutions.apply(d, tid)
            after = scorer.score(d, tid)
            ok_before = (not before["passed"]) and (not before["hidden_passed"]) and before["hidden_failures"] > 0
            ok_after = (after["passed"] and after["ci_passed"] and after["ci_ran_hidden"] and after["hidden_passed"]
                        and not after["stale_generated"] and after["hidden_tests_run"] >= after["hidden_tests_expected"])
            kind = TASKS[tid]["kind"]
            check(f"{tid:<8} {TASKS[tid]['split']:<5} {kind:<8}", ok_before and ok_after,
                  f"untouched: {before['hidden_failures']}/{before['hidden_tests_run']} hidden failing; "
                  f"solved: {after['hidden_tests_run']}/{after['hidden_tests_expected']} hidden passing, CI "
                  f"{'green' if after['ci_passed'] else 'RED'}")

        print("finish-line obligation")
        for tid in sorted(solutions.REGISTRY):
            d = fresh(tmp, f"stale-{tid}")
            solutions.apply(d, tid, regenerate=False)
            s = scorer.score(d, tid)
            check(f"{tid:<8} registry edit without codegen fails", s["stale_generated"] and not s["passed"]
                  and not s["ci_passed"] and scorer.STALE_MARK in s["ci_tail"])

        print("scorer guard")
        d = fresh(tmp, "skip")
        solutions.apply(d, "INC-58")
        pj = d / "package.json"
        pj.write_text(pj.read_text().replace("aliases.mjs --test\"", "aliases.mjs --test test/ports.test.ts\""))
        s = scorer.score(d, "INC-58")
        check("a CI that skips the hidden test is not a pass", s["ci_passed"] and not s["ci_ran_hidden"] and not s["passed"])

        print("all solutions together")
        d = fresh(tmp, "all")
        for tid in TASKS:
            solutions.apply(d, tid)
        results = {tid: scorer.score(d, tid) for tid in TASKS}
        bad = [tid for tid, s in results.items() if not s["passed"]]
        check("every hidden test passes with every fix applied", not bad, ", ".join(bad))

    print("\nALL OK" if not FAILED else f"\nFAILURES: {', '.join(FAILED)}")
    return 0 if not FAILED else 1


if __name__ == "__main__":
    sys.exit(main())
