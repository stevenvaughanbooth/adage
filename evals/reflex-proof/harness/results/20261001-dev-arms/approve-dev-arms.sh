#!/bin/sh
set -eu
printf '%s\n' 'Development arms: enable E2 only in 28 new temporary homes and approve one learned pytest verification.'
printf '%s\n' 'Sandbox required; automatic approval off; declared pytest/bytecode caches only; no global changes.'
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/01-A1-positive-b1/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/01-A1-positive-b1/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/01-A1-positive-b1/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/01-A1-positive-b1/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/01-A1-positive-b1/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/01-A1-positive-b1/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/01-A1-positive-b1/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/rev7/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/01-A1-positive-b1/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/02-A2-positive-b1/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/02-A2-positive-b1/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/02-A2-positive-b1/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/02-A2-positive-b1/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/02-A2-positive-b1/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/02-A2-positive-b1/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/02-A2-positive-b1/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/02-A2-positive-b1/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/03-A3-positive-b1/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/03-A3-positive-b1/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/03-A3-positive-b1/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/03-A3-positive-b1/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/03-A3-positive-b1/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/03-A3-positive-b1/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/03-A3-positive-b1/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/03-A3-positive-b1/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/04-A1-positive-b2/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/04-A1-positive-b2/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/04-A1-positive-b2/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/04-A1-positive-b2/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/04-A1-positive-b2/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/04-A1-positive-b2/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/04-A1-positive-b2/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/rev7/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/04-A1-positive-b2/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/05-A2-positive-b2/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/05-A2-positive-b2/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/05-A2-positive-b2/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/05-A2-positive-b2/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/05-A2-positive-b2/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/05-A2-positive-b2/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/05-A2-positive-b2/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/05-A2-positive-b2/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/06-A3-positive-b2/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/06-A3-positive-b2/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/06-A3-positive-b2/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/06-A3-positive-b2/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/06-A3-positive-b2/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/06-A3-positive-b2/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/06-A3-positive-b2/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/06-A3-positive-b2/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/08-A2-positive-b3/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/08-A2-positive-b3/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/08-A2-positive-b3/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/08-A2-positive-b3/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/08-A2-positive-b3/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/08-A2-positive-b3/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/08-A2-positive-b3/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/08-A2-positive-b3/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/09-A3-positive-b3/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/09-A3-positive-b3/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/09-A3-positive-b3/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/09-A3-positive-b3/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/09-A3-positive-b3/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/09-A3-positive-b3/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/09-A3-positive-b3/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/09-A3-positive-b3/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/11-A1-positive-b3/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/11-A1-positive-b3/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/11-A1-positive-b3/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/11-A1-positive-b3/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/11-A1-positive-b3/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/11-A1-positive-b3/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/11-A1-positive-b3/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/rev7/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/11-A1-positive-b3/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/12-A3-positive-b4/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/12-A3-positive-b4/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/12-A3-positive-b4/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/12-A3-positive-b4/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/12-A3-positive-b4/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/12-A3-positive-b4/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/12-A3-positive-b4/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/12-A3-positive-b4/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/14-A1-positive-b4/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/14-A1-positive-b4/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/14-A1-positive-b4/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/14-A1-positive-b4/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/14-A1-positive-b4/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/14-A1-positive-b4/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/14-A1-positive-b4/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/rev7/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/14-A1-positive-b4/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/15-A2-positive-b4/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/15-A2-positive-b4/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/15-A2-positive-b4/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/15-A2-positive-b4/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/15-A2-positive-b4/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/15-A2-positive-b4/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/15-A2-positive-b4/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/15-A2-positive-b4/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/17-A1-positive-b5/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/17-A1-positive-b5/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/17-A1-positive-b5/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/17-A1-positive-b5/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/17-A1-positive-b5/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/17-A1-positive-b5/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/17-A1-positive-b5/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/rev7/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/17-A1-positive-b5/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/18-A2-positive-b5/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/18-A2-positive-b5/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/18-A2-positive-b5/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/18-A2-positive-b5/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/18-A2-positive-b5/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/18-A2-positive-b5/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/18-A2-positive-b5/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/18-A2-positive-b5/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/19-A3-positive-b5/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/19-A3-positive-b5/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/19-A3-positive-b5/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/19-A3-positive-b5/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/19-A3-positive-b5/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/19-A3-positive-b5/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/19-A3-positive-b5/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/19-A3-positive-b5/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/20-A1-positive-b6/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/20-A1-positive-b6/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/20-A1-positive-b6/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/20-A1-positive-b6/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/20-A1-positive-b6/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/20-A1-positive-b6/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/20-A1-positive-b6/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/rev7/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/20-A1-positive-b6/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/21-A2-positive-b6/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/21-A2-positive-b6/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/21-A2-positive-b6/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/21-A2-positive-b6/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/21-A2-positive-b6/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/21-A2-positive-b6/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/21-A2-positive-b6/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/21-A2-positive-b6/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/22-A3-positive-b6/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/22-A3-positive-b6/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/22-A3-positive-b6/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/22-A3-positive-b6/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/22-A3-positive-b6/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/22-A3-positive-b6/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/22-A3-positive-b6/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/22-A3-positive-b6/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/24-A2-failed/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/24-A2-failed/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/24-A2-failed/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/24-A2-failed/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/24-A2-failed/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/24-A2-failed/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/24-A2-failed/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/24-A2-failed/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/25-A3-failed/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/25-A3-failed/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/25-A3-failed/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/25-A3-failed/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/25-A3-failed/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/25-A3-failed/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/25-A3-failed/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/25-A3-failed/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/26-A2-later-edit/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/26-A2-later-edit/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/26-A2-later-edit/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/26-A2-later-edit/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/26-A2-later-edit/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/26-A2-later-edit/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/26-A2-later-edit/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/26-A2-later-edit/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/27-A3-later-edit/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/27-A3-later-edit/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/27-A3-later-edit/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/27-A3-later-edit/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/27-A3-later-edit/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/27-A3-later-edit/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/27-A3-later-edit/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/27-A3-later-edit/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/28-A2-config-change/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/28-A2-config-change/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/28-A2-config-change/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/28-A2-config-change/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/28-A2-config-change/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/28-A2-config-change/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/28-A2-config-change/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/28-A2-config-change/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/29-A3-config-change/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/29-A3-config-change/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/29-A3-config-change/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/29-A3-config-change/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/29-A3-config-change/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/29-A3-config-change/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/29-A3-config-change/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/29-A3-config-change/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/30-A2-broader/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/30-A2-broader/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/30-A2-broader/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/30-A2-broader/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/30-A2-broader/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/30-A2-broader/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/30-A2-broader/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/30-A2-broader/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/31-A3-broader/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/31-A3-broader/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/31-A3-broader/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/31-A3-broader/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/31-A3-broader/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/31-A3-broader/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/31-A3-broader/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/31-A3-broader/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/32-A2-rerun/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/32-A2-rerun/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/32-A2-rerun/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/32-A2-rerun/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/32-A2-rerun/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/32-A2-rerun/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/32-A2-rerun/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/32-A2-rerun/repo --by launch-evaluation-human
test -d /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/33-A3-rerun/repo && test -f /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/33-A3-rerun/adage-home/index.json || { echo 'Prepare the study first.' >&2; exit 1; }
test ! -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/33-A3-rerun/adage-home/approvals.json || { echo 'Grant already present; stop and review.' >&2; exit 1; }
if [ -e /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/33-A3-rerun/adage-home/policy.toml ]; then printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' | cmp -s - /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/33-A3-rerun/adage-home/policy.toml || { echo 'Existing policy differs; stop.' >&2; exit 1; }; else printf '%s' '[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
' > /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/33-A3-rerun/adage-home/policy.toml; fi
ADAGE_HOME=/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/33-A3-rerun/adage-home /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/bin/current/adage review delegate github-com-marshmallow-code-marshmallow/learned run-175d85 --level e2 --effect verify --cache '.pytest_cache/**' --cache 'tests/__pycache__/**' --cache 'src/**/__pycache__/**' --cache .pytest_cache/.gitignore --timeout-ms 30000 --repo /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b/sessions/33-A3-rerun/repo --by launch-evaluation-human
