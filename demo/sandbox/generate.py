#!/usr/bin/env python3
"""Generate acme-billing sandbox repository with seeded bugs."""

import json
import os
import sys
import subprocess
from pathlib import Path
from textwrap import dedent

def create_sandbox(out_dir):
    """Generate acme-billing sandbox with 12 work items."""
    out_path = Path(out_dir)
    out_path.mkdir(parents=True, exist_ok=True)

    # Create directories
    (out_path / ".tracker").mkdir(exist_ok=True)
    (out_path / "schema").mkdir(exist_ok=True)
    (out_path / "scripts").mkdir(exist_ok=True)
    (out_path / "tools").mkdir(exist_ok=True)
    (out_path / "acme" / "generated").mkdir(parents=True, exist_ok=True)
    (out_path / "tests").mkdir(exist_ok=True)

    # README
    (out_path / "README.md").write_text(dedent("""\
        # acme-billing

        Billing and customer management for ACME Corporation.

        Work items are tracked in the internal tracker.
        Development tasks are defined in the Makefile.
        """))

    # Makefile
    (out_path / "Makefile").write_text(dedent("""\
        .PHONY: gen test ci lint

        gen:
        	python3 tools/gen.py

        test:
        	ACME_ENV=test python3 -m unittest discover -s tests

        ci:
        	@python3 tools/gen.py --check
        	ACME_ENV=test python3 -m unittest discover -s tests

        lint:
        	python3 -m py_compile acme/*.py acme/generated/*.py tests/*.py
        """))

    # Work items tracker
    items = {
        "W-1041": {
            "id": "W-1041",
            "title": "Fix discount rounding",
            "description": "Discount calculations are losing cents. When a 10% discount on $99.99 is calculated, it should round to nearest cent using banker's rounding (round half to even), not floor.",
            "linked_files": ["acme/billing.py"],
            "acceptance": "10% discount on $99.99 cents yields $89.99, not $89.98"
        },
        "W-1042": {
            "id": "W-1042",
            "title": "EU tax region uses wrong rate",
            "description": "Invoices for customers in the EU are charged at 15% tax, but should be 20%.",
            "linked_files": ["acme/billing.py"],
            "acceptance": "EU region tax rate is 20%, not 15%"
        },
        "W-1043": {
            "id": "W-1043",
            "title": "Late fees applied too early",
            "description": "Late fees are being applied on the due date, but should only apply after the due date has passed.",
            "linked_files": ["acme/billing.py"],
            "acceptance": "Late fee applies only when payment is after due_date, not on it"
        },
        "W-1044": {
            "id": "W-1044",
            "title": "Email validation too lenient",
            "description": "Email validation accepts addresses like 'user@localhost' without a dot in the domain. Should require at least one dot.",
            "linked_files": ["acme/customers.py"],
            "acceptance": "Email must have domain with dot: 'user@localhost' rejected, 'user@example.com' accepted"
        },
        "W-1045": {
            "id": "W-1045",
            "title": "Add currency field to invoices",
            "description": "Invoices need to track currency. Add a 'currency' string field with default 'USD' to the invoice schema. Use it in format_invoice output.",
            "linked_files": ["schema/invoice.json", "acme/billing.py"],
            "acceptance": "Invoice schema has currency field (default USD), and format_invoice includes it in output"
        },
        "W-1046": {
            "id": "W-1046",
            "title": "Add configurable due days",
            "description": "Invoice due date should be configurable per invoice. Add 'due_days' int field with default 30 to invoice schema and use it in due_date calculation.",
            "linked_files": ["schema/invoice.json", "acme/billing.py"],
            "acceptance": "Invoice schema has due_days field, and due_date uses it instead of hardcoded 30"
        },
        "W-2041": {
            "id": "W-2041",
            "title": "Fix VAT calculation precision",
            "description": "VAT calculations on large invoices lose precision. When calculating 20% tax on $10000, result should be exactly $2000.00, not $1999.99.",
            "linked_files": ["acme/billing.py"],
            "acceptance": "20% tax on $10000.00 yields exactly $2000.00 with proper rounding"
        },
        "W-2042": {
            "id": "W-2042",
            "title": "Customer region lookup case-sensitive",
            "description": "Customer region lookup fails when region case doesn't match. 'us-west' and 'US-WEST' should both work.",
            "linked_files": ["acme/customers.py"],
            "acceptance": "get_tax_rate handles region lookup case-insensitively"
        },
        "W-2043": {
            "id": "W-2043",
            "title": "Discount cannot exceed 100%",
            "description": "Discount percentage validation allows discounts >= 100%, resulting in negative or zero totals. Should reject discounts >= 100%.",
            "linked_files": ["acme/billing.py"],
            "acceptance": "discount >= 100% raises ValueError before calculation"
        },
        "W-2044": {
            "id": "W-2044",
            "title": "Empty name validation",
            "description": "Customer name validation accepts empty strings. Should require at least one character.",
            "linked_files": ["acme/customers.py"],
            "acceptance": "Empty customer name raises ValueError during validation"
        },
        "W-2045": {
            "id": "W-2045",
            "title": "Add payment terms",
            "description": "Add 'payment_terms' string field (default 'NET30') to invoice schema and use in payment deadline calculation.",
            "linked_files": ["schema/invoice.json", "acme/billing.py"],
            "acceptance": "Invoice schema has payment_terms field, used in deadline calculation"
        },
        "W-2046": {
            "id": "W-2046",
            "title": "Add customer tier field",
            "description": "Add 'tier' string field (default 'standard') to customer schema. Use it for discount eligibility checks.",
            "linked_files": ["schema/customer.json", "acme/customers.py"],
            "acceptance": "Customer schema has tier field, used in validation logic"
        }
    }

    # The tracker is an external service in real life; locally its cache is opaque, so the
    # realistic way in is the CLI (scripts/wi-lookup), not reading a JSON file.
    import base64, zlib
    (out_path / ".tracker" / "cache.bin").write_bytes(base64.b64encode(zlib.compress(json.dumps(items).encode())))

    # Schema files - invoice (with bugs)
    invoice_schema = {
        "id": {"type": "str", "default": None},
        "customer_id": {"type": "str", "default": None},
        "amount_cents": {"type": "int", "default": 0},
        "tax_region": {"type": "str", "default": "US"},
        "discount_pct": {"type": "float", "default": 0.0}
    }
    (out_path / "schema" / "invoice.json").write_text(json.dumps(invoice_schema, indent=2))

    # Schema files - customer
    customer_schema = {
        "id": {"type": "str", "default": None},
        "name": {"type": "str", "default": ""},
        "email": {"type": "str", "default": ""},
        "region": {"type": "str", "default": "US"}
    }
    (out_path / "schema" / "customer.json").write_text(json.dumps(customer_schema, indent=2))

    # tools/gen.py
    (out_path / "tools" / "gen.py").write_text(dedent("""\
        #!/usr/bin/env python3
        \"\"\"Generate dataclass models from schema files.\"\"\"
        import json
        from pathlib import Path
        from dataclasses import dataclass, field

        def load_schema(path):
            with open(path) as f:
                return json.load(f)

        def generate_models(check=False):
            schema_dir = Path("schema")
            output = Path("acme/generated/models.py")

            lines = ["from dataclasses import dataclass, field"]
            lines.append("")

            for schema_file in sorted(schema_dir.glob("*.json")):
                schema = load_schema(schema_file)
                name = schema_file.stem.capitalize()
                lines.append(f"@dataclass")
                lines.append(f"class {name}:")
                for field_name, field_def in schema.items():
                    ftype = field_def["type"]
                    default = field_def.get("default")
                    if default is None:
                        lines.append(f"    {field_name}: {ftype}")
                    else:
                        if isinstance(default, str):
                            default_repr = repr(default)
                        else:
                            default_repr = default
                        lines.append(f"    {field_name}: {ftype} = {default_repr}")
                lines.append("")

            text = "\\n".join(lines)
            if check:
                current = output.read_text() if output.exists() else ""
                if current != text:
                    raise SystemExit("generated models are stale — run 'make gen'")
                return
            output.write_text(text)

        if __name__ == "__main__":
            import sys
            generate_models(check="--check" in sys.argv)
        """))
    (out_path / "tools" / "gen.py").chmod(0o755)

    # acme/__init__.py
    (out_path / "acme" / "__init__.py").write_text("")

    # acme/billing.py with bugs
    (out_path / "acme" / "billing.py").write_text(dedent("""\
        \"\"\"Billing calculations with seeded bugs.\"\"\"
        import math
        from datetime import datetime, timedelta

        # Bug in W-1041: uses floor instead of banker's rounding
        def apply_discount(amount_cents, discount_pct):
            discount_amount = amount_cents * (discount_pct / 100)
            return int(math.floor(discount_amount))  # Bug: should use round

        # Bug in W-1042: EU region uses 15% instead of 20%
        def get_tax_rate(region):
            rates = {
                "US": 0.08,
                "EU": 0.15,  # Bug: should be 0.20
                "CA": 0.13
            }
            return rates.get(region.upper(), 0.08)

        # Bug in W-1043: includes due date, should be after
        def is_late(due_date, payment_date):
            return payment_date >= due_date  # Bug: should be >

        def calculate_late_fee(amount_cents, days_late):
            if days_late > 0:
                return amount_cents * 0.01 * days_late
            return 0

        def calculate_total(amount_cents, discount_pct, region):
            discounted = amount_cents - apply_discount(amount_cents, discount_pct)
            tax = discounted * get_tax_rate(region)
            return discounted + tax

        def format_invoice(invoice_dict):
            # Bug in W-2041: uses integer division for cents, losing precision
            amount_dollars = invoice_dict['amount_cents'] // 100
            return f"Invoice {invoice_dict['id']}: ${amount_dollars}"
        """))

    # acme/customers.py with bugs
    (out_path / "acme" / "customers.py").write_text(dedent("""\
        \"\"\"Customer management with seeded bugs.\"\"\"

        # Bug in W-1044: no dot requirement in domain
        def validate_email(email):
            if "@" not in email:
                raise ValueError("Invalid email: missing @")
            return True  # Bug: should check for dot in domain

        def validate_customer(name, email, region):
            if not name:  # Bug in W-2044: accepts empty string
                pass
            validate_email(email)
            return True

        # Bug in W-2042: case-sensitive region lookup
        def get_tax_rate(region):
            rates = {
                "us": 0.08,
                "eu": 0.20,
                "ca": 0.13
            }
            return rates.get(region, 0.0)  # Bug: should be region.lower()
        """))

    # acme/generated/__init__.py
    (out_path / "acme" / "generated" / "__init__.py").write_text("")

    # acme/generated/models.py (generated from schema)
    models_content = dedent("""\
        from dataclasses import dataclass, field

        @dataclass
        class Customer:
            id: str
            name: str = ""
            email: str = ""
            region: str = "US"

        @dataclass
        class Invoice:
            id: str
            customer_id: str
            amount_cents: int = 0
            tax_region: str = "US"
            discount_pct: float = 0.0
        """)
    (out_path / "acme" / "generated" / "models.py").write_text(models_content)

    # tests/__init__.py with guard
    (out_path / "tests" / "__init__.py").write_text(dedent("""\
        import os

        if os.environ.get("ACME_ENV") != "test":
            raise RuntimeError("ACME_ENV is not set — run tests with 'make test'")
        """))

    # tests/test_billing.py
    (out_path / "tests" / "test_billing.py").write_text(dedent("""\
        import tests
        import unittest
        from acme import billing

        class TestBilling(unittest.TestCase):
            def test_tax_rates(self):
                self.assertEqual(billing.get_tax_rate("US"), 0.08)
                self.assertEqual(billing.get_tax_rate("CA"), 0.13)

            def test_discount_basic(self):
                result = billing.apply_discount(10000, 10)
                self.assertGreater(result, 0)
        """))

    # tests/test_customers.py
    (out_path / "tests" / "test_customers.py").write_text(dedent("""\
        import tests
        import unittest
        from acme import customers

        class TestCustomers(unittest.TestCase):
            def test_email_validation(self):
                customers.validate_email("user@example.com")
                with self.assertRaises(ValueError):
                    customers.validate_email("notanemail")
        """))

    # scripts/wi-lookup
    (out_path / "scripts" / "wi-lookup").write_text(dedent("""\
        #!/usr/bin/env python3
        # Look up a work item in the internal tracker.
        import base64
        import json
        import sys
        import zlib
        from pathlib import Path

        _cache = Path(__file__).resolve().parent.parent / ".tracker" / "cache.bin"
        items = json.loads(zlib.decompress(base64.b64decode(_cache.read_bytes())))

        if len(sys.argv) < 2:
            print("Usage: wi-lookup <ID> [--json]")
            sys.exit(1)

        item_id = sys.argv[1]
        if item_id not in items:
            print(f"Work item {item_id} not found")
            sys.exit(1)

        item = items[item_id]
        if "--json" in sys.argv:
            print(json.dumps(item, indent=2))
        else:
            print(f"ID: {item['id']}")
            print(f"Title: {item['title']}")
            print(f"Description: {item['description']}")
            print(f"Linked files: {', '.join(item['linked_files'])}")
            print(f"Acceptance: {item['acceptance']}")
        """))
    (out_path / "scripts" / "wi-lookup").chmod(0o755)

    # Initialize as git repo
    os.chdir(out_path)
    # Generated models must match the generator exactly, so a fresh sandbox passes `make ci`.
    subprocess.run(["python3", "tools/gen.py"], cwd=out_path, check=True)
    subprocess.run(["git", "init"], capture_output=True)
    subprocess.run(["git", "config", "user.email", "test@example.com"], capture_output=True)
    subprocess.run(["git", "config", "user.name", "Test User"], capture_output=True)
    subprocess.run(["git", "add", "."], capture_output=True)
    subprocess.run(["git", "commit", "-m", "Initial commit"], capture_output=True)

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("Usage: generate.py <output_dir>")
        sys.exit(1)
    create_sandbox(sys.argv[1])
    print(f"Generated sandbox in {sys.argv[1]}")
