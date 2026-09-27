"""The bug catalog: small functions with one realistic defect each, in every supported language.

Each template is a correct function, a mutated version of it (the seeded bug), a hidden test that
fails on the mutation and passes on the correct code, and the words a tracker item (or a control
prompt) uses to describe the problem. A domain skin gives each template its function name, a noun
phrase and field names, so the same defect reads naturally in every domain.

Code uses `@@key@@` placeholders (braces are common in the code itself):
  @@fn@@      the function name in the stack's naming style
  @@field@@   a record field (sum_all, sort_desc)
  @@const@@   a constant table name (ci_lookup)
  @@table@@   the constant table's literal (ci_lookup)
  @@noun@@    a noun for error messages (require_name)
Text uses the same placeholders plus @@name@@ (the function as written in prose) and @@Phrase@@ /
@@phrase@@ (the skin's noun phrase).

Adding a language: add a `<lang>_good` / `<lang>_bad` / `<lang>_test` entry to every template.
"""

import re
from dataclasses import dataclass


def fill(text, **kw):
    for k, v in kw.items():
        text = text.replace(f"@@{k}@@", str(v))
    left = re.findall(r"@@\w+@@", text)
    if left:
        raise ValueError(f"unfilled placeholders {left} in {text[:80]!r}")
    return text


def camel(snake):
    head, *rest = snake.split("_")
    return head + "".join(w[:1].upper() + w[1:] for w in rest)


@dataclass(frozen=True)
class Template:
    id: str
    title: str          # tracker title
    description: str    # tracker description (no path-like text: `adage learn` reads paths from it)
    acceptance: str
    control: str        # a self-contained prompt fragment for control tasks: the problem, precisely
    py_good: str
    py_bad: str
    py_test: str        # unittest method bodies; `m` is the module
    js_good: str
    js_bad: str
    js_test: str        # node:test bodies; `m` is the module namespace


