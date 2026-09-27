#!/usr/bin/env python3
"""Rehearse learning on generated sandboxes with scripted sessions. Spends no tokens.

  python3 evals/gen/rehearse.py DIR [--out ROOT] [--repeats N] [--adage PATH] [--no-learn] [--json]

DIR is a bundle or a suite. For each bundle, a scripted "agent" (plain Python, no model) works every
task `--repeats` times (default 2), each in a fresh copy of the repository, the way unaided agents
do: it explores, runs decoys now and then (while exploring, and after some code edits), looks the item up and reads the files
the lookup lists (tracker tasks) or the file the prompt names (control tasks), applies the reference
fix, regenerates after registry edits (sometimes only after CI reports stale output), often tries
the obvious test runner and the universal default first (the dead end and the instinct violation),
and finishes with the working verify command. Every command really runs in the sandbox, and each
session is written as a Claude Code transcript (JSONL) with the real output, in the layout the live
runner uses (ROOT/<bundle>/transcripts/<project>/<session>.jsonl, outcomes.json).

Then, unless --no-learn, it runs `adage learn --demo` on those transcripts with an isolated
ADAGE_HOME (ROOT/<bundle>/adage-home) and compares the learned routes with conventions.json:
each non-decoy convention must be recovered, and no route may mention a decoy. The adage binary is
--adage, else target/release/adage, else target/debug/adage in this repository. It writes
ROOT/<bundle>/report.json and prints recovered vs expected. The default ROOT is DIR/rehearsal (for
a bundle) or DIR/<bundle>/rehearsal (for a suite).

This proves each generated convention is learnable from sessions that exhibit it, and that decoys
do not become routes. It says nothing about how real agents behave: the live runner measures that.
"""

import json
import re
import shutil
import subprocess
import sys
import time
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))
import grammar  # noqa: E402
import score as scorer  # noqa: E402
import selfcheck  # noqa: E402

T0 = 1788000000.0  # 2026-08-29


