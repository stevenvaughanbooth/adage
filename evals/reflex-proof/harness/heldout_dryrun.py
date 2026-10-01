#!/usr/bin/env python3
"""Offline dry run of the held-out freeze pipeline, on a scratch copy of the evaluation authority.

The aim is that `heldout_cohort.py freeze`, `prepare`, `approval-script` and `verify-grants` work
the first time once a real development report exists. This tool spends nothing.

What it never does:
- write under the real authority;
- run Python from the real authority's runtime or bases (it copies the runtime and caches, and
  runs only the copies);
- write a grant or an approvals file.

What it builds, all under `scratch`:
1. A scratch authority: the authority's manifest, with paths repointed; copies of its runtime,
   uv cache and repository mirror; its validation, confinement and allocation records.
2. A scratch cohort with its native-confinement recipe, and a synthetic review marked `dry_run`.
   That review is never valid evidence. The real held-out run needs its own reviewed native
   session.
3. Validated held-out task controls (`heldout_tasks.validate` on the scratch substrate).
4. A scratch copy of the frozen learned home, with its preparation interpreter repointed to the
   scratch runtime.
5. A simulated `dev-arms-2` report in which A4 passes, marked `simulated`.

It then runs `heldout_cohort` freeze, prepare, approval-script and verify-grants. verify-grants is
expected to report every BEST home's missing person-issued grant, and nothing else.

The held-out tasks' hidden-test identities come from their validation (`freeze` copies them). The
finite control task's identities are copied from an earlier study root: computing them would run
the real development controls' gold environment, which is linked to the real runtime. The real
`prepare` computes them itself, as the development studies did.

Usage: python3 heldout_dryrun.py <scratch dir> --binaries <dir with adage, adage-exec> [--build-commit SHA]
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import cohort_v5  # noqa: E402
import dev_v9  # noqa: E402
import heldout_cohort as runner  # noqa: E402
import heldout_tasks  # noqa: E402
import real_repo_pairs as pairs  # noqa: E402

AUTHORITY = Path("/private/tmp/adage-real-repo-gate-v1")
CUMULATIVE = AUTHORITY / "cohorts/curve-guided-v2/cumulative-launch-10"
DEV_CONTROLS = AUTHORITY / "cohorts/curve-v2"
IDENTITIES = AUTHORITY / "cohorts/launch-gate-v5/dev-v8b/private/hidden-identities/mm-curve--finite.json"
EXTRA_AUDIT = [Path("/private/tmp/adage-learning-curve-v2")]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def writable(root):
    for d, dirs, files in os.walk(root):
        for name in [*dirs, *files]:
            p = Path(d) / name
            if not p.is_symlink():
                p.chmod(p.stat().st_mode | 0o200)
    Path(root).chmod(Path(root).stat().st_mode | 0o200)


def repoint(root, old, new):
    """Absolute links into `old` point into `new` instead."""
    for d, dirs, files in os.walk(root):
        for name in dirs + files:
            p = Path(d) / name
            if p.is_symlink() and os.readlink(p).startswith(str(old) + "/"):
                target = os.readlink(p)
                p.unlink()
                p.symlink_to(str(new) + target[len(str(old)):])


def scratch_authority(scratch):
    a = scratch / "authority"
    a.mkdir(parents=True)
    for relative in ("runtime/python", "cache/uv", "cache/repobench"):
        shutil.copytree(AUTHORITY / relative, a / relative, symlinks=True)
        writable(a / relative)
        repoint(a / relative, AUTHORITY, a)
    (a / "bases").mkdir()                 # nothing in the held-out path reads a base checkout
    (a / "private/validation").mkdir(parents=True)
    # Only where the authority lives and its interpreter move. The families' frozen dependency lines
    # (an editable `file://` base path among them) stay as recorded: the held-out freeze compares
    # them with the learned home's.
    m = pairs.read(AUTHORITY / "manifest.json")
    m.update(root=str(a), preparation_python=m["preparation_python"].replace(str(AUTHORITY), str(a)))
    pairs.write(a / "manifest.json", m)
    for name in ("validation.json", "confinement.json"):
        shutil.copy2(AUTHORITY / name, a / name)
    pairs.write(a / "evaluation-allocation.json", dict(pairs.read(AUTHORITY / "evaluation-allocation.json"),
                                                       authority=str(a), dry_run=True))
    pairs.write(a / "paid-state.json", dict(runs={}, accounting=dict(version=1, attempts=[], blocked_reason=None)))
    return a


def scratch_cohort(a):
    cohort = a / "cohorts/heldout-dry"
    cohort_v5.prepare_cohort(a, cohort)
    native = cohort_v5.prepare_session(cohort)
    review = dict(passed=True, dry_run=True, cohort_root=str(cohort), manifest_sha256=sha(native),
                  note="SYNTHETIC: a dry run's stand-in. The real held-out cohort needs a reviewed native session.")
    pairs.write(cohort / "native-confinement-reviewed.json", review)
    return cohort


def scratch_learned_home(a):
    """The frozen learned home, copied, with its preparation interpreter pointed at the scratch runtime."""
    guided = a / "cohorts/curve-guided-v2"
    shutil.copytree(CUMULATIVE, guided / CUMULATIVE.name, symlinks=True)
    m = pairs.read(CUMULATIVE.parent / "manifest.json")
    m["preparation_python"] = m["preparation_python"].replace(str(AUTHORITY), str(a))
    pairs.write(guided / "manifest.json", m)
    return guided / CUMULATIVE.name


def simulated_report(scratch):
    path = scratch / "dev-v9b-simulated-report.json"
    pairs.write(path, dict(protocol=dev_v9.PROTOCOL, simulated=True, valid=True, problems=[],
                           arms=dict(A0=dict(requests=17.0, cost_usd=0.12), A2f=dict(requests=15.0, cost_usd=0.11),
                                     A4=dict(requests=12.5, cost_usd=0.10)),
                           verdict=dict(A2f=dict(passed=False, eligible=False), A4=dict(passed=True, eligible=True)),
                           candidates=list(dev_v9.CONTROL_ARMS), best="A4"))
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("scratch", type=Path)
    parser.add_argument("--binaries", type=Path, required=True)
    parser.add_argument("--build-commit", default="HEAD")
    parser.add_argument("--reuse", action="store_true",
                        help="reuse a scratch authority, cohort, controls and learned home built by an earlier run")
    args = parser.parse_args()
    scratch = args.scratch.resolve()
    if AUTHORITY == scratch or AUTHORITY in scratch.parents:
        raise SystemExit("the dry run never writes under the real authority")
    steps = {}
    a, controls = scratch / "authority", scratch / "heldout-controls"
    cohort, cumulative = a / "cohorts/heldout-dry", a / "cohorts/curve-guided-v2" / CUMULATIVE.name
    if not args.reuse:
        scratch.mkdir(parents=True, exist_ok=False)
        scratch_authority(scratch)
        scratch_cohort(a)
        heldout_tasks.validate(controls, a)
        scratch_learned_home(a)
    v = pairs.read(controls / "validation.json")
    steps.update(authority=str(a), cohort=str(cohort), reused=args.reuse,
                 heldout_controls=dict(root=str(controls), complete=v["complete"], valid=v["valid"],
                                       evaluation_valid=v["evaluation_valid"]))
    report = simulated_report(scratch)
    binaries = {n: sha(args.binaries / n) for n in ("adage", "adage-exec")}
    commit = subprocess.check_output(["git", "rev-parse", args.build_commit], cwd=HERE, text=True).strip()
    root = next(cohort / ("heldout-dry-%d" % n) for n in range(1, 100) if not (cohort / ("heldout-dry-%d" % n)).exists())
    m = runner.freeze(root, cohort, cumulative, DEV_CONTROLS, controls, report, args.binaries, commit, binaries,
                      [str(AUTHORITY), *map(str, EXTRA_AUDIT)])
    steps["freeze"] = dict(root=str(root), sessions=len(m["matrix"]), best=m["best"], budget=m["budget"])
    (root / "private/hidden-identities").mkdir(parents=True, exist_ok=True)
    shutil.copy2(IDENTITIES, root / "private/hidden-identities" / IDENTITIES.name)
    prepared = runner.prepare(root)
    steps["prepare"] = dict(assignments=len(prepared["assignments"]), runtime_protected=bool(prepared["runtime"]))
    steps["approval_script"] = runner.approval_script(root)
    grants = runner.verify_grants(root)
    missing = [p for p in grants["problems"] if "approvals.json" in p["problem"] or "No such file" in p["problem"]]
    steps["verify_grants"] = dict(passed=grants["passed"], problems=len(grants["problems"]),
                                  only_missing_best_grants=len(missing) == len(grants["problems"]) == len(runner.granted(m)))
    pairs.write(scratch / "dry-run.json", steps)
    print(json.dumps(steps, indent=2, default=str))


if __name__ == "__main__":
    main()
