"""Recurring-workflow task families: the same procedure, different specifics.

Two families on two real repositories, each at its current upstream head:

  click-type   "add a new built-in parameter type `click.<NAME>`" (types.py, the package export, the
               API reference, the parameter-types guide, the changelog, tests; Click's light-imports
               invariant is part of its CI)
  mm-validator "add a new validator `marshmallow.validate.<Name>`" (validate.py with the validators'
               error/repr conventions, a docstring, the changelog, tests)

Each variant has an issue-style prompt that names the deliverables (tests, docs, changelog) but not
where they go or how, a hidden check (`hidden_test`) and a gold patch (`apply_gold`) that the
validator uses to prove the check is satisfiable. The variant order is fixed here, before any
session ran: the first TRAIN_N of each family are the training sessions.
"""

from __future__ import annotations

import re
import textwrap
from pathlib import Path

TRAIN_N = 4

# ----------------------------------------------------------------------------------- click-type

CLICK_BASE = "06b2a67"  # pallets/click main, "Deprecate parameter names ..." (#3866), 8.6.0 unreleased

CLICK_PROMPT = (
    "Add a new built-in parameter type to Click: `click.{name}`, {desc} "
    "Values it cannot convert must fail with Click's usual \"Invalid value\" usage error. "
    "Examples of valid input: {valid_ex}. Examples of invalid input: {invalid_ex}.\n\n"
    "Include tests, document the new type in the API reference and in the parameter types guide, "
    "and add a changelog entry."
)

