#!/usr/bin/env python3
"""Self-check for generated sandboxes. Runs no agent and spends nothing.

  python3 evals/gen/selfcheck.py DIR [--quick] [--json]

DIR is a bundle (gen.py --out) or a suite (gen.py --suite; every bundle in it is checked). This
generalises the hand-built sandboxes' self-checks. For each bundle, on fresh copies of its repo:

  - determinism: regenerating from spec.json and the seed gives the same bytes and commit;
  - the fresh repository is a clean one-commit git repository whose CI command passes;
  - the lookup resolves every tracker task (text and --json, any case), lists exactly the task's
    files (all of which exist, and no other path-like text), and rejects unknown IDs; the store is
    opaque (no task ID appears in its bytes); prompts of tracker tasks name only the ID, and no
    control prompt contains anything ID-like;
  - every task fails untouched (its hidden test runs and fails) and passes with its reference
    solution (CI green, hidden test seen in CI and run explicitly, generated code fresh);
  - every dead end and instinct violation: the avoided command fails and its output contains the
    declared signature; the working command passes;
  - every finish-line obligation: the reference edit without the regenerate step fails CI with the
    declared signature, is scored stale, and fails the hidden check;
  - every decoy is inert: it exits 0 and changes nothing, on a fresh repository and after a
    registry edit (it does not stand in for the real regenerate step);
  - all reference solutions together pass every task.

`--quick` skips the per-task untouched/solved pairs except one task of each kind. Bundles of a stack
whose toolchain is missing (node for node-npm) are skipped with a note, not failed.
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
import gen  # noqa: E402
import grammar  # noqa: E402
import score as scorer  # noqa: E402
from skins import SKINS  # noqa: E402
from stacks.base import PATH_SPAN, TICKET  # noqa: E402

NODE_MIN = (20, 6)


def node_ok():
    exe = shutil.which("node")
    if not exe or not shutil.which("npm"):
        return False, "node/npm not on PATH"
    out = subprocess.run([exe, "--version"], capture_output=True, text=True).stdout.strip()
    try:
        ver = tuple(int(x) for x in out.lstrip("v").split(".")[:2])
    except ValueError:
        return False, f"unreadable node version {out!r}"
    if ver < NODE_MIN:
        return False, f"node {out} is older than {NODE_MIN[0]}.{NODE_MIN[1]}"
    return True, out


def tree_digest(root):
    h = hashlib.sha256()
    for p in sorted(Path(root).rglob("*")):
        rel = p.relative_to(root)
        if rel.parts[0] == ".git" or (len(rel.parts) > 1 and rel.parts[1] == ".git") or not p.is_file():
            continue
        h.update(str(rel).encode() + b"\0" + oct(p.stat().st_mode & 0o777).encode() + b"\0" + p.read_bytes() + b"\0")
    return h.hexdigest()


def bundles(root):
    root = Path(root)
    if (root / "spec.json").exists():
        return [root]
    return sorted(p.parent for p in root.glob("*/spec.json"))


class Checker:
    def __init__(self, bundle, quick=False):
        self.b = Path(bundle).resolve()
        self.quick = quick
        self.failed = []
        self.lines = []
        self.spec = json.loads((self.b / "spec.json").read_text())
        self.tasks = json.loads((self.b / "tasks.json").read_text())
        self.conv = grammar.entries(json.loads((self.b / "conventions.json").read_text()))
        self.sol = json.loads((self.b / "solutions.json").read_text())
        self.sc = json.loads((self.b / "scoring.json").read_text())
        self.tmp = tempfile.TemporaryDirectory(prefix="adage-gen-selfcheck-")
        self.n = 0

    def check(self, name, ok, detail=""):
        self.lines.append(f"  {'ok ' if ok else 'BAD'}  {name}{('  ' + detail) if detail else ''}")
        print(self.lines[-1], flush=True)
        if not ok:
            self.failed.append(name)

    def fresh(self):
        self.n += 1
        d = Path(self.tmp.name) / f"r{self.n}" / self.spec["workdir"]
        shutil.copytree(self.b / "repo", d, symlinks=True)
        return d

    def sh(self, cmd, cwd):
        p = subprocess.run(cmd, shell=True, cwd=cwd, capture_output=True, text=True,
                           env=scorer.env(self.sc.get("scrub_env", [])), timeout=300)
        return p.returncode, p.stdout + p.stderr

    def status(self, cwd):
        return subprocess.run(["git", "status", "--porcelain", "--untracked-files=all"], cwd=cwd,
                              capture_output=True, text=True).stdout

    def apply(self, d, tid, run=True):
        for e in self.sol[tid]["edits"]:
            p = d / e["file"]
            s = p.read_text()
            if s.count(e["old"]) < 1:
                raise SystemExit(f"{tid}: reference edit does not apply to {e['file']}")
            p.write_text(s.replace(e["old"], e["new"], 1))
        if run:
            for cmd in self.sol[tid]["run"]:
                rc, out = self.sh(cmd, d)
                if rc != 0:
                    raise SystemExit(f"{tid}: `{cmd}` failed:\n{out}")

    def conv_of(self, kind):
        return [c for c in self.conv if grammar.kind_of(c) == kind]

    # ------------------------------------------------------------------------------ the checks

    def run_all(self):
        name = self.b.name
        print(f"{name}  ({self.tasks['stack']}, skin {self.tasks['skin']}, seed {self.tasks['seed']})", flush=True)
        if self.tasks["stack"] == "node-npm":
            ok, why = node_ok()
            if not ok:
                self.lines.append(f"  skip  {why}")
                print(self.lines[-1])
                return "skipped"
        self.determinism()
        self.fresh_repo()
        self.lookup()
        self.dead_ends()
        self.tasks_pairs()
        self.finish_lines()
        self.new_kinds()
        self.decoys()
        self.all_together()
        return "ok" if not self.failed else "failed"

    def determinism(self):
        with tempfile.TemporaryDirectory() as t:
            again = gen.build(self.spec["spec"], self.spec["seed"], Path(t) / "b")
            same = tree_digest(again) == tree_digest(self.b)
            head = gen.git(again / "repo", "rev-parse", "HEAD")
        self.check("determinism: regenerating from spec.json + seed gives the same bytes and commit",
                   same and head == self.spec["commit"])

    def fresh_repo(self):
        d = self.fresh()
        log = gen.git(d, "log", "--oneline")
        self.check("fresh repository: one commit, clean tree", len(log.splitlines()) == 1 and not self.status(d))
        rc, out = self.sh(self.tasks["verify"], d)
        self.check(f"fresh repository: `{self.tasks['verify']}` passes", rc == 0, "" if rc == 0 else out[-400:])

    def lookup(self):
        look = self.conv_of("lookup")
        tracker = [t for t in self.tasks["tasks"] if t["kind"] in grammar.TRACKER_KINDS]
        for t in self.tasks["tasks"]:
            shape = self.spec["spec"].get("id_shape", "prefix_dash")
            found = TICKET if shape == "prefix_dash" else grammar.mention_regex(shape, SKINS[self.tasks["skin"]].id_prefix)
            ids = found.findall(" ".join(t["prompts"]))
            if t["kind"] not in grammar.TRACKER_KINDS:
                self.check(f"{t['id']}: {t['kind']} prompts contain nothing ID-like", not ids, str(ids))
            else:
                self.check(f"{t['id']}: prompts name only the ID", set(ids) == {grammar.mention(t["id"])}, str(ids))
        if not look:
            return
        cmd = look[0]["command"]
        pat = re.compile(look[0]["id_pattern"])
        d = self.fresh()
        blob = (d / SKINS[self.tasks["skin"]].store).read_bytes()
        self.check("lookup: the tracker store is opaque (no task ID in its bytes)",
                   blob != b"" and not any(t["id"].encode() in blob for t in tracker))
        linked = bool(self.conv_of("linked_files"))
        sol_files = {tid: [e["file"] for e in s["edits"]] for tid, s in self.sol.items()}
        for t in tracker:
            ok_pat = bool(pat.fullmatch(t["id"]))
            rc, text = self.sh(cmd.replace("{item}", t["id"].lower()), d)
            rcj, js = self.sh(cmd.replace("{item}", t["id"]) + " --json", d)
            try:
                item = json.loads(js)
            except ValueError:
                item = {}
            spans = sorted({m.group(1) for m in PATH_SPAN.finditer(text)})
            files = item.get("files", [])
            good = rc == 0 and rcj == 0 and item.get("id") == t["id"] and ok_pat
            if linked:
                good = good and spans == sorted(files) and all((d / f).exists() for f in files) \
                    and set(sol_files[t["id"]]) <= set(files)
            else:
                good = good and not spans
            self.check(f"lookup {t['id']}: resolves (any case, text and --json); lists exactly its files",
                       good, "" if good else f"rc={rc}/{rcj} spans={spans} files={files}")
        rc, _ = self.sh(cmd.replace("{item}", "NOPE-1"), d)
        self.check("lookup: an unknown ID is rejected", rc != 0)

    def dead_ends(self):
        for c in self.conv_of("dead_end") + self.conv_of("instinct_violation"):
            d = self.fresh()
            rc, out = self.sh(c["avoided"][0], d)
            self.check(f"{c['id']}: `{c['avoided'][0]}` fails with the declared signature",
                       rc != 0 and c["signature"] in out, "" if rc != 0 and c["signature"] in out else out[-400:])
            rc, out = self.sh(c["command"], d)
            self.check(f"{c['id']}: `{c['command']}` passes", rc == 0, "" if rc == 0 else out[-400:])
            self.check(f"{c['id']}: the probes leave the tree clean", not self.status(d), self.status(d))

    def tasks_pairs(self):
        picked, seen = [], set()
        for t in self.tasks["tasks"]:
            if not self.quick or t["kind"] not in seen:
                picked.append(t)
                seen.add(t["kind"])
        for t in picked:
            d = self.fresh()
            before = scorer.score(self.b, d, t["id"])
            self.apply(d, t["id"])
            after = scorer.score(self.b, d, t["id"])
            ok = (not before["passed"] and before["hidden_tests_run"] > 0 and not before["hidden_passed"]
                  and after["passed"])
            detail = "" if ok else json.dumps({k: v for k, v in {"before": before, "after": after}.items()})[:1500]
            self.check(f"task {t['id']} ({t['kind']}): fails untouched, passes with its reference solution", ok, detail)

    def finish_lines(self):
        # A sequence (Track G G3) owes the same regenerate step first.
        fl = self.conv_of("finish_line") or self.conv_of("sequence")
        regs = [t for t in self.tasks["tasks"] if t["kind"] == "registry"]
        if not fl:
            return
        for t in regs[: 1 if self.quick else len(regs)]:
            d = self.fresh()
            self.apply(d, t["id"], run=False)
            rc, out = self.sh(self.tasks["verify"], d)
            s = scorer.score(self.b, d, t["id"])
            ok = rc != 0 and fl[0]["signature"] in out and s["stale_generated"] and not s["passed"] \
                and not s["hidden_passed"]
            self.check(f"finish line {t['id']}: the edit without `{fl[0]['command']}` fails CI with the signature, "
                       f"is stale, and fails the hidden check", ok, "" if ok else out[-400:])

    def new_kinds(self):
        """Track G G3 kinds: every package's own suite passes on a fresh repository (the scope
        convention's command, bound to each package); a requirements edit without the environment
        step fails CI with the declared signature, and passes after it."""
        for c in self.conv_of("scope"):
            d = self.fresh()
            for p in c.get("samples", []):
                cmd = c["command"].replace("{part}", p)
                rc, out = self.sh(cmd, d)
                self.check(f"scope: `{cmd}` passes on a fresh repository", rc == 0, "" if rc == 0 else out[-400:])
            self.check("scope: the package suites leave the tree clean", not self.status(d), self.status(d))
        for c in self.conv_of("env_sync"):
            deps = [t for t in self.tasks["tasks"] if t["kind"] == "deps"]
            if not deps:
                continue
            d = self.fresh()
            self.apply(d, deps[0]["id"], run=False)
            rc, out = self.sh(self.tasks["verify"], d)
            self.check(f"env_sync {deps[0]['id']}: the edit without `{c['command']}` fails CI with the signature",
                       rc != 0 and c["signature"] in out, out[-400:])
            self.sh(c["command"], d)
            rc, out = self.sh(self.tasks["verify"], d)
            self.check(f"env_sync {deps[0]['id']}: after `{c['command']}` CI passes", rc == 0, out[-400:])

    def decoys(self):
        regs = [t for t in self.tasks["tasks"] if t["kind"] == "registry"]
        declared = {c["command"] for c in self.conv if c["kind"] != "decoy"}
        for c in self.conv_of("decoy"):
            self.check(f"decoy {c['id']}: not any convention's command", c["command"] not in declared)
            d = self.fresh()
            rc, out = self.sh(c["command"], d)
            self.check(f"decoy {c['id']}: `{c['command']}` exits 0 and changes nothing",
                       rc == 0 and not self.status(d), out[-300:] if rc else self.status(d))
            if regs:
                d = self.fresh()
                self.apply(d, regs[0]["id"], run=False)
                self.sh(c["command"], d)
                s = scorer.score(self.b, d, regs[0]["id"])
                self.check(f"decoy {c['id']}: does not stand in for the regenerate step", not s["passed"])

    def all_together(self):
        d = self.fresh()
        for t in self.tasks["tasks"]:
            self.apply(d, t["id"], run=False)
        for cmd in sorted({c for s in self.sol.values() for c in s["run"]}):
            self.sh(cmd, d)
        bad = [t["id"] for t in self.tasks["tasks"] if not scorer.score(self.b, d, t["id"])["passed"]]
        self.check("all reference solutions together pass every task", not bad, str(bad))


def main(argv):
    args = [a for a in argv if not a.startswith("--")]
    if len(args) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    found = bundles(args[0])
    if not found:
        print(f"selfcheck.py: no bundle under {args[0]}", file=sys.stderr)
        return 2
    results = {}
    for b in found:
        c = Checker(b, quick="--quick" in argv)
        try:
            results[b.name] = {"status": c.run_all(), "failed": c.failed}
        finally:
            c.tmp.cleanup()
    if "--json" in argv:
        print(json.dumps(results, indent=2))
    bad = [k for k, v in results.items() if v["status"] == "failed"]
    skipped = [k for k, v in results.items() if v["status"] == "skipped"]
    print(f"\n{len(found) - len(bad) - len(skipped)} ok, {len(bad)} failed, {len(skipped)} skipped")
    print("ALL OK" if not bad else "FAILURES: " + ", ".join(bad))
    return 0 if not bad else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
