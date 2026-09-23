"""Hidden test for W-1041: Fix discount rounding."""
import unittest
from acme import billing

class TestDiscountRounding(unittest.TestCase):
    def test_discount_banker_rounding(self):
        # 10% of 99.99 should be 9.99 (rounds to 10.00)
        # So remaining should be 89.99 (8999 cents), not 90.00 (9000 cents)
        discount_amount = billing.apply_discount(9999, 10)  # 99.99 in cents
        self.assertEqual(discount_amount, 1000, f"Expected 1000 cents for discount, got {discount_amount}")
