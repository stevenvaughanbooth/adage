"""Hidden test for W-2042: Customer region lookup case-sensitive."""
import unittest
from acme import customers

class TestRegionCaseInsensitive(unittest.TestCase):
    def test_region_lookup_case_insensitive(self):
        # Should handle both cases equally
        rate_lower_us = customers.get_tax_rate("us")
        rate_upper_us = customers.get_tax_rate("US")

        # Both should return the US rate of 0.08, not 0.0
        self.assertEqual(rate_lower_us, 0.08)
        self.assertEqual(rate_upper_us, 0.08)
