"""Development canary-v9: matrix, prompts, arms, grants, the card knob, and the registered rule."""
import json
import os
from pathlib import Path
import shlex
import tempfile
import unittest
from unittest.mock import patch

import dev_v9 as v9

PROTOCOL = json.loads(v9.canary.GATE.read_text())
T = PROTOCOL[v9.THRESHOLDS]


MOD_ROUTES = ["github-com-marshmallow-code-marshmallow/learned/run-finish"]


def frozen(root, design=v9.DESIGN):
    bins = {"adage": str(root / "bin/adage"), "adage-exec": str(root / "bin/adage-exec")}
    mod = design == "dev-v9-mod"
    m = dict(protocol=v9.DESIGNS[design]["protocol"], design=design, source_sha256=v9.sources(), frozen_inputs={},
             matrix=v9.matrix(design), route=v9.canary.ROUTE, command=v9.canary.COMMAND,
             caches=list(v9.canary.CACHES), binaries=bins, predict_routes=MOD_ROUTES if mod else [],
             plugin=str(root / "plugin") if mod else None, model="m", thresholds=T,
             primary=v9.DESIGNS[design]["primary"])
    for row in m["matrix"]:
        row["prompt"] = "p"
    v9.pairs.write(root / "manifest.json", m)
    (root / "manifest.sha256").write_text(v9.pairs.sha(root / "manifest.json") + "\n")
    return m


class Design(unittest.TestCase):
    def test_controls_first_then_sixteen_rotating_blocks_of_three_arms(self):
        rows = v9.matrix()
        self.assertEqual(len(rows), 5 + 16 * 3)
        self.assertEqual([(r["arm"], r["condition"]) for r in rows[:5]], [("A4", c) for c in v9.CONTROLS])
        positives = rows[5:]
        for block in range(1, 17):
            self.assertEqual(sorted(r["arm"] for r in positives if r["block"] == block), ["A0", "A2f", "A4"])
        firsts = [next(r["arm"] for r in positives if r["block"] == b) for b in range(1, 17)]
        self.assertEqual({a: firsts.count(a) for a in v9.DESIGNS["dev-v9"]["arms"]}, {"A0": 6, "A2f": 5, "A4": 5},
                         "no arm always goes first")
        self.assertEqual({r["task"] for r in rows}, {"mm-curve--finite"})
        self.assertEqual([r["order"] for r in rows], list(range(len(rows))))
        self.assertEqual(len({v9.session_dir(Path("/r"), r) for r in rows}), len(rows))

    def test_every_prompt_carries_the_realism_line_before_its_condition(self):
        base = "Add a validator."
        for condition, (suffix, _, _) in v9.canary.CONDITIONS.items():
            p = v9.prompt(base, condition)
            self.assertEqual(p, base + "\n\nDo not create git commits." + suffix)
        self.assertEqual(v9.REALISM, v9.heldout_tasks.REALISM, "the held-out line, verbatim")

    def test_the_arms_differ_only_in_the_card(self):
        self.assertFalse(v9.ARMS["A0"]["hooks"])
        self.assertEqual((v9.ARMS["A2f"]["card_orientation"], v9.ARMS["A4"]["card_orientation"]), (False, True))
        self.assertEqual(v9.CONTROL_ARMS, ("A4",))
        self.assertIn(("PostToolBatch", None), v9.EVENTS)
        self.assertLessEqual(v9.STUDY_CEILING_USD, 8.50)
        # The per-session cap leaves room for every session at the ceiling's stop rule.
        self.assertGreater(v9.STUDY_CEILING_USD - v9.SESSION_CAP_USD, 53 * 0.13)

    def test_the_rule_is_registered_in_the_protocol(self):
        self.assertEqual(T["arm"], "A4")
        self.assertEqual((T["D1_requests_max_ratio_to_A0"], T["D1_blocks_fewer_min"]), (0.80, 10))
        self.assertEqual(T["D4_sessions_with_repeat_attempts_max"], 5)
        self.assertEqual(T["blocks"], v9.BLOCKS)


