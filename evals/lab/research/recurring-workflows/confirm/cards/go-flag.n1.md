# How changes of this kind were done here before

Learned from 1 earlier session(s) of the same kind of task in this repository (0 passed the project's checks). Use it as a checklist; the specifics of your task differ.

## Files such a change touched (in order)

- `flag_url.go` (1/1 sessions)
- `flag_url_test.go` (1/1 sessions)
- `docs/v3/examples/flags/basics.md` (1/1 sessions)

## What the edit looked like last time (another variant; copy the format, not the content)

`flag_url.go`:
```
package cli

import (
	"fmt"
	"net/url"
)

// URLFlag is a flag that accepts a URL with a scheme and host.
type URLFlag = FlagBase[*url.URL, NoConfig, urlValue]

type urlValue struct {
	destination **url.URL
}

…
```
`flag_url_test.go`:
```
package cli

import (
	"io"
	"net/url"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func Test_URLFlag(t *testing.T) {
	tests := []struct {
		name          string
…
```
`docs/v3/examples/flags/basics.md`:
```
- `URLFlag`
```

## Commands

- `find . -type f -name *.go` (ran in 1/1 sessions, 1/1 succeeded)
- `ls -la` (ran in 1/1 sessions, 1/1 succeeded)
- `find . -type f -name *.md` (ran in 1/1 sessions, 1/1 succeeded)
- `find docs -type f -name *.md` (ran in 1/1 sessions, 1/1 succeeded)
- `find . -name *flag*test.go` (ran in 1/1 sessions, 1/1 succeeded)
- `grep -r api.*doc . --include=*.md` (ran in 1/1 sessions, 1/1 succeeded)
- avoid `go run /tmp/test_example.go --url example.com`: failed 1/1 times (`Incorrect Usage: invalid value "example.com" for flag -url: URL must have a scheme`)
- avoid `go run /tmp/test_example.go --url ://x`: failed 1/1 times (`Incorrect Usage: invalid value "://x" for flag -url: parse "://x": missing protocol scheme`)
- avoid `go run /tmp/test_example.go --url file:///path`: failed 1/1 times (`Incorrect Usage: invalid value "file:///path" for flag -url: URL must have a host`)
- avoid `go run /tmp/final_test.go`: failed 1/1 times (`go: cannot run *_test.go files (/tmp/final_test.go)`)
