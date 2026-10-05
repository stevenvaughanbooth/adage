import json
from pathlib import Path
import shutil
import tempfile
import unittest

import prepare_native_probe_v3 as revision


class DiagnosticPreparationTests(unittest.TestCase):
    def original(self, root):
        old = root / "sessions/original"
        old.mkdir(parents=True)
        checkout = old / "repo"
        checkout.mkdir()
        (checkout / "native_canary.py").write_text("print('canary')\n")
        settings = old / "settings.json"
        settings.write_text('{"sandbox":{"enabled":true,"allowUnsandboxedCommands":false}}')
        (old / "run.json").write_text('{"cancelled":"Empty"}')
        (old / "stdout.jsonl").write_text("")
        manifest = old / "manifest.json"
        value = dict(kind="native-confinement",root=str(old),execution_root=str(checkout),budget_usd=.5,max_turns=5,
            canary_sha256=revision.sha(checkout / "native_canary.py"), settings_sha256=revision.sha(settings),
            argv=["claude","--settings",str(settings),"--debug-file",str(old / "debug.log"),"same prompt"],
            shared_sources={})
        manifest.write_text(json.dumps(value))
        return manifest

    def test_preserves_original_and_confinement_and_never_changes_unknown_ledger(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d).resolve()
            original = self.original(root)
            before = original.read_bytes()
            ledger = root / "paid-state.json"
            ledger.write_text('{"cost":"unknown"}')
            target = revision.prepare(original, root / "sessions/revision3")
            value = json.loads(target.read_text())
            self.assertEqual(original.read_bytes(), before)
            self.assertEqual(ledger.read_text(), '{"cost":"unknown"}')
            self.assertEqual(value["argv"][:2], ["/usr/bin/env","CLAUDE_CODE_PROFILE_STARTUP=1"])
            self.assertEqual(value["argv"][-1], "same prompt")
            self.assertEqual((target.parent / "settings.json").read_bytes(), (original.parent / "settings.json").read_bytes())
            self.assertEqual(value["paid_calls"],0)
            socket = Path(value["argv"][value["argv"].index("--messaging-socket-path")+1])
            self.assertLess(len(str(socket).encode()),100)
            self.assertEqual(socket.parent.stat().st_mode & 0o777, 0o700)
            shutil.rmtree(socket.parent)

    def test_executed_canary_or_nonempty_stream_refuses_revision(self):
        for kind in ("canary","stream"):
            with tempfile.TemporaryDirectory() as d:
                root = Path(d).resolve()
                original = self.original(root)
                path = original.parent / ("repo/inside-write-canary" if kind == "canary" else "stdout.jsonl")
                path.write_text("evidence")
                with self.assertRaises(ValueError):
                    revision.prepare(original, root / "sessions/revision3")


if __name__ == "__main__":
    unittest.main()
