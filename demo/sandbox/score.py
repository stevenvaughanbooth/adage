#!/usr/bin/env python3
"""Score a work item by running its hidden test."""

import json
import os
import sys
import subprocess
import shutil
from pathlib import Path

def score(sandbox_dir, item_id):
    """Run hidden test for item_id in sandbox_dir."""
    sandbox_path = Path(sandbox_dir)
    hidden_test_file = Path(__file__).parent / "hidden" / f"test_{item_id}.py"

    if not hidden_test_file.exists():
        return {
            "id": item_id,
            "passed": False,
            "error": f"Hidden test not found: {hidden_test_file}"
        }

    # Copy hidden test into sandbox
    sandbox_test_file = sandbox_path / "tests" / f"test_{item_id}.py"
    shutil.copy(hidden_test_file, sandbox_test_file)

    # Run make ci
    os.chdir(sandbox_path)
    result = subprocess.run(["make", "ci"], capture_output=True, text=True)

    # Clean up
    sandbox_test_file.unlink()

    return {
        "id": item_id,
        "passed": result.returncode == 0,
        "stdout": result.stdout,
        "stderr": result.stderr
    }

if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("Usage: score.py <sandbox_dir> <item_id>")
        sys.exit(1)

    result = score(sys.argv[1], sys.argv[2])
    print(json.dumps(result, indent=2))
