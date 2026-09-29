"""Showcase family on Go: `go-flag`, ported from the recurring-workflows confirmation.

  go-flag  "add a new flag type `cli.<Name>Flag` with a `cmd.<Name>(name)` accessor" (urfave/cli v3 @ 58ec406e)

Prompt, hidden check and gold patch are the confirmation's own (evals/lab/research/recurring-workflows/
confirm/family_urfave.py, validated 10/10 there), imported unchanged. 2 new variants are appended to the
held-out list (12 = 4 training + 8 held out). The procedure the hidden check requires includes the
generated API docs AND their approved copy (`testdata/godoc-v3.x.txt`, which CI's v3diff compares): in
the confirmation no training session did the approval step, and both arms passed 2/12. That is kept:
it is the project's real CI.
"""

from __future__ import annotations

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "lab" / "research" / "recurring-workflows" / "confirm"))
import family_urfave as lab  # noqa: E402

NEW = [
    dict(id="hostport", flag="HostPortFlag", acc="HostPort", gotype="string", imports=["fmt", "net", "strconv"],
         desc="which accepts a `host:port` pair (a non-empty host, IPv6 hosts in brackets, and a port from 1 to "
              "65535) and keeps it as a `string`.",
         valid=[("localhost:8080", "localhost:8080"), ("[::1]:443", "[::1]:443")],
         invalid=["localhost", ":80", "host:0", "host:http"],
         parse="""
            host, port, err := net.SplitHostPort(s)
            if err != nil || host == "" {
                return "", fmt.Errorf("invalid host:port %q", s)
            }
            n, err := strconv.Atoi(port)
            if err != nil || n < 1 || n > 65535 {
                return "", fmt.Errorf("invalid port in %q", s)
            }
            return s, nil
         """),
    dict(id="uuid", flag="UUIDFlag", acc="UUID", gotype="string", imports=["fmt", "regexp", "strings"],
         desc="which accepts a UUID in its canonical 8-4-4-4-12 hexadecimal form (case-insensitive) and "
              "normalizes it to lower case (a `string`).",
         valid=[("123E4567-E89B-12D3-A456-426614174000", "123e4567-e89b-12d3-a456-426614174000"),
                ("00000000-0000-0000-0000-000000000000", "00000000-0000-0000-0000-000000000000")],
         invalid=["123e4567e89b12d3a456426614174000", "not-a-uuid", "123e4567-e89b-12d3-a456-42661417400g"],
         parse="""
            if !regexp.MustCompile(`^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$`).MatchString(s) {
                return "", fmt.Errorf("invalid UUID %q", s)
            }
            return strings.ToLower(s), nil
         """),
]

VARIANTS = lab.GO + NEW

NEGATIVES = [
    dict(id="urfave-cli-7492c456", source="repobench"),
    dict(id="urfave-cli-72ca6b88", source="repobench"),
    dict(id="urfave-cli-db7d18f2", source="repobench"),
    dict(id="neg-go-trimspace-env", source="handwritten", edits=["flag_string.go", "flag_test.go"],
         prompt="`StringFlag` ignores `TrimSpace` when the value comes from an environment variable. Fix it and "
                "add a test."),
    dict(id="neg-go-timestamp-local", source="handwritten",
         edits=["flag_timestamp.go", "flag_test.go", "docs/v3/examples/flags/basics.md"],
         prompt="Add a `Local` option to `TimestampFlag` so timestamps are parsed in the local time zone, with "
                "tests and docs."),
    dict(id="neg-go-yaml-docs", source="handwritten", edits=["docs/v3/examples/flags/value-sources.md"],
         prompt="Add an example of reading flag values from a YAML file to the value sources documentation."),
]

FAMILY = dict(lab.FAMILY, id="go-flag", variants=VARIANTS, negatives=NEGATIVES, name_key="flag", lang="go",
              repo_key="github.com/urfave/cli", checks=None, forward_env=[], confine_expose=[], prune=[],
              probe_edits=["flag_url.go", "flag_url_test.go", "docs/v3/examples/flags/basics.md", "godoc-current.txt"])
