"""The question's examples, through the grammar and back.

    python3 demo.py

Labels each example against a real checkout's facts (click and vuejs/core from the RepoBench
runs), with `src/click/core.py` (resp. `packages/runtime-core/src/renderer.ts`) edited earlier in
the session, then compiles the abstract action back to the command a directive would carry.
"""
from __future__ import annotations

import corpus
import compile_back as CB
import grammar as G
import shell

EXAMPLES = {
    "click": ("src/click/core.py", [
        ".venv/bin/python -m pytest tests/test_commands.py::test_x -xvs | head -50",
        "uv run pytest -k parser",
        "python -m pytest tests/",
        "source .venv/bin/activate && python -m pytest tests/test_options.py -q 2>&1 | tail -20",
        "python3 -m pytest tests/test_basic.py::test_group -xvs",
        "python3 -c \"import click; print(click.__file__)\"",
        "cat > /tmp/repro.py << 'EOF'\nimport click\n@click.command()\ndef cli(): pass\nEOF\npython /tmp/repro.py",
        ".venv/bin/python << 'EOF'\nfrom click.testing import CliRunner\nEOF",
        "pip install -e . -q",
    ]),
    "vue-core": ("packages/runtime-core/src/renderer.ts", [
        "npx vitest run packages/runtime-core",
        "npm test -- packages/runtime-core/__tests__/rendererChildren.spec.ts --run 2>&1 | tail -30",
        "pnpm vitest run -t 'keyed children'",
        "node /tmp/repro.js",
    ]),
}


def main():
    L = corpus.load()
    for repo, (edited, cmds) in EXAMPLES.items():
        l = next(x for x in L if x.sess.repo == repo and x.sess.phase == "eval")
        fx, base, cwd = l.facts, l.base, l.sess.cwd
        print(f"== {repo} (edited: {edited}) ==")
        for cmd in cmds:
            segs = shell.segments(cmd, cwd)
            acts = [G.classify_seg(s, fx, cmd) for s in segs]
            a = min(acts, key=lambda z: G.ROLE_PRIORITY.index(z.role))
            a.relation = G.relation_of(a, [edited], set(), "", repo, base, fx)
            print(f"  {cmd.splitlines()[0][:74]:<76} -> {a.level(3):<38} intent {a.intent():<28} [{a.env}]")
        comp = CB.compile_test_run(fx, repo, base, [edited], [])
        if comp:
            print(f"  compile(test-run|project|file|src-test) -> {' '.join(comp[0])}   safe={CB.safe(comp[0], fx)}")
        print()


if __name__ == "__main__":
    main()
