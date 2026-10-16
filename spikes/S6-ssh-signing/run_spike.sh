#!/usr/bin/env bash
# S6 - SSH signing of tracks. Reproduces every number in FINDINGS.md.
#
# Safety:
#  - Never touches ~/.ssh or the user's real ssh-agent. Starts a private
#    agent with its own socket under sandbox/, and kills it in an EXIT trap.
#  - Never sets global/system git config. Every git config call below is
#    either `git -C <sandbox repo> config` (repo-local) or wrapped with
#    GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null.
#  - Writes only inside this spike folder.
#
# Usage: ./run_spike.sh   (from this directory, or any directory: it cd's to
# its own location first)

set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
ROOT="$(pwd)"
SANDBOX="$ROOT/sandbox"
RESULTS="$ROOT/results"
KEYS="$ROOT/keys"
BIN="$ROOT/target/release/s6_ssh_signing"

rm -rf "$SANDBOX" "$RESULTS" "$KEYS"
mkdir -p "$SANDBOX" "$RESULTS" "$KEYS"

log() { printf '\n=== %s ===\n' "$*" | tee -a "$RESULTS/run.log"; }
run() { # run NAME -- cmd...   logs stdout+stderr+exit code to results/NAME.txt
  local name="$1"; shift
  printf '$ %s\n' "$*" > "$RESULTS/$name.txt"
  if "$@" >> "$RESULTS/$name.txt" 2>&1; then
    echo "[exit 0]" >> "$RESULTS/$name.txt"
    echo "ok"
  else
    local rc=$?
    echo "[exit $rc]" >> "$RESULTS/$name.txt"
    echo "FAIL(rc=$rc)"
  fi
}

source "$HOME/.cargo/env"

log "0. Build"
( cd "$ROOT" && cargo build --release ) | tee -a "$RESULTS/run.log"
test -x "$BIN" || { echo "build failed, no binary at $BIN"; exit 1; }

# --------------------------------------------------------------------------
log "1. Keys and a private ssh-agent (never touches ~/.ssh or the real agent)"
# --------------------------------------------------------------------------
"$BIN" keygen --out-prefix "$KEYS/dev_ed25519" --comment "adage-spike-dev" | tee -a "$RESULTS/run.log"

AGENT_SOCK="$SANDBOX/agent.sock"
AGENT_OUT=$(ssh-agent -a "$AGENT_SOCK")
eval "$(echo "$AGENT_OUT" | grep -v '^echo ')" > /dev/null   # sets SSH_AGENT_PID in THIS shell only
AGENT_PID="$SSH_AGENT_PID"
echo "$AGENT_OUT" > "$RESULTS/agent_start.txt"
echo "private agent pid=$AGENT_PID sock=$AGENT_SOCK"
cleanup() {
  if [ -n "${AGENT_PID:-}" ]; then
    kill "$AGENT_PID" 2>/dev/null
    echo "killed private ssh-agent pid=$AGENT_PID"
  fi
}
trap cleanup EXIT

SSH_AUTH_SOCK="$AGENT_SOCK" ssh-add "$KEYS/dev_ed25519" > "$RESULTS/agent_add.txt" 2>&1
echo "agent add: $(tail -1 "$RESULTS/agent_add.txt")"
SSH_AUTH_SOCK="$AGENT_SOCK" ssh-add -l > "$RESULTS/agent_list.txt" 2>&1
"$BIN" list-agent --sock "$AGENT_SOCK" > "$RESULTS/agent_list_rust.txt" 2>&1 || true
cat "$RESULTS/agent_list_rust.txt"

# --------------------------------------------------------------------------
log "2. A sample track directory + canonical digest"
# --------------------------------------------------------------------------
TRACK="$SANDBOX/work-items"
mkdir -p "$TRACK/intents" "$TRACK/routes" "$TRACK/scripts"
cat > "$TRACK/track.toml" <<'EOF'
[track]
name = "acme/work-items"
version = "1.3.0"
description = "Context gathering for work-item prompts"
authors = ["steven"]

