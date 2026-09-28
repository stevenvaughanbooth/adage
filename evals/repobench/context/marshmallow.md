# AGENTS.md

- Run `pip install -e . -q` before anything else; running tests or importing marshmallow directly fails otherwise.
- Run `pip install pytest -q` too; pytest is not preinstalled.
- Also install `simplejson`, `python-dateutil`, and `pytz` — some tests depend on them and fail with import/collection errors without them.
- Run the whole suite with `python -m pytest tests/ -q`.
- Run a single file with `python -m pytest tests/test_<name>.py -q`.
- Run a single test with `python -m pytest tests/<file>.py::<test_name> -xvs`.
- Don't guess a test's exact name or class from the bug description alone; `grep -n "def test_"` or `grep -B 10 "def test_name"` in the relevant `tests/*.py` file to confirm the real test name/class before invoking pytest.
- If a targeted pytest invocation fails only because pytest/deps aren't installed, install the missing package and rerun the same command rather than switching approach.
- After editing, run the specific test file(s) related to the change, then run `tests/test_deserialization.py`, `tests/test_serialization.py`, and `tests/test_fields.py` (or `tests/test_schema.py` for schema-level changes) to check for regressions.
- Source lives under `src/marshmallow/`; tests live under `tests/`.
- Before committing, run `git status` and `git diff <changed files>` to review the exact change.
- Commit only the specific files changed (e.g. `git add src/marshmallow/x.py tests/test_x.py`), not `git add -A`.
