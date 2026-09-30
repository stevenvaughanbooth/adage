"""Showcase family on Go: `chi-middleware`, "add a middleware to go-chi/chi's middleware package".

go-chi/chi @ 3d1777a (v5, the upstream head on 2026-09-29). New for the showcase: this repository was in
the data-expansion corpus (one-off bug fixes only), never in card research.

The procedure that repeats is the project's own (e.g. upstream #844 + #893 `Sunset`, #503
`RouteHeaders`, `PageRoute`, `PathRewrite`):
  - `middleware/<name>.go`: the exported middleware with a Go doc comment (`// Name ...`);
  - `middleware/<name>_test.go`: tests (package middleware, net/http/httptest);
  - `README.md`: a row in the "Core middlewares" table and its pkg.go.dev link reference;
  - CI: `make test` (`go test` of the root and ./middleware).

Each variant's prompt gives the behaviour with examples, and names the deliverables (tests, README)
but not where or how. The hidden check:
  documented  a `// Name` doc comment directly above `func Name(` in middleware/*.go
  readme      the README's middleware table has a `| [Name]` row and a `[Name]: https://pkg.go.dev/...#Name`
              link reference
  own_test    a test file the agent added or changed under middleware/ names the middleware
  tests       `go test -count=1 ./...` with the hidden test middleware/zz_showcase_hidden_test.go: every
              example of the prompt, through httptest

Order fixed before any session: the first 4 are the training sessions (a user's history).
"""

from __future__ import annotations

import json
import re
import subprocess
import textwrap
from pathlib import Path

CHI_BASE = "3d1777a"

PROMPT = (
    "Add a new middleware to chi's middleware package: `middleware.{sig}`, which {desc}\n\n"
    "Examples{ctor_note}:\n{examples}\n\n"
    "Include tests, and document it in the README's list of middlewares, as the project does for its "
    "other middlewares."
)

# A case: request (method, target, host, headers, content_length, body) -> expected status, response
# headers (exact value, or None = absent), and what the next handler saw (method, path, query).
# `ctor` is the Go expression that builds the middleware (func(http.Handler) http.Handler).


def C(method="GET", target="/", status=200, headers=(), host="", cl=None, body="", want=None, seen=None, ctor=None):
    return dict(method=method, target=target, status=status, headers=list(headers), host=host, cl=cl, body=body,
                want=dict(want or {}), seen=dict(seen or {}), ctor=ctor)


