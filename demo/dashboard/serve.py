#!/usr/bin/env python3
"""Serve the live demo dashboard and its map through the current Adage binary.

/state.json reads $DEMO_ROOT/state.json. /brain redirects to an owned, read-only
`adage map` server on an ephemeral loopback port, with the same $ADAGE_HOME.
The binary supplies the React assets, trace API and event stream together.
"""
import functools
import http.server
import os
import subprocess
from urllib.parse import urlsplit
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = Path(os.environ.get("DEMO_ROOT", "/tmp/adage-demo"))
PORT = int(os.environ.get("DEMO_PORT", "8766"))  # adage map keeps 8765
ADAGE_HOME = os.environ.get("ADAGE_HOME", str(ROOT / "adage-home"))
ADAGE_BIN = os.environ.get("ADAGE_BIN", str(HERE.parent.parent / "target" / "release" / "adage"))
MAP_URL = None


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
        if urlsplit(self.path).path in ("/brain", "/brain.html"):
            if not MAP_URL:
                self.send_error(503, "The local map server is unavailable")
                return
            query = urlsplit(self.path).query
            self.send_response(302)
            self.send_header("Location", MAP_URL + ("?" + query if query else ""))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
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


def start_map():
    """The map owns its API and assets; no duplicate web server implementation."""
    proc = subprocess.Popen(
        [ADAGE_BIN, "map", "--no-open", "--port", "0"],
        env={**os.environ, "ADAGE_HOME": ADAGE_HOME},
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        text=True, bufsize=1,
    )
    try:
        first = proc.stdout.readline()
        url = next((word for word in first.split() if word.startswith("http://127.0.0.1:")), None)
        if not url:
            raise RuntimeError("The Adage map server did not report its loopback address")
        return proc, url
    except BaseException:
        stop_map(proc)
        raise


def stop_map(proc):
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
    if proc.stdout:
        proc.stdout.close()


if __name__ == "__main__":
    map_process, MAP_URL = start_map()
    try:
        print(f"Adage dashboard: http://127.0.0.1:{PORT}  (state: {ROOT / 'state.json'})")
        print(f"Map:             {MAP_URL}  (ADAGE_HOME={ADAGE_HOME})")
        # An explicit directory avoids os.getcwd(), which fails when the cwd is not readable.
        handler = functools.partial(H, directory=str(HERE))
        with http.server.ThreadingHTTPServer(("127.0.0.1", PORT), handler) as server:
            server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        stop_map(map_process)
