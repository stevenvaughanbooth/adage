"""Hidden test for W-1043: Late fees applied too early."""
import unittest
from datetime import date
from acme import billing

class TestLateFeeiming(unittest.TestCase):
    def test_late_fee_after_due_date(self):
        due_date = date(2024, 1, 15)
        payment_on_due_date = date(2024, 1, 15)
        payment_after_due_date = date(2024, 1, 16)

        # Should NOT be late on due date
        self.assertFalse(billing.is_late(due_date, payment_on_due_date))
        # Should be late after due date
        self.assertTrue(billing.is_late(due_date, payment_after_due_date))