VARIANTS = [
    # ------------------------------------------------------------------ training (a user's history)
    dict(id="nosniff", name="NoSniff", file="no_sniff", sig="NoSniff(next http.Handler) http.Handler",
         ctor="NoSniff", imports=[],
         desc="sets the `X-Content-Type-Options: nosniff` response header on every response.",
         row="Sets the X-Content-Type-Options: nosniff response header",
         cases=[C("GET", "/", want={"X-Content-Type-Options": "nosniff"}),
                C("POST", "/upload", want={"X-Content-Type-Options": "nosniff"})],
         gold="""
            // NoSniff sets the X-Content-Type-Options: nosniff response header, which tells browsers not to
            // guess the content type of a response.
            func NoSniff(next http.Handler) http.Handler {
            	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            		w.Header().Set("X-Content-Type-Options", "nosniff")
            		next.ServeHTTP(w, r)
            	})
            }
         """),
    dict(id="allow-methods", name="AllowMethods", file="allow_methods",
         sig="AllowMethods(methods ...string) func(http.Handler) http.Handler", ctor='AllowMethods("GET", "POST")',
         imports=["strings"],
         desc="responds `405 Method Not Allowed`, with an `Allow` header listing the allowed methods "
              "(comma-separated, in the order given), to requests whose method is not one of `methods`; other "
              "requests reach the next handler.",
         row="Responds 405 with an Allow header to requests with other methods",
         cases=[C("GET", "/a", 200), C("DELETE", "/a", 405, want={"Allow": "GET, POST"}), C("POST", "/a", 200)],
         gold="""
            // AllowMethods responds 405 Method Not Allowed, with an Allow header, to requests whose method is
            // not one of methods.
            func AllowMethods(methods ...string) func(http.Handler) http.Handler {
            	allow := strings.Join(methods, ", ")
            	return func(next http.Handler) http.Handler {
            		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            			for _, m := range methods {
            				if r.Method == m {
            					next.ServeHTTP(w, r)
            					return
            				}
            			}
            			w.Header().Set("Allow", allow)
            			w.WriteHeader(http.StatusMethodNotAllowed)
            		})
            	}
            }
         """),
    dict(id="hsts", name="HSTS", file="hsts", sig="HSTS(maxAge time.Duration, includeSubdomains bool) "
                                                  "func(http.Handler) http.Handler",
         ctor=None, imports=["strconv", "time"],
         desc="sets the `Strict-Transport-Security` response header to `max-age=<seconds>`, followed by "
              "`; includeSubDomains` when `includeSubdomains` is true.",
         row="Sets the Strict-Transport-Security response header",
         cases=[C("GET", "/", ctor="HSTS(24*time.Hour, false)", want={"Strict-Transport-Security": "max-age=86400"}),
                C("GET", "/", ctor="HSTS(time.Hour, true)",
                  want={"Strict-Transport-Security": "max-age=3600; includeSubDomains"})],
         gold="""
            // HSTS sets the Strict-Transport-Security response header to max-age=<seconds>, with
            // includeSubDomains when includeSubdomains is true.
            func HSTS(maxAge time.Duration, includeSubdomains bool) func(http.Handler) http.Handler {
            	value := "max-age=" + strconv.FormatInt(int64(maxAge/time.Second), 10)
            	if includeSubdomains {
            		value += "; includeSubDomains"
            	}
            	return func(next http.Handler) http.Handler {
            		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            			w.Header().Set("Strict-Transport-Security", value)
            			next.ServeHTTP(w, r)
            		})
            	}
            }
         """),
    dict(id="require-header", name="RequireHeader", file="require_header",
         sig="RequireHeader(name string) func(http.Handler) http.Handler", ctor='RequireHeader("X-Tenant")',
         imports=[],
         desc="responds `400 Bad Request` to requests that do not carry the request header `name`, or carry it "
              "empty; other requests reach the next handler.",
         row="Responds 400 to requests without a given request header",
         cases=[C("GET", "/", 200, headers=[("X-Tenant", "acme")]), C("GET", "/", 400),
                C("GET", "/", 400, headers=[("X-Tenant", "")])],
         gold="""
            // RequireHeader responds 400 Bad Request to requests that do not carry the request header name, or
            // carry it empty.
            func RequireHeader(name string) func(http.Handler) http.Handler {
            	return func(next http.Handler) http.Handler {
            		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            			if r.Header.Get(name) == "" {
            				http.Error(w, http.StatusText(http.StatusBadRequest), http.StatusBadRequest)
            				return
            			}
            			next.ServeHTTP(w, r)
            		})
            	}
            }
         """),
    # ---------------------------------------------------------------------------------- held out
    dict(id="method-override", name="MethodOverride", file="method_override",
         sig="MethodOverride(next http.Handler) http.Handler", ctor="MethodOverride", imports=["strings"],
         desc="lets a `POST` request override its method with the `X-HTTP-Method-Override` request header: when "
              "the header is `PUT`, `PATCH` or `DELETE` (case-insensitive), the next handler sees that method, "
              "upper-cased. Any other method or header value is left unchanged.",
         row="Overrides a POST request's method from the X-HTTP-Method-Override header",
         cases=[C("POST", "/items", headers=[("X-HTTP-Method-Override", "DELETE")], seen={"method": "DELETE"}),
                C("POST", "/items", headers=[("X-HTTP-Method-Override", "patch")], seen={"method": "PATCH"}),
                C("GET", "/items", headers=[("X-HTTP-Method-Override", "DELETE")], seen={"method": "GET"}),
                C("POST", "/items", headers=[("X-HTTP-Method-Override", "TRACE")], seen={"method": "POST"})],
         gold="""
            // MethodOverride lets a POST request override its method with the X-HTTP-Method-Override request
            // header (PUT, PATCH or DELETE).
            func MethodOverride(next http.Handler) http.Handler {
            	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            		if r.Method == http.MethodPost {
            			switch m := strings.ToUpper(r.Header.Get("X-HTTP-Method-Override")); m {
            			case http.MethodPut, http.MethodPatch, http.MethodDelete:
            				r.Method = m
            			}
            		}
            		next.ServeHTTP(w, r)
            	})
            }
         """),
    dict(id="allow-hosts", name="AllowHosts", file="allow_hosts",
         sig="AllowHosts(hosts ...string) func(http.Handler) http.Handler",
         ctor='AllowHosts("example.com", "api.example.com")', imports=["net", "strings"],
         desc="responds `403 Forbidden` to requests whose `Host`, without the port and compared "
              "case-insensitively, is not one of `hosts`; other requests reach the next handler.",
         row="Responds 403 to requests for hosts not in an allowlist",
         cases=[C("GET", "/", 200, host="example.com"), C("GET", "/", 200, host="API.Example.com:8080"),
                C("GET", "/", 403, host="evil.com")],
         gold="""
            // AllowHosts responds 403 Forbidden to requests whose Host (without the port, case-insensitive) is
            // not one of hosts.
            func AllowHosts(hosts ...string) func(http.Handler) http.Handler {
            	return func(next http.Handler) http.Handler {
            		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            			host := r.Host
            			if h, _, err := net.SplitHostPort(host); err == nil {
            				host = h
            			}
            			for _, a := range hosts {
            				if strings.EqualFold(host, a) {
            					next.ServeHTTP(w, r)
            					return
            				}
            			}
            			http.Error(w, http.StatusText(http.StatusForbidden), http.StatusForbidden)
            		})
            	}
            }
         """),
    dict(id="lowercase-path", name="LowercasePath", file="lowercase_path",
         sig="LowercasePath(next http.Handler) http.Handler", ctor="LowercasePath", imports=["strings"],
         desc="lower-cases the request's URL path (`r.URL.Path`, and `r.URL.RawPath` when it is set) before the "
              "next handler sees it; the query string is left as it is.",
         row="Lower-cases the request path",
         cases=[C("GET", "/Users/ABC?Q=1", seen={"path": "/users/abc", "query": "Q=1"}),
                C("GET", "/already/lower", seen={"path": "/already/lower"})],
         gold="""
            // LowercasePath lower-cases the request's URL path (and RawPath when set) before the next handler
            // sees it.
            func LowercasePath(next http.Handler) http.Handler {
            	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            		r.URL.Path = strings.ToLower(r.URL.Path)
            		if r.URL.RawPath != "" {
            			r.URL.RawPath = strings.ToLower(r.URL.RawPath)
            		}
            		next.ServeHTTP(w, r)
            	})
            }
         """),
    dict(id="block-user-agents", name="BlockUserAgents", file="block_user_agents",
         sig="BlockUserAgents(substrings ...string) func(http.Handler) http.Handler",
         ctor='BlockUserAgents("curl", "BadBot")', imports=["strings"],
         desc="responds `403 Forbidden` to requests whose `User-Agent` contains any of `substrings`, compared "
              "case-insensitively; requests without a User-Agent, and all others, reach the next handler.",
         row="Responds 403 to requests from blocked user agents",
         cases=[C("GET", "/", 403, headers=[("User-Agent", "curl/8.0")]),
                C("GET", "/", 403, headers=[("User-Agent", "Mozilla/5.0 (compatible; badbot/1.2)")]),
                C("GET", "/", 200, headers=[("User-Agent", "Mozilla/5.0")]), C("GET", "/", 200)],
         gold="""
            // BlockUserAgents responds 403 Forbidden to requests whose User-Agent contains any of substrings
            // (case-insensitive).
            func BlockUserAgents(substrings ...string) func(http.Handler) http.Handler {
            	return func(next http.Handler) http.Handler {
            		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            			ua := strings.ToLower(r.UserAgent())
            			for _, s := range substrings {
            				if ua != "" && strings.Contains(ua, strings.ToLower(s)) {
            					http.Error(w, http.StatusText(http.StatusForbidden), http.StatusForbidden)
            					return
            				}
            			}
            			next.ServeHTTP(w, r)
            		})
            	}
            }
         """),
    dict(id="api-key", name="APIKey", file="api_key",
         sig="APIKey(header string, keys ...string) func(http.Handler) http.Handler",
         ctor='APIKey("X-API-Key", "k1", "k2")', imports=[],
         desc="responds `401 Unauthorized` to requests whose request header `header` is not exactly one of "
              "`keys` (a missing header included); other requests reach the next handler.",
         row="Responds 401 to requests without a valid API key header",
         cases=[C("GET", "/", 200, headers=[("X-API-Key", "k2")]), C("GET", "/", 401, headers=[("X-API-Key", "K2")]),
                C("GET", "/", 401)],
         gold="""
            // APIKey responds 401 Unauthorized to requests whose request header is not exactly one of keys.
            func APIKey(header string, keys ...string) func(http.Handler) http.Handler {
            	return func(next http.Handler) http.Handler {
            		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            			got := r.Header.Get(header)
            			for _, k := range keys {
            				if got != "" && got == k {
            					next.ServeHTTP(w, r)
            					return
            				}
            			}
            			http.Error(w, http.StatusText(http.StatusUnauthorized), http.StatusUnauthorized)
            		})
            	}
            }
         """),
    dict(id="vary", name="Vary", file="vary", sig="Vary(headers ...string) func(http.Handler) http.Handler",
         ctor='Vary("Accept-Encoding", "Origin")', imports=[],
         desc="adds each of `headers` to the response's `Vary` header, keeping the `Vary` values the response "
              "already has.",
         row="Adds values to the Vary response header",
         cases=[C("GET", "/", want={"Vary": "Accept-Encoding, Origin"})],
         gold="""
            // Vary adds each of headers to the response's Vary header.
            func Vary(headers ...string) func(http.Handler) http.Handler {
            	return func(next http.Handler) http.Handler {
            		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            			for _, h := range headers {
            				w.Header().Add("Vary", h)
            			}
            			next.ServeHTTP(w, r)
            		})
            	}
            }
         """),
    dict(id="content-length-required", name="ContentLengthRequired", file="content_length_required",
         sig="ContentLengthRequired(next http.Handler) http.Handler", ctor="ContentLengthRequired", imports=[],
         desc="responds `411 Length Required` to `POST`, `PUT` and `PATCH` requests whose body length is unknown "
              "(`r.ContentLength == -1`, as for a chunked body); other requests reach the next handler.",
         row="Responds 411 to POST, PUT and PATCH requests without a Content-Length",
         cases=[C("POST", "/", 411, cl=-1), C("PUT", "/", 411, cl=-1), C("POST", "/", 200, body="hello"),
                C("GET", "/", 200, cl=-1)],
         gold="""
            // ContentLengthRequired responds 411 Length Required to POST, PUT and PATCH requests whose body
            // length is unknown.
            func ContentLengthRequired(next http.Handler) http.Handler {
            	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            		switch r.Method {
            		case http.MethodPost, http.MethodPut, http.MethodPatch:
            			if r.ContentLength == -1 {
            				http.Error(w, http.StatusText(http.StatusLengthRequired), http.StatusLengthRequired)
            				return
            			}
            		}
            		next.ServeHTTP(w, r)
            	})
            }
         """),
    dict(id="max-header-count", name="MaxHeaderCount", file="max_header_count",
         sig="MaxHeaderCount(n int) func(http.Handler) http.Handler", ctor="MaxHeaderCount(2)", imports=[],
         desc="responds `431 Request Header Fields Too Large` to requests that carry more than `n` header fields "
              "(counted as distinct names in `r.Header`); other requests reach the next handler.",
         row="Responds 431 to requests with too many header fields",
         cases=[C("GET", "/", 200, headers=[("A", "1"), ("B", "2")]),
                C("GET", "/", 431, headers=[("A", "1"), ("B", "2"), ("C", "3")]), C("GET", "/", 200)],
         gold="""
            // MaxHeaderCount responds 431 Request Header Fields Too Large to requests with more than n header
            // fields.
            func MaxHeaderCount(n int) func(http.Handler) http.Handler {
            	return func(next http.Handler) http.Handler {
            		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            			if len(r.Header) > n {
            				http.Error(w, http.StatusText(http.StatusRequestHeaderFieldsTooLarge),
            					http.StatusRequestHeaderFieldsTooLarge)
            				return
            			}
            			next.ServeHTTP(w, r)
            		})
            	}
            }
         """),
]

