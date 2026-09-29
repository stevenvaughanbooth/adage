# How changes of this kind were done here before

Learned from 1 earlier session(s) of the same kind of task in this repository (1 passed the project's checks). Use it as a checklist; the specifics of your task differ.

## Files such a change touched (in order)

- `src/marshmallow/validate.py` (1/1 sessions)
- `tests/test_validate.py` (1/1 sessions)
- `CHANGELOG.rst` (1/1 sessions)

## What the edit looked like last time (another variant; copy the format, not the content)

`src/marshmallow/validate.py`:
```
class StartsWith(Validator):
    """Validator which succeeds if the value starts with the specified prefix.

    :param prefix: The prefix that the value should start with.
    :param error: Error message to raise in case of a validation error.
        Can be interpolated with `{input}` and `{prefix}`.

    default_message = "Must start with {prefix}."

    def __init__(self, prefix: str, *, error: str | None = None):
        self.prefix = prefix
        self.error: str = error or self.default_message

    def _repr_args(self) -> str:
…
```
`tests/test_validate.py`:
```
def test_starts_with_valid():
    validator = validate.StartsWith("ab")
    assert validator("ab") == "ab"
    assert validator("abc") == "abc"
    assert validator("abcd") == "abcd"
    assert validator("a") != "ab"  # This will not raise


def test_starts_with_invalid():
    validator = validate.StartsWith("ab")
    with pytest.raises(ValidationError, match="Must start with ab."):
        validator("xab")
    with pytest.raises(ValidationError, match="Must start with ab."):
        validator("")
…
```
`CHANGELOG.rst`:
```
- Add `marshmallow.validate.StartsWith` validator for validating strings that start with a specified prefix.
```

## Commands

- `find . -type f -name *.py` (ran in 1/1 sessions, 1/1 succeeded)
- `find tests -name *.py` (ran in 1/1 sessions, 1/1 succeeded)
- `find . -name CHANG* -o -name` (ran in 1/1 sessions, 1/1 succeeded)
- `grep -n __all__ src/marshmallow/validate.py` (ran in 1/1 sessions, 1/1 succeeded)
- `wc -l tests/test_validate.py` (ran in 1/1 sessions, 1/1 succeeded)
- `ls -la` (ran in 1/1 sessions, 1/1 succeeded)
- avoid `python -m pytest tests/test_validate.py -xvs`: failed 3/4 times (`/usr/local/bin/python: No module named pytest`)
- avoid `/tmp/adage-research/recurring-workflows/runs/mm-validator/runs/train-mm-validator--starts-with-r1/home/.local/bin/pytest tests/test_validate.py -xvs`: failed 1/1 times (`ImportError while loading conftest '/tmp/adage-research/recurring-workflows/runs/mm-validator/runs/train-mm-validator--starts-with-r1/repo/tests/conftest.py'.`)

## Finishing check

The passing sessions ended by running `python -m pytest tests/test_validate.py -xvs` successfully.