[gauge]
harness = ["claude-code >=2.1.200"]
models = ["claude-sonnet-5*"]
EOF
cat > "$TRACK/intents/work-item-start.yaml" <<'EOF'
label: work-item-start
phrasings:
  - "let's start on W-1234"
  - "pick up the next work item"
EOF
cat > "$TRACK/routes/work-item-context.yaml" <<'EOF'
id: work-item-context
trigger:
  at: prompt
  intent: work-item-start
steps:
  - id: lookup
    effect: observe
    run: "./scripts/wi-lookup {item} --json"
EOF
cat > "$TRACK/scripts/wi-lookup" <<'EOF'
#!/usr/bin/env bash
echo '{"item": "'"$1"'"}'
EOF
chmod +x "$TRACK/scripts/wi-lookup"
cat > "$TRACK/evidence.json" <<'EOF'
{"sessions": 42, "confirmed": 40}
EOF
cat > "$TRACK/CHANGELOG.md" <<'EOF'
# Changelog

## 1.3.0
- Initial spike fixture.
EOF

"$BIN" digest --track "$TRACK" --out "$RESULTS/track.digest" --manifest-out "$RESULTS/track.manifest" \
  > "$RESULTS/digest_compute.txt" 2>&1
echo "digest: $(xxd -p "$RESULTS/track.digest" | tr -d '\n')"
cat "$RESULTS/track.manifest"

DIGEST_HEX=$(xxd -p "$RESULTS/track.digest" | tr -d '\n')
DIGEST_FILE="$RESULTS/track.digest"
PUBKEY="$KEYS/dev_ed25519.pub"
PRIVKEY="$KEYS/dev_ed25519"
NS="adage-track"

cat > "$SANDBOX/allowed_signers" <<EOF
steven $(cat "$PUBKEY")
EOF

# --------------------------------------------------------------------------
log "3. Four signers over the same canonical digest, namespace=$NS"
# --------------------------------------------------------------------------

# S1: ssh-keygen -Y sign, file key
cp "$DIGEST_FILE" "$SANDBOX/s1.msg"
run S1_sign ssh-keygen -Y sign -f "$PRIVKEY" -n "$NS" "$SANDBOX/s1.msg"
[ -f "$SANDBOX/s1.msg.sig" ] && cp "$SANDBOX/s1.msg.sig" "$RESULTS/S1.sig"

# S2: ssh-keygen -Y sign, via agent (-f points at the PUBLIC key)
cp "$DIGEST_FILE" "$SANDBOX/s2.msg"
SSH_AUTH_SOCK="$AGENT_SOCK" run S2_sign ssh-keygen -Y sign -f "$PUBKEY" -n "$NS" "$SANDBOX/s2.msg"
[ -f "$SANDBOX/s2.msg.sig" ] && cp "$SANDBOX/s2.msg.sig" "$RESULTS/S2.sig"

# S3: Rust ssh-key crate, file key, no subprocess
run S3_sign "$BIN" sign-file --key "$PRIVKEY" --namespace "$NS" --digest "$DIGEST_FILE" --out "$RESULTS/S3.sig"

# S4: Rust through the agent protocol, no subprocess
run S4_sign "$BIN" sign-agent --pubkey "$PUBKEY" --namespace "$NS" --digest "$DIGEST_FILE" --out "$RESULTS/S4.sig" --sock "$AGENT_SOCK"

# --------------------------------------------------------------------------
log "4. Interoperability matrix: 4 signers x 2 verifiers"
# --------------------------------------------------------------------------
MATRIX="$RESULTS/matrix.csv"
echo "signer,verifier,result,detail" > "$MATRIX"

verify_ssh_keygen() { # sigfile -> 0/1
  SSH_AUTH_SOCK="$AGENT_SOCK" ssh-keygen -Y verify -f "$SANDBOX/allowed_signers" -I steven -n "$NS" -s "$1" < "$DIGEST_FILE"
}
verify_rust() { # sigfile -> 0/1
  "$BIN" verify --sig "$1" --namespace "$NS" --digest "$DIGEST_FILE" --allowed-signers "$SANDBOX/allowed_signers" --principal steven
}

