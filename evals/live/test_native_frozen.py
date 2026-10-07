"""Model-free native-profile integration through the real Rust frozen contract.

The fake harness emits labeled fixture histories/debug events; it never runs a model or claims
measured savings. Existing fixture setup simulates operator context only within these tests.
"""
import copy
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import evidence
import frozen
import native_runtime
import runner
import test_frozen
import test_pilot
import pilot

VERSION = native_runtime.HARNESS_VERSION + ' (Claude Code)'


class NativeFakeLive(test_frozen.FakeLive):
    fault = None

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.state['harness_version'] = VERSION
        self.transcripts = self.root / 'private/transcripts'
        original = self.observer.event

        def event(row):
            if row.get('type') == 'system' and row.get('subtype') == 'init':
                row['claude_code_version'] = native_runtime.HARNESS_VERSION
                row['plugins'] = copy.deepcopy(native_runtime.BUILTIN_PLUGINS)
                if self.observer.job['side'] == 'intervention':
                    row['plugins'].insert(0, {'name': 'adage', 'path': self.cfg['plugin_dir'], 'source': 'adage@inline'})
            original(row)
        self.observer.event = event

    def run_one(self, run_id, phase, arm, task, prompt):
        super().run_one(run_id, phase, arm, task, prompt)
        directory = self.root / 'runs' / run_id
        accounting = evidence.read_stream(directory / 'stream.jsonl')
        history = self.transcripts / 'fixture' / (run_id + '.jsonl')
        history.parent.mkdir(parents=True, exist_ok=True)
        history.write_text(''.join(json.dumps({'type': 'assistant', 'requestId': c['id'].removeprefix('anthropic/'),
                                              'sessionId': run_id}) + '\n' for c in accounting.calls))
        log = ''.join(f't [DEBUG] [API REQUEST] /v1/messages x-client-request-id=client-{i} source=main\n'
                      for i, _ in enumerate(accounting.calls))
        if arm == 'mod':
            if self.fault != 'missing-module':
                log += 't [DEBUG] hooks module adage@inline loaded (fixture)\n'
            log += 't [DEBUG] hooks module adage@inline turn.step stream settled (fixture)\n'
            data = directory / 'home/.claude/plugins/data/adage-inline/bin'
            data.mkdir(parents=True)
            for name in ['adage', 'adage-exec']:
                path = Path(self.cfg['plugin_dir']) / 'dist' / self.observer.spec['native_inputs']['plugin']['binaries'][name]
                sha = frozen.file_digest(path)
                dest = data / (name + '-' + sha)
                shutil.copy2(path, dest)
                (data / name).symlink_to(dest.name)
            (data / 'hook').symlink_to('adage')
            spool = self.home / 'sessions' / (run_id + '.spool')
            spool.parent.mkdir()
            spool.write_bytes(native_runtime.adage_metrics.frame({'kind': 'session_start'}))
            if self.fault == 'corrupt-spool':
                spool.write_bytes(spool.read_bytes() + b'corruption')
        (directory / 'claude-debug.log').write_text(log)