HIDDEN_PATH = "middleware/zz_showcase_hidden_test.go"


def _case_line(c: dict) -> str:
    req = f"`{c['method']} {c['target']}`"
    extra = []
    if c["host"]:
        extra.append(f"`Host: {c['host']}`")
    for k, x in c["headers"]:
        extra.append(f"`{k}: {x}`" if x else f"an empty `{k}`")
    if c["cl"] == -1:
        extra.append("a body of unknown length")
    if c["body"]:
        extra.append(f"the body `{c['body']}`")
    left = req + (" with " + ", ".join(extra) if extra else "")
    right = []
    if c["seen"]:
        right.append("the next handler sees " + ", ".join(
            {"method": f"method `{c['seen'].get('method')}`", "path": f"path `{c['seen'].get('path')}`",
             "query": f"query `{c['seen'].get('query')}`"}[k] for k in c["seen"]))
    else:
        right.append(f"`{c['status']}`" + (" (reaches the next handler)" if c["status"] == 200 else ""))
    for k, x in c["want"].items():
        right.append(f"`{k}: {x}`")
    pre = f"`{c['ctor']}`: " if c["ctor"] else ""
    return f"- {pre}{left} -> {', '.join(right)}"


def prompt(v: dict) -> str:
    note = f" with `{v['ctor']}`" if v["ctor"] and v["ctor"] != v["name"] else ""
    return PROMPT.format(sig=v["sig"], desc=v["desc"], ctor_note=note,
                         examples="\n".join(_case_line(c) for c in v["cases"]))