class Grants(unittest.TestCase):
    def test_script_grants_both_hook_arms_with_one_binary_and_nothing_in_a0(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            frozen(root)
            text = Path(v9.approval_script(root)["path"]).read_text()
            self.assertEqual(text.count("review delegate"), 16 * 2 + 5)
            self.assertNotIn("-A0-", text)
            self.assertIn('auto_approve = "off"', text)
            self.assertNotIn("approvals.json >", text)
            self.assertEqual(text.count("--timeout-ms 20000 "), 16 * 2 + 5, "within E2's limit: nothing is clamped")

    def test_grant_check_requires_none_in_the_no_hook_arm(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            m = frozen(root)
            for row in m["matrix"]:
                home = v9.session_dir(root, row) / "adage-home"
                home.mkdir(parents=True)
                if v9.ARMS[row["arm"]]["hooks"]:
                    (home / "policy.toml").write_text(v9.POLICY)
                    v9.pairs.write(home / "approvals.json", dict(approvals=[dict(
                        route=v9.canary.ROUTE, by="launch-evaluation-human", auto=False,
                        key=dict(level="e2", effect="verify", argv_templates=[shlex.split(v9.canary.COMMAND)]),
                        terms=dict(cache=list(v9.canary.CACHES), timeout_ms=v9.TIMEOUT_MS))]))
            self.assertTrue(v9.verify_grants(root)["passed"])
            a0 = next(r for r in m["matrix"] if r["arm"] == "A0")
            v9.pairs.write(v9.session_dir(root, a0) / "adage-home/approvals.json", dict(approvals=[]))
            self.assertFalse(v9.verify_grants(root)["passed"])


class Hook(unittest.TestCase):
    def test_a2f_gets_the_earlier_card_and_a4_the_oriented_one_and_no_knob_carries_over(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "manifest.json"
            path.write_text(json.dumps(dict(arm="agent")))
            with self.assertRaises(ValueError):
                v9.hook(path, {})
            with patch.object(v9.pairs, "hook", return_value="out"), patch.dict(os.environ, {"ADAGE_CARD_CHECKS": "0"}):
                path.write_text(json.dumps(dict(arm="automatic", card_orientation=False)))
                self.assertEqual(v9.hook(path, {}), "out")
                self.assertEqual(os.environ.get("ADAGE_CARD_ORIENTATION"), "0")
                self.assertNotIn("ADAGE_CARD_CHECKS", os.environ, "the checks line stays as shipped in both arms")
                path.write_text(json.dumps(dict(arm="automatic", card_orientation=True)))
                v9.hook(path, {})
                self.assertNotIn("ADAGE_CARD_ORIENTATION", os.environ)


def row(arm, block, requests, condition="positive", **extra):
    """A collected row whose integrity facts hold, in its arm's card form."""
    hooked = v9.ARMS[arm]["hooks"]
    form = v9.ARMS[arm]["card_orientation"]
    receipt = dict(kind="receipt", phase="completed", outcome="failed" if condition == "failed" else "applied", digest="a")
    base = dict(dev_arm=arm, block=block, condition=condition, requests=requests, cost_usd=0.1, wall_seconds=60,
                tokens=dict(input=1, cache_write=1, cache_read=1, output=1), pytest_requests=1,
                quality=dict(passed=True), replay={"receipts": 1}, stop_gate_outputs=[], order=0,
                observed_dispatches=[dict(source="sdk")] * requests, censored=[], contract_emitted=hooked,
                exec_decisions=[dict(outcome="applied", withheld=None)] if hooked else [],
                all_journal_receipts=[dict(id="op")] if hooked else [],
                card_oriented=form is True, card_exemplar=form is False,
                timeline=dict(events=[receipt] if hooked else [],
                              final_digest="b" if condition in ("later-edit", "config-change") else "a"))
    base.update(extra)
    return base


def study(a4, a2f=None, a0=18):
    out = [row("A4", None, 15, condition=c) for c in v9.CONTROLS]
    for b in range(1, 17):
        out += [row("A0", b, a0), row("A2f", b, (a2f or a4)[b - 1]), row("A4", b, a4[b - 1])]
    for order, r in enumerate(out):
        r["order"] = order
    return out


class Rule(unittest.TestCase):
    def test_a4_passes_on_a_real_paired_reduction_and_is_best(self):
        out = v9.summarize(study([14] * 16), T)
        self.assertEqual(out["problems"], [])
        self.assertTrue(out["valid"] and out["verdict"]["A4"]["passed"], out["verdict"])
        self.assertEqual(out["verdict"]["A4"]["requests_ratio_to_A0"], round(14 / 18, 4))
        self.assertEqual(out["best"], "A4")
        self.assertFalse(out["verdict"]["A2f"]["passed"], "A2f has no controls: reported, never BEST")
        self.assertFalse(out["verdict"]["A2f"]["eligible"])

    def test_ratio_and_block_count_each_bind(self):
        self.assertFalse(v9.summarize(study([15] * 16), T)["verdict"]["A4"]["D1_requests"], "15/18 = 0.83 > 0.80")
        lumpy = [0] * 9 + [18] * 7  # mean 7.9 but fewer than A0 in only 9 of 16 blocks
        v = v9.summarize(study(lumpy), T)["verdict"]["A4"]
        self.assertTrue(v["requests_ratio_to_A0"] <= 0.80 and not v["D1_requests"])
        self.assertIsNone(v9.summarize(study(lumpy), T)["best"])

    def test_safety_quality_and_repeats_each_fail_a4(self):
        rows = study([14] * 16)
        next(r for r in rows if r["condition"] == "later-edit")["replay"] = dict(necessary_work_skipped=True)
        self.assertFalse(v9.summarize(rows, T)["verdict"]["A4"]["D3_safety"])
        rows = study([14] * 16)
        for r in rows:
            if r["dev_arm"] == "A4" and r["condition"] == "positive" and r["block"] <= 1:
                r["quality"] = dict(passed=False)
        self.assertFalse(v9.summarize(rows, T)["verdict"]["A4"]["D2_quality"])
        rows = study([14] * 16)
        for r in [r for r in rows if r["dev_arm"] == "A4" and r["condition"] == "positive"][:6]:
            r["replay"] = dict(repeat_attempts=1)
        self.assertFalse(v9.summarize(rows, T)["verdict"]["A4"]["D4_repeat_sessions"])

    def test_gate_runs_are_reported_on_their_own_and_never_unrealize_a_control(self):
        rows = study([14] * 16)
        failed = rows[0]
        self.assertEqual(failed["condition"], "failed")
        # dev-v9's order 0: the context run failed, the agent fixed the code, the gate passed at finish.
        failed["timeline"] = dict(events=[dict(kind="receipt", phase="completed", outcome="failed", digest="a",
                                               origin="context", at="PostToolBatch"),
                                          dict(kind="receipt", phase="completed", outcome="applied", digest="b",
                                               origin="stop-gate", at="Stop")], final_digest="b")
        failed["gate_runs"] = [dict(outcome="applied", digest="b", covers_final=True)]
        out = v9.summarize(rows, T)
        self.assertTrue(out["valid"], out["problems"])
        self.assertEqual(out["controls"][0]["gate_runs"], failed["gate_runs"])
        self.assertTrue(out["controls"][0]["realized"])
        positives = study([14] * 16)
        positives[7]["gate_runs"] = [dict(outcome="applied", digest="a", covers_final=True)]
        arm = v9.summarize(positives, T)["arms"]["A4"]["gate_runs"]
        self.assertEqual(arm, dict(sessions=1, runs=1, passed=1, covered_final=1))

    def test_lapses_unrealized_controls_and_the_wrong_card_invalidate_the_study(self):
        for change in (lambda rs: rs[7].update(contract_emitted=False),
                       lambda rs: rs[6].update(exec_decisions=[dict(withheld="approval", reason="exec-changed")]),
                       lambda rs: rs[7].update(all_journal_receipts=[]),
                       lambda rs: rs[1]["timeline"].update(final_digest="a"),  # later-edit not stale: unrealized
                       lambda rs: rs[6].update(card_oriented=True, card_exemplar=False),  # A2f got the new card
                       lambda rs: rs[7].update(card_oriented=False)):  # A4's card had no orientation
            rows = study([14] * 16)
            change(rows)
            out = v9.summarize(rows, T)
            self.assertFalse(out["valid"], out["problems"])
            self.assertIsNone(out["best"])


def mod_study(mod, a4=None, a0=18):
    """A dev-v9-mod study: MOD's five controls, then A0, A4 and MOD in each of the 16 blocks."""
    out = [row("MOD", None, 15, condition=c) for c in v9.CONTROLS]
    for b in range(1, 17):
        out += [row("A0", b, a0), row("A4", b, (a4 or [16] * 16)[b - 1]), row("MOD", b, mod[b - 1])]
    for order, r in enumerate(out):
        r["order"] = order
    return out


class Mod(unittest.TestCase):
    """E2's MOD arm (plan E2, T-E2-01 and T-E2-02): its design, the person's predict grant and the
    registered rule."""

    def test_the_mod_design_runs_mods_five_controls_then_sixteen_blocks_of_a0_a4_and_mod(self):
        rows = v9.matrix("dev-v9-mod")
        self.assertEqual(len(rows), 5 + 16 * 3, "the dev-v9 budget's 53 sessions")
        self.assertEqual([(r["arm"], r["condition"]) for r in rows[:5]], [("MOD", c) for c in v9.CONTROLS])
        for block in range(1, 17):
            self.assertEqual(sorted(r["arm"] for r in rows[5:] if r["block"] == block), ["A0", "A4", "MOD"])
        self.assertEqual(v9.DESIGNS["dev-v9-mod"]["control_arms"], ("MOD",))
        self.assertEqual(v9.DESIGNS["dev-v9-mod"]["protocol"], "dev-arms-3")
        mod = v9.ARMS["MOD"]
        self.assertTrue(mod["hooks"] and mod["mod"] and mod["card_orientation"], "MOD is A4's hooks plus the mod")
        self.assertEqual(v9.matrix(), v9.matrix("dev-v9"), "the registered design is unchanged")
        self.assertEqual(v9.PROTOCOL, "dev-arms-2")

    def test_the_dry_run_shows_the_persons_predict_grant_before_any_positive_block(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            frozen(root, "dev-v9-mod")
            out = v9.dry_run(root)
            self.assertEqual((out["design"], out["protocol"], out["paid_calls"]), ("dev-v9-mod", "dev-arms-3", 0))
            self.assertEqual(out["steps"][0]["step"], "person")
            self.assertTrue(out["person_before_positives"])
            grants = out["steps"][0]["predict_grants"]
            self.assertEqual(len(grants), 5 + 16, "every MOD home: its controls and its blocks")
            self.assertTrue(all(g["routes"] == MOD_ROUTES for g in grants))
            dispatches = [s for s in out["steps"] if s["step"] == "dispatch"]
            self.assertEqual(len(dispatches), 53)
            self.assertEqual([s["condition"] for s in dispatches[:5]], list(v9.CONTROLS))
            for s in dispatches:
                if s["arm"] == "MOD":
                    self.assertEqual(s["plugin_dir"], str(root / "plugin"))
                    self.assertEqual(s["grants"], ["e2 " + v9.canary.ROUTE, "predict " + MOD_ROUTES[0]])
                else:
                    self.assertIsNone(s["plugin_dir"])
                    self.assertNotIn("predict " + MOD_ROUTES[0], s["grants"])
            self.assertEqual(out["thresholds"], T, "the frozen registered rule")
            self.assertEqual(out["primary"], "MOD")
            self.assertEqual(sorted(p.name for p in root.iterdir()), ["manifest.json", "manifest.sha256"],
                             "the dry run writes nothing")

    def test_the_mod_homes_get_a_person_issued_predict_grant_in_the_reviewed_script(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            m = frozen(root, "dev-v9-mod")
            text = Path(v9.approval_script(root)["path"]).read_text()
            self.assertEqual(text.count("review delegate"), 16 * 2 + 5 + (16 + 5), "E2 for A4 and MOD, predict for MOD")
            self.assertEqual(text.count("--predict "), 16 + 5)
            self.assertIn("review delegate github-com-marshmallow-code-marshmallow/learned run-finish --predict --repo ", text)
            self.assertNotIn("-A0-", text)
            for line in (x for x in text.splitlines() if "--predict" in x):
                self.assertIn("-MOD-", line)
                self.assertIn("--by launch-evaluation-human", line)
            for row in m["matrix"]:
                home = v9.session_dir(root, row) / "adage-home"
                home.mkdir(parents=True)
                if not v9.ARMS[row["arm"]]["hooks"]:
                    continue
                (home / "policy.toml").write_text(v9.POLICY)
                grants = [dict(route=v9.canary.ROUTE, by="launch-evaluation-human", auto=False,
                               key=dict(level="e2", effect="verify", argv_templates=[shlex.split(v9.canary.COMMAND)]),
                               terms=dict(cache=list(v9.canary.CACHES), timeout_ms=v9.TIMEOUT_MS))]
                if v9.ARMS[row["arm"]].get("mod"):
                    grants.append(dict(route=MOD_ROUTES[0], by="launch-evaluation-human", auto=False,
                                       key=dict(level="e2", mode="predict")))
                v9.pairs.write(home / "approvals.json", dict(approvals=grants))
            self.assertTrue(v9.verify_grants(root)["passed"], v9.verify_grants(root))
            # A predict grant in an A4 home, or none in a MOD home, is not the reviewed script.
            a4 = next(r for r in m["matrix"] if r["arm"] == "A4")
            store = v9.pairs.read(v9.session_dir(root, a4) / "adage-home/approvals.json")
            store["approvals"].append(dict(route=MOD_ROUTES[0], by="launch-evaluation-human", auto=False,
                                           key=dict(level="e2", mode="predict")))
            v9.pairs.write(v9.session_dir(root, a4) / "adage-home/approvals.json", store)
            self.assertFalse(v9.verify_grants(root)["passed"])

    def test_the_argv_loads_the_frozen_mod_for_mod_sessions_only(self):
        m = dict(model="m", plugin="/study/plugin")
        mod = v9.session_argv(m, dict(arm="MOD", prompt="p"), Path("/s"))
        self.assertEqual(mod[-3:], ["--plugin-dir", "/study/plugin", "p"])
        self.assertNotIn("--plugin-dir", v9.session_argv(m, dict(arm="A4", prompt="p"), Path("/s")))

    def test_the_staged_mod_carries_no_command_hook(self):
        with tempfile.TemporaryDirectory() as tmp:
            src, dst = Path(tmp) / "plugin", Path(tmp) / "staged"
            (src / "hooks").mkdir(parents=True)
            (src / "hooks/hooks.json").write_text(json.dumps(dict(hooks=dict(Stop=[]), modules=["./register.ts"])))
            (src / "hooks/register.ts").write_text("export {}\n")
            (src / "hooks/register.test.ts").write_text("test\n")
            digests = v9.stage_mod_plugin(src, dst)
            self.assertEqual(json.loads((dst / "hooks/hooks.json").read_text()), dict(modules=["./register.ts"]))
            self.assertEqual(sorted(digests), ["hooks/hooks.json", "hooks/register.ts"])
        self.assertTrue((v9.PLUGIN / "hooks/hooks.json").is_file(), "the repository's plugin is the default")

    def test_the_mod_revision_must_be_registered_before_a_mod_study_freezes(self):
        protocol = json.loads(v9.canary.GATE.read_text())
        self.assertFalse(v9.registered_mod_revision(protocol), "no MOD revision is registered yet: no MOD run")
        self.assertTrue(v9.registered_mod_revision(dict(revisions=["r13 ... development canary dev-v9-mod ..."])))
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(ValueError):
                v9.freeze(Path(tmp) / "study", Path(tmp), Path(tmp), Path(tmp), Path(tmp), [], "dev-v9-mod", ())
            with self.assertRaises(ValueError):
                v9.freeze(Path(tmp) / "study", Path(tmp), Path(tmp), Path(tmp), Path(tmp), [], "dev-v9-mod",
                          (v9.canary.ROUTE,))

    def test_mod_is_judged_by_the_registered_rule_and_is_the_only_candidate(self):
        out = v9.summarize(mod_study([14] * 16), T, "dev-v9-mod")
        self.assertTrue(out["valid"], out["problems"])
        self.assertTrue(out["verdict"]["MOD"]["passed"], out["verdict"])
        self.assertEqual(out["best"], "MOD")
        self.assertEqual(out["candidates"], ["MOD"])
        self.assertFalse(out["verdict"]["A4"]["eligible"], "A4 has no controls here: reported, never BEST")
        self.assertNotIn("A2f", out["verdict"])
        self.assertEqual(out["rule"], dict(D1_requests_max_ratio_to_A0=0.8, D1_blocks_fewer_min=10,
                                           D4_sessions_with_repeat_attempts_max=5, registered="dev_thresholds_v9"))
        self.assertFalse(v9.summarize(mod_study([15] * 16), T, "dev-v9-mod")["verdict"]["MOD"]["D1_requests"])
        # The thresholds are read, never written: a stricter registered ratio fails the same study.
        stricter = dict(T, D1_requests_max_ratio_to_A0=0.7)
        self.assertFalse(v9.summarize(mod_study([14] * 16), stricter, "dev-v9-mod")["verdict"]["MOD"]["passed"])
        rows = mod_study([14] * 16)
        rows[2]["timeline"]["final_digest"] = "a"  # MOD's config-change control was not realized
        self.assertFalse(v9.summarize(rows, T, "dev-v9-mod")["valid"])


if __name__ == "__main__":
    unittest.main()
