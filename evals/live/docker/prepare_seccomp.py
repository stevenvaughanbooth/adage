#!/usr/bin/env python3
"""Add the verified Bubblewrap namespace rules to one pinned Docker profile.

Download the upstream file separately using the URL in docs/paid-pilot-docker.md.
The original profile is not vendored; both it and the generated profile belong
with the private pilot preparation evidence. No network or Docker calls occur.
"""

import argparse
import hashlib
import json
from pathlib import Path


UPSTREAM_SHA256 = "6416b47770785a41ac59073cdc77d9fe98517df2799dc83ef207e622de3053f6"
NAMESPACE_RULE = {
    "names": ["clone", "unshare", "mount", "umount2", "pivot_root"],
    "action": "SCMP_ACT_ALLOW",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    raw = args.source.read_bytes()
    if hashlib.sha256(raw).hexdigest() != UPSTREAM_SHA256:
        parser.error("source is not the pinned upstream Docker seccomp profile")
    profile = json.loads(raw)
    if profile.get("defaultAction") != "SCMP_ACT_ERRNO":
        parser.error("expected a deny-by-default Docker profile")
    profile["syscalls"].append(NAMESPACE_RULE)
    # Never overwrite the profile already used by a running/pinned pilot.
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(profile, output, indent=2)
        output.write("\n")


if __name__ == "__main__":
    main()
