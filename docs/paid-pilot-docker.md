# Run the paid pilot in Docker Desktop on macOS

Docker Desktop can supply the Linux execution host for the [paid pilot](paid-pilot.md).
This recipe was checked on an Apple Silicon Mac with Linux arm64, Rust 1.98.1,
Python 3.11.2, Bubblewrap 0.8.0 and Claude Code 2.1.287. It preserves the Linux
collector and its Bubblewrap boundary. The namespace probe and free checks must
pass on each host before model calls. Docker Desktop must already be installed
and running with Linux containers.

Run operator-only commands in a normal terminal. An agent may prepare the
container and run free checks, but must forward its agent markers when entering
it. Do not unset markers, use privileged mode, add capabilities, turn off
seccomp, or use the runner's `--no-isolate` switch to make a blocked command run.

## Private preparation and image

From the Adage checkout, after reviewing and committing the exact source to run:

```sh
umask 077
export PILOT_NAME=adage-paid-pilot-001
export PILOT_IMAGE=adage-paid-pilot:001
export SETUP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/adage-pilot-setup.XXXXXX")"
git status --short
git rev-parse HEAD > "$SETUP_ROOT/source-commit.txt"
git bundle create "$SETUP_ROOT/adage.bundle" HEAD
cp evals/live/docker/Dockerfile "$SETUP_ROOT/Dockerfile"
cp evals/live/docker/prepare_seccomp.py "$SETUP_ROOT/prepare_seccomp.py"

docker build --platform linux/arm64 --tag "$PILOT_IMAGE" evals/live/docker
docker image inspect "$PILOT_IMAGE" > "$SETUP_ROOT/image-inspect.json"
docker image save --output "$SETUP_ROOT/pilot-image.tar" "$PILOT_IMAGE"
```

The Git bundle contains committed source only; local changes remain in the host
checkout. Save this directory's absolute path. It contains the image archive,
source bundle and isolation configuration, and should be retained outside Git.
Use a new name and directory for a new pilot; do not reuse another pilot's state.
The image archive can be several gigabytes. Apt packages are resolved at build
time, so rebuilding the Dockerfile alone does not reproduce every dependency:
preserve the image archive and record the installed package versions below.

