---
name: click-bugfix
description: How to reproduce, test and fix a reported bug in the pallets/click repository. Use when a prompt reports wrong behaviour in click (options, completion, prompts, help output).
---

# Fixing a bug in click

Source lives in `src/click/`, tests in `tests/`, roughly one test file per module
(`src/click/shell_completion.py` is tested by `tests/test_shell_completion.py`). The project's
virtualenv `.venv` has click installed editable and pytest; the system Python has neither.

## Procedure

1. **Reproduce the bug first.** Put the example from the report in `repro.py` at the repository root
   and run it with `.venv/bin/python repro.py`. Expect it to show the wrong behaviour.
2. **Find the code.** Search the package, for example `grep -rn "prompt_suffix" src/click`, and read
   the module that defines it.
3. **Run the tests for that module** with `.venv/bin/python -m pytest tests/test_<module>.py -q`.
   Never use `python -m pytest`, `python3 -m pytest` or bare `pytest` instead of
   `.venv/bin/python -m pytest`: they fail with `No module named pytest`.
4. **Fix the code** in `src/click/<module>.py` and add a regression test to `tests/test_<module>.py`.
5. After editing `src/click/*.py`, run `.venv/bin/python -m pytest tests -q -x` to check the whole
   suite. Expect `passed` and no failures.
6. **Before you finish**, run `.venv/bin/python repro.py` again to confirm the fix, then delete
   `repro.py`.
