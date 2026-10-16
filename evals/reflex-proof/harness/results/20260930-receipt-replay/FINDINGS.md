# Offline receipt-consumption replay of transfer-v3

Post-hoc, zero-spend diagnostic. It does not change the frozen transfer-v3 decision (0/4).

`receipt_replay.py` rebuilds each session's timeline from the collected `production-hooks.jsonl`:
agent tool requests with the whole-source digest observed before each request, and journal
receipts with the digest at execution. The digest excludes `.git`, `.venv` and declared caches.
`timelines/` holds the eight compact timelines (repository paths replaced by `<repo>`);
`sources.json` binds each one to the SHA-256 of its archived native hook log. `replay.json` is
the classifier output. `test_receipt_replay.py` pins these numbers and the negative controls.

| Task | CPU receipts | Repeats of the receipt (exact + equivalent) | Narrower nodes inside its scope |
| --- | ---: | ---: | ---: |
| Odd | 1 | 2 (2 + 0) | 4 |
| Finite | 1 | 1 (1 + 0) | 2 |
| NoAdjacentDuplicates | 1 | 2 (1 + 1) | 1 |
| MaxUtf8Bytes | 1 | 1 (1 + 0) | 2 |

Every repeat ran on the same source digest as the receipt; no automatic session edited source
after its receipt. The equivalent repeat differs only by `--tb=short` and an output pipe. The
learned (no-execution) arms ran the same shape of routine: targeted node tests, then the whole
file. The automatic arm's post-receipt requests reproduce that habit rather than consuming the
receipt. In all eight sessions the first test request used the bare `python`, and Adage's
project-interpreter hint then supplied the `.venv` form of the same narrow node. That hint fires
in both arms, so it does not explain the difference; it does show that a receipt-aware hint
should not offer a runnable command whose scope a fresh receipt already covers.

Candidate mechanisms, in order of the evidence for each:

1. **Scope mismatch.** Agents first verify their new tests by node id. A quiet file-level `-q`
   receipt without test counts or node coverage does not tell them their new tests ran. Of the 21
   post-receipt test requests, 9 are narrower subsets, 6 repeats, 4 bare-interpreter attempts
   and 2 `-k` selections.
2. **Habitual final check.** The exact repeat follows `git diff` review near the end, as in
   the no-execution arm. Completion prose in the advice envelope did not change it.
3. **Channel and framing.** The old completion used the advice envelope and said it was
   "not requested by the user". The candidate production integration (structured receipt plus
   a SessionStart responsibility contract) targets this; its live effect is untested here.

Predeclared scoring for new studies (see `docs/launch-gate-protocol.md`): a repeat has the same
interpreter token and targets, differing only in output-only flags (`-q`, `-v`, `-vv`, `-s`,
`-x`, `-rA`, `--tb=*`, `--color=*`, `--no-header`, `--disable-warnings`, `-p no:cacheprovider`)
and output redirection or pipes into other commands. `-k`, `-m`, other flags, other
interpreters and different targets are different actions. Narrower nodes are reported
separately and never count as eliminations or repeats.
