"""Adage inside Harbor (github.com/harbor-framework/harbor), the Terminal-Bench 2 harness.

    harbor run -c job.json            # job.json from `plan.py eval`, agents use this class
    harbor run ... -a adage_harbor:AdageClaudeCode \
        --ak arm=adage --ak adage_bin=/abs/adage-x86_64-musl \
        --ak adage_home=/abs/artifacts/<suite>/<repo>.tgz --ak adage_repo_key=adage-eval:<suite>/<repo>

(`PYTHONPATH=evals/external` so Harbor can import this module.)

`AdageClaudeCode` is Harbor's own `ClaudeCode` agent (src/harbor/agents/installed/claude_code.py,
read at harbor commit 3c82380) with Adage's Claude Code hooks added for the arms that have them:

  off        no hooks, no binary, no Adage files: plain Claude Code (the baseline)
  holdout    hooks installed, `ADAGE_ARM=holdout`: every decision is made and spooled, nothing is
             delivered (crates/adage-cli/src/hook.rs forces the arm from ADAGE_ARM). The control.
  adage      hooks installed, `ADAGE_ARM=treated`: directives delivered via additionalContext
  agents-md  no hooks; the learned routes' summaries are appended to the system prompt
             (`--append-system-prompt`), the "just write it down in AGENTS.md" baseline, without
             writing into the repository

What it guarantees, and how:

- Hooks: exactly the block `adage hooks-config <bin>` prints (harness::claude::settings), merged into
  any user `config` settings the way `adage install --write` merges (install::merge_hooks: Adage's
  own entries replaced, everything else kept). Passed through Harbor's native `config` path, which
  uploads it to /tmp/claude-code-settings/settings.json and runs `claude --settings <that>`.
- The binary goes to /installed-agent/adage/bin/adage (Harbor creates /installed-agent); the frozen
  ADAGE_HOME is unpacked to <agent logs>/adage-home (= /logs/agent/adage-home), so spools
  (`sessions/*.spool`) come back with the trial's agent logs. `ADAGE_AUTO_LEARN=0` keeps the index
  frozen for the whole trial.
- Repository keys: the index is learned under a canonical key (`adage learn --repo <key>`). The key a
  hook computes inside the container depends on the checkout (origin URL, else `<dir>@<root commit>`,
  else `<dir>@path-<hash>`; adage-core/src/repo.rs). At setup the agent probes the checkout and
  re-keys the chosen repo's routes under every key the hook could compute there, so the lookup
  matches whatever the object store allows. Nothing else in the index changes.
- Nothing is written into the repository worktree: no .claude/, .adage/, AGENTS.md or CLAUDE.md.
  After the run the agent records `git status --porcelain` and `git diff` (with
  --no-optional-locks, which does not even refresh the index) into <agent logs>/adage/ for the patch
  cleanliness check (protocol.check_patch_clean).
- Web tools are disallowed by default (`disable_web_search=True`, Harbor's own option).
- Claude's config dir is Harbor's per-trial CLAUDE_CONFIG_DIR (<agent logs>/sessions), fresh for
  every trial, so there is no user memory; `memory_dir` is refused, and auto-memory is switched off
  by environment (CLAUDE_CODE_DISABLE_AUTO_MEMORY=1; an assumption, see README "Verified vs assumed").

Harbor is imported lazily: the pure helpers here are importable (and unit-tested) without it.
"""

from __future__ import annotations

import base64
import copy
import hashlib
import json
import re
import shlex
import tarfile
import tempfile
from pathlib import Path, PurePosixPath
from typing import Any

