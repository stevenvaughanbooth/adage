# S8 · Normaliser coverage

**Informs:** the normaliser (Design §11, Architecture §4.7) · **Wave:** 2 · **Budget:** one day, no
tokens

## Question

How far do **tree-sitter-bash** plus **per-command flag grammars** get at turning real coding-agent
tool calls into canonical actions — and what is left in `unknown`?

## Why it matters

In the early proof of concept, 71% of tool calls were shell commands. Every downstream component —
templates, routes, familiar commands, follow/derail detection, effect classification — depends on
the normaliser. The quality gate is ≥ 95% of calls normalised to something other than `unknown`.

## Method

1. **Rust normaliser prototype** using `tree-sitter` + `tree-sitter-bash`:
   - unwrap `bash -lc` / `sh -c`; track `cd`; split `&&`, `||`, `;`; pipelines with pure-filter tails
   - compound constructs (`for`, `while`, `if`, `case`, subshells, `$(…)`) — parse properly rather
     than treating them as opaque, and decide what a loop over reads should become
   - redirects, heredocs, `tee`, `sed -i`, and other mutation signals
   - per-command flag grammars as **data** (a table), covering at least: `cat head tail sed nl rg grep
     find fd ls tree git npm pnpm yarn cargo go make python node jq curl wget`
   - effect classification into the Design §8.6 classes (observe, verify, regenerate, environment,
     author, publish), with unknown commands defaulting to environment
   - argument templating: typed placeholders for paths, IDs, SHAs, numbers, URLs
2. **Corpus.** The Claude Code transcripts on this machine (`~/.claude/projects/**`, including
   `subagents/`), read-only. Also port the proof of concept's normaliser cases from
   `research/poc/test/shell.test.ts`.
3. **Golden fixtures:** ≥ 300 command → canonical-action cases. **Fixtures committed to git must be
   synthetic or scrubbed** — no real paths, hosts, tokens, or identifiers from the user's sessions.
   Any analysis output derived from real transcripts goes in git-ignored `local/`, and only
   aggregate statistics go in `results/`.
4. **Measure** on the real corpus: share normalised to non-`unknown`, share low-confidence, top
   unhandled shapes, effect-class distribution, template repeat rate (literal vs templated — the
   proof of concept saw 37% → 73%), and throughput (calls per second).
5. Compare against the TypeScript proof-of-concept normaliser on the same corpus.

## Pass / fail

≥ 95% non-`unknown` on the real corpus, ≥ 300 passing golden fixtures, and a ranked list of what
remains.

## Deliverables

`FINDINGS.md`, the Rust prototype, the fixture suite, the flag-grammar table, and aggregate
`results/`.
