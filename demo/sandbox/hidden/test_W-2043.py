"""Hidden test for W-2043: Discount cannot exceed 100%."""
import unittest
from acme import billing

class TestDiscountValidation(unittest.TestCase):
    def test_discount_validation(self):
        # Valid discounts should work
        result = billing.apply_discount(10000, 50)
        self.assertGreater(result, 0)

        # Discounts >= 100% should raise error
        with self.assertRaises(ValueError):
            billing.apply_discount(10000, 100)

        with self.assertRaises(ValueError):
            billing.apply_discount(10000, 150)
