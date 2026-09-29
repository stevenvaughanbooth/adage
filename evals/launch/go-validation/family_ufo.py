"""GO-validation family: `ufo-util`, "add a new URL utility to unjs/ufo" (TypeScript, pnpm, vitest).

A recurring task in a repository never used for card research, in an ecosystem the card was never
tested on (the families so far: click parameter types, marshmallow validators, urfave/cli flags).

The procedure that repeats (the project's own conventions, from its sources and CI):
  - the function in `src/utils.ts`, with a JSDoc block (description, `@example`, `@group utils`);
  - tests (`test/utilities.test.ts` or a new `test/*.test.ts`);
  - the README's utilities reference, which is GENERATED from the JSDoc by `automd`
    (`<!-- automd:jsdocs src=./src defaultGroup=utils -->`; `pnpm automd`, also run by `pnpm build`);
  - the project's CI: `pnpm lint` (eslint with eslint-config-unjs, then `prettier -c src test`) and
    `vitest run --typecheck` (`pnpm test` runs both).

Each variant has an issue-style prompt that names the deliverables (exported utility, tests, README
documentation) but not where they go or how, a hidden check and a gold patch. The prompts stay close
to one template, as in the earlier families: the shipped detector is strict and literal (0/6
paraphrases in the Go family), and this validation tests the product's end-to-end flow, not
paraphrase recall. The order is fixed here, before any session: the first TRAIN_N are the training
sessions (a user's history), the rest are held out.

NEGATIVES are same-repository prompts that are NOT this family (for the false-fire check): 5
RepoBench ufo bug-fix prompts whose real fix edits `src/utils.ts` (the card's own file, so the
grey-zone file signal is exercised too) and 3 hand-written hard negatives (docs-only, a bug fix, and
an option added to an EXISTING utility, whose wording is closest to the template).
"""

from __future__ import annotations

import json
import subprocess
import textwrap
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parents[2]

TRAIN_N = 4
UFO_BASE = "f06c800"  # unjs/ufo main, "chore(release): v1.6.4" (after #335), the upstream head on 2026-09-29
FAMILY_ID = "ufo-util"

PROMPT = (
    "Add a new utility to ufo: `{sig}`, which {desc}\n\n"
    "```js\n{examples}\n```\n\n"
    "It must be exported from the package like the other utilities. Include tests, and document it "
    "in the README, as the project does for its other utilities."
)

