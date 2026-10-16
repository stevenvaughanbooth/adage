import tests
import unittest
from acme import billing

class TestBilling(unittest.TestCase):
    def test_tax_rates(self):
        self.assertEqual(billing.get_tax_rate("US"), 0.08)
        self.assertEqual(billing.get_tax_rate("CA"), 0.13)

    def test_discount_basic(self):
        result = billing.apply_discount(10000, 10)
        self.assertGreater(result, 0)