@unittest.skipUnless(sys.platform == 'linux', 'native frozen integration requires Linux')
class NativeFrozenFlow(unittest.TestCase):
    def setUp(self):
        self.fixture = test_frozen.FrozenFlow()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        f = self.fixture
        probe = patch.object(runner, 'harness_version', return_value=VERSION)
        probe.start()
        self.addCleanup(probe.stop)
        self.plugin = f.root / 'plugin'
        target = {'aarch64': 'aarch64', 'arm64': 'aarch64', 'x86_64': 'x86_64'}[platform.machine()] + '-unknown-linux-gnu'
        bins = self.plugin / 'dist' / target
        bins.mkdir(parents=True)
        self.adage = bins / 'adage'
        executor = test_frozen.ADAGE.parent / 'adage-exec'
        self.assertTrue(executor.is_file(), 'build adage-exec before native integration checks')
        for source in [test_frozen.ADAGE, executor]:
            shutil.copy2(source, bins / source.name)
        sums = ''.join(frozen.file_digest(bins / name) + '  ' + target + '/' + name + '\n'
                       for name in ['adage', 'adage-exec'])
        (self.plugin / 'dist/SHA256SUMS').write_text(sums)
        for rel in ['.claude-plugin/plugin.json', 'hooks/hooks.json', 'hooks/register.ts', 'scripts/session-start']:
            path = self.plugin / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('fixture - not executed')
        (self.plugin / 'scripts/session-start').chmod(0o700)
        config = frozen.read_json(f.config)
        config['eval']['arms'] = ['off', 'mod']
        config.update(plugin_dir=str(self.plugin), capture_debug=True)
        self.config = f.root / 'native-config.json'
        frozen.publish(self.config, config)
        self.path = f.root / 'native-frozen.json'
        self.frozen = frozen.prepare(self.config, f.audit, f.index, f.objects, self.path, self.adage,
                                     [f.generator, f.grader], learner_revision='synthetic-free-fixture',
                                     runtime_profile=native_runtime.PROFILE,
                                     policy={'min_groups': 2, 'max_quality_loss': 1,
                                             'min_savings': 0.2, 'min_baseline_success': 0.8})
        self.root = f.root / 'native-run'
        NativeFakeLive.runs, NativeFakeLive.fault = [], None

    def execute(self):
        return frozen.execute(self.path, self.frozen, self.fixture.objects, self.root,
                              self.adage, live_factory=NativeFakeLive)

    def test_native_pair_records_observations_and_resumes_without_new_jobs(self):
        first = self.execute()
        self.assertEqual(first.returncode, 0, first.stderr)
        report = json.loads(first.stdout)
        self.assertTrue(report['evidence_complete'])
        self.assertEqual(report['submitted_receipts'], 4)
        self.assertFalse(report['paired']['cost_target_accepted'])
        self.assertFalse(report['paired']['token_target_accepted'], 'identical fixture usage is not savings')
        for path in (self.root / 'receipts').glob('*.json'):
            receipt = self.fixture.objects.get(frozen.read_json(path)['sha256'])
            observed = self.fixture.objects.get(receipt['evidence']['native_runtime'])
            self.assertTrue(observed['history_matches_stream'])
        second = self.execute()
        self.assertEqual(second.stdout, first.stdout)
        self.assertEqual(len(NativeFakeLive.runs), 4)

    def test_runtime_environment_drift_blocks_before_any_dispatch(self):
        with patch.dict(os.environ, {'LANG': 'fixture-changed-locale'}):
            with self.assertRaisesRegex(ValueError, 'native plugin'):
                self.execute()
        self.assertEqual(NativeFakeLive.runs, [])

    def test_plugin_drift_blocks_before_any_dispatch(self):
        (self.plugin / 'hooks/register.ts').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'native plugin'):
            self.execute()
        self.assertEqual(NativeFakeLive.runs, [])

    def test_bad_native_evidence_stops_and_original_attempt_is_not_replaced(self):
        for fault in ['missing-module', 'corrupt-spool']:
            with self.subTest(fault=fault):
                self.root = self.fixture.root / fault
                NativeFakeLive.runs, NativeFakeLive.fault = [], fault
                with self.assertRaises(ValueError):
                    self.execute()
                count = len(NativeFakeLive.runs)
                self.assertLess(count, 4)
                plan = frozen.preflight(self.frozen, self.fixture.objects, self.root, self.adage)
                self.assertFalse(plan['ready_to_execute'])
                self.assertTrue(any(x['code'] == 'interrupted_jobs' for x in plan['blockers']))
                NativeFakeLive.fault = None
                with self.assertRaisesRegex(ValueError, 'started without a terminal receipt'):
                    self.execute()
                self.assertEqual(len(NativeFakeLive.runs), count)




class NativeTrainingLive(test_pilot.TrainingLive):
    """Untreated fixture only: native history/debug accounting, no model calls."""
    fault = None

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.home = self.root / 'private/adage-home'
        self.state['harness_version'] = VERSION
        original = self.observer.event

        def event(row):
            if row.get('type') == 'system' and row.get('subtype') == 'init':
                row['claude_code_version'] = native_runtime.HARNESS_VERSION
                row['plugins'] = copy.deepcopy(native_runtime.BUILTIN_PLUGINS)
            original(row)
        self.observer.event = event

    def run_one(self, run_id, phase, arm, task, prompt):
        super().run_one(run_id, phase, arm, task, prompt)
        directory = self.root / 'runs' / run_id
        accounting = evidence.read_stream(directory / 'stream.jsonl')
        history = self.root / 'private/transcripts/project' / (run_id + '.jsonl')
        rows = [json.loads(line) for line in history.read_text().splitlines()]
        calls = [c['id'].removeprefix('anthropic/') for c in accounting.calls]
        assistants = [row for row in rows if row['type'] == 'assistant']
        for i, row in enumerate(assistants):
            row['requestId'] = calls[i % len(calls)]
        for request in set(calls) - {row['requestId'] for row in assistants}:
            rows.append({'type': 'assistant', 'sessionId': run_id, 'requestId': request,
                         'message': {'role': 'assistant', 'content': []}})
        history.write_text(''.join(json.dumps(row) + '\n' for row in rows))
        log = ''.join(f't [DEBUG] [API REQUEST] /v1/messages x-client-request-id=client-{i} source=main\n'
                      for i, _ in enumerate(accounting.calls))
        if self.fault == 'unexpected-mod':
            log += 't [DEBUG] hooks module adage@inline loaded (fixture)\n'
        (directory / 'claude-debug.log').write_text(log)