def _go(s) -> str:
    return json.dumps(s)


def hidden(v: dict) -> str:
    rows = []
    for c in v["cases"]:
        hdr = ", ".join(f"{{{_go(k)}, {_go(x)}}}" for k, x in c["headers"])
        want = ", ".join(f"{_go(k)}: {_go(x)}" for k, x in c["want"].items())
        seen = ", ".join(f"{_go(k)}: {_go(x)}" for k, x in c["seen"].items())
        cl = -2 if c["cl"] is None else c["cl"]
        rows.append(f"\t\t{{{c['ctor'] or v['ctor']}, {_go(c['method'])}, {_go(c['target'])}, {_go(c['host'])}, "
                    f"[][2]string{{{hdr}}}, {cl}, {_go(c['body'])}, {c['status']}, map[string]string{{{want}}}, "
                    f"map[string]string{{{seen}}}}},")
    imports = sorted({"io", "net/http", "net/http/httptest", "strings", "testing"}
                     | ({"time"} if any("time." in (c["ctor"] or "") for c in v["cases"]) else set()))
    imp = "\n".join(f"\t{_go(i)}" for i in imports)
    return f'''// Hidden check of the Adage showcase (evals/showcase). Written by the scorer.
package middleware

import (
{imp}
)

func TestSCHidden{v["name"]}(t *testing.T) {{
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {{
		w.Header().Set("X-SC-Method", r.Method)
		w.Header().Set("X-SC-Path", r.URL.Path)
		w.Header().Set("X-SC-Query", r.URL.RawQuery)
		w.WriteHeader(http.StatusOK)
	}})
	for i, tc := range []struct {{
		mw                   func(http.Handler) http.Handler
		method, target, host string
		hdr                  [][2]string
		cl                   int64
		body                 string
		status               int
		want, seen           map[string]string
	}}{{
{chr(10).join(rows)}
	}} {{
		var body io.Reader
		if tc.body != "" {{
			body = strings.NewReader(tc.body)
		}}
		req := httptest.NewRequest(tc.method, tc.target, body)
		if tc.host != "" {{
			req.Host = tc.host
		}}
		for _, h := range tc.hdr {{
			req.Header.Set(h[0], h[1])
		}}
		if tc.cl != -2 {{
			req.ContentLength = tc.cl
		}}
		rec := httptest.NewRecorder()
		tc.mw(next).ServeHTTP(rec, req)
		res := rec.Result()
		if len(tc.seen) == 0 && res.StatusCode != tc.status {{
			t.Errorf("case %d: status %d, want %d", i, res.StatusCode, tc.status)
		}}
		for k, want := range tc.want {{
			if got := strings.Join(res.Header.Values(k), ", "); got != want {{
				t.Errorf("case %d: header %s = %q, want %q", i, k, got, want)
			}}
		}}
		for k, want := range tc.seen {{
			got := res.Header.Get(map[string]string{{"method": "X-SC-Method", "path": "X-SC-Path", "query": "X-SC-Query"}}[k])
			if res.StatusCode != http.StatusOK || got != want {{
				t.Errorf("case %d: next handler saw %s %q (status %d), want %q", i, k, got, res.StatusCode, want)
			}}
		}}
	}}
}}
'''


