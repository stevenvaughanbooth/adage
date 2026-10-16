# Follow-up: card framing (not yet applied or validated)

The pre-registered showcase ([../RESULTS.md](../RESULTS.md)) found a harm in one family. On
hyperium/http, hidden tests passed 4 of 16 times with Adage against 14 of 16 in control. The
tasks asked for a test. None of the 4 training sessions wrote one, so the learned card listed only
`name.rs` and `mod.rs`, and agents treated the card as the whole job.

`card-framing-fix.patch` holds the fix, stopped on 2026-09-30 before its live check ran. It has
two commits, taken from `74461ac` on `claude/repo-maturity-roadmap-l95ohs`:

1. **`cards: put the task first and name the tests the repository keeps`** (`crates/adage-learn`,
   `crates/adage-tracks`):
   - the rendered card says it records what earlier sessions did, and that the task's own
     requirements, tests included, come first;
   - it names where the repository keeps tests for the edited files, even when no training
     session touched them;
   - it includes regression tests.
2. **`showcase: post-hoc follow-up (card framing) recorded in prereg.json before any follow-up
   session`**. This adds:
   - the amendment in `prereg.json`;
   - the configs for re-running the Adage arm of http-header and mm-validator against the
     showcase's existing control sessions;
   - `followup_cardfix.py`.

**Status: not applied, gates not run, no live session completed.** To pick it up:

```bash
git am evals/showcase/followup/card-framing-fix.patch   # on a branch
cargo fmt --all --check
cargo clippy -p adage-learn -p adage-tracks -p adage-runtime --all-targets -- -D warnings
cargo test -p adage-learn -p adage-tracks -p adage-runtime
# then the live follow-up (budget about $15): see followup_cardfix.py and the prereg amendment
```
