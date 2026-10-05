# Explicit conservative cap charges

An unknown terminal bill remains unknown. When the human explicitly authorizes charging a stopped
attempt's entire original reservation, `Budget.resolve_unknown_at_cap(run, authorization)` records
`status: cap_charged`, `charged_usd: <original cap>`, and `cost_usd: null`. Its resolution embeds the
human statement, source and timestamp. This is conservative admission accounting, not a measured
provider cost or a successful evaluation result.

`Budget.spent` includes measured costs plus these charges. `measured_spent` and
`conservative_charges` expose them separately. Restart validates the charge and its authorization.
A smaller charge, missing cap, missing human record, ambiguous run, active/reserved/known-cost attempt,
or altered resolution is rejected. Other unknown attempts and persistence/overrun/operator blocks
remain blocking. An unsuccessful persistence operation cannot authorize another dispatch.

The file helper previews by default and never launches Claude:

```sh
python3 -B -m evals.live.resolve_cap STATE_JSON HUMAN_AUTHORIZATION_JSON \
  --total-usd 27.50 --session-cap-usd 0.50
```

After reviewing its concrete output, the authorized operator adds `--apply`. This uses the same
execution lock as live runners, requires the corresponding run to be terminal, compares the state
hash pinned by the authorization document, preserves an immutable before-state backup, and writes
the updated ledger durably. It does not increase a runner's hard cap or approve future unknown bills.

The authorization document has `schema_version: 1`, the exact `user_reply`, `approved_question`,
`original_paid_state_sha256`, and a `scope` containing `run`, `original_cap_usd`, `charged_usd`, and
`actual_cost_usd: null`. Both cap amounts must equal the original reservation. The API authorization
record uses `kind: human_cap_charge`, `approved: true`, exact `run`/`cap_usd`, `statement`, and `source`.

This shared source change invalidates previous live recipes that froze `budget.py`. Preserve those
manifests and prepare an explicit new protocol revision chained to their hashes and the recorded
human authorization. Never silently replace a historical recipe's expected source hash. Existing
outcome gates requiring actual terminal costs continue to reject cap-charged attempts as efficacy
results; a startup-censored probe remains visible as a censored attempt.
