"""Hidden test for W-1042: EU tax region uses wrong rate."""
import unittest
from acme import billing

class TestEUTaxRate(unittest.TestCase):
    def test_eu_region_tax_rate(self):
        rate = billing.get_tax_rate("EU")
        self.assertEqual(rate, 0.20, f"EU tax rate should be 0.20, got {rate}")