# id, name, signature (as the README heading shows it), description, cases [(args, expected)],
# gold body (TypeScript, inside the function), gold signature (TypeScript).
# Every hidden case is one of the prompt's examples: nothing the prompt does not state is tested.
VARIANTS = [
    # ------------------------------------------------------------------ training (a user's history)
    dict(id="without-query", name="withoutQuery", sig="withoutQuery(input)",
         desc="removes the query section from a URL and keeps everything else, the fragment included.",
         cases=[(["/foo?bar=1#baz"], "/foo#baz"),
                (["http://example.com/path?a=1&b=2"], "http://example.com/path"),
                (["/foo"], "/foo")],
         ts_sig="input: string): string",
         body="""
            return stringifyParsedURL({ ...parseURL(input), search: "" });
         """),
    dict(id="with-port", name="withPort", sig="withPort(input, port)",
         desc="sets or replaces the port of an absolute URL. A URL without a host is returned unchanged.",
         cases=[(["http://example.com/foo", 8080], "http://example.com:8080/foo"),
                (["https://example.com:3000/", 443], "https://example.com:443/"),
                (["/foo", 8080], "/foo")],
         ts_sig="input: string, port: number | string): string",
         body="""
            const parsed = parseURL(input);
            if (!parsed.host) {
              return input;
            }
            parsed.host = parsed.host.replace(/:\\d*$/, "") + ":" + port;
            return stringifyParsedURL(parsed);
         """),
    dict(id="get-origin", name="getOrigin", sig="getOrigin(input)",
         desc="returns the origin of an absolute URL (its protocol and host, with the port if there is one, "
              "without credentials), or an empty string for a URL without a host.",
         cases=[(["https://example.com:8080/foo?x=1"], "https://example.com:8080"),
                (["http://user:pass@example.com/x"], "http://example.com"),
                (["/foo"], "")],
         ts_sig="input: string): string",
         body="""
            const parsed = parseURL(input);
            if (!parsed.protocol || !parsed.host) {
              return "";
            }
            return parsed.protocol + "//" + parsed.host;
         """),
    dict(id="path-segments", name="getPathSegments", sig="getPathSegments(input)",
         desc="returns the non-empty segments of a URL's path as an array of decoded strings (the query and "
              "the fragment are ignored).",
         cases=[(["/foo/bar%20baz/"], ["foo", "bar baz"]),
                (["http://example.com/a/b?c=1#d"], ["a", "b"]),
                (["/"], [])],
         ts_sig="input: string): string[]",
         body="""
            return parseURL(input)
              .pathname.split("/")
              .filter(Boolean)
              .map((segment) => decodePath(segment));
         """),
    # ---------------------------------------------------------------------------------- held out
    dict(id="without-auth", name="withoutAuth", sig="withoutAuth(input)",
         desc="removes the credentials (`user:password@`) from a URL and keeps everything else.",
         cases=[(["https://user:pass@example.com/foo?x=1"], "https://example.com/foo?x=1"),
                (["http://admin@example.com"], "http://example.com"),
                (["https://example.com/foo"], "https://example.com/foo")],
         ts_sig="input: string): string",
         body="""
            return stringifyParsedURL({ ...parseURL(input), auth: "" });
         """),
    dict(id="same-origin", name="isSameOrigin", sig="isSameOrigin(a, b)",
         desc="checks whether two absolute URLs have the same origin: the same protocol and the same host, "
              "port included, compared case-insensitively. URLs without a host never have the same origin.",
         cases=[(["https://example.com/a", "https://EXAMPLE.com/b?x=1"], True),
                (["http://example.com", "https://example.com"], False),
                (["https://example.com:8080/", "https://example.com/"], False),
                (["/a", "/b"], False)],
         ts_sig="a: string, b: string): boolean",
         body="""
            const pa = parseURL(a);
            const pb = parseURL(b);
            if (!pa.host || !pb.host) {
              return false;
            }
            return (
              (pa.protocol || "").toLowerCase() === (pb.protocol || "").toLowerCase() &&
              pa.host.toLowerCase() === pb.host.toLowerCase()
            );
         """),
    dict(id="sort-query", name="sortQuery", sig="sortQuery(input)",
         desc="sorts the query parameters of a URL by key, alphabetically, and keeps the path and the fragment.",
         cases=[(["/foo?b=2&a=1&c=3#top"], "/foo?a=1&b=2&c=3#top"),
                (["http://example.com/?z=1&y=2"], "http://example.com/?y=2&z=1"),
                (["/foo"], "/foo")],
         ts_sig="input: string): string",
         body="""
            const parsed = parseURL(input);
            if (!parsed.search) {
              return input;
            }
            const query = parseQuery(parsed.search);
            const sorted: QueryObject = {};
            for (const key of Object.keys(query).sort()) {
              sorted[key] = query[key];
            }
            parsed.search = "?" + stringifyQuery(sorted);
            return stringifyParsedURL(parsed);
         """),
    dict(id="lowercase-host", name="withLowercaseHost", sig="withLowercaseHost(input)",
         desc="lower-cases the protocol and the host of a URL and leaves the path, the query and the fragment as "
              "they are.",
         cases=[(["HTTPS://Example.COM/Foo?Q=1#Top"], "https://example.com/Foo?Q=1#Top"),
                (["http://WWW.Example.org"], "http://www.example.org"),
                (["/Foo/Bar"], "/Foo/Bar")],
         ts_sig="input: string): string",
         body="""
            const parsed = parseURL(input);
            if (!parsed.host) {
              return input;
            }
            parsed.protocol = (parsed.protocol || "").toLowerCase();
            parsed.host = parsed.host.toLowerCase();
            return stringifyParsedURL(parsed);
         """),
    dict(id="get-extension", name="getExtension", sig="getExtension(input)",
         desc="returns the file extension of the last segment of a URL's path, lower-cased and without the dot, "
              "or `undefined` when it has none (the query and the fragment are ignored).",
         cases=[(["/files/report.PDF?download=1"], "pdf"),
                (["http://example.com/archive.tar.gz"], "gz"),
                (["/files/README"], None),
                (["/folder/"], None)],
         ts_sig="input: string): string | undefined",
         body="""
            const segment = parseURL(input).pathname.split("/").pop() || "";
            const match = /^[^.].*\\.([^.]+)$/.exec(segment);
            return match ? match[1].toLowerCase() : undefined;
         """),
    dict(id="dot-segments", name="withoutDotSegments", sig="withoutDotSegments(input)",
         desc="resolves the `.` and `..` segments of a URL's path (as a browser does) and keeps the protocol, "
              "the host, the query and the fragment.",
         cases=[(["/a/./b/../c"], "/a/c"),
                (["http://example.com/a/b/../../c?x=1"], "http://example.com/c?x=1"),
                (["/a/.."], "/"),
                (["/a/b/"], "/a/b/")],
         ts_sig="input: string): string",
         body="""
            const parsed = parseURL(input);
            const out: string[] = [];
            const segments = parsed.pathname.split("/");
            for (const [i, segment] of segments.entries()) {
              const last = i === segments.length - 1;
              if (segment === "." || segment === "..") {
                if (segment === ".." && out.length > 1) {
                  out.pop();
                }
                if (last) {
                  out.push("");
                }
                continue;
              }
              out.push(segment);
            }
            parsed.pathname = out.join("/") || "/";
            return stringifyParsedURL(parsed);
         """),
    dict(id="query-param", name="getQueryParam", sig="getQueryParam(input, key)",
         desc="returns the decoded value of one query parameter of a URL: the first value when the key is "
              "repeated, or `undefined` when the key is absent.",
         cases=[(["/foo?a=1&b=2", "b"], "2"),
                (["/foo?a=1&a=2", "a"], "1"),
                (["/foo?q=hello+world", "q"], "hello world"),
                (["/foo", "a"], None)],
         ts_sig="input: string, key: string): string | undefined",
         body="""
            const value = getQuery(input)[key] as string | string[] | undefined;
            if (value === undefined) {
              return undefined;
            }
            return Array.isArray(value) ? value[0] : value;
         """),
    dict(id="is-http", name="isHttpURL", sig="isHttpURL(input)",
         desc="checks whether the input is an absolute `http:` or `https:` URL (the protocol is compared "
              "case-insensitively; protocol-relative and relative URLs are not).",
         cases=[(["https://example.com"], True),
                (["HTTP://example.com/x"], True),
                (["ftp://example.com"], False),
                (["//example.com"], False),
                (["/foo"], False)],
         ts_sig="input: string): boolean",
         body="""
            return /^https?:\\/\\//i.test(input.trim());
         """),
]

