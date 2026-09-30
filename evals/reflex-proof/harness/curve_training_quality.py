"""Independently grade every completed training assignment, without altering learning inputs."""
import argparse
import json
from pathlib import Path
import shutil

import curve_sessions as sessions
import real_repo_pairs as pairs

gate = pairs.substrate


def collect(root):
    manifest = pairs.read(root / "manifest.json")
    plan = manifest["families"][0]
    folder = root / "training-quality"
    folder.mkdir(exist_ok=False)
    rows = []
    for task in plan["train"]:
        row = dict(task=task, status="unknown", paid_calls=0)
        try:
            session = root / "sessions" / task
            native = pairs.read(session / "transcript-provenance.json")
            if pairs.sha(session / "native-transcript.jsonl") != native["sha256"]:
                raise ValueError("native transcript changed")
            checkout = Path(pairs.read(session / "manifest.json")["execution_root"])
            final = pairs.tree_state(checkout)
            if any("symlink" in f for f in final["files"].values()):
                raise ValueError("source symlink requires confinement review")
            with sessions.registry(root):
                expected = pairs.prepare_hidden_expectations(root, task)
            scoring = folder / task / "repo"
            gate.create_checkout(root, plan["repo"], plan["base_full"], scoring)
            pairs.frozen_setup(root, scoring, plan, manifest["python_requested"])
            for name in set(pairs.tree_state(scoring)["files"]) - set(final["files"]):
                (scoring / name).unlink()
            for name in final["files"]:
                dest = scoring / name
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(checkout / name, dest)
            if pairs.tree_state(scoring) != final:
                raise ValueError("scorer source generation mismatch")
            with sessions.registry(root):
                quality = pairs.score_with_integrity(root, task, scoring, expected)
            row.update(status="scored", quality=quality, final_generation=final["digest"],
                       native_sha256=native["sha256"], hidden_sha256=expected["hidden_sha256"])
        except Exception as error:
            row["error"] = str(error)
        rows.append(row)
        pairs.write(folder / (task + ".json"), row)
        print(json.dumps(dict(task=task, status=row["status"], passed=row.get("quality", {}).get("passed"))), flush=True)
    result = dict(kind="post-training-diagnostic", rows=rows, paid_calls=0,
        passed=sum(r.get("quality", {}).get("passed", False) for r in rows), total=len(rows),
        limitation="Training quality only; no treatment comparison. Grades never filter or modify frozen learning histories.")
    pairs.write(folder / "report.json", result)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    args = parser.parse_args()
    collect(args.root.resolve(strict=True))
