import pathlib
import re

import pytest

from marshmallow import validate
from marshmallow.exceptions import ValidationError

ROOT = pathlib.Path(__file__).resolve().parents[1]
NAME = 'LatLong'


def make(**kw):
    cls = getattr(validate, NAME, None)
    assert cls is not None, f"marshmallow.validate.{NAME} is missing"
    return cls(**kw)


@pytest.mark.parametrize("value", [(0, 0), [51.5, -0.12], (-90, 180), (90.0, -180.0)])
def test_hidden_valid(value):
    assert make()(value) == value


@pytest.mark.parametrize("value", [(91, 0), (0, 180.5), (10, 20, 30), (10,)])
def test_hidden_invalid(value):
    with pytest.raises(ValidationError) as e:
        make()(value)
    assert e.value.messages == ['Must satisfy LatLong.']


def test_hidden_custom_error():
    with pytest.raises(ValidationError) as e:
        make(error='invalid: {input}')((91, 0))
    assert e.value.messages == ['invalid: (91, 0)']


def test_hidden_repr():
    r = repr(make())
    assert r.startswith("<" + NAME + "("), r
    assert "error=" in r and 'Must satisfy LatLong.' in r, r
    assert '' in r, r


def test_hidden_docstring():
    assert (getattr(validate, NAME).__doc__ or "").strip()


def test_hidden_changelog():
    text = (ROOT / "CHANGELOG.rst").read_text()
    first = re.split(r"\n\d+\.\d+\.\d+ \(", text)[1]
    assert re.search(r"\b" + NAME + r"\b", first), "no entry in the unreleased version's changes"


def test_hidden_has_tests():
    word = re.compile(r"\b" + NAME + r"\b")
    own = pathlib.Path(__file__).name
    assert any(word.search(p.read_text(errors="replace")) for p in (ROOT / "tests").rglob("*.py") if p.name != own)
