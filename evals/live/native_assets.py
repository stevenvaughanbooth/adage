"""Read-only identity of a native Claude mod's inputs, before a session starts.

This records bytes, modes, additions and absence. It does not approve reflexes, validate an
execution grant, freeze a running session's mutable state, or authorize model dispatch.
"""
from __future__ import annotations

import hashlib
import os
from pathlib import Path
import re
import stat
import sys

SCHEMA = "adage-native-inputs/v1"


def entry(path):
    """Read regular inputs only; reject links/special files and changes during the read."""
    path = Path(path)
    before = path.lstat()
    mode = stat.S_IMODE(before.st_mode)
    if stat.S_ISDIR(before.st_mode):
        return {"kind": "directory", "mode": mode}
    if not stat.S_ISREG(before.st_mode):
        raise ValueError(f"native input is linked or nonregular: {path}")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    fd = os.open(path, flags)
    with os.fdopen(fd, "rb") as handle:
        opened = os.fstat(handle.fileno())
        if not stat.S_ISREG(opened.st_mode) or (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino):
            raise ValueError(f"native input changed before reading: {path}")
        sha = hashlib.sha256()
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            sha.update(block)
        after = os.fstat(handle.fileno())
    final = path.lstat()
    identity = lambda value: (value.st_dev, value.st_ino, value.st_size, value.st_mode,
                              value.st_mtime_ns, value.st_ctime_ns)
    if identity(before) != identity(after) or identity(before) != identity(final):
        raise ValueError(f"native input changed while reading: {path}")
    return {"kind": "file", "mode": mode, "size": before.st_size, "sha256": sha.hexdigest()}


def tree(root):
    """Inventory every entry, including empty directories and their permissions."""
    root = Path(root)
    if entry(root)["kind"] != "directory":
        raise ValueError("native input tree must be a directory")
    result = {".": entry(root)}
    for directory, names, files in os.walk(root, followlinks=False):
        names.sort()
        for name in sorted(names + files):
            path = Path(directory) / name
            result[path.relative_to(root).as_posix()] = entry(path)
    return result


def optional_file(path):
    path = Path(path)
    # Presence is itself an input. A dangling link must not look absent.
    if not path.exists() and not path.is_symlink():
        return None
    result = entry(path)
    if result["kind"] != "file":
        raise ValueError("managed policy must be a regular file or absent")
    return result


def plugin_identity(plugin, adage, adage_exec):
    plugin = Path(plugin).absolute()
    assets = tree(plugin)
    required = (".claude-plugin/plugin.json", "hooks/hooks.json", "hooks/register.ts",
                "scripts/session-start", "dist/SHA256SUMS")
    if any(assets.get(path, {}).get("kind") != "file" for path in required):
        raise ValueError("native plugin is missing its manifest, module, shim or checksums")
    if not assets["scripts/session-start"]["mode"] & 0o111:
        raise ValueError("native plugin session-start is not executable")
    checksums = {}
    for line in (plugin / "dist/SHA256SUMS").read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([^\s]+)", line)
        if match is None or match[2] in checksums:
            raise ValueError("invalid or duplicate staged binary checksum")
        checksums[match[2]] = match[1]
    binaries = {}
    for name, supplied in (("adage", adage), ("adage-exec", adage_exec)):
        supplied = Path(supplied).absolute()
        try:
            relative = supplied.relative_to(plugin / "dist")
        except ValueError:
            raise ValueError("native binary must be staged inside the pinned plugin") from None
        if len(relative.parts) != 2 or relative.name != name or ".." in relative.parts:
            raise ValueError("native binary must use dist/<target>/<binary>")
        key = relative.as_posix()
        observed = assets.get("dist/" + key, {})
        if (observed.get("kind") != "file" or not observed["mode"] & 0o111
                or observed.get("sha256") != checksums.get(key)):
            raise ValueError("native binary is absent, nonexecutable or differs from staged checksums")
        binaries[name] = key
    if Path(binaries["adage"]).parent != Path(binaries["adage-exec"]).parent:
        raise ValueError("native binaries must have the same target")
    if tree(plugin) != assets:
        raise ValueError("native plugin changed during capture")
    return {"path": str(plugin), "entries": assets, "binaries": binaries}


def capture(plugin, home, managed_policy, adage, adage_exec):
    """Explicit paths only: never discover or read the operator's personal home or credentials.

    `managed_policy` must be the effective path in the eventual runtime, not a substitute
    chosen to hide an installed policy. Capture runs on that runtime; platform is recorded.
    Fresh-state creation and runtime-path binding are the collector's responsibility.
    """
    initial = tree(home)
    policy = optional_file(managed_policy)
    result = {"schema": SCHEMA, "scope": "pre_session_identity_only", "platform": sys.platform,
              "plugin": plugin_identity(plugin, adage, adage_exec),
              "initial_home": {"path": str(Path(home).absolute()), "entries": initial},
              "managed_policy": {"path": str(Path(managed_policy).absolute()), "entry": policy}}
    if tree(home) != initial or optional_file(managed_policy) != policy:
        raise ValueError("native runtime inputs changed during capture")
    return result


def verify(snapshot):
    """Reject any pre-session drift. This is not an after-session mutable-state validator."""
    if snapshot.get("schema") != SCHEMA or snapshot.get("scope") != "pre_session_identity_only":
        raise ValueError("unsupported native input identity")
    plugin = snapshot["plugin"]
    root = Path(plugin["path"])
    actual = capture(root, snapshot["initial_home"]["path"], snapshot["managed_policy"]["path"],
                     root / "dist" / plugin["binaries"]["adage"],
                     root / "dist" / plugin["binaries"]["adage-exec"])
    if actual != snapshot:
        raise ValueError("native runtime inputs differ from the pre-session snapshot")
    return actual
