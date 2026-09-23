#!/usr/bin/env bash
# Reproduces the Q2 measurement: does CLAUDE_PLUGIN_ROOT (installPath) change
# across a plugin version bump when installed from a *local-directory*
# marketplace with a relative-path source? Uses only `claude plugin ...`
# subcommands, which need no model auth, so this works even when `claude -p`
# cannot authenticate.
#
# Entirely contained: CLAUDE_CONFIG_DIR points inside this spike folder, so
# nothing touches the real ~/.claude marketplaces or plugins.
#
# Usage: ./scripts/version-bump-test.sh | tee results/version-bump.txt
set -euo pipefail
SPIKE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CLAUDE_CONFIG_DIR="$SPIKE/cfg"
cd "$SPIKE"

rm -rf "$SPIKE/marketplace/probe-mp/adage-probe-src"
cp -R "$SPIKE/plugins/adage-probe" "$SPIKE/marketplace/probe-mp/adage-probe-src"
python3 -c "
import json
p = '$SPIKE/marketplace/probe-mp/adage-probe-src/.claude-plugin/plugin.json'
d = json.load(open(p)); d['version'] = '1.0.0'
json.dump(d, open(p,'w'), indent=2)
"

echo "== add local-directory marketplace =="
claude plugin marketplace add ./marketplace/probe-mp

echo "== install adage-probe@probe-mp (version 1.0.0) =="
claude plugin install adage-probe@probe-mp --scope user

echo "== list --json (before bump) =="
claude plugin list --json

echo "== bump version to 2.0.0 in the marketplace source =="
python3 -c "
import json
p = '$SPIKE/marketplace/probe-mp/adage-probe-src/.claude-plugin/plugin.json'
d = json.load(open(p)); d['version'] = '2.0.0'
json.dump(d, open(p,'w'), indent=2)
"

echo "== claude plugin marketplace update probe-mp =="
claude plugin marketplace update probe-mp

echo "== claude plugin update adage-probe@probe-mp =="
claude plugin update adage-probe@probe-mp

echo "== list --json (after bump) =="
claude plugin list --json

echo "== cache directory contents (both versions kept?) =="
find "$SPIKE/cfg/plugins/cache/probe-mp/adage-probe" -maxdepth 1

echo "== plugin details =="
claude plugin details adage-probe@probe-mp

echo "== cleanup =="
claude plugin uninstall adage-probe@probe-mp --scope user || true
claude plugin marketplace remove probe-mp || true
