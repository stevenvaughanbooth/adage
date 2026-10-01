# Edit-then-verify receipt diagnostic

The clean receipt-edit-v2 cohort completed all eight assignments after stdin isolation.
Baseline agents requested the verifier in 2/2 sessions, generic completion prose in 2/2,
structured receipts in 1/2, and structured receipts with startup responsibility in 0/2.
All eight final implementations passed the independent verifier; its protected source stayed
unchanged. All six non-baseline hooks executed the CPU verifier successfully.

This is an authored two-file fixture, not a learned reflex or a real-repository benchmark.
The sample is two runs per arm in fixed order; generic wording was labeled simulated.
Use the result to choose the next integration experiment, not to estimate launch savings.
Provider cost for this cohort was $0.1513985. Full per-assignment results are retained.

receipt-edit-v1 is excluded: one agent reproduced the heredoc launcher's program in an
attempted Bash command. Permissions denied that command. The runner inherited stdin;
it now connects Claude's stdin to DEVNULL because the task prompt is supplied in argv.
A local child-process regression fails before the fix and passes afterward. No outcomes
were rerun within the old cohort, and its costs and raw records remain preserved.

The production candidate now supplies a scoped SessionStart responsibility contract and
journal-backed completion metadata. Unlike the diagnostic, production does not expose raw
stdout or fixture test counts. Its exact native live effect, stale/failure/explicit-rerun
controls, and new real-repository cohort remain to be tested. See docs/launch-handoff.md.