ARMS = ("off", "holdout", "adage", "agents-md")
HOOK_ARMS = ("holdout", "adage")
REMOTE_BIN = PurePosixPath("/installed-agent/adage/bin/adage")
# The Claude Code events Adage registers (crates/adage-cli/src/harness/claude.rs `settings`).
CLAUDE_EVENTS = ("SessionStart", "UserPromptSubmit", "PostToolBatch", "PostToolUseFailure", "Stop", "SessionEnd")
HOOK_TIMEOUT = 5
# gitobj::MAX_WALK: a longer first-parent history has no readable root commit.
MAX_WALK = 100_000
# Files the patch checker refuses in a diff (protocol.FORBIDDEN_PREFIXES mirrors this).
ADAGE_LOG_DIR = "adage"


# ------------------------------------------------------------------------------ hook settings

def hooks_settings(bin_path: str) -> dict:
    """What `adage hooks-config <bin>` prints: one exec-form command hook per event."""
    def h(ev):
        return [{"hooks": [{"type": "command", "command": bin_path, "args": ["hook", "claude", ev],
                            "timeout": HOOK_TIMEOUT}]}]
    return {"hooks": {ev: h(ev) for ev in CLAUDE_EVENTS}}


def _is_ours(entry: Any) -> bool:
    """install.rs `is_ours` for Claude Code: a hook whose args start `hook claude`."""
    if not isinstance(entry, dict):
        return False
    for h in entry.get("hooks") or []:
        args = [a for a in (h.get("args") or []) if isinstance(a, str)] if isinstance(h, dict) else []
        if len(args) >= 2 and args[0] == "hook" and args[1] == "claude":
            return True
    return False


def merge_hooks(settings: dict | None, ours: dict) -> dict:
    """install.rs `merge_hooks`: Adage's own entries replaced, everything else kept."""
    doc = copy.deepcopy(settings) if settings else {}
    if not isinstance(doc, dict):
        raise ValueError("settings is not a JSON object")
    hooks = doc.setdefault("hooks", {})
    if not isinstance(hooks, dict):
        raise ValueError('"hooks" in settings is not an object')
    for event, entries in ours["hooks"].items():
        lst = hooks.setdefault(event, [])
        if not isinstance(lst, list):
            raise ValueError(f"hooks.{event} in settings is not an array")
        lst[:] = [e for e in lst if not _is_ours(e)] + copy.deepcopy(entries)
    return doc


def strip_hooks(settings: dict | None) -> dict:
    """The settings with any Adage hook removed (for arms without hooks)."""
    doc = copy.deepcopy(settings) if settings else {}
    for event, lst in list((doc.get("hooks") or {}).items()):
        if isinstance(lst, list):
            lst[:] = [e for e in lst if not _is_ours(e)]
            if not lst:
                del doc["hooks"][event]
    if doc.get("hooks") == {}:
        del doc["hooks"]
    return doc


def settings_for_arm(arm: str, base: dict | None, bin_path: str = str(REMOTE_BIN)) -> dict:
    check_arm(arm)
    if arm in HOOK_ARMS:
        return merge_hooks(base, hooks_settings(bin_path))
    return strip_hooks(base)


def check_arm(arm: str) -> str:
    if arm not in ARMS:
        raise ValueError(f"unknown arm {arm!r} (one of {', '.join(ARMS)})")
    return arm


# Environment for every arm: no background auto-update or telemetry; auto-memory off (assumed name).
ISOLATION_ENV = {
    "CLAUDE_CODE_DISABLE_AUTO_MEMORY": "1",
    "DISABLE_AUTOUPDATER": "1",
}


def arm_env(arm: str, adage_home: str) -> dict:
    """Environment for one arm. Mirrors evals/live/runner.py `env_for`: hook arms force the arm
    (ADAGE_ARM wins over ADAGE_HOLDOUT/runtime.holdout in hook.rs) and freeze learning."""
    check_arm(arm)
    env = dict(ISOLATION_ENV)
    if arm in HOOK_ARMS:
        env.update({"ADAGE_HOME": adage_home, "ADAGE_ARM": "holdout" if arm == "holdout" else "treated",
                    "ADAGE_AUTO_LEARN": "0"})
    return env