for s in S1 S2 S3 S4; do
  sigfile="$RESULTS/$s.sig"
  if [ ! -s "$sigfile" ]; then
    echo "$s,ssh-keygen,SKIP,no signature produced" >> "$MATRIX"
    echo "$s,rust,SKIP,no signature produced" >> "$MATRIX"
    continue
  fi
  if out=$(verify_ssh_keygen "$sigfile" 2>&1); then
    echo "$s,ssh-keygen,PASS,\"$(echo "$out" | tr '\n' ' ' | tr ',' ';')\"" >> "$MATRIX"
  else
    echo "$s,ssh-keygen,FAIL,\"$(echo "$out" | tr '\n' ' ' | tr ',' ';')\"" >> "$MATRIX"
  fi
  echo "$out" > "$RESULTS/verify_sshkeygen_${s}.txt"

  if out=$("$BIN" verify --sig "$sigfile" --namespace "$NS" --digest "$DIGEST_FILE" --allowed-signers "$SANDBOX/allowed_signers" --principal steven 2>&1); then
    echo "$s,rust,PASS,\"$(echo "$out" | tr '\n' ' ' | tr ',' ';')\"" >> "$MATRIX"
  else
    echo "$s,rust,FAIL,\"$(echo "$out" | tr '\n' ' ' | tr ',' ';')\"" >> "$MATRIX"
  fi
  echo "$out" > "$RESULTS/verify_rust_${s}.txt"
done

cat "$MATRIX"

# --------------------------------------------------------------------------
log "5. Namespace enforcement: a git-namespace signature must not verify as adage-track"
# --------------------------------------------------------------------------
# 5a. Synthetic: sign the SAME digest bytes but with namespace=git using S3 (rust file signer)
"$BIN" sign-file --key "$PRIVKEY" --namespace git --digest "$DIGEST_FILE" --out "$RESULTS/S3_git_ns.sig" \
  > "$RESULTS/S3_git_ns_sign.txt" 2>&1

echo "-- Rust verify, correct namespace (git) --" > "$RESULTS/namespace_test.txt"
"$BIN" verify --sig "$RESULTS/S3_git_ns.sig" --namespace git --digest "$DIGEST_FILE" \
  --allowed-signers "$SANDBOX/allowed_signers" --principal steven >> "$RESULTS/namespace_test.txt" 2>&1
echo "exit=$?" >> "$RESULTS/namespace_test.txt"

echo "-- Rust verify, WRONG namespace (adage-track) on a git-namespace signature --" >> "$RESULTS/namespace_test.txt"
"$BIN" verify --sig "$RESULTS/S3_git_ns.sig" --namespace "$NS" --digest "$DIGEST_FILE" \
  --allowed-signers "$SANDBOX/allowed_signers" --principal steven >> "$RESULTS/namespace_test.txt" 2>&1
echo "exit=$?" >> "$RESULTS/namespace_test.txt"

echo "-- ssh-keygen -Y verify, WRONG namespace (adage-track) on a git-namespace signature --" >> "$RESULTS/namespace_test.txt"
ssh-keygen -Y verify -f "$SANDBOX/allowed_signers" -I steven -n "$NS" -s "$RESULTS/S3_git_ns.sig" < "$DIGEST_FILE" >> "$RESULTS/namespace_test.txt" 2>&1
echo "exit=$?" >> "$RESULTS/namespace_test.txt"

# 5b. Real git commit: sign an actual commit with the same key/agent, extract the
# embedded sshsig, and show it is namespace=git and refused as adage-track.
GITREPO="$SANDBOX/gitrepo"
mkdir -p "$GITREPO"
git -C "$GITREPO" init -q -b main
git -C "$GITREPO" config user.name "S6 Spike"
git -C "$GITREPO" config user.email "spike@example.invalid"
git -C "$GITREPO" config gpg.format ssh
git -C "$GITREPO" config user.signingkey "$PUBKEY"
git -C "$GITREPO" config commit.gpgsign true
git -C "$GITREPO" config gpg.ssh.allowedSignersFile "$SANDBOX/allowed_signers"
echo "hello" > "$GITREPO/file.txt"
git -C "$GITREPO" add file.txt
SSH_AUTH_SOCK="$AGENT_SOCK" git -C "$GITREPO" commit -q -m "test commit" > "$RESULTS/git_commit.txt" 2>&1
echo "commit rc=$?" >> "$RESULTS/git_commit.txt"
SSH_AUTH_SOCK="$AGENT_SOCK" git -C "$GITREPO" log --show-signature -1 > "$RESULTS/git_show_signature.txt" 2>&1