class Session:
    """One scripted session, recorded as a Claude Code transcript."""

    def __init__(self, run_id, cwd, prompt, t0, scrub):
        self.run_id, self.cwd, self.t0, self.scrub = run_id, Path(cwd), t0, scrub
        self.sid = str(uuid.uuid5(uuid.NAMESPACE_URL, f"adage-gen-rehearsal/{run_id}"))
        self.lines, self.turn, self.calls = [], 0, []
        self.lines.append(self.rec("user", {"role": "user", "content": prompt}))

    def rec(self, typ, message, **extra):
        ts = time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime(self.t0 + self.turn * 4))
        return {"type": typ, "sessionId": self.sid, "cwd": str(self.cwd), "timestamp": ts, "message": message, **extra}

    def call(self, tool, inp, output, failed=False):
        self.turn += 1
        tid = f"toolu_{self.turn:03d}"
        self.lines.append(self.rec("assistant", {"id": f"msg_{self.turn:03d}", "model": "scripted-rehearsal",
                                                 "role": "assistant",
                                                 "content": [{"type": "tool_use", "id": tid, "name": tool, "input": inp}]}))
        self.lines.append(self.rec("user", {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": tid, "content": output, "is_error": failed}]},
            toolUseResult={"stdout": output} if tool == "Bash" else {}))
        self.calls.append({"tool": tool, "input": inp.get("command") or inp.get("file_path"), "failed": failed})
        return output, failed

    def bash(self, cmd):
        p = subprocess.run(cmd, shell=True, cwd=self.cwd, capture_output=True, text=True,
                           env=scorer.env(self.scrub), timeout=300)
        out = (p.stdout + p.stderr).rstrip("\n")
        if p.returncode != 0:
            out = f"Exit code {p.returncode}\n{out}"
        return self.call("Bash", {"command": cmd, "description": cmd}, out, p.returncode != 0)

    def read(self, rel):
        text = (self.cwd / rel).read_text()
        numbered = "\n".join(f"{i + 1:>6}\t{line}" for i, line in enumerate(text.splitlines()))
        return self.call("Read", {"file_path": str(self.cwd / rel)}, numbered)

    def edit(self, rel, old, new):
        p = self.cwd / rel
        p.write_text(p.read_text().replace(old, new, 1))
        return self.call("Edit", {"file_path": str(p), "old_string": old, "new_string": new},
                         f"The file {p} has been updated successfully.")

    def write(self, transcripts, project):
        d = transcripts / project
        d.mkdir(parents=True, exist_ok=True)
        (d / f"{self.sid}.jsonl").write_text("".join(json.dumps(x) + "\n" for x in self.lines))


class Bundle:
    def __init__(self, path):
        self.path = Path(path).resolve()
        self.spec = json.loads((self.path / "spec.json").read_text())
        self.tasks = json.loads((self.path / "tasks.json").read_text())
        self.conv = json.loads((self.path / "conventions.json").read_text())
        self.sol = json.loads((self.path / "solutions.json").read_text())
        self.sc = json.loads((self.path / "scoring.json").read_text())
        self.by_kind = {}
        for c in self.conv:
            self.by_kind.setdefault(c["kind"], []).append(c)

    def one(self, kind):
        return (self.by_kind.get(kind) or [None])[0]

    def dead_end_variants(self):
        c = self.one("dead_end")
        if not c:
            return []
        runner = c["avoided"]
        if runner.startswith("python3"):
            return [runner, f"{runner} discover -s tests", f"{runner} -v"]
        return [runner, f"{runner} test/", f"{runner} test/*.test.mjs"]


def listed_files(output):
    lines = output.splitlines()
    if "Files:" not in lines:
        return []
    out = []
    for line in lines[lines.index("Files:") + 1:]:
        if not line.startswith("  ") or line.strip() == "(none)":
            break
        out.append(line.strip())
    return out


def work(b, s, task, i):
    """What an unaided agent typically does, with deterministic variation by session index."""
    decoys = b.by_kind.get("decoy", [])
    s.bash("ls")
    s.read("README.md")
    for k, d in enumerate(decoys):
        if (i + k) % 3 == 0:
            s.bash(d["command"])
    if task["kind"] == "control":
        for f in task["files"]:
            s.read(f)
    else:
        if i % 3 == 0:
            s.bash("cat Makefile" if b.tasks["stack"] == "python-make" else "cat package.json")
        look = b.one("lookup")["command"]
        out, _ = s.bash(look.replace("{item}", task["id"]) + (" --json" if i % 5 == 4 else ""))
        files = listed_files(out)
        if not files:
            try:
                files = json.loads(out).get("files", [])
            except ValueError:
                files = []
        for f in files or task["files"]:
            s.read(f)
    for e in b.sol[task["id"]]["edits"]:
        s.edit(e["file"], e["old"], e["new"])
    if task["kind"] != "registry" and i % 4 == 2:
        for d in decoys:  # decoys after an edit too, as agents try things (never the regenerate step)
            s.bash(d["command"])
    if task["kind"] == "registry":
        if i % 4 == 1:
            s.bash(b.tasks["verify"])  # CI reports stale output
        for cmd in b.sol[task["id"]]["run"]:
            s.bash(cmd)
    variants = b.dead_end_variants()
    if variants and i % 3 != 2:
        s.bash(variants[i % len(variants)])  # the dead end
    inst = b.one("instinct_violation")
    if inst and i % 3 != 1:
        s.bash(inst["avoided"])  # the universal default
    s.bash(b.tasks["verify"])


def find_adage(explicit):
    if explicit:
        return Path(explicit)
    for p in (REPO / "target" / "release" / "adage", REPO / "target" / "debug" / "adage"):
        if p.exists():
            return p
    return None


def norm(cmd):
    return " ".join(x[2:] if x.startswith("./") else x for x in (cmd or "").split())


def adage_runner(cmd):
    """adage-learn's runner generalisation (crates/adage-learn/src/mine.rs `runner`): a command's
    runner without its targets. `python3 -m unittest -v` -> `python3 -m unittest`, `node --test` -> `node`."""
    argv, out = cmd.split(), []
    for i, t in enumerate(argv):
        if i > 0 and ("/" in t or "." in t or (t.startswith("-") and t != "-m")):
            break
        out.append(t)
        if t == "-m":
            if i + 1 < len(argv):
                out.append(argv[i + 1])
            break
        if len(out) >= 3:
            break
    return " ".join(out)


def yaml_load(text):
    """The subset of YAML that serde_yaml writes for a route: block mappings, block lists, and
    plain or quoted one-line scalars. Enough to read learned routes without a YAML library."""
    lines = [ln for ln in text.splitlines() if ln.strip() and not ln.lstrip().startswith("#")]
    pos = 0
    ind = lambda ln: len(ln) - len(ln.lstrip(" "))  # noqa: E731

    def scalar(s):
        s = s.strip()
        if len(s) >= 2 and s[0] == s[-1] == "'":
            return s[1:-1].replace("''", "'")
        if s.startswith('"'):
            return json.loads(s)
        special = {"[]": [], "{}": {}, "true": True, "false": False, "null": None, "~": None}
        if s in special:
            return special[s]
        for conv in (int, float):
            try:
                return conv(s)
            except ValueError:
                pass
        return s

    def block(indent):
        nonlocal pos
        if lines[pos].lstrip().startswith("- "):
            out = []
            while pos < len(lines) and ind(lines[pos]) == indent and lines[pos].lstrip().startswith("- "):
                item = lines[pos].lstrip()[2:]
                if re.match(r"^[\w.-]+:( |$)", item):
                    lines[pos] = " " * (indent + 2) + item
                    out.append(block(indent + 2))
                else:
                    pos += 1
                    out.append(scalar(item))
            return out
        out = {}
        while pos < len(lines) and ind(lines[pos]) == indent and not lines[pos].lstrip().startswith("- "):
            key, _, rest = lines[pos].strip().partition(":")
            pos += 1
            if rest.strip():
                out[key] = scalar(rest)
            elif pos < len(lines) and (ind(lines[pos]) > indent
                                       or (ind(lines[pos]) == indent and lines[pos].lstrip().startswith("- "))):
                out[key] = block(ind(lines[pos]))
            else:
                out[key] = None
        return out

    return block(0) if lines else {}


def learned_routes(home):
    """Every learned route in any state (the index holds only active ones; drafts await review)."""
    routes = []
    for p in sorted((Path(home) / "tracks").glob("*/*/routes/*.yaml")):
        r = yaml_load(p.read_text())
        routes.append({"track": f"{p.parent.parent.parent.name}/{p.parent.parent.name}", "route": r})
    return routes


def compare(b, routes):
    """Match learned routes against conventions.json."""
    rows, used = [], set()

    def take(pred):
        for r in routes:
            if pred(r["route"]):
                used.add(r["route"]["id"])
                return r["route"]
        return None

    for c in b.conv:
        kind, got = c["kind"], None
        if kind == "lookup":
            prefix = c["id_pattern"].split("-\\d")[0]
            got = take(lambda r: r["id"] == "lookup-work-item"
                       and norm(r["step"].get("command")).split(" --json")[0] == norm(c["command"])
                       and any(x["contract"].get("regex", "").startswith(prefix + "-") for x in r.get("slots", [])))
        elif kind == "linked_files":
            got = take(lambda r: r["id"] == "read-linked-files")
        elif kind in ("dead_end", "instinct_violation"):
            runner = adage_runner(c["avoided"])
            got = take(lambda r: r["id"].startswith("dead-end")
                       and runner in [x.strip() for x in str(r["step"].get("avoid", "")).split(" / ")]
                       and r["step"].get("instead") == c["command"])
        elif kind == "finish_line":
            got = take(lambda r: r["id"].startswith("finish-line") and r["trigger"].get("paths") == c["glob"]
                       and r["step"].get("command") == c["command"])
        if kind == "decoy":
            hits = [r["route"]["id"] for r in routes if norm(c["command"]) in norm(json.dumps(r["route"]))]
            rows.append({"convention": c["id"], "kind": kind, "expected": "no route", "ok": not hits,
                         "learned": hits})
            continue
        detail = None
        if got:
            detail = {"id": got["id"], "aspect": got.get("aspect"), "state": got.get("state")}
            if kind == "lookup":
                detail.update(command=got["step"]["command"], contract=got["slots"][0]["contract"].get("regex"),
                              declared_pattern=c["id_pattern"])
            elif kind in ("dead_end", "instinct_violation"):
                detail.update(avoid=got["step"]["avoid"], instead=got["step"]["instead"],
                              learned_signature=got["step"]["signature"], declared_signature=c["signature"],
                              declared_avoided=c["avoided"],
                              broadened=adage_runner(c["avoided"]) != c["avoided"],
                              signature_matches=c["signature"] in str(got["step"]["signature"]))
            elif kind == "finish_line":
                detail.update(paths=got["trigger"]["paths"], command=got["step"]["command"])
        rows.append({"convention": c["id"], "kind": kind, "expected": "route", "ok": bool(got), "learned": detail})
    unexpected = [r["route"]["id"] for r in routes if r["route"]["id"] not in used]
    return rows, unexpected


def rehearse_bundle(bundle, root, repeats, adage, learn):
    b = Bundle(bundle)
    root = Path(root).resolve()
    shutil.rmtree(root, ignore_errors=True)
    workdir = b.spec["workdir"]
    project = "-rehearsal-" + b.path.name
    sessions, outcomes = [], {}
    i = 0
    for r in range(repeats):
        for n, t in enumerate(b.tasks["tasks"]):
            run_id = f"train-{t['id'].replace('/', '_')}-r{r + 1}"  # ids of some shapes hold a `/`
            cwd = root / "runs" / run_id / workdir
            cwd.parent.mkdir(parents=True)
            shutil.copytree(b.path / "repo", cwd, symlinks=True)
            prompt = t["prompts"][(n + r) % len(t["prompts"])]
            s = Session(run_id, cwd, prompt, T0 + i * 5 * 3600, b.sc.get("scrub_env", []))
            work(b, s, t, i)
            s.write(root / "transcripts", project)
            sc = scorer.score(b.path, cwd, t["id"])
            outcomes[s.sid] = sc["passed"]
            sessions.append({"run": run_id, "task": t["id"], "kind": t["kind"], "session": s.sid, "turns": s.turn,
                             "passed": sc["passed"], "failed_calls": sum(1 for c in s.calls if c["failed"])})
            i += 1
    (root / "outcomes.json").write_text(json.dumps(outcomes, indent=2, sort_keys=True) + "\n")
    # Fresh ids and near misses from the generator's id distribution, for `adage eval types`.
    if b.one("lookup"):
        tracker = [t["id"] for t in b.tasks["tasks"] if t["kind"] != "control"]
        ho = grammar.holdout(b.spec["spec"], tracker, b.spec["seed"])
        (root / "holdout.json").write_text(json.dumps(ho, indent=1, sort_keys=True) + "\n")
    report = {"bundle": b.path.name, "stack": b.tasks["stack"], "skin": b.tasks["skin"], "seed": b.tasks["seed"],
              "sessions": sessions, "all_passed": all(x["passed"] for x in sessions)}
    if learn:
        tx = sorted(str(p) for p in (root / "transcripts").glob("*/*.jsonl"))
        home = root / "adage-home"
        argv = [str(adage), "learn", "--demo", "--json", "--repo", b.tasks["repo"], "--outcomes",
                str(root / "outcomes.json"), "--transcripts", *tx]
        env = dict(scorer.env(), ADAGE_HOME=str(home), ADAGE_AUTO_LEARN="0")
        out = subprocess.run(argv, capture_output=True, text=True, env=env)
        report["learn_argv"] = ["adage", *argv[1:9], f"<{len(tx)} transcripts>"]
        if out.returncode != 0:
            report["learn_error"] = out.stderr[-2000:]
            report["ok"] = False
        else:
            routes = learned_routes(home)
            report["active_in_index"] = sorted(r["route"]["id"] for r in
                                               json.loads((home / "index.json").read_text()).get("routes", []))
            rows, unexpected = compare(b, routes)
            expected = [x for x in rows if x["expected"] == "route"]
            report.update(
                learn=json.loads(out.stdout), routes=[r["route"] for r in routes], comparison=rows,
                unexpected_routes=unexpected,
                recovered=sum(1 for x in expected if x["ok"]), expected=len(expected),
                decoys_learned=sum(1 for x in rows if x["expected"] == "no route" and not x["ok"]))
            report["ok"] = (report["all_passed"] and report["recovered"] == report["expected"]
                            and report["decoys_learned"] == 0 and not unexpected)
    else:
        report["ok"] = report["all_passed"]
    (root / "report.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    return report


def print_report(rep):
    n = len(rep["sessions"])
    print(f"{rep['bundle']}  ({rep['stack']}, skin {rep['skin']}, seed {rep['seed']}): {n} scripted sessions, "
          f"{sum(1 for s in rep['sessions'] if s['passed'])} passed")
    if "learn_error" in rep:
        print(f"  learn failed: {rep['learn_error']}")
        return
    if "comparison" not in rep:
        return
    for row in rep["comparison"]:
        mark = "ok " if row["ok"] else "BAD"
        if row["expected"] == "no route":
            print(f"  {mark}  {row['convention']:<22} {row['kind']:<19} expected no route, learned "
                  f"{row['learned'] or 'none'}")
        else:
            d = row["learned"] or {}
            extra = ""
            if row["kind"] == "lookup" and d:
                extra = f" {d['command']!r} contract {d['contract']} (declared {d['declared_pattern']})"
            elif row["kind"] in ("dead_end", "instinct_violation") and d:
                extra = f" avoid {d['avoid']!r} instead {d['instead']!r} signature {d['learned_signature']!r}"
                if d["broadened"]:
                    extra += f" [declared {d['declared_avoided']!r}: adage names the runner only]"
            elif row["kind"] == "finish_line" and d:
                extra = f" {d['paths']} -> {d['command']!r}"
            state = f" [{d['state']}]" if d and d.get("state") != "active" else ""
            print(f"  {mark}  {row['convention']:<22} {row['kind']:<19} "
                  f"{('recovered ' + d['id'] + state + extra) if d else 'NOT recovered'}")
    print(f"  recovered {rep['recovered']}/{rep['expected']} conventions, {rep['decoys_learned']} decoys learned, "
          f"unexpected routes: {rep['unexpected_routes'] or 'none'}")


def main(argv):
    args = list(argv)

    def take(name, default=None):
        if name not in args:
            return default
        i = args.index(name)
        v = args[i + 1]
        del args[i:i + 2]
        return v

    out, repeats, adage = take("--out"), int(take("--repeats", "2")), take("--adage")
    learn = "--no-learn" not in args
    as_json = "--json" in args
    args = [a for a in args if not a.startswith("--")]
    if len(args) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    found = selfcheck.bundles(args[0])
    if not found:
        print(f"rehearse.py: no bundle under {args[0]}", file=sys.stderr)
        return 2
    exe = find_adage(adage) if learn else None
    if learn and (not exe or not exe.exists()):
        print("rehearse.py: no adage binary (cargo build -p adage-cli, or pass --adage PATH, or --no-learn)",
              file=sys.stderr)
        return 2
    reports = []
    for b in found:
        stack = json.loads((b / "tasks.json").read_text())["stack"]
        if stack == "node-npm":
            ok, why = selfcheck.node_ok()
            if not ok:
                print(f"{b.name}: skipped ({why})")
                reports.append({"bundle": b.name, "skipped": why, "ok": True})
                continue
        root = Path(out) / b.name if out and len(found) > 1 else Path(out) if out else b / "rehearsal"
        rep = rehearse_bundle(b, root, repeats, exe, learn)
        reports.append(rep)
        if not as_json:
            print_report(rep)
    if as_json:
        print(json.dumps(reports, indent=2))
    bad = [r["bundle"] for r in reports if not r["ok"]]
    print("\nREHEARSAL OK" if not bad else f"\nREHEARSAL FAILED: {', '.join(bad)}")
    return 0 if not bad else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
