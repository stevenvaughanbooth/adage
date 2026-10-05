#!/usr/bin/env python3
"""Adage's procedural sandbox generator.

  python3 evals/gen/gen.py --spec spec.json --seed N --out DIR       one sandbox bundle
  python3 evals/gen/gen.py --suite smoke --out DIR                   a suite (specs/<suite>.json)
  python3 evals/gen/gen.py --spec-json '{...}' --seed N --repo-only DIR [--task ID]
                                                                     just the repository (what the
                                                                     live runner's `sandbox` runs)

A bundle is:
  repo/                 the sandbox: a git repository with one commit
  hidden/               one hidden test per task (never inside the repository)
  pristine/             the scorer's own copies of the generator and test setup
  conventions.json      ground truth: the conventions the repository has, and the decoys
  tasks.json            tasks: prompts, split, the conventions each exercises
  solutions.json        reference solutions: exact edits, then commands
  scoring.json          how score.py scores this stack
  spec.json             the normalised spec and the seed
  config.json           a live-runner config (tracker tasks), for evals/live/runner.py
  config-control.json   a live-runner config for the control tasks (their brief goes to BRIEF.md)
  score.py              the scorer: score.py <sandbox> <task>

Output is a pure function of (spec, seed): the same inputs give the same bytes and the same commit.
Python 3 standard library only. Generation needs git; it does not need node.
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import grammar  # noqa: E402
from stacks import STACKS  # noqa: E402

FIXED_DATE = "2026-09-01T09:00:00+00:00"


def dump(obj):
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


# Variables that point git at another repository. Under `git bisect run`, a hook or any git that
# exports them, a sandbox's `git init` would reinitialise that repository (as bare) and commit the
# sandbox into it: the sandbox's git sees only its own directory.
REPO_ENV = ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_COMMON_DIR",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_NAMESPACE", "GIT_PREFIX")


def git_env(**extra):
    """The environment for git in a sandbox: this process's, without REPO_ENV."""
    return {**{k: v for k, v in os.environ.items() if k not in REPO_ENV}, **extra}


