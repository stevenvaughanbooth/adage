# CPU transfer preparation: preserve the approval failure

The first preparation dispatched zero paid sessions. A human ran the approval script, but its
`.pytest_cache/**` declaration was rejected by the shared glob validator; the root bytecode
wildcard was also unsupported. The grant preflight correctly prevented all eight model calls.

The fix admits exactly `.pytest_cache/**` as an additional **cache** declaration. It does not admit
hidden outputs, `.env`, `.codex`, `.git`, traversal or root wildcards. Bytecode declarations now
name `tests/` and `src/` explicitly. Pack validation and runtime requests use the same predicate.
88 pack/execution library tests and 84 harness tests passed; both release binaries built.

A separate local replay of the identical ten histories again certifies four triggers for the same
pytest command. No paid training was repeated and no evidence threshold changed. A replacement
transfer preparation pins both rebuilt executable hashes, retains the same eight assignments and
strict scoring, and requires fresh human grants. The original preparation and its failure remain
in this record. These are preparation results, not evidence of action elimination or CPU savings.

The replacement manifest and approval proposal bind every assignment and requested scope. Full
native inputs remain hash-addressed in the local archive. Approval files and permission grants
are deliberately excluded from published artifacts.
