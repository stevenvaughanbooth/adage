"""Independent semantic issue groups and externally retained acceptance cases.

Variants share their group's identity. Entire groups, including all variants, have
one fixed split. This small synthetic mechanism pack is not customer savings evidence.
"""
import json

REPO_KEY = "github.com/adage-evals/pilot"
ORIGIN = "https://github.com/adage-evals/pilot"

# name, split, issue specification, buggy source, reference source, acceptance pairs.
GROUPS = [
    ("normalize-lines", "train", "Normalize a text document: remove blank lines, trim each remaining line, collapse every run of whitespace within a line to a single space, and join lines with a newline.",
     'def solve(value):\n    return "\\n".join(line.strip() for line in value.split("\\n"))\n',
     'def solve(value):\n    return "\\n".join(" ".join(line.split()) for line in value.split("\\n") if line.strip())\n',
     [("  alpha   beta \n\n gamma\t delta ", "alpha beta\ngamma delta"), ("", ""), (" \n\t", ""), ("a\nb", "a\nb"), ("\tx  y\n z ", "x y\nz"), ("hello\r\nworld", "hello\nworld")]),
    ("ascii-slug", "train", "Create an ASCII slug: lowercase ASCII letters, preserve digits, replace each run of all other characters with one hyphen, and remove leading/trailing hyphens. Non-ASCII letters are separators.",
     'def solve(value):\n    return value.lower().replace(" ", "-")\n',
     'def solve(value):\n    out = ""\n    for char in value:\n        if char in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789":\n            out += char.lower()\n        elif out and not out.endswith("-"):\n            out += "-"\n    return out.strip("-")\n',
     [(" Hello, WORLD! ", "hello-world"), ("A__B---C", "a-b-c"), ("", ""), ("café 24", "caf-24"), ("///", ""), ("Version 2.0", "version-2-0"), ("aKb", "a-b"), ("aİb", "a-b")]),
    ("stable-dedupe", "train", "Deduplicate a list of strings while preserving the order of their first appearance. Comparisons are case-sensitive; empty strings are valid values.",
     'def solve(value):\n    return sorted(set(value))\n',
     'def solve(value):\n    out = []\n    for item in value:\n        if item not in out:\n            out.append(item)\n    return out\n',
     [(["b", "a", "b", "c"], ["b", "a", "c"]), ([], []), (["A", "a", "A"], ["A", "a"]), (["", "z", ""], ["", "z"]), (["x", "x"], ["x"]), (["z", "b", "a"], ["z", "b", "a"])]),
    ("clamp-range", "train", "Clamp the integer value to inclusive lower and upper bounds. Input is an object with value, lower, upper; lower is always less than or equal to upper.",
     'def solve(value):\n    return min(value["value"], value["upper"])\n',
     'def solve(value):\n    return max(value["lower"], min(value["value"], value["upper"]))\n',
     [({"value": -5, "lower": 0, "upper": 10}, 0), ({"value": 20, "lower": 0, "upper": 10}, 10), ({"value": 4, "lower": 0, "upper": 10}, 4), ({"value": 3, "lower": 3, "upper": 3}, 3), ({"value": -20, "lower": -10, "upper": -2}, -10), ({"value": -3, "lower": -10, "upper": -2}, -3)]),
    ("boolean-parser", "train", "Parse a text flag after trimming whitespace and ignoring case. true/yes/on/1 mean true; false/no/off/0 mean false; anything else returns null.",
     'def solve(value):\n    return value == "true"\n',
     'def solve(value):\n    text = value.strip().lower()\n    if text in ["true", "yes", "on", "1"]:\n        return True\n    if text in ["false", "no", "off", "0"]:\n        return False\n    return None\n',
     [(" YES ", True), ("off", False), ("maybe", None), ("True", True), (" 0 ", False), ("", None), ("ON", True), (" No ", False)]),
    ("median", "train", "Return the median of an integer list. For an even length, return the arithmetic mean of the two central values; return null for an empty list.",
     'def solve(value):\n    if not value:\n        return None\n    return sorted(value)[len(value) // 2]\n',
     'def solve(value):\n    if not value:\n        return None\n    ordered = sorted(value)\n    mid = len(ordered) // 2\n    if len(ordered) % 2:\n        return ordered[mid]\n    return (ordered[mid - 1] + ordered[mid]) / 2\n',
     [([4, 1, 3, 2], 2.5), ([], None), ([9], 9), ([-5, -1], -3), ([8, 3, 2], 3), ([1, 1, 9, 9], 5)]),
    ("csv-row", "train", "Serialize a list of text fields into one CSV row, without a trailing newline. Fields containing comma, quote, CR, or LF must be enclosed in double quotes; embedded double quotes must be doubled. Other fields are emitted unchanged.",
     'def solve(value):\n    return ",".join(value)\n',
     'def solve(value):\n    out = []\n    for field in value:\n        if any(char in field for char in [",", "\\\"", "\\r", "\\n"]):\n            out.append("\\\"" + field.replace("\\\"", "\\\"\\\"") + "\\\"")\n        else:\n            out.append(field)\n    return ",".join(out)\n',
     [(["alpha", "b,c"], 'alpha,"b,c"'), (["say \"hi\""], '"say ""hi"""'), ([], ""), (["", "x"], ",x"), (["a\nb", "c"], '"a\nb",c'), (["a\rb"], '"a\rb"')]),
    ("balanced-brackets", "train", "Determine whether (), [], and {} are balanced and correctly nested in a string. Ignore all other characters. The empty string is balanced.",
     'def solve(value):\n    return value.count("(") == value.count(")") and value.count("[") == value.count("]") and value.count("{") == value.count("}")\n',
     'def solve(value):\n    stack = []\n    pairs = {")": "(", "]": "[", "}": "{"}\n    for char in value:\n        if char in "([{":\n            stack.append(char)\n        elif char in pairs:\n            if not stack or stack.pop() != pairs[char]:\n                return False\n    return not stack\n',
     [("([)]", False), ("a{b[c](d)}", True), ("", True), (")(", False), ("((", False), ("[()]{}", True), ("plain", True)]),
    ("chunk-list", "validation", "Split input.items into consecutive chunks of input.size, preserving order and keeping the final partial chunk. size is a positive integer; an empty items list returns an empty list.",
     'def solve(value):\n    size = value["size"]\n    items = value["items"]\n    return [items[i:i + size] for i in range(0, len(items) - size + 1, size)]\n',
     'def solve(value):\n    size = value["size"]\n    items = value["items"]\n    return [items[i:i + size] for i in range(0, len(items), size)]\n',
     [({"items": [1, 2, 3, 4, 5], "size": 2}, [[1, 2], [3, 4], [5]]), ({"items": [], "size": 3}, []), ({"items": [1], "size": 5}, [[1]]), ({"items": ["a", "b"], "size": 1}, [["a"], ["b"]]), ({"items": [1, 2, 3, 4], "size": 2}, [[1, 2], [3, 4]]), ({"items": [0, 0, 0], "size": 2}, [[0, 0], [0]])]),
    ("inventory-deltas", "validation", "Aggregate an ordered list of inventory adjustments. Each object has sku and integer delta. Return an object mapping each SKU to the sum of its deltas, omitting SKUs whose final sum is zero.",
     'def solve(value):\n    return {item["sku"]: item["delta"] for item in value}\n',
     'def solve(value):\n    totals = {}\n    for item in value:\n        sku = item["sku"]\n        totals[sku] = totals.get(sku, 0) + item["delta"]\n    return {sku: total for sku, total in totals.items() if total != 0}\n',
     [([{"sku": "A", "delta": 3}, {"sku": "A", "delta": -1}], {"A": 2}), ([], {}), ([{"sku": "Z", "delta": 0}], {}), ([{"sku": "X", "delta": 2}, {"sku": "X", "delta": -2}], {}), ([{"sku": "a", "delta": -4}, {"sku": "b", "delta": 2}], {"a": -4, "b": 2}), ([{"sku": "a", "delta": 1}, {"sku": "A", "delta": 1}], {"a": 1, "A": 1})]),
    ("merge-intervals", "test", "Merge closed integer intervals. Input is a list of [start,end] with start <= end. Return sorted disjoint intervals, merging overlaps and intervals that touch at an endpoint. Empty input returns an empty list.",
     'def solve(value):\n    return sorted(value)\n',
     'def solve(value):\n    out = []\n    for interval in sorted(value):\n        start, end = interval\n        if out and start <= out[-1][1]:\n            out[-1][1] = max(out[-1][1], end)\n        else:\n            out.append([start, end])\n    return out\n',
     [([[5, 7], [1, 3], [3, 6]], [[1, 7]]), ([], []), ([[1, 2], [4, 5]], [[1, 2], [4, 5]]), ([[1, 10], [3, 4]], [[1, 10]]), ([[-5, -2], [-3, 0]], [[-5, 0]]), ([[2, 2], [2, 2]], [[2, 2]])]),
    ("discounted-total", "test", "Compute an invoice total in integer cents. Input has items (unit_cents and quantity, both nonnegative integers) and discount_bps (0 through 10000). Sum all line subtotals, apply the discount once to the sum, then round half up to the nearest cent using integer arithmetic. Empty items total zero.",
     'def solve(value):\n    subtotal = sum(item["unit_cents"] * item["quantity"] for item in value["items"])\n    return subtotal * (10000 - value["discount_bps"]) // 10000\n',
     'def solve(value):\n    subtotal = sum(item["unit_cents"] * item["quantity"] for item in value["items"])\n    return (subtotal * (10000 - value["discount_bps"]) + 5000) // 10000\n',
     [({"items": [{"unit_cents": 101, "quantity": 1}], "discount_bps": 5000}, 51), ({"items": [], "discount_bps": 3000}, 0), ({"items": [{"unit_cents": 125, "quantity": 3}], "discount_bps": 2000}, 300), ({"items": [{"unit_cents": 99, "quantity": 2}], "discount_bps": 10000}, 0), ({"items": [{"unit_cents": 1, "quantity": 1}, {"unit_cents": 1, "quantity": 1}], "discount_bps": 5000}, 1), ({"items": [{"unit_cents": 999, "quantity": 0}], "discount_bps": 0}, 0)]),
]


