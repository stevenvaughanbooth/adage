"""Third recurring-workflow family, on a different ecosystem (Go): urfave/cli v3.

  go-flag  "add a new flag type `cli.<Name>Flag` with a `cmd.<Name>(name)` accessor": a new
           flag_<x>.go (a FlagBase alias, its value type, the accessor), tests, the flag list in
           docs/v3/examples/flags/basics.md, and the generated API docs (godoc-current.txt via
           `go doc -all`, approved into testdata/godoc-v3.x.txt, as the project's
           `make generate` / `v3approve` do)

Order fixed before any session ran: the first 4 are training, the other 6 held out.
"""

from __future__ import annotations

import subprocess
import textwrap
from pathlib import Path

URFAVE_BASE = "58ec406e"  # urfave/cli main, "Merge pull request #2437 ..."

PROMPT = (
    "Add a new flag type to urfave/cli: `cli.{flag}`, {desc} Its value is read with a new accessor "
    "`cmd.{acc}(name string) {gotype}`. A value it cannot parse must make the command fail with an "
    "error. Examples of valid input: {valid_ex}. Examples of invalid input: {invalid_ex}.\n\n"
    "Include tests, list the new flag in the flags documentation, and update the generated API "
    "documentation, as the project does for new flag types."
)

GO = [
    dict(id="url", flag="URLFlag", acc="URL", gotype="*url.URL", imports=["fmt", "net/url"],
         desc="which accepts an absolute URL (it must have a scheme and a host) and parses it into a `*url.URL`.",
         valid=[("https://example.com/x", "https://example.com/x"), ("http://localhost:8080", "http://localhost:8080")],
         invalid=["example.com", "not a url", "://x"],
         parse="""
            u, err := url.Parse(s)
            if err != nil {
                return nil, err
            }
            if u.Scheme == "" || u.Host == "" {
                return nil, fmt.Errorf("invalid URL %q", s)
            }
            return u, nil
         """),
    dict(id="port", flag="PortFlag", acc="Port", gotype="uint16", imports=["fmt", "strconv"],
         desc="which accepts a TCP/UDP port number, an integer from 1 to 65535 inclusive, as a `uint16`.",
         valid=[("80", "80"), ("65535", "65535"), ("1", "1")], invalid=["0", "70000", "http"],
         parse="""
            n, err := strconv.ParseUint(s, 10, 16)
            if err != nil || n == 0 {
                return 0, fmt.Errorf("invalid port %q", s)
            }
            return uint16(n), nil
         """),
    dict(id="regexp", flag="RegexpFlag", acc="Regexp", gotype="*regexp.Regexp", imports=["fmt", "regexp"],
         desc="which accepts a regular expression and compiles it into a `*regexp.Regexp`.",
         valid=[("^a+$", "^a+$"), ("[0-9]{3}", "[0-9]{3}")], invalid=["(", "a["],
         parse="""
            re, err := regexp.Compile(s)
            if err != nil {
                return nil, fmt.Errorf("invalid regexp %q: %w", s, err)
            }
            return re, nil
         """),
    dict(id="ip", flag="IPFlag", acc="IP", gotype="net.IP", imports=["fmt", "net"],
         desc="which accepts an IPv4 or IPv6 address and parses it into a `net.IP`.",
         valid=[("127.0.0.1", "127.0.0.1"), ("::1", "::1")], invalid=["999.1.1.1", "localhost"],
         parse="""
            ip := net.ParseIP(s)
            if ip == nil {
                return nil, fmt.Errorf("invalid IP address %q", s)
            }
            return ip, nil
         """),
    dict(id="percent", flag="PercentFlag", acc="Percent", gotype="float64", imports=["fmt", "strconv", "strings"],
         desc="which accepts a percentage from 0 to 100 inclusive, written with or without a trailing `%` "
              "(for example `50%`, `12.5`), as a `float64` fraction (`50%` is `0.5`).",
         valid=[("50%", "0.5"), ("12.5", "0.125"), ("100%", "1")], invalid=["101%", "-1", "abc"],
         parse="""
            f, err := strconv.ParseFloat(strings.TrimSuffix(s, "%"), 64)
            if err != nil || f < 0 || f > 100 {
                return 0, fmt.Errorf("invalid percentage %q", s)
            }
            return f / 100, nil
         """),
    dict(id="bytesize", flag="ByteSizeFlag", acc="ByteSize", gotype="int64", imports=["fmt", "regexp", "strconv", "strings"],
         desc="which accepts a size in bytes written as a number with an optional unit: `B`, `KB`, `MB`, `GB` "
              "(powers of 1000) or `KiB`, `MiB`, `GiB` (powers of 1024), case-insensitive, as an `int64` number "
              "of bytes (a bare number is bytes; `1.5MB` is `1500000`).",
         valid=[("10KB", "10000"), ("1KiB", "1024"), ("512", "512"), ("1.5MB", "1500000")],
         invalid=["10XB", "-1KB", "KB"],
         parse="""
            m := regexp.MustCompile(`^(\\d+(?:\\.\\d+)?)\\s*([a-zA-Z]*)$`).FindStringSubmatch(s)
            units := map[string]float64{"": 1, "b": 1, "kb": 1e3, "mb": 1e6, "gb": 1e9, "kib": 1 << 10, "mib": 1 << 20, "gib": 1 << 30}
            if m == nil {
                return 0, fmt.Errorf("invalid size %q", s)
            }
            mult, ok := units[strings.ToLower(m[2])]
            if !ok {
                return 0, fmt.Errorf("invalid size %q", s)
            }
            f, _ := strconv.ParseFloat(m[1], 64)
            return int64(f * mult), nil
         """),
    dict(id="date", flag="DateFlag", acc="Date", gotype="time.Time", imports=["time"],
         desc="which accepts a calendar date in ISO format `YYYY-MM-DD` and parses it into a `time.Time` at "
              "midnight UTC.",
         valid=[("2024-02-29", "2024-02-29 00:00:00 +0000 UTC")], invalid=["2024-13-01", "01/02/2024", "2023-02-29"],
         parse="""
            return time.Parse("2006-01-02", s)
         """),
    dict(id="hexcolor", flag="HexColorFlag", acc="HexColor", gotype="string", imports=["fmt", "regexp", "strings"],
         desc="which accepts a CSS hex color written as `#RGB` or `#RRGGBB` (case-insensitive, the `#` is "
              "required) and normalizes it to the lower-case 6-digit form `#rrggbb` (a `string`).",
         valid=[("#FFF", "#ffffff"), ("#1a2B3c", "#1a2b3c")], invalid=["fff", "#12345", "#ggg"],
         parse="""
            if !regexp.MustCompile(`^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$`).MatchString(s) {
                return "", fmt.Errorf("invalid hex color %q", s)
            }
            h := strings.ToLower(s[1:])
            if len(h) == 3 {
                h = string([]byte{h[0], h[0], h[1], h[1], h[2], h[2]})
            }
            return "#" + h, nil
         """),
    dict(id="cidr", flag="CIDRFlag", acc="CIDR", gotype="netip.Prefix", imports=["net/netip"],
         desc="which accepts an IP network in CIDR notation (IPv4 or IPv6) and parses it into a `netip.Prefix`.",
         valid=[("10.0.0.0/8", "10.0.0.0/8"), ("2001:db8::/32", "2001:db8::/32")],
         invalid=["10.0.0.0", "10.0.0.0/33", "x/8"],
         parse="""
            return netip.ParsePrefix(s)
         """),
    dict(id="semver", flag="SemverFlag", acc="Semver", gotype="[3]int", imports=["fmt", "regexp", "strconv"],
         desc="which accepts a semantic version `MAJOR.MINOR.PATCH` of non-negative integers, optionally "
              "prefixed with `v`, as a `[3]int`.",
         valid=[("1.2.3", "[1 2 3]"), ("v0.10.0", "[0 10 0]")], invalid=["1.2", "1.2.x", "1.2.3.4"],
         parse="""
            m := regexp.MustCompile(`^v?(\\d+)\\.(\\d+)\\.(\\d+)$`).FindStringSubmatch(s)
            if m == nil {
                return [3]int{}, fmt.Errorf("invalid version %q", s)
            }
            var v [3]int
            for i := range v {
                v[i], _ = strconv.Atoi(m[i+1])
            }
            return v, nil
         """),
]

