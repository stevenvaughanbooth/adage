# How changes of this kind were done here before

Learned from 4 earlier session(s) of the same kind of task in this repository (0 passed the project's checks). Use it as a checklist; the specifics of your task differ.

## Files such a change touched (in order)

- `flag_<x>.go` (4/4 sessions)
- `flag_<x>_test.go` (4/4 sessions)
- `docs/v3/examples/flags/basics.md` (4/4 sessions)

## What the edit looked like last time (another variant; copy the format, not the content)

`flag_<x>.go`:
```
package cli

import (
	"io"
	"net"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func Test_IPFlag(t *testing.T) {
	tests := []struct {
		name          string
…
```
`flag_<x>_test.go`:
```
package cli

import (
	"io"
	"net"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func Test_IPFlag(t *testing.T) {
	tests := []struct {
		name          string
…
```
`docs/v3/examples/flags/basics.md`:
```
- `IPFlag`
```

## Commands

- `find . -type f -name *.go` (ran in 4/4 sessions, 4/4 succeeded)
- `cat` (ran in 4/4 sessions, 11/12 succeeded)
- `find docs -type f -name *.md` (ran in 3/4 sessions, 3/3 succeeded)
- `go test ./... -v` (ran in 3/4 sessions, 8/8 succeeded)
- `go test ./...` (ran in 3/4 sessions, 6/6 succeeded)
- `git diff docs/v3/examples/flags/basics.md` (ran in 3/4 sessions, 3/3 succeeded)
