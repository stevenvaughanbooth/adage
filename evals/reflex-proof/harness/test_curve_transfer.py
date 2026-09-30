"""Transfer provenance and all-arm authorization preflight contracts; no paid calls."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import curve_transfer as transfer


class TransferContracts(unittest.TestCase):
    def training(self, root):
        inputs = []
        for i in range(10):
            session = root / f"session-{i}"
            session.mkdir()
            source = session / "native-transcript.jsonl"
            source.write_text(json.dumps({"sessionId": str(i)}))
            digest = transfer.pairs.sha(source)
            manifest = session / "manifest.json"
            provenance = session / "transcript-provenance.json"
            transfer.pairs.write(manifest, dict(kind=transfer.curve.GUIDED, training_hooks_enabled=False,
                workflow_guidance=transfer.curve.GUIDANCE))
            transfer.pairs.write(provenance, dict(synthetic=False, sha256=digest, session_id=str(i)))
            inputs.append(dict(copy=str(source), source=str(source), sha256=digest, session_id=str(i),
                task=f"task-{i % 5}", kind=transfer.curve.GUIDED, workflow_guidance=transfer.curve.GUIDANCE,
                hooks_enabled=False, synthetic=False, manifest_sha256=transfer.pairs.sha(manifest),
                provenance_sha256=transfer.pairs.sha(provenance)))
        record = dict(status="completed", count=10, exit_code=0, inputs=inputs)
        transfer.pairs.write(root / "attempt.json", record)
        return record

    def test_provenance_drift_rejected_even_when_transcript_bytes_match(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            record = self.training(root)
            self.assertEqual(transfer.training_inputs(root)[0]["count"], 10)
            manifest = Path(record["inputs"][0]["source"]).parent / "manifest.json"
            m = transfer.pairs.read(manifest)
            m["training_hooks_enabled"] = True
            transfer.pairs.write(manifest, m)
            # Even replacing the row's checksum cannot authorize exposed training.
            record["inputs"][0]["manifest_sha256"] = transfer.pairs.sha(manifest)
            transfer.pairs.write(root / "attempt.json", record)
            with self.assertRaisesRegex(ValueError, "provenance"):
                transfer.training_inputs(root)

    def test_duplicate_training_sessions_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            record = self.training(root)
            record["inputs"][-1] = record["inputs"][0]
            transfer.pairs.write(root / "attempt.json", record)
            with self.assertRaisesRegex(ValueError, "duplicate"):
                transfer.training_inputs(root)

    def test_missing_last_grant_prevents_spending_on_earlier_arms(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            matrix = [dict(task=f"task-{i}", arm=arm) for i in range(4) for arm in transfer.ARMS]
            for i, e in enumerate(matrix):
                session = root / "heldout" / e["task"] / e["arm"]
                home = session / "adage-home"
                home.mkdir(parents=True)
                transfer.pairs.write(session / "manifest.json", dict(arm=e["arm"], home=str(home)))
                if e["arm"] == "automatic" and i < 7:
                    (home / "approvals.json").write_text("{}")
            with patch.object(transfer, "verify", return_value=(dict(matrix=matrix), None)), \
                    patch.object(transfer.pairs, "verify"), patch.object(transfer.heldout, "live") as live:
                with self.assertRaisesRegex(ValueError, "human grants"):
                    transfer.run(root)
                live.assert_not_called()


if __name__ == "__main__":
    unittest.main()