HIDDEN = '''\
package cli

import (
	"context"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

func rwHiddenRun(args ...string) (string, error) {
	var got string
	cmd := &Command{
		Name:      "app",
		Writer:    io.Discard,
		ErrWriter: io.Discard,
		Flags:     []Flag{&%(flag)s{Name: "v"}},
		Action: func(ctx context.Context, c *Command) error {
			got = fmt.Sprint(c.%(acc)s("v"))
			return nil
		},
	}
	err := cmd.Run(context.Background(), append([]string{"app"}, args...))
	return got, err
}

func TestRWHiddenValid(t *testing.T) {
	for _, tc := range []struct{ in, want string }{
%(valid)s
	} {
		got, err := rwHiddenRun("--v", tc.in)
		if err != nil {
			t.Errorf("%%q: unexpected error %%v", tc.in, err)
			continue
		}
		if got != tc.want {
			t.Errorf("%%q: got %%q, want %%q", tc.in, got, tc.want)
		}
	}
}

func TestRWHiddenInvalid(t *testing.T) {
	for _, in := range []string{%(invalid)s} {
		if _, err := rwHiddenRun("--v", in); err == nil {
			t.Errorf("%%q: expected an error", in)
		}
	}
}

func TestRWHiddenDocs(t *testing.T) {
	word := regexp.MustCompile(`\\b%(flag)s\\b`)
	for _, p := range []string{"godoc-current.txt", "testdata/godoc-v3.x.txt", "docs/v3/examples/flags/basics.md"} {
		b, err := os.ReadFile(p)
		if err != nil || !word.Match(b) {
			t.Errorf("%%s does not mention %(flag)s", p)
		}
	}
}

func TestRWHiddenHasTests(t *testing.T) {
	files, _ := filepath.Glob("*_test.go")
	for _, f := range files {
		if f == "zz_rw_hidden_test.go" {
			continue
		}
		b, _ := os.ReadFile(f)
		if strings.Contains(string(b), "%(flag)s") {
			return
		}
	}
	t.Error("no test mentions %(flag)s")
}
'''


