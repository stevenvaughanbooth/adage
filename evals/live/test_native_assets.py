"""Free native input-closure checks; no binaries or model harnesses are executed."""
import hashlib
import os
from pathlib import Path
import tempfile
import unittest
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import native_assets


class NativeAssets(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.plugin = self.root / 'plugin'
        self.home = self.root / 'initial-home'
        self.home.mkdir()
        self.policy = self.root / 'managed-policy.toml'
        self.adage = self.plugin / 'dist/test-target/adage'
        self.executor = self.plugin / 'dist/test-target/adage-exec'
        for rel in ['.claude-plugin/plugin.json', 'hooks/hooks.json', 'hooks/register.ts', 'scripts/session-start']:
            p = self.plugin / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text('fixture\n')
        (self.plugin / 'scripts/session-start').chmod(0o700)
        self.adage.parent.mkdir(parents=True)
        for p in [self.adage, self.executor]:
            p.write_text(p.name)
            p.chmod(0o700)
        self.sums = self.plugin / 'dist/SHA256SUMS'
        self.sums.write_text(''.join(hashlib.sha256(p.read_bytes()).hexdigest() + '  test-target/' + p.name + '\n'
                                     for p in [self.adage, self.executor]))
        (self.home / 'index.json').write_text('fixture index')

    def capture(self):
        return native_assets.capture(self.plugin, self.home, self.policy, self.adage, self.executor)

    def test_identity_is_repeatable_and_contains_no_file_contents(self):
        snapshot = self.capture()
        self.assertEqual(snapshot, self.capture())
        self.assertEqual(snapshot, native_assets.verify(snapshot))
        self.assertEqual(snapshot['scope'], 'pre_session_identity_only')
        self.assertEqual(snapshot['managed_policy']['entry'], None)
        self.assertEqual(set(snapshot['initial_home']['entries']['index.json']), {'kind', 'mode', 'size', 'sha256'})

    def test_added_runtime_authority_and_settings_are_detected(self):
        for name in ['config.toml', 'approvals.json', 'policy.toml', 'exec/enabled', 'exec/predict-off',
                     'exec/rewrite-off', 'models/model', 'tracks/track', 'packs/pack']:
            with self.subTest(path=name):
                snapshot = self.capture()
                p = self.home / name
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_text('changed')
                with self.assertRaisesRegex(ValueError, 'differ'):
                    native_assets.verify(snapshot)
                p.unlink()

    def test_existing_home_bytes_modes_and_removal_are_detected(self):
        p = self.home / 'index.json'
        for mutation in ['bytes', 'mode', 'remove']:
            with self.subTest(mutation=mutation):
                p.write_text('original')
                p.chmod(0o600)
                snapshot = self.capture()
                if mutation == 'bytes':
                    p.write_text('altered!')
                elif mutation == 'mode':
                    p.chmod(0o700)
                else:
                    p.unlink()
                with self.assertRaisesRegex(ValueError, 'differ'):
                    native_assets.verify(snapshot)

    def test_managed_policy_presence_bytes_and_removal_are_detected(self):
        for mutation in ['add', 'change', 'remove']:
            with self.subTest(mutation=mutation):
                self.policy.unlink(missing_ok=True)
                if mutation != 'add':
                    self.policy.write_text('old')
                snapshot = self.capture()
                if mutation == 'remove':
                    self.policy.unlink()
                else:
                    self.policy.write_text('new')
                with self.assertRaisesRegex(ValueError, 'differ'):
                    native_assets.verify(snapshot)

    def test_plugin_module_addition_deletion_and_modes_are_detected(self):
        for mutation in ['change', 'add', 'delete', 'mode']:
            with self.subTest(mutation=mutation):
                p = self.plugin / 'hooks/helper.ts'
                p.write_text('original')
                p.chmod(0o600)
                snapshot = self.capture()
                if mutation == 'change':
                    p.write_text('altered!')
                elif mutation == 'add':
                    (self.plugin / 'hooks/extra.ts').write_text('extra')
                elif mutation == 'delete':
                    p.unlink()
                else:
                    p.chmod(0o700)
                with self.assertRaisesRegex(ValueError, 'differ'):
                    native_assets.verify(snapshot)
                (self.plugin / 'hooks/extra.ts').unlink(missing_ok=True)

    def test_binary_drift_and_fallback_are_refused(self):
        self.adage.write_text('different')
        with self.assertRaisesRegex(ValueError, 'checksums'):
            self.capture()
        with self.assertRaisesRegex(ValueError, 'staged inside'):
            native_assets.plugin_identity(self.plugin, self.root / 'adage', self.executor)

    def test_duplicate_checksum_is_refused(self):
        self.sums.write_text(self.sums.read_text() + self.sums.read_text().splitlines()[0] + '\n')
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            self.capture()

    @unittest.skipUnless(os.name == 'posix', 'links and FIFO inputs require POSIX')
    def test_links_and_special_files_are_refused_without_opening(self):
        p = self.home / 'bad'
        p.symlink_to(self.root / 'missing')
        with self.assertRaisesRegex(ValueError, 'linked or nonregular'):
            self.capture()
        p.unlink()
        os.mkfifo(p)
        with self.assertRaisesRegex(ValueError, 'linked or nonregular'):
            self.capture()


if __name__ == '__main__':
    unittest.main()