# name, class name, description, valid [(raw, expected expr)], invalid [raw], imports the hidden test
# needs, gold convert body (value -> converted, `fail(msg)` on error; lazy imports inside)
CLICK = [
    dict(id="hex-color", name="HEX_COLOR", cls="HexColorParamType",
         desc="which accepts a CSS hex color written as `#RGB` or `#RRGGBB` (case-insensitive, the `#` is "
              "required) and converts it to the normalized lower-case 6-digit form `#rrggbb` (a `str`).",
         valid=[("#FFF", "'#ffffff'"), ("#1a2B3c", "'#1a2b3c'"), ("#000000", "'#000000'")],
         invalid=["fff", "#12345", "#ggg"],
         gold="""
            m = re.fullmatch(r"#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})", value.strip())
            if not m:
                fail(value)
            h = m.group(1).lower()
            if len(h) == 3:
                h = "".join(c * 2 for c in h)
            return "#" + h
         """),
    dict(id="port", name="PORT", cls="PortParamType",
         desc="which accepts a TCP/UDP port number, an integer from 1 to 65535 inclusive, and converts it "
              "to an `int`.",
         valid=[("80", "80"), ("65535", "65535"), ("1", "1")],
         invalid=["0", "65536", "http"],
         gold="""
            try:
                n = int(value)
            except ValueError:
                fail(value)
            if not 1 <= n <= 65535:
                fail(value)
            return n
         """),
    dict(id="duration", name="DURATION", cls="DurationParamType",
         desc="which accepts a duration written as one or more `<integer><unit>` parts with the units "
              "`d`, `h`, `m` and `s`, in that order (for example `1h30m`, `45s`, `2d`), and converts it to "
              "a `datetime.timedelta`.",
         valid=[("1h30m", "datetime.timedelta(hours=1, minutes=30)"), ("45s", "datetime.timedelta(seconds=45)"),
                ("2d", "datetime.timedelta(days=2)"), ("1d2h3m4s", "datetime.timedelta(days=1, hours=2, minutes=3, seconds=4)")],
         invalid=["10", "1x", "h", "30m1h"],
         gold="""
            import datetime as _dt
            m = re.fullmatch(r"(?:(\\d+)d)?(?:(\\d+)h)?(?:(\\d+)m)?(?:(\\d+)s)?", value.strip())
            if not value.strip() or not m or not any(m.groups()):
                fail(value)
            d, h, mi, s = (int(x or 0) for x in m.groups())
            return _dt.timedelta(days=d, hours=h, minutes=mi, seconds=s)
         """),
    dict(id="ip-address", name="IP_ADDRESS", cls="IPAddressParamType",
         desc="which accepts an IPv4 or IPv6 address and converts it to an `ipaddress.IPv4Address` or "
              "`ipaddress.IPv6Address`.",
         valid=[("127.0.0.1", "ipaddress.ip_address('127.0.0.1')"), ("::1", "ipaddress.ip_address('::1')"),
                ("192.168.1.20", "ipaddress.ip_address('192.168.1.20')")],
         invalid=["256.1.1.1", "localhost", "1.2.3"],
         gold="""
            import ipaddress
            try:
                return ipaddress.ip_address(value.strip())
            except ValueError:
                fail(value)
         """),
    dict(id="semver", name="SEMVER", cls="SemVerParamType",
         desc="which accepts a semantic version `MAJOR.MINOR.PATCH` of non-negative integers, optionally "
              "prefixed with `v`, and converts it to a tuple of three `int`s.",
         valid=[("1.2.3", "(1, 2, 3)"), ("v0.10.0", "(0, 10, 0)"), ("10.0.1", "(10, 0, 1)")],
         invalid=["1.2", "1.2.x", "1.2.3.4"],
         gold="""
            m = re.fullmatch(r"v?(\\d+)\\.(\\d+)\\.(\\d+)", value.strip())
            if not m:
                fail(value)
            return tuple(int(x) for x in m.groups())
         """),
    dict(id="percentage", name="PERCENTAGE", cls="PercentageParamType",
         desc="which accepts a percentage from 0 to 100 inclusive, written with or without a trailing `%` "
              "(for example `50%`, `12.5`), and converts it to a `float` fraction (`50%` becomes `0.5`).",
         valid=[("50%", "0.5"), ("12.5", "0.125"), ("100%", "1.0"), ("0", "0.0")],
         invalid=["101%", "-1", "abc"],
         gold="""
            v = value.strip()
            if v.endswith("%"):
                v = v[:-1]
            try:
                f = float(v)
            except ValueError:
                fail(value)
            if not 0 <= f <= 100:
                fail(value)
            return f / 100
         """),
    dict(id="byte-size", name="BYTE_SIZE", cls="ByteSizeParamType",
         desc="which accepts a size in bytes written as a number with an optional unit: `B`, `KB`, `MB`, `GB` "
              "(powers of 1000) or `KiB`, `MiB`, `GiB` (powers of 1024), case-insensitive, and converts it to "
              "an `int` number of bytes (a bare number is bytes; `1.5MB` is `1500000`).",
         valid=[("10KB", "10000"), ("1KiB", "1024"), ("512", "512"), ("1.5MB", "1500000"), ("2gib", "2147483648")],
         invalid=["10XB", "-1KB", "KB"],
         gold="""
            units = {"": 1, "b": 1, "kb": 10**3, "mb": 10**6, "gb": 10**9, "kib": 2**10, "mib": 2**20, "gib": 2**30}
            m = re.fullmatch(r"(\\d+(?:\\.\\d+)?)\\s*([a-zA-Z]*)", value.strip())
            if not m or m.group(2).lower() not in units:
                fail(value)
            return int(float(m.group(1)) * units[m.group(2).lower()])
         """),
    dict(id="slug", name="SLUG", cls="SlugParamType",
         desc="which accepts a URL slug: lower-case ASCII letters and digits in groups separated by single "
              "hyphens (for example `my-post-2`), and returns it unchanged as a `str`.",
         valid=[("my-post-2", "'my-post-2'"), ("abc", "'abc'"), ("a1-b2", "'a1-b2'")],
         invalid=["Hello", "a--b", "-a", "a_b"],
         gold="""
            if not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", value):
                fail(value)
            return value
         """),
    dict(id="date", name="DATE", cls="DateParamType",
         desc="which accepts a calendar date in ISO format `YYYY-MM-DD` and converts it to a "
              "`datetime.date`.",
         valid=[("2024-02-29", "datetime.date(2024, 2, 29)"), ("1999-12-31", "datetime.date(1999, 12, 31)")],
         invalid=["2024-13-01", "01/02/2024", "2023-02-29"],
         gold="""
            import datetime as _dt
            if not re.fullmatch(r"\\d{4}-\\d{2}-\\d{2}", value.strip()):
                fail(value)
            try:
                return _dt.date.fromisoformat(value.strip())
            except ValueError:
                fail(value)
         """),
    dict(id="host-port", name="HOST_PORT", cls="HostPortParamType",
         desc="which accepts a `host:port` pair (a non-empty host name and a port from 1 to 65535) and "
              "converts it to a tuple `(host, port)` of a `str` and an `int`.",
         valid=[("localhost:8080", "('localhost', 8080)"), ("example.com:443", "('example.com', 443)")],
         invalid=["localhost", "host:0", ":80", "host:http"],
         gold="""
            host, sep, port = value.strip().rpartition(":")
            if not sep or not host or not port.isdigit() or not 1 <= int(port) <= 65535:
                fail(value)
            return (host, int(port))
         """),
]

