"""Showcase family on Rust (cargo): `http-header`, "add a standard header name constant to the http crate".

hyperium/http @ fdc6ab8 (1.5.0 + 2 commits, the upstream head on 2026-09-29). New for the showcase:
this repository was in the data-expansion corpus (one-off bug fixes only), never in card research.

The procedure that repeats is the project's own. Upstream #583 (`feat: add cache-status and
cdn-cache-control header name constants`) touched, for two headers:
  - `src/header/name.rs`: an entry `(Konst, UPPER, b"lower-name");` in the `standard_headers!` table,
    with a doc comment (the crate is `#![deny(warnings, missing_docs)]`, so an undocumented constant
    does not build), which also puts it in the table-driven tests `test_parse_standard_headers` and
    `test_standard_headers_into_bytes`;
  - `src/header/mod.rs`: the re-export in `pub use self::name::{...}`;
  - `tests/header_map.rs` (`STD`) and `tests/header_map_fuzz.rs` (`STANDARD_HEADERS`): the lists of all
    standard headers;
  - `benches/`, `CHANGELOG.md` (recent feature PRs, e.g. #798 `Method::QUERY`, #758 `EARLY_HINTS`, no
    longer touch the changelog: it is written at release).

Hidden check (all must hold):
  standard   the constant is an entry of the `standard_headers!` table in src/header/name.rs, with a doc
             comment right above it (a `from_static` constant would be a custom name: no fast path, and
             not what the project does)
  own_test   a test the agent added or changed (tests/*.rs, or name.rs's test module) names the constant
  tests      `cargo test` (the crate's whole suite: unit, integration and doc tests) with the hidden
             integration test tests/zz_showcase_hidden.rs: `http::header::UPPER` exists, `as_str()` is the
             lower-case name, and `HeaderName::from_bytes` / `from_static` of the name in the prompt's
             letter cases equal the constant

Order fixed before any session: the first 4 are the training sessions (a user's history).
"""

from __future__ import annotations

import re
from pathlib import Path

HTTP_BASE = "fdc6ab8"

PROMPT = (
    "Add a constant for the `{header}` header to the http crate: `http::header::{const}`. {desc}\n\n"
    "It must be a standard header name like the crate's other header constants: parsing the name in any "
    "letter case gives the constant, for example `HeaderName::from_bytes(b\"{header}\")` equals "
    "`header::{const}`, and `header::{const}.as_str()` is `\"{lower}\"`.\n\n"
    "Include tests, and document the constant, as the project does for its other standard headers."
)

# const, header (canonical case), description (the prompt's and the gold doc comment's first line), spec
VARIANTS = [
    # ------------------------------------------------------------------ training (a user's history)
    dict(id="priority", const="PRIORITY", header="Priority",
         desc="It carries the client's priority signals for a response (urgency and incremental delivery).",
         spec="RFC 9218"),
    dict(id="accept-patch", const="ACCEPT_PATCH", header="Accept-Patch",
         desc="It advertises which media types the server accepts in a PATCH request.",
         spec="RFC 5789"),
    dict(id="content-digest", const="CONTENT_DIGEST", header="Content-Digest",
         desc="It carries a digest of the message content, computed over the actual content bytes.",
         spec="RFC 9530"),
    dict(id="server-timing", const="SERVER_TIMING", header="Server-Timing",
         desc="It communicates one or more performance metrics of the request-response cycle to the client.",
         spec="W3C Server Timing"),
    # ---------------------------------------------------------------------------------- held out
    dict(id="coop", const="CROSS_ORIGIN_OPENER_POLICY", header="Cross-Origin-Opener-Policy",
         desc="It lets a document isolate its browsing context group from cross-origin documents.",
         spec="WHATWG HTML"),
    dict(id="sec-fetch-dest", const="SEC_FETCH_DEST", header="Sec-Fetch-Dest",
         desc="It is a fetch metadata request header indicating the request's destination (for example "
              "`image` or `document`).",
         spec="W3C Fetch Metadata"),
    dict(id="permissions-policy", const="PERMISSIONS_POLICY", header="Permissions-Policy",
         desc="It lets a site allow or deny the use of browser features in a document and its frames.",
         spec="W3C Permissions Policy"),
    dict(id="x-forwarded-for", const="X_FORWARDED_FOR", header="X-Forwarded-For",
         desc="It is the de-facto standard header identifying the originating IP addresses of a client "
              "connecting through proxies.",
         spec="de-facto standard"),
    dict(id="clear-site-data", const="CLEAR_SITE_DATA", header="Clear-Site-Data",
         desc="It asks the browser to clear browsing data (cookies, storage, cache) for the requesting origin.",
         spec="W3C Clear Site Data"),
    dict(id="timing-allow-origin", const="TIMING_ALLOW_ORIGIN", header="Timing-Allow-Origin",
         desc="It lists the origins allowed to see timing values retrieved via the Resource Timing API.",
         spec="W3C Resource Timing"),
    dict(id="repr-digest", const="REPR_DIGEST", header="Repr-Digest",
         desc="It carries a digest of the selected representation of the target resource.",
         spec="RFC 9530"),
    dict(id="early-data", const="EARLY_DATA", header="Early-Data",
         desc="It indicates that the request was conveyed in TLS early data (0-RTT).",
         spec="RFC 8470"),
]