# ---------------------------------------------------------------------------- repository keys

# Runs in the container (POSIX sh, git optional). Prints key=value lines; config is base64 so any
# byte survives. Mirrors adage-core repo.rs: find_root walks up to the nearest `.git`.
PROBE_SCRIPT = r"""
d=$(pwd -P); cwd="$d"
while [ "$d" != "/" ] && [ ! -e "$d/.git" ]; do d=$(dirname "$d"); done
if [ ! -e "$d/.git" ]; then echo "cwd=$cwd"; echo "root="; exit 0; fi
echo "cwd=$cwd"; echo "root=$d"
if [ -d "$d/.git" ]; then echo "git_is_dir=1"; else echo "git_is_dir=0"; fi
if [ -f "$d/.git/config" ]; then echo "config_b64=$(base64 < "$d/.git/config" | tr -d '\n')"; fi
[ -e "$d/.git/shallow" ] && echo "shallow=1"
[ -e "$d/.git/objects/info/alternates" ] && echo "alternates=1"
if command -v git >/dev/null 2>&1; then
  echo "git=1"
  n=$(git -C "$d" --no-optional-locks rev-list --first-parent --count HEAD 2>/dev/null) && echo "first_parent_count=$n"
  r=$(git -C "$d" --no-optional-locks rev-list --first-parent --max-parents=0 HEAD 2>/dev/null | tail -n 1) && echo "root_commit=$r"
  h=$(git -C "$d" --no-optional-locks rev-parse --verify -q HEAD 2>/dev/null) && echo "head=$h"
fi
"""


def parse_probe(text: str) -> dict:
    out = {}
    for line in (text or "").splitlines():
        k, sep, v = line.partition("=")
        if sep and k.strip():
            out[k.strip()] = v.strip()
    if out.get("config_b64"):
        try:
            out["git_config"] = base64.b64decode(out.pop("config_b64")).decode("utf-8", "replace")
        except ValueError:
            out.pop("config_b64", None)
    return out


def _trim_end(s: str, suffix: str) -> str:
    """Rust `trim_end_matches`: removes every trailing repeat."""
    while suffix and s.endswith(suffix):
        s = s[: -len(suffix)]
    return s


def normalise_origin(url: str) -> str:
    """repo.rs `normalise_origin`."""
    u = _trim_end(_trim_end(url, ".git"), "/")
    if u.startswith("git@"):
        u = u[4:].replace(":", "/", 1)
    u = u.split("://")[-1]
    return u.lower()


def origin_from_config(cfg: str) -> str | None:
    """repo.rs `origin_key`: the first `url =` line in the `[remote "origin"]` section."""
    in_origin = False
    for line in cfg.splitlines():
        s = line.strip()
        if s.startswith("["):
            in_origin = s == '[remote "origin"]'
        elif in_origin and s.startswith("url ="):
            return normalise_origin(s[len("url ="):].strip())
    return None


def short_hash(s: str) -> str:
    """repo.rs `short_hash`: the first 6 bytes of SHA-256, as hex."""
    return hashlib.sha256(s.encode()).hexdigest()[:12]


def candidate_keys(probe: dict) -> list[str]:
    """Every key a hook could look routes up under in the probed checkout, most likely first.

    With an origin, the hook uses only the normalised origin URL. Without one it uses
    `<dir>@<root commit[:12]>` when adage's own object reader can find the root, else
    `<dir>@path-<hash>`. The probe reads the root with git, which can succeed where adage's reader
    gives up (a pack format it does not follow, a missing git), so both no-origin forms are listed.
    """
    root = probe.get("root") or ""
    if not root:
        return []
    origin = origin_from_config(probe.get("git_config") or "") if probe.get("git_is_dir") == "1" else None
    if origin:
        return [origin]
    name = PurePosixPath(root).name
    keys = []
    rc = probe.get("root_commit") or ""
    readable = (probe.get("git_is_dir") == "1" and not probe.get("shallow") and not probe.get("alternates")
                and re.fullmatch(r"[0-9a-f]{40}", rc) is not None
                and int(probe.get("first_parent_count") or 0) <= MAX_WALK)
    if readable:
        keys.append(f"{name}@{rc[:12]}")
    keys.append(f"{name}@path-{short_hash(root)}")
    return keys


