# AGENTS.md

- This repo is a single Go module/package (`cli`) with all source at the repo root; run all commands from there.
- Run the full suite with `go test ./...` (add `--timeout=10m` if a run seems slow).
- Run a single test with `go test -run TestName -v` — no need for `./...` since it's one package.
- Do not create standalone scratch `main.go` files that `go run` and import `github.com/urfave/cli/v3` from inside this repo; the import/build fails. Instead, add a temporary `_test.go` file in the package and exercise it via `go test -run`.
- Never invoke `go run` with an inline heredoc (`go run << 'EOF' ...`) or with `-exec echo`; write the snippet to a real `.go` file first.
- If a manual standalone repro is truly needed, create it as its own module in `/tmp` and `go get github.com/urfave/cli/v3` there rather than referencing the local repo path.
- Delete any scratch/debug `.go` files you created (e.g. `test_*.go` reproduction files) before finishing — confirm with `git status`.
- After editing source, rerun the full suite (`go test ./...`), not just the targeted test, before considering the change complete.
- Run `git status` and `git diff` on the exact files before staging to confirm only intended changes are included.
- Stage only the actual source/test files that were fixed (e.g. `git add foo.go foo_test.go`); never `git add .` given the scratch files produced during debugging.
- Write commit messages as a short title plus a multi-line body (via heredoc) explaining the bug and the fix.