# RepoBench ufo tasks (tasks/ufo.json) whose real fix edits src/utils.ts; their prompts are used
# verbatim as negatives, with the files their fix edited (the grey-zone file-signal probe edits them).
NEGATIVE_REPOBENCH = ["ufo-0e060c7f", "ufo-f437a3b4", "ufo-f4c0400d", "ufo-62199a0c", "ufo-eb299454"]
NEGATIVE_HANDWRITTEN = [
    dict(id="neg-docs-parse-filename", edits=["src/parse.ts", "README.md"],
         prompt="The README entry for `parseFilename` is confusing: it does not say what is returned without "
                "`{ strict: true }` for a path whose last segment has no extension. Improve the documentation "
                "of `parseFilename` so the README explains both modes with an example of each."),
    dict(id="neg-bug-clean-double-slashes", edits=["src/utils.ts"],
         prompt="`cleanDoubleSlashes` also collapses double slashes inside the query string, which corrupts "
                "URLs passed as query parameters. For example `cleanDoubleSlashes('/a//b?redirect=http://x.com//y')` "
                "should return `/a/b?redirect=http://x.com//y`. Please fix it and add a regression test."),
    dict(id="neg-option-with-query", edits=["src/utils.ts", "README.md"],
         prompt="Add an option to ufo's existing `withQuery` so that the given query can replace the URL's "
                "query instead of being merged into it: `withQuery('/foo?a=1', { b: 2 }, { replace: true })` "
                "should return `/foo?b=2`, and the default stays merging. Include tests and document the option."),
]


def task_id(vid: str) -> str:
    return f"{FAMILY_ID}--{vid}"


def variant(tid: str) -> dict:
    fam, vid = tid.split("--", 1)
    if fam != FAMILY_ID:
        raise KeyError(tid)
    return next(v for v in VARIANTS if v["id"] == vid)


def order() -> list[str]:
    return [task_id(v["id"]) for v in VARIANTS]


def train_tasks() -> list[str]:
    return order()[:TRAIN_N]


