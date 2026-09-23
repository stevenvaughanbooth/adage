#!/usr/bin/env python3
"""Serve the live demo dashboard: http://127.0.0.1:8765  (reads $DEMO_ROOT/state.json)."""
import functools
import http.server
import os
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = Path(os.environ.get("DEMO_ROOT", "/tmp/adage-demo"))
PORT = int(os.environ.get("DEMO_PORT", "8765"))


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
    # An explicit directory avoids os.getcwd(), which fails when the cwd is not readable.
    handler = functools.partial(H, directory=str(HERE))
    http.server.ThreadingHTTPServer(("127.0.0.1", PORT), handler).serve_forever()
