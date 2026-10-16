#!/bin/sh
set -eu
printf '%s\n' 'Launch canary: enable E2 only in 12 new temporary homes and approve one learned pytest verification.'
printf '%s\n' 'Sandbox required; automatic approval off; declared pytest/bytecode caches only; no global changes.'
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/00-positive-1/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/00-positive-1/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/00-positive-1/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/00-positive-1/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/00-positive-1/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/00-positive-1/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/00-positive-1/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/00-positive-1/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/01-failed-1/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/01-failed-1/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/01-failed-1/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/01-failed-1/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/01-failed-1/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/01-failed-1/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/01-failed-1/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/01-failed-1/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/02-positive-2/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/02-positive-2/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/02-positive-2/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/02-positive-2/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/02-positive-2/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/02-positive-2/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/02-positive-2/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/02-positive-2/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/03-later-edit-1/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/03-later-edit-1/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/03-later-edit-1/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/03-later-edit-1/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/03-later-edit-1/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/03-later-edit-1/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/03-later-edit-1/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/03-later-edit-1/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/04-positive-3/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/04-positive-3/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/04-positive-3/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/04-positive-3/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/04-positive-3/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/04-positive-3/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/04-positive-3/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/04-positive-3/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/05-config-change-1/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/05-config-change-1/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/05-config-change-1/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/05-config-change-1/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/05-config-change-1/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/05-config-change-1/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/05-config-change-1/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/05-config-change-1/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/06-positive-4/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/06-positive-4/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/06-positive-4/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/06-positive-4/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/06-positive-4/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/06-positive-4/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/06-positive-4/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/06-positive-4/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/07-broader-1/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/07-broader-1/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/07-broader-1/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/07-broader-1/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/07-broader-1/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/07-broader-1/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/07-broader-1/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/07-broader-1/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/08-positive-5/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/08-positive-5/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/08-positive-5/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/08-positive-5/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/08-positive-5/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/08-positive-5/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/08-positive-5/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/08-positive-5/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/09-rerun-1/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/09-rerun-1/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/09-rerun-1/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/09-rerun-1/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/09-rerun-1/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/09-rerun-1/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/09-rerun-1/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/09-rerun-1/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/10-positive-6/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/10-positive-6/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/10-positive-6/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/10-positive-6/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/10-positive-6/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/10-positive-6/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/10-positive-6/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/10-positive-6/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/11-later-edit-2/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/11-later-edit-2/adage-home/index.json || { echo 'Prepare the canary first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/11-later-edit-2/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/11-later-edit-2/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/11-later-edit-2/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/11-later-edit-2/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/11-later-edit-2/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/canary-v6/sessions/11-later-edit-2/repo --by launch-evaluation-human