CLICK_HIDDEN = '''\
import datetime
import ipaddress
import pathlib
import re

import pytest

import click
from click.testing import CliRunner

ROOT = pathlib.Path(__file__).resolve().parents[1]
NAME = {name!r}
VALID = [{valid}]
INVALID = {invalid!r}


def _type():
    t = getattr(click, NAME, None)
    assert t is not None, f"click.{{NAME}} is missing"
    return t


def _run(args):
    @click.command()
    @click.option("--v", type=_type())
    def cli(v):
        click.echo(repr(v))

    return CliRunner().invoke(cli, args)


def test_is_param_type():
    assert isinstance(_type(), click.ParamType)


@pytest.mark.parametrize(("raw", "expected"), VALID)
def test_valid(raw, expected):
    assert _type().convert(raw, None, None) == expected
    r = _run(["--v", raw])
    assert r.exit_code == 0, r.output
    assert r.output.strip() == repr(expected)


@pytest.mark.parametrize("raw", INVALID)
def test_invalid(raw):
    r = _run(["--v", raw])
    assert r.exit_code == 2, r.output
    assert "Invalid value" in r.output


def test_documented():
    word = re.compile(r"\\b" + NAME + r"\\b")
    assert word.search((ROOT / "docs" / "api.md").read_text()), "not in the API reference"
    assert word.search((ROOT / "docs" / "parameter-types.md").read_text()), "not in the parameter types guide"


def test_changelog():
    text = (ROOT / "CHANGES.md").read_text()
    first = re.split(r"^## Version ", text, flags=re.M)[1]
    assert re.search(r"\\b" + NAME + r"\\b", first), "no entry in the unreleased version's changes"


def test_has_tests():
    word = re.compile(r"\\b" + NAME + r"\\b")
    own = pathlib.Path(__file__).name
    assert any(word.search(p.read_text(errors="replace")) for p in (ROOT / "tests").rglob("*.py") if p.name != own)
'''


def click_prompt(v: dict) -> str:
    return CLICK_PROMPT.format(name=v["name"], desc=v["desc"],
                               valid_ex=", ".join(f"`{r}`" for r, _ in v["valid"]),
                               invalid_ex=", ".join(f"`{r}`" for r in v["invalid"]))


def click_hidden(v: dict) -> str:
    valid = ", ".join(f"({r!r}, {e})" for r, e in v["valid"])
    return CLICK_HIDDEN.format(name=v["name"], valid=valid, invalid=v["invalid"])