DOCS = [
    ("docs-timeout", "train", "The configuration guide incorrectly lists a 5 second default timeout. Replace `Default timeout: 5 seconds.` with `Default timeout: 30 seconds.` in docs/guide.md; preserve the rest of the document.",
     "# Configuration guide\n\nDefault timeout: 5 seconds.\nRetries: 2.\n",
     "# Configuration guide\n\nDefault timeout: 30 seconds.\nRetries: 2.\n"),
    ("docs-endpoint", "train", "The API guide has the wrong plural route. Replace `GET /v1/item` with `GET /v1/items` in docs/guide.md; preserve the rest of the document.",
     "# API guide\n\nList items: GET /v1/item\nResponse: JSON array.\n",
     "# API guide\n\nList items: GET /v1/items\nResponse: JSON array.\n"),
    ("docs-support", "test", "The support guide lists a retired mailbox. Replace `help@old.example` with `support@example.test` in docs/guide.md; preserve the rest of the document.",
     "# Support guide\n\nContact: help@old.example\nHours: weekdays 09:00-17:00 UTC.\n",
     "# Support guide\n\nContact: support@example.test\nHours: weekdays 09:00-17:00 UTC.\n"),
]


def manifest():
    return {"schema": 1, "repo": REPO_KEY, "evidence_kind": "synthetic_mechanism_pilot",
            "tasks": {f"{split}-{name}-{variant:02d}": {
                "split": split, "group_id": f"pilot/{name}", "variant": variant,
                "label": name, "prompt": "Read TASK.md and complete the requested fix."}
                for name, split, *_ in [*GROUPS, *DOCS] for variant in (1, 2)}}