T = [
    Template(
        "pct_round",
        "@@Phrase@@ are truncated instead of rounded",
        "@@name@@ drops the fraction instead of rounding to the nearest whole unit. 15 percent of 999 "
        "comes out as 149 when it should be 150.",
        "@@name@@(999, 15) returns 150 and @@name@@(1999, 10) returns 200.",
        "@@name@@ truncates instead of rounding to the nearest whole unit: @@name@@(999, 15) must return 150, not 149",
        'def @@fn@@(amount, pct):\n    return round(amount * pct / 100)\n',
        'def @@fn@@(amount, pct):\n    return int(amount * pct / 100)\n',
        "self.assertEqual(m.@@fn@@(999, 15), 150)\nself.assertEqual(m.@@fn@@(1999, 10), 200)\nself.assertEqual(m.@@fn@@(1000, 15), 150)",
        "export function @@fn@@(amount, pct) {\n  return Math.round((amount * pct) / 100);\n}\n",
        "export function @@fn@@(amount, pct) {\n  return Math.trunc((amount * pct) / 100);\n}\n",
        "assert.equal(m.@@fn@@(999, 15), 150);\nassert.equal(m.@@fn@@(1999, 10), 200);\nassert.equal(m.@@fn@@(1000, 15), 150);",
    ),
    Template(
        "after_boundary",
        "@@Phrase@@: a value equal to the limit counts as over it",
        "@@name@@(limit, value) treats a value exactly at the limit as past it. Only values strictly "
        "greater than the limit should count.",
        "@@name@@(10, 10) is false, @@name@@(10, 11) is true and @@name@@(10, 9) is false.",
        "@@name@@(limit, value) returns true when the value equals the limit; only values strictly greater than the limit should count",
        'def @@fn@@(limit, value):\n    return value > limit\n',
        'def @@fn@@(limit, value):\n    return value >= limit\n',
        "self.assertFalse(m.@@fn@@(10, 10))\nself.assertTrue(m.@@fn@@(10, 11))\nself.assertFalse(m.@@fn@@(10, 9))",
        "export function @@fn@@(limit, value) {\n  return value > limit;\n}\n",
        "export function @@fn@@(limit, value) {\n  return value >= limit;\n}\n",
        "assert.equal(m.@@fn@@(10, 10), false);\nassert.equal(m.@@fn@@(10, 11), true);\nassert.equal(m.@@fn@@(10, 9), false);",
    ),
    Template(
        "ci_lookup",
        "@@Phrase@@ lookup misses codes typed in lower case",
        "@@name@@ finds nothing for a code typed in lower case or with stray spaces around it. Codes "
        "should match after trimming and upper-casing, the same way everywhere else.",
        "@@name@@ with ' @@lc@@ ' returns the same value as with '@@uc@@', and an unknown code still returns nothing.",
        "@@name@@ does not match codes typed in lower case or with surrounding spaces; it should trim and upper-case the code before looking it up",
        '@@const@@ = @@table@@\n\n\ndef @@fn@@(code):\n    return @@const@@.get(normalize_code(code))\n',
        '@@const@@ = @@table@@\n\n\ndef @@fn@@(code):\n    return @@const@@.get(code.strip())\n',
        "self.assertEqual(m.@@fn@@(' @@lc@@ '), m.@@const@@['@@uc@@'])\nself.assertEqual(m.@@fn@@('@@uc@@'), m.@@const@@['@@uc@@'])\nself.assertIsNone(m.@@fn@@('nope'))",
        "export const @@const@@ = Object.freeze(@@table@@);\n\nexport function @@fn@@(code) {\n  return @@const@@[normalizeCode(code)] ?? null;\n}\n",
        "export const @@const@@ = Object.freeze(@@table@@);\n\nexport function @@fn@@(code) {\n  return @@const@@[code.trim()] ?? null;\n}\n",
        "assert.equal(m.@@fn@@(' @@lc@@ '), m.@@const@@['@@uc@@']);\nassert.equal(m.@@fn@@('@@uc@@'), m.@@const@@['@@uc@@']);\nassert.equal(m.@@fn@@('nope'), null);",
    ),
    Template(
        "clamp_hi",
        "@@Phrase@@ are not capped at the upper bound",
        "@@name@@(value, lo, hi) keeps the lower bound but lets values run past the upper one. A value "
        "above the upper bound should come back as the upper bound.",
        "@@name@@(15, 0, 10) returns 10, @@name@@(-3, 0, 10) returns 0 and @@name@@(5, 0, 10) returns 5.",
        "@@name@@(value, lo, hi) does not cap values at hi: @@name@@(15, 0, 10) must return 10",
        'def @@fn@@(value, lo, hi):\n    return max(lo, min(value, hi))\n',
        'def @@fn@@(value, lo, hi):\n    return max(lo, value)\n',
        "self.assertEqual(m.@@fn@@(15, 0, 10), 10)\nself.assertEqual(m.@@fn@@(-3, 0, 10), 0)\nself.assertEqual(m.@@fn@@(5, 0, 10), 5)",
        "export function @@fn@@(value, lo, hi) {\n  return Math.max(lo, Math.min(value, hi));\n}\n",
        "export function @@fn@@(value, lo, hi) {\n  return Math.max(lo, value);\n}\n",
        "assert.equal(m.@@fn@@(15, 0, 10), 10);\nassert.equal(m.@@fn@@(-3, 0, 10), 0);\nassert.equal(m.@@fn@@(5, 0, 10), 5);",
    ),
    Template(
        "sum_all",
        "@@Phrase@@ total leaves out the last entry",
        "@@name@@ adds up the @@field@@ of every entry except the last one, so every total is short. "
        "All entries should be counted.",
        "@@name@@ over entries with @@field@@ 2, 3 and 4 returns 9, and over no entries returns 0.",
        "@@name@@ skips the last entry when adding up @@field@@; entries with @@field@@ 2, 3 and 4 must total 9",
        'def @@fn@@(items):\n    return sum(item["@@field@@"] for item in items)\n',
        'def @@fn@@(items):\n    return sum(item["@@field@@"] for item in items[:-1])\n',
        "self.assertEqual(m.@@fn@@([{'@@field@@': 2}, {'@@field@@': 3}, {'@@field@@': 4}]), 9)\nself.assertEqual(m.@@fn@@([]), 0)",
        "export function @@fn@@(items) {\n  return items.reduce((total, item) => total + item.@@field@@, 0);\n}\n",
        "export function @@fn@@(items) {\n  return items.slice(0, -1).reduce((total, item) => total + item.@@field@@, 0);\n}\n",
        "assert.equal(m.@@fn@@([{ @@field@@: 2 }, { @@field@@: 3 }, { @@field@@: 4 }]), 9);\nassert.equal(m.@@fn@@([]), 0);",
    ),
    Template(
        "avg_empty",
        "@@Phrase@@ breaks on an empty list",
        "@@name@@ does not cope with an empty list: it should return 0 when there is nothing to average, "
        "and the plain mean otherwise.",
        "@@name@@ of an empty list returns 0, and of 2 and 4 returns 3.",
        "@@name@@ fails for an empty list instead of returning 0",
        'def @@fn@@(values):\n    if not values:\n        return 0\n    return sum(values) / len(values)\n',
        'def @@fn@@(values):\n    return sum(values) / len(values)\n',
        "self.assertEqual(m.@@fn@@([]), 0)\nself.assertEqual(m.@@fn@@([2, 4]), 3)",
        "export function @@fn@@(values) {\n  if (values.length === 0) return 0;\n  return values.reduce((a, b) => a + b, 0) / values.length;\n}\n",
        "export function @@fn@@(values) {\n  return values.reduce((a, b) => a + b, 0) / values.length;\n}\n",
        "assert.equal(m.@@fn@@([]), 0);\nassert.equal(m.@@fn@@([2, 4]), 3);",
    ),
    Template(
        "require_name",
        "Blank @@noun@@ accepted",
        "@@name@@ accepts a @@noun@@ made only of spaces. A blank @@noun@@ should be rejected with an "
        "error, and a valid one returned without surrounding spaces.",
        "@@name@@ raises an error for three spaces, and returns Ada for '  Ada '.",
        "@@name@@ accepts a @@noun@@ made only of spaces; it must raise an error for a blank value and return the trimmed value otherwise",
        'def @@fn@@(name):\n    cleaned = name.strip()\n    if not cleaned:\n        raise ValueError("@@noun@@ is required")\n    return cleaned\n',
        'def @@fn@@(name):\n    if not name:\n        raise ValueError("@@noun@@ is required")\n    return name.strip()\n',
        "with self.assertRaises(ValueError):\n    m.@@fn@@('   ')\nself.assertEqual(m.@@fn@@('  Ada '), 'Ada')",
        "export function @@fn@@(name) {\n  const cleaned = name.trim();\n  if (!cleaned) throw new Error(\"@@noun@@ is required\");\n  return cleaned;\n}\n",
        "export function @@fn@@(name) {\n  if (!name) throw new Error(\"@@noun@@ is required\");\n  return name.trim();\n}\n",
        "assert.throws(() => m.@@fn@@('   '));\nassert.equal(m.@@fn@@('  Ada '), 'Ada');",
    ),
    Template(
        "sort_desc",
        "@@Phrase@@ come out in the wrong order",
        "@@name@@ lists entries with the smallest @@field@@ first. The list should start with the largest "
        "@@field@@.",
        "@@name@@ over entries with @@field@@ 1, 3 and 2 returns them in the order 3, 2, 1.",
        "@@name@@ sorts ascending by @@field@@; it must return the largest @@field@@ first",
        'def @@fn@@(rows):\n    return sorted(rows, key=lambda row: row["@@field@@"], reverse=True)\n',
        'def @@fn@@(rows):\n    return sorted(rows, key=lambda row: row["@@field@@"])\n',
        "rows = [{'@@field@@': 1}, {'@@field@@': 3}, {'@@field@@': 2}]\nself.assertEqual([r['@@field@@'] for r in m.@@fn@@(rows)], [3, 2, 1])",
        "export function @@fn@@(rows) {\n  return [...rows].sort((a, b) => b.@@field@@ - a.@@field@@);\n}\n",
        "export function @@fn@@(rows) {\n  return [...rows].sort((a, b) => a.@@field@@ - b.@@field@@);\n}\n",
        "const rows = [{ @@field@@: 1 }, { @@field@@: 3 }, { @@field@@: 2 }];\nassert.deepEqual(m.@@fn@@(rows).map((r) => r.@@field@@), [3, 2, 1]);",
    ),
    Template(
        "unit_convert",
        "@@Phrase@@ are sixty times too long",
        "@@name@@ converts minutes to seconds twice over, so every window is sixty times too long. Two "
        "minutes should be 120 seconds.",
        "@@name@@(2) returns 120 and @@name@@(0) returns 0.",
        "@@name@@(minutes) converts to seconds twice over: @@name@@(2) must return 120",
        'def @@fn@@(minutes):\n    return minutes * 60\n',
        'def @@fn@@(minutes):\n    return minutes * 60 * 60\n',
        "self.assertEqual(m.@@fn@@(2), 120)\nself.assertEqual(m.@@fn@@(0), 0)",
        "export function @@fn@@(minutes) {\n  return minutes * 60;\n}\n",
        "export function @@fn@@(minutes) {\n  return minutes * 60 * 60;\n}\n",
        "assert.equal(m.@@fn@@(2), 120);\nassert.equal(m.@@fn@@(0), 0);",
    ),
    Template(
        "dedupe_order",
        "@@Phrase@@ lose their original order",
        "@@name@@ removes duplicates but also sorts the codes. The first occurrence of each code should "
        "keep its original position.",
        "@@name@@ of B, A, B, C returns B, A, C.",
        "@@name@@ sorts the codes while removing duplicates; it must keep first occurrences in their original order",
        'def @@fn@@(codes):\n    seen = set()\n    out = []\n    for code in codes:\n        if code not in seen:\n            seen.add(code)\n            out.append(code)\n    return out\n',
        'def @@fn@@(codes):\n    return sorted(set(codes))\n',
        "self.assertEqual(m.@@fn@@(['B', 'A', 'B', 'C']), ['B', 'A', 'C'])",
        "export function @@fn@@(codes) {\n  return [...new Set(codes)];\n}\n",
        "export function @@fn@@(codes) {\n  return [...new Set(codes)].sort();\n}\n",
        "assert.deepEqual(m.@@fn@@(['B', 'A', 'B', 'C']), ['B', 'A', 'C']);",
    ),
    Template(
        "inclusive_range",
        "@@Phrase@@ leave out the last day",
        "@@name@@(start, end) stops one day short. Both the start and the end day should be included.",
        "@@name@@(3, 5) returns 3, 4 and 5.",
        "@@name@@(start, end) leaves out the end day; @@name@@(3, 5) must return 3, 4 and 5",
        'def @@fn@@(start, end):\n    return list(range(start, end + 1))\n',
        'def @@fn@@(start, end):\n    return list(range(start, end))\n',
        "self.assertEqual(m.@@fn@@(3, 5), [3, 4, 5])\nself.assertEqual(m.@@fn@@(4, 4), [4])",
        "export function @@fn@@(start, end) {\n  const out = [];\n  for (let d = start; d <= end; d += 1) out.push(d);\n  return out;\n}\n",
        "export function @@fn@@(start, end) {\n  const out = [];\n  for (let d = start; d < end; d += 1) out.push(d);\n  return out;\n}\n",
        "assert.deepEqual(m.@@fn@@(3, 5), [3, 4, 5]);\nassert.deepEqual(m.@@fn@@(4, 4), [4]);",
    ),
    Template(
        "percent_valid",
        "Negative @@phrase@@ accepted",
        "@@name@@ rejects percentages above 100 but lets negative ones through. Anything below 0 should "
        "be rejected with an error too.",
        "@@name@@(-5) raises an error, @@name@@(101) raises an error and @@name@@(50) returns one half.",
        "@@name@@ accepts negative percentages; values below 0 must raise an error like values above 100 do",
        'def @@fn@@(pct):\n    if pct < 0 or pct > 100:\n        raise ValueError("percentage out of range")\n    return pct / 100\n',
        'def @@fn@@(pct):\n    if pct > 100:\n        raise ValueError("percentage out of range")\n    return pct / 100\n',
        "with self.assertRaises(ValueError):\n    m.@@fn@@(-5)\nwith self.assertRaises(ValueError):\n    m.@@fn@@(101)\nself.assertEqual(m.@@fn@@(50), 0.5)",
        "export function @@fn@@(pct) {\n  if (pct < 0 || pct > 100) throw new RangeError(\"percentage out of range\");\n  return pct / 100;\n}\n",
        "export function @@fn@@(pct) {\n  if (pct > 100) throw new RangeError(\"percentage out of range\");\n  return pct / 100;\n}\n",
        "assert.throws(() => m.@@fn@@(-5));\nassert.throws(() => m.@@fn@@(101));\nassert.equal(m.@@fn@@(50), 0.5);",
    ),
]

TEMPLATES = {t.id: t for t in T}
IDS = [t.id for t in T]