def click_gold(v: dict, d: Path) -> None:
    body = textwrap.indent(textwrap.dedent(v["gold"]).strip("\n"), " " * 8)
    cls = f'''

class {v["cls"]}(ParamType[t.Any]):
    name = {v["id"].replace("-", " ")!r}

    def convert(self, value: t.Any, param: Parameter | None, ctx: Context | None) -> t.Any:
        def fail(v: t.Any) -> t.NoReturn:
            self.fail(_("{{value!r}} is not a valid {v["id"].replace("-", " ")}.").format(value=v), param, ctx)

        if not isinstance(value, str):
            return value
{body}

    def __repr__(self) -> str:
        return {v["name"]!r}


#: A {v["id"].replace("-", " ")} parameter.
{v["name"]}: t.Final[{v["cls"]}] = {v["cls"]}()
'''
    p = d / "src/click/types.py"
    src = p.read_text()
    if "\nimport re\n" not in src:
        src = src.replace("import os\n", "import os\nimport re\n", 1)
    p.write_text(src + cls)
    p = d / "src/click/__init__.py"
    p.write_text(p.read_text().replace("from .types import UUID as UUID\n",
                                       f"from .types import UUID as UUID\nfrom .types import {v['name']} as {v['name']}\n"))
    p = d / "docs/api.md"
    p.write_text(p.read_text().replace(".. autodata:: UUID\n```\n",
                                       f".. autodata:: UUID\n```\n\n```{{eval-rst}}\n.. autodata:: {v['name']}\n```\n", 1))
    p = d / "docs/parameter-types.md"
    p.write_text(p.read_text().replace("  {class}`uuid.UUID`.\n",
                                       f"  {{class}}`uuid.UUID`.\n\n- {{data}}`click.{v['name']}`: {v['desc']}\n", 1))
    p = d / "CHANGES.md"
    p.write_text(p.read_text().replace("Unreleased\n\n", f"Unreleased\n\n- Add the {{data}}`{v['name']}` parameter type.\n", 1))
    t = d / "tests/test_types" / f"test_{v['id'].replace('-', '_')}.py"
    t.write_text(f"import click\n\n\ndef test_{v['id'].replace('-', '_')}():\n"
                 f"    assert click.{v['name']}.convert({v['valid'][0][0]!r}, None, None) is not None\n")


# ---------------------------------------------------------------------------------- mm-validator

MM_BASE = "7f0792bd"  # marshmallow-code/marshmallow dev, 4.4.0 unreleased

MM_PROMPT = (
    "Add a new validator to marshmallow: `marshmallow.validate.{sig}`, {desc} "
    "Its default error message is `{msg}`; a custom `error` message may interpolate {interp}. "
    "Examples of valid input: {valid_ex}. Examples of invalid input: {invalid_ex}.\n\n"
    "Follow the conventions of the existing validators, include tests and a docstring, and add a "
    "changelog entry."
)