HIDDEN_PATH = "tests/zz_showcase_hidden.rs"


def konst(v: dict) -> str:
    """The `StandardHeader` variant name (CamelCase of the header)."""
    return "".join(p[:1].upper() + p[1:].lower() for p in v["header"].split("-"))


def prompt(v: dict) -> str:
    return PROMPT.format(header=v["header"], const=v["const"], desc=v["desc"], lower=v["header"].lower())


def hidden(v: dict) -> str:
    lower = v["header"].lower()
    return f'''// Hidden check of the Adage showcase (evals/showcase). Written by the scorer.
use http::header::{{self, HeaderName}};

#[test]
fn showcase_hidden_constant() {{
    assert_eq!(header::{v["const"]}.as_str(), {lower!r:s});
}}

#[test]
fn showcase_hidden_parse_any_case() {{
    for name in [{v["header"]!r:s}, {lower!r:s}, {v["header"].upper()!r:s}] {{
        assert_eq!(HeaderName::from_bytes(name.as_bytes()).unwrap(), header::{v["const"]}, "{{}}", name);
    }}
    assert_eq!(HeaderName::from_static({lower!r:s}), header::{v["const"]});
}}
'''.replace("'", '"')


def entry_re(v: dict) -> re.Pattern:
    return re.compile(r"\(\s*(\w+)\s*,\s*" + re.escape(v["const"]) + r'\s*,\s*b"' + re.escape(v["header"].lower())
                      + r'"\s*\)\s*;')


def checks(v: dict, d: Path, changed: list[str]) -> dict:
    """The source-level checks (before the hidden test is written)."""
    name_rs = d / "src/header/name.rs"
    text = name_rs.read_text(errors="replace") if name_rs.exists() else ""
    m = entry_re(v).search(text)
    documented = False
    if m:
        before = text[:m.start()].rstrip().splitlines()
        documented = bool(before) and before[-1].strip().startswith("///")
    own = False
    for p in changed:
        if p == HIDDEN_PATH or not p.endswith(".rs") or not (d / p).is_file():
            continue
        body = (d / p).read_text(errors="replace")
        if p.startswith("tests/") and re.search(r"\b" + v["const"] + r"\b", body):
            own = True
        elif p == "src/header/name.rs":
            tests = body.split("mod tests", 1)
            if len(tests) == 2 and re.search(r"\b" + v["const"] + r"\b|\b" + konst(v) + r"\b", tests[1]):
                own = True
    return {"standard": bool(m) and documented, "own_test": own}


