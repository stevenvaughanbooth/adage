# How changes of this kind were done here before

Learned from 4 earlier session(s) of the same kind of task in this repository (4 passed the project's checks). Use it as a checklist; the specifics of your task differ.

## Files such a change touched (in order)

- `src/marshmallow/validate.py` (4/4 sessions)
- `tests/test_validate.py` (4/4 sessions)
- `CHANGELOG.rst` (4/4 sessions)

## What the edit looked like last time (another variant; copy the format, not the content)

`src/marshmallow/validate.py`:
```
class Hexadecimal(Validator):
    """Validate a hexadecimal string.

    A valid hexadecimal string is non-empty and contains only hexadecimal
    characters (0-9, a-f, A-F). No prefix (like '0x') is allowed.

    :param error: Error message to raise in case of a validation error.
        Can be interpolated with `{input}`.

    Example: ::

        from marshmallow import validate, ValidationError

        validator = validate.Hexadecimal()
…
```
`tests/test_validate.py`:
```
@pytest.mark.parametrize(
    "valid_hex",
    [
        "0",
        "1",
        "a",
        "f",
        "A",
        "F",
        "deadBEEF",
        "DEADBEEF",
        "deadbeef",
        "0123456789",
        "abcdef",
…
```
`CHANGELOG.rst`:
```
- Add `marshmallow.validate.Hexadecimal` validator for validating hexadecimal strings.
```

## Commands

- `find . -type f -name *.py` (ran in 4/4 sessions, 4/4 succeeded)
- `wc -l tests/test_validate.py` (ran in 4/4 sessions, 4/4 succeeded)
- `ls -la` (ran in 4/4 sessions, 4/4 succeeded)
- `grep -n __all__ src/marshmallow/validate.py` (ran in 3/4 sessions, 3/3 succeeded)
- `pip install -e . -q` (ran in 3/4 sessions, 3/3 succeeded)
- `pip install simplejson -q` (ran in 3/4 sessions, 3/3 succeeded)
- avoid `python -m pytest tests/test_validate.py -xvs`: failed 5/6 times (`/usr/local/bin/python: No module named pytest`)
