"""Free native-mod runtime observation fixtures; no model or hook processes run."""
import copy
import hashlib
import json
from pathlib import Path
import shutil
import sys
import types
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import native_assets
import native_runtime
import test_native_assets
import runner
import frozen


class NativeRuntime(unittest.TestCase):
    def setUp(self):
        self.fixture = test_native_assets.NativeAssets()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        f = self.fixture
        self.plugin = native_assets.plugin_identity(f.plugin, f.adage, f.executor)
        self.config = f.root / 'claude-config'
        self.bin = self.config / 'plugins/data/adage-inline/bin'
        self.bin.mkdir(parents=True)
        for source in [f.adage, f.executor]:
            sha = hashlib.sha256(source.read_bytes()).hexdigest()
            dest = self.bin / (source.name + '-' + sha)
            shutil.copy2(source, dest)
            (self.bin / source.name).symlink_to(dest.name)
        (self.bin / 'hook').symlink_to('adage')
        self.init = {'claude_code_version': native_runtime.HARNESS_VERSION, 'session_id': 'session-1',
                     'plugins': [{'name': 'adage', 'path': self.plugin['path'], 'source': 'adage@inline'}]
                     + copy.deepcopy(native_runtime.BUILTIN_PLUGINS)}
        self.accounting = types.SimpleNamespace(complete=True, calls=[{'id': 'anthropic/server-1'}])
        self.history = f.root / 'history.jsonl'
        self.history.write_text(json.dumps({'type': 'assistant', 'requestId': 'server-1', 'sessionId': 'session-1'}) + '\n')
        self.debug = f.root / 'debug.log'
        self.dispatch = 't [DEBUG] [API REQUEST] /v1/messages x-client-request-id=client-1 source=main\n'
        self.loaded = 't [DEBUG] hooks module adage@inline loaded (fixture)\n'
        self.step = 't [DEBUG] hooks module adage@inline turn.step stream settled (fixture)\n'
        self.debug.write_text(self.dispatch + self.loaded + self.step)
        self.spool = f.home / 'sessions/session-1.spool'
        self.spool.parent.mkdir()
        self.spool.write_bytes(native_runtime.adage_metrics.frame({'kind': 'session_start'}))

    def observe(self, **overrides):
        args = {'init': self.init, 'accounting': self.accounting, 'debug_path': self.debug,
                'history_path': self.history, 'plugin': self.plugin, 'config_dir': self.config,
                'home': self.fixture.home, 'intervention': True}
        return native_runtime.observe(**(args | overrides))

    def test_active_native_module_is_distinct_from_reflex_delivery(self):
        result = self.observe()
        self.assertEqual(result['native_module_loads'], 1)
        self.assertEqual(result['native_step_observations'], 1)
        self.assertEqual(result['recorded_behavior']['directives'], 0)
        self.assertIsNone(result['billed_cost_usd'])
        self.assertEqual(set(result['resolved_binaries']), {'adage', 'adage-exec', 'hook'})

    def test_incomplete_or_extra_dispatch_accounting_stops_observation(self):
        self.accounting.complete = False
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            self.observe()
        self.accounting.complete = True
        self.debug.write_text(self.debug.read_text() + self.dispatch.replace('client-1', 'client-2')
                              + 't [ERROR] API error (attempt 1/2)\n')
        with self.assertRaisesRegex(ValueError, 'dispatch count'):
            self.observe()

    def test_history_must_match_actual_request_and_session_ids(self):
        for record in [{'type': 'assistant', 'requestId': 'server-2', 'sessionId': 'session-1'},
                       {'type': 'assistant', 'requestId': 'server-1', 'sessionId': 'other-session'},
                       {'type': 'assistant', 'sessionId': 'session-1'}]:
            with self.subTest(record=record):
                self.history.write_text(json.dumps(record) + '\n')
                with self.assertRaises(ValueError):
                    self.observe()

    def test_wrong_plugin_missing_step_and_duplicate_activation_are_refused(self):
        init = copy.deepcopy(self.init)
        init['plugins'][0]['path'] = '/wrong'
        with self.assertRaisesRegex(ValueError, 'plugins differ'):
            self.observe(init=init)
        for log in [self.dispatch + self.loaded, self.dispatch + self.loaded * 2 + self.step]:
            self.debug.write_text(log)
            with self.assertRaisesRegex(ValueError, 'activation or step'):
                self.observe()

    def test_resolved_path_cannot_fall_back_outside_fresh_plugin_data(self):
        (self.bin / 'hook').unlink()
        (self.bin / 'hook').symlink_to(self.fixture.adage)
        with self.assertRaisesRegex(ValueError, 'outside'):
            self.observe()

    def test_a_shared_plugin_data_directory_is_not_a_fresh_copy(self):
        data = self.bin.parent
        moved = self.fixture.root / 'shared-data'
        data.rename(moved)
        data.symlink_to(moved, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'linked directories'):
            self.observe()

    def test_serving_binary_bytes_cannot_change(self):
        (self.bin / 'adage').resolve().write_text('tampered')
        with self.assertRaisesRegex(ValueError, 'unpinned'):
            self.observe()

    def test_corrupt_spool_is_not_silently_skipped(self):
        original = self.spool.read_bytes()
        for data in [b'x' + original, original[:-1], original[:-1] + b'x', original + b'x']:
            self.spool.write_bytes(data)
            with self.assertRaises(ValueError):
                self.observe()

    def test_execution_prediction_is_not_e0_advice(self):
        self.spool.write_bytes(self.spool.read_bytes() + native_runtime.adage_metrics.frame({'kind': 'turn_step', 'predicted': True}))
        with self.assertRaisesRegex(ValueError, 'unexpected execution or prediction'):
            self.observe()

    def test_baseline_requires_no_native_mod(self):
        init = copy.deepcopy(self.init)
        init['plugins'] = copy.deepcopy(native_runtime.BUILTIN_PLUGINS)
        self.debug.write_text(self.dispatch)
        result = self.observe(init=init, intervention=False)
        self.assertEqual(result['native_module_loads'], 0)
        self.assertNotIn('recorded_behavior', result)
        self.debug.write_text(self.dispatch + self.loaded + self.step)
        with self.assertRaisesRegex(ValueError, 'activation or step'):
            self.observe(init=init, intervention=False)

    def test_initial_home_is_fresh_and_runtime_settings_remain_absent(self):
        home = self.fixture.home
        with self.assertRaisesRegex(ValueError, 'unknown state'):
            native_runtime.check_home(home, initial=True, intervention=True)
        native_runtime.check_home(home, intervention=True)
        for name in ['config.toml', 'approvals.json', 'exec/enabled', 'models/custom']:
            p = home / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text('altered')
            with self.assertRaisesRegex(ValueError, 'unknown state'):
                native_runtime.check_home(home, intervention=True)
            p.unlink()
            if p.parent != home:
                p.parent.rmdir()

    def test_harness_is_a_pinned_standalone_binary_and_dispatch_uses_it(self):
        binary = self.fixture.root / 'claude'
        binary.write_bytes(b'\x7fELFfixture')
        binary.chmod(0o700)
        with patch.object(native_runtime.shutil, 'which', return_value=str(binary)):
            before = native_runtime.harness_identity()
            binary.write_bytes(b'\x7fELFchanged')
            self.assertNotEqual(before, native_runtime.harness_identity())
            binary.write_text('#!/bin/sh\nexternal-dependency\n')
            with self.assertRaisesRegex(ValueError, 'script dependencies'):
                native_runtime.harness_identity()
        cfg = copy.deepcopy(runner.DEFAULTS)
        cfg['harness_binary'] = str(binary)
        self.assertEqual(runner.claude_command(cfg, 'fixture', self.fixture.root / 'settings')[0], str(binary))

    def test_native_profile_is_explicit_debugged_and_same_arm_pair(self):
        cfg = {'eval': {'arms': ['off', 'mod']}, 'capture_debug': True}
        frozen.require_profile_config(cfg, native_runtime.PROFILE)
        for arms, debug in [(['off', 'mod-observe'], True), (['mod'], True), (['off', 'mod'], False)]:
            with self.subTest(arms=arms, debug=debug), self.assertRaises(ValueError):
                frozen.require_profile_config({'eval': {'arms': arms}, 'capture_debug': debug}, native_runtime.PROFILE)
        with self.assertRaisesRegex(ValueError, 'verified Claude Code version'):
            native_runtime.check_version('different-version')

    def test_native_profile_rejects_unfrozen_forwarded_settings(self):
        cfg = {'eval': {'arms': ['off', 'mod']}, 'capture_debug': True,
               'forward_env': ['CLAUDE_CODE_SETTINGS']}
        with self.assertRaisesRegex(ValueError, 'custom forwarded settings'):
            frozen.require_profile_config(cfg, native_runtime.PROFILE)

    def test_an_unregistered_builtin_is_not_silently_accepted(self):
        init = copy.deepcopy(self.init)
        init['plugins'].append({'name': 'unplanned', 'path': 'builtin', 'source': 'unplanned@builtin'})
        with self.assertRaisesRegex(ValueError, 'plugins differ'):
            self.observe(init=init)

    def test_repository_disable_and_linked_settings_require_review(self):
        root = self.fixture.root / 'checkout'
        root.mkdir()
        native_runtime.check_checkout(root)
        (root / '.adage').mkdir()
        (root / '.adage/disabled').touch()
        with self.assertRaisesRegex(ValueError, 'disabled'):
            native_runtime.check_checkout(root)


if __name__ == '__main__':
    unittest.main()