def _insert_sorted(lines: list[str], start: int, end: int, item: str, key) -> None:
    """Insert `item` in lines[start:end] before the first line whose key sorts after it."""
    for i in range(start, end):
        k = key(lines[i])
        if k is not None and k > key(item):
            lines.insert(i, item)
            return
    lines.insert(end, item)


def gold(v: dict, d: Path) -> None:
    """The reference patch, as upstream #583: table entry with a doc comment (alphabetical), the
    re-export, and the two all-standard-headers test lists."""
    p = d / "src/header/name.rs"
    src = p.read_text()
    entries = list(re.finditer(r'^    \((\w+), (\w+), b"([a-z0-9-]+)"\);\n', src, re.M))
    after = next((e for e in entries if e.group(3) > v["header"].lower()), None)
    first = v["desc"][3:] if v["desc"].startswith("It ") else v["desc"]
    block = (f"    /// {first[:1].upper() + first[1:]}\n    ///\n"
             f"    /// See {v['spec']}.\n    ({konst(v)}, {v['const']}, b\"{v['header'].lower()}\");\n\n")
    if after is None:
        at = entries[-1].end() + 1
    else:  # before the doc comment of the entry that follows alphabetically
        head = src[:after.start()].splitlines(keepends=True)
        i = len(head)
        while i > 0 and head[i - 1].strip().startswith("///"):
            i -= 1
        at = sum(len(x) for x in head[:i])
    p.write_text(src[:at] + block + src[at:])

    def listed(path: str, prefix: str, opener: str) -> None:
        f = d / path
        lines = f.read_text().splitlines(keepends=True)
        s = next(i for i, x in enumerate(lines) if opener in x) + 1
        e = next(i for i in range(s, len(lines)) if lines[i].strip().startswith(("]", "}")))
        indent = re.match(r"\s*", lines[s]).group(0)

        def key(line):
            m = re.match(r"\s*(?:header::)?([A-Z0-9_]+),\s*$", line)
            return m.group(1) if m else None

        _insert_sorted(lines, s, e, f"{indent}{prefix}{v['const']},\n", key)
        f.write_text("".join(lines))

    listed("src/header/mod.rs", "", "pub use self::name::{")
    listed("tests/header_map.rs", "", "const STD: &")
    listed("tests/header_map_fuzz.rs", "header::", "const STANDARD_HEADERS")


NEGATIVES = [
    dict(id="http-6e2dd42a", source="repobench"),
    dict(id="http-bc3b0441", source="repobench"),
    dict(id="http-7767ff70", source="repobench"),
    dict(id="neg-http-doc-accept-ranges", source="handwritten", edits=["src/header/name.rs"],
         prompt="The documentation of the existing `header::ACCEPT_RANGES` constant is out of date: it does not "
                "mention that `none` is a valid value. Improve its doc comment, with a link to RFC 9110."),
    dict(id="neg-http-status-const", source="handwritten", edits=["src/status.rs"],
         prompt="Add a `StatusCode` constant for the 421 Misdirected Request status if it is missing, and make "
                "sure `canonical_reason()` returns its reason phrase. Include a test."),
    dict(id="neg-http-headername-bug", source="handwritten", edits=["src/header/name.rs"],
         prompt="`HeaderName::from_lowercase` accepts some upper-case bytes in names longer than 64 bytes. Fix it "
                "so upper-case input is always rejected, and add a regression test."),
]

FAMILY = dict(id="http-header", repo="http", base=HTTP_BASE, variants=VARIANTS, prompt=prompt, hidden=hidden,
              gold=gold, hidden_path=HIDDEN_PATH, pristine=[], lang="rust", name_key="const",
              command="cargo test -q 2>&1", checks=checks, negatives=NEGATIVES,
              repo_key="github.com/hyperium/http", forward_env=["RUSTUP_HOME"], confine_expose=["/root/.rustup"],
              prune=["target"], probe_edits=["src/header/name.rs", "src/header/mod.rs", "tests/header_map.rs"])
