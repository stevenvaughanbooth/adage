#!/bin/bash
# ev.sh <binary> <repo> [extra eval args...]: adage eval on one RepoBench dataset, isolated home.
D=${LD:-/tmp/adage-learner}
bin=$1; repo=$2; shift 2
h=$(mktemp -d "$D/home.XXXX")
export ADAGE_HOME=$h HOME=$h
unset CLAUDECODE
"$bin" eval "$D/ds/rb-$repo" "$@"
rc=$?
rm -rf "$h"
exit $rc
