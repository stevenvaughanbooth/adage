#!/usr/bin/env python3
"""Offline CLI scale smoke test: repeated canonical routine, never a savings study.

Linux/macOS; no extra Python packages. Uses the literal learner fixture and checks complete
track/index byte equality across cold, warm and forced-full mining. Raw output stays outside
the repository in the required, initially empty --work-dir.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time


def digest_artifacts(home):
    paths = [home / "index.json"]
    paths.extend(p for p in (home / "tracks").rglob("*") if p.is_file())
    return {
        str(p.relative_to(home)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted(paths)
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adage", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--sessions", type=int, default=1000)
    args = parser.parse_args()
    if args.sessions < 5:
        parser.error("--sessions must be at least 5")
    if not hasattr(os, "wait4"):
        parser.error("resource measurements require Linux or macOS")
    binary = args.adage.resolve(strict=True)
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    if any(work.iterdir()):
        parser.error("--work-dir must be empty")
    home = work / "home"
    fixture = Path(__file__).resolve().parent.parent / "crates/adage-learn/tests/fixtures/canonical-routine.json"
    session = json.loads(fixture.read_bytes())
    repo = session["repo"]
    source = work / "corpus.jsonl"
    with source.open("w") as output:
        for i in range(args.sessions):
            session["id"] = f"scale-{i:08d}"
            session["identity"]["task_id"] = f"SYNTHETIC-{i:08d}"
            output.write(json.dumps({"schema": 1, "session": session}, separators=(",", ":")) + "\n")
    env = dict(os.environ, ADAGE_HOME=str(home), ADAGE_EMBED_MODEL="none", ADAGE_COLOR="never")
    measurements = []

    def run(name, commands):
        started = time.monotonic()
        out_path = work / f"{name}.json"
        err_path = work / f"{name}.stderr"
        with out_path.open("wb") as out, err_path.open("wb") as err:
            process = subprocess.Popen([str(binary), *commands], env=env, stdout=out, stderr=err)
            _, status, usage = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
        if process.returncode:
            raise RuntimeError(f"{name} failed ({process.returncode}): {err_path.read_text()}")
        result = json.loads(out_path.read_bytes())
        measurement = {
            "phase": name,
            "wall_seconds": round(time.monotonic() - started, 4),
            "peak_rss_mib": round(usage.ru_maxrss / (1024 * 1024 if platform.system() == "Darwin" else 1024), 2),
        }
        if "repos" in result:
            measurement["mining_cache_hit"] = result["repos"][repo]["engine"]["mining_cache_hit"]
        measurements.append(measurement)
        print(json.dumps(measurement), flush=True)
        return result

    first = run("import", ["corpus", "import", str(source), "--repo", repo])
    assert first["committed"] and first["added"] == args.sessions
    duplicate = run("duplicate-import", ["corpus", "import", str(source), "--repo", repo])
    assert duplicate["generation"] == first["generation"]
    assert duplicate["duplicates"] == args.sessions and duplicate["added"] == 0
    cold = run("cold-learn", ["corpus", "learn", "--repo", repo])
    assert cold["corpus_generation"] == first["generation"]
    assert cold["sessions_used"] == args.sessions
    assert cold["repos"][repo]["engine"]["routes"] == 1, "the reviewed routine should produce one route"
    assert not cold["repos"][repo]["engine"]["mining_cache_hit"]
    expected = digest_artifacts(home)
    for name, extra in [("warm-learn", []), ("full-learn", ["--no-cache"])]:
        report = run(name, ["corpus", "learn", "--repo", repo, *extra])
        assert report["corpus_generation"] == first["generation"]
        assert digest_artifacts(home) == expected, f"{name} differs from cold artifacts"
        assert report["repos"][repo]["engine"]["mining_cache_hit"] == (name == "warm-learn")
    report = {
        "workload": "synthetic copies of canonical-routine with distinct declared task ids; three steps each",
        "sessions": args.sessions,
        "steps": args.sessions * len(session["steps"]),
        "input_bytes": source.stat().st_size,
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "platform": platform.platform(),
        "cpu_count": os.cpu_count(),
        "embedding_model": "none",
        "corpus_generation": first["generation"],
        "routes": cold["repos"][repo]["engine"],
        "artifacts": expected,
        "measurements": measurements,
        "artifacts_equal": True,
    }
    (work / "summary.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
