# harbor-dispatch

Berth planning, cargo manifests and arrival estimates for the terminal operations team.

The code is TypeScript and runs directly on Node 26; there is no build step and there are no
runtime dependencies. Port and vessel reference data live under `registry/`. Incidents are
tracked in the ops queue.
