# How changes of this kind were done here before

Learned from 4 earlier session(s) of the same kind of task in this repository (3 passed the project's checks). Use it as a checklist; the specifics of your task differ.

## Files such a change touched (in order)

- `src/click/types.py` (4/4 sessions)
- `src/click/__init__.py` (4/4 sessions)
- `tests/test_types/<new>.py` (4/4 sessions)
- `docs/parameter-types.md` (4/4 sessions)
- `docs/api.md` (4/4 sessions)
- `CHANGES.md` (4/4 sessions)

## What the edit looked like last time (another variant; copy the format, not the content)

`src/click/types.py`:
```
class Duration(ParamType[timedelta]):
    """The Duration type converts duration strings into `timedelta` objects.

    A duration string consists of one or more `<integer><unit>` parts, where
    units must be in the order: days (d), hours (h), minutes (m), seconds (s).

    Examples of valid input:
    - ``1h30m`` - 1 hour and 30 minutes
    - ``45s`` - 45 seconds
    - ``2d`` - 2 days
    - ``1d2h3m4s`` - 1 day, 2 hours, 3 minutes, and 4 seconds

    Examples of invalid input:
    - ``10`` - no unit specified
…
```
`src/click/__init__.py`:
```
from .types import Duration as Duration
from .types import DURATION as DURATION
```
`tests/test_types/<new>.py`:
```
import pytest
from datetime import timedelta

import click


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("1h30m", timedelta(hours=1, minutes=30)),
        ("45s", timedelta(seconds=45)),
        ("2d", timedelta(days=2)),
        ("1d2h3m4s", timedelta(days=1, hours=2, minutes=3, seconds=4)),
        ("1d", timedelta(days=1)),
…
```
`docs/parameter-types.md`:
```
(duration-type)=

### Duration

The {class}`Duration` type converts duration strings into `datetime.timedelta` objects.
Duration strings consist of one or more `<integer><unit>` parts, where units must be
specified in order: days (`d`), hours (`h`), minutes (`m`), and seconds (`s`).

```{eval-rst}
.. click:example::

    @click.command()
    @click.option("--timeout", type=click.DURATION)
    def main(timeout):
…
```
`docs/api.md`:
```
.. autodata:: DURATION
```
`CHANGES.md`:
```
- Add a built-in {class}`Duration` parameter type and {data}`DURATION` instance.
  It accepts duration strings like ``1h30m``, ``45s``, ``2d``, or ``1d2h3m4s``
  and converts them to `datetime.timedelta` objects.
```

## Commands

- `find . -type f -name *.py` (ran in 4/4 sessions, 4/4 succeeded)
- `git status` (ran in 4/4 sessions, 4/4 succeeded)
- `ls -la .` (ran in 3/4 sessions, 3/3 succeeded)
- `ls -la docs` (ran in 2/4 sessions, 2/2 succeeded)
- `source .venv/bin/activate && python -m pytest` (ran in 2/4 sessions, 11/11 succeeded)
- `source .venv/bin/activate && python` (ran in 2/4 sessions, 5/5 succeeded)