@unittest.skipUnless(sys.platform == 'linux', 'native pilot requires Linux')
class NativePilotFlow(unittest.TestCase):
    def setUp(self):
        self.fixture = NativeFrozenFlow()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        f = self.fixture
        self.root = f.fixture.root / 'native-pilot'
        self.manifest = pilot.initialize(self.root, f.adage, 'fixture-model-v1', '30', '1',
                                        runtime_profile=native_runtime.PROFILE, plugin_dir=f.plugin)
        NativeTrainingLive.runs, NativeTrainingLive.mode, NativeTrainingLive.fault = [], 'complete', None

    def test_native_pilot_pins_profile_and_rejects_asset_drift(self):
        self.assertEqual(self.manifest['runtime_profile'], native_runtime.PROFILE)
        for cfg in self.manifest['configs'].values():
            self.assertEqual(cfg['eval']['arms'], ['off', 'mod'])
            self.assertEqual(cfg['learn_args'], [])
            self.assertTrue(cfg['capture_debug'])
        pilot.load(self.root)
        (self.fixture.plugin / 'hooks/register.ts').write_text('changed fixture')
        with self.assertRaisesRegex(ValueError, 'runtime inputs changed'):
            pilot.load(self.root)

    def test_untreated_training_native_receipt_replays_and_detects_debug_drift(self):
        def once(*args, **kwargs):
            if NativeTrainingLive.runs:
                raise RuntimeError('fixture stop after one job')
            return NativeTrainingLive(*args, **kwargs)
        with self.assertRaisesRegex(RuntimeError, 'fixture stop'):
            pilot.train(self.root, execute=True, live_factory=once)
        readiness = pilot.plan(self.root, runtime=False)
        self.assertFalse(readiness['blockers'])
        self.assertEqual(readiness['stages']['train']['completed'], 1)
        run_id = NativeTrainingLive.runs[0]
        pointer = frozen.read_json(self.root / 'train/receipts' / (run_id + '.json'))
        objects = frozen.Objects(self.root / 'objects')
        receipt = objects.get(pointer['sha256'])
        observed = objects.get(receipt['evidence']['native_runtime'])
        self.assertTrue(observed['complete'])
        self.assertEqual(observed['observations']['native_module_loads'], 0)
        debug = self.root / 'train/runs' / run_id / 'claude-debug.log'
        debug.write_text(debug.read_text() + 'changed fixture\n')
        self.assertTrue(any('observations changed' in reason for reason in pilot.plan(self.root, runtime=False)['blockers']))
        with self.assertRaisesRegex(ValueError, 'not ready'):
            pilot.train(self.root, execute=True, live_factory=NativeTrainingLive)
        self.assertEqual(len(NativeTrainingLive.runs), 1)

    def test_production_learning_and_review_freeze_both_native_cohorts(self):
        NativeTrainingLive.mode = 'gold'
        collected = pilot.train(self.root, execute=True, live_factory=NativeTrainingLive)
        self.assertEqual(collected['returncode'], 0, collected)
        learned = pilot.learn(self.root)
        self.assertGreater(learned['candidate_count'], 0, learned)
        objects = frozen.Objects(self.root / 'objects')
        # Disclosed scripted fixture only; existing setup simulates an operator for tests.
        # The production coordinator never approves routes or clears agent markers.
        for route in objects.get(learned['routes_sha256']):
            if route['state'] == 'nascent':
                proc = subprocess.run([str(self.fixture.adage), 'review', 'approve', route['reference'],
                                       route['id'], '--by', 'synthetic-fixture-reviewer'],
                                      env=dict(os.environ, ADAGE_HOME=str(self.root / 'learner-home'),
                                               ADAGE_ALLOW_NONINTERACTIVE='1'), capture_output=True, text=True)
                self.assertEqual(proc.returncode, 0, proc.stderr)
        readiness = pilot.freeze(self.root, 'synthetic-fixture-reviewer')
        self.assertTrue(readiness['ready_for_paid_dispatch'], readiness)
        for stage in ['validation', 'test']:
            study = frozen.read_json(self.root / (stage + '-frozen.json'))
            for side in ['baseline', 'intervention']:
                spec = objects.get(study['protocol'][side]['configuration_sha256'])
                self.assertEqual(spec['runtime_profile'], native_runtime.PROFILE)
                self.assertEqual(spec['config']['eval']['arms'], ['off', 'mod'])
                self.assertEqual(spec['native_inputs'], self.manifest['native_inputs'])
        self.assertEqual(len(NativeTrainingLive.runs), 20)

    def test_contaminated_training_retains_receipt_and_stops(self):
        NativeTrainingLive.fault = 'unexpected-mod'
        result = pilot.train(self.root, execute=True, live_factory=NativeTrainingLive)
        self.assertEqual(result['returncode'], 2)
        self.assertEqual(len(NativeTrainingLive.runs), 1)
        self.assertEqual(len(list((self.root / 'train/receipts').glob('*.json'))), 1)
        self.assertTrue(any('incomplete native observations' in reason for reason in result['blockers']))
        with self.assertRaisesRegex(ValueError, 'not ready'):
            pilot.train(self.root, execute=True, live_factory=NativeTrainingLive)


if __name__ == '__main__':
    unittest.main()
