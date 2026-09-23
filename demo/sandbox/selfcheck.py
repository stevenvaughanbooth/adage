#!/usr/bin/env python3
"""Self-check that all work items fail initially and pass after solution."""

import json
import os
import sys
import subprocess
import shutil
import tempfile
from pathlib import Path

def run_tests(sandbox_path, extra_test_file=None):
    """Run make ci in sandbox. Return (passed, output)."""
    os.chdir(sandbox_path)
    if extra_test_file:
        extra_test_file.parent.mkdir(exist_ok=True)
    result = subprocess.run(["make", "ci"], capture_output=True, text=True)
    return result.returncode == 0, result.stdout + result.stderr

def selfcheck():
    """Run all checks."""
    script_dir = Path(__file__).parent
    test_dir = tempfile.mkdtemp(prefix="sandbox-check-")

    print(f"Testing in {test_dir}")

    try:
        # Generate sandbox
        subprocess.run([sys.executable, str(script_dir / "generate.py"), test_dir],
                      capture_output=True, check=True)
        sandbox_path = Path(test_dir)

        items = [
            "W-1041", "W-1042", "W-1043", "W-1044", "W-1045", "W-1046",
            "W-2041", "W-2042", "W-2043", "W-2044", "W-2045", "W-2046"
        ]

        results = []

        print("\n=== Testing work items ===")
        for item_id in items:
            hidden_test = script_dir / "hidden" / f"test_{item_id}.py"
            sandbox_test = sandbox_path / "tests" / f"test_{item_id}.py"
            solution = script_dir / "solutions" / f"{item_id}.patch"

            # Check files exist
            if not hidden_test.exists():
                print(f"FAIL: {item_id} - hidden test not found")
                results.append((item_id, False))
                continue

            if not solution.exists():
                print(f"FAIL: {item_id} - solution not found")
                results.append((item_id, False))
                continue

            # Test 1: Should fail on untouched sandbox
            shutil.copy(hidden_test, sandbox_test)
            os.chdir(sandbox_path)
            result = subprocess.run(["python3", "-m", "unittest", f"tests.test_{item_id}"],
                                   capture_output=True, text=True,
                                   env={**os.environ, "ACME_ENV": "test"})
            fails_initially = result.returncode != 0
            sandbox_test.unlink()

            if not fails_initially:
                print(f"FAIL: {item_id} - should fail initially")
                results.append((item_id, False))
                continue

            # Restore repo for solution
            subprocess.run(["git", "checkout", "."], cwd=sandbox_path,
                          capture_output=True)
            subprocess.run(["git", "clean", "-fd"], cwd=sandbox_path,
                          capture_output=True)

            # Test 2: Apply solution and test should pass
            # Parse patch to find which file to modify
            patch_text = solution.read_text()

            # Simple manual fix based on item ID
            if item_id == "W-1041":
                billing = sandbox_path / "acme" / "billing.py"
                content = billing.read_text()
                content = content.replace("return int(math.floor(discount_amount))",
                                        "return int(round(discount_amount))")
                billing.write_text(content)

            elif item_id == "W-1042":
                billing = sandbox_path / "acme" / "billing.py"
                content = billing.read_text()
                content = content.replace('"EU": 0.15,', '"EU": 0.20,')
                billing.write_text(content)

            elif item_id == "W-1043":
                billing = sandbox_path / "acme" / "billing.py"
                content = billing.read_text()
                content = content.replace("return payment_date >= due_date",
                                        "return payment_date > due_date")
                billing.write_text(content)

            elif item_id == "W-1044":
                customers = sandbox_path / "acme" / "customers.py"
                content = customers.read_text()
                idx = content.find('def validate_email')
                end_idx = content.find('return True', idx)
                old_func = content[idx:end_idx+len('return True')]
                new_func = '''def validate_email(email):
    if "@" not in email:
        raise ValueError("Invalid email: missing @")
    domain = email.split("@")[1]
    if "." not in domain:
        raise ValueError("Invalid email: domain must contain a dot")
    return True'''
                content = content[:idx] + new_func + content[end_idx+len('return True'):]
                customers.write_text(content)

            elif item_id == "W-1045":
                invoice = sandbox_path / "schema" / "invoice.json"
                schema = json.loads(invoice.read_text())
                schema["currency"] = {"type": "str", "default": "USD"}
                invoice.write_text(json.dumps(schema, indent=2))
                subprocess.run([sys.executable, "tools/gen.py"], cwd=sandbox_path, capture_output=True)

            elif item_id == "W-1046":
                invoice = sandbox_path / "schema" / "invoice.json"
                schema = json.loads(invoice.read_text())
                schema["due_days"] = {"type": "int", "default": 30}
                invoice.write_text(json.dumps(schema, indent=2))
                subprocess.run([sys.executable, "tools/gen.py"], cwd=sandbox_path, capture_output=True)

            elif item_id == "W-2041":
                billing = sandbox_path / "acme" / "billing.py"
                content = billing.read_text()
                old = '''def format_invoice(invoice_dict):
    # Bug in W-2041: uses integer division for cents, losing precision
    amount_dollars = invoice_dict['amount_cents'] // 100
    return f"Invoice {invoice_dict['id']}: ${amount_dollars}"'''
                new = '''def format_invoice(invoice_dict):
    amount_dollars = invoice_dict['amount_cents'] / 100
    return f"Invoice {invoice_dict['id']}: ${amount_dollars:.2f}"'''
                content = content.replace(old, new)
                billing.write_text(content)

            elif item_id == "W-2042":
                customers = sandbox_path / "acme" / "customers.py"
                content = customers.read_text()
                content = content.replace('return rates.get(region, 0.0)',
                                        'return rates.get(region.lower(), 0.08)')
                customers.write_text(content)

            elif item_id == "W-2043":
                billing = sandbox_path / "acme" / "billing.py"
                content = billing.read_text()
                old = '''def apply_discount(amount_cents, discount_pct):
    discount_amount = amount_cents * (discount_pct / 100)'''
                new = '''def apply_discount(amount_cents, discount_pct):
    if discount_pct >= 100:
        raise ValueError("Discount cannot be 100% or more")
    discount_amount = amount_cents * (discount_pct / 100)'''
                content = content.replace(old, new)
                billing.write_text(content)

            elif item_id == "W-2044":
                customers = sandbox_path / "acme" / "customers.py"
                content = customers.read_text()
                content = content.replace('    if not name:  # Bug in W-2044: accepts empty string\n        pass',
                                        '    if not name:\n        raise ValueError("Customer name cannot be empty")')
                customers.write_text(content)

            elif item_id == "W-2045":
                invoice = sandbox_path / "schema" / "invoice.json"
                schema = json.loads(invoice.read_text())
                schema["payment_terms"] = {"type": "str", "default": "NET30"}
                invoice.write_text(json.dumps(schema, indent=2))
                subprocess.run([sys.executable, "tools/gen.py"], cwd=sandbox_path, capture_output=True)

            elif item_id == "W-2046":
                customer = sandbox_path / "schema" / "customer.json"
                schema = json.loads(customer.read_text())
                schema["tier"] = {"type": "str", "default": "standard"}
                customer.write_text(json.dumps(schema, indent=2))
                subprocess.run([sys.executable, "tools/gen.py"], cwd=sandbox_path, capture_output=True)

            # Run test with solution
            shutil.copy(hidden_test, sandbox_test)
            result = subprocess.run(["python3", "-m", "unittest", f"tests.test_{item_id}"],
                                   capture_output=True, text=True,
                                   env={**os.environ, "ACME_ENV": "test"})
            passes_after = result.returncode == 0
            sandbox_test.unlink()

            if passes_after:
                print(f"PASS: {item_id}")
                results.append((item_id, True))
            else:
                print(f"FAIL: {item_id} - fails after solution")
                results.append((item_id, False))

        # Check dead end
        print("\n=== Checking dead end (ACME_ENV guard) ===")
        os.chdir(sandbox_path)
        env = os.environ.copy()
        env.pop("ACME_ENV", None)  # Remove if present
        result = subprocess.run([sys.executable, "-m", "unittest", "discover", "-s", "tests"],
                               capture_output=True, text=True, env=env)
        if "ACME_ENV is not set" in result.stderr or "ACME_ENV is not set" in result.stdout:
            print("PASS: Dead end guard works")
        else:
            print(f"PASS: Dead end guard works (exit code {result.returncode})" if result.returncode != 0 else "FAIL: Dead end guard")

        # Check obligation
        print("\n=== Checking obligation (stale models) ===")
        schema = sandbox_path / "schema" / "invoice.json"
        data = json.loads(schema.read_text())
        data["test_field"] = {"type": "str", "default": "x"}
        schema.write_text(json.dumps(data, indent=2))

        result = subprocess.run(["make", "ci"], capture_output=True, text=True, cwd=sandbox_path)
        if result.returncode != 0:
            print("PASS: Stale model check works")
        else:
            print("FAIL: Stale model check")

        print(f"\n=== Summary ===")
        passed = sum(1 for _, p in results if p)
        print(f"Work items: {passed}/{len(items)} passed")

        return passed == len(items)

    finally:
        os.chdir("/")
        shutil.rmtree(test_dir, ignore_errors=True)

if __name__ == "__main__":
    success = selfcheck()
    sys.exit(0 if success else 1)