def git(cwd, *args, author=("Sandbox Maintainers", "maintainers@sandbox.example")):
    env = git_env(GIT_AUTHOR_NAME=author[0], GIT_AUTHOR_EMAIL=author[1], GIT_COMMITTER_NAME=author[0],
                  GIT_COMMITTER_EMAIL=author[1], GIT_AUTHOR_DATE=FIXED_DATE, GIT_COMMITTER_DATE=FIXED_DATE,
                  GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
    return subprocess.run(["git", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false",
                           "-c", "core.autocrlf=false", *args],
                          cwd=cwd, env=env, check=True, capture_output=True, text=True).stdout.strip()


def empty_dir(out):
    out = Path(out)
    if out.exists() and (not out.is_dir() or any(out.iterdir())):
        raise SystemExit(f"gen.py: {out} exists and is not empty")
    out.mkdir(parents=True, exist_ok=True)
    return out


def write(path, text, executable=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode() if isinstance(text, str) else text)
    path.chmod(0o755 if executable else 0o644)


def plan_for(spec, seed):
    plan = grammar.resolve(spec, seed, STACKS)
    return plan, STACKS[plan.stack](plan)


def write_repo(stack, out, brief_task=None):
    """The sandbox repository: files, the generated registry, one commit. Returns the commit id."""
    out = empty_dir(out)
    for rel, (text, exe) in sorted(stack.repo_files().items()):
        write(out / rel, text, exe)
    sk = stack.skin
    if stack.conv.lookup:
        write(out / sk.store, stack.store_bytes())
    if stack.conv.finish_line:
        write(out / stack.generated_path(), stack.generated_registry(out))
    git(out, "init", "-q")
    git(out, "add", "-A")
    git(out, "commit", "-q", "-m", f"{sk.repo} 2.3.0")
    head = git(out, "rev-parse", "HEAD")
    if brief_task:
        brief = stack.brief(brief_task)
        if brief:
            write(out / "BRIEF.md", brief)
    return head


def build(spec, seed, out, repo=True):
    """Write a bundle to `out` (which must be empty or absent). Returns the bundle path."""
    plan, stack = plan_for(spec, seed)
    out = empty_dir(out)
    head = write_repo(stack, out / "repo") if repo else None
    hidden = {}
    for t in plan.tasks:
        name, text = stack.hidden_test(t)
        hidden[t.id] = name
        write(out / "hidden" / name, text)
    for rel, text in sorted(stack.pristine_files().items()):
        write(out / "pristine" / rel, text)
    tasks = stack.tasks_json(hidden)
    for e in tasks["tasks"]:
        e["hidden_tests"] = stack.hidden_count((out / e["hidden"]).read_text())
    write(out / "tasks.json", dump(tasks))
    write(out / "conventions.json", dump(stack.conventions()))
    write(out / "solutions.json", dump({t.id: stack.solution(t) for t in plan.tasks}))
    write(out / "scoring.json", dump({**stack.scoring(), "stack": stack.NAME}))
    write(out / "spec.json", dump({"spec": plan.spec, "seed": seed, "commit": head, "workdir": stack.skin.repo}))
    for name, cfg in stack.live_configs(grammar.canonical(plan.spec)).items():
        write(out / name, dump(cfg))
    write(out / "score.py", (HERE / "score.py").read_text(), True)
    return out


def load_suite(name):
    p = Path(name)
    if not p.exists():
        p = HERE / "specs" / f"{name}.json"
    if not p.exists():
        raise SystemExit(f"gen.py: no suite {name!r} (looked for {p})")
    return json.loads(p.read_text())


def build_suite(name, out):
    suite = load_suite(name)
    out = empty_dir(out)
    listing = []
    for entry in suite["sandboxes"]:
        spec, seed = entry["spec"], entry["seed"]
        plan, _ = plan_for(spec, seed)
        d = f"{plan.name}-s{seed}"
        build(spec, seed, out / d)
        listing.append({"dir": d, "stack": plan.stack, "skin": plan.skin.name, "seed": seed,
                        "dead_end": plan.conv.dead_end, "tasks": len(plan.tasks)})
    write(out / "suite.json", dump({"name": suite.get("name", str(name)), "sandboxes": listing}))
    return out


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    src = ap.add_mutually_exclusive_group(required=True)
    src.add_argument("--spec", help="a spec file (JSON)")
    src.add_argument("--spec-json", help="a spec, inline")
    src.add_argument("--suite", help="a suite name (specs/<name>.json) or file")
    ap.add_argument("--seed", type=int, help="the seed (required with --spec / --spec-json)")
    ap.add_argument("--out", help="the output directory (must be empty or absent)")
    ap.add_argument("--repo-only", metavar="DIR", help="write only the sandbox repository to DIR")
    ap.add_argument("--task", help="with --repo-only: a control task's brief is written to BRIEF.md")
    a = ap.parse_args(argv)
    try:
        if a.suite:
            if not a.out:
                ap.error("--suite needs --out")
            out = build_suite(a.suite, a.out)
            print(f"generated suite {a.suite} in {out}")
            return 0
        if a.seed is None:
            ap.error("--seed is required with --spec / --spec-json")
        spec = json.loads(Path(a.spec).read_text()) if a.spec else json.loads(a.spec_json)
        if "spec" in spec and "seed" in spec and set(spec) <= {"spec", "seed", "commit", "workdir"}:
            spec = spec["spec"]  # a bundle's spec.json
        if a.repo_only:
            _, stack = plan_for(spec, a.seed)
            write_repo(stack, a.repo_only, brief_task=a.task)
            print(f"generated {stack.skin.repo} in {a.repo_only}")
            return 0
        if not a.out:
            ap.error("--out or --repo-only is required")
        out = build(spec, a.seed, a.out)
        print(f"generated {out}")
        return 0
    except grammar.SpecError as e:
        print(f"gen.py: invalid spec: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