# cls, signature, ctor args (for the hidden test, as source), desc, default message, interpolation
# keys, the default message formatted for the test's instance, the repr fragment, valid, invalid,
# a custom error template and its expected text for the first invalid input, gold __call__ check.
MM = [
    dict(id="starts-with", cls="StartsWith", sig="StartsWith(prefix, *, error=None)", args="'ab'",
         desc="which succeeds if the input string starts with `prefix`.",
         msg="Must start with {prefix}.", keys=["input", "prefix"], formatted="Must start with ab.",
         repr_arg="prefix='ab'", valid=["'abc'", "'ab'"], invalid=["'xab'", "''"],
         custom=("{input} lacks {prefix}", "xab lacks ab"), init=["prefix"], check="value.startswith(self.prefix)"),
    dict(id="multiple-of", cls="MultipleOf", sig="MultipleOf(base, *, error=None)", args="3",
         desc="which succeeds if the input number is an exact multiple of `base`.",
         msg="Must be a multiple of {base}.", keys=["input", "base"], formatted="Must be a multiple of 3.",
         repr_arg="base=3", valid=["9", "0", "-6"], invalid=["10", "4"],
         custom=("{input} is not a multiple of {base}", "10 is not a multiple of 3"), init=["base"],
         check="value % self.base == 0"),
    dict(id="unique", cls="Unique", sig="Unique(*, error=None)", args="",
         desc="which succeeds if the input sequence has no duplicate elements.",
         msg="Must not contain duplicates.", keys=["input"], formatted="Must not contain duplicates.",
         repr_arg="", valid=["[1, 2, 3]", "[]", "['a', 'b']"], invalid=["[1, 2, 1]", "['a', 'a']"],
         custom=("{input} has duplicates", "[1, 2, 1] has duplicates"), init=[], check="len(set(value)) == len(value)"),
    dict(id="hexadecimal", cls="Hexadecimal", sig="Hexadecimal(*, error=None)", args="",
         desc="which succeeds if the input is a non-empty string made only of hexadecimal digits "
              "(`0-9`, `a-f`, `A-F`, no prefix).",
         msg="Not a valid hexadecimal string.", keys=["input"], formatted="Not a valid hexadecimal string.",
         repr_arg="", valid=["'deadBEEF'", "'0'"], invalid=["'0x1f'", "''", "'xyz'"],
         custom=("bad hex: {input}", "bad hex: 0x1f"), init=[],
         check="bool(value) and all(c in '0123456789abcdefABCDEF' for c in value)"),
    dict(id="ends-with", cls="EndsWith", sig="EndsWith(suffix, *, error=None)", args="'.py'",
         desc="which succeeds if the input string ends with `suffix`.",
         msg="Must end with {suffix}.", keys=["input", "suffix"], formatted="Must end with .py.",
         repr_arg="suffix='.py'", valid=["'main.py'", "'.py'"], invalid=["'main.pyc'", "''"],
         custom=("{input} does not end with {suffix}", "main.pyc does not end with .py"), init=["suffix"],
         check="value.endswith(self.suffix)"),
    dict(id="ip-address", cls="IPAddress", sig="IPAddress(version=None, *, error=None)", args="",
         desc="which succeeds if the input string is an IP address; `version` may be `4` or `6` to accept only "
              "that version (the default `None` accepts both).",
         msg="Not a valid IP address.", keys=["input", "version"], formatted="Not a valid IP address.",
         repr_arg="version=None", valid=["'127.0.0.1'", "'::1'"], invalid=["'256.0.0.1'", "'localhost'"],
         custom=("{input} is not an IP", "256.0.0.1 is not an IP"), init=["version"],
         check="__import__('ipaddress').ip_address(value).version in ((4, 6) if self.version is None else (self.version,))",
         extra="""
def test_hidden_version():
    v4 = validate.IPAddress(4)
    assert v4('10.0.0.1') == '10.0.0.1'
    with pytest.raises(ValidationError):
        v4('::1')
    assert validate.IPAddress(version=6)('::1') == '::1'
"""),
    dict(id="not-blank", cls="NotBlank", sig="NotBlank(*, error=None)", args="",
         desc="which succeeds if the input string contains at least one non-whitespace character.",
         msg="Field may not be blank.", keys=["input"], formatted="Field may not be blank.",
         repr_arg="", valid=["'a'", "' x '"], invalid=["''", "'   '", "'\\t\\n'"],
         custom=("blank: {input!r}", "blank: ''"), init=[], check="bool(value.strip())"),
    dict(id="ascii", cls="Ascii", sig="Ascii(*, error=None)", args="",
         desc="which succeeds if the input string contains only ASCII characters.",
         msg="Must contain only ASCII characters.", keys=["input"], formatted="Must contain only ASCII characters.",
         repr_arg="", valid=["'hello'", "''"], invalid=["'héllo'", "'日本'"],
         custom=("{input} is not ascii", "héllo is not ascii"), init=[], check="value.isascii()"),
    dict(id="contains", cls="Contains", sig="Contains(item, *, error=None)", args="'x'",
         desc="which succeeds if `item` is in the input (a substring of a string, or a member of a list).",
         msg="Must contain {item}.", keys=["input", "item"], formatted="Must contain x.",
         repr_arg="item='x'", valid=["'axb'", "['x', 'y']"], invalid=["'abc'", "['y']"],
         custom=("{input} misses {item}", "abc misses x"), init=["item"], check="self.item in value"),
    dict(id="lowercase", cls="Lowercase", sig="Lowercase(*, error=None)", args="",
         desc="which succeeds if the input string is entirely lower-case (`value == value.lower()`).",
         msg="Must be lowercase.", keys=["input"], formatted="Must be lowercase.",
         repr_arg="", valid=["'abc'", "'a-1'"], invalid=["'Abc'", "'ABC'"],
         custom=("{input} has capitals", "Abc has capitals"), init=[], check="value == value.lower()"),
]

