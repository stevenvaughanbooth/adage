# AGENTS.md

- Run the whole test suite with `npm test -- --run` (plain `npm test` launches Vitest watch mode).
- Run a single spec file: `npm test -- packages/<pkg>/__tests__/<file>.spec.ts`.
- Run a single test case: add `-t "<test name>"` to the single-file command above.
- While iterating on a fix, scope runs to the package's test directory (`npm test -- packages/<pkg>/__tests__/`) before running the whole suite.
- After a fix passes its own spec, also run the directly related packages' suites (e.g. `runtime-core`, `runtime-dom`, `server-renderer`, `packages/vue/__tests__/e2e/`) to catch cross-package regressions.
- Only run `npm run build` if a test needs compiled output (e.g. requiring `dist/*.cjs.js` directly); source-level tests import from `src` and don't need it.
- Do not use `sed -i` to patch regex-bearing source lines — shell quoting mangles the regex; edit the file directly instead.
- Do not write throwaway scratch scripts (`cat > /tmp/foo.js` + manual `node`/require) to probe behavior; add/extend a case in the relevant `__tests__/*.spec.ts` file and run it through Vitest.
- Before committing, run `git status` and `git diff` to confirm only intended files are staged.
- Write commit messages via a heredoc (`git commit -m "$(cat <<'EOF' ... EOF)"`); verify with `git log -1` and correct with `git commit --amend` rather than an extra commit.
- After any source edit, re-run the specific spec file for that change before considering the task done.
