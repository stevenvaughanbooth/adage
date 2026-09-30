# Native cwd metadata diagnostic

Result: **inconclusive**, retained rather than retried under weaker permission rules.

The prepared six-command sequence began with `pwd`, whose output agreed with recorded cwd.
The native harness then denied `cd tests`; the model stopped after two calls. The test therefore
did not establish whether recorded cwd tracks a successful shell directory change. The learner's
conservative cwd requirement remains unchanged. This diagnostic is excluded from learning and
efficacy analysis. Reported cost: $0.0137145, within its $0.15 cap and the shared evaluation ledger.

[report.json](report.json) preserves the expected sequence, actual tool calls, denial, native
transcript hash and cost. No execution grant was created and no permission restriction was bypassed.
