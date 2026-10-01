import datetime
import ipaddress
import pathlib
import re

import pytest

import click
from click.testing import CliRunner

ROOT = pathlib.Path(__file__).resolve().parents[1]
NAME = 'KEY_VALUE'
VALID = [('mode=fast', ('mode', 'fast')), ('EMPTY=', ('EMPTY', '')), ('url=a=b', ('url', 'a=b')), ('_max_2=10 MB', ('_max_2', '10 MB'))]
INVALID = ['noequals', '=value', '1st=x', 'two words=x', 'naïve=1']


def _type():
    t = getattr(click, NAME, None)
    assert t is not None, f"click.{NAME} is missing"
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
    word = re.compile(r"\b" + NAME + r"\b")
    assert word.search((ROOT / "docs" / "api.md").read_text()), "not in the API reference"
    assert word.search((ROOT / "docs" / "parameter-types.md").read_text()), "not in the parameter types guide"


def test_changelog():
    text = (ROOT / "CHANGES.md").read_text()
    first = re.split(r"^## Version ", text, flags=re.M)[1]
    assert re.search(r"\b" + NAME + r"\b", first), "no entry in the unreleased version's changes"


def test_has_tests():
    word = re.compile(r"\b" + NAME + r"\b")
    own = pathlib.Path(__file__).name
    assert any(word.search(p.read_text(errors="replace")) for p in (ROOT / "tests").rglob("*.py") if p.name != own)
