# AGENTS.md

- Editable-install the package before running any script or import (`import attr` fails otherwise): `pip install -e . -q`.
- For quick one-off checks without a full install, run `PYTHONPATH=src python3 ...` instead.
- Test/dev dependencies (pytest, hypothesis) are not preinstalled — run `pip install -e ".[dev]" -q` or `pip install pytest hypothesis -q` before invoking pytest.
- Check for an existing `.venv` first (`ls .venv/bin/pytest`); if present, use `.venv/bin/pytest` rather than reinstalling.
- Run the whole suite with `python -m pytest tests/ -x --tb=short --ignore=tests/test_packaging.py` — `test_packaging.py` fails against an editable install and should be skipped.
- Run a single file with `python -m pytest tests/test_make.py -x --tb=short`.
- Narrow to related cases with `-k "<keyword>"` while iterating on a fix.
- Core class-building logic lives in `src/attr/_make.py`; check there first for `attr.s`/`attr.define`/`Converter`/validator behavior.
- After editing source under `src/attr/`, rerun the relevant test file (and the full suite before finishing) to confirm the fix.
- Reproduce the reported behavior with a minimal script before touching source, then rerun the same script after the fix to verify.
- Stage and commit only the files you actually changed, with a descriptive message explaining the bug and fix.
