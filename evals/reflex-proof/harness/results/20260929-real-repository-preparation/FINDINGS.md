# Real-repository preparation and startup-censored probe

This is preparation evidence, not a completed learned-reflex coding comparison.

All four prospective held-out feature tasks validate on their pinned upstream repositories:
the unmodified base produces real hidden feature failures, and the reference solution passes.
Marshmallow's two gold suites each pass 1,201 tests; Click's pass 2,252 and 2,253 tests.
[Complete results](validation.json) retain suite counts and hidden failures.

The first validation failed because the confined scorer could not read canonical system timezone
data and Click's test expected the `/tmp` directory node to be readable. Those failures remain in
[the original report](validation-initial-failed.json). The correction grants system timezone data
and literal temp-directory nodes, without granting their descendants. Disposable canaries verify
that private file reads, writes outside the checkout and network connections remain denied.
[Direct scorer confinement](confinement.json) passes after the correction.

The subsequent real Claude probe was cancelled at its 120-second startup deadline. It emitted
no stream records or native transcript and showed no model request in the version-specific debug
log. It did not reach the canary command, so this is **not a passing live-agent sandbox test**.
There is no terminal cost: [accounting](paid-state.json) records an unknown attempt with a $0.50
reservation and blocks further paid dispatch. A startup failure is not an agent task failure.

The [export manifest](report.json) records source/export hashes and redaction scope. Broad debug
logs, host credentials and local execution grants are excluded. Earlier protocols and failed
attempts are preserved; this run provides no coding productivity or GPU-energy statistic.
