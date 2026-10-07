"""Native Claude mod observations for a binary-default, E0-only frozen treatment.

No approvals, model dispatch or source changes. Wiring these checks into a collector is required
before its receipts can claim the native profile. Spool counters describe recorded behavior;
independent grades and observed stream delivery remain separate authorities.
"""
from __future__ import annotations

import json
import os
import hashlib
import platform
import shutil
from pathlib import Path
import re
import sys
import zlib

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent / 'demo'))
import adage_metrics
import native_assets

PROFILE = 'native-mod-bundled-defaults-e0/v1'
HARNESS_VERSION = '2.1.288'
BUILTIN_PLUGINS = [
    {'name': name, 'path': 'builtin', 'source': name + '@builtin'}
    for name in ['cc-plugin-agents-md', 'cc-plugin-telemetry', 'cc-plugin-plugin-authoring']
]
MANAGED_POLICY = Path('/etc/adage/policy.toml')
MANAGED_HARNESS = Path('/etc/claude-code')
STATIC_ENV = ('PATH', 'LANG', 'LANGUAGE', 'LC_ALL', 'LC_CTYPE', 'TZ', 'SHELL',
              'ANTHROPIC_BASE_URL', 'CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST',
              'HTTPS_PROXY', 'HTTP_PROXY', 'NO_PROXY', 'https_proxy', 'http_proxy', 'no_proxy')
MUTABLE_HOME = {'sessions', 'hook-receipts', 'repo-keys', 'cache', 'ledger', 'fail-open.log'}
MAX_EVIDENCE = 64 * 1024 * 1024


def check_version(version):
    if not isinstance(version, str) or version.split(" ", 1)[0] != HARNESS_VERSION:
        raise ValueError('native frozen profile requires the verified Claude Code version')


def harness_identity():
    command = shutil.which('claude')
    if command is None:
        raise ValueError('native frozen profile requires its standalone Claude Code executable')
    binary = Path(command).resolve(strict=True)
    identity = native_assets.entry(binary)
    with binary.open('rb') as f:
        magic = f.read(4)
    if magic != b'\x7fELF' or not identity['mode'] & 0o111:
        raise ValueError('native frozen profile supports a pinned standalone Linux harness, not script dependencies')
    return {'command_path': str(Path(command).absolute()), 'resolved_path': str(binary), 'entry': identity}


def static_inputs(cfg, adage):
    if sys.platform != 'linux':
        raise ValueError('native frozen inputs require the Linux execution host')
    plugin = Path(cfg['plugin_dir'])
    arch = {'aarch64': 'aarch64', 'arm64': 'aarch64', 'x86_64': 'x86_64'}.get(platform.machine())
    if arch is None:
        raise ValueError('unsupported native Linux architecture')
    target = arch + '-unknown-linux-musl'
    if not (plugin / 'dist' / target / 'adage').is_file():
        target = arch + '-unknown-linux-gnu'
    if Path(adage).absolute() != (plugin / 'dist' / target / 'adage').absolute():
        raise ValueError('native collector binary differs from the shim-selected platform binary')
    executor = Path(cfg.get('adage_exec') or Path(adage).parent / 'adage-exec')
    harness = harness_identity()
    if cfg.get('harness_binary') != harness['resolved_path']:
        raise ValueError('native dispatch must use the pinned standalone harness binary')
    if any(parent.is_symlink() for parent in MANAGED_POLICY.parents):
        raise ValueError('native managed policy must not resolve through linked directories')
    managed = (native_assets.tree(MANAGED_HARNESS)
               if MANAGED_HARNESS.exists() or MANAGED_HARNESS.is_symlink() else None)
    # Never retain credentials or literal proxy URLs in the immutable metadata.
    environment = json.dumps({key: os.environ.get(key) for key in STATIC_ENV}, sort_keys=True).encode()
    return {'plugin': native_assets.plugin_identity(plugin, adage, executor), 'harness': harness,
            'managed_harness': {'path': str(MANAGED_HARNESS), 'entries': managed},
            'environment_sha256': hashlib.sha256(environment).hexdigest(),
            'managed_policy': {'path': str(MANAGED_POLICY), 'entry': native_assets.optional_file(MANAGED_POLICY)}}


def check_home(home, *, initial=False, intervention=False):
    entries = native_assets.tree(home)
    allowed = {'index.json'} if intervention else set()
    if not initial:
        allowed |= MUTABLE_HOME
    if any(key != '.' and key.split('/')[0] not in allowed for key in entries):
        raise ValueError('native E0 home contains settings, grants, models, learning assets or unknown state')
    if intervention and entries.get('index.json', {}).get('kind') != 'file':
        raise ValueError('native intervention requires its frozen index')


def check_checkout(checkout):
    root = Path(checkout)
    if (root / '.adage').is_symlink() or (root / '.adage/disabled').exists() or (root / '.adage/disabled').is_symlink():
        raise ValueError('native treatment is disabled or has linked local runtime settings')


def bounded(path):
    with Path(path).open('rb') as f:
        body = f.read(MAX_EVIDENCE + 1)
    if len(body) > MAX_EVIDENCE:
        raise ValueError('native observation exceeds evidence size limit')
    return body


def spool_records(body):
    """Unlike dashboard metrics, frozen evidence cannot silently skip corrupt frames."""
    records, offset = [], 0
    while offset < len(body):
        if body[offset:offset + 4] != adage_metrics.SPOOL_MAGIC or offset + 12 > len(body):
            raise ValueError('corrupt native session spool')
        length = int.from_bytes(body[offset + 4:offset + 8], 'little')
        checksum = int.from_bytes(body[offset + 8:offset + 12], 'little')
        end = offset + 12 + length
        if length > adage_metrics.SPOOL_MAX_RECORD or end > len(body):
            raise ValueError('truncated or oversized native session spool')
        payload = body[offset + 12:end]
        if zlib.crc32(payload) != checksum:
            raise ValueError('native session spool checksum mismatch')
        record = json.loads(payload)
        if not isinstance(record, dict):
            raise ValueError('invalid native session spool record')
        records.append(record)
        offset = end
    return records


