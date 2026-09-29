#!/usr/bin/env python3
"""Mine candidate bug-fix tasks from a repository's real history.

  python3 evals/repobench/mine.py <repo-id> [--limit N] [--out FILE]

A candidate is a first-parent commit (a squash merge, or a merge commit diffed against its first
parent) inside the repository's window whose subject reads as a fix, that changes 1..max_src_files
source files by at most max_src_lines lines, and that adds or changes at least one test file. Its
task: the sandbox is the parent commit (the source fix is not there, nor are the new tests), the
prompt paraphrases the commit message or its issue, and the hidden test is the commit's version
of the test files it touched. Candidates are only candidates: validate.py keeps those whose hidden
test fails at the base, passes with the real fix, and runs in under 60 s.

Prints JSON: {"repo", "candidates": [{sha, parent, date, subject, body, src_files, test_files,
src_lines}]}, newest first.
"""

from __future__ import annotations

import argparse
import json
import re
import sys

import common

FIX = re.compile(r"\b(fix(es|ed)?|bug|correct(ly)?|handle|crash|error|wrong|regression|respect|preserve|ensure|prevent)\b", re.I)
NOT_FIX = re.compile(r"\b(typo|lint|bump|deps?|dependabot|renovate|chore\(deps|ci:|docs?:|test\(|tests?:|refactor|release|revert|perf)\b", re.I)


def parse_log(text: str) -> list[dict]:
    """`git log --format=@@%H%x00%P%x00%cI%x00%s%x00%b%x00 --numstat` into commit records."""
    out = []
    for block in text.split("@@")[1:]:
        parts = block.split("\x00")
        if len(parts) < 6:
            continue
        sha, parents, date, subject, body, stat = parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]
        files = []
        for line in stat.strip().splitlines():
            cols = line.split("\t")
            if len(cols) != 3:
                continue
            a, d, path = cols
            if " => " in path:  # renames: keep the new side
                path = re.sub(r"\{[^}]* => ([^}]*)\}", r"\1", path).replace("//", "/")
                path = path.split(" => ")[-1]
            files.append((path, 0 if a == "-" else int(a), 0 if d == "-" else int(d)))
        out.append({"sha": sha, "parents": parents.split(), "date": date, "subject": subject.strip(),
                    "body": body.strip(), "files": files})
    return out


def candidate(r: dict, c: dict) -> dict | None:
    if not c["parents"]:
        return None
    text = c["subject"] + "\n" + c["body"]
    # A merge commit's subject is "Merge pull request #n from user/branch": read the branch and body.
    if not FIX.search(text) or NOT_FIX.search(c["subject"]) and not re.search(r"\bfix", c["subject"], re.I):
        return None
    tests = [p for p, _, _ in c["files"] if common.is_test(r, p)]
    srcs = [(p, a + d) for p, a, d in c["files"] if common.is_source(r, p)]
    if not tests or not srcs or len(srcs) > r.get("max_src_files", 2):
        return None
    lines = sum(n for _, n in srcs)
    if lines > r.get("max_src_lines", 50) or lines == 0:
        return None
    return {"sha": c["sha"], "parent": c["parents"][0], "date": c["date"], "subject": c["subject"],
            "body": c["body"][:2000], "src_files": [p for p, _ in srcs], "test_files": tests, "src_lines": lines,
            "other_files": [p for p, _, _ in c["files"] if p not in tests and p not in dict(srcs)]}


def mine(rid: str, limit: int | None = None) -> dict:
    r = common.repo(rid)
    m = common.ensure_mirror(r)
    args = ["log", "--first-parent", "--diff-merges=first-parent", f"--since={r['since']}"]
    if r.get("until"):
        args.append(f"--until={r['until']}")
    args += ["--format=@@%H%x00%P%x00%cI%x00%s%x00%b%x00", "--numstat", "HEAD"]
    commits = parse_log(common.git(m, *args))
    cands = [x for x in (candidate(r, c) for c in commits) if x]
    if limit:
        cands = cands[:limit]
    return {"repo": rid, "window": [r["since"], r.get("until")], "commits_scanned": len(commits), "candidates": cands}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repo")
    ap.add_argument("--limit", type=int)
    ap.add_argument("--out")
    a = ap.parse_args(argv)
    res = mine(a.repo, a.limit)
    text = json.dumps(res, indent=1)
    if a.out:
        open(a.out, "w").write(text)
        print(f"{a.repo}: {len(res['candidates'])} candidates of {res['commits_scanned']} commits -> {a.out}")
    else:
        print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