def task(task_id):
    entry = manifest()["tasks"].get(task_id)
    if entry is None:
        raise ValueError(f"unknown task: {task_id}")
    if entry["label"].startswith("docs-"):
        name, split, description, before, after = next(row for row in DOCS if row[0] == entry["label"])
        source = "def solve(value):\n    return value\n"
        return {**entry, "id": task_id, "kind": "documentation", "description": description,
                "buggy": source, "reference": source, "cases": [(1, 1), ([], []), ({"x": 2}, {"x": 2})],
                "visible": [{"input": 1, "expected": 1}, {"input": [1, 2], "expected": [1, 2]}],
                "doc_before": before, "doc_after": after}
    row = next(row for row in GROUPS if row[0] == entry["label"])
    name, split, description, buggy, reference, cases = row
    # Variants expose different examples but remain members of the same issue group.
    visible_indices = (0, 1, 2) if entry["variant"] == 1 else (0, 3, 4)
    visible = [{"input": cases[index][0], "expected": cases[index][1]} for index in visible_indices]
    return {**entry, "id": task_id, "kind": "code", "description": description, "buggy": buggy,
            "reference": reference, "cases": cases, "visible": visible}


def dump(value):
    return json.dumps(value, sort_keys=True, indent=2, ensure_ascii=False, allow_nan=False) + "\n"
