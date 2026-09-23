"""Run one clustering in a fresh subprocess under /usr/bin/time -l, so wall-clock and peak RSS
belong to that run alone. Rust methods run the s7c binary; Python methods run s7.run_method."""
from __future__ import annotations

import json
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
S7C = ROOT / "rust" / "target" / "release" / "s7c"
RUST = {"louvain", "knn_dbscan", "hdbscan_crate"}


def write_emb(path: Path, E: np.ndarray) -> None:
    E = np.ascontiguousarray(E, dtype="<f4")
    with open(path, "wb") as f:
        f.write(np.array([E.shape[0], E.shape[1]], dtype="<u4").tobytes())
        f.write(E.tobytes())


def _timed(cmd: list[str], timeout: int) -> tuple[str, float, int, bool]:
    import os
    import signal
    t = time.time()
    # Own process group, so a timeout kills the measured process too, not just /usr/bin/time.
    proc = subprocess.Popen(["/usr/bin/time", "-l", *cmd], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, start_new_session=True)
    try:
        out, err = proc.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.communicate()
        return "", time.time() - t, 0, True
    wall = time.time() - t
    p = subprocess.CompletedProcess(proc.args, proc.returncode, out, err)
    m = re.search(r"(\d+)\s+maximum resident set size", p.stderr)
    if p.returncode != 0:
        raise RuntimeError(p.stderr[-2000:])
    return p.stdout, wall, int(m.group(1)) if m else 0, False


def run(method: str, E: np.ndarray, params: dict, groups: np.ndarray | None = None,
        canonical: bool = False, timeout: int = 1800) -> dict:
    with tempfile.TemporaryDirectory(dir=ROOT / ".cache") as td:
        td = Path(td)
        out = td / "labels.bin"
        if method in RUST:
            write_emb(td / "emb.bin", E)
            cmd = [str(S7C), str(td / "emb.bin"), str(out), "--method", method]
            for k, v in params.items():
                cmd += [f"--{k.replace('_', '-')}", str(v)]
            if groups is not None:
                (td / "groups.bin").write_bytes(np.asarray(groups, dtype="<u4").tobytes())
                cmd += ["--groups", str(td / "groups.bin")]
            if canonical:
                cmd.append("--canonical")
        else:
            np.save(td / "emb.npy", E)
            np.save(td / "groups.npy", np.zeros(len(E), dtype=np.int64) if groups is None else np.asarray(groups))
            cmd = [sys.executable, "-W", "ignore", "-m", "s7.run_method", str(td / "emb.npy"), str(td / "groups.npy"),
                   str(out), method, json.dumps(params)]
        stdout, wall, rss, timed_out = _timed(cmd, timeout)
        if timed_out:
            return {"timeout": True, "wall_s": round(wall, 1)}
        labels = np.frombuffer(out.read_bytes(), dtype="<i4").astype(np.int64)
        stats = json.loads(stdout.strip().splitlines()[-1]) if stdout.strip() else {}
        return {"labels": labels, "wall_s": round(wall, 2), "peak_rss_mb": round(rss / 2**20, 1), **stats}