# Extract the embedded "gpgsig ..." header out of the raw commit object into a PEM file.
git -C "$GITREPO" cat-file commit HEAD > "$RESULTS/git_commit_raw.txt"
awk '
  /^gpgsig / { inhdr=1; sub(/^gpgsig /, ""); print; next }
  inhdr && /^ / { sub(/^ /, ""); print; next }
  inhdr { inhdr=0 }
' "$RESULTS/git_commit_raw.txt" > "$RESULTS/git_commit.sig"

echo "-- git commit signature namespace field --" >> "$RESULTS/namespace_test.txt"
grep -A2 "SSH SIG" "$RESULTS/git_commit.sig" | head -1 >> "$RESULTS/namespace_test.txt" 2>&1 || true

echo "-- Rust verify of the REAL git commit signature, namespace=adage-track (expect reject) --" >> "$RESULTS/namespace_test.txt"
"$BIN" verify --sig "$RESULTS/git_commit.sig" --namespace "$NS" --digest "$DIGEST_FILE" \
  --allowed-signers "$SANDBOX/allowed_signers" --principal steven >> "$RESULTS/namespace_test.txt" 2>&1
echo "exit=$?" >> "$RESULTS/namespace_test.txt"

echo "-- ssh-keygen -Y verify of the REAL git commit signature, namespace=adage-track (expect reject) --" >> "$RESULTS/namespace_test.txt"
ssh-keygen -Y verify -f "$SANDBOX/allowed_signers" -I steven -n "$NS" -s "$RESULTS/git_commit.sig" < "$DIGEST_FILE" >> "$RESULTS/namespace_test.txt" 2>&1
echo "exit=$?" >> "$RESULTS/namespace_test.txt"

cat "$RESULTS/namespace_test.txt"

# --------------------------------------------------------------------------
log "6. Tamper tests"
# --------------------------------------------------------------------------
TAMPER="$RESULTS/tamper_test.txt"
: > "$TAMPER"

# 6a. Content tamper: flip a byte in a route file, recompute digest, verify old sig against new digest.
TRACK2="$SANDBOX/work-items-tampered-content"
cp -R "$TRACK" "$TRACK2"
sed -i '' 's/work-item-context/work-item-context-EVIL/' "$TRACK2/routes/work-item-context.yaml"
"$BIN" digest --track "$TRACK2" --out "$SANDBOX/tampered.digest" > /dev/null 2>&1
echo "content tamper: original digest=$(xxd -p "$DIGEST_FILE" | tr -d '\n')" >> "$TAMPER"
echo "content tamper: tampered digest=$(xxd -p "$SANDBOX/tampered.digest" | tr -d '\n')" >> "$TAMPER"
if cmp -s "$DIGEST_FILE" "$SANDBOX/tampered.digest"; then
  echo "content tamper: DIGESTS MATCH (BAD)" >> "$TAMPER"
else
  echo "content tamper: digests differ (expected)" >> "$TAMPER"
fi
echo "content tamper: verifying S1 signature (over ORIGINAL digest) against TAMPERED digest:" >> "$TAMPER"
"$BIN" verify --sig "$RESULTS/S1.sig" --namespace "$NS" --digest "$SANDBOX/tampered.digest" \
  --allowed-signers "$SANDBOX/allowed_signers" --principal steven >> "$TAMPER" 2>&1
echo "exit=$?" >> "$TAMPER"