def rekey_index(index: dict, source_key: str | None, targets: list[str]) -> dict:
    """The index with `source_key`'s routes installed under each target key, and nothing else.

    `source_key` None means: the index must hold exactly one repository, and that one is used.
    `generation` is kept (it names the learned inputs, which are unchanged). Per-repository
    `triggers` are copied the same way; an index without them is fine (derived on load)."""
    routes = index.get("routes") or []
    repos = sorted({r.get("repo") for r in routes})
    if source_key is None:
        if len(repos) != 1:
            raise ValueError(f"index holds {len(repos)} repositories {repos}; pass adage_repo_key")
        source_key = repos[0]
    chosen = [r for r in routes if r.get("repo") == source_key]
    if not chosen:
        raise ValueError(f"index holds no routes for {source_key!r} (it has {repos})")
    out = {"generation": index.get("generation", ""), "routes": []}
    trig = (index.get("triggers") or {}).get(source_key)
    for key in dict.fromkeys(targets):
        for r in chosen:
            rr = copy.deepcopy(r)
            rr["repo"] = key
            out["routes"].append(rr)
        if trig is not None:
            out.setdefault("triggers", {})[key] = copy.deepcopy(trig)
    return out


def agents_md_text(index: dict, source_key: str | None = None, limit: int = 40) -> str:
    """The `agents-md` baseline: the learned routes' summaries as a short conventions note."""
    routes = [r for r in index.get("routes") or [] if source_key is None or r.get("repo") == source_key]
    lines = []
    for r in routes[:limit]:
        s = (r.get("route") or {}).get("summary") or ""
        if s:
            lines.append(f"- {s}")
    if not lines:
        return ""
    return "Repository conventions observed in earlier sessions:\n" + "\n".join(lines) + "\n"


# ------------------------------------------------------------------------ frozen ADAGE_HOME

def read_home_archive(path: str | Path) -> dict[str, bytes]:
    """Members of a frozen ADAGE_HOME tarball (or directory): relative path -> bytes. Refuses
    absolute or escaping paths and links."""
    p = Path(path)
    files: dict[str, bytes] = {}
    if p.is_dir():
        for f in sorted(p.rglob("*")):
            if f.is_file() and not f.is_symlink():
                files[f.relative_to(p).as_posix()] = f.read_bytes()
        return files
    with tarfile.open(p, "r:*") as tf:
        for m in tf.getmembers():
            if m.name.startswith("/") or ".." in PurePosixPath(m.name).parts:
                raise ValueError(f"unsafe member {m.name!r} in {p}")
            name = PurePosixPath(*[x for x in PurePosixPath(m.name).parts if x != "."]).as_posix() \
                if PurePosixPath(m.name).parts else ""
            if not name or name == ".":
                continue
            if m.issym() or m.islnk():
                raise ValueError(f"link member {m.name!r} in {p}")
            if m.isfile():
                files[name] = tf.extractfile(m).read()
    return files


def prepare_home(files: dict[str, bytes], source_key: str | None, targets: list[str], dest: Path) -> dict:
    """Write the ADAGE_HOME to upload into `dest`: every archived file except session state and
    caches, with index.json re-keyed. Returns a summary for setup.json."""
    if "index.json" not in files:
        raise ValueError("frozen ADAGE_HOME has no index.json")
    index = json.loads(files["index.json"])
    rekeyed = rekey_index(index, source_key, targets)
    skip = ("sessions/", "cache/", "learn-queue/", "repo-keys/")
    for name, data in files.items():
        if name == "index.json" or name.startswith(skip) or name.endswith((".lock", "learn.log")):
            continue
        out = dest / name
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(data)
    (dest / "index.json").write_text(json.dumps(rekeyed, indent=1))
    keys = list(dict.fromkeys(targets))
    return {"generation": rekeyed["generation"], "source_key": source_key or index["routes"][0]["repo"],
            "keys": keys, "routes": len(rekeyed["routes"]) // len(keys),
            "index_sha256": hashlib.sha256(files["index.json"]).hexdigest()}


