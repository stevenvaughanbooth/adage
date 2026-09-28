"""Tests for build_demo.py's redaction: paths as paths, encoded project paths, and account names.

  python3 -m unittest evals/datasets/tools/test_build_demo.py
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_demo  # noqa: E402


class Redaction(unittest.TestCase):
    def setUp(self):
        self.addCleanup(build_demo.REDACTIONS.clear)

    def test_home_is_root_rewrites_paths_not_words(self):
        build_demo.set_redactions({"/tmp/live/scratchpad"}, home="/root")
        r = build_demo.redact
        self.assertEqual(r("cat /root/.claude/settings.json"), "cat /home/user/.claude/settings.json")
        self.assertEqual(r("HOME=/root"), "HOME=/home/user")
        self.assertEqual(r('"/root"'), '"/home/user"')
        # Never inside a longer name, and never the bare word.
        for keep in ["the root commit", "rootDir: src", "/rootfs/etc", "/srv/root/x", "chroot /x", "Root cause"]:
            self.assertEqual(r(keep), keep)
        self.assertIsNone(build_demo.forbidden().search("the root commit; rootDir"))
        self.assertIsNotNone(build_demo.forbidden().search("ls /root/x"))

    def test_run_roots_and_their_encoded_project_paths(self):
        root = "/tmp/claude-0/-home-u/abc/scratchpad"
        build_demo.set_redactions({root}, home="/home/alice")
        r = build_demo.redact
        self.assertEqual(r(f"{root}/runs/train-W-1-r1/acme"), "/sandbox/runs/train-W-1-r1/acme")
        # `ls ~/.claude/projects` shows Claude Code's encoded form of the project path.
        enc = build_demo.encode_project(f"{root}/runs/train-W-1-r1/acme")
        self.assertEqual(enc, "-tmp-claude-0--home-u-abc-scratchpad-runs-train-W-1-r1-acme")
        self.assertEqual(r(f"projects/{enc}/s1.jsonl"), "projects/-sandbox-runs-train-W-1-r1-acme/s1.jsonl")
        self.assertEqual(r("-home-alice-work"), "-home-user-work")
        # The account name as a word; not inside another word.
        self.assertEqual(r("alice@host: whoami -> alice"), "user@host: whoami -> user")
        self.assertEqual(r("malice and alicen"), "malice and alicen")
        self.assertIsNone(build_demo.forbidden().search(r(f"{root}/x {enc} /home/alice/y alice")))

    def test_private_tmp_on_macos(self):
        build_demo.set_redactions({"/private/tmp/adage-demo"}, home="/Users/bob")
        r = build_demo.redact
        self.assertEqual(r("/private/tmp/adage-demo/runs/x"), "/sandbox/runs/x")
        self.assertEqual(r("/tmp/adage-demo/runs/x"), "/sandbox/runs/x")
        self.assertEqual(r("/Users/bob/.claude"), "/home/user/.claude")
        self.assertEqual(r("-Users-bob--claude-projects"), "-home-user--claude-projects")


if __name__ == "__main__":
    unittest.main()
