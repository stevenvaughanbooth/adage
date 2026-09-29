"""Pure helpers of the Harbor agent; Harbor itself is not needed."""

import hashlib
import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path

import adage_harbor as h


class SettingsTest(unittest.TestCase):
    def test_hooks_match_adage_hooks_config(self):
        # crates/adage-cli/src/harness/claude.rs `settings`: exec form, six events, timeout 5.
        s = h.hooks_settings("/bin/adage")
        self.assertEqual(sorted(s["hooks"]), sorted(["SessionStart", "UserPromptSubmit", "PostToolBatch",
                                                     "PostToolUseFailure", "Stop", "SessionEnd"]))
        self.assertEqual(s["hooks"]["Stop"], [{"hooks": [{"type": "command", "command": "/bin/adage",
                                                          "args": ["hook", "claude", "Stop"], "timeout": 5}]}])

    def test_merge_keeps_user_settings_and_replaces_our_entries(self):
        other = {"hooks": [{"type": "command", "command": "echo hi"}]}
        stale = {"hooks": [{"type": "command", "command": "/old/adage", "args": ["hook", "claude", "Stop"]}]}
        base = {"model": "x", "hooks": {"Stop": [other, stale], "PreToolUse": [other]}}
        out = h.merge_hooks(base, h.hooks_settings("/new/adage"))
        self.assertEqual(out["model"], "x")
        self.assertEqual(out["hooks"]["PreToolUse"], [other])
        self.assertEqual(len(out["hooks"]["Stop"]), 2)
        self.assertEqual(out["hooks"]["Stop"][0], other)
        self.assertEqual(out["hooks"]["Stop"][1]["hooks"][0]["command"], "/new/adage")
        self.assertEqual(base["hooks"]["Stop"], [other, stale], "input not mutated")
        # Idempotent.
        self.assertEqual(h.merge_hooks(out, h.hooks_settings("/new/adage")), out)

    def test_arms(self):
        base = {"hooks": {"Stop": [{"hooks": [{"command": "a", "args": ["hook", "claude", "Stop"]}]}]}, "k": 1}
        self.assertEqual(h.settings_for_arm("off", base), {"k": 1})
        self.assertEqual(h.settings_for_arm("agents-md", None), {})
        self.assertIn("SessionStart", h.settings_for_arm("control", None)["hooks"])
        with self.assertRaises(ValueError):
            h.settings_for_arm("treated", None)

    def test_arm_env(self):
        self.assertNotIn("ADAGE_HOME", h.arm_env("off", "/logs/agent/adage-home"))
        e = h.arm_env("control", "/logs/agent/adage-home")
        self.assertEqual((e["ADAGE_ARM"], e["ADAGE_AUTO_LEARN"], e["ADAGE_HOME"]),
                         ("control", "0", "/logs/agent/adage-home"))
        self.assertEqual(h.arm_env("adage", "/x")["ADAGE_ARM"], "treated")
        for arm in h.ARMS:
            self.assertEqual(h.arm_env(arm, "/x")["CLAUDE_CODE_DISABLE_AUTO_MEMORY"], "1")


