#!/usr/bin/env python3
"""Serve the live demo dashboard: http://127.0.0.1:8765  (reads $DEMO_ROOT/state.json), and the
map at /brain, which draws `adage graph --json` for $ADAGE_HOME (default
$DEMO_ROOT/adage-home) and refreshes it live. $ADAGE_BIN overrides the binary. The map page
lives with the binary (crates/adage-cli/assets/brain.html), which also serves it on its own:
`adage graph --serve`."""
import functools
import http.server
import os
import re
import subprocess
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

HERE = Path(__file__).resolve().parent
ROOT = Path(os.environ.get("DEMO_ROOT", "/tmp/adage-demo"))
PORT = int(os.environ.get("DEMO_PORT", "8765"))
ADAGE_HOME = os.environ.get("ADAGE_HOME", str(ROOT / "adage-home"))
ADAGE_BIN = os.environ.get("ADAGE_BIN", str(HERE.parent.parent / "target" / "release" / "adage"))
BRAIN = HERE.parent.parent / "crates" / "adage-cli" / "assets" / "brain.html"


def graph_json(session=None, repo=None):
    env = {**os.environ, "ADAGE_HOME": ADAGE_HOME}
    args = [ADAGE_BIN, "graph", "--json", "--events", "400"]
    if session is not None:
        args += ["--session", session, "--repo", repo]
    out = subprocess.run(args, env=env, capture_output=True, timeout=10, check=True)
    return out.stdout


class H(http.server.SimpleHTTPRequestHandler):
    def do_HEAD(self):
        self.do_GET()

    def body(self, body):
        if self.command != "HEAD":
            self.wfile.write(body)

    def do_GET(self):
        if self.headers.get("Host", "").lower() not in (f"127.0.0.1:{self.server.server_port}", f"localhost:{self.server.server_port}"):
            self.send_error(403, "Local requests only")
            return
        url = urlsplit(self.path)
        if url.path == "/trace.json":
            try:
                if re.search(r"%(?![0-9a-fA-F]{2})", url.query):
                    raise ValueError("Malformed escape")
                query = parse_qs(url.query, keep_blank_values=True, strict_parsing=True, errors="strict")
                if set(query) != {"session", "repo"} or any(len(v) != 1 for v in query.values()):
                    raise ValueError("Expected one session and repository")
                session, repo = query["session"][0], query["repo"][0]
                if not re.fullmatch(r"[A-Za-z0-9_-]{1,113}", session) or len(repo.encode()) > 2048 or any(ord(c) < 32 or ord(c) == 127 for c in repo):
                    raise ValueError("Invalid identifier")
            except (ValueError, UnicodeError):
                self.send_error(400, "Invalid trace identifiers")
                return
            try:
                body, code = graph_json(session, repo), 200
            except subprocess.CalledProcessError as error:
                body, code = b"Recorded trace not found", 404 if b"Recorded trace not found" in error.stderr else 503
            except Exception as error:
                body, code = str(error).encode(), 503
            self.send_response(code)
            self.send_header("Content-Type", "application/json" if code == 200 else "text/plain")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.body(body)
            return
        if url.path == "/state.json":
            p = ROOT / "state.json"
            body = p.read_bytes() if p.exists() else b'{"runs":{},"phase":"idle"}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.body(body)
            return
        if url.path == "/graph.json":
            try:
                body, code = graph_json(), 200
            except Exception as e:  # the page shows "offline" and retries
                body, code = str(e).encode(), 503
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.body(body)
            return
        if url.path in ("/brain", "/brain.html"):
            body = BRAIN.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.body(body)
            return
        if url.path == "/snapshot.html" and (HERE / "snapshot.html").exists():
            body = (HERE / "snapshot.html").read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.body(body)
            return
        if url.path in ("/", "/index.html"):
            body = (HERE / "index.html").read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.body(body)
            return
        self.send_error(404)

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    print(f"Adage dashboard: http://127.0.0.1:{PORT}  (state: {ROOT / 'state.json'})")
    print(f"Adage map:       http://127.0.0.1:{PORT}/brain  (ADAGE_HOME={ADAGE_HOME})")
    # An explicit directory avoids os.getcwd(), which fails when the cwd is not readable.
    handler = functools.partial(H, directory=str(HERE))
    http.server.ThreadingHTTPServer(("127.0.0.1", PORT), handler).serve_forever()
