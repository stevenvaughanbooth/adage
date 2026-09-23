#!/usr/bin/env python3
"""Reference solutions for the harbor-dispatch incidents, as exact text edits. They exist only to
prove that every incident is solvable (selfcheck.py); agents never see them.

  python3 solutions.py <sandbox_dir> <INC-id>     apply one solution (and regenerate if needed)
"""

import shutil
import subprocess
import sys
from pathlib import Path

# (file, old, new): each `old` must occur in the untouched sandbox.
EDITS = {
    "INC-58": [("src/cargo/manifest.ts", "    case 45:\n      return 2;", "    case 45:\n      return 2.25;")],
    "INC-63": [("src/ports/locode.ts", "return input.trim().toUpperCase();",
                'return input.trim().toUpperCase().replace(/[\\s-]+/g, "");')],
    "INC-107": [("src/berths/fit.ts", "vessel.draftM <= berth.depthM",
                 "vessel.draftM + UNDER_KEEL_CLEARANCE_M <= berth.depthM")],
    "INC-112": [("src/eta/transit.ts", "return distanceKm(distanceNm) / speedKn;", "return distanceNm / speedKn;")],
    "INC-86": [("src/cargo/hazmat.ts",
                "  const left = SEGREGATION[primaryClass(a)] ?? [];\n"
                "  const right = SEGREGATION[primaryClass(b)] ?? [];\n"
                "  return left.includes(primaryClass(b)) || right.includes(primaryClass(a));",
                "  const rules = (c: string) => SEGREGATION[c.trim()] ?? SEGREGATION[primaryClass(c)] ?? [];\n"
                "  const listed = (list: readonly string[], c: string) => list.includes(c.trim()) || list.includes(primaryClass(c));\n"
                "  return listed(rules(a), b) || listed(rules(b), a);")],
    "INC-94": [("src/berths/schedule.ts",
                "return a.start.getTime() <= b.end.getTime() && b.start.getTime() <= a.end.getTime();",
                "return a.start.getTime() < b.end.getTime() && b.start.getTime() < a.end.getTime();")],
    "INC-203": [("src/cargo/manifest.ts", "return Math.round(container.cargoKg) / 1000;",
                 "return Math.round(container.cargoKg + container.tareKg) / 1000;")],
    "INC-219": [("src/eta/local.ts", 'return stamp(eta, "UTC");', "return stamp(eta, port.timezone);")],
    "INC-164": [("registry/ports.csv", "PLGDY,Gdynia,PL,Europe/Warsaw,13,2\n",
                 "PLGDY,Gdynia,PL,Europe/Warsaw,13,2\nPLGDN,Gdansk,PL,Europe/Warsaw,15,2\n")],
    "INC-171": [("registry/vessels.csv", "Feeder,140,22,8,1000\n", "Feeder,140,22,8,1000\nFeedermax,200,30,10,3000\n")],
    "INC-245": [("registry/ports.csv", "NLRTM,Rotterdam,NL,Europe/Amsterdam,22.6,2", "NLRTM,Rotterdam,NL,Europe/Amsterdam,24,2")],
    "INC-268": [("registry/vessels.csv", "ULCV,400,61,16,24000\n", "Neopanamax,366,51,15,14000\nULCV,400,61,16,24000\n")],
}

# Incidents whose fix is a registry change, which must be followed by the regeneration step.
REGISTRY = {"INC-164", "INC-171", "INC-245", "INC-268"}


def apply(sandbox, item, regenerate=True):
    sandbox = Path(sandbox)
    for f, old, new in EDITS[item]:
        p = sandbox / f
        s = p.read_text()
        if old not in s:
            raise SystemExit(f"{item}: reference edit does not apply to {f}")
        p.write_text(s.replace(old, new, 1))
    if item in REGISTRY and regenerate:
        subprocess.run([shutil.which("node") or "node", "tools/codegen.mjs"], cwd=sandbox, check=True, capture_output=True)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: solutions.py <sandbox_dir> <INC-id>")
    apply(sys.argv[1], sys.argv[2])