def capture_command(root: str, out_dir: str) -> str:
    """Record the worktree's state without writing into it: porcelain status (untracked included)
    and the diff against HEAD, both with --no-optional-locks."""
    r, o = shlex.quote(root), shlex.quote(out_dir)
    return (f"mkdir -p {o} && cd {r} && "
            f"git --no-optional-locks status --porcelain=v1 --untracked-files=all > {o}/status.txt 2>&1; "
            f"git --no-optional-locks diff HEAD --binary > {o}/patch.diff 2>/dev/null; "
            f"git --no-optional-locks ls-files --others --exclude-standard > {o}/untracked.txt 2>/dev/null; true")


# ------------------------------------------------------------------------------ Harbor agent

try:  # Harbor is optional: tests and the planner need only the helpers above.
    from pydantic import Field
    from harbor.agents.installed.claude_code import ClaudeCode, ClaudeCodeOptions
    from harbor.environments.base import BaseEnvironment
    from harbor.models.agent.context import AgentContext
    HARBOR = True
except ImportError:  # pragma: no cover - exercised only where Harbor is installed
    HARBOR = False


if HARBOR:  # pragma: no cover
    class AdageClaudeCodeOptions(ClaudeCodeOptions):
        arm: str = Field(default="off", description="off | holdout | adage | agents-md")
        adage_bin: str | None = Field(default=None, description="Host path of a static linux adage binary.")
        adage_home: str | None = Field(default=None, description="Host path of the frozen ADAGE_HOME (.tgz or dir).")
        adage_repo_key: str | None = Field(default=None, description="Key the index was learned under.")
        agents_md: str | None = Field(default=None, description="Host path of the agents-md text (else derived).")
        arm_label: str | None = Field(default=None, description="Name recorded for this arm (e.g. adage-lro).")
        disable_web_search: bool = Field(default=True, description="Disallow WebSearch and WebFetch.")

    class AdageClaudeCode(ClaudeCode):
        options_model = AdageClaudeCodeOptions
        options: AdageClaudeCodeOptions

        def __init__(self, logs_dir: Path, memory_dir: str | None = None, *args, **kwargs):
            if memory_dir or kwargs.get("memory_dir"):
                raise ValueError("AdageClaudeCode refuses memory_dir: every arm starts with no memory")
            arm = check_arm(str(kwargs.get("arm", "off")))
            if arm == "agents-md" and not kwargs.get("append_system_prompt"):
                kwargs["append_system_prompt"] = self._agents_md(kwargs)
            super().__init__(logs_dir, None, *args, **kwargs)
            o = self.options
            if arm in HOOK_ARMS and not (o.adage_bin and o.adage_home):
                raise ValueError(f"arm {arm} needs adage_bin and adage_home")
            self._arm = arm
            self._remote_home = (self.environment_logs_dir / "adage-home").as_posix()
            self._remote_log = (self.environment_logs_dir / ADAGE_LOG_DIR).as_posix()
            self._probe: dict = {}
            # ClaudeCode.run uploads `_base_settings` whenever `config_source` is not None.
            self._base_settings = settings_for_arm(arm, self._base_settings)
            if self._config is None:
                self._config = self._base_settings
            self._resolved_env_vars.update(arm_env(arm, self._remote_home))

        @staticmethod
        def _agents_md(kwargs) -> str:
            if kwargs.get("agents_md"):
                return Path(kwargs["agents_md"]).read_text()
            if not kwargs.get("adage_home"):
                raise ValueError("arm agents-md needs agents_md or adage_home")
            files = read_home_archive(kwargs["adage_home"])
            text = agents_md_text(json.loads(files["index.json"]), kwargs.get("adage_repo_key"))
            if not text:
                raise ValueError("arm agents-md: the index has no route summaries for this repository")
            return text

        def _write_log(self, name: str, data: Any) -> None:
            d = self.logs_dir / ADAGE_LOG_DIR
            d.mkdir(parents=True, exist_ok=True)
            (d / name).write_text(json.dumps(data, indent=2) if not isinstance(data, str) else data)

        async def setup(self, environment: BaseEnvironment) -> None:
            await super().setup(environment)
            res = await environment.exec(command=PROBE_SCRIPT)
            self._probe = parse_probe(res.stdout or "")
            info = {"arm": self._arm, "arm_label": self.options.arm_label, "probe": {k: v for k, v in self._probe.items() if k != "git_config"},
                    "claude_code_version": self._version}
            if self._arm in HOOK_ARMS:
                keys = candidate_keys(self._probe)
                if not keys:
                    raise RuntimeError(f"no git repository at or above the working directory {self._probe.get('cwd')}")
                with tempfile.TemporaryDirectory(prefix="adage-home-") as tmp:
                    home = Path(tmp)
                    info["home"] = prepare_home(read_home_archive(self.options.adage_home),
                                                self.options.adage_repo_key, keys, home)
                    await environment.upload_dir(home, self._remote_home)
                await environment.upload_file(self.options.adage_bin, REMOTE_BIN.as_posix())
                user = shlex.quote(str(environment.default_user)) if environment.default_user is not None else None
                own = f" && chown -R {user} {shlex.quote(self._remote_home)}" if user else ""
                await self.exec_as_root(environment, command=(
                    f"chmod 755 {REMOTE_BIN.as_posix()} && mkdir -p {shlex.quote(self._remote_home)}/sessions"
                    f"{own} && chmod -R a+rwX {shlex.quote(self._remote_home)}"))
                ver = await environment.exec(command=f"{REMOTE_BIN.as_posix()} version")
                info["adage_version"] = (ver.stdout or "").strip()
            self._write_log("setup.json", info)

        async def run(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
            try:
                await super().run(instruction, environment, context)
            finally:
                root = self._probe.get("root")
                if root:
                    try:
                        await environment.exec(command=capture_command(root, self._remote_log))
                    except Exception as exc:  # noqa: BLE001 - measurement only
                        self._write_log("capture-error.txt", repr(exc))

    class AdageCodex:
        """TODO. Codex differs from Claude Code in ways that change the wiring, not the protocol:
        hooks are registered in `$CODEX_HOME/hooks.json` (adage install codex; harness/codex.rs),
        as a shell-string command `<bin> hook codex <Event>`, and Codex asks a person to approve new
        hooks (`/hooks`) unless trust is pre-seeded, which a headless trial cannot do; Harbor's
        codex agent (installed/codex.py) owns CODEX_HOME, so the hooks file must be written there,
        never into <repo>/.codex (that would dirty the patch). Transcripts are rollouts under
        $CODEX_HOME/sessions (learn with --codex-sessions). Stop re-enters as a user message."""

    class AdageOpenCode:
        """TODO. OpenCode has no command hooks: Adage ships an in-process JS plugin
        (harness/opencode.rs `plugin_source`) that spawns `adage hook opencode <event>`. It must be
        installed at $XDG_CONFIG_HOME/opencode/plugins/adage.js (never <repo>/.opencode), and
        directives arrive as a context-only synthetic message (`noReply: true`), not
        additionalContext, so delivery differs from Claude Code's. Transcripts are OpenCode's own
        storage, which `adage learn` does not import yet."""
