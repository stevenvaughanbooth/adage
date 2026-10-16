# AGENTS.md

- Source lives in `src/click/`, tests in `tests/` (e.g. `tests/test_options.py`, `tests/test_shell_completion.py`, `tests/test_termui.py`) — one test file roughly per source module.
- Check for a `.venv` first; if present, use it (`source .venv/bin/activate` or call `.venv/bin/python` directly) — it typically already has click installed editably plus pytest.
- If no venv, or `import click` / running a script fails, run `pip install -e .` before anything else.
- If `python -m pytest` fails because pytest isn't found, `pip install pytest` (or `pip install -e . -q && pip install pytest -q`).
- Don't run throwaway repro scripts with plain `python script.py` from a fresh checkout — it will `ImportError` on click until the editable install (or `PYTHONPATH=src`) is in place.
- Run the whole suite: `python -m pytest tests/`.
- Run a single file or test: `python -m pytest tests/test_shell_completion.py -v` or `python -m pytest tests/test_shell_completion.py::test_name -xvs`.
- A full-suite run can include unrelated pre-existing failures; when confirming a fix, prefer running the specific test file(s) for the area you touched rather than assuming full-suite failures are caused by your change.
- After editing anything in `src/click/`, re-run the relevant test file before committing.
- Before committing, run `git status`/`git diff` to confirm only the intended source and test files are staged.