def resolved_binaries(config_dir, plugin):
    config_dir = Path(config_dir)
    data = config_dir / 'plugins/data/adage-inline'
    if any(path.is_symlink() for path in (data, *data.parents)):
        raise ValueError('native plugin data must not use linked directories')
    result = {}
    for name in ['adage', 'adage-exec', 'hook']:
        link = data / 'bin' / name
        resolved = link.resolve(strict=True)
        if not resolved.is_relative_to(data.resolve()):
            raise ValueError('native shim selected a binary outside the fresh plugin data directory')
        expected_name = 'adage' if name == 'hook' else name
        expected = plugin['entries']['dist/' + plugin['binaries'][expected_name]]
        actual = native_assets.entry(resolved)
        if actual.get('sha256') != expected['sha256'] or not actual['mode'] & 0o111:
            raise ValueError('native shim selected an unpinned or nonexecutable binary')
        result[name] = {'path': str(resolved), 'sha256': actual['sha256']}
    return result


def history_for_session(root, sid):
    if not isinstance(sid, str) or not re.fullmatch(r'[A-Za-z0-9_-]+', sid):
        raise ValueError('invalid native session identity')
    histories = list(Path(root).glob('*/' + sid + '.jsonl'))
    if len(histories) != 1:
        raise ValueError('native session history is missing or duplicated')
    return histories[0]


def observe(*, init, accounting, debug_path, history_path, plugin, config_dir, home, intervention):
    """Cross-check canonical native usage, history IDs, mod activation and resolved binaries.

    Expected init comes from the parent-captured stream. Debug client request IDs are distinct
    from server request IDs: reconcile counts there, and exact IDs against native history.
    Any unexplained or retry request surplus is incomplete for this frozen profile.
    """
    if not accounting.complete:
        raise ValueError('native request accounting is incomplete')
    if init.get('claude_code_version') != HARNESS_VERSION:
        raise ValueError('native observation profile requires its verified Claude Code version')
    expected_plugins = ([{'name': 'adage', 'path': plugin['path'], 'source': 'adage@inline'}]
                        if intervention else []) + BUILTIN_PLUGINS
    actual_plugins = init.get('plugins')
    if not isinstance(actual_plugins, list):
        raise ValueError('native plugin activation list is missing')
    actual = [{key: p.get(key) for key in ('name', 'path', 'source')}
              for p in actual_plugins if isinstance(p, dict)]
    if len(actual) != len(actual_plugins) or actual != expected_plugins:
        raise ValueError('actual plugins differ from the native study arm')
    sid = init.get('session_id')
    if not isinstance(sid, str) or not re.fullmatch(r'[A-Za-z0-9_-]+', sid):
        raise ValueError('native session identity is missing or invalid')
    history_ids = set()
    history_body = bounded(history_path)
    for line in history_body.splitlines():
        row = json.loads(line)
        if not isinstance(row, dict):
            raise ValueError('invalid native history record')
        if row.get('type') != 'assistant':
            continue
        request = row.get('requestId')
        if not isinstance(request, str) or not request:
            raise ValueError('native history assistant request ID is missing')
        if row.get('sessionId') != sid:
            raise ValueError('native history belongs to another session')
        history_ids.add(request)
    stream_ids = {c['id'].removeprefix('anthropic/') for c in accounting.calls}
    if not stream_ids or history_ids != stream_ids:
        raise ValueError('native history and captured stream request IDs differ')
    debug_body = bounded(debug_path)
    debug = debug_body.decode('utf-8')
    requests = adage_metrics.requests(debug, [{'value': {'request_id': rid}} for rid in sorted(stream_ids)])
    if not requests['verified'] or not requests['cross_check']:
        raise ValueError('native debug dispatch count does not reconcile with complete request usage')
    loaded = len(re.findall(r'^\S+ \[DEBUG\] hooks module adage@inline loaded ', debug, re.M))
    steps = len(re.findall(r'^\S+ \[DEBUG\] hooks module adage@inline turn\.step stream settled ', debug, re.M))
    if loaded != int(intervention) or intervention and not steps or not intervention and steps:
        raise ValueError('native module activation or step observations are missing or duplicated')
    result = {'profile': PROFILE, 'requests': requests, 'history_matches_stream': True,
              'native_module_loads': loaded, 'native_step_observations': steps, 'plugins': actual_plugins,
              'delivery_scope': 'spool records; inspect captured hook output and prompt evidence separately',
              'billed_cost_usd': None,
              'source_sha256': {'debug': hashlib.sha256(debug_body).hexdigest(),
                                'history': hashlib.sha256(history_body).hexdigest()}}
    if intervention:
        result['resolved_binaries'] = resolved_binaries(config_dir, plugin)
        spool_body = bounded(Path(home) / 'sessions' / (sid + '.spool'))
        result['source_sha256']['spool'] = hashlib.sha256(spool_body).hexdigest()
        records = spool_records(spool_body)
        metrics = adage_metrics.spool_metrics(records)
        if metrics['session_starts'] != 1:
            raise ValueError('native session start was not recorded exactly once')
        if metrics['predicted_steps'] or metrics['receipts_as_results'] or metrics['dead_end_substituted']:
            raise ValueError('unexpected execution or prediction in the frozen E0 treatment')
        result['recorded_behavior'] = metrics
    return result