def checks(v: dict, d: Path, changed: list[str]) -> dict:
    name = v["name"]
    documented = False
    for p in sorted((d / "middleware").glob("*.go")):
        if p.name.endswith("_test.go"):
            continue
        text = p.read_text(errors="replace")
        m = re.search(r"^func " + re.escape(name) + r"\(", text, re.M)
        if m:  # the Go convention: the comment block right above the func starts with its name
            block = []
            for line in reversed(text[:m.start()].rstrip("\n").splitlines()):
                if not line.strip().startswith("//"):
                    break
                block.insert(0, line.strip())
            documented = bool(block) and re.match(r"//\s*" + re.escape(name) + r"\b", block[0]) is not None
            break
    readme = (d / "README.md").read_text(errors="replace") if (d / "README.md").exists() else ""
    row = re.search(r"^\|\s*\[" + re.escape(name) + r"\]\s*\|", readme, re.M) is not None
    link = re.search(r"^\[" + re.escape(name) + r"\]:\s*https://pkg\.go\.dev/\S*#" + re.escape(name) + r"\s*$",
                     readme, re.M) is not None
    own = any(p.startswith("middleware/") and p.endswith("_test.go") and p != HIDDEN_PATH and (d / p).is_file()
              and re.search(r"\b" + re.escape(name) + r"\b", (d / p).read_text(errors="replace")) for p in changed)
    return {"documented": documented, "readme": row and link, "own_test": bool(own)}


