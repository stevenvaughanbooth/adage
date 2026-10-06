#!/usr/bin/env python3
"""The visible project's generated client example and verification commands."""
import hashlib
import json
from pathlib import Path
import sys

from safe_python import evaluate


def equal(actual, expected):
    if isinstance(expected, bool) or expected is None:
        return type(actual) is type(expected) and actual == expected
    if isinstance(expected, (int, float)):
        return type(actual) in (int, float) and actual == expected
    if isinstance(expected, list):
        return isinstance(actual, list) and len(actual) == len(expected) and all(equal(a, b) for a, b in zip(actual, expected))
    if isinstance(expected, dict):
        return isinstance(actual, dict) and actual.keys() == expected.keys() and all(equal(actual[k], expected[k]) for k in expected)
    return type(actual) is type(expected) and actual == expected


def artifact(source, examples, outputs):
    return {"schema": 1, "source_sha256": hashlib.sha256(source.encode()).hexdigest(),
            "examples": [{"input": example["input"], "output": output}
                         for example, output in zip(examples, outputs)]}


def run(root, action):
    source = (root / "src/solution.py").read_text(encoding="utf-8")
    examples = json.loads((root / "examples.json").read_text(encoding="utf-8"))
    outputs = evaluate(source, [example["input"] for example in examples])
    expected_artifact = artifact(source, examples, outputs)
    path = root / "generated/client-examples.json"
    if action == "generate":
        path.parent.mkdir(exist_ok=True)
        path.write_text(json.dumps(expected_artifact, sort_keys=True, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print("Generated client examples.")
    else:
        if not all(equal(out, example["expected"]) for out, example in zip(outputs, examples)):
            raise ValueError("visible acceptance cases failed")
        if not path.is_file() or json.loads(path.read_text()) != expected_artifact:
            raise ValueError("generated client examples are stale: run make generate")
        print(f"Passed {len(examples)} visible cases; generated client examples are current.")


if __name__ == "__main__":
    if sys.argv[1:] not in (["generate"], ["check"]):
        raise SystemExit("usage: python3 tools/project.py generate|check")
    try:
        run(Path(__file__).resolve().parents[1], sys.argv[1])
    except (OSError, ValueError) as error:
        raise SystemExit(str(error))
