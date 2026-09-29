Repository notes learned from earlier sessions in this repo:
- `Grep: Hidden type=go head_limit=30` shows: flag_impl.go, help.go
- `Grep: Version.*Flag|version.*flag type=go head_limit=20` shows: command.go
- `Bash: find . -type f -name "*.go" | head -20` shows: completion.go, completion_test.go, flag.go, help_test.go, suggestions_test.go
- `Bash: find . -type f -name "*.go" | head -20` shows: args.go, command_setup.go, flag.go, flag_bool.go, flag_bool_with_inverse.go, flag_duration.go, flag_ext.go, flag_int_slice.go, flag_string_map.go, flag_string_slice.go, value_source.go
- `Bash: find . -type f -name "*.go" | grep -E "(command|app)" | head -20` shows: command.go, command_parse.go, command_run.go, command_test.go
- `Bash: cd /tmp && go run test_empty.go` shows: go.mod
- `Read: flag_bool_with_inverse_test.go offset=[565, 569]` failed here; what worked: `Read: flag_bool_with_inverse_test.go offset=545 limit=30`
- `Bash: grep -l "CommandHelpTemplate\|SubcommandHelpTemplate" *.go` shows: template.go
- `Grep: mutually|MutuallyExclusive type=go output_mode=files_with_matches` shows: errors.go, flag_mutex.go, flag_mutex_test.go
- `Bash: find . -type f -name "*.go" | grep -E "(flag|generic)" | head -20` shows: flag.go, flag_generic.go, flag_schema_type_test.go
- `Bash: go run -exec echo 'package main
import (
	"bytes"
	"context"
	"fmt"
	"github.com/urfave/cli/v3"
)
func main() {
	out := &bytes.Buffer{` failed here; what worked: `Write: /tmp/test_completion_order.go content=package main

import (
	"bytes"
	"context"
	"fmt"
	"github.com/urfave/cli/v3"
)

func main() {
	out := &bytes.Buffe`
- `Edit: replace_all=False command_version_test.go old_string=package cli

import (
	"context"
	"testing"
)

func TestVersionFlagWithVerboseCla` failed here; what worked: `Read: command_version_test.go ; Edit: replace_all=False command_version_test.go old_string=package cli

import (
	"context"
	"testing"
)

func TestVersionFlagWithVerboseClash(t *testing.T) {
	`
