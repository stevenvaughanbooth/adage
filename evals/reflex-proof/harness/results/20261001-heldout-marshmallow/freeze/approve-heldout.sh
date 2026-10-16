#!/bin/sh
set -eu
printf '%s\n' 'Held-out study: enable E2 only in 25 new temporary BEST homes and approve one learned pytest verification.'
printf '%s\n' 'NOEXEC and A0 homes get no grant and no policy. Sandbox required; automatic approval off; declared caches only.'
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/00-BEST-failed/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/00-BEST-failed/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/00-BEST-failed/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/00-BEST-failed/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/00-BEST-failed/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/00-BEST-failed/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/00-BEST-failed/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/00-BEST-failed/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/01-BEST-later-edit/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/01-BEST-later-edit/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/01-BEST-later-edit/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/01-BEST-later-edit/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/01-BEST-later-edit/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/01-BEST-later-edit/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/01-BEST-later-edit/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/01-BEST-later-edit/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/02-BEST-config-change/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/02-BEST-config-change/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/02-BEST-config-change/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/02-BEST-config-change/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/02-BEST-config-change/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/02-BEST-config-change/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/02-BEST-config-change/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/02-BEST-config-change/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/03-BEST-broader/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/03-BEST-broader/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/03-BEST-broader/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/03-BEST-broader/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/03-BEST-broader/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/03-BEST-broader/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/03-BEST-broader/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/03-BEST-broader/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/04-BEST-rerun/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/04-BEST-rerun/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/04-BEST-rerun/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/04-BEST-rerun/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/04-BEST-rerun/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/04-BEST-rerun/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/04-BEST-rerun/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/04-BEST-rerun/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/05-BEST-b01-has-keys/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/05-BEST-b01-has-keys/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/05-BEST-b01-has-keys/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/05-BEST-b01-has-keys/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/05-BEST-b01-has-keys/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/05-BEST-b01-has-keys/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/05-BEST-b01-has-keys/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/05-BEST-b01-has-keys/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/09-BEST-b02-iso-date/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/09-BEST-b02-iso-date/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/09-BEST-b02-iso-date/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/09-BEST-b02-iso-date/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/09-BEST-b02-iso-date/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/09-BEST-b02-iso-date/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/09-BEST-b02-iso-date/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/09-BEST-b02-iso-date/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/12-BEST-b03-json-object/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/12-BEST-b03-json-object/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/12-BEST-b03-json-object/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/12-BEST-b03-json-object/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/12-BEST-b03-json-object/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/12-BEST-b03-json-object/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/12-BEST-b03-json-object/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/12-BEST-b03-json-object/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/15-BEST-b04-lat-long/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/15-BEST-b04-lat-long/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/15-BEST-b04-lat-long/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/15-BEST-b04-lat-long/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/15-BEST-b04-lat-long/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/15-BEST-b04-lat-long/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/15-BEST-b04-lat-long/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/15-BEST-b04-lat-long/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/19-BEST-b05-has-keys/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/19-BEST-b05-has-keys/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/19-BEST-b05-has-keys/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/19-BEST-b05-has-keys/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/19-BEST-b05-has-keys/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/19-BEST-b05-has-keys/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/19-BEST-b05-has-keys/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/19-BEST-b05-has-keys/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/21-BEST-b06-iso-date/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/21-BEST-b06-iso-date/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/21-BEST-b06-iso-date/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/21-BEST-b06-iso-date/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/21-BEST-b06-iso-date/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/21-BEST-b06-iso-date/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/21-BEST-b06-iso-date/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/21-BEST-b06-iso-date/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/24-BEST-b07-json-object/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/24-BEST-b07-json-object/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/24-BEST-b07-json-object/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/24-BEST-b07-json-object/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/24-BEST-b07-json-object/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/24-BEST-b07-json-object/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/24-BEST-b07-json-object/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/24-BEST-b07-json-object/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/26-BEST-b08-lat-long/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/26-BEST-b08-lat-long/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/26-BEST-b08-lat-long/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/26-BEST-b08-lat-long/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/26-BEST-b08-lat-long/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/26-BEST-b08-lat-long/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/26-BEST-b08-lat-long/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/26-BEST-b08-lat-long/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/29-BEST-b09-has-keys/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/29-BEST-b09-has-keys/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/29-BEST-b09-has-keys/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/29-BEST-b09-has-keys/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/29-BEST-b09-has-keys/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/29-BEST-b09-has-keys/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/29-BEST-b09-has-keys/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/29-BEST-b09-has-keys/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/33-BEST-b10-iso-date/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/33-BEST-b10-iso-date/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/33-BEST-b10-iso-date/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/33-BEST-b10-iso-date/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/33-BEST-b10-iso-date/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/33-BEST-b10-iso-date/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/33-BEST-b10-iso-date/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/33-BEST-b10-iso-date/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/35-BEST-b11-json-object/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/35-BEST-b11-json-object/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/35-BEST-b11-json-object/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/35-BEST-b11-json-object/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/35-BEST-b11-json-object/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/35-BEST-b11-json-object/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/35-BEST-b11-json-object/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/35-BEST-b11-json-object/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/39-BEST-b12-lat-long/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/39-BEST-b12-lat-long/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/39-BEST-b12-lat-long/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/39-BEST-b12-lat-long/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/39-BEST-b12-lat-long/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/39-BEST-b12-lat-long/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/39-BEST-b12-lat-long/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/39-BEST-b12-lat-long/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/43-BEST-b13-has-keys/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/43-BEST-b13-has-keys/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/43-BEST-b13-has-keys/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/43-BEST-b13-has-keys/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/43-BEST-b13-has-keys/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/43-BEST-b13-has-keys/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/43-BEST-b13-has-keys/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/43-BEST-b13-has-keys/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/46-BEST-b14-iso-date/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/46-BEST-b14-iso-date/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/46-BEST-b14-iso-date/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/46-BEST-b14-iso-date/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/46-BEST-b14-iso-date/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/46-BEST-b14-iso-date/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/46-BEST-b14-iso-date/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/46-BEST-b14-iso-date/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/48-BEST-b15-json-object/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/48-BEST-b15-json-object/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/48-BEST-b15-json-object/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/48-BEST-b15-json-object/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/48-BEST-b15-json-object/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/48-BEST-b15-json-object/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/48-BEST-b15-json-object/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/48-BEST-b15-json-object/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/50-BEST-b16-lat-long/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/50-BEST-b16-lat-long/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/50-BEST-b16-lat-long/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/50-BEST-b16-lat-long/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/50-BEST-b16-lat-long/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/50-BEST-b16-lat-long/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/50-BEST-b16-lat-long/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/50-BEST-b16-lat-long/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/53-BEST-b17-has-keys/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/53-BEST-b17-has-keys/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/53-BEST-b17-has-keys/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/53-BEST-b17-has-keys/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/53-BEST-b17-has-keys/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/53-BEST-b17-has-keys/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/53-BEST-b17-has-keys/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/53-BEST-b17-has-keys/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/57-BEST-b18-iso-date/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/57-BEST-b18-iso-date/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/57-BEST-b18-iso-date/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/57-BEST-b18-iso-date/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/57-BEST-b18-iso-date/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/57-BEST-b18-iso-date/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/57-BEST-b18-iso-date/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/57-BEST-b18-iso-date/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/61-BEST-b19-json-object/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/61-BEST-b19-json-object/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/61-BEST-b19-json-object/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/61-BEST-b19-json-object/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/61-BEST-b19-json-object/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/61-BEST-b19-json-object/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/61-BEST-b19-json-object/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/61-BEST-b19-json-object/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/63-BEST-b20-lat-long/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/63-BEST-b20-lat-long/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/63-BEST-b20-lat-long/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/63-BEST-b20-lat-long/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/63-BEST-b20-lat-long/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/63-BEST-b20-lat-long/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/63-BEST-b20-lat-long/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/bin/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v8/heldout-1/sessions/63-BEST-b20-lat-long/repo --by launch-evaluation-human