# 6b. Reorder: rebuild the same track by creating files in reverse order; digest must be identical.
TRACK3="$SANDBOX/work-items-reordered"
mkdir -p "$TRACK3/scripts" "$TRACK3/routes" "$TRACK3/intents"
# create in an order opposite to $TRACK's creation order
cp "$TRACK/CHANGELOG.md" "$TRACK3/CHANGELOG.md"
cp "$TRACK/evidence.json" "$TRACK3/evidence.json"
cp "$TRACK/scripts/wi-lookup" "$TRACK3/scripts/wi-lookup"
chmod +x "$TRACK3/scripts/wi-lookup"
cp "$TRACK/routes/work-item-context.yaml" "$TRACK3/routes/work-item-context.yaml"
cp "$TRACK/intents/work-item-start.yaml" "$TRACK3/intents/work-item-start.yaml"
cp "$TRACK/track.toml" "$TRACK3/track.toml"
"$BIN" digest --track "$TRACK3" --out "$SANDBOX/reordered.digest" > /dev/null 2>&1
echo "reorder: original digest=$(xxd -p "$DIGEST_FILE" | tr -d '\n')" >> "$TAMPER"
echo "reorder: rebuilt (reverse creation order) digest=$(xxd -p "$SANDBOX/reordered.digest" | tr -d '\n')" >> "$TAMPER"
if cmp -s "$DIGEST_FILE" "$SANDBOX/reordered.digest"; then
  echo "reorder: digests match (expected -- manifest is sorted by path)" >> "$TAMPER"
else
  echo "reorder: DIGESTS DIFFER (BAD)" >> "$TAMPER"
fi

# 6c. Line endings: CRLF in a non-scripts file must NOT change the digest;
#     CRLF in a scripts/ file (hashed verbatim) MUST change the digest.
TRACK4="$SANDBOX/work-items-crlf-yaml"
cp -R "$TRACK" "$TRACK4"
perl -pi -e 's/\n/\r\n/' "$TRACK4/routes/work-item-context.yaml"
"$BIN" digest --track "$TRACK4" --out "$SANDBOX/crlf_yaml.digest" > /dev/null 2>&1
echo "CRLF in routes/*.yaml: original=$(xxd -p "$DIGEST_FILE" | tr -d '\n')" >> "$TAMPER"
echo "CRLF in routes/*.yaml: after=$(xxd -p "$SANDBOX/crlf_yaml.digest" | tr -d '\n')" >> "$TAMPER"
if cmp -s "$DIGEST_FILE" "$SANDBOX/crlf_yaml.digest"; then
  echo "CRLF in routes/*.yaml: digests match (expected -- normalised)" >> "$TAMPER"
else
  echo "CRLF in routes/*.yaml: DIGESTS DIFFER (BAD, normalisation not working)" >> "$TAMPER"
fi

TRACK5="$SANDBOX/work-items-crlf-script"
cp -R "$TRACK" "$TRACK5"
perl -pi -e 's/\n/\r\n/' "$TRACK5/scripts/wi-lookup"
"$BIN" digest --track "$TRACK5" --out "$SANDBOX/crlf_script.digest" > /dev/null 2>&1
echo "CRLF in scripts/wi-lookup: original=$(xxd -p "$DIGEST_FILE" | tr -d '\n')" >> "$TAMPER"
echo "CRLF in scripts/wi-lookup: after=$(xxd -p "$SANDBOX/crlf_script.digest" | tr -d '\n')" >> "$TAMPER"
if cmp -s "$DIGEST_FILE" "$SANDBOX/crlf_script.digest"; then
  echo "CRLF in scripts/wi-lookup: DIGESTS MATCH (expected DIFFER -- scripts hashed verbatim)" >> "$TAMPER"
else
  echo "CRLF in scripts/wi-lookup: digests differ (expected -- scripts hashed verbatim)" >> "$TAMPER"
fi

cat "$TAMPER"

# --------------------------------------------------------------------------
log "7. valid-after / valid-before enforcement"
# --------------------------------------------------------------------------
VALID="$RESULTS/valid_time_test.txt"
: > "$VALID"