def gold(v: dict, d: Path) -> None:
    code = textwrap.dedent(v["gold"]).strip("\n")
    imports = ["net/http", *v["imports"]]
    src = "package middleware\n\nimport (\n" + "".join(f'\t"{i}"\n' for i in sorted(imports)) + ")\n\n" + code + "\n"
    f = d / "middleware" / f"{v['file']}.go"
    f.write_text(src)
    subprocess.run(["gofmt", "-w", str(f)], check=True)
    c0 = v["cases"][0]
    t = d / "middleware" / f"{v['file']}_test.go"
    t.write_text(f'package middleware\n\nimport (\n\t"net/http"\n\t"net/http/httptest"\n\t"testing"\n'
                 + ('\t"time"\n' if "time." in (c0["ctor"] or "") else "") +
                 f')\n\nfunc Test{v["name"]}Gold(t *testing.T) {{\n'
                 f'\th := {c0["ctor"] or v["ctor"]}(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {{}}))\n'
                 f'\th.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest("GET", "/", nil))\n}}\n')
    subprocess.run(["gofmt", "-w", str(t)], check=True)
    p = d / "README.md"
    text = p.read_text()
    rows = list(re.finditer(r"^\| \[(\w+)\]\s*\|.*\|\n", text, re.M))
    after = next((r for r in rows if r.group(1).lower() > v["name"].lower()), rows[-1])
    row = f"| [{v['name']}]{' ' * max(1, 22 - len(v['name']) - 2)}| {v['row']} |\n"
    text = text[:after.start()] + row + text[after.start():]
    links = list(re.finditer(r"^\[(\w+)\]: https://pkg\.go\.dev/github\.com/go-chi/chi\S*\n", text, re.M))
    link = f"[{v['name']}]: https://pkg.go.dev/github.com/go-chi/chi/v5/middleware#{v['name']}\n"
    text = text[:links[-1].end()] + link + text[links[-1].end():]
    p.write_text(text)


NEGATIVES = [
    dict(id="chi-1f927a8e", source="repobench"),
    dict(id="chi-87c34a24", source="repobench"),
    dict(id="chi-05f1ef7b", source="repobench"),
    dict(id="neg-chi-throttle-retry-after", source="handwritten", edits=["middleware/throttle.go",
                                                                          "middleware/throttle_test.go"],
         prompt="Add an option to the existing `Throttle` middleware so that it sets a `Retry-After` header on the "
                "responses it rejects. Include tests and mention the option in the README."),
    dict(id="neg-chi-readme-clientip", source="handwritten", edits=["README.md"],
         prompt="The README's section on client IPs is confusing. Add a short example showing how to pick between "
                "`ClientIPFromXFF` and `ClientIPFromRemoteAddr` behind a load balancer."),
    dict(id="neg-chi-nocache-bug", source="handwritten", edits=["middleware/nocache.go"],
         prompt="`NoCache` deletes the ETag request headers but leaves `If-None-Match` in place when it is sent in "
                "lower case by an HTTP/2 client. Fix it and add a regression test."),
]

FAMILY = dict(id="chi-middleware", repo="chi", base=CHI_BASE, variants=VARIANTS, prompt=prompt, hidden=hidden,
              gold=gold, hidden_path=HIDDEN_PATH, pristine=[], lang="go", name_key="name",
              command="go test -count=1 ./... 2>&1", checks=checks, negatives=NEGATIVES,
              repo_key="github.com/go-chi/chi", forward_env=[], confine_expose=[], prune=[],
              probe_edits=lambda v: [f"middleware/{v['file']}.go", f"middleware/{v['file']}_test.go", "README.md"])
