#!/usr/bin/env python3
"""Reference solutions, as exact text edits. Used only to prove every work item is solvable.

  python3 solutions.py verify     # every item: fails untouched, passes when solved
"""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent

EDITS = {
    "W-1041": [("acme/billing.py", "return int(math.floor(discount_amount))", "return int(round(discount_amount))")],
    "W-1042": [("acme/billing.py", '"EU": 0.15,', '"EU": 0.20,')],
    "W-1043": [("acme/billing.py", "return payment_date >= due_date", "return payment_date > due_date")],
    "W-1044": [("acme/customers.py",
                '    return True\n\ndef validate_customer',
                '    domain = email.split("@", 1)[1]\n    if "." not in domain:\n        raise ValueError("Invalid email: domain needs a dot")\n    return True\n\ndef validate_customer')],
    "W-2041": [("acme/billing.py",
                "amount_dollars = invoice_dict['amount_cents'] // 100\n    return f\"Invoice {invoice_dict['id']}: ${amount_dollars}\"",
                "amount_dollars = invoice_dict['amount_cents'] / 100\n    return f\"Invoice {invoice_dict['id']}: ${amount_dollars:.2f}\"")],
    "W-2042": [("acme/customers.py", "return rates.get(region, 0.0)", "return rates.get(region.lower(), 0.0)")],
    "W-2043": [("acme/billing.py",
                "def apply_discount(amount_cents, discount_pct):\n",
                "def apply_discount(amount_cents, discount_pct):\n    if discount_pct >= 100:\n        raise ValueError(\"discount must be below 100%\")\n")],
    "W-2044": [("acme/customers.py", "    if not name:\n        pass", "    if not name:\n        raise ValueError(\"name is required\")")],
}

SCHEMA = {
    "W-1045": ("schema/invoice.json", "currency", {"type": "str", "default": "USD"}),
    "W-1046": ("schema/invoice.json", "due_days", {"type": "int", "default": 30}),
    "W-2045": ("schema/invoice.json", "payment_terms", {"type": "str", "default": "NET30"}),
    "W-2046": ("schema/customer.json", "tier", {"type": "str", "default": "standard"}),
}


def apply(sandbox, item):
    sandbox = Path(sandbox)
    for f, old, new in EDITS.get(item, []):
        p = sandbox / f
        s = p.read_text()
        if old not in s:
            raise SystemExit(f"{item}: reference edit does not apply to {f}")
        p.write_text(s.replace(old, new, 1))
    if item in SCHEMA:
        f, field, spec = SCHEMA[item]
        p = sandbox / f
        d = json.loads(p.read_text())
        d[field] = spec
        p.write_text(json.dumps(d, indent=2))
        subprocess.run(["make", "gen"], cwd=sandbox, check=True, capture_output=True)


def score(sandbox, item):
    out = subprocess.run([sys.executable, str(HERE / "score.py"), str(sandbox), item], capture_output=True, text=True)
    return json.loads(out.stdout)


def verify():
    items = sorted(set(EDITS) | set(SCHEMA))
    ok = True
    for item in items:
        with tempfile.TemporaryDirectory() as d:
            subprocess.run([sys.executable, str(HERE / "generate.py"), d], check=True, capture_output=True)
            before = score(d, item)
            apply(d, item)
            after = score(d, item)
        good = (not before["passed"]) and after["passed"]
        ok &= good
        print(f"{item}  untouched: {'PASS' if before['passed'] else 'fail'}  solved: {'PASS' if after['passed'] else 'fail'}  {'ok' if good else 'BROKEN'}")
    # Obligation: a schema change without `make gen` must fail CI.
    with tempfile.TemporaryDirectory() as d:
        subprocess.run([sys.executable, str(HERE / "generate.py"), d], check=True, capture_output=True)
        f, field, spec = SCHEMA["W-2045"]
        p = Path(d) / f
        j = json.loads(p.read_text())
        j[field] = spec
        p.write_text(json.dumps(j, indent=2))
        stale = score(d, "W-2045")
        good = stale["stale_generated"] and not stale["passed"]
        ok &= good
        print(f"obligation (schema edit without make gen fails CI): {'ok' if good else 'BROKEN'}")
    print("ALL OK" if ok else "FAILURES")
    return ok


if __name__ == "__main__":
    if sys.argv[1:] == ["verify"]:
        sys.exit(0 if verify() else 1)
    apply(sys.argv[1], sys.argv[2])
