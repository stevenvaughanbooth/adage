#!/bin/sh
set -eu
printf '%s\n' 'Development canary-v9: enable E2 only in 37 new temporary homes and approve one learned pytest verification.'
printf '%s\n' 'Sandbox required; automatic approval off; declared pytest/bytecode caches only; no global changes.'
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/00-A4-failed/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/00-A4-failed/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/00-A4-failed/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/00-A4-failed/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/00-A4-failed/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/00-A4-failed/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/00-A4-failed/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/00-A4-failed/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/01-A4-later-edit/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/01-A4-later-edit/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/01-A4-later-edit/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/01-A4-later-edit/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/01-A4-later-edit/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/01-A4-later-edit/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/01-A4-later-edit/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/01-A4-later-edit/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/02-A4-config-change/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/02-A4-config-change/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/02-A4-config-change/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/02-A4-config-change/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/02-A4-config-change/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/02-A4-config-change/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/02-A4-config-change/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/02-A4-config-change/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/03-A4-broader/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/03-A4-broader/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/03-A4-broader/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/03-A4-broader/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/03-A4-broader/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/03-A4-broader/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/03-A4-broader/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/03-A4-broader/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/04-A4-rerun/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/04-A4-rerun/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/04-A4-rerun/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/04-A4-rerun/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/04-A4-rerun/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/04-A4-rerun/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/04-A4-rerun/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/04-A4-rerun/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/06-A2f-positive-b01/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/06-A2f-positive-b01/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/06-A2f-positive-b01/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/06-A2f-positive-b01/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/06-A2f-positive-b01/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/06-A2f-positive-b01/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/06-A2f-positive-b01/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/06-A2f-positive-b01/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/07-A4-positive-b01/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/07-A4-positive-b01/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/07-A4-positive-b01/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/07-A4-positive-b01/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/07-A4-positive-b01/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/07-A4-positive-b01/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/07-A4-positive-b01/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/07-A4-positive-b01/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/08-A2f-positive-b02/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/08-A2f-positive-b02/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/08-A2f-positive-b02/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/08-A2f-positive-b02/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/08-A2f-positive-b02/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/08-A2f-positive-b02/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/08-A2f-positive-b02/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/08-A2f-positive-b02/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/09-A4-positive-b02/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/09-A4-positive-b02/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/09-A4-positive-b02/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/09-A4-positive-b02/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/09-A4-positive-b02/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/09-A4-positive-b02/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/09-A4-positive-b02/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/09-A4-positive-b02/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/11-A4-positive-b03/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/11-A4-positive-b03/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/11-A4-positive-b03/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/11-A4-positive-b03/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/11-A4-positive-b03/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/11-A4-positive-b03/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/11-A4-positive-b03/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/11-A4-positive-b03/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/13-A2f-positive-b03/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/13-A2f-positive-b03/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/13-A2f-positive-b03/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/13-A2f-positive-b03/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/13-A2f-positive-b03/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/13-A2f-positive-b03/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/13-A2f-positive-b03/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/13-A2f-positive-b03/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/15-A2f-positive-b04/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/15-A2f-positive-b04/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/15-A2f-positive-b04/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/15-A2f-positive-b04/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/15-A2f-positive-b04/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/15-A2f-positive-b04/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/15-A2f-positive-b04/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/15-A2f-positive-b04/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/16-A4-positive-b04/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/16-A4-positive-b04/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/16-A4-positive-b04/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/16-A4-positive-b04/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/16-A4-positive-b04/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/16-A4-positive-b04/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/16-A4-positive-b04/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/16-A4-positive-b04/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/17-A2f-positive-b05/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/17-A2f-positive-b05/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/17-A2f-positive-b05/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/17-A2f-positive-b05/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/17-A2f-positive-b05/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/17-A2f-positive-b05/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/17-A2f-positive-b05/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/17-A2f-positive-b05/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/18-A4-positive-b05/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/18-A4-positive-b05/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/18-A4-positive-b05/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/18-A4-positive-b05/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/18-A4-positive-b05/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/18-A4-positive-b05/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/18-A4-positive-b05/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/18-A4-positive-b05/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/20-A4-positive-b06/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/20-A4-positive-b06/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/20-A4-positive-b06/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/20-A4-positive-b06/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/20-A4-positive-b06/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/20-A4-positive-b06/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/20-A4-positive-b06/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/20-A4-positive-b06/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/22-A2f-positive-b06/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/22-A2f-positive-b06/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/22-A2f-positive-b06/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/22-A2f-positive-b06/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/22-A2f-positive-b06/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/22-A2f-positive-b06/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/22-A2f-positive-b06/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/22-A2f-positive-b06/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/24-A2f-positive-b07/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/24-A2f-positive-b07/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/24-A2f-positive-b07/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/24-A2f-positive-b07/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/24-A2f-positive-b07/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/24-A2f-positive-b07/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/24-A2f-positive-b07/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/24-A2f-positive-b07/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/25-A4-positive-b07/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/25-A4-positive-b07/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/25-A4-positive-b07/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/25-A4-positive-b07/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/25-A4-positive-b07/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/25-A4-positive-b07/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/25-A4-positive-b07/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/25-A4-positive-b07/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/26-A2f-positive-b08/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/26-A2f-positive-b08/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/26-A2f-positive-b08/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/26-A2f-positive-b08/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/26-A2f-positive-b08/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/26-A2f-positive-b08/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/26-A2f-positive-b08/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/26-A2f-positive-b08/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/27-A4-positive-b08/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/27-A4-positive-b08/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/27-A4-positive-b08/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/27-A4-positive-b08/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/27-A4-positive-b08/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/27-A4-positive-b08/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/27-A4-positive-b08/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/27-A4-positive-b08/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/29-A4-positive-b09/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/29-A4-positive-b09/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/29-A4-positive-b09/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/29-A4-positive-b09/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/29-A4-positive-b09/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/29-A4-positive-b09/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/29-A4-positive-b09/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/29-A4-positive-b09/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/31-A2f-positive-b09/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/31-A2f-positive-b09/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/31-A2f-positive-b09/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/31-A2f-positive-b09/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/31-A2f-positive-b09/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/31-A2f-positive-b09/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/31-A2f-positive-b09/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/31-A2f-positive-b09/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/33-A2f-positive-b10/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/33-A2f-positive-b10/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/33-A2f-positive-b10/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/33-A2f-positive-b10/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/33-A2f-positive-b10/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/33-A2f-positive-b10/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/33-A2f-positive-b10/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/33-A2f-positive-b10/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/34-A4-positive-b10/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/34-A4-positive-b10/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/34-A4-positive-b10/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/34-A4-positive-b10/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/34-A4-positive-b10/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/34-A4-positive-b10/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/34-A4-positive-b10/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/34-A4-positive-b10/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/35-A2f-positive-b11/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/35-A2f-positive-b11/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/35-A2f-positive-b11/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/35-A2f-positive-b11/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/35-A2f-positive-b11/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/35-A2f-positive-b11/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/35-A2f-positive-b11/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/35-A2f-positive-b11/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/36-A4-positive-b11/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/36-A4-positive-b11/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/36-A4-positive-b11/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/36-A4-positive-b11/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/36-A4-positive-b11/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/36-A4-positive-b11/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/36-A4-positive-b11/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/36-A4-positive-b11/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/38-A4-positive-b12/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/38-A4-positive-b12/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/38-A4-positive-b12/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/38-A4-positive-b12/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/38-A4-positive-b12/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/38-A4-positive-b12/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/38-A4-positive-b12/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/38-A4-positive-b12/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/40-A2f-positive-b12/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/40-A2f-positive-b12/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/40-A2f-positive-b12/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/40-A2f-positive-b12/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/40-A2f-positive-b12/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/40-A2f-positive-b12/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/40-A2f-positive-b12/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/40-A2f-positive-b12/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/42-A2f-positive-b13/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/42-A2f-positive-b13/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/42-A2f-positive-b13/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/42-A2f-positive-b13/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/42-A2f-positive-b13/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/42-A2f-positive-b13/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/42-A2f-positive-b13/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/42-A2f-positive-b13/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/43-A4-positive-b13/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/43-A4-positive-b13/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/43-A4-positive-b13/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/43-A4-positive-b13/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/43-A4-positive-b13/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/43-A4-positive-b13/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/43-A4-positive-b13/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/43-A4-positive-b13/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/44-A2f-positive-b14/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/44-A2f-positive-b14/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/44-A2f-positive-b14/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/44-A2f-positive-b14/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/44-A2f-positive-b14/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/44-A2f-positive-b14/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/44-A2f-positive-b14/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/44-A2f-positive-b14/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/45-A4-positive-b14/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/45-A4-positive-b14/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/45-A4-positive-b14/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/45-A4-positive-b14/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/45-A4-positive-b14/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/45-A4-positive-b14/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/45-A4-positive-b14/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/45-A4-positive-b14/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/47-A4-positive-b15/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/47-A4-positive-b15/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/47-A4-positive-b15/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/47-A4-positive-b15/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/47-A4-positive-b15/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/47-A4-positive-b15/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/47-A4-positive-b15/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/47-A4-positive-b15/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/49-A2f-positive-b15/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/49-A2f-positive-b15/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/49-A2f-positive-b15/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/49-A2f-positive-b15/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/49-A2f-positive-b15/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/49-A2f-positive-b15/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/49-A2f-positive-b15/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/49-A2f-positive-b15/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/51-A2f-positive-b16/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/51-A2f-positive-b16/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/51-A2f-positive-b16/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/51-A2f-positive-b16/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/51-A2f-positive-b16/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/51-A2f-positive-b16/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/51-A2f-positive-b16/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/51-A2f-positive-b16/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/52-A4-positive-b16/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/52-A4-positive-b16/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/52-A4-positive-b16/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/52-A4-positive-b16/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/52-A4-positive-b16/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/52-A4-positive-b16/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/52-A4-positive-b16/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 20000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9/sessions/52-A4-positive-b16/repo --by launch-evaluation-human
