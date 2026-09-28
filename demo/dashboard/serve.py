#!/usr/bin/env python3
"""Serve the live demo dashboard: http://127.0.0.1:8765  (reads $DEMO_ROOT/state.json), and the
signal box at /brain, which draws `adage graph --json` for $ADAGE_HOME (default
$DEMO_ROOT/adage-home) and refreshes it live. $ADAGE_BIN overrides the binary. The signal box page
lives with the binary (crates/adage-cli/assets/brain.html), which also serves it on its own:
`adage map`."""
import functools
import http.server
import os
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = Path(os.environ.get("DEMO_ROOT", "/tmp/adage-demo"))
PORT = int(os.environ.get("DEMO_PORT", "8765"))
ADAGE_HOME = os.environ.get("ADAGE_HOME", str(ROOT / "adage-home"))
ADAGE_BIN = os.environ.get("ADAGE_BIN", str(HERE.parent.parent / "target" / "release" / "adage"))
BRAIN = HERE.parent.parent / "crates" / "adage-cli" / "assets" / "brain.html"
PAGE_HEAD = b'<!doctype html>\n<html lang="en"><meta charset="utf-8">\n<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">\n'


def graph_json():
    env = {**os.environ, "ADAGE_HOME": ADAGE_HOME}
    out = subprocess.run([ADAGE_BIN, "graph", "--json", "--events", "400"], env=env, capture_output=True, timeout=10, check=True)
    return out.stdout


class H(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/state.json"):
            p = ROOT / "state.json"
            body = p.read_bytes() if p.exists() else b'{"runs":{},"phase":"idle"}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path.startswith("/graph.json"):
            try:
                body, code = graph_json(), 200
            except Exception as e:  # the page shows "offline" and retries
                body, code = str(e).encode(), 503
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path in ("/brain", "/brain.html"):
            body = PAGE_HEAD + BRAIN.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path == "/snapshot.html" and (HERE / "snapshot.html").exists():
            body = (HERE / "snapshot.html").read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path in ("/", "/index.html"):
            body = (HERE / "index.html").read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(body)
            return
        self.send_error(404)

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    print(f"Adage dashboard: http://127.0.0.1:{PORT}  (state: {ROOT / 'state.json'})")
    print(f"Signal box:      http://127.0.0.1:{PORT}/brain  (ADAGE_HOME={ADAGE_HOME})")
    # An explicit directory avoids os.getcwd(), which fails when the cwd is not readable.
    handler = functools.partial(H, directory=str(HERE))
    http.server.ThreadingHTTPServer(("127.0.0.1", PORT), handler).serve_forever()