class RepoKeyTest(unittest.TestCase):
    def test_normalise_origin_matches_rust(self):
        # Cases from adage-core repo.rs `origin_normalisation`, plus repeated suffixes.
        self.assertEqual(h.normalise_origin("git@github.com:Acme/Billing.git"), "github.com/acme/billing")
        self.assertEqual(h.normalise_origin("https://github.com/acme/billing/"), "github.com/acme/billing")
        self.assertEqual(h.normalise_origin("ssh://git@github.com/acme/billing.git"), "git@github.com/acme/billing")
        self.assertEqual(h.normalise_origin("https://x/y.git.git"), "x/y")

    def probe(self, **kw):
        lines = [f"{k}={v}" for k, v in kw.items()]
        return h.parse_probe("\n".join(lines))

    def test_origin_wins(self):
        import base64
        cfg = '[core]\n\tbare = false\n[remote "upstream"]\n\turl = https://x/other\n[remote "origin"]\n\turl = https://github.com/Ansible/ansible.git\n'
        p = self.probe(cwd="/app", root="/app", git_is_dir="1", config_b64=base64.b64encode(cfg.encode()).decode(),
                       root_commit="a" * 40, first_parent_count="10")
        self.assertEqual(h.candidate_keys(p), ["github.com/ansible/ansible"])

    def test_no_origin_root_commit_and_path_fallback(self):
        p = self.probe(cwd="/app/sub", root="/app", git_is_dir="1", root_commit="ab" * 20, first_parent_count="5")
        path_key = "app@path-" + hashlib.sha256(b"/app").hexdigest()[:12]
        self.assertEqual(h.candidate_keys(p), ["app@" + "ab" * 6, path_key])
        for extra in ({"shallow": "1"}, {"alternates": "1"}, {"first_parent_count": str(h.MAX_WALK + 1)}):
            q = dict(p, **extra)
            self.assertEqual(h.candidate_keys(q), [path_key], extra)
        self.assertEqual(h.candidate_keys(self.probe(cwd="/tmp", root="")), [])

    def test_rekey_index(self):
        idx = {"generation": "g1", "routes": [{"repo": "k1", "route": {"id": "a"}}, {"repo": "k2", "route": {"id": "b"}}],
               "triggers": {"k1": {"words": ["make"]}, "k2": {}}}
        out = h.rekey_index(idx, "k1", ["x", "y", "x"])
        self.assertEqual(out["generation"], "g1")
        self.assertEqual([(r["repo"], r["route"]["id"]) for r in out["routes"]], [("x", "a"), ("y", "a")])
        self.assertEqual(out["triggers"], {"x": {"words": ["make"]}, "y": {"words": ["make"]}})
        with self.assertRaises(ValueError):
            h.rekey_index(idx, None, ["x"])
        with self.assertRaises(ValueError):
            h.rekey_index(idx, "nope", ["x"])
        one = {"generation": "g", "routes": [{"repo": "only", "route": {"id": "a"}}]}
        self.assertEqual(h.rekey_index(one, None, ["z"])["routes"][0]["repo"], "z")


class HomeTest(unittest.TestCase):
    def tarball(self, d, members):
        p = Path(d) / "home.tgz"
        with tarfile.open(p, "w:gz") as tf:
            for name, data in members.items():
                ti = tarfile.TarInfo(name)
                ti.size = len(data)
                tf.addfile(ti, io.BytesIO(data))
        return p

    def test_prepare_home(self):
        idx = {"generation": "g", "routes": [{"repo": "adage-eval:s/o__a", "route": {"id": "r", "summary": "Run make gen"}}]}
        with tempfile.TemporaryDirectory() as d:
            tgz = self.tarball(d, {"./index.json": json.dumps(idx).encode(), "./tracks/t/track.toml": b"x",
                                   "./sessions/old.spool": b"s", "./learn.log": b"l"})
            files = h.read_home_archive(tgz)
            self.assertEqual(sorted(files), ["index.json", "learn.log", "sessions/old.spool", "tracks/t/track.toml"])
            dest = Path(d) / "out"
            info = h.prepare_home(files, "adage-eval:s/o__a", ["k1", "k2"], dest)
            self.assertEqual(info["keys"], ["k1", "k2"])
            self.assertEqual(info["routes"], 1)
            self.assertFalse((dest / "sessions").exists(), "no stale session state is shipped")
            self.assertFalse((dest / "learn.log").exists())
            self.assertTrue((dest / "tracks/t/track.toml").exists())
            self.assertEqual({r["repo"] for r in json.loads((dest / "index.json").read_text())["routes"]}, {"k1", "k2"})
            self.assertEqual(h.agents_md_text(idx), "Repository conventions observed in earlier sessions:\n- Run make gen\n")

    def test_unsafe_archives_refused(self):
        with tempfile.TemporaryDirectory() as d:
            for bad in ("../x", "/etc/x"):
                with self.assertRaises(ValueError):
                    h.read_home_archive(self.tarball(d, {bad: b"x"}))

    def test_capture_never_writes_into_the_worktree(self):
        c = h.capture_command("/app", "/logs/agent/adage")
        self.assertIn("--no-optional-locks status --porcelain=v1 --untracked-files=all > /logs/agent/adage/status.txt", c)
        self.assertNotIn("> /app", c)
        self.assertNotIn("git add", c)


if __name__ == "__main__":
    unittest.main()