def held_out_tasks() -> list[str]:
    return order()[TRAIN_N:]


def js(x) -> str:
    """A JS literal (None is `undefined`)."""
    if x is None:
        return "undefined"
    if isinstance(x, bool):
        return "true" if x else "false"
    if isinstance(x, list):
        return "[" + ", ".join(js(y) for y in x) + "]"
    return json.dumps(x)


def example_lines(v: dict) -> str:
    return "\n".join(f"{v['name']}({', '.join(js(a) for a in args)}); // {js(exp)}" for args, exp in v["cases"])


def prompt(v: dict) -> str:
    return PROMPT.format(sig=v["sig"], desc=v["desc"], examples=example_lines(v))


def negatives() -> list[dict]:
    """The 8 same-repository negative prompts, each with the files its task edits."""
    tasks = {t["id"]: t for t in json.loads((REPO_ROOT / "evals/repobench/tasks/ufo.json").read_text())["tasks"]}
    out = [dict(id=t, prompt=tasks[t]["prompt"], edits=list(tasks[t]["src_files"]), source="repobench")
           for t in NEGATIVE_REPOBENCH]
    out += [dict(n, source="handwritten") for n in NEGATIVE_HANDWRITTEN]
    return out


HIDDEN_PATH = "test/zz-rw-hidden.test.ts"


def hidden(v: dict) -> str:
    """The hidden behaviour test (vitest). It reaches the utility through the package entry, so a
    missing export fails its cases instead of breaking collection."""
    cases = "\n".join(f"  [[{', '.join(js(a) for a in args)}], {js(exp)}]," for args, exp in v["cases"])
    return f'''// Hidden check of the GO validation (evals/launch/go-validation). Written by the scorer.
import {{ describe, expect, test }} from "vitest";
import * as ufo from "../src";

const fn = (ufo as Record<string, any>)[{json.dumps(v["name"])}];

describe("RWHidden {v["name"]}", () => {{
  test("is exported from the package entry", () => {{
    expect(typeof fn).toBe("function");
  }});
  const cases: [unknown[], unknown][] = [
{cases}
  ];
  for (const [args, expected] of cases) {{
    test(JSON.stringify(args), () => {{
      expect(fn(...args)).toEqual(expected);
    }});
  }}
}});
'''


def readme_heading(v: dict) -> str:
    return f"### `{v['name']}("


def gold(v: dict, d: Path) -> None:
    """The reference patch: the function with its JSDoc in src/utils.ts, a test file, the README
    regenerated by automd, everything formatted by prettier (as `pnpm lint:fix` would)."""
    body = textwrap.indent(textwrap.dedent(v["body"]).strip("\n"), "  ")
    example = "\n".join(" * " + line for line in example_lines(v).splitlines())
    code = f'''

/**
 * {v["desc"][0].upper() + v["desc"][1:]}
 *
 * @example
 *
 * ```js
{example}
 * ```
 *
 * @group utils
 */
export function {v["name"]}({v["ts_sig"]} {{
{body}
}}
'''
    p = d / "src/utils.ts"
    p.write_text(p.read_text() + code)
    first_args, first_exp = v["cases"][0]
    t = d / "test" / f"gold-{v['id']}.test.ts"
    t.write_text(f'import {{ expect, test }} from "vitest";\nimport {{ {v["name"]} }} from "../src";\n\n'
                 f'test("{v["name"]}", () => {{\n  expect({v["name"]}({", ".join(js(a) for a in first_args)}))'
                 f'.toEqual({js(first_exp)});\n}});\n')
    bins = d / "node_modules" / ".bin"
    subprocess.run([str(bins / "prettier"), "-w", "src/utils.ts", str(t.relative_to(d))], cwd=d, check=True,
                   capture_output=True)
    subprocess.run([str(bins / "automd")], cwd=d, check=True, capture_output=True)


FAMILY = dict(id=FAMILY_ID, repo="ufo", base=UFO_BASE, variants=VARIANTS, prompt=prompt, hidden=hidden,
              gold=gold, hidden_path=HIDDEN_PATH, lang="typescript")


if __name__ == "__main__":
    for tid in order():
        print(tid, "|", prompt(variant(tid))[:200].replace("\n", " "))
    for n in negatives():
        print(n["id"], n["edits"], "|", n["prompt"][:120].replace("\n", " "))
