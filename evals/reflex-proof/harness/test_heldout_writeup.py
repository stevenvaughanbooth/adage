"""The held-out decision record: keyed to the registered criteria, filled only from the report."""
import unittest

import heldout_writeup as writeup
from test_heldout_analysis import positives, rows, run


class DecisionRecord(unittest.TestCase):
    def test_every_registered_section_is_filled_from_the_report(self):
        report = dict(run(rows()), best=dict(arm="A4", label="orientation card", report="dev.json"),
                      build_commit="abc1234", binary_sha256={"adage": "x"}, assignments=65, collected=65, valid=True)
        text = writeup.render(report)
        for heading in ("## Outcome", "## Primary (registered)", "## Quality (registered)", "## G criteria",
                        "## Sensitivity analyses", "## Censoring", "## Request cross-check", "## Controls",
                        "## Report only", "## What this does not establish"):
            self.assertIn(heading, text)
        self.assertIn(writeup.OUTCOMES[report["outcome"]], text)
        p = report["primary"]
        self.assertIn("| BEST / A0 | %s | %s | %s |" % (p["ratio"], p["lower"], p["upper"]), text)
        for g in ("G1", "G2", "G3", "G4", "G5", "G6", "G7", "G8", "G9"):
            self.assertIn("| %s |" % g, text)
        self.assertIn("No GPU savings", text)
        self.assertNotIn("INVALID", text)

    def test_an_incomplete_or_invalid_result_says_so_and_claims_nothing(self):
        out = rows()
        positives(out, "BEST")[0]["status"] = "uncollected"
        incomplete = writeup.render(dict(run(out), valid=True))
        self.assertIn("INCOMPLETE", incomplete)
        self.assertIn("- problem: incomplete blocks:", incomplete, "the analysis's own problems are shown")
        self.assertNotIn("## Primary", incomplete)
        invalid = writeup.render(dict(run(rows()), valid=False,
                                      lapses=[dict(order=7, problem="learned verification withheld (approval: x)")]))
        self.assertIn("INVALID", invalid)
        self.assertIn("- lapsed grant: order 7", invalid)

    def test_a_prominent_sensitivity_warning_is_shown(self):
        report = dict(run(rows()), warnings=["PROMINENT: the has-keys sensitivity fails"])
        self.assertIn("**warning:** PROMINENT", writeup.render(report))


if __name__ == "__main__":
    unittest.main()