Docker's default seccomp policy blocks namespace operations needed by Bubblewrap.
Generate a profile from the pinned upstream [Moby default profile](https://github.com/moby/profiles/blob/2ceae35d351c156cb5a8efc0fdc4a08cf94569d8/seccomp/default.json):

```sh
curl --fail --location \
  https://raw.githubusercontent.com/moby/profiles/2ceae35d351c156cb5a8efc0fdc4a08cf94569d8/seccomp/default.json \
  --output "$SETUP_ROOT/seccomp-default.json"
python3 evals/live/docker/prepare_seccomp.py \
  "$SETUP_ROOT/seccomp-default.json" "$SETUP_ROOT/seccomp-bwrap.json"
```

The script checks the upstream bytes' SHA-256, retains the default deny policy
and existing rules, and adds `SCMP_ACT_ALLOW` for `clone`, `unshare`, `mount`,
`umount2` and `pivot_root`. Docker's `systempaths=unconfined` option is also
required for Bubblewrap to construct `/proc` in its nested namespace. These
exceptions permit nested isolation within Docker's Linux VM; they do not grant
host mounts or host credentials. Do not broaden them when a probe fails.

## Create the execution host

Only two named volumes are attached. The Docker socket, host home directory,
macOS credentials and host checkout must not be mounted into this container.

```sh
docker volume create "$PILOT_NAME-work"
docker volume create "$PILOT_NAME-home"
docker run --detach --name "$PILOT_NAME" --platform linux/arm64 \
  --security-opt "seccomp=$SETUP_ROOT/seccomp-bwrap.json" \
  --security-opt systempaths=unconfined \
  --mount "type=volume,source=$PILOT_NAME-work,target=/workspace" \
  --mount "type=volume,source=$PILOT_NAME-home,target=/home/pilot" \
  "$PILOT_IMAGE"
docker cp "$SETUP_ROOT/adage.bundle" "$PILOT_NAME:/workspace/source.bundle"
```

Use this entry command for every session, including agent preparation. Docker
otherwise drops host environment variables, which would conceal an agent
session from the coordinator. `--env NAME` forwards each variable from the
invoking process without printing its value.

```sh
docker exec --interactive --tty \
  --env CLAUDECODE --env OPENCODE \
  --env CODEX_SANDBOX --env CODEX_SANDBOX_NETWORK_DISABLED \
  "$PILOT_NAME" /bin/bash
```

The following commands run **inside** that container. Clone and build once:

```sh
umask 077
git clone /workspace/source.bundle /workspace/adage
cd /workspace/adage
git rev-parse HEAD
cargo build --release --locked -p adage-cli
cargo build --locked -p adage-cli
cargo test --locked -p adage-cli --test learning_delivery -- --nocapture
cargo test --locked -p adage-eval --lib --test paired --test experiment
python3 -m unittest discover -s evals/live -p 'test_*.py'
python3 evals/live/sandboxes/pilot/selfcheck.py
python3 evals/live/sandboxes/cardinal/selfcheck.py
bwrap --die-with-parent --unshare-pid --ro-bind / / --proc /proc --dev /dev -- /bin/true
mkdir -m 700 /workspace/preparation
cp /workspace/source.bundle /workspace/preparation/source.bundle
git rev-parse HEAD > /workspace/preparation/source-commit.txt
dpkg-query -W > /workspace/preparation/debian-packages.txt
npm list --global --depth=0 > /workspace/preparation/npm-packages.txt
claude --version > /workspace/preparation/claude-version.txt
sha256sum target/release/adage > /workspace/preparation/adage-sha256.txt
cp target/release/adage /workspace/preparation/adage
```

Preserve test output alongside these records. The coordinator also performs its
full confinement probe and rejects visible private inputs. A successful minimal
Bubblewrap probe alone does not establish private-grader isolation. Leave this
checkout, installed tools and binaries unchanged through collection.

## Authentication, initialization and plan

From a normal operator terminal, enter the container with the command above and
log in interactively:

```sh
claude auth login
cd /workspace/adage
export PILOT_ROOT=/workspace/pilot
python3 evals/live/pilot.py init --root "$PILOT_ROOT" \
  --adage /workspace/preparation/adage \
  --model claude-haiku-4-5-20251001 \
  --budget-usd 30 --session-budget-usd 1 \
  --max-turns 30 --timeout-seconds 600
python3 evals/live/pilot.py plan --root "$PILOT_ROOT"
```

Authentication stays in the private home volume. Never paste credentials into
chat, command arguments, source files, logs or Git. If preparation already
initialized `/workspace/pilot`, omit `init` and run `plan`; initialization refuses
to overwrite an existing pilot. Its manifest pins model revision, task splits,
configuration, binary and collector/task/grader source hashes. The source bundle
and saved image preserve the inputs needed to restore those exact bytes.

Review the printed plan before the first paid command. The fixed admission
allocation is **$30 total**: 20 untreated training sessions ($20), four validation
arm runs ($4), and six test arm runs ($6), at $1 per run. Both held-out arms use
the pinned model above. Each run has at most 30 turns and 600 seconds. This is the
packaged exploratory synthetic pilot; the 20–30% savings objective is unmeasured.
Do not increase any allocation without authorization.

A configured login and installed CLI cannot prove credential validity or model
access. There is no extra paid smoke test outside this allocation: the first
scheduled training attempt must be retained even if authentication/model access
fails. Inspect `ready_for_paid_dispatch` and all blockers, not just command exit status.
If the coordinator refuses an agent session, stop and give its exact command to
the operator. The refusal is an intentional product boundary.

## Operator stage boundaries

Only after the plan is reviewed and its blockers are resolved, the operator runs:

```sh
cd /workspace/adage
export PILOT_ROOT=/workspace/pilot
python3 evals/live/pilot.py train --root "$PILOT_ROOT" --execute
python3 evals/live/pilot.py report --root "$PILOT_ROOT"
```

Reissuing the training command resumes only untouched jobs. Keep native histories,
independent grades, reservations, failures and interrupted attempts. Unknown
accounting, started attempts without receipts, changed source or grader failures
block expansion; reconcile original evidence without deleting or replacing runs.
CLI cost estimates control admission; actual billed cost stays unknown without
provider billing evidence, and settlement can exceed a CLI cap.

After complete training, offline learning uses the canonical export/import and
production learner:

```sh
python3 evals/live/pilot.py learn --root "$PILOT_ROOT"
ADAGE_HOME="$PILOT_ROOT/learner-home" /workspace/preparation/adage review
```

Inspect `learning.json`, supporting semantic groups, rejected candidates and
review evidence. Do not lower thresholds, enable demo learning, insert authored
reflexes or relabel related tasks. No useful candidate is a valid outcome. A
person selects specific candidates and uses the exact references printed by the
queue in a normal operator terminal:

```sh
ADAGE_HOME="$PILOT_ROOT/learner-home" /workspace/preparation/adage \
  review approve REFLEX_REFERENCE ROUTE_ID
```

After that review, the operator freezes **both** held-out cohorts before either
runs. Replace the reviewer placeholder with the actual person's name:

```sh
python3 evals/live/pilot.py freeze --root "$PILOT_ROOT" --reviewed-by 'REVIEWER NAME'
python3 evals/live/pilot.py run --root "$PILOT_ROOT" --split validation
python3 evals/live/pilot.py run --root "$PILOT_ROOT" --split validation --execute
python3 evals/live/pilot.py report --root "$PILOT_ROOT"
```

Pause after validation. Inspect independent correctness (especially baseline-pass /
treatment-fail pairs), actual advice delivery, unnecessary interventions,
verification omissions and complete usage/accounting. An observed verification
command or exit zero does not prove adequate coverage. Do not tune the frozen
treatment or replace unfavorable runs. After this inspection, explicitly release
the untouched test cohort:

```sh
python3 evals/live/pilot.py run --root "$PILOT_ROOT" --split test --execute
python3 evals/live/pilot.py report --root "$PILOT_ROOT"
```

Preserve `/workspace/pilot`, `/workspace/preparation`, both named volumes and the
host preparation directory. Do not run `docker volume rm` or prune these volumes.
The [pilot artifact table](paid-pilot.md) describes the retained reports, native
histories, canonical corpus, review records, frozen protocols and receipts.
Report training cost separately from evaluation cost, input/output/cache tokens,
model calls, candidate support, enabled routes, independent correctness,
delivery/omissions, failures and unknowns. Cached input is part of input tokens.
Keep conclusions exploratory and include exact paths and commands; a completed
small pilot does not establish customer savings.