def prompt(v: dict) -> str:
    return PROMPT.format(flag=v["flag"], acc=v["acc"], gotype=v["gotype"], desc=v["desc"],
                         valid_ex=", ".join(f"`{a}`" for a, _ in v["valid"]),
                         invalid_ex=", ".join(f"`{a}`" for a in v["invalid"]))


def gostr(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def hidden(v: dict) -> str:
    valid = "\n".join(f"\t\t{{{gostr(a)}, {gostr(b)}}}," for a, b in v["valid"])
    return HIDDEN % {"flag": v["flag"], "acc": v["acc"], "valid": valid,
                     "invalid": ", ".join(gostr(x) for x in v["invalid"])}


def gold(v: dict, d: Path) -> None:
    x = v["id"] + "RWValue"
    T = v["gotype"]
    body = textwrap.indent(textwrap.dedent(v["parse"]).strip("\n").replace("    ", "\t"), "\t")
    imports = sorted(set(v["imports"]) | {"fmt"})
    src = f'''package cli

import (
{chr(10).join(f'	"{i}"' for i in imports)}
)

// {v["flag"]} is a flag of type {T}.
type {v["flag"]} = FlagBase[{T}, NoConfig, {x}]

type {x} struct{{ destination *{T} }}

func (v {x}) Create(val {T}, p *{T}, c NoConfig) Value {{
	*p = val
	return &{x}{{destination: p}}
}}

func (v {x}) ToString(val {T}) string {{ return fmt.Sprint(val) }}

func (v *{x}) Set(s string) error {{
	val, err := parse{v["acc"]}RW(s)
	if err != nil {{
		return err
	}}
	*v.destination = val
	return nil
}}

func (v *{x}) Get() any {{ return *v.destination }}

func (v *{x}) String() string {{
	if v.destination == nil {{
		return ""
	}}
	return fmt.Sprint(*v.destination)
}}

// {v["acc"]} looks up the value of a local {v["flag"]}.
func (cmd *Command) {v["acc"]}(name string) {T} {{
	if val, ok := cmd.Value(name).({T}); ok {{
		return val
	}}
	var zero {T}
	return zero
}}

func parse{v["acc"]}RW(s string) ({T}, error) {{
{body}
}}
'''
    (d / f"flag_{v['id']}.go").write_text(src)
    subprocess.run(["gofmt", "-w", str(d / f"flag_{v['id']}.go")], check=True)
    (d / f"flag_{v['id']}_test.go").write_text(
        f'package cli\n\nimport "testing"\n\nfunc TestGold{v["flag"]}(t *testing.T) {{\n\t_ = &{v["flag"]}{{Name: "x"}}\n}}\n')
    p = d / "docs/v3/examples/flags/basics.md"
    p.write_text(p.read_text().replace("- `TimestampFlag`\n", f"- `TimestampFlag`\n- `{v['flag']}`\n", 1))
    doc = subprocess.run(["go", "doc", "-all", "."], cwd=d, capture_output=True, text=True, check=True).stdout
    (d / "godoc-current.txt").write_text(doc)
    (d / "testdata/godoc-v3.x.txt").write_text(doc)


FAMILY = dict(repo="urfave-cli", base=URFAVE_BASE, variants=GO, prompt=prompt, hidden=hidden, gold=gold,
              hidden_path="zz_rw_hidden_test.go", pristine=[], command="go test -count=1 .", lang="go")