MM_HIDDEN = '''\
import pathlib
import re

import pytest

from marshmallow import validate
from marshmallow.exceptions import ValidationError

ROOT = pathlib.Path(__file__).resolve().parents[1]
NAME = {cls!r}


def make(**kw):
    cls = getattr(validate, NAME, None)
    assert cls is not None, f"marshmallow.validate.{{NAME}} is missing"
    return cls({args}{sep}**kw)


@pytest.mark.parametrize("value", [{valid}])
def test_hidden_valid(value):
    assert make()(value) == value


@pytest.mark.parametrize("value", [{invalid}])
def test_hidden_invalid(value):
    with pytest.raises(ValidationError) as e:
        make()(value)
    assert e.value.messages == [{formatted!r}]


def test_hidden_custom_error():
    with pytest.raises(ValidationError) as e:
        make(error={custom_t!r})({first_invalid})
    assert e.value.messages == [{custom_e!r}]


def test_hidden_repr():
    r = repr(make())
    assert r.startswith("<" + NAME + "("), r
    assert "error=" in r and {msg!r} in r, r
    assert {repr_arg!r} in r, r


def test_hidden_docstring():
    assert (getattr(validate, NAME).__doc__ or "").strip()


def test_hidden_changelog():
    text = (ROOT / "CHANGELOG.rst").read_text()
    first = re.split(r"\\n\\d+\\.\\d+\\.\\d+ \\(", text)[1]
    assert re.search(r"\\b" + NAME + r"\\b", first), "no entry in the unreleased version's changes"


def test_hidden_has_tests():
    word = re.compile(r"\\b" + NAME + r"\\b")
    own = pathlib.Path(__file__).name
    assert any(word.search(p.read_text(errors="replace")) for p in (ROOT / "tests").rglob("*.py") if p.name != own)
{extra}'''


def mm_prompt(v: dict) -> str:
    interp = " and ".join(f"`{{{k}}}`" for k in v["keys"])
    ex = lambda xs: ", ".join(f"`{x}`" for x in xs)  # noqa: E731
    args = f"`{v['cls']}({v['args']})`: " if v["args"] else ""
    return MM_PROMPT.format(sig=v["sig"], desc=v["desc"], msg=v["msg"], interp=interp,
                            valid_ex=args + ex(v["valid"]), invalid_ex=ex(v["invalid"]))


def mm_hidden(v: dict) -> str:
    return MM_HIDDEN.format(cls=v["cls"], args=v["args"], sep=", " if v["args"] else "",
                            valid=", ".join(v["valid"]), invalid=", ".join(v["invalid"]), formatted=v["formatted"],
                            custom_t=v["custom"][0], custom_e=v["custom"][1], first_invalid=v["invalid"][0],
                            msg=v["msg"], repr_arg=v["repr_arg"], extra=v.get("extra", ""))


