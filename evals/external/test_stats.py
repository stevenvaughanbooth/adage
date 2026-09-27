import math
import unittest

import stats as s


def rec(task, arm, passed, cost=1.0, turns=10, repo="r1", status="ok"):
    return {"task_id": task, "repo_key": repo, "arm": arm, "status": status, "outcome": {"passed": passed},
            "cost": {"usd": cost}, "efficiency": {"turns": turns}}


class MeansTest(unittest.TestCase):
    def test_per_task_means_over_repeats_skip_errors(self):
        rs = [rec("t1", "adage", True), rec("t1", "adage", False), rec("t1", "adage", True, status="infra_error"),
              rec("t1", "holdout", False), rec("t1", "holdout", False, status="censored")]
        m = s.per_task_means(rs, "resolve")
        self.assertEqual(m[("r1", "t1")], {"adage": 0.5, "holdout": 0.0})

    def test_paired_and_log_ratio(self):
        m = {("r", "t1"): {"a": 2.0, "c": 1.0}, ("r", "t2"): {"a": 1.0}}
        self.assertEqual(s.paired(m, "a", "c"), {"r": [1.0]})
        self.assertAlmostEqual(s.paired(m, "a", "c", s.log_ratio)["r"][0], math.log(2))


class TestsTest(unittest.TestCase):
    def test_permutation_exact_small(self):
        # All four differences positive: two of 16 sign patterns reach |sum| = 4.
        self.assertAlmostEqual(s.permutation_test([1, 1, 1, 1]), 2 / 16)
        self.assertEqual(s.permutation_test([0, 0]), 1.0)
        self.assertIsNone(s.permutation_test([]))

    def test_permutation_monte_carlo_detects_shift_and_not_noise(self):
        shifted = [0.5 + 0.1 * ((i * 7) % 5 - 2) for i in range(40)]
        self.assertLess(s.permutation_test(shifted, n=2000, seed=1), 0.01)
        noise = [(-1) ** i * (1 + i % 3) for i in range(40)]
        self.assertGreater(s.permutation_test(noise, n=2000, seed=1), 0.2)

    def test_cluster_bootstrap_brackets_the_mean_and_is_seeded(self):
        clusters = {"a": [0.1, 0.2, 0.3], "b": [0.0, 0.1], "c": [0.4, 0.2, 0.3, 0.1]}
        ci = s.cluster_bootstrap(clusters, b=2000, seed=3)
        flat = [x for v in clusters.values() for x in v]
        self.assertLess(ci[0], sum(flat) / len(flat))
        self.assertGreater(ci[1], sum(flat) / len(flat))
        self.assertEqual(ci, s.cluster_bootstrap(clusters, b=2000, seed=3))
        const = s.cluster_bootstrap({"a": [1.0, 1.0], "b": [1.0]}, b=200)
        self.assertEqual(const, (1.0, 1.0))

    def test_holm(self):
        adj = s.holm({"a": 0.01, "b": 0.04, "c": 0.03, "d": None})
        self.assertAlmostEqual(adj["a"], 0.03)
        self.assertAlmostEqual(adj["c"], 0.06)
        self.assertAlmostEqual(adj["b"], 0.06, msg="monotone: never below an earlier adjusted p")
        self.assertIsNone(adj["d"])


class EffectTest(unittest.TestCase):
    def data(self):
        rs = []
        for repo in ("r1", "r2", "r3"):
            for i in range(8):
                t = f"{repo}-t{i}"
                for rep in range(2):
                    rs.append(rec(t, "holdout", i % 4 == 0, cost=2.0, turns=20, repo=repo))
                    rs.append(rec(t, "adage", i % 2 == 0, cost=1.5, turns=15, repo=repo))
                    rs.append(rec(t, "off", i % 4 == 0, cost=2.0, turns=20, repo=repo))
        return rs

    def test_effects(self):
        out = s.analyse(self.data(), comparisons=[("adage", "holdout")], b=500)
        by = {e["metric"]: e for e in out["effects"]}
        self.assertAlmostEqual(by["resolve"]["estimate"], 0.25)
        self.assertEqual(by["resolve"]["pairs"], 24)
        self.assertEqual(by["resolve"]["repos"], 3)
        self.assertAlmostEqual(by["cost"]["estimate"], 0.75)
        self.assertAlmostEqual(by["turns"]["estimate"], 0.75)
        lo, hi = by["resolve"]["ci95"]
        self.assertLessEqual(lo, 0.25)
        self.assertGreaterEqual(hi, 0.25)
        self.assertLess(by["resolve"]["p"], 0.05)
        self.assertGreaterEqual(by["resolve"]["p_holm"], by["resolve"]["p"])
        self.assertEqual(out["status_by_arm"]["adage"], {"ok": 48})

    def test_no_pairs(self):
        e = s.effect([rec("t", "adage", True)], "adage", "holdout", "resolve")
        self.assertEqual(e["pairs"], 0)


class PowerTest(unittest.TestCase):
    def test_binary_matches_connor(self):
        # Connor (1987): discordance 0.2, difference 0.1, alpha .05, power .8 -> about 155 pairs.
        n = s.pairs_for_binary(0.2, 0.1)
        self.assertTrue(150 <= n <= 160, n)
        self.assertGreater(s.pairs_for_binary(0.2, 0.05), n)
        self.assertGreater(s.pairs_for_binary(0.3, 0.1), n)
        with self.assertRaises(ValueError):
            s.pairs_for_binary(0.05, 0.1)

    def test_continuous(self):
        # (1.96 + 0.8416)^2 * (1/0.5)^2 = 31.4 -> 32
        self.assertEqual(s.pairs_for_continuous(1.0, 0.5), 32)
        self.assertAlmostEqual(s.design_effect(10, 0.1), 1.9)
        self.assertTrue(s.power_table())


if __name__ == "__main__":
    unittest.main()
