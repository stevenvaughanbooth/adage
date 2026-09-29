#!/usr/bin/env python3
"""Generate dataclass models from schema files."""
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

    text = "\n".join(lines)
    if check:
        current = output.read_text() if output.exists() else ""
        if current != text:
            raise SystemExit("generated models are stale — run 'make gen'")
        return
    output.write_text(text)

if __name__ == "__main__":
    import sys
    generate_models(check="--check" in sys.argv)