def mm_gold(v: dict, d: Path) -> None:
    params = ", ".join(v["init"])
    sig = (params + ", " if params else "") + "*, error: str | None = None"
    if v["id"] == "ip-address":
        sig = "version=None, *, error: str | None = None"
    sets = "".join(f"        self.{p} = {p}\n" for p in v["init"])
    fmt = ", ".join(f"{p}=self.{p}" for p in v["init"])
    rep = " + \", \" + ".join(f'f"{p}={{self.{p}!r}}"' for p in v["init"]) or '""'
    code = f'''

class {v["cls"]}(Validator):
    """{v["desc"]}

    :param error: Error message to raise in case of a validation error.
    """

    default_message = {v["msg"]!r}

    def __init__(self, {sig}):
{sets}        self.error: str = error or self.default_message

    def _repr_args(self) -> str:
        return {rep}

    def _format_error(self, value) -> str:
        return self.error.format(input=value{", " + fmt if fmt else ""})

    def __call__(self, value):
        try:
            ok = {v["check"]}
        except (TypeError, ValueError):
            ok = False
        if not ok:
            raise ValidationError(self._format_error(value))
        return value
'''
    p = d / "src/marshmallow/validate.py"
    p.write_text(p.read_text() + code)
    p = d / "CHANGELOG.rst"
    p.write_text(p.read_text().replace("Features:\n\n", f"Features:\n\n- Add `marshmallow.validate.{v['cls']}`.\n", 1))
    t = d / "tests" / f"test_gold_{v['id'].replace('-', '_')}.py"
    t.write_text(f"from marshmallow import validate\n\n\ndef test_{v['id'].replace('-', '_')}():\n"
                 f"    assert validate.{v['cls']}({v['args']})({v['valid'][0]}) == {v['valid'][0]}\n")


# -------------------------------------------------------------------------------------- registry

FAMILIES = {
    "click-type": dict(repo="click", base=CLICK_BASE, variants=CLICK, prompt=click_prompt, hidden=click_hidden,
                       gold=click_gold, hidden_path="tests/test_rw_hidden.py",
                       # Click's CI invariant: importing click stays light. Restored before scoring.
                       pristine=["tests/test_imports.py"],
                       command=".venv/bin/python -m pytest -q -p no:cacheprovider tests"),
    "mm-validator": dict(repo="marshmallow", base=MM_BASE, variants=MM, prompt=mm_prompt, hidden=mm_hidden,
                         gold=mm_gold, hidden_path="tests/test_rw_hidden.py", pristine=[],
                         command=".venv/bin/python -m pytest -q -p no:cacheprovider tests"),
}


# The confirming experiment's third family (Go), kept under confirm/.
try:
    import sys as _sys
    _sys.path.insert(0, str(Path(__file__).resolve().parent / "confirm"))
    import family_urfave as _fu  # noqa: E402

    FAMILIES["go-flag"] = _fu.FAMILY
except ImportError:  # pragma: no cover
    pass


def task_id(family: str, vid: str) -> str:
    return f"{family}--{vid}"


def parse_task(tid: str) -> tuple[dict, dict]:
    fam, vid = tid.split("--", 1)
    f = FAMILIES[fam]
    v = next(x for x in f["variants"] if x["id"] == vid)
    return f, v


def order(family: str) -> list[str]:
    return [task_id(family, v["id"]) for v in FAMILIES[family]["variants"]]


def names(family: str) -> list[str]:
    key = {"click-type": "name", "mm-validator": "cls", "go-flag": "flag"}[family]
    return [v[key] for v in FAMILIES[family]["variants"]]


if __name__ == "__main__":
    for fam in FAMILIES:
        for tid in order(fam):
            f, v = parse_task(tid)
            print(tid, "|", f["prompt"](v)[:160].replace("\n", " "))
    assert re  # used by the gold code's generated modules