cat > "$SANDBOX/allowed_signers_expired" <<EOF
steven valid-after="20200101",valid-before="20200102" $(cat "$PUBKEY")
EOF
cat > "$SANDBOX/allowed_signers_future" <<EOF
steven valid-after="20990101" $(cat "$PUBKEY")
EOF
cat > "$SANDBOX/allowed_signers_open" <<EOF
steven valid-after="20200101" $(cat "$PUBKEY")
EOF

echo "-- expired window (valid-before=2020-01-02), rust verify, default now --" >> "$VALID"
"$BIN" verify --sig "$RESULTS/S3.sig" --namespace "$NS" --digest "$DIGEST_FILE" \
  --allowed-signers "$SANDBOX/allowed_signers_expired" --principal steven >> "$VALID" 2>&1
echo "exit=$?" >> "$VALID"

echo "-- expired window, ssh-keygen -Y verify, default now --" >> "$VALID"
ssh-keygen -Y verify -f "$SANDBOX/allowed_signers_expired" -I steven -n "$NS" -s "$RESULTS/S1.sig" < "$DIGEST_FILE" >> "$VALID" 2>&1
echo "exit=$?" >> "$VALID"

echo "-- future window (valid-after=2099-01-01), rust verify, default now --" >> "$VALID"
"$BIN" verify --sig "$RESULTS/S3.sig" --namespace "$NS" --digest "$DIGEST_FILE" \
  --allowed-signers "$SANDBOX/allowed_signers_future" --principal steven >> "$VALID" 2>&1
echo "exit=$?" >> "$VALID"

echo "-- future window, ssh-keygen -Y verify, default now --" >> "$VALID"
ssh-keygen -Y verify -f "$SANDBOX/allowed_signers_future" -I steven -n "$NS" -s "$RESULTS/S1.sig" < "$DIGEST_FILE" >> "$VALID" 2>&1
echo "exit=$?" >> "$VALID"

echo "-- open window (valid-after=2020-01-01, no valid-before), rust verify --" >> "$VALID"
"$BIN" verify --sig "$RESULTS/S3.sig" --namespace "$NS" --digest "$DIGEST_FILE" \
  --allowed-signers "$SANDBOX/allowed_signers_open" --principal steven >> "$VALID" 2>&1
echo "exit=$?" >> "$VALID"

echo "-- ssh-keygen -Y verify -Overify-time= inside the expired window (pin clock to 2020-01-01 12:00) --" >> "$VALID"
ssh-keygen -Y verify -f "$SANDBOX/allowed_signers_expired" -I steven -n "$NS" -Overify-time=20200101120000Z -s "$RESULTS/S1.sig" < "$DIGEST_FILE" >> "$VALID" 2>&1
echo "exit=$?" >> "$VALID"

echo "-- rust verify --verify-time pinned inside the expired window --" >> "$VALID"
"$BIN" verify --sig "$RESULTS/S3.sig" --namespace "$NS" --digest "$DIGEST_FILE" \
  --allowed-signers "$SANDBOX/allowed_signers_expired" --principal steven --verify-time 20200101120000Z >> "$VALID" 2>&1
echo "exit=$?" >> "$VALID"

cat "$VALID"

# --------------------------------------------------------------------------
log "8. git config plumbing (gpg.format=ssh, user.signingkey, gpg.ssh.allowedSignersFile)"
# --------------------------------------------------------------------------
GITCFG="$RESULTS/git_config_plumbing.txt"
: > "$GITCFG"
echo "repo-local config only; nothing global was touched. Values as Adage would read them:" >> "$GITCFG"
for key in gpg.format user.signingkey commit.gpgsign gpg.ssh.allowedSignersFile; do
  val=$(git -C "$GITREPO" config --get "$key" 2>&1)
  echo "$key = $val" >> "$GITCFG"
done
echo "" >> "$GITCFG"
echo "proof global config is untouched (should be empty/absent):" >> "$GITCFG"
git config --global --get gpg.format >> "$GITCFG" 2>&1 || echo "(not set globally, good)" >> "$GITCFG"
cat "$GITCFG"

log "Done. See results/ for raw output and results/matrix.csv for the interoperability matrix."
